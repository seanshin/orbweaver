//! The expert residency state machine, driven by the POA.
//!
//! `docs/CORBAMoEArchitecture.md` §4.2 makes the POA's `ServantActivator` the
//! expert loader, and §5 gives the machine it drives:
//!
//! ```text
//! OFFLOADED ──prefetch requested──▶ PREFETCHING ──load completed──▶ RESIDENT
//!     ▲                                                              │   ▲
//!     └──────────────── evict (guarded) ─────────────────────────────┘   │
//!                                          call begins ──▶ ACTIVE ───────┘
//!                                                       (call ends)
//! ```
//!
//! [`ExpertLoader`] is that machine over expert ids, and [`ExpertLocator`] is
//! the `ServantLocator` that puts it on a [`Poa`](crate::Poa). The states are
//! `orbweaver_trading::Residency` — the same enum the offer store and the §6
//! loading policy already speak, deliberately not a second copy of it — and
//! the commands are that crate's [`Decision`]s.
//!
//! # No per-token hook, by construction
//!
//! §5's last line is absolute: **transitions happen at batch and statistics
//! period, never per token.** Documenting that rule would not enforce it, so
//! the API has no shape a per-token caller could use. [`ExpertLoader::apply`]
//! takes a *slice* of decisions and a [`BatchStats`] window; there is no
//! callback to register, no `on_token`, no per-invocation hook of any kind,
//! and the only per-call surface — [`ExpertLoader::begin_call`] and
//! [`ExpertLoader::end_call`] — moves between RESIDENT and ACTIVE and can
//! neither load nor evict. A caller who wants token-period residency has to
//! call `apply` in a loop with hand-built decisions, which is visible at the
//! call site rather than hidden behind a hook. The absence is tested: see the
//! `compile_fail` doc test on [`ExpertLoader`].
//!
//! # What a "load" is here, honestly
//!
//! There is no accelerator, no weight file and no data plane in this
//! repository. A load is a state transition plus an opaque `Vec<u8>` the
//! loader keeps on the expert's behalf, and PERSISTENT lifespan means that
//! blob survives eviction (§4.2: `PERSISTENT` — 오프로딩 시 상태 보존).
//! Nothing here measures or predicts time: §11's targets (<5% control-plane
//! overhead, prefetch hiding load latency) need a data-plane simulator that
//! does not exist, so this module reports state-machine correctness and
//! nothing about latency (PLAN-MOE §5).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use orbweaver_trading::OfferStore;
use orbweaver_trading::policy::{Decision, LoadingPolicy};

use crate::{Lifespan, Located, ObjectId, Poa, ServantLocator};

pub use orbweaver_trading::Residency;

/// Which condition of the §5 eviction guard was not met.
///
/// Named individually because "eviction refused" without the reason is what a
/// caller cannot act on: pressure and route_freq say *wait for a colder
/// window*, inflight says *retry after the call*, and pinned says *never*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardCondition {
    /// Free accelerator memory is not under the low watermark. Evicting
    /// without pressure throws away a load nobody asked to reclaim.
    MemoryPressure,
    /// The expert's routing frequency has not fallen: it is still hot, and
    /// §6's LFU rule wants the coldest expert, not merely an evictable one.
    RouteFreqLow,
    /// A call is inflight. ACTIVE is exactly `inflight > 0`, and an ACTIVE
    /// expert refuses here rather than as a missing edge — §5 lists inflight
    /// as a guard condition, and a caller that asked to evict a busy expert
    /// wants to be told *why*, not that the request was malformed.
    NoInflight,
    /// The expert is pinned (§6: system and hot experts stay resident).
    Unpinned,
}

impl fmt::Display for GuardCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            GuardCondition::MemoryPressure => "free memory is not under the low watermark",
            GuardCondition::RouteFreqLow => "the routing frequency has not fallen",
            GuardCondition::NoInflight => "a call is inflight",
            GuardCondition::Unpinned => "the expert is pinned",
        };
        f.write_str(s)
    }
}

