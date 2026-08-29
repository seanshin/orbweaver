#!/usr/bin/env python3
"""What does a real client *do* with a conversion list we advertise?
(D009 §8, batch 4 — the other half of `codeset_peer_probe.py`)

`codeset_peer_probe.py` asks whether a peer exists that needs us to advertise a
`char` conversion. This one asks what advertising it would cost the peers that
already work, and it asks omniORB rather than asking ourselves: a hand-written
GIOP listener publishes an IOR whose `TAG_CODE_SETS` component we choose, an
unmodified omniORB 4.3.4 client calls `echo_string` on it, and the `CodeSets`
service context plus the argument octets it actually sent are decoded and
printed.

The listener is ours, so it proves nothing about what an ORB *advertises* —
that is `codeset_peer_probe.py`'s job, and only a peer we did not write can
answer it. What it does prove is the direction of omniORB's choice, which is
a fact about omniORB: §7.10.2.6 lists "client's native, which the server
converts" **before** "the server's native, which the client converts", so a
conversion list is not a free addition. It changes what an existing peer picks.

TEST FIXTURE ONLY — omniORB is LGPL/GPL, run here as a separate process over
TCP. See docs/PLAN.md §10.

    python3 spikes/codeset_advertise_probe.py

Exit 0 when both advertisements were measured, 2 when either could not be.
An unmeasured check is a failure, never a pass.

*광고한 변환 목록이 실제 클라이언트의 선택을 어떻게 바꾸는지 잰다. 목록을
늘리는 것은 공짜가 아니라 이미 동작하는 피어의 선택을 바꾸는 일이다.*
"""

import os
import pathlib
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave, wrap_child

HERE = pathlib.Path(__file__).resolve().parent

ISO_8859_1 = 0x00010001
UTF_8 = 0x05010001
UTF_16 = 0x00010109
TAG_CODE_SETS = 1
SERVICE_ID_CODE_SETS = 1
NAMES = {ISO_8859_1: "ISO-8859-1", UTF_8: "UTF-8", UTF_16: "UTF-16", 0x00010100: "UCS-2"}

TYPE_ID = "IDL:spike/Echo:1.0"
OBJECT_KEY = b"codeset-probe"
# Latin-1 representable, and one character above ASCII so the two candidate
# codesets produce visibly different octets: "café" is 4 bytes in ISO-8859-1
# and 5 in UTF-8. Asserting the octets is the point — a string that survives a
# wrong conversion twice looks identical.
PROBE_TEXT = "café"
# The text that decides whether a downgrade is cosmetic or lossy: ISO-8859-1
# has no representation for it at all.
KOREAN_TEXT = "함정 전투체계"


# ── the smallest CDR that can write an IOR and read a request ───────────────


class Out:
    """A CDR encoder whose alignment origin is byte 0 of whatever it writes —
    a message for the wire, or an encapsulation, which restarts at its own
    first byte and therefore gets its own `Out`."""

    def __init__(self, big=True):
        self.buf, self.big = bytearray(), big

    def align(self, n):
        while len(self.buf) % n:
            self.buf.append(0)

    def raw(self, b):
        self.buf += b

    def u16(self, v):
        self.align(2)
        self.buf += struct.pack(">H" if self.big else "<H", v)

    def u32(self, v):
        self.align(4)
        self.buf += struct.pack(">I" if self.big else "<I", v)

    def string(self, s):
        b = s.encode("latin-1")
        self.u32(len(b) + 1)
        self.raw(b + b"\0")

    def sequence(self, b):
        self.u32(len(b))
        self.raw(b)

    def encapsulation(self, write):
        inner = Out(self.big)
        inner.raw(b"\0" if self.big else b"\1")
        write(inner)
        self.sequence(bytes(inner.buf))


class In:
    def __init__(self, buf, big, pos=0):
        self.buf, self.big, self.pos = buf, big, pos

    def align(self, n):
        pad = self.pos % n
        if pad:
            self.pos += n - pad

    def octets(self, n):
        b = self.buf[self.pos : self.pos + n]
        if len(b) != n:
            raise ValueError("truncated message")
        self.pos += n
        return b

    def u8(self):
        return self.octets(1)[0]

    def u16(self):
        self.align(2)
        return struct.unpack(">H" if self.big else "<H", self.octets(2))[0]

    def u32(self):
        self.align(4)
        return struct.unpack(">I" if self.big else "<I", self.octets(4))[0]

    def string_octets(self):
        n = self.u32()
        return self.octets(n)[:-1]

    def sequence(self):
        return self.octets(self.u32())


