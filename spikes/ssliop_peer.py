#!/usr/bin/env python3
"""An SSLIOP peer: a socket that completes a TLS handshake and then speaks GIOP.

`docs/decisions/D010` §4 B3 files SSLIOP peer proof as class B — buildable,
oracle absent — because "brew's omniORBpy ships no `sslTP`" and JacORB's SSL
transport is not configured. `spikes/tls/PEER-STATUS.md` records that probe and
names three unblock paths, all three of which install an ORB.

The premise is true and the conclusion does not follow, in the same way it did
not for B5. **SSLIOP is not a protocol an ORB implements on top of IIOP.** The
OMG Security Service's SSLIOP chapter defines exactly two things: unmodified
GIOP/IIOP messages carried over a TLS connection, and a `TAG_SSL_SEC_TRANS`
component in the IOR saying where that TLS listener is. There is no SSLIOP
handshake, no negotiation, no framing of its own. So the peer this needs is a
TLS socket that writes GIOP by hand — and Python's `ssl` is in the standard
library, while `spikes/tls/` has carried the certificates since 2026-08-13.

What an ORB peer would still add, and this file honestly does not: a
`TAG_SSL_SEC_TRANS` component produced by *omniORB's or JacORB's own* encoder,
with whatever association-option bits and port conventions that implementation
chooses. That residue is a claim about their encoder and only they can make it.
Everything else B3 names — the handshake against a foreign TLS stack, in
another process; GIOP crossing it; the advertisement read out of an IOR our
encoder did not write; the refusals — is reachable from here.

*B3의 전제는 참이고 결론은 따라 나오지 않는다. SSLIOP은 ORB가 구현하는 별도
프로토콜이 아니라 TLS 위의 GIOP과 IOR의 `TAG_SSL_SEC_TRANS` 컴포넌트가 전부다.
필요한 피어는 ORB가 아니라 손으로 GIOP을 쓰는 TLS 소켓이다. 다만 실제 ORB의
인코더가 만든 컴포넌트만은 그 구현체만이 만들 수 있으므로 남는다.*

What it does, in order:

  1. binds a listener, and builds a **stringified IOR by hand** — every octet
     from §7.6.9 and §9.7.2, including the `TAG_SSL_SEC_TRANS` encapsulation
     from the Security Service's SSLIOP chapter — then publishes it;
  2. accepts once, and (unless ``--transport plain``) completes a TLS
     handshake with ``spikes/tls/server.pem`` + ``server.key``;
  3. reads ``--requests`` whole GIOP ``Request`` messages, parsed by hand;
  4. answers each with a ``Reply`` whose byte order is ``--reply-endian``,
     chosen independently of the request's — GIOP sets the order per message
     and our ``Connection`` always writes its own native order, so a peer that
     echoed the request would leave one of the two orders unmeasured on any
     one machine. *두 바이트 순서 모두.*

The IOR's byte order (``--ior-endian``) and the component encapsulation's
(``--component-endian``) are independent axes too: an encapsulation restarts
alignment and carries its own order octet, so a component written
little-endian inside a big-endian IOR is a shape a real deployment produces
and our own encoder never does.

TEST FIXTURE ONLY, and deliberately not an ORB: nothing is imported beyond the
standard library, every GIOP and IOR octet is built here from the published
specification, and nothing in this file is linked into Orbweaver. The point of
writing it by hand is that bytes produced by the encoder under test cannot
agree with it by construction.

    python3 spikes/ssliop_peer.py --port-file p.txt --ior-file e.ior \\
        [--transport tls|plain] [--advertise ssl-only|same-port|elsewhere|
         none|unreadable] [--ior-endian big|little]
        [--component-endian big|little] [--reply-endian big|little]
        [--requests 1] [--deadline-s 20]

Prints one JSON object on stdout when the script has run to the end, reporting
**what it observed** — whether anyone connected, whether the handshake
completed, what the first octets were when it was not GIOP, whether the object
key the caller used is the one it published, and what it served. It does not
decide whether that was right: the runner knows the trust configuration and
judges. Two processes, separately, or the claim is the client agreeing with
itself. **The exit code is the verdict** on whether this script ran to the end.

*손으로 GIOP을 쓰는 TLS 소켓. 관찰한 것을 JSON으로 보고하고 판정은 러너가 한다.
종료 코드가 이 스크립트의 판정이다.*
"""

