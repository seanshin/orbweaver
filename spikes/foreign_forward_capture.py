#!/usr/bin/env python3
"""Take a foreign ORB's `LOCATION_FORWARD` off the wire and read it, byte by byte.

This is the provenance half of the foreign-forward leg. It imports no ORB: it
builds its own GIOP requests against the published specification, sends them to
an omniORB process that has been rigged to forward, and reads the reply out of
the octets that came back.

Why it does not ask a library. Every fact this script reports — which byte
order the reply is in, which reply status it carries, whose address the
forwarded IOR names — is a fact about *omniORB's* encoding. Handing the bytes
to a decoder of ours to find out would mean a convention we apply on read could
hide a defect on the other end's write, which is the exact failure mode
CLAUDE.md records twelve wire changes against. So every field here is unpacked
with `struct` under an endianness **read off the flag byte**, never assumed and
never taken from what we sent.

That distinction has a measured instance in this project: a probe that reported
an order it had assumed. Both `reply_order` and `profile_order` below are
labelled `observed`, and the request order we chose is reported separately as
`sent_order`, so the three can be compared instead of conflated.

    python3 spikes/foreign_forward_capture.py --ior a.ior [--minor 0|1|2]
                                              [--order big|little] [--expect-port N]

Prints one `key=value` line per observation and exits:

    0  a LOCATION_FORWARD came back and every assertion held
    1  something came back and it was not the forward we require (a measured
       refutation — this is what the negative control must produce)
    3  nothing could be measured: the peer never answered, or the socket died

Exit 3 is kept separate from exit 1 on purpose. CLAUDE.md, D034 section 5.1: a
peer's first draft filed a TCP reset under UNMEASURED and so could not report
its own strongest refutation as a failure. A reset *here* is an observation —
if the forwarder answers a request by dropping the connection, that is a
measured failure of the leg, exit 1 — and exit 3 is reserved for the cases
where this script never got to look: no listener, no fixture, no bytes at all.

TEST FIXTURE ONLY. The peer is a separate process reached over TCP; nothing
from it is imported, linked or redistributed.

*피어의 LOCATION_FORWARD를 와이어에서 직접 읽는다. 바이트 순서는 플래그 바이트에서
읽으며 절대 가정하지 않는다 — 이 저장소에는 가정한 순서를 보고한 프로브의 실측
사례가 있다.*
"""

import argparse
import binascii
import socket
import struct
import sys
from pathlib import Path

TAG_INTERNET_IOP = 0

MSG_REQUEST, MSG_REPLY = 0, 1

# CORBA 3.4 section 9.4.3.2 ReplyStatusType.
REPLY_STATUS = {
    0: "NO_EXCEPTION",
    1: "USER_EXCEPTION",
    2: "SYSTEM_EXCEPTION",
    3: "LOCATION_FORWARD",
    4: "LOCATION_FORWARD_PERM",
    5: "NEEDS_ADDRESSING_MODE",
}


class Unmeasured(Exception):
    """Nothing could be looked at — no listener, no bytes. Exit 3."""


class Refuted(Exception):
    """Something came back and it was not what the leg requires. Exit 1."""


# ── a reader that never guesses its endianness ──────────────────────────────


class Cursor:
    """A CDR reader whose alignment origin and byte order are both explicit.

    `origin` is the offset alignment is measured from. A GIOP message aligns
    from the first byte of its 12-byte header; an encapsulation restarts at its
    own first byte (CLAUDE.md, and CORBA 3.4 section 9.3.3). Getting this wrong
    reads plausible garbage, so the origin is a constructor argument rather than
    something each call site remembers.
    """

    def __init__(self, buf, pos=0, origin=0, little=True):
        self.buf = buf
        self.pos = pos
        self.origin = origin
        self.e = "<" if little else ">"
        self.little = little

    def align(self, n):
        rel = self.pos - self.origin
        self.pos += (-rel) % n

    def take(self, n):
        if self.pos + n > len(self.buf):
            raise Refuted(
                f"message ended after {len(self.buf)} octets while reading"
                f" {n} at offset {self.pos}"
            )
        out = self.buf[self.pos : self.pos + n]
        self.pos += n
        return out

    def octet(self):
        return self.take(1)[0]

    def ushort(self):
        self.align(2)
        return struct.unpack(self.e + "H", self.take(2))[0]

    def ulong(self):
        self.align(4)
        return struct.unpack(self.e + "I", self.take(4))[0]

    def string(self):
        n = self.ulong()
        if n == 0:
            raise Refuted("CDR string with length 0; the NUL is counted")
        raw = self.take(n)
        return raw[:-1].decode("utf-8", "replace")

    def octets(self):
        return self.take(self.ulong())


