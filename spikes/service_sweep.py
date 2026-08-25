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
        """One request, re-sent once if the peer closes the connection.

        §13.5.1: the requests outstanding when a `CloseConnection` arrives
        "were not processed, and may be safely resent on a new connection" —
        a promise about processing, not about idempotence, which is exactly
        what makes one re-send correct and a second one a hot loop against a
        server that is refusing. Treating the close as a failed measurement
        was this driver reading a normal wire event as a fault: it appeared
        only under harness load, where a servant is slow enough to reach a
        condition that closes, and never in three standalone runs.
        """
        try:
            return self._call_once(key, operation, write_args, raw_body)
        except EOFError:
            self.reconnect()
            return self._call_once(key, operation, write_args, raw_body)

    def reconnect(self):
        self.close()
        self.sock = socket.create_connection((self.ref.host, self.ref.port), TIMEOUT)
        self.sock.settimeout(TIMEOUT)

    def _call_once(self, key, operation, write_args=None, raw_body=None):
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
# Lines that legitimately stand inside an interface and declare no operation:
# braces, preprocessor and pragma lines, comments, and the type declarations
# an interface may carry. The keyword must be followed by space — `enum X {`
# is a type, `enumKind k();` is an operation whose return type starts with
# those four letters, and the `startswith` guard on OP_RE above cannot tell
# them apart. Writing the distinction here means the counter reports that
# operation as unread rather than joining the guard in dropping it.
STRUCTURAL_RE = re.compile(
    r"^(?:[{}();]+\s*$|#|//|/\*|\*|"
    r"(?:typedef|struct|enum|union|exception|const|native)\s)")