import argparse
import json
import os
import socket
import ssl
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TLS_DIR = os.path.join(HERE, "tls")

MAGIC = b"GIOP"
HEADER_LEN = 12
MSG_REQUEST = 0
MSG_REPLY = 1
LITTLE_ENDIAN_FLAG = 0b01
MORE_FRAGMENTS = 0b10

TAG_INTERNET_IOP = 0
TAG_SSL_SEC_TRANS = 20

# `Security::AssociationOptions`, the transport-capable bits only. Named here
# rather than imported: this file may not depend on the crate under test.
INTEGRITY = 0x0002
CONFIDENTIALITY = 0x0004
ESTABLISH_TRUST_IN_TARGET = 0x0020

TYPE_ID = "IDL:orbweaver/SslEcho:1.0"
OBJECT_KEY = b"ssliop-echo"


# ── CDR, by hand ─────────────────────────────────────────────────────────────
# Alignment origin is the first octet of the enclosing thing: an encapsulation
# starts its own, a GIOP message starts one at the first octet of its 12-byte
# header. That distinction has its own line in CLAUDE.md; both are honoured by
# giving each buffer its own writer whose offset zero *is* the origin.


class Writer:
    """A CDR stream whose offset zero is its alignment origin."""

    def __init__(self, little, prefix=b""):
        self.little = little
        self.buf = bytearray(prefix)

    def align(self, n):
        while len(self.buf) % n:
            self.buf.append(0)

    def u8(self, v):
        self.buf.append(v & 0xFF)

    def raw(self, b):
        self.buf += b

    def u16(self, v):
        self.align(2)
        self.buf += struct.pack("<H" if self.little else ">H", v)

    def u32(self, v):
        self.align(4)
        self.buf += struct.pack("<I" if self.little else ">I", v)

    def i32(self, v):
        self.align(4)
        self.buf += struct.pack("<i" if self.little else ">i", v)

    def string(self, s):
        b = s.encode("utf-8") + b"\x00"
        self.u32(len(b))  # §9.3.2.7: the length counts the NUL
        self.buf += b

    def octets(self, b):
        self.u32(len(b))
        self.buf += b

    def bytes(self):
        return bytes(self.buf)


def encapsulation(little):
    """A CDR encapsulation: its byte-order octet is offset zero (§9.3.3)."""
    return Writer(little, bytes([1 if little else 0]))


