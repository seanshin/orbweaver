//! The wire surface of the MoE control plane: `moe::ExpertRegistry` and
//! `moe::ExpertLoader` from `corpus/golden/22-moe-control-plane.idl`, served
//! on our own [`Server`](orbweaver_giop::server::Server).
//!
//! PLAN-SERVICES §3 defers the standard `CosTrading::Lookup` facade until a
//! foreign trading client is named, and lands **the project contract**
//! instead. That contract is corpus/golden/22 and nothing else: the two
//! interfaces below are served exactly as declared there, with no operation
//! added and none half-served.
//!
//! # The join this file exists for
//!
//! F2 landed the decision engine (`orbweaver_trading`: offers, the §6 loading
//! policy) and F3 landed the residency state machine
//! ([`crate::residency`]). Neither knows about the other's *inputs*: the
//! policy cannot know inflight calls or pins the loader owns, and the loader
//! cannot know free accelerator memory or a decayed `route_freq` the store
//! owns. [`ExpertService`] is the one place they meet, and
//! [`ExpertService::apply_policy`] is that meeting written down — a heartbeat
//! updates the offer, the policy runs over the store, and its
//! [`Decision`]s drive the loader, in one call so a control loop is one line
//! rather than a re-derivation at every call site.
//!
//! # Marshalling: by hand, against the declared layout
//!
//! [`Capability`] and `CapabilityId` are hand-marshalled in the IDL's
//! declaration order rather than driven by a TypeCode. This crate already
//! depends on `orbweaver-registry`, so a TypeCode is *reachable* — but a
//! TypeCode does not marshal anything on its own: driving CDR from one needs
//! `orbweaver-dynamic` as well, plus parsing corpus/golden/22 at run time and
//! carrying a `Value` tree in and out of every operation. That is a large
//! coupling to buy two structs on a fixed, nine-member contract. The trade is
//! only acceptable because the layout is *pinned by test* rather than by
//! memory: `capability_members_are_in_the_idls_declaration_order` decodes an
//! encoded capability with the primitive getters, in order, so a member
//! reordered here fails independently of [`Capability::read_from`].
//!
//! # What the contract does not carry
//!
//! - **No `specialization`, no `latency_p50`.** [`Offer`] has both and
//!   `moe::Capability` has neither, so a wire-registered offer carries an
//!   empty specialization and a zero p50. A `specialization == 'math'`
//!   constraint query therefore cannot be satisfied by an offer that arrived
//!   over this contract. Stated rather than papered over with a guess: the
//!   fix is a contract change, which is F1's territory, not a default here.
//! - **No exceptions.** Not one operation declares `raises`, so every refusal
//!   is a *system* exception. Inventing a user exception would produce bytes
//!   the generated client for corpus/golden/22 has no branch for — worse than
//!   a system exception that says less. This is why [`ExpertService`]
//!   implements plain [`Dispatch::dispatch`] and not the
//!   `dispatch_body`/`UserException` path the naming server needed.
//! - **No `unpin`, no `complete_load`, no call markers.** `pin` has no
//!   inverse in the IDL and there is deliberately no wire operation for "a
//!   call began": a per-call wire hook is precisely the per-token surface §5
//!   forbids. Those transitions arrive out of band, through
//!   [`ExpertService::begin_call`] and friends.
//!
//! # Who wins when two copies disagree
//!
//! Three fields exist twice, and in each case the wire copy is a *report* and
//! the control plane is the authority:
//!
//! | field | authority | why |
//! |---|---|---|
//! | `Capability::state` | the loader | F3's guard is the only thing that moves residency; an expert announcing RESIDENT would desynchronise the offer store from the machine that owns it |
//! | `Capability::route_freq` | the store | `OfferStore::heartbeat` already refuses to let a heartbeat rewrite routing history; letting `register_expert` seed it would be the same hole on the other side |
//! | pins | both, set together | the loader's guard is still the authority, but pinning only one copy lets the policy propose evictions the guard will always refuse |
//!
//! [`Decision`]: orbweaver_trading::policy::Decision
//! [`Offer`]: orbweaver_trading::Offer

use std::collections::BTreeMap;

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::server::{Completion, Dispatch, Request, SystemException};
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_trading::policy::{Decision, LoadingPolicy};
use orbweaver_trading::{FREQ_SCALE, Offer, OfferStore, Residency, StoreError};

use crate::residency::{Applied, BatchStats, ExpertLoader, GuardCondition, TransitionError};
use crate::{Lifespan, OBJECT_ID, get_reference, is_equivalent};

/// Repository id of `moe::ExpertRegistry`.
pub const EXPERT_REGISTRY_ID: &str = "IDL:moe/ExpertRegistry:1.0";
/// Repository id of `moe::ExpertLoader`.
pub const EXPERT_LOADER_ID: &str = "IDL:moe/ExpertLoader:1.0";
/// Repository id of `moe::Expert` — the type of the reference
/// `register_expert`, `deregister` and `heartbeat` take.
pub const EXPERT_ID: &str = "IDL:moe/Expert:1.0";

/// `BAD_PARAM`: an argument named an expert this service does not know, or
/// re-registered one it already does.
pub const BAD_PARAM: &str = "IDL:omg.org/CORBA/BAD_PARAM:1.0";
/// `BAD_INV_ORDER`: the machine has no such edge from where the expert is —
/// prefetching something already resident, evicting something offloaded.
pub const BAD_INV_ORDER: &str = "IDL:omg.org/CORBA/BAD_INV_ORDER:1.0";
/// `NO_PERMISSION`: the expert is pinned. The one guard refusal that is not
/// "try again later" — a pin does not lapse with the window.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";
/// `TRANSIENT`: the §5 guard refused *this window* — no memory pressure, the
/// routing frequency has not fallen, or a call is inflight. Retry after the
/// next window and it may well succeed.
pub const TRANSIENT: &str = "IDL:omg.org/CORBA/TRANSIENT:1.0";

// ─────────────────────────────────────────────────────────────────────────────
// moe::Residency and moe::Capability on the wire
// ─────────────────────────────────────────────────────────────────────────────

/// The CDR ordinal of a `moe::Residency`, from the IDL's declaration order:
/// `enum Residency { OFFLOADED, PREFETCHING, RESIDENT, ACTIVE }` — so 0, 1, 2,
/// 3, marshalled as an `unsigned long` (§9.3.2.6).
///
/// Written as an explicit match rather than an `as u32` cast on the Rust enum:
/// the two orders agree today, and a cast would keep compiling on the day
/// somebody inserts a state into [`Residency`] for the state machine's
/// convenience and silently renumbers the wire.
pub fn residency_ordinal(state: Residency) -> u32 {
    match state {
        Residency::Offloaded => 0,
        Residency::Prefetching => 1,
        Residency::Resident => 2,
        Residency::Active => 3,
    }
}

