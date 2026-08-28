#!/usr/bin/env python3
"""The three answers `resolve_initial_references` has, taken from omniORB live.

`orbweaver-console`'s `RESOLUTION_NOTE` tells an operator that three states get
three answers, and cites omniORB for two of them *"measured 2026-08-25"*. That
citation was a date in a doc comment with no gate under it, so this re-takes it
from the live peer on every run -- the same discipline as `spikes/*_capture.py`.

The states, and why collapsing any two would be a lie to an operator:

  * an id with a reference registered under it            -> it resolves;
  * a **reserved** id (CORBA 3.4 §8.5.2) with nothing
    bound to it -- a name this ORB knows and has not
    been given                                            -> `NO_RESOURCES`;
  * an id in neither list -- not a name this ORB knows
    at all, i.e. a typo                                   -> `BAD_PARAM`.

The fixes differ: register the service, or fix the spelling.

`rir` is resolved **locally** by whichever ORB is asked (§8.5.2: *"a simplified,
local version of the Naming Service"*), so this measures **omniORB's** table and
says nothing about ours -- which is the point. Our side is `spike-rir`, in the
other direction, and the harness runs both. Legs 2 and 3 are the exception:
they dial what the table answered, so the registered state is a reference that
reaches a live foreign servant rather than a reference-shaped object.

A fourth answer turned up while pinning the third and is pinned with it: the
**URL** form of a never-reserved id is refused by the URL parser
(`BAD_PARAM(BadURIOther)`) before the table is consulted at all, while the
**operation** §8.5.2 actually specifies raises `CORBA::ORB::InvalidName`. Both
are recorded, so neither can drift into the other.

Usage: `rir_peer.py [<corbaloc-url-for-NameService>]`, against a live omniNames.

Exit code is the verdict, and it is the only verdict -- no marker, because a
traceback echoes the source line it failed on and this file names every
exception it compares against:

    0   every leg answered as expected, and the count of legs is printed
    1   at least one leg answered wrong (the claim is refuted)
    3   nothing was measured -- omniORBpy is absent, or every failing leg failed
        because the peer could not be reached. Told apart from 1 the way
        `spikes/ssliop.sh` tells them apart: unmeasured only when *every*
        failing leg was unmeasured. Exit 3 is not a pass.

The leg count is printed because exit 0 over no legs is the green-while-
measuring-nothing shape; the caller floors it.
"""

import sys

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

try:
    import CORBA
    import omniORB
except ImportError as e:
    print(f"UNMEASURED: omniORBpy is not importable ({e})")
    raise SystemExit(3) from None

NS = sys.argv[1] if len(sys.argv) > 1 else "corbaloc::127.0.0.1:2809/NameService"
NC = "IDL:omg.org/CosNaming/NamingContext:1.0"

# omniORB's minor code for "this ORB reserves that ObjectId and nothing is bound
# to it". Where omniORB exports the constant we ask omniORB rather than retype
# the number -- a retyped classifier is the drift this project keeps finding.
# It exports `BAD_PARAM_BadURIOther` and does NOT export the NO_RESOURCES one,
# so that single literal carries its provenance: omniORB 4.3.4, measured
# 2026-08-26 by this script's own first run.
BAD_URI = omniORB.BAD_PARAM_BadURIOther
INITIAL_REF_NOT_FOUND = 1096024115

UNREACHABLE = (CORBA.TRANSIENT, CORBA.COMM_FAILURE, CORBA.NO_RESPONSE)

legs = 0
wrong = []
unmeasured = []


def _fail(what, why, reached=True):
    (wrong if reached else unmeasured).append(f"{what}: {why}")
    print(f"  {what}: {why}  <-- {'WRONG' if reached else 'UNMEASURED'}")


