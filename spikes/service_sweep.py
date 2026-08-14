#!/usr/bin/env python3
"""Sweeps every operation the served services' IDL declares, over the wire.

HARNESS FIXTURE. See `docs/SERVICES-COVERAGE.md`, which is this script's
output written up.

# The question

`docs/COMPONENTS.md` marks five served services ✅ and each servant implements
a *subset*. A reader cannot tell a considered refusal from an omission. This
script answers, per declared operation, which of three it is:

  served    a real reply came back
  refused   a system exception came back AND the servant's module docs give a
            reason for it (the reason is quoted in the report, not here)
  absent    BAD_OPERATION with nothing written down anywhere

Note what that means: the *wire* cannot distinguish the last two. A refused
pull operation and a forgotten one both answer BAD_OPERATION. So this script
measures the wire and the report supplies the documented reason; an operation
that answers BAD_OPERATION and has no documented reason is the finding.

# Why raw GIOP and not our own client

Our client and our server were written together. A sweep built on
`orbweaver_giop::Connection` inherits every assumption both halves share, and
the one failure mode that matters here — an operation that dispatches but is
broken — is exactly the kind a shared assumption hides. This speaks GIOP 1.2
over a socket with nothing but the standard library, so what it reports is
what a foreign ORB would see.

# How an operation is probed

Each probe is a real GIOP request carrying 64 zero bytes as its body. Zero
bytes are not a marshalling trick, they are the shortest legal thing that is
not *empty*: an empty body makes `Request::body()` fail and every servant maps
that to MARSHAL, which would make every operation look present. Sixty-four
zeros decode as empty strings, empty sequences and nil references, so an
operation that exists either answers, raises, or returns MARSHAL — and one
that does not exist answers BAD_OPERATION, which is the distinction being
measured.

Real calls with real arguments come first, in `walk()`, because the probes are
degenerate by design and some of them have side effects.

Usage: service_sweep.py <ior-dir>
"""

import os
import re
import socket
import struct
import subprocess
import sys

# ── omniORB's own IDL, used as the source for the OMG operation lists ────────
#
# Probed rather than assumed: `resolve_idl_root()` reports what it found, and
# the report says which files were read. A hand-typed operation list would be
# a claim about a specification; this is a reading of one.
IDL_ROOT_CANDIDATES = [
    "/opt/homebrew/share/idl/omniORB",
    "/usr/local/share/idl/omniORB",
    "/usr/share/idl/omniORB",
]

TIMEOUT = 5.0
PROBE_BODY = b"\0" * 64

CORBA_OBJECT_ID = "IDL:omg.org/CORBA/Object:1.0"


# ─────────────────────────────────────────────────────────────────────────────
# CDR
# ─────────────────────────────────────────────────────────────────────────────


def _align(pos, n):
    return (pos + n - 1) // n * n


class W:
    """A CDR encoder. `start` is where these bytes will land in the enclosing
    buffer, because alignment is measured from the enclosing origin — a GIOP
    body aligns from the first byte of the 12-byte header, not from its own
    first byte."""

    def __init__(self, little=True, start=0):
        self.b = bytearray()
        self.little = little
        self.start = start

    @property
    def _fmt(self):
        return "<" if self.little else ">"

    def align(self, n):
        here = self.start + len(self.b)
        self.b += b"\0" * (_align(here, n) - here)

    def u8(self, v):
        self.b.append(v)

    def u16(self, v):
        self.align(2)
        self.b += struct.pack(self._fmt + "H", v)

    def u32(self, v):
        self.align(4)
        self.b += struct.pack(self._fmt + "I", v)

    def u64(self, v):
        self.align(8)
        self.b += struct.pack(self._fmt + "Q", v)

    def f32(self, v):
        self.align(4)
        self.b += struct.pack(self._fmt + "f", v)

    def raw(self, data):
        self.b += data

    def octets(self, data):
        self.u32(len(data))
        self.b += data

    def string(self, s):
        encoded = s.encode("utf-8") + b"\0"
        self.u32(len(encoded))
        self.b += encoded


