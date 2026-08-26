//! D026 §5 S1 — the seeded population, checked for the things a fixture that
//! invents its own population cannot check.
//!
//! These are not tests of `orbweaver-trading`. They are tests of
//! `corpus/state/`, and every one of them asserts something that was
//! **unaskable** while each fixture built its population inline:
//!
//! - *Is every offer placed on a node the estate declares?* Three fixtures
//!   model MoE placement over three disjoint node namespaces (`node-a`,
//!   `gpu-04`, `gpu-eu-1`/`gpu-us-1`), and one of them refuses undeclared
//!   nodes default-deny. Nothing was red, because they are separate processes
//!   and no node was ever checked against another fixture's declaration.
//! - *Does the stated expected order agree with the stated property values?*
//!   A hand-built population with a hand-written expected order beside it is
//!   one author agreeing with themselves: edit a cost and the ranking
//!   expectation does not move, and the check stays green over the drift.
//!
//! The half these cannot do is *"and the implementation agrees"* — that is
//! `spikes/seed_trading_client.py`, which reads the same file with a reader
//! sharing no code with this one and asks omniORB.

use orbweaver_test::state::{MoeEstate, MoeExperts};

/// Every seed file parses, and parsing is strict.
///
/// Strict is the point: the loader refuses a missing member rather than
/// defaulting one, because `latency_p50: 0.0` as a stand-in for "nobody
/// measured it" did not merely fail to match — it matched *every* upper
/// bound, so a router selecting on `latency_p50 < 20` preferred exactly the
/// experts nobody had timed.
#[test]
fn the_seeds_load() {
    let experts = MoeExperts::load().expect("moe-experts.json loads");
    let estate = MoeEstate::load().expect("moe-estate.json loads");
    assert!(!experts.offers.is_empty(), "the expert population is not empty");
    assert!(!estate.nodes.is_empty(), "the estate declares nodes");
}

/// `null` survived the round trip as an absence, not as a zero.
#[test]
fn absent_is_absent_and_not_zero() {
    let experts = MoeExperts::load().unwrap();
    let untimed = experts.offer("untimed").expect("the population states `untimed`");
    assert_eq!(
        untimed.latency_p50, None,
        "`untimed` carries no latency_p50: that is what makes it the unranked case"
    );
    let unlabelled = experts.offer("unlabelled").expect("the population states `unlabelled`");
    assert_eq!(unlabelled.specialization, None, "`unlabelled` carries no specialization");

    // And the offers that do carry one really do, so the assertion above is
    // about this offer rather than about a loader that returns None for
    // everything.
    let fast = experts.offer("math-fast").unwrap();
    assert_eq!(fast.latency_p50, Some(2.0));
    assert_eq!(fast.specialization.as_deref(), Some("math"));
}

/// The 64-bit spelling crossed as a 64-bit value.
#[test]
fn a_sixty_four_bit_integer_kept_its_digits() {
    let experts = MoeExperts::load().unwrap();
    let fast = experts.offer("math-fast").unwrap();
    assert_eq!(fast.mem_footprint, 1_048_576);
    assert_eq!(fast.route_freq, 16);
}

/// **The invariant no fixture checks.**
///
/// `spike_tenants` declares its nodes and refuses an undeclared one
/// default-deny; `spike_experts` places every expert on `gpu-04`, which
/// `spike_tenants` never declared; the trading fixture uses `node-a`, which
/// neither declared. As three populations that is three separate facts and
/// nothing can be wrong. As **one** population it is a single question with a
/// yes-or-no answer, and this is it being asked for the first time.
#[test]
fn every_offer_is_placed_on_a_declared_node() {
    let experts = MoeExperts::load().unwrap();
    let estate = MoeEstate::load().unwrap();

    let undeclared: Vec<&str> = experts
        .offers
        .iter()
        .filter(|o| !estate.declares(&o.placement_node))
        .map(|o| o.placement_node.as_str())
        .collect();

    assert!(
        undeclared.is_empty(),
        "these offers are placed on nodes corpus/state/moe-estate.json does not declare: {undeclared:?}. \
         Either the estate is missing a node or the offer is placed nowhere — and which of the two \
         it is, is exactly the question three fixtures could not ask."
    );
}

/// The probe really is undeclared, so a residency check shown refusing it is
/// refusing something.
///
/// Without this, `_undeclared_node_probe` could quietly be added to `nodes`
/// and every default-deny demonstration built on it would keep passing while
/// demonstrating nothing.
#[test]
fn the_undeclared_probe_is_undeclared() {
    let estate = MoeEstate::load().unwrap();
    assert!(
        !estate.declares(&estate.undeclared_probe),
        "`{}` is named as the undeclared-node probe but the estate declares it",
        estate.undeclared_probe
    );
}