def build_ior(host, port, conversions):
    """A stringified IOR for an IIOP 1.2 profile carrying exactly the
    `TAG_CODE_SETS` component this probe wants to test."""

    def profile(p):
        p.raw(b"\x01\x02")  # IIOP version 1.2
        p.string(host)
        p.u16(port)
        p.sequence(OBJECT_KEY)
        p.u32(1)  # one component
        p.u32(TAG_CODE_SETS)

        def codesets(c):
            c.u32(UTF_8)  # char native
            c.u32(len(conversions))
            for cs in conversions:
                c.u32(cs)
            c.u32(UTF_16)  # wchar native
            c.u32(0)

        p.encapsulation(codesets)

    e = Out(big=True)
    e.raw(b"\0")  # big-endian encapsulation flag
    e.string(TYPE_ID)
    e.u32(1)  # one profile
    e.u32(0)  # TAG_INTERNET_IOP
    e.encapsulation(profile)
    return "IOR:" + bytes(e.buf).hex()


# ── the listener ────────────────────────────────────────────────────────────


def parse_request(msg):
    """-> (request_id, operation, {ctx id: body}, first string argument octets)."""
    if msg[:4] != b"GIOP":
        raise ValueError(f"not a GIOP message: {msg[:8]!r}")
    major, minor, flags, mtype = msg[4], msg[5], msg[6], msg[7]
    big = not (flags & 1)
    d = In(msg, big, pos=12)  # alignment origin is byte 0 of the header
    if (major, minor) != (1, 2):
        raise ValueError(f"probe speaks GIOP 1.2 only, peer sent {major}.{minor}")
    if mtype == 3:  # LocateRequest
        return ("locate", d.u32())
    if mtype != 0:
        raise ValueError(f"unexpected GIOP message type {mtype}")
    request_id = d.u32()
    d.u8()  # response flags
    d.octets(3)  # reserved
    if d.u16() != 0:
        raise ValueError("probe understands KeyAddr targets only")
    d.sequence()  # object key
    operation = d.string_octets().decode("latin-1")
    contexts = {}
    for _ in range(d.u32()):
        cid = d.u32()
        contexts[cid] = d.sequence()
    d.align(8)  # §15.4.2: a 1.2 request body starts 8-aligned
    return ("request", request_id, operation, contexts, d.string_octets())


def build_reply(request_id, payload):
    e = Out(big=True)
    e.raw(b"GIOP\x01\x02\x00\x01")
    e.raw(b"\0\0\0\0")  # size, patched below
    e.u32(request_id)
    e.u32(0)  # NO_EXCEPTION
    e.u32(0)  # no service contexts
    e.align(8)
    e.u32(len(payload) + 1)
    e.raw(payload + b"\0")
    body = bytes(e.buf)
    return body[:8] + struct.pack(">I", len(body) - 12) + body[12:]


def build_locate_reply(request_id):
    e = Out(big=True)
    e.raw(b"GIOP\x01\x02\x00\x04")
    e.raw(b"\0\0\0\0")
    e.u32(request_id)
    e.u32(1)  # OBJECT_HERE
    body = bytes(e.buf)
    return body[:8] + struct.pack(">I", len(body) - 12) + body[12:]


def read_message(conn):
    header = b""
    while len(header) < 12:
        chunk = conn.recv(12 - len(header))
        if not chunk:
            return None
        header += chunk
    size = struct.unpack(">I" if not (header[6] & 1) else "<I", header[8:12])[0]
    body = b""
    while len(body) < size:
        chunk = conn.recv(size - len(body))
        if not chunk:
            return None
        body += chunk
    return header + body


def serve_one_call(sock, result, deadline):
    """Accepts one connection and answers whatever it is asked, echoing the
    argument octets back unchanged.

    A completed client `connect` does not mean `accept` has returned, and a
    non-blocking single `accept()` misses fresh connections on macOS loopback;
    hence the sleeping, deadline-bounded loop rather than one try.
    """
    sock.settimeout(0.2)
    conn = None
    while conn is None and time.time() < deadline and not result.get("stop"):
        try:
            conn, _ = sock.accept()
        except socket.timeout:
            time.sleep(0.02)
        except OSError as exc:
            result["error"] = f"accept failed: {exc}"
            return
    if conn is None:
        result["error"] = "no connection before the deadline"
        return
    conn.settimeout(5.0)
    try:
        while time.time() < deadline:
            msg = read_message(conn)
            if msg is None:
                return
            parsed = parse_request(msg)
            if parsed[0] == "locate":
                conn.sendall(build_locate_reply(parsed[1]))
                continue
            _, request_id, operation, contexts, arg = parsed
            result.setdefault("calls", []).append((operation, contexts, arg))
            conn.sendall(build_reply(request_id, arg))
    except Exception as exc:  # noqa: BLE001 — the report wants the reason
        result.setdefault("error", f"{type(exc).__name__}: {exc}")
    finally:
        conn.close()