class R:
    """A CDR decoder over `buf`, whose first byte is CDR position zero."""

    def __init__(self, buf, little=True, pos=0):
        self.buf = buf
        self.little = little
        self.pos = pos

    @property
    def _fmt(self):
        return "<" if self.little else ">"

    def align(self, n):
        self.pos = _align(self.pos, n)

    def remaining(self):
        return len(self.buf) - self.pos

    def u8(self):
        v = self.buf[self.pos]
        self.pos += 1
        return v

    def u16(self):
        self.align(2)
        (v,) = struct.unpack_from(self._fmt + "H", self.buf, self.pos)
        self.pos += 2
        return v

    def u32(self):
        self.align(4)
        (v,) = struct.unpack_from(self._fmt + "I", self.buf, self.pos)
        self.pos += 4
        return v

    def u64(self):
        self.align(8)
        (v,) = struct.unpack_from(self._fmt + "Q", self.buf, self.pos)
        self.pos += 8
        return v

    def f32(self):
        self.align(4)
        (v,) = struct.unpack_from(self._fmt + "f", self.buf, self.pos)
        self.pos += 4
        return v

    def octets(self):
        n = self.u32()
        v = bytes(self.buf[self.pos : self.pos + n])
        self.pos += n
        return v

    def string(self):
        return self.octets().rstrip(b"\0").decode("utf-8", "replace")


# ─────────────────────────────────────────────────────────────────────────────
# IOR
# ─────────────────────────────────────────────────────────────────────────────


class Ref:
    """An object reference: what to dial, what key to address, what it says it
    is."""

    def __init__(self, type_id, host, port, key):
        self.type_id = type_id
        self.host = host
        self.port = port
        self.key = key

    def with_key(self, key, type_id=None):
        """The same endpoint, a different object key.

        Legitimate here and only here: the MoE tenant service derives every one
        of its keys from a documented, reversible template
        (`<base>/t/<tenant>/policy/<domain>`), and the contract declares no
        operation that hands out a `PolicyDomain`. Deriving the key is what a
        deployment does; guessing one the servant does not serve just gets
        OBJECT_NOT_EXIST, which the sweep would report."""
        return Ref(type_id or self.type_id, self.host, self.port, key)

    def __repr__(self):
        return f"<{self.type_id} @{self.host}:{self.port} key={self.key!r}>"


def parse_ior(text):
    text = text.strip()
    if not text.lower().startswith("ior:"):
        raise ValueError(f"not a stringified IOR: {text[:20]!r}")
    data = bytes.fromhex(text[4:])
    r = R(data)
    r.little = r.u8() != 0
    type_id = r.string()
    profiles = r.u32()
    for _ in range(profiles):
        tag = r.u32()
        body = r.octets()
        if tag != 0:  # TAG_INTERNET_IOP
            continue
        p = R(body)
        p.little = p.u8() != 0
        p.u8()  # iiop major
        p.u8()  # iiop minor
        host = p.string()
        port = p.u16()
        key = p.octets()
        return Ref(type_id, host, port, key)
    raise ValueError("the IOR carries no IIOP profile")


def read_objref(r):
    """Decodes an object reference from a reply body (not an encapsulation)."""
    type_id = r.string()
    profiles = r.u32()
    ref = None
    for _ in range(profiles):
        tag = r.u32()
        body = r.octets()
        if tag != 0 or ref is not None:
            continue
        p = R(body)
        p.little = p.u8() != 0
        p.u8()
        p.u8()
        host = p.string()
        port = p.u16()
        ref = Ref(type_id, host, port, p.octets())
    return ref  # None is a nil reference, which is a legal answer


def write_objref(w, ref):
    """Encodes an object reference into a request body."""
    if ref is None:
        w.string("")
        w.u32(0)
        return
    w.string(ref.type_id)
    w.u32(1)
    w.u32(0)  # TAG_INTERNET_IOP
    p = W(w.little, start=0)
    p.u8(1 if w.little else 0)
    p.u8(1)
    p.u8(2)
    p.string(ref.host)
    p.u16(ref.port)
    p.octets(ref.key)
    p.u32(0)  # no components
    w.octets(bytes(p.b))


# ─────────────────────────────────────────────────────────────────────────────
# GIOP 1.2
# ─────────────────────────────────────────────────────────────────────────────

NO_EXCEPTION, USER_EXCEPTION, SYSTEM_EXCEPTION, LOCATION_FORWARD = 0, 1, 2, 3


class Answer:
    def __init__(self, status, body=None, exc_id=None, minor=0, completed=0, note=""):
        self.status = status
        self.body = body
        self.exc_id = exc_id
        self.minor = minor
        self.completed = completed
        self.note = note

    def short(self):
        if self.note:
            return self.note
        if self.status == NO_EXCEPTION:
            return "reply"
        if self.status == USER_EXCEPTION:
            return f"user {short_id(self.exc_id)}"
        if self.status == SYSTEM_EXCEPTION:
            return short_id(self.exc_id)
        if self.status == LOCATION_FORWARD:
            return "LOCATION_FORWARD"
        return f"status {self.status}"


