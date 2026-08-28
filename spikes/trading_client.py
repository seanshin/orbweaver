#!/usr/bin/env python3
"""omniORB's own `CosTrading::Lookup` client against our trader.

TEST FIXTURE. See docs/decisions/D022 section 6 T4.

**This is the point of opening the trading service.** `PLAN-SERVICES` section 3
deferred the standard `CosTrading` facade until a foreign trading client was
named, and the argument for opening it was that omniORB is one. So this script
imports omniORB's COS stubs and nothing of ours: it resolves the IOR our
fixture published, narrows it to `CosTrading::Lookup`, and calls `query`. Every
octet on the wire is written by an ORB we did not write, against IDL we did not
compile, which is the only kind of evidence that settles whether the servant is
conformant rather than merely self-consistent.

The licence boundary is intact: omniORB runs here as a **separate-process wire
peer over TCP** (CLAUDE.md, sanctioned use (a)). Nothing under `crates/` links
it, and `cargo tree` is unchanged.

Usage: `trading_client.py <ior-path>`
"""
import pathlib
import sys

from omniORB import CORBA
import omniORB.COS.CosTrading_idl  # noqa: F401  (registers the COS stubs)
import CosTrading  # noqa: E402

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

L = CosTrading.Lookup

fails = 0
asserted = 0


def check(label, got, want):
    global fails, asserted
    asserted += 1
    if got == want:
        print(f"  ok   {label} -> {got!r}")
    else:
        print(f"  FAIL {label} -> {got!r}, expected {want!r}")
        fails += 1


def check_that(label, ok, detail=""):
    global fails, asserted
    asserted += 1
    if ok:
        print(f"  ok   {label}")
    else:
        print(f"  FAIL {label}{(': ' + detail) if detail else ''}")
        fails += 1


def raises(label, want_exc, call, **members):
    """Calls `call`, expecting `want_exc`, and checks the members it carries.

    The members matter as much as the type: every one of these exceptions
    carries the caller's own offending string back, and a client reads that
    rather than any prose the server logs.
    """
    global fails, asserted
    asserted += 1
    try:
        call()
    except want_exc as e:
        bad = [
            f"{k}={getattr(e, k, None)!r} (wanted {v!r})"
            for k, v in members.items()
            if getattr(e, k, None) != v
        ]
        if bad:
            print(f"  FAIL {label} raised {want_exc.__name__} but {', '.join(bad)}")
            fails += 1
        else:
            print(f"  ok   {label} -> {want_exc.__name__}({members})")
        return
    except Exception as e:  # noqa: BLE001 — the whole point is what came instead
        print(f"  FAIL {label} raised {type(e).__name__}({e}), wanted {want_exc.__name__}")
        fails += 1
        return
    print(f"  FAIL {label} did not raise; wanted {want_exc.__name__}")
    fails += 1


ALL = L.SpecifiedProps(L.all, None)
NONE = L.SpecifiedProps(L.none, None)


def props(offer):
    """An offer's property names, in the order they arrived."""
    return [p.name for p in offer.properties]


def val(v):
    """Unwraps a `PropertyValue`.

    `CosTrading::Property::value` is an `any`, and omniORB hands back a
    `CORBA.Any` whenever it cannot infer a narrower Python type — which is
    every value here, since the trader writes explicit TypeCodes. Unwrapping
    rather than comparing against `Any` is what makes the checks below about
    the *values* on the wire.
    """
    return v.value() if isinstance(v, CORBA.Any) else v


def by_name(offer):
    """An offer's properties as a name -> value map, values unwrapped."""
    return {p.name: val(p.value) for p in offer.properties}


