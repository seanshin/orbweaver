#!/usr/bin/env python3
"""omniORB reads the same population file our trader was seeded from.

TEST FIXTURE. D026 section 5, S1b.

**What this measures that `trading_client.py` cannot.** That script is a good
measurement of the servant and it stays. What it cannot be is a measurement of
the *ranking*, because its expected order --

    check("specialization == 'math' MIN cost", ids, ["math-fast", ...])

-- is a literal typed at the client end by the same author who typed the
offers at the server end. If the ranker regressed and that literal were wrong
in the same direction, nothing would notice. CLAUDE.md: *a convention both ends
apply cannot be refuted by a round trip.*

So here there is **one file and three statements**, and no two of them were
written by the same reader:

  1. the **population** -- `corpus/state/moe-experts.json`'s `offers`, read
     here by Python's stdlib `json`;
  2. the **stated answer** -- the same file's `queries[].expect_ids`, checked
     against the population by `orbweaver-test`'s Rust gate, which shares no
     code with this script;
  3. the **wire answer** -- what our trader actually sent, decoded by omniORB's
     own COS stubs from IDL we did not compile.

A wrong expectation and a wrong ranker cannot cancel, because each would have
to agree with a set of property values neither of them wrote.

**The licence boundary.** omniORB runs here as a separate-process wire peer
over TCP (CLAUDE.md, sanctioned use (a)). Nothing under `crates/` links,
vendors or copies it, and `cargo tree --workspace` is unchanged by this file.

Usage:
    cargo run -q -p orbweaver-test --bin spike-seeded-trading -- \\
        spikes/seeded-trading.ior --hold &
    python3 spikes/seed_trading_client.py spikes/seeded-trading.ior
"""
import json
import pathlib
import sys

from omniORB import CORBA
import omniORB.COS.CosTrading_idl  # noqa: F401  (registers the COS stubs)
import CosTrading  # noqa: E402

L = CosTrading.Lookup

# The seed lives beside the contracts, and this script finds it the same way
# the Rust loader does -- relative to the checked-out tree, not to the caller's
# cwd. A client that resolved it differently could read a different file from
# the one the server was seeded with, which is the one failure this whole
# arrangement exists to make impossible.
SEED = pathlib.Path(__file__).resolve().parent.parent / "corpus" / "state" / "moe-experts.json"

fails = 0
asserted = 0


def check(label, got, want, subject=None):
    """Asserts equality and, on failure, says WHICH READER disagreed.

    `subject` is the point. `corpus/divergences.tsv` answers "which front end
    read this differently" for IDL, and nothing answered it for the wire: a
    peer check that fails today says "one of these two scripts is wrong". With
    a stated population the question has an owner, and the failure line names
    it.
    """
    global fails, asserted
    asserted += 1
    if got == want:
        print(f"  ok   {label}")
        return True
    print(f"  FAIL {label}")
    print(f"       the file states : {want!r}")
    print(f"       omniORB decoded : {got!r}")
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


def val(v):
    """Unwraps a `PropertyValue`; `CosTrading::Property::value` is an `any`."""
    return v.value() if isinstance(v, CORBA.Any) else v


def by_name(offer):
    return {p.name: val(p.value) for p in offer.properties}


def seeded_value(offer, name):
    """The value the FILE states for one property of one offer.

    AnyJSON v1 spellings are decoded here, by this reader, with no code shared
    with the Rust one: 64-bit integers are strings, enumerators are names, and
    `null` is an absence rather than a zero.
    """
    if name == "id":
        return offer["id"]
    raw = offer.get(name)
    if raw is None:
        return None  # stated absence
    if name in ("mem_footprint", "route_freq"):
        return int(raw)  # a quoted 64-bit integer
    return raw


# Properties every complete offer carries, in the trader's own order. Absent
# ones are omitted from the PropertySeq rather than sent empty, which is a
# different claim and the right one.
PROPS = [
    "id",
    "specialization",
    "cost",
    "latency_p50",
    "latency_p99",
    "load",
    "residency",
    "mem_footprint",
    "placement_node",
    "route_freq",
]


