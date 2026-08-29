#!/usr/bin/env python3
"""A peer that is mid-call when the ORB shuts down, and reports what it saw.

D029 §5 O1 names the oracle for the ORB's lifecycle: *"a peer mid-call when
shutdown lands … the measurement is what the client **sees**, not what our
counters say."* `half_reply_peer.py` is the shape it points at — a peer held at
a chosen point. This is the mirror of that peer: there the fixture stalls a
*reply* to our client, here the fixture holds its *servant* and this program is
the client doing the observing.

# Why this exists beside a Rust test that measures the same thing

`crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs` asserts the same
three sentences and is the gate — it runs in `cargo test --workspace`. What it
cannot supply is **provenance**: its request bytes come out of
`orbweaver_giop::encode_request`, so both ends of that exchange are built by one
encoder. This program imports no ORB and no part of this project. Its two
requests are assembled by hand from CORBA 3.4 §9.4, and every octet it reads is
parsed by hand. A convention both ends apply cannot be refuted by a round trip;
this is the end that applies none of ours.

*이 프로그램은 우리 것을 아무것도 임포트하지 않는다. 두 요청은 §9.4에서 손으로
조립하고, 읽는 모든 옥텟도 손으로 뜯는다. 양쪽이 같이 지키는 관례는 왕복으로
반증되지 않는다 — 여기가 우리 관례를 하나도 적용하지 않는 쪽이다.*

# What it does

1. dial the fixture's published port;
2. write **two pipelined requests** — `held`, then `ping` — before either can be
   answered, so the second one's octets are at the server while the first is
   inside the servant;
3. the fixture notices its servant was entered and calls `Orb::shutdown`;
4. read every message until the conversation ends, and report the sequence.

# The verdict is the exit code

- **0** — exactly what D034 §3 says: a reply to request 1, then a
  `CloseConnection` with an empty body, and **no reply to request 2**.
- **1** — refuted. The transcript is in the JSON and says which third failed.
- **3** — nothing measured (could not connect, could not read). Not a pass: an
  unmeasured check is a failure by this project's own rule.

`--expect kill` measures the OTHER arm, and its claim runs the other way. D029's
lifecycle row names a second floor: *a target removed by being killed rather
than stopped — `Orb::shutdown` says §9.4.10's goodbye and a killed process
leaves a reset, which a caller can tell apart.* With `--expect kill` this peer
exits 0 when there was **no goodbye, no reply at all, and an abrupt end**, which
is that floor asserted rather than left to prose. The absence of the goodbye is
what is pinned; whether the abrupt end was an RST or a FIN is the platform's
business and is only recorded.

*`--expect kill`은 반대 방향의 주장을 잰다: **호출자가 구별할 수 있다**. 산문으로
이름만 붙여둔 바닥은 조용히 참이 아니게 될 수 있고, 주장된 바닥은 그럴 수 없다.
고정하는 것은 goodbye의 부재이고, RST냐 FIN이냐는 플랫폼의 일이라 기록만 한다.*

Stdlib only. No ORB is imported, installed or required.
"""

import argparse
import json
import socket
import struct
import sys

MAGIC = b"GIOP"
HEADER_LEN = 12
MSG_REQUEST = 0
MSG_REPLY = 1
MSG_CLOSE_CONNECTION = 5

MORE_FRAGMENTS = 0b10
LITTLE_ENDIAN_FLAG = 0b01

UNMEASURED = 3


def pack_u32(value, little):
    return struct.pack("<I" if little else ">I", value)


def unpack_u32(raw, little):
    return struct.unpack("<I" if little else ">I", raw)[0]


def unpack_i32(raw, little):
    return struct.unpack("<i" if little else ">i", raw)[0]


def pad_to(payload, align, origin):
    """Pads `payload` so the next octet lands on an `align` boundary.

    `origin` is how many octets precede `payload` in the message, because
    **alignment origin matters**: a GIOP message aligns from the first octet of
    its twelve-octet header, so a header-relative offset is not a message-relative
    one and using the wrong one is off by twelve.
    """
    while (origin + len(payload)) % align != 0:
        payload += b"\x00"
    return payload