def short_id(repo_id):
    if not repo_id:
        return "?"
    stem = repo_id.rsplit(":1.0", 1)[0]
    return stem.rsplit("/", 1)[-1]


class Conn:
    """One connection, many requests — GIOP multiplexes on request id."""

    def __init__(self, ref):
        self.ref = ref
        self.sock = socket.create_connection((ref.host, ref.port), TIMEOUT)
        self.sock.settimeout(TIMEOUT)
        self.next_id = 1

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass

    def _recv(self, n):
        out = b""
        while len(out) < n:
            chunk = self.sock.recv(n - len(out))
            if not chunk:
                raise EOFError("the peer closed the connection")
            out += chunk
        return out

    def call(self, key, operation, write_args=None, raw_body=None):
        rid = self.next_id
        self.next_id += 1

        w = W(little=True)
        w.raw(b"GIOP")
        w.u8(1)
        w.u8(2)
        w.u8(1)  # little-endian flag
        w.u8(0)  # MsgType::Request
        size_at = len(w.b)
        w.raw(b"\0\0\0\0")
        w.u32(rid)
        w.u8(3)  # response_flags: reply expected
        w.raw(b"\0\0\0")
        w.u16(0)  # TargetAddress: KeyAddr
        w.octets(key)
        w.string(operation)
        w.u32(0)  # no service contexts

        if raw_body is not None:
            body = raw_body
        elif write_args is not None:
            bw = W(little=True, start=_align(len(w.b), 8))
            write_args(bw)
            body = bytes(bw.b)
        else:
            body = b""
        if body:
            w.align(8)
            w.raw(body)
        struct.pack_into("<I", w.b, size_at, len(w.b) - 12)

        self.sock.sendall(bytes(w.b))
        return self._reply()

    def _reply(self):
        header = self._recv(12)
        if header[:4] != b"GIOP":
            raise ValueError(f"not a GIOP message: {header[:4]!r}")
        little = bool(header[6] & 1)
        mtype = header[7]
        (size,) = struct.unpack_from("<I" if little else ">I", header, 8)
        payload = self._recv(size)
        whole = header + payload

        if mtype == 5:
            raise EOFError("the peer sent CloseConnection")
        if mtype == 6:
            raise ValueError("the peer sent MessageError")
        if mtype != 1:
            raise ValueError(f"expected a Reply, got message type {mtype}")

        r = R(whole, little=little, pos=12)
        r.u32()  # request id
        status = r.u32()
        contexts = r.u32()
        for _ in range(contexts):
            r.u32()
            r.octets()
        if r.remaining() > 0:
            r.align(8)

        if status == SYSTEM_EXCEPTION:
            return Answer(status, exc_id=r.string(), minor=r.u32(), completed=r.u32())
        if status == USER_EXCEPTION:
            # The body starts at the repository id; members follow, and the
            # caller decodes them if it cares.
            at = r.pos
            exc_id = r.string()
            r.pos = at
            return Answer(status, body=r, exc_id=exc_id)
        return Answer(status, body=r)


def probe(conn, ref, operation):
    """One classification probe. See the module docs for why the body is 64
    zero bytes rather than empty."""
    try:
        return conn.call(ref.key, operation, raw_body=PROBE_BODY)
    except (EOFError, OSError, ValueError) as e:
        return Answer(-1, note=f"UNMEASURED ({e})")


# ─────────────────────────────────────────────────────────────────────────────
# The declared operation lists, read out of IDL rather than typed in
# ─────────────────────────────────────────────────────────────────────────────

INTERFACE_RE = re.compile(r"^\s*interface\s+(\w+)\s*(?::\s*([^{]+))?\{")
FORWARD_RE = re.compile(r"^\s*interface\s+(\w+)\s*;")
MODULE_RE = re.compile(r"^\s*module\s+(\w+)\s*\{")
ATTR_RE = re.compile(r"^\s*(readonly\s+)?attribute\s+.*?(\w+)\s*;")
OP_RE = re.compile(
    r"^\s*(?:oneway\s+)?[\w:]+(?:\s*<[^>]*>)?\s+(\w+)\s*\([^)]*\)\s*(?:raises\s*\([^)]*\))?\s*;"
)


