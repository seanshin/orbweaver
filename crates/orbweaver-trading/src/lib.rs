//! The Trading Service: the MoE control plane's loading and placement
//! decision engine. PLAN-MOE §3 F2, implementing `docs/CORBAMoEArchitecture.md`
//! §4.3 (service offers, constraint queries) and §6 (the loading policy).
//!
//! # What this is, and honestly is not
//!
//! This crate is the **decision engine only**. There is no wire surface yet:
//! binding the IDL `ExpertRegistry`/`ExpertLoader` contracts
//! (`corpus/moe/moe.idl`) to our server is a later batch, as is the residency
//! state machine on the POA (F3) — [`policy::Decision`]s are commands *for*
//! that machine, not the machine. Selection results cross the MCP face as
//! capability handles, never IORs (integration point IF1) — also later. No
//! accelerator, kernel or RDMA path exists in this repository; residency and
//! hit rate here are statements about **deterministic recorded traces**
//! (PLAN-MOE §5), never about hardware.
//!
//! # Determinism discipline
//!
//! Mirrors `orbweaver-mcp`'s promotion engine: there is no clock in scope,
//! and nothing *accumulated* is a float. [`Offer::route_freq`] — the one
//! quantity this crate updates over time — is an integer decayed counter
//! ([`FREQ_SCALE`] units per hit, decayed by the fixed rational [`DECAY`] per
//! batch window), so the same trace produces the same counters bit for bit on
//! every run and every platform. The IDL `Capability` declares `route_freq`
//! as a `float`; converting at the (future) wire boundary is that batch's
//! problem, and losing fractional hits there is acceptable — losing
//! reproducibility here is not. The remaining float fields (`cost`,
//! latencies, `load`) are static properties that are *compared*, never
//! accumulated, and every comparison uses IEEE total order — deterministic,
//! just not something a replayed trace ever feeds back into.

#![deny(missing_docs)]

pub mod policy;
pub mod query;
pub mod replay;

use std::collections::{BTreeMap, BTreeSet};

/// Where an expert's weights currently live, mirroring `moe::Residency`
/// (`corpus/moe/moe.idl`). The declaration order is the loading progression,
/// and it is also the order the derived `Ord` (and therefore every query
/// comparison on `residency`) uses: `OFFLOADED < PREFETCHING < RESIDENT <
/// ACTIVE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Residency {
    /// Weights are in storage; a call would have to wait for a load.
    Offloaded,
    /// A load is underway (the F3 state machine's transient state).
    Prefetching,
    /// Weights are in accelerator memory and callable.
    Resident,
    /// The data plane is actively computing on this expert. Never an
    /// eviction candidate: active is inflight by definition.
    Active,
}

/// How much one routing hit adds to [`Offer::route_freq`].
///
/// The scale exists so the integer decay keeps resolution: at 16 units per
/// hit and a decay of 1/2, a single hit stays visible for exactly four batch
/// windows before flooring to zero — a documented horizon, not float drift.
pub const FREQ_SCALE: u64 = 16;

/// The decay applied to every `route_freq` at each batch window, as the
/// fixed rational (numerator, denominator): `freq' = freq × 1 / 2`, integer
/// division, flooring. A rational rather than an `f64` because repeated
/// multiplication is exactly where floats stop being reproducible.
pub const DECAY: (u64, u64) = (1, 2);

/// One service offer: the properties an expert registers and heartbeats,
/// following the §4.3 Service Offer list. This is the store's mirror of the
/// IDL `Capability` — see the crate docs for why `route_freq` is an integer
/// here.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    /// The capability id the expert registered under (`moe::CapabilityId`).
    pub id: String,
    /// What the expert is for: `math`, `code`, `retrieval`, `vision`, …
    pub specialization: String,
    /// Relative invocation cost.
    pub cost: f64,
    /// Median latency, in milliseconds (the IDL suffixes the unit; queries
    /// use bare numbers and this contract).
    pub latency_p50: f64,
    /// 99th-percentile latency, in milliseconds.
    pub latency_p99: f64,
    /// Current load, `0.0..=1.0` by convention.
    pub load: f64,
    /// Where the weights are right now.
    pub residency: Residency,
    /// Accelerator memory the expert occupies when resident, in bytes.
    pub mem_footprint: u64,
    /// The node the expert is (or would be) placed on.
    pub placement_node: String,
    /// The decayed routing counter, in [`FREQ_SCALE`] units per hit. Owned
    /// by the telemetry feedback loop (§6): [`OfferStore::heartbeat`]
    /// deliberately does not overwrite it.
    pub route_freq: u64,
}

