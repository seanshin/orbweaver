#!/usr/bin/env python3
"""A foreign ORB that forwards our client somewhere else.

Every LOCATION_FORWARD this project has ever followed was written by this
project. CLAUDE.md says why that is not evidence:

    A convention both ends apply cannot be refuted by a round trip, and a
    convention one end applies on read can hide the other end's defect on
    write.

A forward we generate and follow is exactly that shape. This fixture buys the
missing half: omniORB — a foreign ORB, a separate process, never linked —
decides on its own to answer `LOCATION_FORWARD`, and the address it names is a
*second* omniORB process at a *different* port, which our client must then dial
and call successfully.

The forward is produced by omniORB's own mechanism, not by anything we encode:
a POA with USE_SERVANT_MANAGER + NON_RETAIN whose `ServantLocator.preinvoke`
raises `PortableServer.ForwardRequest`. What goes on the wire after that is
omniORB's business, which is the point — we never write the bytes we are
claiming to interoperate with.

TEST FIXTURE ONLY. omniORB is LGPL/GPL and is never imported by, linked into or
shipped with Orbweaver; the Rust client on the other end of the socket speaks
the published GIOP wire and nothing else. See docs/PLAN.md section 10.

    python3 spikes/foreign_forward_peer.py --role dest    --out-ior b.ior
    python3 spikes/foreign_forward_peer.py --role forward --out-ior a.ior \
        --target-ior b.ior

Each role prints `READY <host> <port>` on stdout once its IOR is on disk and
its POA manager is accepting, then runs until killed. Ports are ephemeral
(`giop:tcp::0`) so a concurrent harness run cannot collide with this one.

The forwarder logs one line per `preinvoke` to stderr. That log is printed
beside the verdict and is never allowed to *be* the verdict — what a forwarder
believes it answered is not what the client received (CLAUDE.md, D034 section
5.1: our own counters are not what a peer saw).

*외부 ORB가 스스로 LOCATION_FORWARD를 답하게 만드는 픽스처다. 우리가 만들어 우리가
따라간 포워드는 증거가 되지 못한다 — 빠진 절반을 이 픽스처가 산다.*
"""

import argparse
import pathlib
import socket
import sys
import threading

from omniORB import CORBA, PortableServer
import omniORB

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "foreign_forward.idl"))
import foreign_forward, foreign_forward__POA  # noqa: E402  (made by importIDL)

# The object id the forwarding POA answers for. Any request naming it is
# forwarded; the value itself is arbitrary and only has to match what the
# client dials, which it does because the client dials the published IOR.
MOVED_OID = b"moved-object"

REPO_ID = "IDL:foreign_forward/Waypoint:1.0"


class Waypoint(foreign_forward__POA.Waypoint):
    """The destination servant. Answers with where it actually ran."""

    def __init__(self, tag):
        self.tag = tag
        self.where = "?"

    def where_am_i(self, note):
        return f"{self.tag}@{self.where}:{note}"


class ForwardEverything(PortableServer.ServantLocator):
    """`preinvoke` never returns a servant — it forwards, always.

    omniORB turns the raised `ForwardRequest` into a GIOP reply on its own. We
    never see or write those bytes, which is the entire value of this fixture.
    """

    def __init__(self, target, log):
        self.target = target
        self.log = log
        self.count = 0

    def preinvoke(self, oid, poa, operation):
        self.count += 1
        print(
            f"forwarder: preinvoke #{self.count} oid={oid!r} op={operation!r}"
            f" -> ForwardRequest",
            file=self.log,
            flush=True,
        )
        raise PortableServer.ForwardRequest(self.target)

    def postinvoke(self, oid, poa, operation, cookie, servant):
        # Never reached: preinvoke always raises.
        pass