def declared_operations(idl_paths, includes):
    """Every operation each interface declares, from `omniidl -b dump`.

    omniidl is run as an external program and its text output is read — the
    licensing boundary's clause (b). Nothing here imports any part of omniORB.

    Attributes become the `_get_`/`_set_` operations they are on the wire
    (§11.3.7), because that is the name a request actually carries.
    """
    args = ["omniidl"] + [f"-I{i}" for i in includes] + ["-b", "dump"]
    text = ""
    for path in idl_paths:
        done = subprocess.run(args + [path], capture_output=True, text=True)
        if done.returncode != 0:
            raise RuntimeError(f"omniidl failed on {path}:\n{done.stderr}")
        text += done.stdout + "\n"

    interfaces = {}
    scope = []  # (kind, name) — 'module' | 'interface' | 'block'
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        opens = stripped.count("{") - stripped.count("}")

        m = MODULE_RE.match(line)
        if m:
            scope.append(("module", m.group(1)))
            continue
        m = INTERFACE_RE.match(line)
        if m:
            name = "::".join([s[1] for s in scope if s[0] == "module"] + [m.group(1)])
            bases = []
            if m.group(2):
                bases = [b.strip() for b in m.group(2).split(",") if b.strip()]
            interfaces.setdefault(name, {"bases": [], "ops": []})
            interfaces[name]["bases"] = bases
            scope.append(("interface", name))
            continue
        if FORWARD_RE.match(line):
            continue

        current = scope[-1] if scope else None
        if current and current[0] == "interface" and opens == 0:
            m = ATTR_RE.match(line)
            if m:
                interfaces[current[1]]["ops"].append("_get_" + m.group(2))
                if not m.group(1):
                    interfaces[current[1]]["ops"].append("_set_" + m.group(2))
                continue
            m = OP_RE.match(line)
            if m and not stripped.startswith(("typedef", "struct", "enum", "union", "exception")):
                interfaces[current[1]]["ops"].append(m.group(1))
                continue

        if opens > 0:
            scope.append(("block", None))
        elif opens < 0 and scope:
            for _ in range(-opens):
                if scope:
                    scope.pop()
    return interfaces


def qualify(interfaces, base, from_scope):
    """Resolves an inherited interface name the way IDL scoping does: try the
    innermost enclosing scope first, then each enclosing one outward.

    `interface EnterpriseExpert : Expert` inside `module moe { module
    enterprise` inherits `moe::Expert`, and reading the name as
    `moe::enterprise::Expert` and giving up would silently drop two operations
    the servant does serve."""
    if base in interfaces:
        return base
    parts = from_scope.split("::")[:-1]
    while parts:
        candidate = "::".join(parts + [base])
        if candidate in interfaces:
            return candidate
        parts.pop()
    return base


def resolve(interfaces, name, seen=None):
    """Every operation reachable on `name`, its own first, then its bases'."""
    seen = seen if seen is not None else set()
    if name in seen or name not in interfaces:
        return []
    seen.add(name)
    out = [(name, op) for op in interfaces[name]["ops"]]
    for base in interfaces[name]["bases"]:
        out += resolve(interfaces, qualify(interfaces, base, name), seen)
    return out


# ─────────────────────────────────────────────────────────────────────────────
# The sweep
# ─────────────────────────────────────────────────────────────────────────────


class Sweep:
    """Accumulates rows: (service, object label, interface, operation, answer,
    kind)."""

    def __init__(self):
        self.rows = []
        self.notes = []
        self.unmeasured = []

    def record(self, service, obj, iface, op, answer):
        if answer.status == -1:
            kind = "unmeasured"
            self.unmeasured.append(f"{service}/{obj}/{op}: {answer.note}")
        elif answer.status == SYSTEM_EXCEPTION and answer.exc_id.endswith("BAD_OPERATION:1.0"):
            kind = "not-dispatched"
        elif answer.status == SYSTEM_EXCEPTION and answer.exc_id.endswith("NO_PERMISSION:1.0"):
            kind = "refused"
        else:
            kind = "dispatched"
        self.rows.append(
            {
                "service": service,
                "object": obj,
                "interface": iface,
                "operation": op,
                "answer": answer.short(),
                "kind": kind,
            }
        )

    def note(self, text):
        self.notes.append(text)


def sweep_object(sweep, service, label, conn, ref, ops):
    for iface, op in ops:
        sweep.record(service, label, iface, op, probe(conn, ref, op))


# ── CosNaming ────────────────────────────────────────────────────────────────