def declared_operations(idl_paths, includes, unclassified=None):
    """Every operation each interface declares, from `omniidl -b dump`.

    omniidl is run as an external program and its text output is read — the
    licensing boundary's clause (b). Nothing here imports any part of omniORB.

    Attributes become the `_get_`/`_set_` operations they are on the wire
    (§11.3.7), because that is the name a request actually carries.

    `unclassified`, when a list is passed, receives every line standing
    directly inside an interface that matched neither `ATTR_RE` nor `OP_RE`.
    Those lines are the silent half of this reader: an operation whose shape
    the regex misses is not declared, so it is never probed, never appears in
    `#ABSENT`, and cannot appear in `#UNPROBED-INTERFACES` either — that list
    is built from the interfaces this same regex parsed. The declared count
    and the probe count fall by one each, stay consistent with each other, and
    the run prints `every declared operation was answered`.

    Measured 2026-08-25 over the four COS/IR contracts and golden 22/23:
    **50 interfaces, 189 operations, 0 unclassified.** Two shapes reproduce
    through `omniidl -b dump` and are now reported — `unsigned long f();`
    (`[\\w:]+` takes one word) and `enumKind k();` (the `startswith` guard
    below cannot tell a keyword-prefixed return type from a type
    declaration). Two more are real and this counter does **not** see them,
    which is why they are written down here rather than claimed as covered:
    `abstract interface X` does not match `INTERFACE_RE`, so its whole body
    falls under a `("block", None)` scope and never reaches this
    fall-through; and `attribute long a, b;` matches `ATTR_RE`, which takes
    only `b`. Neither shape occurs in today's input, so neither could be
    widened against real input in this tree. A `context (…)` clause is not a
    hazard here at all: the dump elides it.
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
            # Standing inside an interface, opening no block, and matching
            # neither shape. Structural braces and preprocessor lines are the
            # expected residue; anything else is a declaration this reader did
            # not understand and therefore did not declare.
            if unclassified is not None and not STRUCTURAL_RE.match(stripped):
                unclassified.append((current[1], stripped))

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
            # "No such operation." An oversight and a decision both used to say
            # this, so the only thing separating them was a sentence in a
            # document the client cannot read. Twelve of 107 operations were in
            # that state on 2026-08-14, which is why this row now means one
            # thing: **nobody decided**.
            kind = "not-dispatched"
        elif answer.status == SYSTEM_EXCEPTION and answer.exc_id.endswith("NO_IMPLEMENT:1.0"):
            # "The operation exists in the contract and this servant does not
            # implement it, on purpose." Counted separately because counting it
            # as dispatched overstated the IFR facade by ten operations.
            kind = "deferred"
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

        # The pull half. `obtain_pull_supplier` is served now, so the sweep
        # asks for the real object rather than probing a push proxy that never
        # claimed the interface — which reported the whole interface as
        # *unserved* the moment it started being served, because the sweep was
        # measuring the wrong object. `obtain_pull_consumer` is still refused,
        # deliberately, so its interface has no object and the probe against a
        # push proxy is what stays honest there: an operation nothing can
        # address is still an operation the channel does not have.
        pull_supplier = None
        if consumer_admin:
            a = conn.call(consumer_admin.key, "obtain_pull_supplier")
            sweep.note(f"CosEvent obtain_pull_supplier() -> {a.short()}")
            pull_supplier = read_objref(a.body) if a.status == NO_EXCEPTION else None
        pull_sup_ops = resolve(interfaces, "CosEventChannelAdmin::ProxyPullSupplier")
        if pull_supplier:
            sweep_object(sweep, "CosEvent", "ProxyPullSupplier", conn, pull_supplier, pull_sup_ops)
        elif push_supplier:
            sweep_object(sweep, "CosEvent", "ProxyPushSupplier", conn, push_supplier, pull_sup_ops)
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


def do_experts(sweep, interfaces, registry, loader, router=None):
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
        if router is not None:
            sweep_object(
                sweep,
                "MoE control plane",
                "Router",
                conn,
                router,
                resolve(interfaces, "moe::Router"),
            )

        # The interfaces the contract declares with no object of their own.
        # Probed against the objects that are served, because an operation
        # answered by none of them is answered by nothing. `moe::Router` joins
        # this list only when no router object was published.
        absent = ["moe::Expert"] if router is not None else ["moe::Expert", "moe::Router"]
        for label, obj in [("ExpertRegistry", registry), ("ExpertLoader", loader)]:
            for iface in absent:
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

    # A declaration this reader cannot classify is an operation that is never
    # probed, and silence about it reads as coverage — the same way silence
    # about unprobed interfaces did until 2026-08-19. Collected here and given
    # to the sweep as unmeasured, which is what it is.
    unclassified = []
    omg = declared_operations(
        [
            os.path.join(cos, "CosNaming.idl"),
            os.path.join(cos, "CosEventComm.idl"),
            os.path.join(cos, "CosEventChannelAdmin.idl"),
            os.path.join(root, "ir.idl"),
        ],
        [root, cos],
        unclassified,
    )
    # The two project contracts are read *separately* and never merged: each
    # declares its own `moe::Expert`, and golden 22's carries a `delegate` that
    # golden 23's does not. Merging them would invent an operation neither
    # servant's contract asks for.
    control = declared_operations(["corpus/golden/22-moe-control-plane.idl"], [], unclassified)
    enterprise = declared_operations(["corpus/golden/23-moe-enterprise.idl"], [], unclassified)

    sweep = Sweep()
    for iface, line in unclassified:
        sweep.unmeasured.append(
            f"{iface}: declaration not classified by this reader, so it was never "
            f"probed — {line}")
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
        router_path = os.path.join(ior_dir, "moe-router.ior")
        router = parse_ior(open(router_path).read()) if os.path.exists(router_path) else None
        if router is None:
            sweep.unmeasured.append("moe-router.ior: absent, so Router is probed as unserved")
        do_experts(sweep, control, registry, loader, router)
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
                  ("dispatched", "refused", "deferred", "not-dispatched", "unmeasured")}
        print(
            f"TOTAL\t{service}\tprobes {len(rows)}\tdispatched {counts['dispatched']}"
            f"\tNO_PERMISSION {counts['refused']}\tNO_IMPLEMENT {counts['deferred']}"
            f"\tBAD_OPERATION {counts['not-dispatched']}"
            f"\tunmeasured {counts['unmeasured']}"
        )

    if sweep.unmeasured:
        print("\n#UNMEASURED")
        for u in sweep.unmeasured:
            print(f"UNMEASURED\t{u}")
        print(f"\nservice-sweep: FAIL — {len(sweep.unmeasured)} unmeasured check(s)")
        return 1

    # An absence is a `BAD_OPERATION` from an object that **claims** the
    # interface: the servant is half-serving something it says it is. The same
    # answer from an object that never claimed the interface is correct and is
    # a different fact — an interface no object serves — reported below and not
    # counted here.
    # An object **claims** an interface when it answers at least one of its
    # operations with something other than "no such operation". Read out of the
    # rows already measured rather than asked for over the wire: `_is_a` would
    # mean constructing a repository id from a scoped name, and that guess is
    # wrong for every COS interface — `IDL:CosNaming/…` where the contract's
    # `#pragma prefix` makes it `IDL:omg.org/CosNaming/…`. The first version of
    # this check did guess, and it passed a deliberately broken servant.
    claimed = {
        (r["service"], r["object"], r["interface"])
        for r in sweep.rows
        if r["kind"] != "not-dispatched"
    }
    absent = [
        r for r in sweep.rows
        if r["kind"] == "not-dispatched"
        and (r["service"], r["object"], r["interface"]) in claimed
    ]
    unserved = sorted({
        (r["service"], r["interface"])
        for r in sweep.rows
        if r["kind"] == "not-dispatched"
        and (r["service"], r["object"], r["interface"]) not in claimed
    })
    if unserved:
        print("\n#UNSERVED-INTERFACES")
        for service, iface in unserved:
            print(f"UNSERVED\t{service}\t{iface}\tdeclared, claimed by no object probed")
    # An interface the IDL declares and this sweep probed against *no* object
    # is a third fact, and until 2026-08-19 it was silent: `BindingIterator`
    # (no iterator object exists to address), the pull-side client interfaces
    # (`PullSupplier`, `PullConsumer` — implemented by a client, not this
    # channel), and most of `ir.idl`. Silence read as coverage. Listed so the
    # generated document can say "unmeasured" where it used to say nothing.
    probed = {(r["service"], r["interface"]) for r in sweep.rows}
    scope = [
        ("CosNaming", omg, ("CosNaming::",)),
        ("CosEvent", omg, ("CosEventComm::", "CosEventChannelAdmin::")),
        ("IFR", omg, ("CORBA::",)),
        ("MoE enterprise", enterprise, ("",)),
        ("MoE control plane", control, ("",)),
    ]
    unprobed = []
    for service, decl, prefixes in scope:
        for iface in decl:
            if any(iface.startswith(p) for p in prefixes) and (service, iface) not in probed:
                unprobed.append((service, iface, len(decl[iface]["ops"])))
    if unprobed:
        print("\n#UNPROBED-INTERFACES")
        for service, iface, n in unprobed:
            print(f"UNPROBED\t{service}\t{iface}\t{n} declared operation(s), probed against no object")
    if absent:
        print("\n#ABSENT")
        for r in absent:
            print(f"ABSENT\t{r['service']}\t{r['object']}\t{r['interface']}::{r['operation']}")
        print(
            f"\nservice-sweep: FAIL — {len(absent)} operation(s) answered BAD_OPERATION by an "
            "object that claims the interface. That is the answer an oversight and a decision "
            "both used to give; a decision now says NO_IMPLEMENT, so this list is the one "
            "nobody has decided."
        )
        return 1
    print("\nservice-sweep: PASS — every declared operation was answered")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