class Writer:
    def __init__(self, little, origin=0):
        self.b = bytearray()
        self.e = "<" if little else ">"
        self.origin = origin

    def align(self, n):
        rel = len(self.b) - self.origin
        self.b.extend(b"\x00" * ((-rel) % n))

    def octet(self, v):
        self.b.append(v)

    def ushort(self, v):
        self.align(2)
        self.b.extend(struct.pack(self.e + "H", v))

    def ulong(self, v):
        self.align(4)
        self.b.extend(struct.pack(self.e + "I", v))

    def string(self, s):
        raw = s.encode("utf-8") + b"\x00"
        self.ulong(len(raw))
        self.b.extend(raw)

    def octets(self, raw):
        self.ulong(len(raw))
        self.b.extend(raw)


# ── IOR ─────────────────────────────────────────────────────────────────────


def parse_ior_body(c):
    """Decode an IOR the cursor `c` is positioned at, returning what a client needs.

    Used for both the stringified IOR on disk (which is wrapped in its own
    encapsulation, flag byte and all) and the one inside a LOCATION_FORWARD
    reply body (which is not — it is marshalled inline in the message stream).

    The caller supplies the cursor because those two cases differ in exactly the
    two things a CDR reader must not guess: where alignment is measured from,
    and which byte order applies. The stringified IOR is an encapsulation, so
    its origin is its own first octet and its order is the flag byte sitting
    there; the reply-body IOR is inline, so it inherits the message's origin and
    the message's order and has no flag byte at all. Conflating "where the data
    starts" with "where alignment is measured from" reads plausible garbage —
    this function's first draft did exactly that and reported a 553-megabyte
    string length from a 172-octet reply.
    """
    type_id = c.string()
    nprof = c.ulong()
    profiles = []
    for _ in range(nprof):
        tag = c.ulong()
        body = c.octets()
        profiles.append((tag, body))
    iiop = None
    for tag, body in profiles:
        if tag != TAG_INTERNET_IOP:
            continue
        # The profile body is an encapsulation: alignment restarts here and the
        # first octet is its OWN byte-order flag, independent of whatever
        # carried it. Read it; do not inherit it.
        prof_little = body[0] == 1
        p = Cursor(body, pos=1, origin=0, little=prof_little)
        major = p.octet()
        minor = p.octet()
        host = p.string()
        port = p.ushort()
        key = p.octets()
        iiop = {
            "profile_order": "little" if prof_little else "big",
            "profile_flag_byte": body[0],
            "version": f"{major}.{minor}",
            "host": host,
            "port": port,
            "object_key": bytes(key),
        }
        break
    if iiop is None:
        raise Refuted(f"IOR {type_id!r} carries no TAG_INTERNET_IOP profile")
    return type_id, iiop, len(profiles)


def read_ior_file(path):
    text = Path(path).read_text().strip()
    if not text.startswith("IOR:"):
        raise Unmeasured(f"{path} does not hold a stringified IOR")
    raw = binascii.unhexlify(text[4:])
    # An encapsulation: alignment restarts at its own first octet (origin 0
    # here), whose value is the byte-order flag, and the content follows it.
    little = raw[0] == 1
    c = Cursor(raw, pos=1, origin=0, little=little)
    return parse_ior_body(c), raw


# ── the request we build ourselves ──────────────────────────────────────────


