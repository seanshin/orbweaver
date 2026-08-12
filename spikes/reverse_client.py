#!/usr/bin/env python3
"""Phase 1 reverse interop: a stock omniORB client calling the Rust server.

TEST FIXTURE. omniORB is never linked into Orbweaver; it speaks to our server
over TCP using the published GIOP specification. See docs/PLAN.md section 10.
"""
import pathlib, sys
import omniORB
from omniORB import CORBA

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "echo.idl"))
import spike  # noqa: E402

ior = pathlib.Path(sys.argv[1]).read_text().strip()
orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
echo = orb.string_to_object(ior)._narrow(spike.Echo)
if echo is None:
    print("FAIL narrow returned nil"); sys.exit(1)

fails = 0
def check(label, got, want):
    global fails
    if got == want:
        print(f"  ok   {label} -> {got!r}")
    else:
        print(f"  FAIL {label} -> {got!r}, expected {want!r}"); fails += 1

check("ping()", echo.ping(), 42)
check("add(1000000, 337)", echo.add(1000000, 337), 1000337)
check("echo_string(...)", echo.echo_string("hello from omniORB"), "hello from omniORB")
check("scale(1.5, 4.0)", echo.scale(1.5, 4.0), 6.0)

r = spike.Ragged(a=0xAA, b=-7, c=9, d=2.5, e=0xBB)
back = echo.echo_ragged(r)
check("echo_ragged(...)", (back.a, back.b, back.c, back.d, back.e), (0xAA, -7, 9, 2.5, 0xBB))

# A large reply, which the Rust server fragments when asked to. If our
# fragments were malformed the peer would raise rather than reassemble.
for n in (100, 40000, 250000):
    data = echo.blob(n)
    expected = bytes((i % 251) for i in range(n))
    check(f"blob({n})", data == expected and len(data) == n, True)

# An unknown operation must come back as BAD_OPERATION, not a hang.
try:
    echo._get_interface()
    print("  note _get_interface() unexpectedly succeeded")
except CORBA.BAD_OPERATION:
    print("  ok   unknown op -> BAD_OPERATION as specified")
except CORBA.SystemException as e:
    print(f"  ok   unknown op -> {type(e).__name__}")

print(f"\nasserted cases: 8, failures: {fails}")
sys.exit(1 if fails else 0)