/// The inverse of [`residency_ordinal`]. An ordinal outside 0..=3 is not a
/// `moe::Residency` and gets no guess.
pub fn residency_from_ordinal(ordinal: u32) -> Option<Residency> {
    match ordinal {
        0 => Some(Residency::Offloaded),
        1 => Some(Residency::Prefetching),
        2 => Some(Residency::Resident),
        3 => Some(Residency::Active),
        _ => None,
    }
}

/// `moe::Capability`, member for member and in declaration order.
///
/// Verified against `corpus/golden/22-moe-control-plane.idl` lines 21–31:
///
/// | # | IDL | CDR | Rust |
/// |---|---|---|---|
/// | 1 | `CapabilityId id` (`typedef string`) | string | `String` |
/// | 2 | `float cost` | float | `f32` |
/// | 3 | `float latency_p99_ms` | float | `f32` |
/// | 4 | `float load` | float | `f32` |
/// | 5 | `Residency state` | unsigned long | [`Residency`] |
/// | 6 | `unsigned long long mem_footprint` | unsigned long long | `u64` |
/// | 7 | `float route_freq` | float | `f32` |
/// | 8 | `string placement_node` | string | `String` |
/// | 9 | `string contract_version` | string | `String` |
///
/// `f32` and not `f64`: IDL `float` is 4 bytes and `double` is 8, and this
/// struct's alignment depends on it — member 6 is 8-aligned, so the three
/// floats before it decide where the padding lands.
#[derive(Debug, Clone, PartialEq)]
pub struct Capability {
    /// `CapabilityId id` — the offer key everything else here is looked up by.
    pub id: String,
    /// `float cost` — relative invocation cost.
    pub cost: f32,
    /// `float latency_p99_ms` — 99th-percentile latency, in milliseconds.
    pub latency_p99_ms: f32,
    /// `float load` — current load, `0.0..=1.0` by convention.
    pub load: f32,
    /// `Residency state` — where the expert says its weights are. A report:
    /// the loader is the authority (see the module docs).
    pub state: Residency,
    /// `unsigned long long mem_footprint` — accelerator bytes when resident.
    pub mem_footprint: u64,
    /// `float route_freq` — routing hits. Also a report: the store's decayed
    /// integer counter is the authority, and this is its lossy wire form.
    pub route_freq: f32,
    /// `string placement_node` — the node the expert is placed on.
    pub placement_node: String,
    /// `string contract_version` — carried verbatim; nothing here interprets
    /// it, and pretending to would be inventing a versioning policy.
    pub contract_version: String,
}

