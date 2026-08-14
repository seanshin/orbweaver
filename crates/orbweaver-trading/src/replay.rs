//! Trace replay: the loading policy evaluated over a recorded routing
//! trace, deterministically.
//!
//! A trace is the control-plane view of a run: routing hits, call
//! completions, and batch-period memory snapshots — no clock, no tokens,
//! no hardware. Replaying one drives the §6 feedback loop end to end
//! (hits → `route_freq` → decisions → residency) and produces a
//! [`PolicyReport`] whose decision list tests pin verbatim. This is the
//! only footing on which this crate makes hit-rate claims (PLAN-MOE §5):
//! a rate over a deterministic trace, reported as an exact rational, never
//! a measurement of an accelerator this repository does not have.
//!
//! Two modelling simplifications, stated rather than hidden:
//!
//! - **Free memory is read from the trace, never simulated.** The
//!   [`TraceEvent::Memory`] snapshots are the ground truth a real system
//!   would observe; deriving them from footprints would let the model
//!   drift from what was recorded.
//! - **A prefetch decision takes effect within its window**: the replay
//!   applies `Prefetch` as OFFLOADED → RESIDENT directly, because the
//!   PREFETCHING transient belongs to the F3 state machine and no loader
//!   exists here to spend time in it. `Evict` likewise applies as
//!   RESIDENT → OFFLOADED.

use std::collections::{BTreeMap, BTreeSet};

use crate::policy::{Decision, LoadingPolicy};
use crate::{OfferStore, Residency};

/// One recorded control-plane event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    /// The router selected this expert for a call. Counts toward the hit
    /// rate (a hit iff the expert is RESIDENT or ACTIVE at this moment),
    /// adds one hit to its `route_freq`, and holds the call inflight until
    /// the matching [`TraceEvent::Complete`].
    Route {
        /// The routed expert's offer id.
        id: String,
    },
    /// A previously routed call on this expert finished.
    Complete {
        /// The expert whose call finished.
        id: String,
    },
    /// A batch-period snapshot: the observed free accelerator memory, in
    /// bytes. Closes the window — the policy decides, decisions apply,
    /// then every `route_freq` decays by [`crate::DECAY`].
    Memory {
        /// Free accelerator memory at the snapshot, in bytes.
        free: u64,
    },
}

/// Why a trace could not be replayed. An incoherent trace refuses rather
/// than guesses — the unmeasured-check rule applied to inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayError {
    /// Index of the offending event in the trace.
    pub at_event: usize,
    /// What was wrong with it, phrased as something to fix.
    pub message: String,
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "event {}: {}", self.at_event, self.message)
    }
}

impl std::error::Error for ReplayError {}

/// What one memory snapshot decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDecisions {
    /// The free memory the snapshot reported.
    pub free_memory: u64,
    /// The decisions the policy made from it, in decision order.
    pub decisions: Vec<Decision>,
}

/// The outcome of a replay: every snapshot's decisions, and the hit
/// accounting for the whole trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyReport {
    /// One entry per [`TraceEvent::Memory`], in trace order.
    pub snapshots: Vec<SnapshotDecisions>,
    /// How many routing events the trace held.
    pub routed: u64,
    /// How many of them found the expert RESIDENT or ACTIVE.
    pub hits: u64,
}

impl PolicyReport {
    /// The hit rate as the exact rational `(hits, routed)`. A rational and
    /// not a float, so the honest answer for an empty trace is `(0, 0)`
    /// rather than a NaN dressed up as a rate.
    pub fn hit_rate(&self) -> (u64, u64) {
        (self.hits, self.routed)
    }
}

