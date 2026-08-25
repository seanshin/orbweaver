#!/usr/bin/env python3
"""A GIOP peer that closes the connection **between two writes of one reply**.

`docs/decisions/D010` §4 B5 files this shape as class B — buildable, oracle
absent — because *"neither installed ORB will shut down inside the window
between two fragments on command"*. That is true of omniORB and of JacORB, and
it is the wrong conclusion: the peer this needs is not an ORB. It is a socket
that writes GIOP by hand, and a socket needs nothing that is missing here.

What it does, in order:

  1. binds, publishes its port, and blocks in ``accept``;
  2. reads N whole ``Request`` messages, keeping each one's ``request_id``;
  3. writes the **first** piece of a fragmented ``Reply`` to one of them — the
     more-fragments bit set, so the message is now owed a continuation;
  4. optionally writes further ``Fragment`` continuations, none of them final;
  5. **waits ``--window-ms``**, so the window is a knob and not a race;
  6. writes ``CloseConnection`` (§9.4.7) or ``MessageError`` (§9.4.8) where the
     continuation was due, and stops.

The reply's byte order is ``--reply-endian`` and is chosen independently of the
request's. GIOP sets the order per message, and the request id that decides
which caller hears what is read out of *the reply's* header, so a peer that
simply echoed the client's order would leave one of the two orders unmeasured
on any one machine — which is what every existing measurement of this path
does. *두 바이트 순서 모두.*

It then holds the socket open until the client hangs up. Closing while the
goodbye is still in flight can reach the client as a reset instead, and that
would make this a measurement of the platform's socket teardown rather than of
the reader.

TEST FIXTURE ONLY, and deliberately not an ORB: nothing is imported beyond the
standard library, every byte is built here from the published GIOP wire
specification, and nothing in this file is linked into Orbweaver. The point of
writing it by hand is that bytes produced by the encoder under test cannot
disagree with it.

    python3 spikes/half_reply_peer.py --port-file p.txt [--requests 2] \\
        [--cut 0] [--reply-endian big|little] [--continuations 0] \\
        [--window-ms 0] [--control close|error] [--deadline-s 30]

Prints one JSON object on stdout when the script has run to the end, naming the
ids it read and the one it cut, so the client's account of which call was cut
can be checked against the peer's rather than against itself. **The exit code
is the verdict**: zero only if every step above happened.

*한 응답의 두 번의 쓰기 사이에 연결을 끊는 피어. ORB가 아니라 손으로 GIOP를 쓰는
소켓이며, 창의 길이는 노브이지 경합이 아니다. 종료 코드가 판정이다.*
"""

import argparse
import json
import os
import socket
import struct
import sys

MAGIC = b"GIOP"
HEADER_LEN = 12
MSG_REQUEST = 0
MSG_REPLY = 1
MSG_CLOSE_CONNECTION = 5
MSG_MESSAGE_ERROR = 6
MSG_FRAGMENT = 7

MORE_FRAGMENTS = 0b10
LITTLE_ENDIAN_FLAG = 0b01


def pack_u32(value, little):
    return struct.pack("<I" if little else ">I", value)


def unpack_u32(raw, little):
    return struct.unpack("<I" if little else ">I", raw)[0]


def message(msg_type, little, more, payload):
    """A §9.4.1 header with `payload` after it."""
    flags = (LITTLE_ENDIAN_FLAG if little else 0) | (MORE_FRAGMENTS if more else 0)
    head = MAGIC + bytes([1, 2, flags, msg_type]) + pack_u32(len(payload), little)
    out = head + payload
    if more and len(out) % 8 != 0:
        # §9.4.9: every piece but the last is a multiple of eight octets,
        # header included. Refused here rather than sent, because a peer that
        # got this wrong would be measuring the reader's tolerance for a
        # malformed message instead of the interruption this fixture is about.
        raise ValueError("a non-final piece must be 8-aligned, got %d" % len(out))
    return out


def first_write_of_a_reply(request_id, little):
    """The first piece of a reply that will never have a second one.

    ``ReplyHeader_1_2`` is ``request_id``, ``reply_status`` and the service
    context list; with no contexts that is twelve octets, which leaves the body
    starting at offset 24 — already 8-aligned — so eight octets of body make a
    32-octet piece. Nothing past the request id is ever decoded, because the
    message can never complete; it is written correctly anyway so that what is
    measured is the interruption and not a malformed reply.
    """
    payload = pack_u32(request_id, little)  # request_id
    payload += pack_u32(0, little)  # reply_status = NO_EXCEPTION
    payload += pack_u32(0, little)  # no service contexts
    payload += b"\x00" * 8  # eight octets of body
    return message(MSG_REPLY, little, True, payload)