def do_naming(sweep, interfaces, ref):
    conn = Conn(ref)
    try:
        # Real calls first: the servant's answers, not just its dispatch table.
        a = conn.call(ref.key, "resolve_str", lambda w: w.string("spike/Echo"))
        sweep.note(
            "CosNaming resolve_str('spike/Echo') -> "
            + (
                f"object key {read_objref(a.body).key!r}"
                if a.status == NO_EXCEPTION
                else a.short()
            )
        )
        a = conn.call(ref.key, "to_name", lambda w: w.string("a/b"))
        if a.status == NO_EXCEPTION:
            n = a.body.u32()
            parts = [(a.body.string(), a.body.string()) for _ in range(n)]
            sweep.note(f"CosNaming to_name('a/b') -> {parts}")
        a = conn.call(ref.key, "list", lambda w: w.u32(100))
        if a.status == NO_EXCEPTION:
            sweep.note(f"CosNaming list(100) -> {a.body.u32()} binding(s) + a nil iterator")
        a = conn.call(ref.key, "resolve_str", lambda w: w.string("nope"))
        sweep.note(f"CosNaming resolve_str('nope') -> {a.short()}")

        ops = resolve(interfaces, "CosNaming::NamingContextExt")
        sweep_object(sweep, "CosNaming", "NamingContextExt (root)", conn, ref, ops)
        sweep_object(
            sweep,
            "CosNaming",
            "NamingContextExt (root)",
            conn,
            ref,
            [(CORBA_OBJECT_ID, "_is_a"), (CORBA_OBJECT_ID, "_non_existent")],
        )
    finally:
        conn.close()


# ── CosEvent ─────────────────────────────────────────────────────────────────


def do_event(sweep, interfaces, ref):
    conn = Conn(ref)
    try:
        objects = [("EventChannel", "CosEventChannelAdmin::EventChannel", ref)]

        a = conn.call(ref.key, "for_consumers")
        consumer_admin = read_objref(a.body) if a.status == NO_EXCEPTION else None
        sweep.note(f"CosEvent for_consumers() -> {a.short()}")
        a = conn.call(ref.key, "for_suppliers")
        supplier_admin = read_objref(a.body) if a.status == NO_EXCEPTION else None
        sweep.note(f"CosEvent for_suppliers() -> {a.short()}")

        push_supplier = push_consumer = None
        if consumer_admin:
            objects.append(("ConsumerAdmin", "CosEventChannelAdmin::ConsumerAdmin", consumer_admin))
            a = conn.call(consumer_admin.key, "obtain_push_supplier")
            push_supplier = read_objref(a.body) if a.status == NO_EXCEPTION else None
        if supplier_admin:
            objects.append(("SupplierAdmin", "CosEventChannelAdmin::SupplierAdmin", supplier_admin))
            a = conn.call(supplier_admin.key, "obtain_push_consumer")
            push_consumer = read_objref(a.body) if a.status == NO_EXCEPTION else None
        if push_supplier:
            objects.append(
                (
                    "ProxyPushSupplier",
                    "CosEventChannelAdmin::ProxyPushSupplier",
                    push_supplier,
                )
            )
        if push_consumer:
            objects.append(
                (
                    "ProxyPushConsumer",
                    "CosEventChannelAdmin::ProxyPushConsumer",
                    push_consumer,
                )
            )
            # A real push: a `tk_ulong` any carrying 7. `connect_push_supplier`
            # first, because an unconnected proxy raises Disconnected — which
            # is itself the answer worth recording.
            before = conn.call(push_consumer.key, "push", lambda w: (w.u32(3), w.u32(7)))
            sweep.note(f"CosEvent push() before connect -> {before.short()} (Disconnected)")
            conn.call(push_consumer.key, "connect_push_supplier", lambda w: write_objref(w, None))
            after = conn.call(push_consumer.key, "push", lambda w: (w.u32(3), w.u32(7)))
            sweep.note(f"CosEvent push(any:ulong 7) after connect -> {after.short()}")

        for label, iface, obj in objects:
            sweep_object(sweep, "CosEvent", label, conn, obj, resolve(interfaces, iface))
            sweep_object(
                sweep,
                "CosEvent",
                label,
                conn,
                obj,
                [(CORBA_OBJECT_ID, "_is_a"), (CORBA_OBJECT_ID, "_non_existent")],
            )

        # The pull half declares four interfaces whose objects cannot be
        # obtained at all, because both `obtain_pull_*` are refused. Their
        # operations are probed against the push proxies so the sweep says
        # something measured about them rather than nothing: an operation
        # nothing can address is still an operation the channel does not have.
        if push_supplier:
            pull = resolve(interfaces, "CosEventChannelAdmin::ProxyPullSupplier")
            sweep_object(sweep, "CosEvent", "ProxyPushSupplier", conn, push_supplier, pull)
        if push_consumer:
            pull = resolve(interfaces, "CosEventChannelAdmin::ProxyPullConsumer")
            sweep_object(sweep, "CosEvent", "ProxyPushConsumer", conn, push_consumer, pull)
    finally:
        conn.close()