class Reader:
    """A CDR stream over `data`, alignment origin at index zero."""

    def __init__(self, data, little):
        self.d = data
        self.p = 0
        self.little = little

    def align(self, n):
        self.p = ((self.p + n - 1) // n) * n

    def need(self, n):
        if self.p + n > len(self.d):
            raise ValueError("truncated: wanted %d octets at %d of %d" % (n, self.p, len(self.d)))

    def u8(self):
        self.need(1)
        v = self.d[self.p]
        self.p += 1
        return v

    def u16(self):
        self.align(2)
        self.need(2)
        v = struct.unpack_from("<H" if self.little else ">H", self.d, self.p)[0]
        self.p += 2
        return v

    def u32(self):
        self.align(4)
        self.need(4)
        v = struct.unpack_from("<I" if self.little else ">I", self.d, self.p)[0]
        self.p += 4
        return v

    def i32(self):
        self.align(4)
        self.need(4)
        v = struct.unpack_from("<i" if self.little else ">i", self.d, self.p)[0]
        self.p += 4
        return v

    def octets(self):
        n = self.u32()
        self.need(n)
        v = self.d[self.p : self.p + n]
        self.p += n
        return v

    def string(self):
        raw = self.octets()
        if not raw or raw[-1] != 0:
            raise ValueError("a CDR string must end in a NUL")
        return raw[:-1].decode("utf-8")

    def skip(self, n):
        self.need(n)
        self.p += n


# ── the advertisement, and the reference that carries it ─────────────────────


def ssl_component(supports, requires, port, little):
    """The `SSLIOP::SSL` encapsulation: two option words and a port.

    Three `unsigned short` after the order octet, so they land at offsets 2, 4
    and 6 of the encapsulation — eight octets, and no ORB is consulted about
    any of them.
    """
    e = encapsulation(little)
    e.u16(supports)
    e.u16(requires)
    e.u16(port)
    return e.bytes()


def stringified_ior(host, profile_port, components, little):
    """`IOR:<hex>` for one IIOP 1.2 profile (§7.6.9, §9.7.2), built by hand."""
    prof = encapsulation(little)
    prof.u8(1)  # IIOP major
    prof.u8(2)  # IIOP minor
    prof.string(host)
    prof.u16(profile_port)
    prof.octets(OBJECT_KEY)
    prof.u32(len(components))
    for tag, data in components:
        prof.u32(tag)
        prof.octets(data)

    body = encapsulation(little)
    body.string(TYPE_ID)
    body.u32(1)  # one profile
    body.u32(TAG_INTERNET_IOP)
    body.octets(prof.bytes())
    return "IOR:" + body.bytes().hex()


def free_port(host):
    """A port the OS has just proved free, then released.

    Used where the measurement needs an address that refuses: a guessed number
    makes the refusal attributable to the environment rather than to the
    advertisement, which is the `spike-failover` lesson.
    """
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.bind((host, 0))
        return s.getsockname()[1]
    finally:
        s.close()


# ── GIOP, by hand ────────────────────────────────────────────────────────────


def read_exactly(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError("the caller hung up with %d of %d octets to go" % (n - len(buf), n))
        buf += chunk
    return buf


def read_request(sock):
    """Reads one whole GIOP 1.2 `Request` and returns (id, operation, args).

    The 1.2 header is `request_id`, `response_flags`, three reserved octets, a
    `TargetAddress` union whose `KeyAddr` arm is discriminant 0, the object
    key, the operation, and the service context list; the body then aligns to
    eight **from the first octet of the message header**.
    """
    header = read_exactly(sock, HEADER_LEN)
    if header[:4] != MAGIC:
        return {"kind": "not-giop", "first_bytes": header[:4].hex(), "raw": header}
    if header[7] != MSG_REQUEST:
        raise ValueError("expected a Request, got message type %d" % header[7])
    if header[6] & MORE_FRAGMENTS:
        raise ValueError("the caller fragmented a request this fixture never made big")
    little = bool(header[6] & LITTLE_ENDIAN_FLAG)
    size = struct.unpack_from("<I" if little else ">I", header, 8)[0]
    msg = header + read_exactly(sock, size)

    r = Reader(msg, little)
    r.skip(HEADER_LEN)
    request_id = r.u32()
    r.u8()  # response_flags
    r.skip(3)  # reserved
    target = r.u16()
    if target != 0:
        raise ValueError("TargetAddress discriminant %d is not KeyAddr" % target)
    object_key = r.octets()
    operation = r.string()
    contexts = r.u32()
    for _ in range(contexts):
        r.u32()
        r.octets()

    r.align(8)  # §9.4.2.1, counted from the message header's first octet
    args = []
    while r.p + 4 <= len(msg):
        args.append(r.i32())
    return {
        "kind": "request",
        "request_id": request_id,
        "object_key": object_key,
        "operation": operation,
        "endian": "little" if little else "big",
        "args": args,
    }


def reply(request_id, little, result):
    """A GIOP 1.2 `Reply`, ``NO_EXCEPTION``, with `result` or nothing.

    `ReplyHeader_1_2` is the id, the status and the service context list —
    twelve octets, which puts the body at offset 24 and already 8-aligned.
    """
    w = Writer(little)
    w.raw(MAGIC)
    w.raw(bytes([1, 2, LITTLE_ENDIAN_FLAG if little else 0, MSG_REPLY]))
    w.raw(b"\x00\x00\x00\x00")  # message_size, patched below
    w.u32(request_id)
    w.u32(0)  # reply_status = NO_EXCEPTION
    w.u32(0)  # no service contexts
    if result is not None:
        w.align(8)
        w.i32(result)
    out = bytearray(w.bytes())
    struct.pack_into("<I" if little else ">I", out, 8, len(out) - HEADER_LEN)
    return bytes(out)


def serve(request, little):
    """`add` returns the sum of its arguments; anything else returns nothing."""
    if request["operation"] == "add":
        return reply(request["request_id"], little, sum(request["args"]))
    return reply(request["request_id"], little, None)


# ── the fixture ──────────────────────────────────────────────────────────────


def publish(text, path):
    """Writes `text` where the runner reads it, atomically.

    A wait loop that can read a half-written file is a wait loop that reports a
    phantom failure, so the file appears complete or not at all.
    """
    tmp = path + ".partial"
    with open(tmp, "w") as f:
        f.write(text if text.endswith("\n") else text + "\n")
    os.replace(tmp, path)


def build_advertisement(mode, host, listen_port):
    """(profile_port, components, advertised_tls_port) for one `--advertise`.

    Each mode is a shape a deployment actually produces, and the last three are
    the ones a client must refuse rather than quietly dial in cleartext.
    """
    supports = INTEGRITY | CONFIDENTIALITY | ESTABLISH_TRUST_IN_TARGET
    requires = INTEGRITY | CONFIDENTIALITY
    if mode == "ssl-only":
        # The better-attested convention: the cleartext port is 0 and the
        # component carries the real one.
        return 0, [(supports, requires, listen_port)], listen_port
    if mode == "same-port":
        # Deployed convention, not spec text: component port 0 means "the
        # profile's own port is the TLS port".
        return listen_port, [(supports, requires, 0)], listen_port
    if mode == "elsewhere":
        # The component says one thing and the profile port another: a live
        # cleartext listener sits at the profile port and the advertisement
        # points at a port that refuses. Falling back would connect.
        return listen_port, [(supports, requires, free_port(host))], None
    if mode == "unreadable":
        # A component that *claims* to be an advertisement and cannot be read.
        # Truncated at the point it is built, below; it must still be present,
        # because "present but unreadable" quietly treated as absent is the
        # downgrade this mode exists to refuse.
        return listen_port, [(supports, requires, listen_port)], None
    if mode == "none":
        return listen_port, [], None
    raise ValueError("unknown advertisement mode %r" % mode)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port-file", required=True)
    ap.add_argument("--ior-file", required=True)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--transport", choices=("tls", "plain"), default="tls")
    ap.add_argument(
        "--advertise",
        choices=("ssl-only", "same-port", "elsewhere", "none", "unreadable"),
        default="ssl-only",
    )
    ap.add_argument("--ior-endian", choices=("big", "little"), default="little")
    ap.add_argument("--component-endian", choices=("big", "little"), default="big")
    ap.add_argument("--reply-endian", choices=("big", "little"), default="big")
    ap.add_argument("--requests", type=int, default=1)
    ap.add_argument("--deadline-s", type=float, default=20.0)
    args = ap.parse_args()

    ior_little = args.ior_endian == "little"
    comp_little = args.component_endian == "little"
    reply_little = args.reply_endian == "little"

    # Certificates first: a fixture that cannot present a certificate must fail
    # before it publishes an address, so the runner's wait loop sees a dead
    # process rather than an endpoint that will never handshake.
    tls_context = None
    if args.transport == "tls":
        tls_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls_context.load_cert_chain(
            certfile=os.path.join(TLS_DIR, "server.pem"),
            keyfile=os.path.join(TLS_DIR, "server.key"),
        )

    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((args.host, 0))
    listener.listen(1)
    listener.settimeout(args.deadline_s)
    listen_port = listener.getsockname()[1]

    profile_port, options, tls_port = build_advertisement(args.advertise, args.host, listen_port)
    components = []
    for supports, requires, port in options:
        data = ssl_component(supports, requires, port, comp_little)
        if args.advertise == "unreadable":
            # A component that *claims* to be an advertisement and cannot be
            # read. Ignoring it silently is the downgrade this measures.
            data = data[:2]
        components.append((TAG_SSL_SEC_TRANS, data))
    ior = stringified_ior(args.host, profile_port, components, ior_little)

    observed = {
        "transport": args.transport,
        "advertise": args.advertise,
        "ior_endian": args.ior_endian,
        "component_endian": args.component_endian,
        "reply_endian": args.reply_endian,
        "listen_port": listen_port,
        "profile_port": profile_port,
        "advertised_tls_port": tls_port,
        "accepted": False,
        "handshake": None,
        "first_bytes": None,
        "client_hello": False,
        "object_key_matched": None,
        "served": [],
    }

    # The IOR before the port file: the runner waits on the port file, so
    # publishing it last means everything it names already exists.
    publish(ior, args.ior_file)
    publish(str(listen_port), args.port_file)

    # A blocking accept on a listener bound before the address was published.
    # The harness rule about a missed accept is about a *non-blocking* single
    # accept, which this is not; the deadline is what keeps it from hanging.
    # A timeout here is not an error — for `none`, `unreadable` and `elsewhere`
    # nobody connecting is the correct outcome, and the runner is what knows
    # which case this is.
    try:
        raw, _peer = listener.accept()
    except (socket.timeout, TimeoutError):
        print(json.dumps(observed), flush=True)
        listener.close()
        return 0

    observed["accepted"] = True
    raw.settimeout(args.deadline_s)

    sock = raw
    if tls_context is not None:
        try:
            sock = tls_context.wrap_socket(raw, server_side=True)
            observed["handshake"] = "ok"
            observed["cipher"] = sock.cipher()[0] if sock.cipher() else None
        except (ssl.SSLError, OSError) as exc:
            # The expected outcome when the caller does not trust our CA: it
            # sends an alert and hangs up. Reported, not judged.
            observed["handshake"] = "failed: %s" % (exc,)
            print(json.dumps(observed), flush=True)
            raw.close()
            listener.close()
            return 0
    else:
        observed["handshake"] = "not attempted"

    for _ in range(max(0, args.requests)):
        try:
            request = read_request(sock)
        except (EOFError, ssl.SSLError, OSError, socket.timeout) as exc:
            observed["served"].append({"error": "%s: %s" % (type(exc).__name__, exc)})
            break
        if request["kind"] == "not-giop":
            # A caller that dialed TLS into a plaintext listener puts a
            # ClientHello here: TLS record type 22, version major 3 (§5.1 of
            # RFC 8446 keeps the legacy record version). Recorded as the
            # positive evidence that the client attempted TLS and did not
            # quietly downgrade — proof from the far end, not from our own.
            observed["first_bytes"] = request["first_bytes"]
            observed["client_hello"] = request["raw"][0] == 0x16 and request["raw"][1] == 0x03
            # Closed at once rather than held open. A plaintext ORB that read a
            # ClientHello as a GIOP message would do the same, and holding the
            # socket instead makes the caller wait out its own read timeout —
            # which would measure the timeout rather than the refusal.
            try:
                sock.close()
            except OSError:
                pass
            print(json.dumps(observed), flush=True)
            listener.close()
            return 0
        observed["object_key_matched"] = request["object_key"] == OBJECT_KEY
        observed["served"].append(
            {
                "request_id": request["request_id"],
                "operation": request["operation"],
                "request_endian": request["endian"],
                "args": request["args"],
                "result": sum(request["args"]) if request["operation"] == "add" else None,
            }
        )
        sock.sendall(serve(request, reply_little))

    print(json.dumps(observed), flush=True)

    # Held open until the caller hangs up, so a clean close cannot reach it as
    # a reset. An expired deadline here is not a failure: the script has run to
    # the end and this is only politeness on the way out.
    try:
        sock.recv(1)
    except (ssl.SSLError, OSError, socket.timeout):
        pass
    try:
        sock.close()
    except OSError:
        pass
    listener.close()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 — the exit code is the verdict
        print("ssliop_peer: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        sys.exit(1)
