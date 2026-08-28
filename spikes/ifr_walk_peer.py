#!/usr/bin/env python3
"""Walk our Interface Repository with omniORB's own IR client.

The claim: a client built from the OMG IR IDL -- which we did not write and
cannot influence -- can start at the root, call `contents`, `describe` what it
finds, and follow `defined_in` back up. Nothing here uses a stub of ours.

Usage: `ifr_walk_peer.py <ior-file>`, against `spike-ifr <ior-file> --hold`.

Exit code is the verdict, and it is the only verdict:

    0   every leg answered, and the count of legs is printed
    1   at least one leg raised or answered wrong
    2   the IOR did not narrow to CORBA::Repository

The count exists because exit 0 over *no* legs is the green-while-measuring-
nothing shape: a script whose body stopped running would still exit 0 here, so
the caller floors the count. It is a floor, not a figure -- adding legs raises
it and nothing in this file re-states today's number.
"""

import sys
import traceback

import CORBA
import omniORB.ir_idl  # noqa: F401  (registers the IR stubs)

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

failures = []
legs = 0


def expect(what, fn, want):
    """A check with an answer. `check` alone proves the call did not raise --
    which a swapped pair of same-typed members survives, as one negative
    control found."""
    global legs
    legs += 1
    try:
        got = fn()
    except Exception as e:  # noqa: BLE001
        failures.append(f"{what}: raised {e!r}")
        traceback.print_exc()
        return
    if got != want:
        failures.append(f"{what}: got {got!r}, want {want!r}")
        print(f"  {what}: {got!r}  != {want!r}  <-- WRONG")
    else:
        print(f"  {what}: {got!r}")


def check(what, fn):
    global legs
    legs += 1
    try:
        got = fn()
    except Exception as e:  # noqa: BLE001
        failures.append(f"{what}: raised {e!r}")
        traceback.print_exc()
        return None
    print(f"  {what}: {got}")
    return got


orb = CORBA.ORB_init(sys.argv)
r = orb.string_to_object(open(sys.argv[1]).read().strip())._narrow(CORBA.Repository)
if r is None:
    print("NARROW FAILED")
    leave(2)

print("== Repository::contents(dk_all, exclude_inherited=1) ==")
top = check("names", lambda: sorted(c._get_name() for c in r.contents(CORBA.dk_all, 1)))

print("== the filter actually filters ==")
check(
    "contents(dk_Module) names",
    lambda: sorted(c._get_name() for c in r.contents(CORBA.dk_Module, 1)),
)
check(
    "contents(dk_Interface) names",
    lambda: sorted(c._get_name() for c in r.contents(CORBA.dk_Interface, 1)),
)
check(
    "contents(dk_Struct) at the root is empty (they are inside modules)",
    lambda: [c._get_name() for c in r.contents(CORBA.dk_Struct, 1)],
)

print("== descend into a module ==")
mod = check("lookup('gc10')", lambda: r.lookup("gc10"))
if mod is not None:
    m = mod._narrow(CORBA.ModuleDef)
    check("module def_kind", lambda: m._get_def_kind()._n)
    check("module absolute_name", lambda: m._get_absolute_name())
    check(
        "module contents names",
        lambda: sorted(c._get_name() for c in m.contents(CORBA.dk_all, 1)),
    )
    d = check("module describe kind", lambda: m.describe().kind._n)
    expect("module describe value name", lambda: m.describe().value.value().name, "gc10")
    expect("module describe value id", lambda: m.describe().value.value().id, "IDL:gc10:1.0")
    expect("module describe value defined_in", lambda: m.describe().value.value().defined_in, "")
    check(
        "module defined_in is the repository",
        lambda: m._get_defined_in()._narrow(CORBA.Repository) is not None,
    )
else:
    failures.append("lookup('gc10') was nil, so the module legs did not run")