# ── Interface Repository ─────────────────────────────────────────────────────

IFR_SUBJECT = "IDL:gc10/Both:1.0"


def do_ifr(sweep, interfaces, ref):
    conn = Conn(ref)
    try:
        a = conn.call(ref.key, "lookup_id", lambda w: w.string(IFR_SUBJECT))
        entry = read_objref(a.body) if a.status == NO_EXCEPTION else None
        sweep.note(f"IFR lookup_id({IFR_SUBJECT}) -> {a.short()}")

        objects = [("Repository (root)", "CORBA::Repository", ref)]
        if entry:
            objects.append(("InterfaceDef (gc10::Both)", "CORBA::InterfaceDef", entry))
            for op, args in [
                ("_get_id", None),
                ("_get_name", None),
                ("_get_absolute_name", None),
            ]:
                a = conn.call(entry.key, op, args)
                if a.status == NO_EXCEPTION:
                    sweep.note(f"IFR {op}() -> {a.body.string()!r}")
            a = conn.call(entry.key, "_get_def_kind")
            if a.status == NO_EXCEPTION:
                sweep.note(f"IFR _get_def_kind() -> {a.body.u32()} (dk_Interface is 5)")
            a = conn.call(entry.key, "is_a", lambda w: w.string("IDL:gc10/Nameable:1.0"))
            if a.status == NO_EXCEPTION:
                sweep.note(f"IFR is_a('IDL:gc10/Nameable:1.0') -> {a.body.u8() != 0}")
            a = conn.call(entry.key, "describe_interface")
            if a.status == NO_EXCEPTION:
                name = a.body.string()
                rid = a.body.string()
                sweep.note(f"IFR describe_interface() -> name {name!r}, id {rid!r}")
            a = conn.call(entry.key, "_get_base_interfaces")
            if a.status == NO_EXCEPTION:
                sweep.note(f"IFR _get_base_interfaces() -> {a.body.u32()} base(s)")

        for label, iface, obj in objects:
            sweep_object(sweep, "IFR", label, conn, obj, resolve(interfaces, iface))
            sweep_object(
                sweep,
                "IFR",
                label,
                conn,
                obj,
                [(CORBA_OBJECT_ID, "_is_a"), (CORBA_OBJECT_ID, "_non_existent")],
            )
    finally:
        conn.close()


# ── moe::ExpertRegistry / moe::ExpertLoader (corpus/golden/22) ───────────────


def write_capability(w, cid):
    w.string(cid)
    w.f32(1.5)  # cost
    w.f32(180.0)  # latency_p99_ms
    w.f32(0.25)  # load
    w.u32(2)  # Residency::RESIDENT — a report; the loader is the authority
    w.u64(30)  # mem_footprint
    w.f32(0.0)  # route_freq — likewise a report
    w.string("gpu-04")
    w.string("moe/1.0")


def do_experts(sweep, interfaces, registry, loader):
    conn = Conn(registry)
    try:
        expert = Ref("IDL:moe/Expert:1.0", "192.0.2.7", 4242, b"expert-sweep")

        def register(w):
            write_objref(w, expert)
            write_capability(w, "expert-sweep")

        a = conn.call(registry.key, "register_expert", register)
        sweep.note(f"MoE register_expert(expert-sweep) -> {a.short()}")
        a = conn.call(registry.key, "heartbeat", register)
        sweep.note(f"MoE heartbeat(expert-sweep) -> {a.short()}")

        a = conn.call(loader.key, "status", lambda w: w.string("expert-sweep"))
        if a.status == NO_EXCEPTION:
            sweep.note(f"MoE status('expert-sweep') -> Residency ordinal {a.body.u32()}")
        else:
            sweep.note(f"MoE status('expert-sweep') -> {a.short()}")
        a = conn.call(loader.key, "pin", lambda w: w.string("expert-sweep"))
        sweep.note(f"MoE pin('expert-sweep') -> {a.short()}")
        a = conn.call(loader.key, "evict", lambda w: w.string("expert-sweep"))
        sweep.note(f"MoE evict('expert-sweep') after pin -> {a.short()}")

        sweep_object(
            sweep,
            "MoE control plane",
            "ExpertRegistry",
            conn,
            registry,
            resolve(interfaces, "moe::ExpertRegistry"),
        )
        sweep_object(
            sweep,
            "MoE control plane",
            "ExpertLoader",
            conn,
            loader,
            resolve(interfaces, "moe::ExpertLoader"),
        )
        # The other two interfaces the contract declares. No object of either
        # is served, so they are probed against the two that are — an operation
        # answered by neither is answered by nothing.
        for label, obj in [("ExpertRegistry", registry), ("ExpertLoader", loader)]:
            for iface in ("moe::Expert", "moe::Router"):
                sweep_object(sweep, "MoE control plane", label, conn, obj, resolve(interfaces, iface))
        for label, obj in [("ExpertRegistry", registry), ("ExpertLoader", loader)]:
            sweep_object(
                sweep,
                "MoE control plane",
                label,
                conn,
                obj,
                [(CORBA_OBJECT_ID, "_is_a"), (CORBA_OBJECT_ID, "_non_existent")],
            )
    finally:
        conn.close()


