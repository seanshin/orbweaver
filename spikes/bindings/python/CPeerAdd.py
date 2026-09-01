"""Calls `add` on the C peer's server role, through a generated Python client.

Deliberately tiny: the cell it serves measures that generated code can reach a
program that speaks GIOP and links no ORB — not that Python can add. Every
conversion and the whole call path belong to the generated package.

Exit code is the verdict.
"""
import sys

root, idl, ior, bridge = sys.argv[1:5]
sys.path.insert(0, root)

from cpeer import _rt  # noqa: E402
import cpeer  # noqa: E402

with _rt.connect(idl, ior, command=[bridge]) as conn:
    echo = cpeer.CPeerEcho(conn)
    got = echo.add(40, 2)
    print("  ok   add(40, 2) -> %r" % (got,))
    print("python cpeer: %s" % ("PASS" if got == 42 else "FAIL"))
    raise SystemExit(0 if got == 42 else 1)