def main():
    if len(sys.argv) < 2:
        print("usage: trading_client.py <ior-path>")
        return 2
    ior = pathlib.Path(sys.argv[1]).read_text().strip()

    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    obj = orb.string_to_object(ior)
    trader = obj._narrow(L)
    if trader is None:
        print("FAIL narrow to CosTrading::Lookup returned nil")
        return 1
    print("narrowed to CosTrading::Lookup")

    # --- the interface, as a foreign client establishes it -------------------
    # A client narrows to whichever of the four its stub was generated from.
    for rid in (
        "IDL:omg.org/CosTrading/Lookup:1.0",
        "IDL:omg.org/CosTrading/TraderComponents:1.0",
        "IDL:omg.org/CosTrading/SupportAttributes:1.0",
        "IDL:omg.org/CosTrading/ImportAttributes:1.0",
    ):
        check(f"_is_a({rid.split('/')[-1]})", trader._is_a(rid), True)
    check("_is_a(Register)", trader._is_a("IDL:omg.org/CosTrading/Register:1.0"), False)
    check("_non_existent()", trader._non_existent(), False)

    # --- what this trader admits about itself -------------------------------
    # D022 section 7's prohibitions, as the wire states them.
    check("supports_modifiable_properties", trader.supports_modifiable_properties, False)
    check("supports_dynamic_properties", trader.supports_dynamic_properties, False)
    check("supports_proxy_offers", trader.supports_proxy_offers, False)
    check_that(
        "type_repos is nil (no ServiceTypeRepository servant, D022 section 7)",
        trader.type_repos is None or trader.type_repos._is_nil(),
    )
    for attr in ("register_if", "link_if", "proxy_if", "admin_if"):
        ref = getattr(trader, attr)
        check_that(
            f"{attr} is nil (the specification's way of saying unsupported)",
            ref is None or ref._is_nil(),
        )
    me = trader.lookup_if
    check_that("lookup_if is non-nil and callable", me is not None and not me._non_existent())

    # A trader with no links: zero hops, and local_only is the only follow
    # policy that can mean anything.
    check("def_hop_count", trader.def_hop_count, 0)
    check("max_hop_count", trader.max_hop_count, 0)
    check("def_follow_policy", trader.def_follow_policy, CosTrading.local_only)
    check("max_follow_policy", trader.max_follow_policy, CosTrading.local_only)

    bound = trader.max_return_card
    check_that("max_return_card is a positive bound", bound > 0, f"got {bound}")
    check("max_list agrees with max_return_card", trader.max_list, bound)
    check("def_return_card agrees with max_return_card", trader.def_return_card, bound)
    check("max_search_card is 0 (unlimited)", trader.max_search_card, 0)

    # --- a query that fits: every match, and a NIL iterator ------------------
    # D022 section 5: when the matches are at most `how_many`, all of them are
    # in `offers` and `offer_itr` is nil. A nil iterator here always means
    # "that is all of them" — this trader never truncates to make it true.
    offers, itr, limits = trader.query("moe::Expert", "", "", [], ALL, 10)
    check("a query that fits returns all five offers", len(offers), 5)
    check_that("offer_itr is nil, so the answer is complete", itr is None or itr._is_nil())
    check("limits_applied is empty (nothing was clamped)", list(limits), [])

    # A constrained query, ordered by the preference expression.
    offers, itr, limits = trader.query(
        "moe::Expert", "specialization == 'math'", "MIN cost", [], ALL, 10
    )
    ids = [by_name(o)["id"] for o in offers]
    check("specialization == 'math' MIN cost", ids, ["math-fast", "math-slow", "untimed"])
    costs = [by_name(o)["cost"] for o in offers]
    check_that("MIN cost really ordered them", costs == sorted(costs), f"got {costs}")

    # --- absent is absent, not empty ----------------------------------------
    # `untimed` carries no latency_p50 and `unlabelled` no specialization, so
    # their PropertySeqs are short. A property present-and-empty would be a
    # different claim, and the wrong one.
    offers, _, _ = trader.query("moe::Expert", "", "", [], ALL, 10)
    by_id = {by_name(o)["id"]: o for o in offers}
    check("a complete offer carries all ten properties", len(props(by_id["math-fast"])), 10)
    check_that(
        "an untimed offer omits latency_p50 rather than sending an empty one",
        "latency_p50" not in props(by_id["untimed"]),
        f"got {props(by_id['untimed'])}",
    )
    check_that(
        "an unlabelled offer omits specialization",
        "specialization" not in props(by_id["unlabelled"]),
        f"got {props(by_id['unlabelled'])}",
    )
    # An enum inside an `any` whose repository id omniORB has never seen: it
    # decodes from the TypeCode on the wire, which is what a TypeCode is for.
    check(
        "an unknown enum decodes from its TypeCode alone",
        str(by_name(by_id["math-fast"])["residency"]),
        "RESIDENT",
    )
    check(
        "a counter crosses as the integer it is",
        by_name(by_id["math-fast"])["mem_footprint"],
        1048576,
    )

    # `desired_props = none` returns the offers carrying no properties.
    offers, _, _ = trader.query("moe::Expert", "", "", [], NONE, 10)
    check("desired_props none returns offers with no properties", [props(o) for o in offers], [[]] * 5)

    # `desired_props = some` projects in the order asked for.
    some = L.SpecifiedProps(L.some, ["load", "cost"])
    offers, _, _ = trader.query("moe::Expert", "", "", [], some, 10)
    check("desired_props some projects in the order asked for", props(offers[0]), ["load", "cost"])

    # --- TCL's own spelling --------------------------------------------------
    # A foreign trading client writes TCL, and TCL spells its operators
    # lowercase. Our engine's parser spells them uppercase and says so; the
    # wire facade reconciles the two, which is the only reason a real trading
    # client can talk to this at all.
    offers, _, _ = trader.query(
        "moe::Expert", "specialization == 'math' and cost < 3", "min cost", [], ALL, 10
    )
    check("a lowercase TCL constraint and preference", [by_name(o)["id"] for o in offers],
          ["math-fast", "math-slow"])
    offers, _, _ = trader.query("moe::Expert", "not exist specialization", "", [], ALL, 10)
    check("lowercase 'not exist'", [by_name(o)["id"] for o in offers], ["unlabelled"])
    # A keyword inside a string literal is a value, not a keyword.
    offers, _, _ = trader.query("moe::Expert", "specialization == 'and'", "", [], ALL, 10)
    check("a keyword inside a literal stays a literal", len(offers), 0)
    # Property names are identifiers, so they do *not* fold.
    raises(
        "a property name in the wrong case is refused, not folded",
        CosTrading.IllegalConstraint,
        lambda: trader.query("moe::Expert", "SpEcIaLiZaTiOn == 'math'", "", [], ALL, 10),
        constr="SpEcIaLiZaTiOn == 'math'",
    )

    # --- a query that does NOT fit ------------------------------------------
    # There is no OfferIterator (D022 section 7), so the trader refuses rather
    # than truncating under a nil iterator, which would be a false statement in
    # the direction that loses offers the caller cannot ask for again.
    raises(
        "a query whose answer does not fit how_many is refused, not truncated",
        CORBA.NO_IMPLEMENT,
        lambda: trader.query("moe::Expert", "", "", [], ALL, 2),
    )
    # and the boundary is *at most*, not *fewer than*
    offers, itr, _ = trader.query("moe::Expert", "", "", [], ALL, 5)
    check("how_many exactly equal to the match count fits", len(offers), 5)
    check_that("and its iterator is still nil", itr is None or itr._is_nil())

    # --- every refusal, with the exception the IDL declares ------------------
    # The two nested ones are the ids worth pinning: they are declared inside
    # `Lookup`, so their repository ids carry the interface as a scope, and a
    # stub expecting `.../Lookup/IllegalPreference:1.0` gets UNKNOWN from an id
    # that merely looks right. This client's stubs were generated by omniidl
    # from the OMG IDL, so agreeing with them is the whole assertion.
    raises(
        "an illegal service type name",
        CosTrading.IllegalServiceType,
        lambda: trader.query("1illegal", "", "", [], ALL, 10),
        type="1illegal",
    )
    raises(
        "a service type nobody declared",
        CosTrading.UnknownServiceType,
        lambda: trader.query("moe::Nope", "", "", [], ALL, 10),
        type="moe::Nope",
    )
    raises(
        "a constraint that does not parse",
        CosTrading.IllegalConstraint,
        lambda: trader.query("moe::Expert", "cost <<", "", [], ALL, 10),
        constr="cost <<",
    )
    raises(
        "a preference that does not parse",
        L.IllegalPreference,
        lambda: trader.query("moe::Expert", "", "SIDEWAYS", [], ALL, 10),
        pref="SIDEWAYS",
    )
    raises(
        "an import policy this trader does not implement",
        L.IllegalPolicyName,
        lambda: trader.query(
            "moe::Expert",
            "",
            "",
            [CosTrading.Policy("exact_type_match", CORBA.Any(CORBA.TC_boolean, True))],
            ALL,
            10,
        ),
        name="exact_type_match",
    )
    raises(
        "a property an offer does not carry",
        CosTrading.IllegalPropertyName,
        lambda: trader.query(
            "moe::Expert", "", "", [], L.SpecifiedProps(L.some, ["throughput"]), 10
        ),
        name="throughput",
    )
    raises(
        "the same property asked for twice",
        CosTrading.DuplicatePropertyName,
        lambda: trader.query(
            "moe::Expert", "", "", [], L.SpecifiedProps(L.some, ["cost", "cost"]), 10
        ),
        name="cost",
    )

    print(f"\nasserted cases: {asserted}, failures: {fails}")
    return 1 if fails else 0


if __name__ == "__main__":
    leave(main())