# ── moe::enterprise (corpus/golden/23) ───────────────────────────────────────

TENANT = "acme"
DOMAIN = "default"
BASE_MODEL = "llama-3"
CAPABILITY = "math"


def write_manifest(w):
    w.string(TENANT)
    w.string(BASE_MODEL)
    w.u32(0)  # experts
    w.string(DOMAIN)
    w.string("v1")
    w.string("eu-west")


def write_activation_and_ctx(w):
    w.octets(b"\x01\x02")
    w.string("f32")
    w.string("[2]")
    w.string("req-1")
    w.string("trace-1")
    w.u32(0)


def do_tenants(sweep, interfaces, factory):
    conn = Conn(factory)
    try:
        a = conn.call(factory.key, "create", write_manifest)
        model = read_objref(a.body) if a.status == NO_EXCEPTION else None
        sweep.note(f"MoE-E create(manifest) -> {a.short()}")

        base_prefix = factory.key[: -len(f"/t/{TENANT}/factory".encode())]
        policy = factory.with_key(
            base_prefix + f"/t/{TENANT}/policy/{DOMAIN}".encode(),
            "IDL:moe/enterprise/PolicyDomain:1.0",
        )
        expert = factory.with_key(
            base_prefix + f"/t/{TENANT}/expert/{CAPABILITY}".encode(),
            "IDL:moe/enterprise/EnterpriseExpert:1.0",
        )
        shared = factory.with_key(
            base_prefix + f"/shared/base/{BASE_MODEL}".encode(), "IDL:moe/Expert:1.0"
        )

        if model:
            a = conn.call(model.key, "get_manifest")
            if a.status == NO_EXCEPTION:
                sweep.note(
                    f"MoE-E get_manifest() -> tenant {a.body.string()!r}, "
                    f"base {a.body.string()!r}"
                )
            a = conn.call(model.key, "infer", write_activation_and_ctx)
            sweep.note(f"MoE-E infer(activation) -> {a.short()}")
        a = conn.call(expert.key, "get_tenant_id")
        if a.status == NO_EXCEPTION:
            sweep.note(f"MoE-E get_tenant_id() -> {a.body.string()!r}")
        a = conn.call(expert.key, "base")
        if a.status == NO_EXCEPTION:
            got = read_objref(a.body)
            sweep.note(f"MoE-E base() -> {got.type_id} key {got.key!r}")
        a = conn.call(policy.key, "check_residency", lambda w: w.string("gpu-04"))
        if a.status == NO_EXCEPTION:
            sweep.note(f"MoE-E check_residency('gpu-04') -> {a.body.u8() != 0}")
        else:
            sweep.note(f"MoE-E check_residency('gpu-04') -> {a.short()}")
        a = conn.call(policy.key, "authorize", lambda w: (w.string("nobody"), w.string("math")))
        if a.status == NO_EXCEPTION:
            sweep.note(f"MoE-E authorize('nobody','math') -> {a.body.u8() != 0} (default-deny)")

        objects = [
            ("ModelFactory", "moe::enterprise::ModelFactory", factory),
            ("PolicyDomain", "moe::enterprise::PolicyDomain", policy),
            ("EnterpriseExpert", "moe::enterprise::EnterpriseExpert", expert),
            ("shared ::moe::Expert", "moe::Expert", shared),
        ]
        if model:
            objects.insert(1, ("ComposedModel", "moe::enterprise::ComposedModel", model))

        for label, iface, obj in objects:
            sweep_object(sweep, "MoE enterprise", label, conn, obj, resolve(interfaces, iface))
            sweep_object(
                sweep,
                "MoE enterprise",
                label,
                conn,
                obj,
                [(CORBA_OBJECT_ID, "_is_a"), (CORBA_OBJECT_ID, "_non_existent")],
            )
    finally:
        conn.close()