/// Replays `events` over `store`, mutating it (counters, residency) as the
/// trace unfolds. Same policy, same starting store, same trace: same
/// report and same final store, every run.
pub fn replay(
    policy: &LoadingPolicy,
    store: &mut OfferStore,
    events: &[TraceEvent],
) -> Result<PolicyReport, ReplayError> {
    let mut inflight: BTreeMap<String, u64> = BTreeMap::new();
    let mut report = PolicyReport { snapshots: Vec::new(), routed: 0, hits: 0 };
    for (i, event) in events.iter().enumerate() {
        match event {
            TraceEvent::Route { id } => {
                let Some(offer) = store.get(id) else {
                    return Err(ReplayError {
                        at_event: i,
                        message: format!("route to {id:?}, which is not in the offer store"),
                    });
                };
                report.routed += 1;
                if matches!(offer.residency, Residency::Resident | Residency::Active) {
                    report.hits += 1;
                }
                store.add_hit(id);
                *inflight.entry(id.clone()).or_insert(0) += 1;
            }
            TraceEvent::Complete { id } => match inflight.get_mut(id) {
                Some(n) => {
                    *n -= 1;
                    if *n == 0 {
                        inflight.remove(id);
                    }
                }
                None => {
                    return Err(ReplayError {
                        at_event: i,
                        message: format!("completion for {id:?} with no call inflight"),
                    });
                }
            },
            TraceEvent::Memory { free } => {
                let inflight_ids: BTreeSet<String> = inflight.keys().cloned().collect();
                let decisions = policy.decide(store, *free, &inflight_ids);
                for decision in &decisions {
                    match decision {
                        Decision::Evict(id) => {
                            store.set_residency(id, Residency::Offloaded);
                        }
                        Decision::Prefetch(id) => {
                            store.set_residency(id, Residency::Resident);
                        }
                    }
                }
                store.decay_all();
                report.snapshots.push(SnapshotDecisions { free_memory: *free, decisions });
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Offer;

    fn offer(id: &str, residency: Residency, footprint: u64) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: Some("math".to_owned()),
            cost: 1.0,
            latency_p50: Some(10.0),
            latency_p99: 50.0,
            load: 0.5,
            residency,
            mem_footprint: footprint,
            placement_node: "node-a".to_owned(),
            route_freq: 0,
        }
    }

    fn route(id: &str) -> TraceEvent {
        TraceEvent::Route { id: id.to_owned() }
    }

    fn done(id: &str) -> TraceEvent {
        TraceEvent::Complete { id: id.to_owned() }
    }

    fn call(id: &str, times: usize) -> Vec<TraceEvent> {
        let mut v = Vec::new();
        for _ in 0..times {
            v.push(route(id));
            v.push(done(id));
        }
        v
    }

    /// The flagship pin: a full small trace whose decision list a naive
    /// policy gets wrong twice over.
    ///
    /// At the pressure snapshot the coldest expert (`expert-cold`, freq 16)
    /// has a call inflight and the hottest (`expert-hot`, freq 80) has the
    /// biggest footprint — so naive LFU would evict cold (guard violation)
    /// and evict-the-biggest would take hot (the one §6's score most wants
    /// resident). The pinned answer is the middle one. At the idle
    /// snapshot, `expert-next` has higher decayed freq than the evicted
    /// `expert-mid` (24 vs 16) but a worse *score* (24/100 < 16/60), so
    /// prefetch takes mid and skips next — the ÷footprint half of the
    /// formula doing its job.
    #[test]
    fn a_full_trace_replays_to_the_pinned_decision_list() {
        let mut store = OfferStore::new();
        store.register(offer("expert-hot", Residency::Resident, 200)).expect("registers");
        store.register(offer("expert-mid", Residency::Resident, 60)).expect("registers");
        store.register(offer("expert-cold", Residency::Resident, 50)).expect("registers");
        store.register(offer("expert-next", Residency::Offloaded, 100)).expect("registers");
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };

        let mut events = Vec::new();
        events.extend(call("expert-hot", 5)); // freq 80
        events.extend(call("expert-mid", 2)); // freq 32
        events.push(route("expert-cold")); // freq 16, and inflight across the snapshot
        events.extend(call("expert-next", 3)); // freq 48, all misses (offloaded)
        events.push(TraceEvent::Memory { free: 80 }); // pressure: 80 < low 100
        events.push(done("expert-cold"));
        events.push(TraceEvent::Memory { free: 520 }); // idle: 520 > high 400
        events.push(route("expert-mid")); // back resident: a hit
        events.push(done("expert-mid"));

        let report = replay(&policy, &mut store, &events).expect("a coherent trace replays");

        assert_eq!(
            report.snapshots,
            vec![
                SnapshotDecisions {
                    free_memory: 80,
                    decisions: vec![Decision::Evict("expert-mid".to_owned())],
                },
                SnapshotDecisions {
                    free_memory: 520,
                    decisions: vec![Decision::Prefetch("expert-mid".to_owned())],
                },
            ]
        );

        // Hits: hot 5 + mid 2 + cold 1 (all resident) + mid again after the
        // prefetch; misses: the 3 routes to offloaded expert-next.
        assert_eq!(report.hit_rate(), (9, 12));

        // The store carries the outcome: mid is resident again with its
        // history decayed twice then bumped once (32/2/2 + 16 = 24); hot
        // stayed resident throughout (80/2/2 = 20).
        let mid = store.get("expert-mid").expect("present");
        assert_eq!((mid.residency, mid.route_freq), (Residency::Resident, 24));
        let hot = store.get("expert-hot").expect("present");
        assert_eq!((hot.residency, hot.route_freq), (Residency::Resident, 20));
        let cold = store.get("expert-cold").expect("present");
        assert_eq!((cold.residency, cold.route_freq), (Residency::Resident, 4));
        let next = store.get("expert-next").expect("present");
        assert_eq!((next.residency, next.route_freq), (Residency::Offloaded, 12));
    }

    /// The same trace replayed from the same starting point twice gives the
    /// same report — the property every pin above stands on, held directly.
    #[test]
    fn a_replay_is_reproducible() {
        let policy = LoadingPolicy { affinity_weight: 3, low_watermark: 64, high_watermark: 256 };
        let mut events = vec![route("expert-a")];
        events.extend(call("expert-b", 4));
        events.push(TraceEvent::Memory { free: 32 });
        events.push(done("expert-a"));
        events.push(TraceEvent::Memory { free: 512 });

        let build = || {
            let mut s = OfferStore::new();
            s.register(offer("expert-a", Residency::Resident, 40)).expect("registers");
            s.register(offer("expert-b", Residency::Offloaded, 30)).expect("registers");
            s
        };
        let (mut s1, mut s2) = (build(), build());
        let r1 = replay(&policy, &mut s1, &events).expect("replays");
        let r2 = replay(&policy, &mut s2, &events).expect("replays");
        assert_eq!(r1, r2);
        assert_eq!(s1.get("expert-a"), s2.get("expert-a"));
        assert_eq!(s1.get("expert-b"), s2.get("expert-b"));
    }

    /// A miss stays a miss until the policy prefetches: routing to an
    /// offloaded expert does not quietly make it resident.
    #[test]
    fn routing_to_an_offloaded_expert_is_a_miss_and_loads_nothing() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 0, high_watermark: 1000 };
        let mut store = OfferStore::new();
        store.register(offer("expert-a", Residency::Offloaded, 10)).expect("registers");
        let report = replay(&policy, &mut store, &call("expert-a", 2)).expect("replays");
        assert_eq!(report.hit_rate(), (0, 2));
        assert_eq!(store.get("expert-a").expect("present").residency, Residency::Offloaded);
    }

    #[test]
    fn an_empty_trace_reports_zero_over_zero_not_a_rate() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 0, high_watermark: 0 };
        let report = replay(&policy, &mut OfferStore::new(), &[]).expect("replays");
        assert_eq!(report.hit_rate(), (0, 0));
        assert!(report.snapshots.is_empty());
    }

    #[test]
    fn a_route_to_an_unregistered_expert_refuses_naming_event_and_id() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 0, high_watermark: 0 };
        let mut store = OfferStore::new();
        store.register(offer("expert-a", Residency::Resident, 10)).expect("registers");
        let events = vec![route("expert-a"), route("expert-ghost")];
        let err = replay(&policy, &mut store, &events).unwrap_err();
        assert_eq!(err.at_event, 1);
        assert!(err.message.contains("expert-ghost"), "{err}");
    }

    #[test]
    fn a_completion_with_nothing_inflight_refuses() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 0, high_watermark: 0 };
        let mut store = OfferStore::new();
        store.register(offer("expert-a", Residency::Resident, 10)).expect("registers");
        // The second completion has nothing left to complete.
        let events = vec![route("expert-a"), done("expert-a"), done("expert-a")];
        let err = replay(&policy, &mut store, &events).unwrap_err();
        assert_eq!(err.at_event, 2);
        assert!(err.message.contains("no call inflight"), "{err}");
    }
}