/// Why a residency transition did not happen.
///
/// Every refusal is one of these: a state machine that answers an illegal
/// request with a silent no-op is not a state machine, it is a map with
/// opinions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// No expert is registered under this id.
    Unknown {
        /// The id that was asked about.
        id: String,
    },
    /// An id was registered twice. Re-registering would reset the residency
    /// and drop any preserved state, so it refuses instead.
    Duplicate {
        /// The id already registered.
        id: String,
    },
    /// The machine has no such edge.
    Illegal {
        /// The expert asked to move.
        id: String,
        /// Where it actually is.
        from: Residency,
        /// Where the caller asked it to go.
        to: Residency,
    },
    /// RESIDENT → OFFLOADED exists, but the §5 guard refused this one.
    Guarded {
        /// The expert that stays resident.
        id: String,
        /// The first condition, in §5's order, that was not met.
        unmet: GuardCondition,
    },
    /// Adaptation state only exists while the weights are in memory.
    NotLoaded {
        /// The expert whose state was read or written.
        id: String,
        /// Where it actually is.
        state: Residency,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionError::Unknown { id } => {
                write!(f, "expert {id:?} is not registered with the loader")
            }
            TransitionError::Duplicate { id } => {
                write!(f, "expert {id:?} is already registered; registering again would reset it")
            }
            TransitionError::Illegal { id, from, to } => {
                write!(
                    f,
                    "expert {id:?} cannot move {from:?} → {to:?}: the machine has no such edge"
                )
            }
            TransitionError::Guarded { id, unmet } => {
                write!(f, "expert {id:?} stays resident: {unmet}")
            }
            TransitionError::NotLoaded { id, state } => {
                write!(f, "expert {id:?} is {state:?}, so its adaptation state is not in memory")
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// The batch-period statistics the eviction guard reads.
///
/// The loader knows its own inflight counters and pins; it cannot know free
/// accelerator memory or a decayed `route_freq`, both of which belong to the
/// trading store. Passing them per window — rather than caching them on the
/// loader — is what keeps the guard from deciding against a stale reading,
/// and the type name is the reminder that a *window* is the unit.
///
/// [`BatchStats::from_store`] derives both from the offer store. Deriving
/// `cold` some other way is allowed, but deriving it as "the ids the policy
/// chose to evict" makes the guard vacuous, which is the one thing it must
/// not be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchStats {
    /// Free accelerator memory is below the policy's low watermark.
    pub memory_pressure: bool,
    /// The ids whose decayed `route_freq` has fallen far enough to make them
    /// eviction candidates — "route_freq↓" in §5.
    pub cold: BTreeSet<String>,
}

impl BatchStats {
    /// A window with no memory pressure and nothing cold: nothing can be
    /// evicted under it.
    pub fn idle() -> Self {
        Self::default()
    }

    /// The window as the trading store sees it: pressure from `policy`'s low
    /// watermark against the observed `free_memory`, and cold from every
    /// offer whose `route_freq` is below `cold_below`.
    ///
    /// `cold_below` is in [`orbweaver_trading::FREQ_SCALE`] units — the same
    /// scale `route_freq` is counted in — so `2 * FREQ_SCALE` reads as "fewer
    /// than two hits' worth of history left".
    pub fn from_store(
        store: &OfferStore,
        policy: &LoadingPolicy,
        free_memory: u64,
        cold_below: u64,
    ) -> Self {
        Self {
            memory_pressure: free_memory < policy.low_watermark,
            cold: store
                .iter()
                .filter(|o| o.route_freq < cold_below)
                .map(|o| o.id.clone())
                .collect(),
        }
    }
}

/// What one decision from a batch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The decision as the policy issued it.
    pub decision: Decision,
    /// The state the expert reached, or why it did not move. A refusal is
    /// reported, never swallowed: the trading store's residency mirror is
    /// only correct if it learns which decisions the loader declined.
    pub outcome: Result<Residency, TransitionError>,
}

/// One expert's residency, lifespan and preserved state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Expert {
    state: Residency,
    lifespan: Lifespan,
    /// Calls in progress. ACTIVE is exactly `inflight > 0`.
    inflight: u32,
    pinned: bool,
    /// The adaptation state (§4.2: 적응/캐시/LoRA 델타) as an opaque blob.
    /// Kept across eviction for PERSISTENT experts, dropped for TRANSIENT.
    preserved: Option<Vec<u8>>,
}

/// The §5 residency state machine over expert ids — the `ServantActivator`
/// §4.2 asks for, as a value the POA's locator borrows.
///
/// # There is no per-token hook
///
/// The machine takes batch decisions and call markers, and nothing else. This
/// does not compile, and that is the enforcement:
///
/// ```compile_fail
/// let mut loader = orbweaver_object::residency::ExpertLoader::new();
/// loader.on_token("expert-math");
/// ```
///
/// The same construction with the batch-period API does:
///
/// ```
/// use orbweaver_object::residency::{BatchStats, ExpertLoader};
/// let mut loader = ExpertLoader::new();
/// let _ = loader.apply(&[], &BatchStats::idle());
/// ```
#[derive(Debug, Default)]
pub struct ExpertLoader {
    experts: BTreeMap<String, Expert>,
}

impl ExpertLoader {
    /// A loader that knows about no experts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `id`, OFFLOADED, with the servant-management policy §4.2
    /// distinguishes: [`Lifespan::Transient`] discards adaptation state on
    /// eviction, [`Lifespan::Persistent`] preserves it.
    pub fn register(&mut self, id: &str, lifespan: Lifespan) -> Result<(), TransitionError> {
        if self.experts.contains_key(id) {
            return Err(TransitionError::Duplicate { id: id.to_owned() });
        }
        self.experts.insert(
            id.to_owned(),
            Expert {
                state: Residency::Offloaded,
                lifespan,
                inflight: 0,
                pinned: false,
                preserved: None,
            },
        );
        Ok(())
    }

    /// Where `id`'s weights are, or `None` if it is not registered — the IDL
    /// `ExpertLoader::status`, with the unregistered case honest instead of
    /// answering OFFLOADED for an expert nobody has ever heard of.
    pub fn status(&self, id: &str) -> Option<Residency> {
        self.experts.get(id).map(|e| e.state)
    }

    /// `id`'s lifespan policy, if it is registered.
    pub fn lifespan(&self, id: &str) -> Option<Lifespan> {
        self.experts.get(id).map(|e| e.lifespan)
    }

    /// Every expert's residency, in ascending-id order. The map tests pin.
    pub fn states(&self) -> BTreeMap<String, Residency> {
        self.experts.iter().map(|(id, e)| (id.clone(), e.state)).collect()
    }

    /// Pins `id` against eviction (the IDL `ExpertLoader::pin`). Returns
    /// whether it is registered; pinning an unknown id records nothing,
    /// matching `OfferStore::pin`.
    pub fn pin(&mut self, id: &str) -> bool {
        match self.experts.get_mut(id) {
            Some(e) => {
                e.pinned = true;
                true
            }
            None => false,
        }
    }