impl Capability {
    /// Marshals the struct in declaration order. Struct members are laid out
    /// consecutively with each member's own alignment (§9.3.2.5); there is no
    /// encapsulation and no length prefix.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_str(&self.id);
        out.put_f32(self.cost);
        out.put_f32(self.latency_p99_ms);
        out.put_f32(self.load);
        out.put_u32(residency_ordinal(self.state));
        out.put_u64(self.mem_footprint);
        out.put_f32(self.route_freq);
        out.put_str(&self.placement_node);
        out.put_str(&self.contract_version);
    }

    /// Demarshals what [`Capability::write_to`] wrote. An ordinal that is not
    /// a `moe::Residency` is a decoding failure, not an unknown state.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        let id = d.get_string()?;
        let cost = d.get_f32()?;
        let latency_p99_ms = d.get_f32()?;
        let load = d.get_f32()?;
        let ordinal = d.get_u32()?;
        let state = residency_from_ordinal(ordinal)
            .ok_or(orbweaver_cdr::Error::Malformed("moe::Residency ordinal outside 0..=3"))?;
        let mem_footprint = d.get_u64()?;
        let route_freq = d.get_f32()?;
        let placement_node = d.get_string()?;
        let contract_version = d.get_string()?;
        Ok(Capability {
            id,
            cost,
            latency_p99_ms,
            load,
            state,
            mem_footprint,
            route_freq,
            placement_node,
            contract_version,
        })
    }

    /// The offer this capability registers, with the two control-plane fields
    /// taken from the authority rather than from the wire: `residency` from
    /// the loader (passed in by the caller, which is the only thing that knows
    /// it) and `route_freq` seeded at zero.
    ///
    /// `specialization` and `latency_p50` have no member in this contract —
    /// see the module docs. They are left empty and zero rather than guessed
    /// from `latency_p99_ms`, which would put a fabricated number where a
    /// constraint query can read it.
    pub fn to_offer(&self, residency: Residency) -> Offer {
        Offer {
            id: self.id.clone(),
            specialization: String::new(),
            cost: f64::from(self.cost),
            latency_p50: 0.0,
            latency_p99: f64::from(self.latency_p99_ms),
            load: f64::from(self.load),
            residency,
            mem_footprint: self.mem_footprint,
            placement_node: self.placement_node.clone(),
            route_freq: 0,
        }
    }

    /// The capability an offer would be reported as: the shape a future
    /// `Expert::describe` hands back, which is why the encoder half of this
    /// module has a caller and not only a test.
    ///
    /// `route_freq` converts out of [`FREQ_SCALE`] units into hits, losing the
    /// fraction — the loss `orbweaver_trading`'s crate docs already call
    /// acceptable at the wire boundary, because reproducibility lives in the
    /// integer counter and not in this float.
    pub fn from_offer(offer: &Offer, state: Residency, contract_version: &str) -> Self {
        Capability {
            id: offer.id.clone(),
            cost: offer.cost as f32,
            latency_p99_ms: offer.latency_p99 as f32,
            load: offer.load as f32,
            state,
            mem_footprint: offer.mem_footprint,
            route_freq: offer.route_freq as f32 / FREQ_SCALE as f32,
            placement_node: offer.placement_node.clone(),
            contract_version: contract_version.to_owned(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The servant
// ─────────────────────────────────────────────────────────────────────────────

fn system(id: &str, completed: Completion) -> SystemException {
    SystemException { id: id.to_owned(), minor: 0, completed }
}

/// Maps a state-machine refusal onto the system exception a caller can act
/// on. The reason cannot travel any other way — these operations declare no
/// user exception — so the repository id carries the whole answer, and the
/// four cases are kept distinct because "eviction refused" without the reason
/// is exactly what [`GuardCondition`] exists to avoid.
fn refuse(err: &TransitionError) -> SystemException {
    match err {
        // Named an expert that is not here, or named one twice.
        TransitionError::Unknown { .. } | TransitionError::Duplicate { .. } => {
            system(BAD_PARAM, Completion::No)
        }
        // The machine has no such edge from where the expert actually is.
        TransitionError::Illegal { .. } | TransitionError::NotLoaded { .. } => {
            system(BAD_INV_ORDER, Completion::No)
        }
        // A pin does not lapse when the window closes; the other three
        // conditions do, so they are worth retrying and this one is not.
        TransitionError::Guarded { unmet: GuardCondition::Unpinned, .. } => {
            system(NO_PERMISSION, Completion::No)
        }
        TransitionError::Guarded { .. } => system(TRANSIENT, Completion::No),
    }
}

fn refuse_store(_err: &StoreError) -> SystemException {
    system(BAD_PARAM, Completion::No)
}

/// What the registry keeps about an expert besides its offer.
#[derive(Debug, Clone, PartialEq)]
struct Registered {
    /// The `Expert` reference as registered. Held verbatim and never dialled:
    /// a registry stores references, and a future `Router::select` hands this
    /// back rather than re-deriving an address the expert never published.
    reference: Ior,
    /// `Capability::contract_version`, which the offer store has no member
    /// for and which must survive to be reported back.
    contract_version: String,
}

/// `moe::ExpertRegistry` and `moe::ExpertLoader`, served together.
///
/// Two interfaces and one servant, because they are two views of one state:
/// registering an expert has to create it in *both* the offer store and the
/// residency machine, and nothing could keep two servants' halves in step
/// without a shared owner. They stay two *objects* — distinct object keys,
/// distinct repository ids, [`Dispatch::knows`] answering for both — because
/// the contract declares two interfaces and a client narrows to one of them.
///
/// This is not a POA-hosted object set. [`crate::Poa`] mints one repository
/// id per adapter, and these two references claim different ids; the POA does
/// come in for the *experts themselves*, which is
/// [`ExpertLoader::reconcile`](crate::residency::ExpertLoader::reconcile).
#[derive(Debug)]
pub struct ExpertService {
    host: String,
    port: u16,
    registry_key: Vec<u8>,
    loader_key: Vec<u8>,
    store: OfferStore,
    loader: ExpertLoader,
    refs: BTreeMap<String, Registered>,
    policy: LoadingPolicy,
    cold_below: u64,
    free_memory: u64,
}

impl ExpertService {
    /// A service whose two references point at `host:port`, keyed under
    /// `base_key` (`<base>/registry` and `<base>/loader`).
    ///
    /// `host` is separate from the bind address on purpose — Phase 0
    /// assumption D. `cold_below` is in [`FREQ_SCALE`] units, as
    /// [`BatchStats::from_store`] takes it.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        base_key: &[u8],
        policy: LoadingPolicy,
        cold_below: u64,
    ) -> Self {
        let key = |suffix: &str| {
            let mut k = base_key.to_vec();
            k.extend_from_slice(suffix.as_bytes());
            k
        };
        Self {
            host: host.into(),
            port,
            registry_key: key("/registry"),
            loader_key: key("/loader"),
            store: OfferStore::new(),
            loader: ExpertLoader::new(),
            refs: BTreeMap::new(),
            policy,
            cold_below,
            // No snapshot has been reported yet. Zero is the safe start: it
            // is below every sane low watermark, so the guard's *other* three
            // conditions still have to hold before anything is evicted.
            free_memory: 0,
        }
    }

    /// The `ExpertRegistry` object key.
    pub fn registry_key(&self) -> &[u8] {
        &self.registry_key
    }

    /// The `ExpertLoader` object key.
    pub fn loader_key(&self) -> &[u8] {
        &self.loader_key
    }

    /// A publishable `ExpertRegistry` reference.
    pub fn registry_ior(&self) -> Ior {
        self.ior_for(EXPERT_REGISTRY_ID, &self.registry_key)
    }

    /// A publishable `ExpertLoader` reference.
    pub fn loader_ior(&self) -> Ior {
        self.ior_for(EXPERT_LOADER_ID, &self.loader_key)
    }

    fn ior_for(&self, type_id: &str, key: &[u8]) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    /// The offer store, for queries and for tests. Read-only: every mutation
    /// has to go through an operation that keeps the loader in step.
    pub fn store(&self) -> &OfferStore {
        &self.store
    }

    /// The residency machine, read-only, for the same reason.
    pub fn loader(&self) -> &ExpertLoader {
        &self.loader
    }

    /// The reference `register_expert` was given for `id` — what a router
    /// hands back when it selects this expert.
    pub fn reference_for(&self, id: &str) -> Option<&Ior> {
        self.refs.get(id).map(|r| &r.reference)
    }

    /// The capability this service would report for `id`, with the loader's
    /// residency rather than the one the expert last claimed.
    pub fn capability_of(&self, id: &str) -> Option<Capability> {
        let offer = self.store.get(id)?;
        let registered = self.refs.get(id)?;
        let state = self.loader.status(id).unwrap_or(offer.residency);
        Some(Capability::from_offer(offer, state, &registered.contract_version))
    }

    /// Records the free accelerator memory the control loop observed.
    ///
    /// The loader cannot know this and neither can the store — F3's
    /// [`BatchStats`] doc is explicit that it arrives per window from outside.
    /// A wire `evict` is evaluated against the *last reported* snapshot, so a
    /// control loop that never reports one gets a service that never evicts,
    /// which is the right failure direction.
    pub fn observe_free_memory(&mut self, bytes: u64) {
        self.free_memory = bytes;
    }

    /// Records one routing hit against `id`'s decayed counter — §6's feedback
    /// loop, and F4's telemetry when it lands.
    ///
    /// Deliberately not a wire operation: it fires per routed call, and a
    /// per-call operation on this contract is the per-token surface §5
    /// forbids. It arrives in-process from the router instead.
    pub fn record_hit(&mut self, id: &str) -> bool {
        self.store.add_hit(id)
    }

    /// PREFETCHING → RESIDENT: the weight copy finished.
    ///
    /// No wire operation either, and for a different reason: nothing calls
    /// this from outside because nothing outside performs the copy. Whoever
    /// does — in this repository, the spike standing in for it — reports here.
    pub fn complete_load(&mut self, id: &str) -> Result<Residency, TransitionError> {
        self.transition(|l| l.complete_load(id))
    }

    /// A call began on `id`: RESIDENT → ACTIVE, or one more inflight.
    pub fn begin_call(&mut self, id: &str) -> Result<Residency, TransitionError> {
        self.transition(|l| l.begin_call(id))
    }

    /// A call on `id` finished.
    pub fn end_call(&mut self, id: &str) -> Result<Residency, TransitionError> {
        self.transition(|l| l.end_call(id))
    }

    /// One batch window: decide, apply, mirror, decay.
    ///
    /// This is the whole control loop, and the reason this servant exists.
    /// In order:
    ///
    /// 1. the §6 policy decides from the store, the reported free memory and
    ///    the **loader's** inflight set — so the policy never proposes
    ///    evicting an expert the guard would refuse for that reason;
    /// 2. the loader applies the decisions under a [`BatchStats`] derived from
    ///    the same store and the same free memory;
    /// 3. the store's residency mirror is updated from what the loader
    ///    actually did — refusals included, which is why this reads the
    ///    machine's state rather than assuming the decisions took;
    /// 4. the window closes and every `route_freq` decays.
    ///
    /// Returns one [`Applied`] per decision. Not a bare `Vec<Decision>`: that
    /// is the list the policy *asked* for, and F3's whole point is that the
    /// guard may refuse. Dropping the outcomes would hand the caller a list of
    /// things that did not necessarily happen — `Applied::decision` is still
    /// there for a caller that only wants the ask.
    ///
    /// Step 4 makes two consecutive applications differ: the second sees
    /// decayed counters, because a window closed. That is the semantics, not
    /// an accident — call it once per window.
    pub fn apply_policy(&mut self, free_memory: u64) -> Vec<Applied> {
        self.free_memory = free_memory;
        let decisions: Vec<Decision> =
            self.policy.decide(&self.store, free_memory, &self.loader.inflight_ids());
        let stats = self.window();
        let applied = self.loader.apply(&decisions, &stats);
        self.mirror_residency();
        self.store.decay_all();
        applied
    }

    /// The window the eviction guard reads, from the last reported snapshot.
    fn window(&self) -> BatchStats {
        BatchStats::from_store(&self.store, &self.policy, self.free_memory, self.cold_below)
    }

    /// Runs one loader transition and mirrors the result into the store.
    ///
    /// Every mutation of the machine goes through here, and that is the point.
    /// The first version mirrored only at the end of
    /// [`apply_policy`](ExpertService::apply_policy), and the drift was
    /// immediate and silent: a wire `prefetch` and an out-of-band
    /// `complete_load` moved three experts to RESIDENT while the offer store
    /// still said OFFLOADED, so `LoadingPolicy::decide` — whose eviction
    /// candidates are the *store's* resident offers — found none and returned
    /// an empty decision list under real memory pressure. Nothing failed
    /// loudly; the control plane simply stopped deciding. Mirroring at the
    /// single choke point is what makes that impossible rather than merely
    /// fixed at the two call sites where it was noticed.
    fn transition<F>(&mut self, op: F) -> Result<Residency, TransitionError>
    where
        F: FnOnce(&mut ExpertLoader) -> Result<Residency, TransitionError>,
    {
        let outcome = op(&mut self.loader);
        // Mirrored even when the transition refused: a refusal leaves the
        // machine untouched, so the copies agree either way, and a mirror
        // conditioned on success is one more branch to get wrong.
        self.mirror_residency();
        outcome
    }

    /// Copies the loader's residency map into the offer store, so the policy
    /// reads what the machine did rather than what it was asked.
    fn mirror_residency(&mut self) {
        for (id, state) in self.loader.states() {
            self.store.set_residency(&id, state);
        }
    }

    // ── the ExpertRegistry operations ───────────────────────────────────────

    fn register_expert(&mut self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        // Checked against both halves before either is touched: a partial
        // registration would leave an offer the loader has never heard of,
        // and `deregister` keys off the reference table that comes last.
        if self.store.get(&cap.id).is_some() || self.loader.status(&cap.id).is_some() {
            return Err(system(BAD_PARAM, Completion::No));
        }
        // PERSISTENT, and not selectable: §4.2's TRANSIENT drops the
        // adaptation state on eviction, this contract has no member to ask
        // for that, and defaulting to the lossy one would make eviction quietly
        // destructive. Adding a member is a contract change (F1), not a
        // default chosen here.
        self.loader.register(&cap.id, Lifespan::Persistent).map_err(|e| refuse(&e))?;
        let offer = cap.to_offer(Residency::Offloaded);
        self.store.register(offer).map_err(|e| refuse_store(&e))?;
        self.refs.insert(
            cap.id.clone(),
            Registered { reference, contract_version: cap.contract_version.clone() },
        );
        Ok(())
    }

    fn deregister(&mut self, reference: &Ior) -> Result<(), SystemException> {
        // `deregister(in Expert e)` carries no CapabilityId, so the reference
        // is the only key there is. §7.2.1 lets reference comparison answer
        // "different" about two references to the same object, so an expert
        // that re-addressed itself without heartbeating will not be found —
        // refused loudly rather than matched by a guess.
        let Some(id) = self
            .refs
            .iter()
            .find(|(_, r)| is_equivalent(&r.reference, reference))
            .map(|(id, _)| id.clone())
        else {
            return Err(system(BAD_PARAM, Completion::No));
        };
        if self.loader.inflight(&id) > 0 {
            // Forgetting an expert mid-call would strand the caller. Retry
            // when the call ends — genuinely transient.
            return Err(system(TRANSIENT, Completion::No));
        }
        self.loader.forget(&id);
        self.store.deregister(&id);
        self.refs.remove(&id);
        Ok(())
    }

    fn heartbeat(&mut self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        let Some(registered) = self.refs.get_mut(&cap.id) else {
            return Err(system(BAD_PARAM, Completion::No));
        };
        // A heartbeat re-announces the expert, address included: an expert
        // that moved says so here, and this is what keeps `deregister`'s
        // reference lookup able to find it afterwards.
        registered.reference = reference;
        registered.contract_version = cap.contract_version.clone();
        let state = self.loader.status(&cap.id).unwrap_or(Residency::Offloaded);
        // route_freq is dropped by OfferStore::heartbeat; state comes from the
        // loader. Both are in the table in the module docs.
        self.store.heartbeat(cap.to_offer(state)).map_err(|e| refuse_store(&e))
    }

    // ── the ExpertLoader operations ─────────────────────────────────────────

    fn prefetch(&mut self, id: &str) -> Result<(), SystemException> {
        self.transition(|l| l.request_prefetch(id)).map(|_| ()).map_err(|e| refuse(&e))
    }

    fn evict(&mut self, id: &str) -> Result<(), SystemException> {
        let stats = self.window();
        self.transition(|l| l.evict(id, &stats)).map(|_| ()).map_err(|e| refuse(&e))
    }

    fn pin(&mut self, id: &str) -> Result<(), SystemException> {
        if !self.loader.pin(id) {
            return Err(system(BAD_PARAM, Completion::No));
        }
        // Both copies, together: the loader's guard stays the authority, but
        // a policy that cannot see the pin proposes evictions that are always
        // refused, and F3 already showed two pin sets drifting apart.
        self.store.pin(id);
        Ok(())
    }

    fn status(&self, id: &str) -> Result<Residency, SystemException> {
        // An unregistered expert is not OFFLOADED. F3 refused to answer that
        // lie locally and this is the same refusal on the wire.
        self.loader.status(id).ok_or_else(|| system(BAD_PARAM, Completion::No))
    }

    /// Serves one operation, writing the reply body into `out`.
    fn handle(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut args = req.body().map_err(|_| SystemException::marshal())?;
        let on_registry = req.object_key == self.registry_key;

        // Every ORB probes with these before it trusts a narrow, and the
        // answer differs by which of the two objects was addressed.
        match req.operation.as_str() {
            "_is_a" => {
                let want = args.get_string().map_err(|_| SystemException::marshal())?;
                let mine = if on_registry { EXPERT_REGISTRY_ID } else { EXPERT_LOADER_ID };
                out.put_bool(want == mine || want == OBJECT_ID);
                return Ok(());
            }
            "_non_existent" | "_not_existent" => {
                out.put_bool(false);
                return Ok(());
            }
            _ => {}
        }

        if on_registry {
            match req.operation.as_str() {
                "register_expert" => {
                    let reference = get_reference(&mut args)
                        .map_err(|_| SystemException::marshal())?
                        .ok_or_else(|| system(BAD_PARAM, Completion::No))?;
                    let cap =
                        Capability::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    self.register_expert(reference, &cap)
                }
                "deregister" => {
                    let reference = get_reference(&mut args)
                        .map_err(|_| SystemException::marshal())?
                        .ok_or_else(|| system(BAD_PARAM, Completion::No))?;
                    self.deregister(&reference)
                }
                "heartbeat" => {
                    let reference = get_reference(&mut args)
                        .map_err(|_| SystemException::marshal())?
                        .ok_or_else(|| system(BAD_PARAM, Completion::No))?;
                    let cap =
                        Capability::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    self.heartbeat(reference, &cap)
                }
                _ => Err(SystemException::bad_operation()),
            }
        } else {
            // Every `ExpertLoader` operation takes exactly one
            // `in CapabilityId`, so the argument is read once — but only
            // after the operation is known to be one of ours, or an unserved
            // operation with some other body would answer MARSHAL where it
            // owes BAD_OPERATION.
            if !matches!(req.operation.as_str(), "prefetch" | "evict" | "pin" | "status") {
                return Err(SystemException::bad_operation());
            }
            let id = args.get_string().map_err(|_| SystemException::marshal())?;
            match req.operation.as_str() {
                // `oneway void prefetch(in CapabilityId id)`. A refusal here
                // is encoded and then dropped by the Server, because a oneway
                // has no reply to carry it — the caller learns about it from
                // the next `status`, which is what oneway means.
                "prefetch" => self.prefetch(&id),
                "evict" => self.evict(&id),
                "pin" => self.pin(&id),
                "status" => {
                    let state = self.status(&id)?;
                    out.put_u32(residency_ordinal(state));
                    Ok(())
                }
                // Unreachable: the guard above enumerates the same four.
                _ => Err(SystemException::bad_operation()),
            }
        }
    }
}

impl Dispatch for ExpertService {
    /// Two object keys, one servant — see the type docs.
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.registry_key || object_key == self.loader_key
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.handle(request, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::Endian;
    use orbweaver_giop::server::Server;
    use orbweaver_giop::{Connection, DEFAULT_MAX_MESSAGE_SIZE, Error, ReplyStatus};
    use std::io::Write;
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const T: Duration = Duration::from_secs(5);

    fn policy() -> LoadingPolicy {
        LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 }
    }

    /// Two hits' worth of history, the same unit `BatchStats::from_store` takes.
    const COLD_BELOW: u64 = 2 * FREQ_SCALE;

    fn expert_ref(name: &str) -> Ior {
        Ior {
            type_id: EXPERT_ID.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "192.0.2.7".into(),
                port: 4242,
                object_key: name.as_bytes().to_vec(),
                components: Vec::new(),
            }],
        }
    }

    fn cap(id: &str, mem: u64) -> Capability {
        Capability {
            id: id.to_owned(),
            cost: 1.5,
            latency_p99_ms: 42.0,
            load: 0.25,
            state: Residency::Offloaded,
            mem_footprint: mem,
            route_freq: 0.0,
            placement_node: "node-a".into(),
            contract_version: "moe/1.0".into(),
        }
    }

    fn service() -> ExpertService {
        ExpertService::new("127.0.0.1", 4001, b"MoE", policy(), COLD_BELOW)
    }

    // ── the wire layout ─────────────────────────────────────────────────────

    /// The IDL's declaration order, decoded with the primitive getters rather
    /// than with `read_from` — so a member swapped in *both* directions still
    /// fails here. Both byte orders, because an encoder that only works
    /// native-endian passes every local test and fails in the field.
    #[test]
    fn capability_members_are_in_the_idls_declaration_order() {
        let c = Capability {
            id: "expert-math".into(),
            cost: 0.5,
            latency_p99_ms: 180.25,
            load: 0.75,
            state: Residency::Resident,
            mem_footprint: 8_589_934_592,
            route_freq: 3.5,
            placement_node: "gpu-04".into(),
            contract_version: "moe/1.0".into(),
        };
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            c.write_to(&mut e);
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            // corpus/golden/22 lines 21–31, member by member.
            assert_eq!(d.get_string().unwrap(), "expert-math", "1 id {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 0.5, "2 cost {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 180.25, "3 latency_p99_ms {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 0.75, "4 load {endian:?}");
            assert_eq!(d.get_u32().unwrap(), 2, "5 state = RESIDENT {endian:?}");
            assert_eq!(d.get_u64().unwrap(), 8_589_934_592, "6 mem_footprint {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 3.5, "7 route_freq {endian:?}");
            assert_eq!(d.get_string().unwrap(), "gpu-04", "8 placement_node {endian:?}");
            assert_eq!(d.get_string().unwrap(), "moe/1.0", "9 contract_version {endian:?}");

            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(Capability::read_from(&mut d).unwrap(), c, "round trip {endian:?}");
        }
    }

    /// `enum Residency { OFFLOADED, PREFETCHING, RESIDENT, ACTIVE }` — the
    /// ordinals are the contract, and 4 is not a state.
    #[test]
    fn residency_ordinals_are_the_idls_declaration_order() {
        let expected = [
            (Residency::Offloaded, 0),
            (Residency::Prefetching, 1),
            (Residency::Resident, 2),
            (Residency::Active, 3),
        ];
        for (state, ordinal) in expected {
            assert_eq!(residency_ordinal(state), ordinal, "{state:?}");
            assert_eq!(residency_from_ordinal(ordinal), Some(state), "{ordinal}");
        }
        assert_eq!(residency_from_ordinal(4), None, "there is no fifth state");
    }

    /// A capability that survived the wire, became an offer and came back:
    /// the fields the contract carries are preserved, and the two the store
    /// owns are the store's answer, not the sender's.
    #[test]
    fn a_capability_round_trips_through_the_offer_store_minus_what_it_cannot_carry() {
        let mut svc = service();
        let mut sent = cap("expert-math", 4096);
        sent.state = Residency::Active; // an expert claiming to be busy
        sent.route_freq = 99.0; // …with a routing history it invented
        svc.register_expert(expert_ref("expert-math"), &sent).unwrap();

        let got = svc.capability_of("expert-math").unwrap();
        assert_eq!(got.id, sent.id);
        assert_eq!(got.cost, sent.cost);
        assert_eq!(got.latency_p99_ms, sent.latency_p99_ms);
        assert_eq!(got.load, sent.load);
        assert_eq!(got.mem_footprint, sent.mem_footprint);
        assert_eq!(got.placement_node, sent.placement_node);
        assert_eq!(got.contract_version, sent.contract_version);
        assert_eq!(got.state, Residency::Offloaded, "the loader is the authority on state");
        assert_eq!(got.route_freq, 0.0, "the store owns routing history");
        // The two members the contract has no room for.
        let offer = svc.store().get("expert-math").unwrap();
        assert_eq!(offer.specialization, "", "moe::Capability declares no specialization");
        assert_eq!(offer.latency_p50, 0.0, "…and no p50");
    }

    // ── a served instance ───────────────────────────────────────────────────

    /// The service on loopback. `Server` handles one connection at a time, so
    /// clients are used strictly sequentially and `shutdown` is called with
    /// the last one still open — the stop flag is raised before it drops, so
    /// the serve loop observes it after the connection ends rather than
    /// blocking in accept.
    struct Served {
        registry: Ior,
        loader: Ior,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<ExpertService>>,
    }

    impl Served {
        fn start(mut svc: ExpertService) -> Self {
            let server = Server::bind("127.0.0.1:0", b"MoE/registry".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            svc.host = "127.0.0.1".into();
            svc.port = port;
            let (registry, loader) = (svc.registry_ior(), svc.loader_ior());
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve(&mut svc, || flag.load(Ordering::SeqCst)).unwrap();
                svc
            });
            Served { registry, loader, stop, thread: Some(thread) }
        }

        fn registry(&self) -> Connection {
            Connection::connect(&self.registry, T).unwrap()
        }

        fn loader(&self) -> Connection {
            Connection::connect(&self.loader, T).unwrap()
        }

        fn shutdown(mut self, last: Connection) -> ExpertService {
            self.stop.store(true, Ordering::SeqCst);
            drop(last);
            self.thread.take().unwrap().join().unwrap()
        }
    }

    fn do_register(c: &mut Connection, name: &str, mem: u64) -> Result<(), Error> {
        let r = expert_ref(name);
        let capability = cap(name, mem);
        c.invoke("register_expert", |e| {
            r.write_to(e).unwrap();
            capability.write_to(e);
        })
        .map(|_| ())
    }

    fn do_status(c: &mut Connection, name: &str) -> Result<Residency, Error> {
        let id = name.to_owned();
        let reply = c.invoke("status", move |e| e.put_str(&id))?;
        let ordinal = reply.body()?.get_u32()?;
        Ok(residency_from_ordinal(ordinal).expect("a served ordinal is a state"))
    }

    fn exception_id(err: Error) -> String {
        match err {
            Error::SystemException { id, .. } => id,
            other => panic!("expected a system exception, got {other:?}"),
        }
    }

    /// The whole join, over the wire: three registrations, a heartbeat that
    /// changes an offer, a policy pass whose decision the heartbeat decided,
    /// and the loader state it produced.
    #[test]
    fn the_registry_the_policy_and_the_loader_are_one_path() {
        let served = Served::start(service());
        let mut reg = served.registry();
        for (name, mem) in [("expert-code", 30), ("expert-math", 200), ("expert-vision", 100)] {
            do_register(&mut reg, name, mem).expect("registers");
        }
        // The heartbeat: expert-code now occupies twice what it registered.
        let r = expert_ref("expert-code");
        let mut updated = cap("expert-code", 60);
        updated.load = 0.9;
        reg.invoke("heartbeat", |e| {
            r.write_to(e).unwrap();
            updated.write_to(e);
        })
        .expect("heartbeats");
        drop(reg);

        let mut ldr = served.loader();
        for name in ["expert-code", "expert-math", "expert-vision"] {
            assert_eq!(do_status(&mut ldr, name).unwrap(), Residency::Offloaded, "{name}");
            ldr.invoke_oneway("prefetch", {
                let id = name.to_owned();
                move |e| e.put_str(&id)
            })
            .expect("prefetch is oneway");
            assert_eq!(do_status(&mut ldr, name).unwrap(), Residency::Prefetching, "{name}");
        }
        let mut svc = served.shutdown(ldr);

        assert_eq!(svc.store().get("expert-code").unwrap().mem_footprint, 60, "heartbeat landed");
        // …and 0.9 is not 0.9 on the other side: the contract declares
        // `float`, so the offer's f64 holds what an f32 could carry. Pinned
        // as the f32 widening rather than papered over with a tolerance,
        // because the lossy step is the contract's, not an arithmetic slip.
        assert_eq!(svc.store().get("expert-code").unwrap().load, f64::from(0.9_f32));

        // The copies land, one call arrives, and the routing history that the
        // feedback loop (never the wire) owns is recorded.
        for name in ["expert-code", "expert-math", "expert-vision"] {
            svc.complete_load(name).expect("the copy finishes");
        }
        svc.begin_call("expert-math").expect("a call arrives");
        svc.record_hit("expert-code");
        svc.record_hit("expert-math");
        for _ in 0..3 {
            svc.record_hit("expert-vision");
        }

        // free 50 < low 100: eviction, LFU. code(16) and math(16) tie and
        // code sorts first, but math is ACTIVE and never a candidate — so
        // code goes, and 50+60 = 110 reaches the watermark, which is exactly
        // what the heartbeat bought: at the registered 30 bytes the
        // projection would have fallen short and taken expert-vision too.
        let applied = svc.apply_policy(50);
        assert_eq!(
            applied,
            vec![Applied {
                decision: Decision::Evict("expert-code".to_owned()),
                outcome: Ok(Residency::Offloaded),
            }]
        );
        assert_eq!(svc.loader().status("expert-code"), Some(Residency::Offloaded));
        assert_eq!(
            svc.store().get("expert-code").unwrap().residency,
            Residency::Offloaded,
            "the store mirrors what the loader actually did"
        );
        assert_eq!(svc.store().get("expert-math").unwrap().residency, Residency::Active);
    }

    /// The regression for the drift that made the test above fail the first
    /// time it ran: mirroring only at the end of `apply_policy` let the offer
    /// store say OFFLOADED while the machine said RESIDENT, and since the §6
    /// policy's eviction candidates are the *store's* resident offers, the
    /// control plane silently stopped deciding under real pressure. Every
    /// transition now goes through one choke point, so the two copies agree
    /// after every operation and not only at the end of a window.
    #[test]
    fn the_offer_store_never_lags_the_state_machine() {
        let mut svc = service();
        for name in ["expert-a", "expert-b"] {
            svc.register_expert(expert_ref(name), &cap(name, 10)).unwrap();
        }
        let agree = |svc: &ExpertService, what: &str| {
            for (id, state) in svc.loader().states() {
                assert_eq!(
                    svc.store().get(&id).map(|o| o.residency),
                    Some(state),
                    "{what}: {id} disagrees"
                );
            }
        };
        agree(&svc, "after registration");
        svc.prefetch("expert-a").unwrap();
        agree(&svc, "after a wire prefetch");
        svc.complete_load("expert-a").unwrap();
        agree(&svc, "after the copy landed");
        svc.begin_call("expert-a").unwrap();
        agree(&svc, "after a call began");
        svc.end_call("expert-a").unwrap();
        agree(&svc, "after it ended");
        // …and a refusal leaves them agreeing too: nothing moved.
        assert!(svc.prefetch("expert-a").is_err(), "RESIDENT → PREFETCHING is not an edge");
        agree(&svc, "after a refusal");
    }

    /// The counterfactual the test above asserts in prose: without the
    /// heartbeat's larger footprint the same window evicts a second expert.
    /// Two services differing in exactly one heartbeat, so the join is shown
    /// to be load bearing rather than merely present.
    #[test]
    fn the_heartbeat_is_what_changes_the_decision() {
        let prepared = |heartbeat: bool| {
            let mut svc = service();
            for (name, mem) in [("expert-code", 30), ("expert-vision", 100)] {
                svc.register_expert(expert_ref(name), &cap(name, mem)).unwrap();
                svc.prefetch(name).unwrap();
                svc.complete_load(name).unwrap();
            }
            if heartbeat {
                svc.heartbeat(expert_ref("expert-code"), &cap("expert-code", 60)).unwrap();
            }
            svc.record_hit("expert-code");
            for _ in 0..3 {
                svc.record_hit("expert-vision");
            }
            svc
        };

        let decisions = |svc: &mut ExpertService| -> Vec<Decision> {
            svc.apply_policy(50).into_iter().map(|a| a.decision).collect()
        };
        assert_eq!(
            decisions(&mut prepared(false)),
            vec![
                Decision::Evict("expert-code".to_owned()),
                Decision::Evict("expert-vision".to_owned()),
            ],
            "50 + 30 = 80 is short of the watermark, so a second expert goes"
        );
        assert_eq!(
            decisions(&mut prepared(true)),
            vec![Decision::Evict("expert-code".to_owned())],
            "50 + 60 = 110 reaches it, and expert-vision stays resident"
        );
    }

    // ── oneway ──────────────────────────────────────────────────────────────

    /// `oneway void prefetch(...)` must produce no reply at all — not an
    /// empty one. Proven at the byte level: a oneway followed by an ordinary
    /// request, and the very next message off the socket is the reply to the
    /// *second*. A stray empty reply would arrive first and this would read
    /// request id 1 where it wants 2.
    #[test]
    fn a_oneway_prefetch_writes_no_reply_bytes() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let mut svc = service();
                svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
                let served = Served::start(svc);
                let addr = served.loader.primary().unwrap();
                let key = addr.object_key.clone();
                let mut s = TcpStream::connect((addr.host.as_str(), addr.port)).expect("connects");

                let oneway = orbweaver_giop::encode_request(
                    version,
                    endian,
                    1,
                    &key,
                    "prefetch",
                    false,
                    |e| e.put_str("expert-a"),
                )
                .unwrap();
                s.write_all(&oneway).unwrap();
                let query =
                    orbweaver_giop::encode_request(version, endian, 2, &key, "status", true, |e| {
                        e.put_str("expert-a")
                    })
                    .unwrap();
                s.write_all(&query).unwrap();

                let msg = orbweaver_giop::read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                let reply = orbweaver_giop::decode_reply(msg).unwrap();
                assert_eq!(reply.request_id, 2, "{version} {endian:?}: the oneway replied");
                assert_eq!(reply.status, ReplyStatus::NoException);
                assert_eq!(
                    reply.body().unwrap().get_u32().unwrap(),
                    1,
                    "{version} {endian:?}: PREFETCHING — the oneway was served, just not answered"
                );
                drop(s);
                let last = served.loader();
                served.shutdown(last);
            }
        }
    }

    /// A oneway whose operation *fails* is equally silent: the refusal is
    /// encoded and dropped, and the caller learns from the next `status`.
    #[test]
    fn a_refused_oneway_is_silent_too() {
        let served = Served::start(service());
        let mut ldr = served.loader();
        ldr.invoke_oneway("prefetch", |e| e.put_str("expert-ghost")).expect("bytes went out");
        // If the server had answered the oneway, this invoke would read that
        // reply and fail as desynchronised rather than answering BAD_PARAM.
        let err = do_status(&mut ldr, "expert-ghost").unwrap_err();
        assert_eq!(exception_id(err), BAD_PARAM);
        served.shutdown(ldr);
    }

    // ── refusal shapes ──────────────────────────────────────────────────────

    /// Every operation, against an id nobody registered: `BAD_PARAM`, never a
    /// plausible default. `status` is the one that matters — answering
    /// OFFLOADED for an expert nobody has heard of is the lie F3 refused to
    /// tell locally.
    #[test]
    fn an_unknown_id_is_bad_param_on_every_operation() {
        let served = Served::start(service());
        let mut ldr = served.loader();
        for op in ["evict", "pin", "status"] {
            let err = ldr.invoke(op, |e| e.put_str("expert-ghost")).unwrap_err();
            assert_eq!(exception_id(err), BAD_PARAM, "{op}");
        }
        drop(ldr);

        let mut reg = served.registry();
        let ghost = expert_ref("expert-ghost");
        let capability = cap("expert-ghost", 1);
        let err = reg
            .invoke("heartbeat", |e| {
                ghost.write_to(e).unwrap();
                capability.write_to(e);
            })
            .unwrap_err();
        assert_eq!(exception_id(err), BAD_PARAM, "heartbeat for an unregistered expert");
        let err = reg.invoke("deregister", |e| ghost.write_to(e).unwrap()).unwrap_err();
        assert_eq!(exception_id(err), BAD_PARAM, "deregister of an unknown reference");
        served.shutdown(reg);
    }

    /// The four guard conditions, each mapped to the exception a caller can
    /// act on: pinned is `NO_PERMISSION` (a pin does not lapse), the other
    /// three are `TRANSIENT` (the next window may differ), and a missing edge
    /// is `BAD_INV_ORDER` (the request was wrong, not merely early).
    #[test]
    fn each_refusal_reaches_the_wire_as_its_own_exception() {
        let mut svc = service();
        for name in ["expert-hot", "expert-cold", "expert-busy", "expert-pinned"] {
            svc.register_expert(expert_ref(name), &cap(name, 10)).unwrap();
        }
        // hot: resident and still hot. cold/busy/pinned: resident and cold.
        for name in ["expert-hot", "expert-cold", "expert-busy", "expert-pinned"] {
            svc.prefetch(name).unwrap();
            svc.complete_load(name).unwrap();
        }
        for _ in 0..3 {
            svc.record_hit("expert-hot");
        }
        svc.begin_call("expert-busy").unwrap();

        let served = Served::start(svc);
        let mut ldr = served.loader();

        // No snapshot reported yet: free memory is 0, which IS under the low
        // watermark, so pressure holds and the guard's later conditions are
        // the ones under test. (The no-pressure refusal is the local test
        // below, where the snapshot can be raised.)
        ldr.invoke("pin", |e| e.put_str("expert-pinned")).expect("pins");
        for (id, want, unchanged) in [
            // route_freq has not fallen
            ("expert-hot", TRANSIENT, Residency::Resident),
            // a call is inflight — which is what ACTIVE means
            ("expert-busy", TRANSIENT, Residency::Active),
            // pinned
            ("expert-pinned", NO_PERMISSION, Residency::Resident),
        ] {
            let owned = id.to_owned();
            let err = ldr.invoke("evict", move |e| e.put_str(&owned)).unwrap_err();
            assert_eq!(exception_id(err), want, "evict {id}");
            assert_eq!(do_status(&mut ldr, id).unwrap(), unchanged, "{id} moved");
        }
        // …and the one that is allowed, so the fixture is not vacuous.
        ldr.invoke("evict", |e| e.put_str("expert-cold")).expect("evicts");
        assert_eq!(do_status(&mut ldr, "expert-cold").unwrap(), Residency::Offloaded);
        // An edge the machine does not have: prefetch is not evict's inverse
        // for something already offloaded… it is, so evict it twice instead.
        let err = ldr.invoke("evict", |e| e.put_str("expert-cold")).unwrap_err();
        assert_eq!(exception_id(err), BAD_INV_ORDER, "OFFLOADED → OFFLOADED is not an edge");
        served.shutdown(ldr);
    }

    /// The fourth condition, which needs a reported snapshot above the
    /// watermark rather than the zero a fresh service starts at.
    #[test]
    fn without_memory_pressure_eviction_is_transient() {
        let mut svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.prefetch("expert-a").unwrap();
        svc.complete_load("expert-a").unwrap();
        svc.observe_free_memory(10_000);
        let err = svc.evict("expert-a").unwrap_err();
        assert_eq!(err.id, TRANSIENT);
        assert_eq!(svc.loader().status("expert-a"), Some(Residency::Resident));
    }

    /// Registering twice would reset the residency and the routing history,
    /// and both halves must refuse it — the store's and the loader's.
    #[test]
    fn a_duplicate_registration_refuses_and_leaves_both_halves_untouched() {
        let mut svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.record_hit("expert-a");
        let err = svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 999)).unwrap_err();
        assert_eq!(err.id, BAD_PARAM);
        assert_eq!(svc.store().get("expert-a").unwrap().mem_footprint, 10);
        assert_eq!(svc.store().get("expert-a").unwrap().route_freq, FREQ_SCALE);
        assert_eq!(svc.store().len(), 1);
    }

    /// `deregister` has only the reference to go on, so the reference table,
    /// the offer and the loader entry must all go — and the heartbeat that
    /// re-announced an address is what keeps the lookup working afterwards.
    #[test]
    fn deregistration_is_by_reference_and_clears_all_three_halves() {
        let mut svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();

        // A reference to the same object at a different address: §7.2.1 lets
        // comparison say "different", so this is refused rather than guessed.
        let mut moved = expert_ref("expert-a");
        moved.profiles[0].host = "192.0.2.99".into();
        assert_eq!(svc.deregister(&moved).unwrap_err().id, BAD_PARAM);
        assert_eq!(svc.store().len(), 1, "the refusal changed nothing");

        // The heartbeat is how an expert that moved says so.
        svc.heartbeat(moved.clone(), &cap("expert-a", 10)).unwrap();
        assert_eq!(svc.reference_for("expert-a"), Some(&moved));
        svc.deregister(&moved).unwrap();
        assert!(svc.store().get("expert-a").is_none());
        assert_eq!(svc.loader().status("expert-a"), None, "the loader forgot it too");
        assert_eq!(svc.reference_for("expert-a"), None);
    }

    /// Forgetting an expert with a call inflight would strand the caller.
    #[test]
    fn deregistration_waits_for_an_inflight_call() {
        let mut svc = service();
        let r = expert_ref("expert-a");
        svc.register_expert(r.clone(), &cap("expert-a", 10)).unwrap();
        svc.prefetch("expert-a").unwrap();
        svc.complete_load("expert-a").unwrap();
        svc.begin_call("expert-a").unwrap();
        assert_eq!(svc.deregister(&r).unwrap_err().id, TRANSIENT);
        svc.end_call("expert-a").unwrap();
        svc.deregister(&r).expect("the call finished");
    }

    /// A pin set through the wire has to reach both copies, or the policy
    /// keeps proposing an eviction the guard will always refuse.
    #[test]
    fn pinning_reaches_both_the_loader_and_the_store() {
        let mut svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.pin("expert-a").unwrap();
        assert!(svc.loader().is_pinned("expert-a"));
        assert!(svc.store().is_pinned("expert-a"));
    }

    /// Both objects answer `_is_a` for their own interface and for
    /// `CORBA::Object`, and for nothing else. An operation outside the served
    /// surface is `BAD_OPERATION` — including one that belongs to the *other*
    /// interface, which is what makes these two objects rather than one.
    #[test]
    fn each_object_answers_only_for_its_own_interface() {
        let served = Served::start(service());
        let mut reg = served.registry();
        for (id, want) in [(EXPERT_REGISTRY_ID, true), (OBJECT_ID, true), (EXPERT_LOADER_ID, false)]
        {
            let reply = reg.invoke("_is_a", move |e| e.put_str(id)).unwrap();
            assert!(reply.body().unwrap().get_bool().unwrap() == want, "registry _is_a {id}");
        }
        let err = reg.invoke("status", |e| e.put_str("expert-a")).unwrap_err();
        assert_eq!(
            exception_id(err),
            orbweaver_giop::server::BAD_OPERATION,
            "loader op on registry"
        );
        drop(reg);

        let mut ldr = served.loader();
        for (id, want) in [(EXPERT_LOADER_ID, true), (OBJECT_ID, true), (EXPERT_REGISTRY_ID, false)]
        {
            let reply = ldr.invoke("_is_a", move |e| e.put_str(id)).unwrap();
            assert!(reply.body().unwrap().get_bool().unwrap() == want, "loader _is_a {id}");
        }
        assert!(!ldr.invoke_nullary("_non_existent").unwrap().body().unwrap().get_bool().unwrap());
        let err = ldr.invoke_nullary("select").unwrap_err();
        assert_eq!(exception_id(err), orbweaver_giop::server::BAD_OPERATION, "Router op");
        served.shutdown(ldr);
    }

    /// An object key this service never minted is nobody's.
    #[test]
    fn an_unknown_object_key_is_not_ours() {
        let svc = service();
        assert!(svc.knows(svc.registry_key()));
        assert!(svc.knows(svc.loader_key()));
        assert!(!svc.knows(b"MoE"), "the base key alone names neither object");
        assert!(!svc.knows(b"NameService"));
    }
}