/// Every specialization an offer carries is a word the estate's vocabulary
/// knows.
#[test]
fn specializations_come_from_the_stated_vocabulary() {
    let experts = MoeExperts::load().unwrap();
    let estate = MoeEstate::load().unwrap();
    for offer in &experts.offers {
        if let Some(spec) = &offer.specialization {
            assert!(
                estate.capability_vocabulary.contains(spec),
                "offer `{}` is specialized in `{spec}`, which is not in the estate's \
                 capability vocabulary {:?}",
                offer.id,
                estate.capability_vocabulary
            );
        }
    }
}

/// **The stated order agrees with the stated values.**
///
/// This is the check that makes the wire check worth running. `expect_ids` is
/// a third statement, written independently of both the population above it
/// and the ranker that will be asked to reproduce it — so a wrong expectation
/// and a wrong ranker cannot cancel, because they would each have to agree
/// with a set of property values neither of them wrote.
///
/// It does **not** evaluate the constraint: that would be a second copy of
/// `orbweaver-trading`'s engine living in a test, and a check that
/// reimplements what it checks is not a check. It asks only the question a
/// file can answer about itself — given these ids, in this stated order, are
/// they sorted by the property the entry says they are sorted by?
#[test]
fn a_stated_order_is_sorted_by_the_property_it_claims() {
    let experts = MoeExperts::load().unwrap();
    let mut checked = 0;

    for q in &experts.queries {
        let Some(key) = &q.order_by else { continue };
        if !q.ordered {
            continue;
        }
        let values: Vec<(String, Option<f64>)> = q
            .expect_ids
            .iter()
            .map(|id| {
                let o = experts
                    .offer(id)
                    .unwrap_or_else(|| panic!("query `{}` expects `{id}`, which the population does not state", q.name));
                let v = match key.as_str() {
                    "cost" => Some(o.cost),
                    "latency_p50" => o.latency_p50,
                    "latency_p99" => Some(o.latency_p99),
                    "load" => Some(o.load),
                    other => panic!("query `{}` orders by `{other}`, which this check does not know how to read", q.name),
                };
                (o.id.clone(), v)
            })
            .collect();

        // The ranked prefix ascends, and everything the preference could not
        // place is behind it. Both halves matter: `unranked` offers go last
        // rather than being dropped, because dropping them would make the
        // answer report fewer matches than matched — as false as truncating.
        let ranked: Vec<f64> = values.iter().filter_map(|(_, v)| *v).collect();
        assert!(
            ranked.windows(2).all(|w| w[0] <= w[1]),
            "query `{}` states the order {:?}, but by `{key}` those offers are {:?} — \
             the stated order and the stated property values disagree",
            q.name,
            q.expect_ids,
            values
        );

        let first_unranked = values.iter().position(|(_, v)| v.is_none());
        if let Some(cut) = first_unranked {
            assert!(
                values[cut..].iter().all(|(_, v)| v.is_none()),
                "query `{}` states the order {:?}, but an offer carrying a `{key}` sits \
                 *after* one that carries none — the unrankable ones go last, together",
                q.name,
                q.expect_ids
            );
        }

        // An id the entry names as unranked must be unranked *for the stated
        // reason* — it carries no value for the ordering key — and must
        // actually be present, since dropping it is the failure this whole
        // entry is about.
        for id in &q.expect_unranked_last {
            let o = experts.offer(id).unwrap_or_else(|| {
                panic!("query `{}` names `{id}`, which the population does not state", q.name)
            });
            let missing = match key.as_str() {
                "cost" => false,
                "latency_p50" => o.latency_p50.is_none(),
                "latency_p99" | "load" => false,
                _ => false,
            };
            assert!(
                missing,
                "query `{}` says `{id}` cannot be ranked by `{key}`, but `{id}` carries a \
                 `{key}` — so it would be ranked, and the entry is wrong",
                q.name
            );
            let at = q.expect_ids.iter().position(|e| e == id).unwrap_or_else(|| {
                panic!(
                    "query `{}` says `{id}` is unranked-and-last, but expect_ids {:?} does not \
                     contain it at all — an unrankable offer goes last, it is not dropped",
                    q.name, q.expect_ids
                )
            });
            assert_eq!(
                at,
                q.expect_ids.len() - q.expect_unranked_last.len()
                    + q.expect_unranked_last.iter().position(|e| e == id).unwrap(),
                "query `{}` places `{id}` at {at} in {:?}, but the unrankable offers belong at \
                 the tail",
                q.name,
                q.expect_ids
            );
        }
        checked += 1;
    }

    // A floor, and the comment is the rationale rather than a figure: if
    // every ordered entry were deleted this test would pass over an empty
    // loop, which is the green-while-measuring-nothing class.
    assert!(checked >= 3, "expected at least three ordered queries to check, checked {checked}");
}

/// The population states enough offers for a query to fit under one
/// `how_many` and not under another — the distinction D022 §5 is about.
#[test]
fn the_population_is_large_enough_to_be_truncated() {
    let experts = MoeExperts::load().unwrap();
    assert!(
        experts.offers.len() >= 3,
        "a population of fewer than three offers cannot show a bound being exceeded"
    );
    let ids: Vec<&str> = experts.offers.iter().map(|o| o.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "offer ids are unique: {ids:?}");
}
