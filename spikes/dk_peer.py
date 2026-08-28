#!/usr/bin/env python3
"""Ask omniORB's own IR client what kind our facade says each entry is.

The claim: the ordinal our servant writes for `_get_def_kind` is read back by a
client built from the OMG IR IDL -- which we did not write and cannot influence
-- as the *named* enumerator we meant. A self-test cannot refute a wrong
ordinal, because both halves would be wrong together; this can.

It is served `spikes/dkprobe.idl`, which holds one definition per kind the
facade can answer, including the three the v1 wire cannot carry
(`valuetype`, abstract interface, `native`). Those three are the reason the
probe exists: before 2026-08-25 all three answered `dk_none` -- *"no such
definition"* -- for definitions the registry holds, and nothing was red.

Usage: `dk_peer.py <ior-file>`, against a holding `spike-ifr` that was loaded
with `spikes/dkprobe.idl`.

Exit code is the verdict, and it is the only verdict -- no marker is printed
for a caller to grep, because a traceback echoing this file's source lines can
print any marker this file contains:

    0   every leg answered, correctly, and the count of legs is printed
    1   at least one leg answered wrong (the claim is refuted)
    2   the IOR did not narrow to CORBA::Repository
    3   nothing was measured -- the stubs are absent, the IOR file is not
        there, or every leg came back TRANSIENT/COMM_FAILURE because the peer
        was not reachable. Exit 3 is not a pass: it says the claim is
        untested, which the caller must count as a failure or a SKIPPED, never
        as an ok. Told apart from exit 1 the way `spikes/ssliop.sh` tells them
        apart -- unmeasured only when *every* failing leg was unmeasured.

The leg count exists because exit 0 over *no* legs is the green-while-
measuring-nothing shape: a script whose body stopped running, or whose table
was emptied, still falls off the end with 0. The caller floors the count. It
is a floor, not a figure -- adding definitions to `dkprobe.idl` raises it.
"""

import sys

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

try:
    import CORBA
    import omniORB.ir_idl  # noqa: F401  (registers the IR stubs)
except ImportError as e:  # the fixture is absent: nothing was measured
    print(f"UNMEASURED: omniORBpy IR stubs unavailable ({e})")
    raise SystemExit(3) from None

# The measured answer for every definition in `spikes/dkprobe.idl`, as
# `id -> (ordinal, enumerator name)`. Measured 2026-08-26 against
# `spike-ifr <ior> corpus/golden/10-inheritance.idl
# corpus/golden/19-realistic-service.idl spikes/dkprobe.idl --hold`.
#
# The names here are not decoration: each is checked against omniORB's own
# `DefinitionKind` enum below before any leg runs, so a typo in this table is a
# failure of this table rather than a silent re-spelling of the peer's answer.
EXPECT = {
    "IDL:dkprobe/Amount:1.0": (10, "dk_Struct"),
    "IDL:dkprobe/Colour:1.0": (12, "dk_Enum"),
    "IDL:dkprobe/Refused:1.0": (4, "dk_Exception"),
    "IDL:dkprobe/Longs:1.0": (9, "dk_Alias"),
    "IDL:dkprobe/LIMIT:1.0": (3, "dk_Constant"),
    "IDL:dkprobe/Ordinary:1.0": (5, "dk_Interface"),
    # The three that answered dk_none before 2026-08-25. They are the point.
    "IDL:dkprobe/Describable:1.0": (24, "dk_AbstractInterface"),
    "IDL:dkprobe/Wallet:1.0": (20, "dk_Value"),
    "IDL:dkprobe/Handle:1.0": (23, "dk_Native"),
}

# `TRANSIENT` and friends mean the call never reached a servant, so the claim
# was not tested. Anything else -- BAD_OPERATION, OBJECT_NOT_EXIST, MARSHAL --
# means the peer answered, and answered wrong, which is a refutation.
UNREACHABLE = (CORBA.TRANSIENT, CORBA.COMM_FAILURE, CORBA.NO_RESPONSE)

