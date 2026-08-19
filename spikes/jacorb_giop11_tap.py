#!/usr/bin/env python3
"""A recording tap between a GIOP client and server, so a peer's bytes can be
kept with provenance instead of inferred from what came back.

Used by spikes/jacorb_giop11.sh (D010 B5). It publishes a copy of a server's
IOR whose IIOP profile points at the tap — and, on request, whose profile
*version* is lowered — then relays every connection to the real server while
writing one line per GIOP message to a log: direction, version, type, request
id, operation, and the CodeSets service context when one is carried. Messages
for the operation of interest are dumped in full, and their wide-string bodies
are shown under the length rule of the version they arrived in.

The tap changes nothing in flight. Its whole point is that the version and the
codeset choice come from the ORBs, and the log is what they did — a JacORB
that ignored a property, or a reader that misapplied a version rule, shows up
here as bytes rather than as a green line somebody believed.

Only the IIOP profile's host, port and (optionally) version are rewritten;
every component, including TAG_CODE_SETS, is copied byte for byte, so the peer
still negotiates against what the real server advertises. Rewriting the version
is how a peer whose outbound version follows the profile is made to speak 1.1
to a server that publishes 1.2.

TEST FIXTURE ONLY. Nothing here is linked into Orbweaver; the ORBs on either
side are separate processes speaking the published GIOP wire.

    python3 spikes/jacorb_giop11_tap.py --ior server.ior --out tapped.ior \\
        --log tap.log [--minor 1] [--op echo_wstring] [--host 127.0.0.1]

Prints "READY <port>" once the rewritten IOR is on disk and the listener is
accepting; runs until killed. Every wait is a blocking socket call — nothing
here spins.

*피어의 바이트를 결과에서 역추론하지 않고 출처와 함께 기록하기 위한 탭이다.
프로파일의 host/port/버전만 바꾸고 컴포넌트는 그대로 복사한다.*
"""

import argparse
import socket
import struct
import sys
import threading

TAG_INTERNET_IOP = 0
SERVICE_ID_CODE_SETS = 1
MSG_TYPES = {
    0: "Request",
    1: "Reply",
    2: "CancelRequest",
    3: "LocateRequest",
    4: "LocateReply",
    5: "CloseConnection",
    6: "MessageError",
    7: "Fragment",
}
CODESET_NAMES = {
    0x00010001: "ISO-8859-1",
    0x05010001: "UTF-8",
    0x00010109: "UTF-16",
    0x00010100: "UCS-2",
    0x00010104: "UCS-4",
    0x00010101: "UCS-2-L1",
    0x00010102: "UCS-2-L2",
}


# ── CDR, from the first byte of whatever is being written or read ────────────


class Out:
    """Alignment origin is byte 0 of this buffer: a message, or an
    encapsulation, which restarts at its own first byte and gets its own Out."""

    def __init__(self, big=True):
        self.buf, self.big = bytearray(), big

    def align(self, n):
        while len(self.buf) % n:
            self.buf.append(0)

    def raw(self, b):
        self.buf += b

    def u8(self, v):
        self.buf.append(v)

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
            raise ValueError("truncated")
        self.pos += n
        return bytes(b)

    def u8(self):
        return self.octets(1)[0]

    def u16(self):
        self.align(2)
        return struct.unpack(">H" if self.big else "<H", self.octets(2))[0]

    def u32(self):
        self.align(4)
        return struct.unpack(">I" if self.big else "<I", self.octets(4))[0]

    def string(self):
        n = self.u32()
        return self.octets(n)[:-1].decode("latin-1")

    def sequence(self):
        return self.octets(self.u32())

    def remaining(self):
        return len(self.buf) - self.pos


# ── IOR rewrite ──────────────────────────────────────────────────────────────


