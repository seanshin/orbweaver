#!/usr/bin/env python3
"""omniORB checks a live naming graph against the population file that states it.

TEST FIXTURE. D026 section 5, S1b — the second service.

**The sentence this is here to say.** Not "a name we bound resolved", but
*"this stated graph, with these components and these identities, is what the
other end sees."* `spike-names` still builds the graph inline in Rust; this
script does not change that and does not need to. It reads
`corpus/state/naming-graph.json` with Python's stdlib `json` and omniORB's own
`CosNaming` stubs, and asks the running fixture whether the stated graph is
the graph it is serving. Two readers, one file, sharing no code — so a
disagreement has a **subject** instead of being "one of these two is wrong".

**Why the checks look indirect.** Every reference in this graph points at
192.0.2.1:4000 — RFC 5737 TEST-NET-1, deliberately unroutable. Invoking
anything *on* a resolved reference would hang rather than fail, so every check
below inspects the reference locally: its repository id, and whether two
references are equivalent. Object-key identity is therefore established the
one way it can be without dialling — the file says which names share an
`object_key`, and `_is_equivalent` is asked to agree. That derivation comes
from the file; it is not restated here.

**The licence boundary.** omniORB runs as a separate-process wire peer over
TCP (CLAUDE.md, sanctioned use (a)). Nothing under `crates/` links, vendors or
copies it.

Usage:
    cargo run -q -p orbweaver-giop --bin spike-names -- spikes/names.ior --hold &
    python3 spikes/seed_naming_client.py spikes/names.ior
"""
import json
import pathlib
import sys

from omniORB import CORBA
import CosNaming  # noqa: E402

SEED = pathlib.Path(__file__).resolve().parent.parent / "corpus" / "state" / "naming-graph.json"

fails = 0
asserted = 0


def check(label, got, want, subject=None):
    global fails, asserted
    asserted += 1
    if got == want:
        print(f"  ok   {label}")
        return True
    print(f"  FAIL {label}")
    print(f"       the file states : {want!r}")
    print(f"       omniORB saw     : {got!r}")
    if subject:
        print(f"       subject         : {subject}")
    fails += 1
    return False


def check_that(label, ok, detail=""):
    global fails, asserted
    asserted += 1
    if ok:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}{(': ' + detail) if detail else ''}")
        fails += 1
    return ok


def name_of(entry):
    """The file's component list as a CosNaming::Name."""
    return [CosNaming.NameComponent(c["id"], c["kind"]) for c in entry["path"]]


def main():
    if len(sys.argv) < 2:
        print("usage: seed_naming_client.py <ior-path>")
        return 2

    graph = json.loads(SEED.read_text())
    print(f"seed     {SEED} — {len(graph['bindings'])} binding(s), "
          f"{len(graph['absent'])} stated absence(s)")

    ior = pathlib.Path(sys.argv[1]).read_text().strip()
    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    root = orb.string_to_object(ior)._narrow(CosNaming.NamingContext)
    if root is None:
        print("FAIL narrow to CosNaming::NamingContext returned nil")
        return 1
    print("narrowed to CosNaming::NamingContext")

    want_type = graph["reference_template"]["type_id"]

    # ---- the stated graph, as omniORB resolves it --------------------------
    print("\n1. every stated binding resolves, to a reference of the stated type")
    resolved = {}
    for b in graph["bindings"]:
        label = b["stringified"]
        try:
            ref = root.resolve(name_of(b))
        except Exception as e:  # noqa: BLE001 — which refusal came instead is the finding
            print(f"  FAIL resolve {label} raised {type(e).__name__}({e})")
            globals()["fails"] += 1
            globals()["asserted"] += 1
            continue
        if not check_that(f"resolve {label} returned a non-nil reference", ref is not None):
            continue
        resolved[label] = ref
        check(
            f"{label} carries the stated repository id",
            ref._NP_RepositoryId,
            want_type,
            subject="our naming servant's IOR, or omniORB's decoding of it",
        )

    # ---- identity, derived from the file rather than restated --------------
    # The file says which names share an object_key. Two names that share one
    # are the same object and must be equivalent; two that do not, must not
    # be. Neither expectation is written here -- both are read.
    print("\n2. names the file says share an object identity really do")
    keyed = [b for b in graph["bindings"] if b.get("object_key") and b["stringified"] in resolved]
    for i, a in enumerate(keyed):
        for b in keyed[i + 1:]:
            same = a["object_key"] == b["object_key"]
            got = resolved[a["stringified"]]._is_equivalent(resolved[b["stringified"]])
            check(
                f"{a['stringified']} vs {b['stringified']}: "
                f"{'same' if same else 'different'} object_key",
                got,
                same,
                subject="our naming servant's object keys",
            )

    # ---- a stated absence is a claim, not a gap ----------------------------
    print("\n3. every stated absence is absent, and refuses in the stated way")
    for a in graph["absent"]:
        label = a["stringified"]
        try:
            root.resolve(name_of(a))
        except CosNaming.NamingContext.NotFound as e:
            check(
                f"resolve {label} raised NotFound with the stated reason",
                str(e.why),
                a["expect_not_found_why"],
                subject="our naming servant's NotFound.why",
            )
            check(
                f"resolve {label} named the stated rest_of_name",
                [c.id for c in e.rest_of_name],
                a["expect_not_found_rest"],
                subject="our naming servant's NotFound.rest_of_name",
            )
        except Exception as e:  # noqa: BLE001
            print(f"  FAIL resolve {label} raised {type(e).__name__}({e}), wanted NotFound")
            globals()["fails"] += 1
            globals()["asserted"] += 1
        else:
            print(f"  FAIL resolve {label} succeeded; the file states it is absent")
            globals()["fails"] += 1
            globals()["asserted"] += 1

    # ---- the root listing --------------------------------------------------
    print("\n4. the root holds exactly what the file says it holds")
    want = graph["root_bindings"]
    bindings, itr = root.list(want["count"] + 10)
    check_that("the listing is complete (nil iterator)", itr is None or itr._is_nil())
    check(
        "the root binding count",
        len(bindings),
        want["count"],
        subject="our naming servant's list()",
    )
    check(
        "the root binding names",
        sorted(b.binding_name[0].id for b in bindings),
        sorted(want["names"]),
        subject="our naming servant's list()",
    )
    if want["all_are_contexts"]:
        check_that(
            "every root binding is a context",
            all(b.binding_type == CosNaming.ncontext for b in bindings),
            f"got {[str(b.binding_type) for b in bindings]}",
        )

    print(f"\nasserted cases: {asserted}, failures: {fails}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