legs = 0
wrong = []
unmeasured = []


def _fail(what, why, reached):
    (wrong if reached else unmeasured).append(f"{what}: {why}")
    print(f"  {what}: {why}  <-- {'WRONG' if reached else 'UNMEASURED'}")


def verdict():
    """0/1/2/3 as the docstring says. Never reached with a partial table."""
    if wrong or unmeasured:
        print(f"\nFAILURES ({len(wrong) + len(unmeasured)}, "
              f"{len(unmeasured)} of them UNMEASURED, which is not a pass):")
        for f in wrong + unmeasured:
            print(f"  {f}")
        # Nothing measured is told apart from a claim that did not hold.
        return 3 if not wrong else 1
    print(f"\ndef_kind: every leg answered as expected ({legs} legs)")
    return 0


if len(sys.argv) < 2:
    print("UNMEASURED: no IOR file argument")
    raise SystemExit(3)
try:
    with open(sys.argv[1]) as fh:
        ior = fh.read().strip()
except OSError as e:
    print(f"UNMEASURED: cannot read the IOR file ({e})")
    raise SystemExit(3) from None
if not ior:
    print("UNMEASURED: the IOR file is empty, so the facade never published one")
    raise SystemExit(3)

orb = CORBA.ORB_init(sys.argv)

# Leg 1: this table's names are omniORB's names. The whole enum comes from the
# peer's own stubs, so a wrong ordinal here cannot hide behind a matching name
# we invented -- and the enum's own size is asserted, because a peer that
# shipped a shorter DefinitionKind would make every ordinal below mean
# something else.
legs += 1
items = CORBA.DefinitionKind._items
peer_enum = {i: it._n for i, it in enumerate(items)}
print(f"peer enum ({len(items)} members): "
      + ", ".join(f"{i}={n}" for i, n in peer_enum.items()))
mismatched = [
    f"{oid} expects {ordinal}={name!r} but the peer's enum says "
    f"{ordinal}={peer_enum.get(ordinal)!r}"
    for oid, (ordinal, name) in EXPECT.items()
    if peer_enum.get(ordinal) != name
]
if len(items) != 25:
    mismatched.append(f"the peer's DefinitionKind has {len(items)} members, expected 25")
if mismatched:
    for m in mismatched:
        _fail("expected table vs the peer's own DefinitionKind", m, reached=True)
else:
    print("  expected table vs the peer's own DefinitionKind: every ordinal names "
          "what this table says")

try:
    r = orb.string_to_object(ior)._narrow(CORBA.Repository)
except UNREACHABLE as e:  # noqa: B014
    print(f"UNMEASURED: the peer was not reachable at all ({e!r})")
    leave(3)
if r is None:
    print("NARROW FAILED: the IOR is not a CORBA::Repository")
    leave(2)

print(f"== _get_def_kind for {len(EXPECT)} definitions ==")
for oid, (ordinal, name) in EXPECT.items():
    legs += 1
    try:
        o = r.lookup_id(oid)
    except UNREACHABLE as e:  # noqa: B014
        _fail(oid, f"lookup_id did not reach the peer: {e!r}", reached=False)
        continue
    except Exception as e:  # noqa: BLE001
        _fail(oid, f"lookup_id raised {e!r}", reached=True)
        continue
    if o is None:
        _fail(oid, "lookup_id returned nil for a definition the registry holds",
              reached=True)
        continue
    try:
        k = o._get_def_kind()
    except UNREACHABLE as e:  # noqa: B014
        _fail(oid, f"_get_def_kind did not reach the peer: {e!r}", reached=False)
        continue
    except Exception as e:  # noqa: BLE001
        _fail(oid, f"_get_def_kind raised {e!r}", reached=True)
        continue
    if (k._v, k._n) != (ordinal, name):
        _fail(oid, f"got {k._v} {k._n}, want {ordinal} {name}", reached=True)
    else:
        print(f"  {oid}\t{k._v}\t{k._n}")

leave(verdict())