def rewrite_ior(text, host, port, minor):
    """Re-emits `IOR:<hex>` with the first IIOP profile's host/port replaced,
    and its version minor replaced when `minor` is not None. Returns the new
    stringified IOR and a description of what the original profile said."""
    if not text.startswith("IOR:"):
        raise ValueError("not a stringified IOR")
    raw = bytes.fromhex(text[4:].strip())
    outer = In(raw, big=(raw[0] == 0), pos=1)
    type_id = outer.string()
    n = outer.u32()
    profiles = []
    for _ in range(n):
        tag = outer.u32()
        profiles.append((tag, outer.sequence()))

    e = Out(big=True)
    e.raw(b"\0")
    e.string(type_id)
    e.u32(len(profiles))
    said = None
    for tag, body in profiles:
        if tag != TAG_INTERNET_IOP or said is not None:
            e.u32(tag)
            e.sequence(body)  # verbatim: an encapsulation carries its own order
            continue
        p = In(body, big=(body[0] == 0), pos=1)
        major, old_minor = p.u8(), p.u8()
        old_host, old_port = p.string(), p.u16()
        key = p.sequence()
        components = []
        if old_minor >= 1:
            for _ in range(p.u32()):
                ctag = p.u32()
                components.append((ctag, p.sequence()))
        new_minor = old_minor if minor is None else minor
        said = (
            f"IIOP {major}.{old_minor} at {old_host}:{old_port}, "
            f"{len(key)}-byte key, {len(components)} component(s)"
            + ("" if minor is None else f"; republished as IIOP {major}.{new_minor}")
        )

        def profile(q):
            q.u8(major)
            q.u8(new_minor)
            q.string(host)
            q.u16(port)
            q.sequence(key)
            if new_minor >= 1:
                q.u32(len(components))
                for ctag, cbody in components:
                    q.u32(ctag)
                    q.sequence(cbody)

        e.u32(TAG_INTERNET_IOP)
        e.encapsulation(profile)
    if said is None:
        raise ValueError("no IIOP profile in the IOR")
    return "IOR:" + bytes(e.buf).hex(), said


# ── GIOP framing and the parts of a header worth logging ─────────────────────


def hexdump(b):
    lines = []
    for i in range(0, len(b), 16):
        chunk = b[i : i + 16]
        hx = " ".join(f"{x:02x}" for x in chunk)
        asc = "".join(chr(x) if 32 <= x < 127 else "." for x in chunk)
        lines.append(f"    {i:04x}  {hx:<47}  {asc}")
    return "\n".join(lines)


def codesets_context(contexts):
    for cid, body in contexts:
        if cid == SERVICE_ID_CODE_SETS and len(body) >= 9:
            c = In(body, big=(body[0] == 0), pos=1)
            char_cs, wchar_cs = c.u32(), c.u32()
            return (
                f" codesets(char={CODESET_NAMES.get(char_cs, hex(char_cs))}"
                f" wchar={CODESET_NAMES.get(wchar_cs, hex(wchar_cs))})"
            )
    return ""


def read_contexts(d):
    return [(d.u32(), d.sequence()) for _ in range(d.u32())]


def wstring_at(d, minor):
    """Describes the wide string at the decoder's position under the rule of
    the message's version: 1.1 counts wide characters including a terminator,
    1.2 counts octets. Returns text for the log."""
    n = d.u32()
    if minor >= 2:
        body = d.octets(n)
        return f"wstring 1.2 octets={n} body={body.hex()}"
    body = d.octets(2 * n)
    return f"wstring 1.1 count={n} (wide chars incl. terminator) body={body.hex()}"


def describe(direction, msg, state, op_of_interest, log):
    """One log line per message; the full dump for the operation of interest.
    Anything this cannot parse is logged as such rather than dropped."""
    major, minor = msg[4], msg[5]
    flags = msg[6]
    big = (flags & 1) == 0
    mtype = msg[7]
    name = MSG_TYPES.get(mtype, f"type{mtype}")
    d = In(msg, big, pos=12)
    line = f"{direction} GIOP {major}.{minor} {name} size={len(msg) - 12} {'BE' if big else 'LE'}"
    dump = False
    try:
        if mtype == 0:  # Request
            if minor >= 2:
                rid = d.u32()
                d.u8()  # response flags
                d.octets(3)
                disposition = d.u16()
                if disposition == 0:
                    d.sequence()  # object key
                else:
                    raise ValueError(f"target disposition {disposition}")
                op = d.string()
                ctx = codesets_context(read_contexts(d))
                if d.remaining():
                    d.align(8)
            else:
                ctx = codesets_context(read_contexts(d))
                rid = d.u32()
                d.u8()  # response_expected
                if minor == 1:
                    d.octets(3)
                d.sequence()  # object key
                op = d.string()
                d.sequence()  # requesting_principal
            state["ops"][rid] = (op, minor)
            line += f" id={rid} op={op}{ctx}"
            if op == op_of_interest:
                dump = True
                try:
                    line += "\n    request body: " + wstring_at(d, minor)
                except Exception as exc:  # noqa: BLE001 — logged, never fatal
                    line += f"\n    request body: not a wstring ({exc})"
        elif mtype == 1:  # Reply
            if minor >= 2:
                rid = d.u32()
                status = d.u32()
                read_contexts(d)
                if d.remaining():
                    d.align(8)
            else:
                read_contexts(d)
                rid = d.u32()
                status = d.u32()
            op, _ = state["ops"].get(rid, ("?", minor))
            line += f" id={rid} status={status} for={op}"
            if op == op_of_interest:
                dump = True
                if status == 0:
                    try:
                        line += "\n    reply body: " + wstring_at(d, minor)
                    except Exception as exc:  # noqa: BLE001
                        line += f"\n    reply body: not a wstring ({exc})"
        elif mtype == 7 and minor >= 2:
            line += f" id={d.u32()}"
    except Exception as exc:  # noqa: BLE001
        line += f" (header not parsed: {exc})"
    if dump:
        line += "\n" + hexdump(msg)
    log(line)


