//! The §6 loading policy: who stays resident, who gets prefetched, who is
//! evicted — decided from offer properties and free accelerator memory,
//! deterministically.
//!
//! §6 asks for a prefetch driven by "Markov/n-gram prediction over the
//! routing history". Honestly: no such predictor exists here. What exists is
//! the decayed counter — `route_freq` *is* the recency-weighted history —
//! and prefetch ranks by the §6 score, which that counter feeds. A real
//! sequence predictor would slot in as a different ranking; nothing about
//! one is claimed until it exists and is trace-tested.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::{Offer, OfferStore, Residency};

/// The knobs of §6's residency rules. Watermarks are free-memory thresholds
/// in bytes: below `low_watermark` the policy evicts, above
/// `high_watermark` it may prefetch, and in between it holds still. A
/// policy with `low_watermark > high_watermark` is self-contradictory;
/// [`LoadingPolicy::decide`] resolves it by checking the eviction branch
/// first, so memory safety wins over speculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingPolicy {
    /// The `affinity_weight` in `score = route_freq × affinity_weight ÷
    /// mem_footprint`. One weight for the whole store, so today it cancels
    /// out of every score *comparison* (and a weight of `0` makes every
    /// score zero, disabling prefetch entirely). It is kept because §6
    /// names it and because per-offer gate affinity is the obvious next
    /// occupant of the slot.
    pub affinity_weight: u32,
    /// Free memory below this triggers eviction, in bytes.
    pub low_watermark: u64,
    /// Free memory above this permits prefetch, in bytes.
    pub high_watermark: u64,
}

/// One command for the (future, F3) residency state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Offload this expert: RESIDENT → OFFLOADED.
    Evict(String),
    /// Start loading this expert: OFFLOADED → PREFETCHING.
    Prefetch(String),
}

impl LoadingPolicy {
    /// Compares two offers by the §6 residency score, exactly.
    ///
    /// `score = route_freq × affinity_weight ÷ mem_footprint` is compared by
    /// cross-multiplication in `u128` — `(freq_a × mem_b)` against
    /// `(freq_b × mem_a)`, both at most `u64::MAX²`, which fits — so there
    /// is no division, no rounding, and no float anywhere in the ranking.
    /// A `mem_footprint` of zero is clamped to one byte: a free expert
    /// scores as high as the formula allows rather than dividing by zero.
    /// With `affinity_weight == 0` every score is zero and all offers
    /// compare equal.
    pub fn score_cmp(&self, a: &Offer, b: &Offer) -> Ordering {
        if self.affinity_weight == 0 {
            return Ordering::Equal;
        }
        let lhs = u128::from(a.route_freq) * u128::from(b.mem_footprint.max(1));
        let rhs = u128::from(b.route_freq) * u128::from(a.mem_footprint.max(1));
        lhs.cmp(&rhs)
    }