    /// Removes a pin. Returns whether one was present.
    pub fn unpin(&mut self, id: &str) -> bool {
        match self.experts.get_mut(id) {
            Some(e) => std::mem::replace(&mut e.pinned, false),
            None => false,
        }
    }

    /// Whether `id` is pinned against eviction.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.experts.get(id).is_some_and(|e| e.pinned)
    }

    /// How many calls are inflight on `id`.
    pub fn inflight(&self, id: &str) -> u32 {
        self.experts.get(id).map_or(0, |e| e.inflight)
    }

    /// Every expert with a call inflight, in the shape
    /// [`LoadingPolicy::decide`] wants for its third argument — so the
    /// policy's inflight guard and the loader's are fed from one source.
    pub fn inflight_ids(&self) -> BTreeSet<String> {
        self.experts.iter().filter(|(_, e)| e.inflight > 0).map(|(id, _)| id.clone()).collect()
    }

    /// OFFLOADED → PREFETCHING (the IDL `ExpertLoader::prefetch`, `oneway`).
    ///
    /// A *request*, not a load: it returns immediately and the expert stays
    /// PREFETCHING until whoever performs the copy reports
    /// [`complete_load`](ExpertLoader::complete_load). Nothing in this
    /// repository performs that copy, which is why no timer completes it here.
    pub fn request_prefetch(&mut self, id: &str) -> Result<Residency, TransitionError> {
        self.edge(id, Residency::Prefetching, |from| from == Residency::Offloaded)
    }

    /// PREFETCHING → RESIDENT: the weights arrived and the reference is
    /// serviceable. Any preserved state comes back with them.
    pub fn complete_load(&mut self, id: &str) -> Result<Residency, TransitionError> {
        self.edge(id, Residency::Resident, |from| from == Residency::Prefetching)
    }

    /// RESIDENT → ACTIVE: a call is inflight.
    ///
    /// From ACTIVE this is not a transition at all but an increment of the
    /// inflight counter — concurrent calls on one resident expert are
    /// ordinary, and the guard needs the count, not a boolean.
    pub fn begin_call(&mut self, id: &str) -> Result<Residency, TransitionError> {
        let e = self.expert_mut(id)?;
        match e.state {
            Residency::Resident | Residency::Active => {
                e.inflight += 1;
                e.state = Residency::Active;
                Ok(e.state)
            }
            from => {
                Err(TransitionError::Illegal { id: id.to_owned(), from, to: Residency::Active })
            }
        }
    }

    /// ACTIVE → RESIDENT, once the last inflight call finishes. While others
    /// are still running the expert stays ACTIVE and the count drops by one.
    pub fn end_call(&mut self, id: &str) -> Result<Residency, TransitionError> {
        let e = self.expert_mut(id)?;
        if e.state != Residency::Active {
            return Err(TransitionError::Illegal {
                id: id.to_owned(),
                from: e.state,
                to: Residency::Resident,
            });
        }
        e.inflight -= 1;
        if e.inflight == 0 {
            e.state = Residency::Resident;
        }
        Ok(e.state)
    }

    /// RESIDENT → OFFLOADED, if §5's guard allows: memory pressure **and**
    /// route_freq fallen **and** no inflight call **and** not pinned.
    ///
    /// Conditions are checked in that order and the first unmet one is
    /// reported. A PERSISTENT expert keeps its adaptation state; a TRANSIENT
    /// one loses it, which is the whole content of the distinction.
    pub fn evict(&mut self, id: &str, stats: &BatchStats) -> Result<Residency, TransitionError> {
        let Some(e) = self.experts.get(id) else {
            return Err(TransitionError::Unknown { id: id.to_owned() });
        };
        // PREFETCHING is not an eviction candidate: cancelling a load in
        // flight is an edge this machine does not have, and the §6 policy
        // never asks for one (it filters PREFETCHING out of its candidates),
        // so this arm is the cross-check between the two crates.
        if matches!(e.state, Residency::Offloaded | Residency::Prefetching) {
            return Err(TransitionError::Illegal {
                id: id.to_owned(),
                from: e.state,
                to: Residency::Offloaded,
            });
        }
        let unmet = if !stats.memory_pressure {
            Some(GuardCondition::MemoryPressure)
        } else if !stats.cold.contains(id) {
            Some(GuardCondition::RouteFreqLow)
        } else if e.inflight > 0 {
            Some(GuardCondition::NoInflight)
        } else if e.pinned {
            Some(GuardCondition::Unpinned)
        } else {
            None
        };
        if let Some(unmet) = unmet {
            return Err(TransitionError::Guarded { id: id.to_owned(), unmet });
        }
        let e = self.expert_mut(id)?;
        e.state = Residency::Offloaded;
        if e.lifespan == Lifespan::Transient {
            e.preserved = None;
        }
        Ok(e.state)
    }

    /// Records `id`'s adaptation state — the LoRA delta or session cache
    /// §4.2 has PERSISTENT experts keep. Opaque here on purpose: this crate
    /// has no idea what weights look like and should not pretend to.
    pub fn set_state(&mut self, id: &str, blob: Vec<u8>) -> Result<(), TransitionError> {
        let e = self.expert_mut(id)?;
        if matches!(e.state, Residency::Offloaded | Residency::Prefetching) {
            return Err(TransitionError::NotLoaded { id: id.to_owned(), state: e.state });
        }
        e.preserved = Some(blob);
        Ok(())
    }

    /// The state blob the loader is keeping for `id`, whatever its residency.
    /// `None` means either "no state recorded" or "TRANSIENT, and eviction
    /// dropped it".
    pub fn preserved_state(&self, id: &str) -> Option<&[u8]> {
        self.experts.get(id)?.preserved.as_deref()
    }

    /// Applies one batch window's decisions, in order, and reports what each
    /// one did.
    ///
    /// This is the whole command surface for loading and eviction, and it is
    /// deliberately shaped as *a slice per window*: there is nowhere to hang a
    /// per-token callback, and a caller reaching token period has to write the
    /// loop themselves in plain sight (module docs).
    pub fn apply(&mut self, decisions: &[Decision], stats: &BatchStats) -> Vec<Applied> {
        decisions
            .iter()
            .map(|decision| {
                let outcome = match decision {
                    Decision::Prefetch(id) => self.request_prefetch(id),
                    Decision::Evict(id) => self.evict(id, stats),
                };
                Applied { decision: decision.clone(), outcome }
            })
            .collect()
    }

    /// A `ServantLocator` over this loader, for
    /// [`Poa::dispatch_target`](crate::Poa::dispatch_target).
    pub fn locator(&mut self, miss: MissPolicy) -> ExpertLocator<'_> {
        ExpertLocator { loader: self, miss }
    }

    /// Brings `poa`'s activation set in line with the residency map:
    /// RESIDENT and ACTIVE experts are activated, everything else
    /// deactivated.
    ///
    /// Necessary because [`Located::Here`] makes the POA activate an id
    /// *permanently* — the locator is consulted only for inactive ids, so
    /// without this an evicted expert would keep being served from the POA's
    /// active map. Call it after every [`apply`](ExpertLoader::apply).
    pub fn reconcile(&self, poa: &mut Poa) -> Reconciled {
        let mut out = Reconciled { activated: Vec::new(), deactivated: Vec::new() };
        for (id, e) in &self.experts {
            let oid = ObjectId::from_name(id);
            let serviceable = matches!(e.state, Residency::Resident | Residency::Active);
            if serviceable && !poa.is_active(&oid) {
                poa.activate(oid.clone());
                out.activated.push(oid);
            } else if !serviceable && poa.deactivate(&oid) {
                out.deactivated.push(oid);
            }
        }
        out
    }

    fn expert_mut(&mut self, id: &str) -> Result<&mut Expert, TransitionError> {
        self.experts.get_mut(id).ok_or_else(|| TransitionError::Unknown { id: id.to_owned() })
    }

    /// One unguarded edge: move to `to` when `allowed` accepts the state we
    /// are in, and name the refusal otherwise.
    fn edge(
        &mut self,
        id: &str,
        to: Residency,
        allowed: impl Fn(Residency) -> bool,
    ) -> Result<Residency, TransitionError> {
        let e = self.expert_mut(id)?;
        if !allowed(e.state) {
            return Err(TransitionError::Illegal { id: id.to_owned(), from: e.state, to });
        }
        e.state = to;
        Ok(to)
    }
}