def profile_address(orb, ref):
    """Host and port omniORB actually published, read back out of the IOR.

    Asking the ORB what endpoint it chose is asking a claim. Parsing the IOR it
    published is reading what a client will actually dial, which is the thing
    this leg is about.
    """
    import binascii
    import struct

    text = orb.object_to_string(ref)
    assert text.startswith("IOR:"), text
    raw = binascii.unhexlify(text[4:])
    # Top-level encapsulation: 1 byte endian flag, then the IOR body.
    little = raw[0] == 1
    e = "<" if little else ">"
    off = 1
    # 4-byte aligned within the encapsulation; offset 1 pads to 4.
    off = 4
    (idlen,) = struct.unpack_from(e + "I", raw, off)
    off += 4 + idlen
    off += (-off) % 4
    (nprof,) = struct.unpack_from(e + "I", raw, off)
    off += 4
    for _ in range(nprof):
        (tag,) = struct.unpack_from(e + "I", raw, off)
        off += 4
        (plen,) = struct.unpack_from(e + "I", raw, off)
        off += 4
        body = raw[off : off + plen]
        off += plen
        if tag != 0:  # TAG_INTERNET_IOP
            continue
        # Profile body is its own encapsulation, alignment restarting here.
        pe = "<" if body[0] == 1 else ">"
        p = 2  # skip endian flag + major; minor at 2
        p = 4  # after (flag, major, minor) pad to 4
        (hlen,) = struct.unpack_from(pe + "I", body, p)
        p += 4
        host = body[p : p + hlen - 1].decode("ascii")
        p += hlen
        p += (-p) % 2
        (port,) = struct.unpack_from(pe + "H", body, p)
        return host, port
    raise SystemExit("no TAG_INTERNET_IOP profile in published IOR")


def wait_until_dialable(host, port, seconds=10.0):
    """Sleeping, deadline-bounded wait until the endpoint accepts a connection.

    CLAUDE.md: wait loops must sleep, and a completed connect on macOS loopback
    is the only thing that establishes the listener is really there. Printing
    READY before this returns would hand the client a race that shows up as a
    forwarding failure.
    """
    import time

    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            s = socket.create_connection((host, port), timeout=0.5)
            s.close()
            return True
        except OSError:
            time.sleep(0.05)
    return False


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--role", choices=("dest", "forward"), required=True)
    ap.add_argument("--out-ior", required=True, help="where to publish our IOR")
    ap.add_argument("--target-ior", help="forward role: the IOR file to forward to")
    ap.add_argument("--tag", default=None, help="name this server answers with")
    ap.add_argument("--host", default="127.0.0.1")
    args = ap.parse_args()

    if args.role == "forward" and not args.target_ior:
        ap.error("--role forward needs --target-ior")

    tag = args.tag or args.role
    argv = [sys.argv[0], "-ORBendPoint", f"giop:tcp:{args.host}:0"]
    orb = CORBA.ORB_init(argv, CORBA.ORB_ID)
    root = orb.resolve_initial_references("RootPOA")
    pman = root._get_the_POAManager()

    if args.role == "dest":
        servant = Waypoint(tag)
        ref = servant._this()
        host, port = profile_address(orb, ref)
        servant.where = f"{host}:{port}"
    else:
        target_text = pathlib.Path(args.target_ior).read_text().strip()
        target = orb.string_to_object(target_text)
        policies = [
            root.create_id_assignment_policy(PortableServer.USER_ID),
            root.create_request_processing_policy(PortableServer.USE_SERVANT_MANAGER),
            root.create_servant_retention_policy(PortableServer.NON_RETAIN),
        ]
        poa = root.create_POA("ForwardPOA", pman, policies)
        poa.set_servant_manager(ForwardEverything(target, sys.stderr))
        ref = poa.create_reference_with_id(MOVED_OID, REPO_ID)
        host, port = profile_address(orb, ref)

    out = pathlib.Path(args.out_ior)
    out.write_text(orb.object_to_string(ref))

    pman.activate()

    if not wait_until_dialable(host, port):
        print(f"{tag}: published {host}:{port} but it never accepted", file=sys.stderr)
        return 3

    print(f"READY {host} {port}", flush=True)
    try:
        orb.run()
    except KeyboardInterrupt:
        orb.shutdown(True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