def build_request(minor, little, request_id, object_key, operation, arg):
    """A GIOP `Request` for `operation(in string arg)`, in the named version.

    The three versions do not share a header layout and this writes each one
    rather than writing 1.2 and hoping. 1.0 and 1.1 lead with the service
    context list and address by object key directly; 1.2 leads with the request
    id, addresses through a `TargetAddress` union, and aligns the argument list
    to 8 from the start of the message (CORBA 3.4 section 9.4.1 through 9.4.2).
    """
    w = Writer(little, origin=0)
    if minor >= 2:
        w.ulong(request_id)
        w.octet(0x03)  # response_flags: reply expected
        w.b.extend(b"\x00\x00\x00")  # reserved
        w.ushort(0)  # TargetAddress: KeyAddr
        w.octets(object_key)
        w.string(operation)
        w.ulong(0)  # empty service context list
        # GIOP 1.2 aligns the request body to 8 from the start of the MESSAGE,
        # which is 12 octets of header ahead of this buffer.
        while (len(w.b) + 12) % 8:
            w.b.append(0)
    else:
        w.ulong(0)  # empty service context list
        w.ulong(request_id)
        w.octet(1)  # response_expected
        if minor == 1:
            w.b.extend(b"\x00\x00\x00")  # reserved, 1.1 only
        w.octets(object_key)
        w.string(operation)
        w.octets(b"")  # requesting_principal
    body_start = len(w.b)
    w.string(arg)
    del body_start

    flags = 0x01 if little else 0x00
    header = b"GIOP" + bytes([1, minor, flags, MSG_REQUEST])
    header += struct.pack(("<" if little else ">") + "I", len(w.b))
    return header + bytes(w.b)


def recv_exactly(sock, n):
    out = b""
    while len(out) < n:
        chunk = sock.recv(n - len(out))
        if not chunk:
            if not out:
                raise Refuted(
                    "peer closed the connection without answering the request"
                )
            raise Refuted(
                f"peer closed after {len(out)} of {n} expected octets"
            )
        out += chunk
    return out


def exchange(host, port, message, timeout):
    """Send one message, read one message back. Returns the raw reply octets."""
    try:
        sock = socket.create_connection((host, port), timeout=timeout)
    except OSError as e:
        raise Unmeasured(f"cannot dial {host}:{port}: {e}") from e
    with sock:
        sock.settimeout(timeout)
        try:
            sock.sendall(message)
            head = recv_exactly(sock, 12)
        except socket.timeout as e:
            raise Refuted(f"{host}:{port} accepted the request and never answered") from e
        except ConnectionResetError as e:
            # A reset is an OBSERVATION, not a failure to measure (D034 5.1).
            raise Refuted(f"{host}:{port} reset the connection instead of replying") from e
        if head[:4] != b"GIOP":
            raise Refuted(f"reply does not start with GIOP magic: {head[:4]!r}")
        flags = head[6]
        little = bool(flags & 0x01)
        size = struct.unpack(("<" if little else ">") + "I", head[8:12])[0]
        try:
            body = recv_exactly(sock, size) if size else b""
        except socket.timeout as e:
            raise Refuted(f"reply header promised {size} octets that never arrived") from e
        return head, body


def read_reply(head, body):
    """Decode a GIOP Reply, reading the byte order OFF the flag byte."""
    major, minor, flags, msgtype = head[4], head[5], head[6], head[7]
    little = bool(flags & 0x01)
    obs = {
        "giop_version": f"{major}.{minor}",
        "reply_order": "little" if little else "big",
        "reply_flag_byte": flags,
        "message_type": msgtype,
    }
    if msgtype != MSG_REPLY:
        raise Refuted(
            f"expected a Reply (type {MSG_REPLY}), peer sent message type {msgtype}"
        )
    # The message body is offset 12 from the alignment origin, so the cursor
    # keeps the message's origin and starts at 12 with the header prepended.
    buf = head + body
    c = Cursor(buf, pos=12, origin=0, little=little)
    if minor >= 2:
        obs["request_id"] = c.ulong()
        status = c.ulong()
        nctx = c.ulong()
        for _ in range(nctx):
            c.ulong()  # context_id
            c.octets()  # context_data
        c.align(8)
    else:
        nctx = c.ulong()
        for _ in range(nctx):
            c.ulong()
            c.octets()
        obs["request_id"] = c.ulong()
        status = c.ulong()
    obs["reply_status"] = status
    obs["reply_status_name"] = REPLY_STATUS.get(status, f"unknown({status})")
    return obs, c