/// Why the offer store refused an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    /// What went wrong, phrased as something to fix.
    pub message: String,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StoreError {}

/// The offer store: what the trading service knows about every expert.
///
/// Keyed by offer id in a `BTreeMap`, so every iteration — and therefore
/// every query result and every policy decision built on one — is in
/// ascending-id order regardless of registration order. Determinism is load
/// bearing here: PLAN-MOE §3 pins policy outcomes over recorded traces, and
/// a store that iterated in hash order would make those pins flaky.
#[derive(Debug, Default)]
pub struct OfferStore {
    offers: BTreeMap<String, Offer>,
    pinned: BTreeSet<String>,
}

impl OfferStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new offer. A duplicate id refuses — re-announcing an
    /// existing expert is [`OfferStore::heartbeat`]'s job, and silently
    /// replacing would let a re-registration reset the routing history.
    pub fn register(&mut self, offer: Offer) -> Result<(), StoreError> {
        if self.offers.contains_key(&offer.id) {
            return Err(StoreError {
                message: format!(
                    "offer {:?} is already registered; use heartbeat to update it",
                    offer.id
                ),
            });
        }
        self.offers.insert(offer.id.clone(), offer);
        Ok(())
    }

    /// Removes an offer, and its pin with it. Returns whether it existed.
    pub fn deregister(&mut self, id: &str) -> bool {
        self.pinned.remove(id);
        self.offers.remove(id).is_some()
    }

    /// Updates an existing offer's properties from a heartbeat.
    ///
    /// Every field is taken from `update` **except `route_freq`**, which
    /// stays the store's counter: §6's feedback loop owns that number, and a
    /// heartbeat that could overwrite it would let an expert erase (or
    /// invent) its own routing history. An unknown id refuses.
    pub fn heartbeat(&mut self, update: Offer) -> Result<(), StoreError> {
        match self.offers.get_mut(&update.id) {
            Some(existing) => {
                let kept_freq = existing.route_freq;
                *existing = update;
                existing.route_freq = kept_freq;
                Ok(())
            }
            None => Err(StoreError {
                message: format!(
                    "offer {:?} is not registered; heartbeat has nothing to update",
                    update.id
                ),
            }),
        }
    }

    /// The offer registered under `id`, if any.
    pub fn get(&self, id: &str) -> Option<&Offer> {
        self.offers.get(id)
    }

    /// Every offer, in ascending-id order — pinned, see the type docs.
    pub fn iter(&self) -> impl Iterator<Item = &Offer> {
        self.offers.values()
    }

    /// How many offers are registered.
    pub fn len(&self) -> usize {
        self.offers.len()
    }

    /// Whether no offers are registered.
    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    /// Pins `id` against eviction (§6: system and hot experts stay
    /// resident). Returns whether the offer exists; pinning an unknown id
    /// does nothing rather than recording a pin for an offer that may never
    /// arrive.
    pub fn pin(&mut self, id: &str) -> bool {
        if self.offers.contains_key(id) {
            self.pinned.insert(id.to_owned());
            true
        } else {
            false
        }
    }

    /// Removes a pin. Returns whether one was present.
    pub fn unpin(&mut self, id: &str) -> bool {
        self.pinned.remove(id)
    }

    /// Whether `id` is pinned against eviction.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned.contains(id)
    }

    /// Records one routing hit: `route_freq += FREQ_SCALE`, saturating —
    /// a counter that has somehow reached `u64::MAX / 16` hits has long
    /// since won every comparison it will ever be in. Returns whether the
    /// offer exists.
    pub fn add_hit(&mut self, id: &str) -> bool {
        match self.offers.get_mut(id) {
            Some(o) => {
                o.route_freq = o.route_freq.saturating_add(FREQ_SCALE);
                true
            }
            None => false,
        }
    }

    /// Closes a batch window: applies [`DECAY`] to every offer's
    /// `route_freq`. Integer arithmetic, flooring — the same history always
    /// decays to the same counters.
    pub fn decay_all(&mut self) {
        let (num, den) = DECAY;
        for o in self.offers.values_mut() {
            o.route_freq = o.route_freq * num / den;
        }
    }

    /// Sets an offer's residency, as the F3 state machine (or a replay
    /// applying decisions) reports transitions. Returns whether the offer
    /// exists.
    pub fn set_residency(&mut self, id: &str, residency: Residency) -> bool {
        match self.offers.get_mut(id) {
            Some(o) => {
                o.residency = residency;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &str, freq: u64) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: "math".to_owned(),
            cost: 1.0,
            latency_p50: 10.0,
            latency_p99: 50.0,
            load: 0.5,
            residency: Residency::Offloaded,
            mem_footprint: 64,
            placement_node: "node-a".to_owned(),
            route_freq: freq,
        }
    }

    /// Registration order must not leak into iteration order: the store is
    /// what every deterministic pin downstream stands on.
    #[test]
    fn iteration_is_ascending_by_id_whatever_the_registration_order() {
        let mut s = OfferStore::new();
        for id in ["expert-c", "expert-a", "expert-b"] {
            s.register(offer(id, 0)).expect("registers");
        }
        let ids: Vec<&str> = s.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["expert-a", "expert-b", "expert-c"]);
        assert_eq!(s.len(), 3);
        assert!(!s.is_empty());
    }

    #[test]
    fn a_duplicate_registration_refuses_and_names_the_id() {
        let mut s = OfferStore::new();
        s.register(offer("expert-a", 5)).expect("first registration");
        let err = s.register(offer("expert-a", 0)).unwrap_err();
        assert!(err.message.contains("expert-a"), "{err}");
        // The refusal changed nothing: the original counter survives.
        assert_eq!(s.get("expert-a").expect("still there").route_freq, 5);
    }

    #[test]
    fn heartbeat_updates_properties_but_never_the_routing_counter() {
        let mut s = OfferStore::new();
        s.register(offer("expert-a", 48)).expect("registers");
        let mut update = offer("expert-a", 0); // an expert claiming a clean history
        update.load = 0.9;
        update.residency = Residency::Resident;
        s.heartbeat(update).expect("updates");
        let got = s.get("expert-a").expect("present");
        assert_eq!(got.load, 0.9);
        assert_eq!(got.residency, Residency::Resident);
        assert_eq!(got.route_freq, 48, "the feedback loop owns route_freq, not the heartbeat");
    }

    #[test]
    fn heartbeat_for_an_unknown_offer_refuses() {
        let mut s = OfferStore::new();
        let err = s.heartbeat(offer("expert-ghost", 0)).unwrap_err();
        assert!(err.message.contains("expert-ghost"), "{err}");
    }

    #[test]
    fn deregistration_removes_the_offer_and_its_pin() {
        let mut s = OfferStore::new();
        s.register(offer("expert-a", 0)).expect("registers");
        assert!(s.pin("expert-a"));
        assert!(s.deregister("expert-a"));
        assert!(!s.is_pinned("expert-a"));
        assert!(s.get("expert-a").is_none());
        assert!(!s.deregister("expert-a"), "a second deregistration reports absence");
    }

    #[test]
    fn pinning_an_unknown_id_records_nothing() {
        let mut s = OfferStore::new();
        assert!(!s.pin("expert-ghost"));
        assert!(!s.is_pinned("expert-ghost"));
        assert!(!s.unpin("expert-ghost"));
    }

    /// The documented decay horizon: one hit is 16 units and halves floor,
    /// so it is visible for exactly four windows and gone at the fifth.
    #[test]
    fn one_hit_decays_to_zero_in_exactly_four_windows() {
        let mut s = OfferStore::new();
        s.register(offer("expert-a", 0)).expect("registers");
        assert!(s.add_hit("expert-a"));
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push(s.get("expert-a").expect("present").route_freq);
            s.decay_all();
        }
        seen.push(s.get("expert-a").expect("present").route_freq);
        assert_eq!(seen, [16, 8, 4, 2, 1, 0]);
        assert!(!s.add_hit("expert-ghost"), "a hit on an unknown id reports absence");
    }
}