# ─────────────────────────────────────────────────────────────────────────────


def resolve_idl_root():
    for root in IDL_ROOT_CANDIDATES:
        if os.path.isdir(os.path.join(root, "COS")):
            return root
    return None


def main(argv):
    ior_dir = argv[1] if len(argv) > 1 else "spikes"
    root = resolve_idl_root()
    if root is None:
        print("BLOCKED: omniORB's IDL directory was not found in any of:")
        for c in IDL_ROOT_CANDIDATES:
            print(f"  {c}")
        print("An unmeasured check is a failure, not a pass. Install omniORB.")
        return 1
    cos = os.path.join(root, "COS")
    print(f"idl-root {root}")

    omg = declared_operations(
        [
            os.path.join(cos, "CosNaming.idl"),
            os.path.join(cos, "CosEventComm.idl"),
            os.path.join(cos, "CosEventChannelAdmin.idl"),
            os.path.join(root, "ir.idl"),
        ],
        [root, cos],
    )
    # The two project contracts are read *separately* and never merged: each
    # declares its own `moe::Expert`, and golden 22's carries a `delegate` that
    # golden 23's does not. Merging them would invent an operation neither
    # servant's contract asks for.
    control = declared_operations(["corpus/golden/22-moe-control-plane.idl"], [])
    enterprise = declared_operations(["corpus/golden/23-moe-enterprise.idl"], [])

    sweep = Sweep()
    plan = [
        ("names.ior", lambda ref: do_naming(sweep, omg, ref)),
        ("events.ior", lambda ref: do_event(sweep, omg, ref)),
        ("ifr.ior", lambda ref: do_ifr(sweep, omg, ref)),
        ("moe-factory.ior", lambda ref: do_tenants(sweep, enterprise, ref)),
    ]
    for name, run in plan:
        path = os.path.join(ior_dir, name)
        try:
            ref = parse_ior(open(path).read())
        except OSError as e:
            sweep.unmeasured.append(f"{name}: {e}")
            continue
        try:
            run(ref)
        except Exception as e:  # noqa: BLE001 — an unmeasured check is a failure
            sweep.unmeasured.append(f"{name}: {type(e).__name__}: {e}")

    try:
        registry = parse_ior(open(os.path.join(ior_dir, "moe-registry.ior")).read())
        loader = parse_ior(open(os.path.join(ior_dir, "moe-loader.ior")).read())
        do_experts(sweep, control, registry, loader)
    except Exception as e:  # noqa: BLE001
        sweep.unmeasured.append(f"moe-registry.ior: {type(e).__name__}: {e}")

    # ── output: one TSV row per probe, then the real answers, then totals ────
    print("\n#ROWS\tservice\tobject\tinterface\toperation\tanswer\tkind")
    for row in sweep.rows:
        print(
            "ROW\t{service}\t{object}\t{interface}\t{operation}\t{answer}\t{kind}".format(**row)
        )

    print("\n#ANSWERS")
    for n in sweep.notes:
        print(f"ANSWER\t{n}")

    print("\n#TOTALS")
    services = []
    for row in sweep.rows:
        if row["service"] not in services:
            services.append(row["service"])
    for service in services:
        rows = [r for r in sweep.rows if r["service"] == service]
        counts = {k: sum(1 for r in rows if r["kind"] == k) for k in
                  ("dispatched", "refused", "not-dispatched", "unmeasured")}
        print(
            f"TOTAL\t{service}\tprobes {len(rows)}\tdispatched {counts['dispatched']}"
            f"\tNO_PERMISSION {counts['refused']}\tBAD_OPERATION {counts['not-dispatched']}"
            f"\tunmeasured {counts['unmeasured']}"
        )

    if sweep.unmeasured:
        print("\n#UNMEASURED")
        for u in sweep.unmeasured:
            print(f"UNMEASURED\t{u}")
        print(f"\nservice-sweep: FAIL — {len(sweep.unmeasured)} unmeasured check(s)")
        return 1
    print("\nservice-sweep: PASS — every declared operation was answered")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