OMNIORB_CLIENT = r"""
import sys
from omniORB import CORBA
import omniORB
omniORB.importIDL(sys.argv[1])
import spike
orb = CORBA.ORB_init(sys.argv[4:], CORBA.ORB_ID)
ref = orb.string_to_object(sys.argv[2])._narrow(spike.Echo)
try:
    print("REPLY " + repr(ref.echo_string(sys.argv[3])))
except Exception as exc:
    print("RAISED " + type(exc).__name__ + " " + str(exc))
"""


def run_omniorb_client(ior, text):
    """omniORB 4.3.4, native `char` ISO-8859-1 by default (measured in
    `codeset_peer_probe.py`), which is what makes it worth asking."""
    return subprocess.run(
        [sys.executable, "-c", wrap_child(OMNIORB_CLIENT), str(HERE / "echo.idl"), ior, text],
        capture_output=True,
        text=True,
        timeout=60,
        cwd=str(HERE),
    )


JAVA_HOME_21 = os.environ.get(
    "JAVA_HOME_21", "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
)


def java_tool(name):
    p = pathlib.Path(JAVA_HOME_21) / "bin" / name
    return str(p) if p.is_file() else (shutil.which(name) or "")


def jacorb_classpath():
    lib = HERE / "jacorb" / "lib"
    jars = sorted(str(j) for j in lib.glob("*.jar")) if lib.is_dir() else []
    return ":".join(jars) if jars else None


def build_jacorb_client():
    """Generates the `spike` stubs and compiles the caller. Returns the
    classpath, or raises with the reason it could not be built."""
    cp = jacorb_classpath()
    if not cp:
        raise RuntimeError("no jars in spikes/jacorb/lib — run spikes/jacorb/setup.sh --jars-only")
    javac, java = java_tool("javac"), java_tool("java")
    if not javac or not java:
        raise RuntimeError(f"no JDK 21 at {JAVA_HOME_21} (brew install openjdk@21)")
    gen, classes = HERE / "jacorb" / "gen", HERE / "jacorb" / "classes"
    gen.mkdir(parents=True, exist_ok=True)
    classes.mkdir(parents=True, exist_ok=True)
    # The IDL pass first, and only then the list of files to compile: building
    # both command lines up front globs `gen` before the generator has written
    # to it, which fails on a clean tree and passes on every run after — the
    # kind of green that is about the disk rather than about the code.
    def run(cmd):
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
        if p.returncode != 0:
            raise RuntimeError((p.stderr or p.stdout).strip().replace("\n", " ")[-300:])

    run([java, "-cp", cp, "org.jacorb.idl.parser", "-d", str(gen), str(HERE / "echo.idl")])
    sources = [str(f) for f in sorted(gen.rglob("*.java"))]
    if not sources:
        raise RuntimeError(f"the JacORB IDL compiler produced nothing under {gen}")
    run(
        [javac, "-nowarn", "-cp", cp, "-d", str(classes)]
        + sources
        + [str(HERE / "jacorb" / "CodesetCaller.java")]
    )
    return f"{cp}:{classes}"


def jacorb_client_runner(extra_args):
    def run(ior, text):
        cp = build_jacorb_client()
        return subprocess.run(
            [java_tool("java"), "-cp", cp, *extra_args, "CodesetCaller", ior, text],
            capture_output=True,
            text=True,
            timeout=300,
            env={**os.environ, "JAVA_TOOL_OPTIONS": "-Dfile.encoding=UTF-8"},
        )

    return run


def measure(label, conversions, run_client, text=PROBE_TEXT):
    print(f"\n== {label}")
    sock = socket.socket()
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", 0))
    sock.listen(4)
    port = sock.getsockname()[1]
    ior = build_ior("127.0.0.1", port, conversions)
    advertised = ", ".join(NAMES.get(c, hex(c)) for c in conversions) or "(empty)"
    print(f"  we advertise: char native UTF-8; conversions: {advertised}")

    result = {}
    deadline = time.time() + 120
    t = threading.Thread(target=serve_one_call, args=(sock, result, deadline), daemon=True)
    t.start()
    try:
        p = run_client(ior, text)
    except Exception as exc:  # noqa: BLE001 — a client that will not build is a failure
        result["stop"] = True
        t.join(timeout=5)
        sock.close()
        print(f"  UNMEASURED: {exc}")
        return None
    result["stop"] = True  # the client has exited; nothing more will arrive
    t.join(timeout=5)
    sock.close()

    for line in p.stdout.strip().splitlines():
        print(f"  client: {line}")
    if p.returncode != 0:
        print(f"  client exited {p.returncode}: {p.stderr.strip()[-300:]}")

    calls = result.get("calls", [])
    if not calls:
        print(f"  UNMEASURED: no request reached the listener ({result.get('error', 'no reason')})")
        return None
    operation, contexts, arg = calls[0]
    ctx = contexts.get(SERVICE_ID_CODE_SETS)
    if ctx is None:
        print("  the client sent NO CodeSets context — §7.10.2.5 makes that ISO-8859-1")
        chosen = ISO_8859_1
    else:
        big = ctx[0] == 0
        d = In(ctx, big, pos=1)
        chosen = d.u32()
        wchosen = d.u32()
        print(
            f"  CodeSets context: char TCS {NAMES.get(chosen, hex(chosen))} "
            f"(0x{chosen:08X}), wchar TCS {NAMES.get(wchosen, hex(wchosen))} "
            f"(0x{wchosen:08X})"
        )
    print(f"  operation: {operation}")
    print(f"  argument octets for {text!r}: {arg.hex(' ')} ({len(arg)} bytes)")
    print(f"    UTF-8 would be {text.encode('utf-8').hex(' ')}")
    try:
        print(f"    ISO-8859-1 would be {text.encode('latin-1').hex(' ')}")
    except UnicodeEncodeError:
        print("    ISO-8859-1 cannot represent this text at all")
    return chosen, arg