/// What [`ExpertLoader::reconcile`] changed on the POA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    /// Ids activated because their expert is RESIDENT or ACTIVE.
    pub activated: Vec<ObjectId>,
    /// Ids deactivated because their expert is not.
    pub deactivated: Vec<ObjectId>,
}

/// What the locator does when a request arrives for an OFFLOADED expert.
///
/// Both answers refuse the request. **Loading synchronously inside a request
/// is not offered**, and the omission is the argument: a demand load is
/// exactly the latency §11 says prefetch exists to hide, it would hold a
/// dispatch thread for the whole weight copy, and — being unbounded and
/// unobservable to the caller — it is how a control plane leaks into the data
/// plane's time budget. The honest answer to "this expert is in storage" is
/// to say so now and load it in the next window, not to make the caller wait
/// inside a `locate`.
///
/// `OBJECT_NOT_EXIST` is a slightly wrong-shaped way to say it — the expert
/// exists, it is merely not in memory — but it is what [`Located::Unknown`]
/// produces and it is the honest half-truth available today. Saying "not
/// here, try again" properly needs either a `TRANSIENT` system exception or a
/// `Located` variant for retry-after, and adding one without a client that
/// acts on it would be decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissPolicy {
    /// Refuse, and change nothing. Loading stays entirely with the
    /// batch-period policy: a miss is telemetry, not a command.
    Refuse,
    /// Refuse, and request a prefetch so the *next* window finds the expert
    /// PREFETCHING — §8 step 4a's demand prefetch, with the caller not
    /// waiting for it. Still a control-plane-rate transition: it is driven by
    /// a request (ms), never by a token.
    RefuseAndPrefetch,
}

/// A [`ServantLocator`] backed by an [`ExpertLoader`].
///
/// RESIDENT and ACTIVE experts are served here; everything else is refused
/// per the [`MissPolicy`], which never blocks. `LOCATION_FORWARD` is not
/// produced: forwarding an expert to another node is placement, which §4.3
/// gives to the trading service, and this locator has no view of other nodes
/// to forward to.
#[derive(Debug)]
pub struct ExpertLocator<'a> {
    loader: &'a mut ExpertLoader,
    miss: MissPolicy,
}