def main():
    if len(sys.argv) < 2:
        print("usage: seed_trading_client.py <ior-path>")
        return 2

    seed = json.loads(SEED.read_text())
    offers_by_id = {o["id"]: o for o in seed["offers"]}
    print(f"seed     {SEED} — {len(offers_by_id)} offer(s) of {seed['service_type']['name']}")

    ior = pathlib.Path(sys.argv[1]).read_text().strip()
    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    trader = orb.string_to_object(ior)._narrow(L)
    if trader is None:
        print("FAIL narrow to CosTrading::Lookup returned nil")
        return 1
    print("narrowed to CosTrading::Lookup")

    ALL = L.SpecifiedProps(L.all, None)
    how_many = len(offers_by_id) + 5

    # ---- Sentence 1: a NAMED population crossed the wire intact -------------
    # Not "a value we sent came back". This stated set, with these properties,
    # is what the other end sees -- and "these properties" is read from the
    # file by this script, not restated in it.
    print("\n1. the stated population, as omniORB sees it")
    wire, itr, _ = trader.query(seed["service_type"]["name"], "", "", [], ALL, how_many)
    check_that("the answer is complete (nil iterator)", itr is None or itr._is_nil())

    wire_by_id = {by_name(o)["id"]: o for o in wire}
    check(
        "the set of offer ids on the wire is the set the file states",
        sorted(wire_by_id),
        sorted(offers_by_id),
        subject="our trader's store, if ids are missing or extra",
    )

    for oid in sorted(offers_by_id):
        if oid not in wire_by_id:
            continue
        stated = offers_by_id[oid]
        got = by_name(wire_by_id[oid])
        for prop in PROPS:
            want = seeded_value(stated, prop)
            if want is None:
                # A stated absence is a claim: the property must be OMITTED
                # from the PropertySeq, not present and empty.
                check_that(
                    f"{oid}.{prop} is omitted, because the file states no value",
                    prop not in got,
                    f"the wire carried {got.get(prop)!r}",
                )
                continue
            have = got.get(prop)
            if prop == "residency":
                have = str(have)  # an enum crosses by name
            check(
                f"{oid}.{prop}",
                have,
                want,
                subject="our trader's encoder, or omniORB's decoder — they "
                "disagree with a value neither of them wrote",
            )

    # ---- Sentence 2: ordering and ranking, not merely running ---------------
    # Two independent checks per query. The first compares the wire's ORDER
    # against the FILE's property values, which is a statement about the ranker
    # that does not mention expect_ids at all. The second compares the wire's
    # ids against expect_ids. The Rust gate separately checks expect_ids
    # against the same property values. Three statements, three readers.
    print("\n2. ranking, checked against the population rather than against a literal")
    for q in seed["queries"]:
        name = q["name"]
        try:
            got, itr, _ = trader.query(
                seed["service_type"]["name"], q["constraint"], q["preference"], [], ALL, how_many
            )
        except Exception as e:  # noqa: BLE001 — a refusal here is a failure, and which one matters
            print(f"  FAIL {name}: query raised {type(e).__name__}({e})")
            globals()["fails"] += 1
            globals()["asserted"] += 1
            continue

        ids = [by_name(o)["id"] for o in got]
        check_that(
            f"[{name}] the answer is complete (nil iterator)", itr is None or itr._is_nil()
        )

        if q["ordered"]:
            check(f"[{name}] the ids, in order", ids, q["expect_ids"],
                  subject="our trader's ranker, or the file's stated order — the Rust "
                          "gate checks the file against itself, so a disagreement here "
                          "is the ranker")
            key = q.get("order_by")
            if key:
                # The independent half: does the order the WIRE chose agree
                # with the values the FILE states? This never reads
                # expect_ids, so it stands even if expect_ids is wrong.
                #
                # An offer with no value for `key` could not be placed by the
                # preference. The wire has one OfferSeq and no way to spell
                # "unranked", so those go last -- never first, and never
                # dropped. So the check is two-part: the ranked prefix
                # ascends, and every unrankable one is behind all of them.
                vals = [seeded_value(offers_by_id[i], key) for i in ids if i in offers_by_id]
                ranked = [v for v in vals if v is not None]
                check_that(
                    f"[{name}] the ranked offers really do ascend by `{key}` in the file's own values",
                    all(a <= b for a, b in zip(ranked, ranked[1:])),
                    f"the wire ordered {ids} whose file `{key}` values are {vals}",
                )
                cut = next((i for i, v in enumerate(vals) if v is None), None)
                if cut is not None:
                    check_that(
                        f"[{name}] every offer with no `{key}` sits behind every offer that has one",
                        all(v is None for v in vals[cut:]),
                        f"the wire ordered {ids} whose file `{key}` values are {vals}",
                    )
        else:
            check(f"[{name}] the ids, as a set", sorted(ids), sorted(q["expect_ids"]),
                  subject="our trader's constraint engine")

        # Present, and at the tail. Both halves: dropping an unrankable offer
        # would make the answer report fewer matches than matched, which the
        # caller cannot detect and cannot re-ask for.
        unranked = q.get("expect_unranked_last", [])
        for uid in unranked:
            check_that(
                f"[{name}] `{uid}` is still in the answer, not dropped for being unrankable",
                uid in ids,
                f"the wire returned {ids}",
            )
        if unranked and all(u in ids for u in unranked):
            check(
                f"[{name}] the unrankable offers are the tail of the answer",
                ids[-len(unranked):],
                unranked,
                subject="our trader's wire facade (lookup.rs::matching_offers), which "
                        "appends Selection::unranked after Selection::matched",
            )

    print(f"\nasserted cases: {asserted}, failures: {fails}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