ADVERTISEMENTS = [
    ("today: server_component_info() as it stands", []),
    ("proposed: ISO-8859-1 added to the char conversion list", [ISO_8859_1]),
]

CLIENTS = [
    ("omniORB 4.3.4 (native char ISO-8859-1)", run_omniorb_client),
    (
        "JacORB 3.9 -Djacorb.native_char_codeset=ISO8859_1",
        jacorb_client_runner(["-Djacorb.native_char_codeset=ISO8859_1"]),
    ),
    ("JacORB 3.9 default (native char UTF-8)", jacorb_client_runner([])),
]


def main():
    rows, unmeasured = [], []
    for client_label, runner in CLIENTS:
        print(f"\n########## client: {client_label}")
        for label, conversions in ADVERTISEMENTS:
            got = measure(label, conversions, runner)
            if got is None:
                unmeasured.append((client_label, label))
            else:
                rows.append((client_label, label, got[0], got[1]))

    # And the consequence, for every client the offer moved: text that the
    # codeset it moved to cannot carry. "café" survives ISO-8859-1 and is
    # therefore no test of anything; 함정 is the case that decides whether the
    # downgrade is a cosmetic difference or a data-loss one.
    korean = []
    for client_label, runner in CLIENTS:
        chosen = {r[2] for r in rows if r[0] == client_label}
        if len(chosen) < 2:
            continue
        print(f"\n########## consequence for {client_label}: Korean over the offered codeset")
        got = measure(
            "proposed advertisement, Korean argument",
            [ISO_8859_1],
            runner,
            text=KOREAN_TEXT,
        )
        if got is None:
            unmeasured.append((client_label, "Korean over the proposed advertisement"))
        else:
            korean.append((client_label, got[0], got[1]))

    print("\n-- summary")
    for client_label, label, chosen, arg in rows:
        print(
            f"  {client_label}\n    {label}\n      transmits "
            f"{NAMES.get(chosen, hex(chosen))}, octets {arg.hex(' ')}"
        )
    for client_label, chosen, arg in korean:
        print(
            f"  {client_label}\n    {KOREAN_TEXT!r} under the proposed advertisement\n"
            f"      transmits {NAMES.get(chosen, hex(chosen))}, octets {arg.hex(' ')}\n"
            f"      UTF-8 would have been {KOREAN_TEXT.encode('utf-8').hex(' ')}"
        )
    for client_label, label in unmeasured:
        print(f"  UNMEASURED: {client_label} / {label}")

    changed = [
        c
        for c in {r[0] for r in rows}
        if len({r[2] for r in rows if r[0] == c}) > 1
    ]
    if changed:
        print(
            "\nThe advertisement CHANGED what an unmodified peer transmits: "
            + ", ".join(sorted(changed))
            + ". A conversion list is not a free addition — §7.10.2.6 lets the "
            "client take our offer over its own native fallback, so offering a "
            "narrower codeset lowers what an existing peer sends."
        )
        for client_label, chosen, arg in korean:
            if arg == KOREAN_TEXT.encode("utf-8"):
                continue
            print(
                f"  and {client_label} did not refuse the text the offered "
                "codeset cannot carry. It truncated each character to one "
                f"octet — {arg.hex(' ')}, which reads back as "
                f"{arg.decode('latin-1')!r} — and raised nothing. The loss is "
                "silent and reaches the servant looking like data, which is "
                "the failure mode this project's own converter refuses by "
                "returning Untranslatable."
            )
    else:
        print(
            "\nNo measured client changed what it transmits when the "
            "ISO-8859-1 conversion was offered. Advertising it would therefore "
            "have no observable effect on any peer available here — the peer "
            "that needs it is the one D009 §8 asks for and this machine cannot "
            "produce."
        )
    return 2 if unmeasured else 0


if __name__ == "__main__":
    leave(main())