print("== an interface, its operations as objects ==")
iface = check("lookup_id(gc10::Both)", lambda: r.lookup_id("IDL:gc10/Both:1.0"))
if iface is not None:
    i = iface._narrow(CORBA.InterfaceDef)
    check(
        "contents(dk_all, exclude_inherited=1)",
        lambda: sorted(c._get_name() for c in i.contents(CORBA.dk_all, 1)),
    )
    check(
        "contents(dk_all, exclude_inherited=0) — inherited included",
        lambda: sorted(c._get_name() for c in i.contents(CORBA.dk_all, 0)),
    )
    check(
        "contents(dk_Operation, 0)",
        lambda: sorted(c._get_name() for c in i.contents(CORBA.dk_Operation, 0)),
    )
    check(
        "contents(dk_Attribute, 0)",
        lambda: sorted(c._get_name() for c in i.contents(CORBA.dk_Attribute, 0)),
    )
    check("interface describe kind", lambda: i.describe().kind._n)
    expect(
        "interface describe value name",
        lambda: i.describe().value.value().name,
        "Both",
    )
    expect(
        "interface describe value id",
        lambda: i.describe().value.value().id,
        "IDL:gc10/Both:1.0",
    )
    expect(
        "interface describe value defined_in",
        lambda: i.describe().value.value().defined_in,
        "IDL:gc10:1.0",
    )
    expect(
        "interface describe value version",
        lambda: i.describe().value.value().version,
        "1.0",
    )
    expect(
        "interface describe value bases",
        lambda: list(i.describe().value.value().base_interfaces),
        ["IDL:gc10/Derived:1.0", "IDL:gc10/Nameable:1.0"],
    )
    check("interface _get_type kind", lambda: str(i._get_type().kind()))

    # Not a bare call: a facade that answers NO_IMPLEMENT here would abort the
    # script with a traceback instead of a counted failure, and the caller
    # would print a stack where a diagnosis belongs.
    ops = []

    def _ops():
        global ops
        ops = i.contents(CORBA.dk_Operation, 0)
        return len(ops)

    check("operations as objects, for the walk below", _ops)
    if ops:
        op = ops[0]._narrow(CORBA.OperationDef)
        check("an OperationDef id", lambda: op._get_id())
        check("its def_kind", lambda: op._get_def_kind()._n)
        check("its absolute_name", lambda: op._get_absolute_name())
        check("its describe kind", lambda: op.describe().kind._n)
        expect("its describe value name", lambda: op.describe().value.value().name, "touch")
        expect(
            "its describe value id",
            lambda: op.describe().value.value().id,
            "IDL:gc10/Both/touch:1.0",
        )
        expect(
            "its describe value defined_in",
            lambda: op.describe().value.value().defined_in,
            "IDL:gc10/Both:1.0",
        )
        check("its describe value mode", lambda: op.describe().value.value().mode._n)
        check(
            "defined_in walks back to the interface",
            lambda: op._get_defined_in()._narrow(CORBA.InterfaceDef)._get_id(),
        )
        check(
            "containing_repository is the repository",
            lambda: op._get_containing_repository()._narrow(CORBA.Repository) is not None,
        )
    else:
        failures.append("the interface reported no operations, so the OperationDef legs did not run")
else:
    failures.append("lookup_id('IDL:gc10/Both:1.0') was nil, so the interface legs did not run")

print("== describe_contents, and max_returned_objs ==")
check(
    "describe_contents(dk_all, 1, -1) kinds",
    lambda: sorted(d.kind._n for d in r.describe_contents(CORBA.dk_all, 1, -1)),
)
check(
    "describe_contents values extract as ModuleDescriptions",
    lambda: sorted(d.value.value().name for d in r.describe_contents(CORBA.dk_all, 1, -1)),
)
check(
    "describe_contents contained_object round-trips",
    lambda: sorted(
        d.contained_object._narrow(CORBA.Contained)._get_id()
        for d in r.describe_contents(CORBA.dk_all, 1, -1)
    ),
)
check(
    "describe_contents(dk_all, 1, 1) returns one",
    lambda: len(r.describe_contents(CORBA.dk_all, 1, 1)),
)

print("== lookup vs lookup_name ==")
check("lookup('::gc10::Both') absolute", lambda: r.lookup("::gc10::Both")._get_id())
check("lookup('gc10::Both') relative", lambda: r.lookup("gc10::Both")._get_id())
check("lookup('nope') is nil", lambda: r.lookup("nope") is None)
check(
    "lookup_name('Both', -1, dk_all, 1)",
    lambda: [c._get_id() for c in r.lookup_name("Both", -1, CORBA.dk_all, 1)],
)
check(
    "lookup_name('Both', 1, dk_all, 1) — one level only, so nothing",
    lambda: [c._get_id() for c in r.lookup_name("Both", 1, CORBA.dk_all, 1)],
)


def undefined_levels():
    try:
        r.lookup_name("Both", 0, CORBA.dk_all, 1)
        return "ACCEPTED (expected BAD_PARAM)"
    except CORBA.BAD_PARAM:
        return "BAD_PARAM"


expect("lookup_name levels_to_search=0 is refused", undefined_levels, "BAD_PARAM")

print("== Repository::get_primitive / get_canonical_typecode ==")
check("get_primitive(pk_long)._get_kind()", lambda: r.get_primitive(CORBA.pk_long)._get_kind()._n)
check(
    "get_primitive(pk_long)._get_type()",
    lambda: str(r.get_primitive(CORBA.pk_long)._get_type().kind()),
)
expect("get_primitive(pk_null) is nil", lambda: r.get_primitive(CORBA.pk_null) is None, True)
check(
    "get_canonical_typecode of a bare tk_objref gets its name back",
    lambda: r.get_canonical_typecode(CORBA.TypeCode(CORBA.Object._NP_RepositoryId)).kind()
    is not None,
)

print("== the read-only promise is unchanged ==")


def still_refused():
    try:
        r.create_module("IDL:x:1.0", "x", "1.0")
        return "WRITE ACCEPTED"
    except CORBA.NO_PERMISSION:
        return "NO_PERMISSION"


expect("create_module", still_refused, "NO_PERMISSION")

if failures:
    print(f"\nFAILURES ({len(failures)}):")
    for f in failures:
        print(f"  {f}")
    leave(1)
print(f"\nwalk: every leg answered ({legs} legs)")
leave(0)