# ── re-taking the recording ─────────────────────────────────────────────────
#
# `crates/orbweaver-giop/tests/foreign_forward_bytes.rs` holds three replies
# omniORB wrote on 2026-08-26. A recording is worth what it still describes, so
# this re-takes them from the live fixture and compares.
#
# It compares DECODED VALUES, and not all of them. CLAUDE.md's rule is about
# padding — bytes the specification leaves undefined — and the same argument
# reaches one step further here: the forwarded-to **port** and **object key**
# are regenerated by the fixture on every run, because the ports are ephemeral
# so a concurrent harness cannot collide with this leg. A comparison that
# included them would be red on every run including the correct ones, which is
# a check that has to be switched off, which is a check nobody reads.

RECORDING = "crates/orbweaver-giop/tests/foreign_forward_bytes.rs"

# What must still be true of a re-take. Every field the fixture regenerates is
# deliberately absent, and named in NOT_COMPARED so the omission is a statement
# rather than a gap.
COMPARED = ("reply_status", "giop_version", "type_id", "iiop_version", "profiles", "host")
NOT_COMPARED = ("port", "object_key")


def recorded(root):
    """Parse the three `FORWARD_1_x` constants out of the Rust test.

    There is no separate recording file: the provenance lives beside the
    assertions that use it, and this fails loudly if a constant is renamed
    rather than quietly comparing nothing.
    """
    import re

    path = Path(root) / RECORDING
    if not path.exists():
        raise Unmeasured(f"{RECORDING} is not in the tree; there is no recording to re-take")
    text = path.read_text()
    out = {}
    for minor in (0, 1, 2):
        m = re.search(
            r"const FORWARD_1_%d: &str = concat!\((.*?)\);" % minor, text, re.S
        )
        if not m:
            raise Unmeasured(
                f"FORWARD_1_{minor} is not in {RECORDING} any more — the recording"
                " cannot be re-taken until the constant is found"
            )
        hex_text = "".join(re.findall(r'"([0-9a-f]*)"', m.group(1)))
        if not hex_text:
            raise Unmeasured(f"FORWARD_1_{minor} holds no octets")
        out[minor] = binascii.unhexlify(hex_text)
    return out


def facts(head, body):
    """The decoded fields a re-take is judged on."""
    obs, c = read_reply(head, body)
    if obs["reply_status"] != 3:
        raise Refuted(
            f"the message is not a LOCATION_FORWARD: {obs['reply_status_name']}"
        )
    type_id, iiop, nprof = parse_ior_body(c)
    return {
        "reply_status": obs["reply_status"],
        "giop_version": obs["giop_version"],
        "type_id": type_id,
        "iiop_version": iiop["version"],
        "profiles": nprof,
        "host": iiop["host"],
        "port": iiop["port"],
        "object_key": binascii.hexlify(iiop["object_key"]).decode(),
    }


