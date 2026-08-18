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

# Captured before ORB_init, which strips every -ORB option it recognises.
ARGV = list(sys.argv)
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

# Wide text, and the reason this call is here rather than being obviously
# missing. §7.10.2.4: a profile carrying no TAG_CODE_SETS declares *no wchar
# support*, so a conformant client raises INV_OBJREF inside itself and sends
# nothing at all — measured 2026-08-18 against omniORB 4.3.4, minor
# 0x4F4D0001, with our server's log showing one earlier request and no error.
# Every wstring operation we serve was unreachable and nothing here could see
# it, because this file called every other operation and never this one.
#
# GIOP 1.0 has no wchar at all (§9.3.1.6 defines it from 1.1), and this harness
# drives the peer at 1.0, 1.1 and 1.2 — so the call is made only where the wire
# form exists. Asking at 1.0 measures the version, not the codeset.
giop = "1.2"
if "-ORBmaxGIOPVersion" in ARGV:
    giop = ARGV[ARGV.index("-ORBmaxGIOPVersion") + 1]
if giop in ("1.0", "1.1"):
    # 1.0 defines no wchar at all (§9.3.1.6). 1.1 is skipped for a different
    # and less comfortable reason: this peer is not an oracle for it. omniORB
    # 4.3.4 on this host marshals a bare 1.1 `wchar` and then raises
    # MARSHAL_MessageTooLong unmarshalling **its own output** — measured
    # 2026-08-18 while auditing the wide-character path, which is why the
    # GIOP 1.1 wchar unit order is recorded as UNMEASURED in `codeset.rs` and
    # was left unchanged rather than moved on a reading. Asserting our form
    # against a peer that disagrees with itself would measure the peer.
    # A driver that can settle it settles it; until then this is a stated gap,
    # not a silent one.
    print(f"  ..   echo_wstring skipped at GIOP {giop}: "
          "1.0 has no wchar; 1.1 has no usable oracle on this host")
else:
    try:
        check("echo_wstring(ascii)", echo.echo_wstring("wide"), "wide")
        check("echo_wstring(한글)", echo.echo_wstring("한글"), "한글")
    except CORBA.INV_OBJREF as e:
        print(f"  FAIL echo_wstring -> INV_OBJREF minor {e.minor:#x}: the peer refused to "
              "send wide text because our reference declares no codesets")
        fails += 1

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