def relay(src, dst, direction, state, op_of_interest, log):
    """Forwards bytes as they arrive and parses GIOP frames from a side buffer.
    Forwarding does not wait for a full message, so the tap adds no
    message-level latency and cannot deadlock a peer that writes a header and
    then waits for something."""
    buf = bytearray()
    try:
        while True:
            chunk = src.recv(65536)
            if not chunk:
                break
            dst.sendall(chunk)
            buf += chunk
            while len(buf) >= 12:
                if buf[:4] != b"GIOP":
                    log(f"{direction} not GIOP: {bytes(buf[:12]).hex()}")
                    buf.clear()
                    break
                big = (buf[6] & 1) == 0
                size = struct.unpack(">I" if big else "<I", buf[8:12])[0]
                if len(buf) < 12 + size:
                    break
                msg = bytes(buf[: 12 + size])
                del buf[: 12 + size]
                describe(direction, msg, state, op_of_interest, log)
    except OSError:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def serve(listener, target, op_of_interest, log):
    conn_no = 0
    while True:
        client, _ = listener.accept()
        conn_no += 1
        try:
            server = socket.create_connection(target, timeout=10)
            server.settimeout(None)
        except OSError as exc:
            log(f"conn {conn_no}: cannot reach {target[0]}:{target[1]}: {exc}")
            client.close()
            continue
        log(f"conn {conn_no}: open")
        state = {"ops": {}}
        tag = f"[{conn_no}]"
        threading.Thread(
            target=relay, args=(client, server, f"{tag} C->S", state, op_of_interest, log), daemon=True
        ).start()
        threading.Thread(
            target=relay, args=(server, client, f"{tag} S->C", state, op_of_interest, log), daemon=True
        ).start()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--ior", required=True, help="the real server's stringified IOR file")
    ap.add_argument("--out", required=True, help="where to write the tapped IOR")
    ap.add_argument("--log", required=True, help="one line per GIOP message goes here")
    ap.add_argument("--minor", type=int, default=None, help="republish the profile at IIOP 1.<minor>")
    ap.add_argument("--op", default="echo_wstring", help="operation to dump in full")
    ap.add_argument("--host", default="127.0.0.1", help="host to publish in the tapped IOR")
    args = ap.parse_args()

    with open(args.ior) as f:
        text = f.read().strip()
    raw = bytes.fromhex(text[4:])
    outer = In(raw, big=(raw[0] == 0), pos=1)
    outer.string()
    outer.u32()
    outer.u32()
    body = outer.sequence()
    p = In(body, big=(body[0] == 0), pos=1)
    p.u8()
    p.u8()
    target = (p.string(), p.u16())

    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    port = listener.getsockname()[1]

    new_ior, said = rewrite_ior(text, args.host, port, args.minor)
    with open(args.out, "w") as f:
        f.write(new_ior + "\n")

    logf = open(args.log, "a", buffering=1)
    lock = threading.Lock()

    def log(line):
        with lock:
            logf.write(line + "\n")

    log(f"tap on 127.0.0.1:{port} -> {target[0]}:{target[1]}; original profile: {said}")
    print(f"READY {port}", flush=True)
    serve(listener, target, args.op, log)


if __name__ == "__main__":
    sys.exit(main())