def check_recording(root, ior_path, timeout):
    """Compare the recorded replies against what the live peer writes today."""
    try:
        rec = recorded(root)
    except Unmeasured as e:
        print("verdict=UNMEASURED")
        print(f"reason={e}")
        return 2

    try:
        (_id, iiop, _n), _raw = read_ior_file(ior_path)
    except (Unmeasured, Refuted) as e:
        print("verdict=UNMEASURED")
        print(f"reason={e}")
        return 2

    bad = 0
    for minor in (0, 1, 2):
        try:
            was = facts(rec[minor][:12], rec[minor][12:])
        except Refuted as e:
            print(f"  FAIL 1.{minor}: the RECORDING does not decode: {e}")
            bad += 1
            continue
        try:
            head, body = exchange(
                iiop["host"],
                iiop["port"],
                build_request(minor, True, 0x5157, iiop["object_key"], "where_am_i", "retake"),
                timeout,
            )
            now = facts(head, body)
        except Unmeasured as e:
            print(f"  SKIPPED  1.{minor}: {e}")
            return 2
        except Refuted as e:
            print(f"  FAIL 1.{minor}: the LIVE peer no longer forwards: {e}")
            bad += 1
            continue
        diff = [k for k in COMPARED if was[k] != now[k]]
        if diff:
            bad += 1
            print(f"  FAIL 1.{minor}: the recording and the live peer differ on {diff}")
            for k in diff:
                print(f"       {k}: recorded={was[k]!r} live={now[k]!r}")
        else:
            print(
                f"  ok   1.{minor}: {len(rec[minor])} recorded octets still describe"
                f" the live peer on {len(COMPARED)} decoded field(s);"
                f" {', '.join(NOT_COMPARED)} regenerate per run and are not compared"
            )
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--ior", required=True, help="the forwarder's IOR file")
    ap.add_argument(
        "--check-recording",
        action="store_true",
        help="re-take the recorded replies in %s from the live fixture and"
        " compare the decoded values" % RECORDING,
    )
    ap.add_argument(
        "--root",
        default=str(Path(__file__).resolve().parent.parent),
        help="repository root, for finding the recording",
    )
    ap.add_argument("--minor", type=int, default=2, choices=(0, 1, 2))
    ap.add_argument("--order", choices=("big", "little"), default="little")
    ap.add_argument("--operation", default="where_am_i")
    ap.add_argument("--arg", default="probe")
    ap.add_argument("--timeout", type=float, default=10.0)
    ap.add_argument(
        "--expect-port",
        type=int,
        default=None,
        help="fail unless the forwarded-to IOR names this port",
    )
    ap.add_argument(
        "--expect-status",
        type=int,
        default=3,
        help="reply status the leg requires (3 = LOCATION_FORWARD)",
    )
    ap.add_argument("--hexdump", action="store_true", help="print the reply octets")
    args = ap.parse_args()

    if args.check_recording:
        return check_recording(args.root, args.ior, args.timeout)

    little = args.order == "little"
    try:
        (type_id, iiop, _n), _raw = read_ior_file(args.ior)
        print(f"sent_order={args.order}")
        print(f"sent_giop=1.{args.minor}")
        print(f"forwarder_host={iiop['host']}")
        print(f"forwarder_port={iiop['port']}")

        msg = build_request(
            args.minor, little, 0x5157, iiop["object_key"], args.operation, args.arg
        )
        head, body = exchange(iiop["host"], iiop["port"], msg, args.timeout)
        if args.hexdump:
            print("reply_hex=" + binascii.hexlify(head + body).decode())

        obs, c = read_reply(head, body)
        for k, v in obs.items():
            print(f"{k}={v}")

        if obs["reply_status"] != args.expect_status:
            raise Refuted(
                f"peer answered {obs['reply_status_name']}"
                f" ({obs['reply_status']}), leg requires status {args.expect_status}"
                f" ({REPLY_STATUS.get(args.expect_status)})"
            )

        # The forwarded IOR is marshalled inline in the reply body — no
        # encapsulation, so it has no flag byte of its own and inherits the
        # reply's order. Its IIOP profile DOES have one, and that one is read.
        fwd_id, fwd, nprof = parse_ior_body(c)
        print(f"forwarded_to_type_id={fwd_id}")
        print(f"forwarded_to_host={fwd['host']}")
        print(f"forwarded_to_port={fwd['port']}")
        print(f"forwarded_to_iiop_version={fwd['version']}")
        print(f"profile_order={fwd['profile_order']}  # observed, off the flag byte")
        print(f"profile_flag_byte={fwd['profile_flag_byte']}")
        print(f"forwarded_to_profiles={nprof}")
        print(f"forwarded_to_key_hex={binascii.hexlify(fwd['object_key']).decode()}")

        if fwd["port"] == iiop["port"] and fwd["host"] == iiop["host"]:
            raise Refuted(
                "the forwarded-to address is the SAME address we dialled"
                f" ({fwd['host']}:{fwd['port']}); this leg exists to measure a"
                " move to a DIFFERENT address"
            )
        if args.expect_port is not None and fwd["port"] != args.expect_port:
            raise Refuted(
                f"forwarded to port {fwd['port']}, expected {args.expect_port}"
            )
        if fwd_id != type_id:
            print(f"note=forwarded type_id {fwd_id!r} differs from dialled {type_id!r}")

        print("verdict=FORWARDED")
        return 0
    except Refuted as e:
        print(f"verdict=REFUTED")
        print(f"reason={e}")
        return 1
    except Unmeasured as e:
        print(f"verdict=UNMEASURED")
        print(f"reason={e}")
        return 3


if __name__ == "__main__":
    sys.exit(main())