impl ServantLocator for ExpertLocator<'_> {
    fn locate(&mut self, id: &ObjectId) -> Located {
        // Expert ids are text (`moe::CapabilityId`); a key that is not is not
        // one of ours.
        let Some(name) = id.as_str() else {
            return Located::Unknown;
        };
        match self.loader.status(name) {
            Some(Residency::Resident | Residency::Active) => Located::Here,
            Some(Residency::Offloaded) if self.miss == MissPolicy::RefuseAndPrefetch => {
                // A request, not a load: this returns immediately, and the
                // caller is refused regardless of whether it succeeded.
                let _ = self.loader.request_prefetch(name);
                Located::Unknown
            }
            // OFFLOADED under `Refuse`, PREFETCHING (a load is already
            // underway and waiting for it is the thing we refuse to do), or
            // an id this loader has never heard of.
            _ => Located::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Target, UnknownIdPolicy};
    use orbweaver_trading::{FREQ_SCALE, Offer};

    /// Every operation the machine has, so the transition table can be
    /// written out rather than sampled.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Op {
        Prefetch,
        Load,
        Begin,
        End,
        Evict,
    }

    const EVERY_OP: [Op; 5] = [Op::Prefetch, Op::Load, Op::Begin, Op::End, Op::Evict];
    const EVERY_STATE: [Residency; 4] =
        [Residency::Offloaded, Residency::Prefetching, Residency::Resident, Residency::Active];

    fn run(loader: &mut ExpertLoader, op: Op, id: &str) -> Result<Residency, TransitionError> {
        // The guard's two external conditions hold, so the only evictions
        // that fail here fail structurally.
        let stats = evictable(id);
        match op {
            Op::Prefetch => loader.request_prefetch(id),
            Op::Load => loader.complete_load(id),
            Op::Begin => loader.begin_call(id),
            Op::End => loader.end_call(id),
            Op::Evict => loader.evict(id, &stats),
        }
    }

    /// A window under which `id` is evictable as far as the *external*
    /// conditions go.
    fn evictable(id: &str) -> BatchStats {
        BatchStats { memory_pressure: true, cold: BTreeSet::from([id.to_owned()]) }
    }

    /// A loader holding one TRANSIENT expert parked in `state`, reached only
    /// through legal edges — the fixture cannot fabricate a state the machine
    /// could not produce.
    fn parked(state: Residency) -> ExpertLoader {
        let mut l = ExpertLoader::new();
        l.register("expert-a", Lifespan::Transient).expect("registers");
        let steps: &[Op] = match state {
            Residency::Offloaded => &[],
            Residency::Prefetching => &[Op::Prefetch],
            Residency::Resident => &[Op::Prefetch, Op::Load],
            Residency::Active => &[Op::Prefetch, Op::Load, Op::Begin],
        };
        for op in steps {
            run(&mut l, *op, "expert-a").expect("the fixture only walks legal edges");
        }
        assert_eq!(l.status("expert-a"), Some(state));
        l
    }

    // ── the transition table ────────────────────────────────────────────────

    /// The whole cycle §5 draws, walked once.
    #[test]
    fn the_legal_edges_walk_the_documented_cycle() {
        let mut l = ExpertLoader::new();
        l.register("expert-a", Lifespan::Transient).expect("registers");
        assert_eq!(l.status("expert-a"), Some(Residency::Offloaded));
        assert_eq!(l.request_prefetch("expert-a"), Ok(Residency::Prefetching));
        assert_eq!(l.complete_load("expert-a"), Ok(Residency::Resident));
        assert_eq!(l.begin_call("expert-a"), Ok(Residency::Active));
        assert_eq!(l.end_call("expert-a"), Ok(Residency::Resident));
        assert_eq!(l.evict("expert-a", &evictable("expert-a")), Ok(Residency::Offloaded));
        // …and round again, because a machine that only works once is a
        // sequence.
        assert_eq!(l.request_prefetch("expert-a"), Ok(Residency::Prefetching));
        assert_eq!(l.complete_load("expert-a"), Ok(Residency::Resident));
    }

    /// All twenty state × operation pairs, spelled out. The four legal
    /// non-cycle answers are the interesting ones: BEGIN on ACTIVE is a
    /// counter increment, EVICT on ACTIVE is a *guard* refusal rather than a
    /// missing edge, and EVICT on OFFLOADED/PREFETCHING is a missing edge
    /// rather than a guard refusal.
    #[test]
    fn every_state_times_every_operation_has_a_pinned_answer() {
        for state in EVERY_STATE {
            for op in EVERY_OP {
                let mut l = parked(state);
                let got = run(&mut l, op, "expert-a");
                let want: Result<Residency, TransitionError> = match (state, op) {
                    (Residency::Offloaded, Op::Prefetch) => Ok(Residency::Prefetching),
                    (Residency::Prefetching, Op::Load) => Ok(Residency::Resident),
                    (Residency::Resident, Op::Begin) => Ok(Residency::Active),
                    (Residency::Resident, Op::Evict) => Ok(Residency::Offloaded),
                    (Residency::Active, Op::Begin) => Ok(Residency::Active),
                    (Residency::Active, Op::End) => Ok(Residency::Resident),
                    (Residency::Active, Op::Evict) => Err(TransitionError::Guarded {
                        id: "expert-a".to_owned(),
                        unmet: GuardCondition::NoInflight,
                    }),
                    (from, op) => Err(TransitionError::Illegal {
                        id: "expert-a".to_owned(),
                        from,
                        to: match op {
                            Op::Prefetch => Residency::Prefetching,
                            Op::Load | Op::End => Residency::Resident,
                            Op::Begin => Residency::Active,
                            Op::Evict => Residency::Offloaded,
                        },
                    }),
                };
                assert_eq!(got, want, "{state:?} × {op:?}");
                // A refusal is not a half-transition: the state is untouched.
                if got.is_err() {
                    assert_eq!(l.status("expert-a"), Some(state), "{state:?} × {op:?} moved");
                }
            }
        }
    }

    /// Two concurrent calls need two completions: ACTIVE is `inflight > 0`,
    /// not a flag the first `end_call` can clear.
    #[test]
    fn concurrent_calls_count_rather_than_toggle() {
        let mut l = parked(Residency::Resident);
        assert_eq!(l.begin_call("expert-a"), Ok(Residency::Active));
        assert_eq!(l.begin_call("expert-a"), Ok(Residency::Active));
        assert_eq!(l.inflight("expert-a"), 2);
        assert_eq!(l.end_call("expert-a"), Ok(Residency::Active), "one call is still running");
        assert_eq!(l.end_call("expert-a"), Ok(Residency::Resident));
        assert_eq!(l.inflight("expert-a"), 0);
        assert!(l.inflight_ids().is_empty());
    }

    #[test]
    fn an_unregistered_expert_names_itself_in_every_refusal() {
        let mut l = ExpertLoader::new();
        let want = Err(TransitionError::Unknown { id: "expert-ghost".to_owned() });
        for op in EVERY_OP {
            assert_eq!(run(&mut l, op, "expert-ghost"), want, "{op:?}");
        }
        assert_eq!(l.status("expert-ghost"), None);
        assert_eq!(l.lifespan("expert-ghost"), None);
        assert!(!l.pin("expert-ghost"), "pinning an unknown id records nothing");
        assert!(!l.is_pinned("expert-ghost"));
        assert!(!l.unpin("expert-ghost"));
        assert_eq!(l.inflight("expert-ghost"), 0);
    }

    #[test]
    fn a_duplicate_registration_refuses_rather_than_resetting() {
        let mut l = parked(Residency::Resident);
        assert_eq!(
            l.register("expert-a", Lifespan::Persistent),
            Err(TransitionError::Duplicate { id: "expert-a".to_owned() })
        );
        assert_eq!(l.status("expert-a"), Some(Residency::Resident), "the refusal changed nothing");
        assert_eq!(l.lifespan("expert-a"), Some(Lifespan::Transient));
    }

    // ── the eviction guard: each condition, one at a time ───────────────────

    /// The baseline the four tests below each break exactly once: all four
    /// conditions hold, so the expert goes.
    #[test]
    fn eviction_proceeds_when_all_four_conditions_hold() {
        let mut l = parked(Residency::Resident);
        assert_eq!(l.evict("expert-a", &evictable("expert-a")), Ok(Residency::Offloaded));
    }

    #[test]
    fn eviction_needs_memory_pressure() {
        let mut l = parked(Residency::Resident);
        let stats = BatchStats { memory_pressure: false, ..evictable("expert-a") };
        assert_eq!(
            l.evict("expert-a", &stats),
            Err(TransitionError::Guarded {
                id: "expert-a".to_owned(),
                unmet: GuardCondition::MemoryPressure,
            })
        );
        assert_eq!(l.status("expert-a"), Some(Residency::Resident));
    }

    #[test]
    fn eviction_needs_a_fallen_route_freq() {
        let mut l = parked(Residency::Resident);
        // Pressure, but this expert is still hot: §6 wants the coldest one,
        // and "something must go" is not a reason to take a hot expert.
        let stats = BatchStats { memory_pressure: true, cold: BTreeSet::new() };
        assert_eq!(
            l.evict("expert-a", &stats),
            Err(TransitionError::Guarded {
                id: "expert-a".to_owned(),
                unmet: GuardCondition::RouteFreqLow,
            })
        );
        assert_eq!(l.status("expert-a"), Some(Residency::Resident));
    }

    #[test]
    fn eviction_needs_no_inflight_call() {
        let mut l = parked(Residency::Resident);
        l.begin_call("expert-a").expect("a call arrives");
        assert_eq!(
            l.evict("expert-a", &evictable("expert-a")),
            Err(TransitionError::Guarded {
                id: "expert-a".to_owned(),
                unmet: GuardCondition::NoInflight,
            })
        );
        // …and the moment it finishes, the same window evicts it.
        l.end_call("expert-a").expect("the call finishes");
        assert_eq!(l.evict("expert-a", &evictable("expert-a")), Ok(Residency::Offloaded));
    }

    #[test]
    fn eviction_needs_an_unpinned_expert() {
        let mut l = parked(Residency::Resident);
        assert!(l.pin("expert-a"));
        assert!(l.is_pinned("expert-a"));
        assert_eq!(
            l.evict("expert-a", &evictable("expert-a")),
            Err(TransitionError::Guarded {
                id: "expert-a".to_owned(),
                unmet: GuardCondition::Unpinned,
            })
        );
        assert!(l.unpin("expert-a"));
        assert_eq!(l.evict("expert-a", &evictable("expert-a")), Ok(Residency::Offloaded));
    }

    // ── PERSISTENT state ────────────────────────────────────────────────────

    /// §4.2's whole content: a PERSISTENT expert's adaptation state survives
    /// the round trip through storage.
    #[test]
    fn persistent_state_survives_an_evict_and_reload_cycle() {
        let mut l = ExpertLoader::new();
        l.register("expert-a", Lifespan::Persistent).expect("registers");
        l.request_prefetch("expert-a").expect("prefetch");
        l.complete_load("expert-a").expect("load");
        l.set_state("expert-a", b"lora-delta-v3".to_vec()).expect("records state");

        assert_eq!(l.evict("expert-a", &evictable("expert-a")), Ok(Residency::Offloaded));
        assert_eq!(
            l.preserved_state("expert-a"),
            Some(&b"lora-delta-v3"[..]),
            "PERSISTENT keeps its state while offloaded"
        );

        l.request_prefetch("expert-a").expect("prefetch");
        l.complete_load("expert-a").expect("load");
        assert_eq!(l.preserved_state("expert-a"), Some(&b"lora-delta-v3"[..]));
    }

    /// The contrast that makes the lifespan policy mean something.
    #[test]
    fn transient_state_is_dropped_by_eviction() {
        let mut l = parked(Residency::Resident);
        l.set_state("expert-a", b"scratch".to_vec()).expect("records state");
        assert_eq!(l.preserved_state("expert-a"), Some(&b"scratch"[..]));
        l.evict("expert-a", &evictable("expert-a")).expect("evicts");
        assert_eq!(l.preserved_state("expert-a"), None);
    }

    #[test]
    fn adaptation_state_needs_the_weights_in_memory() {
        let mut l = parked(Residency::Prefetching);
        assert_eq!(
            l.set_state("expert-a", b"x".to_vec()),
            Err(TransitionError::NotLoaded {
                id: "expert-a".to_owned(),
                state: Residency::Prefetching,
            })
        );
        assert_eq!(l.preserved_state("expert-a"), None);
    }

    // ── the trading integration ─────────────────────────────────────────────

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

    #[test]
    fn batch_stats_read_pressure_and_coldness_off_the_store() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };
        let mut store = OfferStore::new();
        store.register(offer("expert-cold", Residency::Resident, 50, FREQ_SCALE)).expect("reg");
        store.register(offer("expert-hot", Residency::Resident, 50, 5 * FREQ_SCALE)).expect("reg");

        let under = BatchStats::from_store(&store, &policy, 20, 2 * FREQ_SCALE);
        assert!(under.memory_pressure);
        assert_eq!(under.cold, BTreeSet::from(["expert-cold".to_owned()]));

        let over = BatchStats::from_store(&store, &policy, 500, 2 * FREQ_SCALE);
        assert!(!over.memory_pressure, "above the low watermark there is no pressure");
    }

    /// End to end: two batch windows of §6 decisions applied to the machine,
    /// with the resulting state map pinned.
    ///
    /// The window-1 pin is the interesting one. The policy asks to evict
    /// `expert-cold` (coldest, unpinned *in the offer store*) and
    /// `expert-mid`; the loader has `expert-cold` pinned and refuses. Two
    /// copies of the pin set can disagree, and the loader is the authority —
    /// the guard is a cross-check on the policy, not a rubber stamp for it.
    #[test]
    fn a_trading_decision_list_applies_to_the_pinned_state_map() {
        let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };
        let mut store = OfferStore::new();
        for (id, residency, mem, hits) in [
            ("expert-hot", Residency::Resident, 200, 5),
            ("expert-mid", Residency::Resident, 60, 2),
            ("expert-cold", Residency::Resident, 50, 1),
            ("expert-next", Residency::Offloaded, 100, 3),
        ] {
            store.register(offer(id, residency, mem, hits * FREQ_SCALE)).expect("registers");
        }

        let mut loader = ExpertLoader::new();
        loader.register("expert-hot", Lifespan::Persistent).expect("registers");
        for id in ["expert-mid", "expert-cold", "expert-next"] {
            loader.register(id, Lifespan::Transient).expect("registers");
        }
        // The three resident offers reach RESIDENT the only way they can.
        for id in ["expert-hot", "expert-mid", "expert-cold"] {
            loader.request_prefetch(id).expect("prefetch");
            loader.complete_load(id).expect("load");
        }
        assert!(loader.pin("expert-cold"), "pinned with the loader, not with the store");

        // ── window 1: pressure (free 20 < low 100) ──────────────────────────
        let stats = BatchStats::from_store(&store, &policy, 20, 3 * FREQ_SCALE);
        let decisions = policy.decide(&store, 20, &loader.inflight_ids());
        assert_eq!(
            decisions,
            vec![
                Decision::Evict("expert-cold".to_owned()),
                Decision::Evict("expert-mid".to_owned()),
            ],
            "LFU order: 16 then 32"
        );
        let applied = loader.apply(&decisions, &stats);
        assert_eq!(
            applied,
            vec![
                Applied {
                    decision: Decision::Evict("expert-cold".to_owned()),
                    outcome: Err(TransitionError::Guarded {
                        id: "expert-cold".to_owned(),
                        unmet: GuardCondition::Unpinned,
                    }),
                },
                Applied {
                    decision: Decision::Evict("expert-mid".to_owned()),
                    outcome: Ok(Residency::Offloaded),
                },
            ]
        );
        // The store mirrors what the loader actually did (OfferStore::
        // set_residency exists for exactly this report), then the window
        // closes and every counter decays.
        for (id, state) in loader.states() {
            store.set_residency(&id, state);
        }
        store.decay_all();

        // ── window 2: idle (free 520 > high 400) ────────────────────────────
        let stats = BatchStats::from_store(&store, &policy, 520, 3 * FREQ_SCALE);
        let decisions = policy.decide(&store, 520, &loader.inflight_ids());
        assert_eq!(
            decisions,
            vec![Decision::Prefetch("expert-mid".to_owned())],
            "mid scores 16/60 against next's 24/100, and next then does not fit"
        );
        assert_eq!(
            loader.apply(&decisions, &stats),
            vec![Applied {
                decision: Decision::Prefetch("expert-mid".to_owned()),
                outcome: Ok(Residency::Prefetching),
            }]
        );

        // The load lands and a call arrives — neither is a policy decision.
        loader.complete_load("expert-mid").expect("the copy finishes");
        loader.begin_call("expert-mid").expect("a call arrives");

        assert_eq!(
            loader.states(),
            BTreeMap::from([
                ("expert-cold".to_owned(), Residency::Resident),
                ("expert-hot".to_owned(), Residency::Resident),
                ("expert-mid".to_owned(), Residency::Active),
                ("expert-next".to_owned(), Residency::Offloaded),
            ])
        );
        assert_eq!(loader.inflight_ids(), BTreeSet::from(["expert-mid".to_owned()]));
    }

    // ── the POA integration ─────────────────────────────────────────────────

    fn poa_with(loader: &ExpertLoader) -> Poa {
        let mut poa = Poa::new("MoEPOA", "IDL:moe/Expert:1.0").with_lifespan(Lifespan::Persistent);
        loader.reconcile(&mut poa);
        poa
    }

    #[test]
    fn a_resident_expert_is_served_here() {
        let mut loader = parked(Residency::Resident);
        let mut poa = Poa::new("MoEPOA", "IDL:moe/Expert:1.0")
            .with_lifespan(Lifespan::Persistent)
            .with_unknown_id(UnknownIdPolicy::AskLocator);
        let key = poa.object_key(&ObjectId::from_name("expert-a"));
        let mut locator = loader.locator(MissPolicy::Refuse);
        assert!(matches!(poa.dispatch_target(&key, Some(&mut locator)), Target::Active(_)));
    }

    /// The design decision, held as a test: a request for an OFFLOADED expert
    /// is refused and the prefetch is left to the next window. Nothing in
    /// `locate` waits for a load.
    #[test]
    fn an_offloaded_expert_is_refused_and_never_loaded_synchronously() {
        let mut loader = parked(Residency::Offloaded);
        let mut poa = Poa::new("MoEPOA", "IDL:moe/Expert:1.0")
            .with_lifespan(Lifespan::Persistent)
            .with_unknown_id(UnknownIdPolicy::AskLocator);
        let key = poa.object_key(&ObjectId::from_name("expert-a"));

        {
            let mut locator = loader.locator(MissPolicy::RefuseAndPrefetch);
            assert!(matches!(poa.dispatch_target(&key, Some(&mut locator)), Target::Unknown));
        }
        // Refused, but not ignored: the next window will find it loading.
        assert_eq!(loader.status("expert-a"), Some(Residency::Prefetching));
        // …and a request landing mid-load is refused too, rather than
        // waiting for the copy to finish.
        let mut locator = loader.locator(MissPolicy::RefuseAndPrefetch);
        assert!(matches!(poa.dispatch_target(&key, Some(&mut locator)), Target::Unknown));
    }

    #[test]
    fn the_refuse_policy_records_nothing_at_all() {
        let mut loader = parked(Residency::Offloaded);
        let mut locator = loader.locator(MissPolicy::Refuse);
        assert!(matches!(locator.locate(&ObjectId::from_name("expert-a")), Located::Unknown));
        assert_eq!(loader.status("expert-a"), Some(Residency::Offloaded));
    }

    #[test]
    fn an_id_the_loader_never_heard_of_is_unknown() {
        let mut loader = parked(Residency::Resident);
        let mut locator = loader.locator(MissPolicy::RefuseAndPrefetch);
        assert!(matches!(locator.locate(&ObjectId::from_name("expert-ghost")), Located::Unknown));
        // A key that is not even text cannot name an expert.
        assert!(matches!(locator.locate(&ObjectId(vec![0xff, 0xfe])), Located::Unknown));
    }

    /// `Located::Here` activates the id *permanently*, so eviction has to be
    /// reflected on the POA or an evicted expert keeps being served out of
    /// the active map — the bug `reconcile` exists to prevent.
    #[test]
    fn reconcile_stops_the_poa_serving_an_evicted_expert() {
        let mut loader = parked(Residency::Resident);
        let mut poa = poa_with(&loader).with_unknown_id(UnknownIdPolicy::AskLocator);
        let id = ObjectId::from_name("expert-a");
        let key = poa.object_key(&id);
        assert!(poa.is_active(&id), "reconcile activated the resident expert");
        assert!(matches!(poa.dispatch_target(&key, None), Target::Active(_)));

        loader.evict("expert-a", &evictable("expert-a")).expect("evicts");
        let changed = loader.reconcile(&mut poa);
        assert_eq!(changed.deactivated, vec![id.clone()]);
        assert!(changed.activated.is_empty());
        assert!(matches!(poa.dispatch_target(&key, None), Target::Unknown));

        // And back: reloading re-activates it, once.
        loader.request_prefetch("expert-a").expect("prefetch");
        loader.complete_load("expert-a").expect("load");
        let changed = loader.reconcile(&mut poa);
        assert_eq!(changed.activated, vec![id]);
        assert!(loader.reconcile(&mut poa).activated.is_empty(), "reconcile is idempotent");
    }
}