def request(request_id, object_key, operation, little):
    """A GIOP 1.2 `Request`, assembled from §9.4.3 and §9.4.5.

    `RequestHeader_1_2`, in order: `request_id` (ulong), `response_flags`
    (octet), three reserved octets, `target` — a `TargetAddress` union whose
    discriminator is a short, `0 = KeyAddr`, followed by the object key as a
    `sequence<octet>` — then `operation` as a string, then the service context
    list. The body follows on an 8-octet boundary **measured from the start of
    the message**, and is empty here because these operations take no arguments.
    """
    p = pack_u32(request_id, little)
    p += b"\x03"  # response_flags: SYNC_WITH_TARGET — a reply is expected
    p += b"\x00\x00\x00"  # reserved
    # TargetAddress: a short discriminator, aligned to 2, then KeyAddr's
    # sequence<octet>.
    p = pad_to(p, 2, HEADER_LEN)
    p += struct.pack("<h" if little else ">h", 0)  # KeyAddr
    p = pad_to(p, 4, HEADER_LEN)
    p += pack_u32(len(object_key), little) + object_key
    # operation: a string is a length that counts the NUL, then the octets.
    p = pad_to(p, 4, HEADER_LEN)
    name = operation.encode("ascii") + b"\x00"
    p += pack_u32(len(name), little) + name
    # An empty service context list.
    p = pad_to(p, 4, HEADER_LEN)
    p += pack_u32(0, little)
    flags = LITTLE_ENDIAN_FLAG if little else 0
    return MAGIC + bytes([1, 2, flags, MSG_REQUEST]) + pack_u32(len(p), little) + p


