#!/usr/bin/env python3
"""Our side of spikes/wide.idl at GIOP 1.1, hand-built, so the single wide
character can meet a peer in both directions and every octet is on record.

Used by spikes/jacorb_wchar11.sh (D010 B5, second half). Two roles:

  server  — publishes an IIOP 1.1 IOR for IDL:spike/Wide:1.0 (TAG_CODE_SETS:
            char UTF-8, wchar UTF-16, as our own server advertises), answers
            _is_a, echo_wchar and echo_wstring, and writes every request and
            reply it saw to a log — version and byte order from the header,
            the wchar's two octets from the body. The reply's byte order and
            the order the echoed unit is written in are command-line choices,
            because that is the half of the measurement JacORB's own client
            cannot supply: it writes big-endian only, so what its *reader*
            does with a little-endian 1.1 message is only visible if somebody
            sends it one.

  client  — dials a server's IOR at GIOP 1.1 in a chosen byte order, sends a
            CodeSets context (UTF-8, UTF-16) on the first request as our
            client does, then one echo_wchar per case with the unit written
            in the message's order — our writer's convention at 1.1 — or
            big-endian on request (the control), and records what came back:
            the reply's version and order from its header, the two octets,
            and the code unit they are.

  recorded — prints a `const NAME: &[u8]` from a Rust test file as hex, so the
            live bytes can be compared to the recording (the pattern of
            spikes/wide_char_capture.py).

The wchar itself is written and read the way crates/orbweaver-giop/src/
codeset.rs writes and reads it at 1.1 — two octets, no length indication, no
mark, in the message's byte order — which is the convention under test; the
Rust tests in crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs pin our
codec to the very octets this fixture sent and received.

TEST FIXTURE ONLY. Nothing here is linked into Orbweaver; JacORB on the other
end is a separate process speaking the published GIOP wire.

Every wait is a blocking socket call with a deadline; nothing spins.

*wide.idl의 우리 쪽을 손으로 만든 GIOP 1.1로 세운다. 서버는 응답의 바이트
순서를 고를 수 있어서, JacORB가 스스로는 만들지 않는 리틀엔디언 1.1 메시지를
JacORB의 리더 앞에 놓을 수 있다.*
"""

import argparse
import os
import re
import socket
import struct
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from jacorb_giop11_tap import (  # noqa: E402 — the tap's CDR helpers, reused
    In,
    Out,
    codesets_context,
    hexdump,
    read_contexts,
)

TYPE_ID = "IDL:spike/Wide:1.0"
OBJECT_KEY = b"OrbweaverWide"
TAG_INTERNET_IOP = 0
TAG_CODE_SETS = 1
SERVICE_ID_CODE_SETS = 1
UTF_8 = 0x05010001
UTF_16 = 0x00010109
BAD_OPERATION = "IDL:omg.org/CORBA/BAD_OPERATION:1.0"


def unit_octets(unit, order):
    return struct.pack(">H" if order == "be" else "<H", unit)


def unit_of(octets, order):
    return struct.unpack(">H" if order == "be" else "<H", octets)[0]


# ── the IOR our server publishes ─────────────────────────────────────────────


def build_ior(host, port):
    """IIOP 1.1, one TAG_CODE_SETS component (char native UTF-8, wchar native
    UTF-16, no conversions), little-endian encapsulations as Ior::to_stringified
    writes them."""

    def codesets(c):
        c.u32(UTF_8)
        c.u32(0)
        c.u32(UTF_16)
        c.u32(0)

    def profile(p):
        p.u8(1)
        p.u8(1)
        p.string(host)
        p.u16(port)
        p.sequence(OBJECT_KEY)
        p.u32(1)
        p.u32(TAG_CODE_SETS)
        p.encapsulation(codesets)

    e = Out(big=False)
    e.raw(b"\1")
    e.string(TYPE_ID)
    e.u32(1)
    e.u32(TAG_INTERNET_IOP)
    e.encapsulation(profile)
    return "IOR:" + bytes(e.buf).hex()


def parse_ior(text):
    """type_id, (major, minor), host, port, key of the first IIOP profile."""
    raw = bytes.fromhex(text[4:].strip())
    outer = In(raw, big=(raw[0] == 0), pos=1)
    type_id = outer.string()
    n = outer.u32()
    for _ in range(n):
        tag = outer.u32()
        body = outer.sequence()
        if tag != TAG_INTERNET_IOP:
            continue
        p = In(body, big=(body[0] == 0), pos=1)
        major, minor = p.u8(), p.u8()
        host, port = p.string(), p.u16()
        key = p.sequence()
        return type_id, (major, minor), host, port, key
    raise ValueError("no IIOP profile")