def another_write_of_the_same_reply(request_id, little):
    """A §9.4.9 continuation: ``FragmentHeader_1_2`` is the id, then payload."""
    return message(MSG_FRAGMENT, little, True, pack_u32(request_id, little) + b"\x00" * 8)


def read_exactly(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError("the client hung up with %d of %d octets to go" % (n - len(buf), n))
        buf += chunk
    return buf


def take_request_id(sock):
    """Reads one whole ``Request`` and returns its id, parsed by hand."""
    header = read_exactly(sock, HEADER_LEN)
    if header[:4] != MAGIC:
        raise ValueError("not a GIOP message: %r" % header[:4])
    if header[7] != MSG_REQUEST:
        raise ValueError("expected a Request, got message type %d" % header[7])
    if header[6] & MORE_FRAGMENTS:
        raise ValueError("the client fragmented a request this fixture never made big")
    little = bool(header[6] & LITTLE_ENDIAN_FLAG)
    size = unpack_u32(header[8:12], little)
    body = read_exactly(sock, size)
    # `RequestHeader_1_2` opens with the request id, whatever follows it.
    return unpack_u32(body[:4], little)


def publish(port, path):
    """Writes the port where the runner can read it, atomically.

    A runner's wait loop that can read a half-written file is a wait loop that
    reports a phantom failure, so the file appears complete or not at all.
    """
    tmp = path + ".partial"
    with open(tmp, "w") as f:
        f.write("%d\n" % port)
    os.replace(tmp, path)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port-file", required=True)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--requests", type=int, default=2)
    ap.add_argument("--cut", type=int, default=0, help="which request gets the unfinished reply")
    ap.add_argument("--reply-endian", choices=("big", "little"), default="big")
    ap.add_argument("--continuations", type=int, default=0)
    ap.add_argument("--window-ms", type=int, default=0)
    ap.add_argument("--control", choices=("close", "error"), default="close")
    ap.add_argument("--deadline-s", type=float, default=30.0)
    args = ap.parse_args()
    if not 0 <= args.cut < args.requests:
        ap.error("--cut must name one of the --requests")

    little = args.reply_endian == "little"
    control = MSG_CLOSE_CONNECTION if args.control == "close" else MSG_MESSAGE_ERROR

    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((args.host, 0))
    listener.listen(1)
    listener.settimeout(args.deadline_s)
    port = listener.getsockname()[1]
    publish(port, args.port_file)

    # Blocking accept on a listener bound before the port was published. The
    # harness rule about a missed accept is about a *non-blocking* single
    # accept, which this is not; the deadline is what keeps it from hanging.
    sock, _peer = listener.accept()
    sock.settimeout(args.deadline_s)

    ids = [take_request_id(sock) for _ in range(args.requests)]
    cut = ids[args.cut]

    # Write one. From here the peer owes a continuation and will not send one.
    sock.sendall(first_write_of_a_reply(cut, little))
    for _ in range(args.continuations):
        sock.sendall(another_write_of_the_same_reply(cut, little))

    # The window. The client is blocked in `read` across it, so its length
    # decides nothing — which is exactly what having it as a knob measures.
    if args.window_ms:
        # A wait that sleeps. The rule this obeys has its own line in CLAUDE.md
        # because a loop that does not sleep does not wait at all.
        import time

        time.sleep(args.window_ms / 1000.0)

    # Write two, and it is not the reply.
    sock.sendall(message(control, little, False, b""))

    print(
        json.dumps(
            {
                "read_ids": ids,
                "cut_id": cut,
                "cut_index": args.cut,
                "reply_endian": args.reply_endian,
                "continuations": args.continuations,
                "window_ms": args.window_ms,
                "control": args.control,
                "port": port,
            }
        ),
        flush=True,
    )

    # Held open until the client hangs up, so the goodbye cannot be raced away
    # by a reset. An expired deadline here is not a failure: the script has run
    # to the end and this is only politeness on the way out.
    try:
        sock.recv(1)
    except (socket.timeout, OSError):
        pass
    sock.close()
    listener.close()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 — the exit code is the verdict
        print("half_reply_peer: %s: %s" % (type(exc).__name__, exc), file=sys.stderr)
        sys.exit(1)