    /// The §6 decision, from the store's current offers and the observed
    /// free accelerator memory. Deterministic: the same store contents,
    /// free memory and inflight set always yield the same decision list,
    /// whatever order the offers were registered in.
    ///
    /// **Eviction** (only when `free_memory < low_watermark`): resident,
    /// unpinned, not-inflight offers are offloaded in LFU order — lowest
    /// `route_freq` first, ties by id — until the projected free memory
    /// reaches the low watermark. `ACTIVE` and `PREFETCHING` offers are
    /// never candidates: active is inflight by definition, and cancelling a
    /// load in flight belongs to the state machine, not to this policy.
    ///
    /// **Prefetch** (only when `free_memory > high_watermark`): offloaded
    /// offers with routing history (`route_freq > 0` — no history, no
    /// prediction) are loaded best score first, ties by id, spending only
    /// the memory above the high watermark. An offer too big for the
    /// remaining budget is skipped and the next one still considered, so a
    /// large cold-ish expert cannot block a small hot one.
    pub fn decide(
        &self,
        store: &OfferStore,
        free_memory: u64,
        inflight: &BTreeSet<String>,
    ) -> Vec<Decision> {
        let mut out = Vec::new();
        if free_memory < self.low_watermark {
            let mut candidates: Vec<&Offer> = store
                .iter()
                .filter(|o| o.residency == Residency::Resident)
                .filter(|o| !store.is_pinned(&o.id))
                .filter(|o| !inflight.contains(&o.id))
                .collect();
            candidates
                .sort_by(|a, b| a.route_freq.cmp(&b.route_freq).then_with(|| a.id.cmp(&b.id)));
            let mut projected = free_memory;
            for offer in candidates {
                if projected >= self.low_watermark {
                    break;
                }
                projected = projected.saturating_add(offer.mem_footprint);
                out.push(Decision::Evict(offer.id.clone()));
            }
        } else if free_memory > self.high_watermark {
            let mut candidates: Vec<&Offer> = store
                .iter()
                .filter(|o| o.residency == Residency::Offloaded)
                .filter(|o| o.route_freq > 0 && self.affinity_weight > 0)
                .collect();
            candidates.sort_by(|a, b| self.score_cmp(b, a).then_with(|| a.id.cmp(&b.id)));
            let mut budget = free_memory - self.high_watermark;
            for offer in candidates {
                if offer.mem_footprint <= budget {
                    budget -= offer.mem_footprint;
                    out.push(Decision::Prefetch(offer.id.clone()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &str, residency: Residency, footprint: u64, freq: u64) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: "math".to_owned(),
            cost: 1.0,
            latency_p50: 10.0,
            latency_p99: 50.0,
            load: 0.5,
            residency,
            mem_footprint: footprint,
            placement_node: "node-a".to_owned(),
            route_freq: freq,
        }
    }

    fn store(offers: &[Offer]) -> OfferStore {
        let mut s = OfferStore::new();
        for o in offers {
            s.register(o.clone()).expect("fixture registers");
        }
        s
    }

    fn policy() -> LoadingPolicy {
        LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 }
    }

    fn no_inflight() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn between_the_watermarks_the_policy_holds_still() {
        let s = store(&[
            offer("expert-a", Residency::Resident, 100, 1),
            offer("expert-b", Residency::Offloaded, 100, 99),
        ]);
        for free in [100, 250, 400] {
            assert_eq!(policy().decide(&s, free, &no_inflight()), Vec::new(), "free={free}");
        }
    }

    /// LFU under pressure: evictions go lowest-freq first and stop as soon
    /// as the projected free memory reaches the low watermark.
    #[test]
    fn eviction_is_lfu_and_stops_at_the_watermark() {
        let s = store(&[
            offer("expert-cold", Residency::Resident, 30, 8),
            offer("expert-cool", Residency::Resident, 30, 16),
            offer("expert-hot", Residency::Resident, 500, 80),
        ]);
        // free 50 < low 100; evicting cold (30) projects 80, still short;
        // cool projects 110 >= 100; hot must survive.
        let got = policy().decide(&s, 50, &no_inflight());
        assert_eq!(
            got,
            vec![
                Decision::Evict("expert-cold".to_owned()),
                Decision::Evict("expert-cool".to_owned()),
            ]
        );
    }

    /// The naive answers are both wrong here: pure LFU would evict the
    /// coldest expert (inflight), and evict-the-biggest would take the
    /// hottest (biggest footprint). §6 takes the coldest *evictable* one.
    #[test]
    fn the_inflight_guard_redirects_eviction_to_the_next_coldest() {
        let s = store(&[
            offer("expert-cold", Residency::Resident, 50, 8),
            offer("expert-mid", Residency::Resident, 60, 32),
            offer("expert-hot", Residency::Resident, 500, 80),
        ]);
        let inflight = BTreeSet::from(["expert-cold".to_owned()]);
        let got = policy().decide(&s, 50, &inflight);
        assert_eq!(got, vec![Decision::Evict("expert-mid".to_owned())]);
    }

    #[test]
    fn a_pinned_expert_is_never_evicted() {
        let mut s = store(&[
            offer("expert-cold", Residency::Resident, 60, 8),
            offer("expert-mid", Residency::Resident, 60, 32),
        ]);
        s.pin("expert-cold");
        let got = policy().decide(&s, 50, &no_inflight());
        assert_eq!(got, vec![Decision::Evict("expert-mid".to_owned())]);
    }

    /// ACTIVE is inflight by definition and PREFETCHING belongs to the
    /// state machine: neither is ever an eviction candidate, even when
    /// nothing else can reach the watermark.
    #[test]
    fn active_and_prefetching_offers_are_not_eviction_candidates() {
        let s = store(&[
            offer("expert-active", Residency::Active, 500, 1),
            offer("expert-loading", Residency::Prefetching, 500, 1),
        ]);
        assert_eq!(policy().decide(&s, 0, &no_inflight()), Vec::new());
    }

    /// Prefetch is by score, and score divides by footprint: the smaller
    /// expert with fewer hits outranks the bigger one with more. The
    /// too-big candidate is skipped, not a roadblock.
    #[test]
    fn prefetch_ranks_by_score_and_skips_what_does_not_fit() {
        let s = store(&[
            // score 16/60 ≈ 0.267
            offer("expert-mid", Residency::Offloaded, 60, 16),
            // score 24/100 = 0.24 — hotter but bulkier, so it ranks second…
            offer("expert-next", Residency::Offloaded, 100, 24),
            // …and at budget 120-60=60 it no longer fits; the tiny one after
            // it must still be considered. score 1/10 = 0.1.
            offer("expert-tiny", Residency::Offloaded, 10, 1),
        ]);
        let got = policy().decide(&s, 520, &no_inflight());
        assert_eq!(
            got,
            vec![
                Decision::Prefetch("expert-mid".to_owned()),
                Decision::Prefetch("expert-tiny".to_owned()),
            ]
        );
    }

    /// No history, no prediction: an offloaded expert that was never routed
    /// is not prefetched however much memory is free.
    #[test]
    fn an_expert_with_no_routing_history_is_never_prefetched() {
        let s = store(&[offer("expert-unrouted", Residency::Offloaded, 10, 0)]);
        assert_eq!(policy().decide(&s, 10_000, &no_inflight()), Vec::new());
    }

    /// A zero affinity weight zeroes every score: prefetch is disabled,
    /// eviction (which is LFU, not score) still works.
    #[test]
    fn zero_affinity_weight_disables_prefetch_but_not_eviction() {
        let p = LoadingPolicy { affinity_weight: 0, ..policy() };
        let s = store(&[
            offer("expert-a", Residency::Offloaded, 10, 99),
            offer("expert-b", Residency::Resident, 200, 1),
        ]);
        assert_eq!(p.decide(&s, 10_000, &no_inflight()), Vec::new());
        assert_eq!(p.decide(&s, 0, &no_inflight()), vec![Decision::Evict("expert-b".to_owned())]);
    }

    /// A zero footprint is clamped to one byte in the score, not a divide
    /// by zero: the weightless expert outranks anything of equal freq.
    #[test]
    fn a_zero_footprint_offer_scores_highest_not_undefined() {
        let p = policy();
        let weightless = offer("expert-zero", Residency::Offloaded, 0, 16);
        let heavy = offer("expert-heavy", Residency::Offloaded, 1000, 16);
        assert_eq!(p.score_cmp(&weightless, &heavy), Ordering::Greater);
        assert_eq!(p.score_cmp(&heavy, &weightless), Ordering::Less);
        let equal = offer("expert-one", Residency::Offloaded, 1, 16);
        assert_eq!(p.score_cmp(&weightless, &equal), Ordering::Equal, "clamped to one byte");
    }

    /// Registration order must not leak into decisions: the same offers
    /// registered in opposite orders decide identically, twice each.
    #[test]
    fn decisions_are_independent_of_registration_order_and_repeatable() {
        let offers = [
            offer("expert-a", Residency::Offloaded, 60, 16),
            offer("expert-b", Residency::Offloaded, 100, 24),
            offer("expert-c", Residency::Resident, 40, 4),
        ];
        let mut reversed = offers.clone();
        reversed.reverse();
        let (s1, s2) = (store(&offers), store(&reversed));
        let p = policy();
        let first = p.decide(&s1, 520, &no_inflight());
        assert_eq!(first, p.decide(&s1, 520, &no_inflight()));
        assert_eq!(first, p.decide(&s2, 520, &no_inflight()));
        assert!(!first.is_empty(), "the fixture must actually decide something");
    }
}