def leg(what, fn, want):
    """`fn` returns a short description of what happened; `want` is the one
    this file expects. Descriptions, not objects, because the distinction being
    measured is *which exception*, and a description makes a wrong answer
    readable in the harness's own output."""
    global legs
    legs += 1
    try:
        got = fn()
    except UNREACHABLE as e:  # noqa: B014
        _fail(what, f"did not reach the peer: {e!r}", reached=False)
        return
    except Exception as e:  # noqa: BLE001
        _fail(what, f"raised {type(e).__name__} {e!r}", reached=True)
        return
    if got != want:
        _fail(what, f"got {got!r}, want {want!r}", reached=True)
    else:
        print(f"  {what}: {got}")


orb = CORBA.ORB_init(["rir_peer", "-ORBInitRef", f"NameService={NS}"])
print(f"peer       -ORBInitRef NameService={NS}")


def url(u):
    """What omniORB answers for a `corbaloc:rir:` URL, as a description."""
    try:
        o = orb.string_to_object(u)
    except CORBA.NO_RESOURCES as e:
        return f"NO_RESOURCES minor={e.minor}"
    except CORBA.BAD_PARAM as e:
        return f"BAD_PARAM minor={e.minor}"
    return "NIL" if o is None else "a reference"


def op(key):
    """What the operation §8.5.2 specifies answers, as a description."""
    try:
        o = orb.resolve_initial_references(key)
    except CORBA.ORB.InvalidName:
        return "CORBA.ORB.InvalidName"
    except CORBA.NO_RESOURCES as e:
        return f"NO_RESOURCES minor={e.minor}"
    except CORBA.BAD_PARAM as e:
        return f"BAD_PARAM minor={e.minor}"
    return "NIL" if o is None else "a reference"


print("== state 1: an id with a reference registered under it ==")
leg("corbaloc:rir:/NameService", lambda: url("corbaloc:rir:/NameService"),
    "a reference")
leg("resolve_initial_references('NameService')", lambda: op("NameService"),
    "a reference")

# The two legs that make state 1 a measurement rather than a shape: the
# reference the table answered is DIALLED, and the thing that answers is a
# separate process in another implementation. A table that returned a
# well-formed reference to nothing would pass the two legs above.
registered = orb.string_to_object("corbaloc:rir:/NameService")
leg("that reference reaches a live servant (_non_existent)",
    lambda: registered._non_existent(), False)
leg("and it is a CosNaming::NamingContext (_is_a)",
    lambda: registered._is_a(NC), True)

print("== state 2: a RESERVED id with nothing bound to it ==")
leg("corbaloc:rir:/InterfaceRepository",
    lambda: url("corbaloc:rir:/InterfaceRepository"),
    f"NO_RESOURCES minor={INITIAL_REF_NOT_FOUND}")
leg("resolve_initial_references('InterfaceRepository')",
    lambda: op("InterfaceRepository"),
    f"NO_RESOURCES minor={INITIAL_REF_NOT_FOUND}")

print("== state 3: an id in neither list -- a typo, not a missing service ==")
leg("corbaloc:rir:/NoSuchService", lambda: url("corbaloc:rir:/NoSuchService"),
    f"BAD_PARAM minor={BAD_URI}")
leg("resolve_initial_references('NoSuchService')", lambda: op("NoSuchService"),
    "CORBA.ORB.InvalidName")

# Nothing above is allowed to be true by accident: if states 2 and 3 answered
# the same thing, every `leg` above could still pass while the distinction the
# console page rests on had gone. Assert the distinction itself.
legs += 1
s2, s3 = url("corbaloc:rir:/InterfaceRepository"), url("corbaloc:rir:/NoSuchService")
if s2 == s3:
    _fail("state 2 and state 3 are told apart",
          f"both answered {s2!r}; a missing registration and a typo are not the "
          "same problem and this peer no longer distinguishes them")
else:
    print(f"  state 2 and state 3 are told apart: {s2} vs {s3}")

if wrong or unmeasured:
    print(f"\nFAILURES ({len(wrong) + len(unmeasured)}, {len(unmeasured)} of them "
          "UNMEASURED, which is not a pass):")
    for f in wrong + unmeasured:
        print(f"  {f}")
    leave(3 if not wrong else 1)
print(f"\nrir-peer: every leg answered as expected ({legs} legs)")
leave(0)