def read_exactly(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            return None
        buf += chunk
    return buf


def read_message(sock):
    """One whole GIOP message, or `None` when the peer hung up."""
    header = read_exactly(sock, HEADER_LEN)
    if header is None:
        return None
    if header[:4] != MAGIC:
        raise ValueError("not a GIOP message: %r" % header[:4])
    little = bool(header[6] & LITTLE_ENDIAN_FLAG)
    size = unpack_u32(header[8:12], little)
    payload = read_exactly(sock, size) if size else b""
    if payload is None:
        raise ValueError("the peer hung up inside a message of %d octets" % size)
    return {
        "type": header[7],
        "little": little,
        "more": bool(header[6] & MORE_FRAGMENTS),
        "size": size,
        "payload": payload,
    }


def describe(msg):
    """One message, reduced to what the verdict is about.

    A reply's `long` result is **decoded**, never compared as raw octets: CDR
    padding content is undefined by the specification, so a buffer comparison
    against a reference is this project's recorded way of manufacturing false
    failures.
    """
    if msg["type"] == MSG_REPLY:
        p, little = msg["payload"], msg["little"]
        request_id = unpack_u32(p[0:4], little)
        status = unpack_u32(p[4:8], little)
        # `ReplyHeader_1_2`: id, status, service contexts. With no contexts that
        # is twelve octets after a twelve-octet message header — offset 24,
        # already 8-aligned — so the body starts immediately.
        contexts = unpack_u32(p[8:12], little)
        value = unpack_i32(p[12:16], little) if contexts == 0 and len(p) >= 16 else None
        return {"kind": "reply", "request_id": request_id, "status": status, "value": value}
    if msg["type"] == MSG_CLOSE_CONNECTION:
        return {"kind": "close_connection", "body_octets": msg["size"]}
    return {"kind": "other", "type": msg["type"], "size": msg["size"]}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--object-key", default="StopProbe")
    ap.add_argument("--endian", choices=("big", "little"), default="big")
    ap.add_argument(
        "--expect",
        choices=("stop", "kill"),
        default="stop",
        help="stop: D034 §3's goodbye. kill: the second floor — a target that "
        "died rather than stopped, which a caller must be able to tell apart",
    )
    ap.add_argument("--answer", type=int, default=42, help="what the servant returns")
    ap.add_argument(
        "--deadline-s",
        type=float,
        default=30.0,
        help="a read that blocks past this is a wedged fixture, not a slow one",
    )
    args = ap.parse_args()
    little = args.endian == "little"
    key = args.object_key.encode("ascii")

    report = {"endian": args.endian, "seen": [], "verdict": "unmeasured"}
    try:
        sock = socket.create_connection((args.host, args.port), timeout=args.deadline_s)
    except OSError as e:
        report["error"] = "could not dial the fixture: %s" % e
        print(json.dumps(report))
        return UNMEASURED

    try:
        with sock:
            sock.settimeout(args.deadline_s)
            sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            # Both requests before either can be answered. The second's octets
            # are then at the server while the first is inside the servant —
            # and whether the kernel has delivered them when the flag goes up
            # does not matter, because the claim is that the second is *never
            # answered*, which holds either way.
            sock.sendall(request(1, key, "held", little))
            sock.sendall(request(2, key, "ping", little))
            while True:
                try:
                    msg = read_message(sock)
                except ConnectionResetError:
                    # **A reset is an observation, not a failure to measure**,
                    # and telling the two apart is not pedantry: D034 §3's third
                    # sentence is that a live connection ends with
                    # `CloseConnection` and never with a bare TCP close, so a
                    # reset here is precisely the thing being refuted. Filing it
                    # under "could not measure" would have let the strongest
                    # refutation this fixture can produce exit 3 instead of 1 —
                    # which is what the first draft did, found by running the
                    # control rather than by reading it.
                    report["seen"].append({"kind": "reset"})
                    break
                if msg is None:
                    report["seen"].append({"kind": "eof"})
                    break
                seen = describe(msg)
                report["seen"].append(seen)
                if seen["kind"] == "close_connection":
                    break
    except (OSError, ValueError) as e:
        report["error"] = "reading the fixture failed: %s" % e
        print(json.dumps(report))
        return UNMEASURED

    seen = report["seen"]
    report["expect"] = args.expect

    if args.expect == "kill":
        # **The second floor, D029's lifecycle row.** `Orb::shutdown` says
        # §9.4.10's goodbye; a process that was killed says nothing at all. The
        # claim here is the opposite of every other claim this peer makes: not
        # that a caller cannot tell, but that it CAN — and a floor that is
        # asserted cannot quietly stop being true, where a floor that is only
        # named in prose can.
        #
        # What is asserted is the ABSENCE of the goodbye, not the presence of a
        # reset. Whether the kernel answers an aborted process with an RST or a
        # FIN depends on what is unread in the receive queue and on the
        # platform, and a test that pinned that would be measuring the OS. Both
        # are recorded so the transcript says which happened.
        ended_abruptly = len(seen) >= 1 and seen[-1]["kind"] in ("reset", "eof")
        no_goodbye = not any(s["kind"] == "close_connection" for s in seen)
        no_reply_at_all = not any(s["kind"] == "reply" for s in seen)
        report["ended_abruptly"] = ended_abruptly
        report["no_goodbye"] = no_goodbye
        report["no_reply_at_all"] = no_reply_at_all
        report["how_it_ended"] = seen[-1]["kind"] if seen else "nothing at all"
        ok = ended_abruptly and no_goodbye and no_reply_at_all
        report["verdict"] = "the caller can tell" if ok else "refuted"
        print(json.dumps(report))
        return 0 if ok else 1

    # The three sentences of D034 §3, each failing on its own.
    answered_in_full = (
        len(seen) >= 1
        and seen[0]["kind"] == "reply"
        and seen[0]["request_id"] == 1
        and seen[0]["status"] == 0
        and seen[0]["value"] == args.answer
    )
    second_never_answered = not any(
        s["kind"] == "reply" and s["request_id"] == 2 for s in seen
    )
    ended_with_goodbye = (
        len(seen) >= 2 and seen[-1]["kind"] == "close_connection" and seen[-1]["body_octets"] == 0
    )
    report["answered_in_full"] = answered_in_full
    report["second_never_answered"] = second_never_answered
    report["ended_with_goodbye"] = ended_with_goodbye
    ok = answered_in_full and second_never_answered and ended_with_goodbye
    report["verdict"] = "held" if ok else "refuted"
    print(json.dumps(report))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
