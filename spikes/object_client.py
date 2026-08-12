#!/usr/bin/env python3
"""Exercises the object model against the Rust server: pseudo-operations,
object references as values, and LOCATION_FORWARD.

TEST FIXTURE. See docs/PLAN.md section 10.
"""
import pathlib, sys, omniORB
from omniORB import CORBA

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "echo.idl"))
import spike  # noqa: E402

fails = 0
def check(label, got, want):
    global fails
    if got == want:
        print(f"  ok   {label} -> {got!r}")
    else:
        print(f"  FAIL {label} -> {got!r}, expected {want!r}"); fails += 1

orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
e = orb.string_to_object(pathlib.Path(sys.argv[1]).read_text().strip())._narrow(spike.Echo)
if e is None:
    print("FAIL narrow returned nil"); sys.exit(1)

check("_is_a(Echo)", e._is_a("IDL:spike/Echo:1.0"), True)
check("_is_a(Object)", e._is_a("IDL:omg.org/CORBA/Object:1.0"), True)
check("_is_a(unrelated)", e._is_a("IDL:nope/Nope:1.0"), False)
check("_non_existent()", e._non_existent(), False)

# A reference returned as a value must be usable, not merely non-nil.
me = e.get_self()
check("get_self() is callable", me.ping(), 42)
check("same_as(get_self())", e.same_as(me), True)

print(f"\nasserted cases: 6, failures: {fails}")
sys.exit(1 if fails else 0)
