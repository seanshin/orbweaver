#!/usr/bin/env python3
"""omniORB's half of spikes/perm_fallback.sh: three pings on one reference,
with a pause between the first and the second during which the harness
kills the server the first ping was forwarded to.

TEST FIXTURE. omniORB is a separate process over TCP, never a dependency
(CLAUDE.md, licensing boundary).

    perm_fallback_client.py <ior-file> <ready-file> <go-file>

  1. ping() on the reference in <ior-file>. The server there forwards it —
     LOCATION_FORWARD or LOCATION_FORWARD_PERM, the harness decides — and
     omniORB follows to the second server, whose ping() answers 2.
  2. Touch <ready-file>. The harness now stops the second server and removes
     its IOR file, after which the first server answers ping() itself with 1.
  3. Wait — sleeping — for <go-file>.
  4. ping() twice more. Each is printed as the number it returned or the
     system exception it raised. "1" means omniORB went back to the original
     address; an exception means it stayed on the dead one.

Nothing here is asserted: the script reads this output and the servers'
logs, and it decides.
"""
import os
import pathlib
import sys
import time

import omniORB
from omniORB import CORBA

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "echo.idl"))
import spike  # noqa: E402

DEADLINE_S = 30.0


def one_ping(k, obj):
    try:
        got = obj.ping()
        print(f"call {k} -> {got}", flush=True)
    except CORBA.SystemException as ex:
        print(
            f"call {k} -> {ex.__class__.__name__} minor=0x{ex.minor:08x} completed={ex.completed}",
            flush=True,
        )


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    ior_path, ready, go = sys.argv[1:4]
    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    obj = orb.string_to_object(pathlib.Path(ior_path).read_text().strip())
    e = obj._narrow(spike.Echo)
    if e is None:
        print("NARROW FAILED", flush=True)
        return 1
    print(f"omniORB {omniORB.__version__}", flush=True)

    one_ping(1, e)
    pathlib.Path(ready).write_text("ready\n")

    # A wait loop that sleeps (CLAUDE.md, harness rules).
    deadline = time.monotonic() + DEADLINE_S
    while not os.path.exists(go):
        if time.monotonic() > deadline:
            print("TIMEOUT waiting for the go file", flush=True)
            return 3
        time.sleep(0.05)

    one_ping(2, e)
    one_ping(3, e)
    print("DONE", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