# ── framing ──────────────────────────────────────────────────────────────────


def recv_exact(sock, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            return None
        buf += chunk
    return bytes(buf)


def read_message(sock):
    head = recv_exact(sock, 12)
    if head is None:
        return None
    if head[:4] != b"GIOP":
        raise ValueError(f"not GIOP: {head.hex()}")
    big = (head[6] & 1) == 0
    size = struct.unpack(">I" if big else "<I", head[8:12])[0]
    body = recv_exact(sock, size)
    if body is None:
        return None
    return head + body


def order_of(msg):
    return "be" if (msg[6] & 1) == 0 else "le"


def giop_header(o, minor, mtype):
    o.raw(b"GIOP")
    o.u8(1)
    o.u8(minor)
    o.u8(0 if o.big else 1)
    o.u8(mtype)
    o.u32(0)  # size, patched


def finish_message(o):
    size = len(o.buf) - 12
    o.buf[8:12] = struct.pack(">I" if o.big else "<I", size)
    return bytes(o.buf)


# ── server ───────────────────────────────────────────────────────────────────


def wchar_1_1(d, order):
    """The two octets of a 1.1 wchar at the decoder's position, and the unit
    they are in `order`; alignment 2 as for an unsigned short."""
    d.align(2)
    octets = d.octets(2)
    return octets, unit_of(octets, order)


def serve_connection(conn, no, opts, log):
    try:
        while True:
            msg = read_message(conn)
            if msg is None:
                break
            minor = msg[5]
            order = order_of(msg)
            big = order == "be"
            mtype = msg[7]
            d = In(msg, big, pos=12)
            if mtype == 3:  # LocateRequest: 1.0/1.1 request_id, key
                rid = d.u32()
                o = Out(big)
                giop_header(o, minor, 4)
                o.u32(rid)
                o.u32(1)  # OBJECT_HERE
                conn.sendall(finish_message(o))
                log(f"[{no}] C->S GIOP 1.{minor} LocateRequest {order.upper()} id={rid} -> OBJECT_HERE")
                continue
            if mtype != 0:
                log(f"[{no}] C->S GIOP 1.{minor} type{mtype} {order.upper()}: not handled, closing")
                break
            if minor >= 2:
                rid = d.u32()
                d.u8()
                d.octets(3)
                if d.u16() != 0:
                    raise ValueError("target disposition")
                d.sequence()
                op = d.string()
                ctx = codesets_context(read_contexts(d))
                if d.remaining():
                    d.align(8)
            else:
                ctx = codesets_context(read_contexts(d))
                rid = d.u32()
                d.u8()
                if minor == 1:
                    d.octets(3)
                d.sequence()
                op = d.string()
                d.sequence()  # requesting_principal
            line = f"[{no}] C->S GIOP 1.{minor} Request {order.upper()} id={rid} op={op}{ctx}"

            reply_order = order if opts.reply_order == "follow" else opts.reply_order
            rbig = reply_order == "be"
            unit_order = reply_order if opts.unit_order == "message" else "be"
            o = Out(rbig)
            giop_header(o, minor, 1)
            if minor >= 2:
                o.u32(rid)
                o.u32(0)
                o.u32(0)  # no contexts
                o.align(8)
            else:
                o.u32(0)  # no contexts
                o.u32(rid)
                o.u32(0)  # NO_EXCEPTION, patched below for BAD_OPERATION
            status_pos = len(o.buf) - 4

            if op == "_is_a":
                asked = d.string()
                o.u8(1 if asked == TYPE_ID else 0)
                line += f" _is_a({asked})"
            elif op == "_non_existent":
                o.u8(0)
            elif op == "echo_wchar":
                if minor >= 2:
                    n = d.u8()
                    octets = d.octets(n)
                    unit = unit_of(octets[-2:], "be")
                    o.u8(2)
                    o.raw(unit_octets(unit, "be"))
                    line += f"\n    request body: wchar 1.2 octets={n} body={octets.hex()} -> U+{unit:04X}"
                else:
                    octets, unit = wchar_1_1(d, order)
                    trailing = d.remaining()
                    out = unit_octets(unit, unit_order)
                    o.align(2)
                    o.raw(out)
                    line += (
                        f"\n    request body: wchar 1.1 body={octets.hex()} read in the message's order"
                        f" ({order.upper()}) -> U+{unit:04X}"
                        + (f", {trailing} octet(s) after it" if trailing else "")
                    )
                    line += (
                        f"\n    reply body: wchar 1.1 body={out.hex()} in a {reply_order.upper()} message,"
                        f" unit written {'big-endian' if unit_order == 'be' else 'little-endian'}"
                    )
            elif op == "echo_wstring" and minor < 2:
                count = d.u32()
                raw = d.octets(2 * count)
                units = [unit_of(raw[i : i + 2], order) for i in range(0, len(raw), 2)]
                o.u32(count)
                for u in units:
                    o.align(2)
                    o.raw(unit_octets(u, unit_order))
                line += f"\n    request body: wstring 1.1 count={count} body={raw.hex()}"
            else:
                o.buf[status_pos : status_pos + 4] = struct.pack(">I" if rbig else "<I", 2)
                o.string(BAD_OPERATION)
                o.u32(0)
                o.u32(0)
                line += " -> BAD_OPERATION"
            reply = finish_message(o)
            conn.sendall(reply)
            log(line + "\n" + hexdump(msg))
            log(f"[{no}] S->C GIOP 1.{minor} Reply {reply_order.upper()} id={rid} for={op}\n" + hexdump(reply))
    except (OSError, ValueError) as exc:
        log(f"[{no}] connection ended: {exc}")
    finally:
        conn.close()


def run_server(opts):
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    port = listener.getsockname()[1]
    logf = open(opts.log, "a", buffering=1)
    lock = threading.Lock()

    def log(line):
        with lock:
            logf.write(line + "\n")

    log(
        f"wide server on 127.0.0.1:{port}, IIOP 1.1, reply order {opts.reply_order},"
        f" unit order {opts.unit_order}"
    )
    with open(opts.out, "w") as f:
        f.write(build_ior("127.0.0.1", port) + "\n")
    print(f"READY {port}", flush=True)
    no = 0
    while True:
        conn, _ = listener.accept()
        no += 1
        threading.Thread(target=serve_connection, args=(conn, no, opts, log), daemon=True).start()


# ── client ───────────────────────────────────────────────────────────────────


class Case:
    def __init__(self, name, op, octets_of, describe):
        self.name, self.op, self.octets_of, self.describe = name, op, octets_of, describe


def cases_from(names):
    out = []
    for name in names:
        if name == "pair":
            # A surrogate pair offered as ONE wchar: four octets where the
            # reader expects two. Behaviour, not a pass — nothing can carry
            # U+1F600 in one UTF-16 unit.
            out.append(
                Case(
                    "pair",
                    "echo_wchar",
                    lambda order: unit_octets(0xD83D, order) + unit_octets(0xDE00, order),
                    "U+1F600 as d83d de00 in one wchar",
                )
            )
        elif name == "wstring-feff":
            # U+FEFF as the first *character* of a wstring: JacORB reads no
            # mark at 1.1; our reader strips one (a decision recorded in
            # codeset.rs). Behaviour, recorded.
            def ws(order):
                o = Out(order == "be")
                o.u32(3)
                for u in (0xFEFF, 0x0078, 0x0000):
                    o.raw(unit_octets(u, order))
                return bytes(o.buf)

            out.append(Case("wstring-feff", "echo_wstring", ws, "wstring FEFF 0078 + terminator"))
        else:
            unit = int(name, 16)
            out.append(
                Case(name, "echo_wchar", lambda order, u=unit: unit_octets(u, order), f"U+{unit:04X}")
            )
    return out


def run_client(opts):
    with open(opts.ior) as f:
        text = f.read().strip()
    type_id, (major, minor), host, port, key = parse_ior(text)
    logf = open(opts.log, "a", buffering=1)

    def log(line):
        logf.write(line + "\n")

    print(f"  info profile IIOP {major}.{minor} at {host}:{port}, type {type_id}")
    log(f"client -> {host}:{port} (profile IIOP {major}.{minor}), requests GIOP 1.1 {opts.order.upper()}")
    if (major, minor) != (1, 1):
        print(f"  FAIL the server's profile says IIOP {major}.{minor}, not 1.1: our client would not speak 1.1 to it")
        return 1

    big = opts.order == "be"
    unit_order = opts.order if opts.unit_order == "message" else "be"
    sock = socket.create_connection((host, port), timeout=10)
    fails = 0
    rid = 0
    first = True
    for case in cases_from(opts.cases):
        rid += 2
        o = Out(big)
        giop_header(o, 1, 0)
        if first:
            # The CodeSets context, on the first request only, as our
            # Connection sends it: char UTF-8, wchar UTF-16.
            o.u32(1)
            o.u32(SERVICE_ID_CODE_SETS)

            def ctx(c):
                c.u32(UTF_8)
                c.u32(UTF_16)

            o.encapsulation(ctx)
            first = False
        else:
            o.u32(0)
        o.u32(rid)
        o.u8(1)  # response_expected
        o.raw(b"\0\0\0")  # reserved (1.1)
        o.sequence(key)
        o.string(case.op)
        o.u32(0)  # requesting_principal
        sent = case.octets_of(unit_order)
        o.align(2 if case.op == "echo_wchar" else 4)
        o.raw(sent)
        req = finish_message(o)
        sock.sendall(req)
        log(f"C->S GIOP 1.1 Request {opts.order.upper()} id={rid} op={case.op} ({case.describe})"
            f"\n    request body: {sent.hex()} unit order {unit_order}\n" + hexdump(req))
        reply = read_message(sock)
        if reply is None:
            print(f"  FAIL {case.name}: the server closed without a reply")
            fails += 1
            log("S->C connection closed")
            break
        rminor = reply[5]
        rorder = order_of(reply)
        d = In(reply, rorder == "be", pos=12)
        read_contexts(d)
        got_id = d.u32()
        status = d.u32()
        log(f"S->C GIOP 1.{rminor} Reply {rorder.upper()} id={got_id} status={status}\n" + hexdump(reply))
        if got_id != rid or status != 0:
            exc = ""
            if status == 2:
                try:
                    exc = f" {d.string()} minor=0x{d.u32():x}"
                except Exception:  # noqa: BLE001
                    pass
            print(f"  FAIL {case.name} ({case.describe}): reply status={status}{exc}, GIOP 1.{rminor} {rorder.upper()}")
            fails += 1
            continue
        if case.op == "echo_wchar":
            d.align(2)
            body = d.octets(2)
            unit = unit_of(body, rorder)
            rest = d.remaining()
            note = f"reply GIOP 1.{rminor} {rorder.upper()} body={body.hex()} -> U+{unit:04X}" + (
                f" (+{rest} octet(s))" if rest else ""
            )
            if case.name in ("pair",):
                print(f"  info {case.name}: sent {sent.hex()} ({case.describe}); {note}")
            else:
                want = int(case.name, 16)
                verdict = "ok  " if unit == want else "FAIL"
                if unit != want:
                    fails += 1
                print(f"  {verdict} {case.name}: sent {sent.hex()} in a {opts.order.upper()} message; {note}")
        else:
            count = d.u32()
            body = d.octets(2 * count)
            units = " ".join(f"U+{unit_of(body[i:i + 2], rorder):04X}" for i in range(0, len(body), 2))
            print(f"  info {case.name}: sent {sent.hex()} ({case.describe});"
                  f" reply GIOP 1.{rminor} {rorder.upper()} count={count} body={body.hex()} -> {units}")
        if rminor != 1:
            print(f"  FAIL {case.name}: the reply came back at GIOP 1.{rminor}, not 1.1")
            fails += 1
    sock.close()
    return 1 if fails else 0


# ── the recording ────────────────────────────────────────────────────────────


def run_recorded(opts):
    text = open(opts.rs).read()
    m = re.search(r"const %s: &\[u8\] = *\n? *&\[(.*?)\];" % re.escape(opts.name), text, re.S)
    if not m:
        print(f"no const {opts.name} in {opts.rs}")
        return 1
    print("".join(re.findall(r"0x([0-9a-fA-F]{2})", m.group(1))).lower())
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="role", required=True)
    s = sub.add_parser("server")
    s.add_argument("--out", required=True)
    s.add_argument("--log", required=True)
    s.add_argument("--reply-order", choices=["be", "le", "follow"], default="follow")
    s.add_argument("--unit-order", choices=["message", "big"], default="message")
    c = sub.add_parser("client")
    c.add_argument("--ior", required=True)
    c.add_argument("--log", required=True)
    c.add_argument("--order", choices=["be", "le"], default="be")
    c.add_argument("--unit-order", choices=["message", "big"], default="message")
    c.add_argument("cases", nargs="+", help="hex UTF-16 units, or 'pair', or 'wstring-feff'")
    r = sub.add_parser("recorded")
    r.add_argument("--rs", required=True)
    r.add_argument("--name", required=True)
    opts = ap.parse_args()
    if opts.role == "server":
        return run_server(opts)
    if opts.role == "client":
        return run_client(opts)
    return run_recorded(opts)


if __name__ == "__main__":
    sys.exit(main())
