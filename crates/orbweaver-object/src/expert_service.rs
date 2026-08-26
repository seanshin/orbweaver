//! The wire surface of the MoE control plane: `moe::ExpertRegistry`,
//! `moe::ExpertLoader` and `moe::Router::select` from
//! `corpus/golden/22-moe-control-plane.idl`, served on our own
//! [`Server`](orbweaver_giop::server::Server).
//!
//! PLAN-SERVICES §3 defers the standard `CosTrading::Lookup` facade until a
//! foreign trading client is named, and lands **the project contract**
//! instead. That contract is corpus/golden/22 and nothing else: the
//! interfaces below are served exactly as declared there, with no operation
//! added and none half-served.
//!
//! # The three objects, and the three operations that are not here
//!
//! `ExpertRegistry` and `ExpertLoader` are served whole. `Router` is served
//! **by half, on purpose**, and the halves are the split PLAN-MOE §4.6
//! reasoned out:
//!
//! - **`Router::select`** returns `ExpertSeq` — references, nothing else. It
//!   is pure control plane and it is the question `orbweaver_trading` already
//!   answers internally, so it is served here by delegating to that engine.
//!   See [`ExpertService::select`], especially for what it answers when the
//!   offer store cannot answer at all.
//! - **`Router::dispatch`** carries an `Activation`, and so does
//!   **`Expert::process`**. Both are control-plane-legal only under the
//!   reading that a `Tensor` holds a handle rather than a payload — a reading
//!   that lives in a comment in corpus/golden/22, binds nothing, and is
//!   enforced by nothing. Serving them would commit the project to it
//!   silently. `dispatch` answers `NO_IMPLEMENT` **and this paragraph is its
//!   reason**, which is what PLAN-SERVICES §8.1 asks of every refusal. (This
//!   sentence said `BAD_OPERATION` from 2026-08-18 to 2026-08-19 while the code
//!   and the sweep said `NO_IMPLEMENT` — the §8.1.1 failure with the polarity
//!   reversed; the wire is the home of the fact, this is its reason.)
//! - **`moe::Expert`'s own operations** are not served here at all, and that
//!   is not an omission either: this registry *stores* expert references and
//!   hands them back, and the experts themselves are served elsewhere
//!   (`tenant_service` serves `::moe::Expert` for corpus/golden/23). An
//!   `Expert` servant on the registry's object would answer for an expert
//!   that does not live here.
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
//! - **`specialization` and `latency_p50` — on the v1.0 path only.**
//!   [`Offer`] has both and `moe::Capability` has neither, so an offer that
//!   arrived through `register_expert`/`heartbeat` carries `None` for each: a
//!   `specialization == 'math'` constraint is *unanswerable* for it and an
//!   `ORDER BY latency_p50` cannot place it. The contract change PLAN-MOE
//!   §4.5 priced is now paid the §5.3 way — v1.1 adds
//!   [`MeasuredCapability`] and `register_measured`/`heartbeat_measured`
//!   *beside* the released type, never inside it — and an offer registered
//!   through those answers both. A v1.0 heartbeat on a measured offer leaves
//!   the two members alone: a message with no room for a fact cannot retract
//!   it.
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
//! # Sharing: one lock, because the mirror is an invariant across two halves
//!
//! This servant implements [`SharedDispatch`], so two calls may run at once.
//! The sharing decision is the strictest in the batch and the reason is
//! already written down two sections above: **the offer store and the
//! residency machine are two copies of one truth**, kept in step by
//! `mirror_residency` at a single choke point, and that section records what
//! happened the last time they were allowed to drift — the control plane
//! quietly stopped deciding, with nothing failing loudly.
//!
//! So: **one lock over the store, the loader, the reference table and the
//! reported free memory, taken once per operation.** Not one per map. A reader
//! that saw a mirrored loader and an unmirrored store would see exactly the
//! desynchronisation the choke point exists to prevent, and two locks would
//! also be two locks to order (see [`orbweaver_giop::guarded`], whose whole
//! discipline is that a thread never holds two).
//!
//! **Not an `RwLock` read path worth speaking of, either.** Of the wire
//! surface, only `status` and `_is_a` do not mutate: `prefetch`, `evict` and
//! `pin` all move the machine, `heartbeat` rewrites the offer, and
//! `apply_policy` decides, applies, mirrors and decays. This servant is a
//! writer, and saying so is more useful than a read path that would almost
//! never be taken. What concurrency buys it is that its callers no longer
//! queue behind *other servants* in the same process — and that `status`, the
//! one a control loop polls, no longer waits behind a heartbeat.
//!
//! Nothing here dials anything: the `Expert` reference is
//! [`Registered::reference`], held verbatim and never invoked, so no outbound
//! call can be made from inside the lock and the tripwire has nothing to fire
//! on. A future `Router::select` that *does* dial must do it after the lock
//! closes, with the reference copied out.
//!
//! [`Decision`]: orbweaver_trading::policy::Decision
//! [`Offer`]: orbweaver_trading::Offer
//! [`SharedDispatch`]: orbweaver_giop::server::SharedDispatch

use std::collections::BTreeMap;

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::guarded::Guarded;
use orbweaver_giop::server::{Completion, Dispatch, Request, SharedDispatch, SystemException};
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_trading::policy::{Decision, LoadingPolicy};
use orbweaver_trading::query::Query;
use orbweaver_trading::{FREQ_SCALE, Offer, OfferStore, Residency, StoreError};

use crate::residency::{Applied, BatchStats, ExpertLoader, GuardCondition, TransitionError};
use crate::{Lifespan, OBJECT_ID, get_reference, is_equivalent, put_reference};

/// Repository id of `moe::ExpertRegistry`.
pub const EXPERT_REGISTRY_ID: &str = "IDL:moe/ExpertRegistry:1.0";
/// Repository id of `moe::ExpertLoader`.
pub const EXPERT_LOADER_ID: &str = "IDL:moe/ExpertLoader:1.0";
/// Repository id of `moe::Router` — `select` served, `dispatch` refused; see
/// the module docs and PLAN-MOE §4.6.
pub const ROUTER_ID: &str = "IDL:moe/Router:1.0";
/// Repository id of `moe::Expert` — the type of the reference
/// `register_expert`, `deregister` and `heartbeat` take.
pub const EXPERT_ID: &str = "IDL:moe/Expert:1.0";

/// The object-key base the MoE control plane is served under, and the **one**
/// place it is spelled.
///
/// D028 §1: *"a key collides with itself."* `spike_experts` bound its
/// [`Server`](orbweaver_giop::server::Server) to `b"MoE/registry"` and handed
/// [`ExpertService::new`] the base `b"MoE"`, from which the registry face's
/// key is derived — **the same bytes arrived at twice, by two routes.**
/// Nothing was red, because a `Server`'s own key is read only by
/// `Server::ior` and that fixture publishes `ExpertService`'s references
/// instead; so the two identities were free to agree by accident, and equally
/// free to stop agreeing.
///
/// A caller that spells the base once cannot make them collide. The derived
/// keys are `<base>/registry`, `<base>/loader` and `<base>/router`, and
/// [`ExpertService::knows`](orbweaver_giop::server::SharedDispatch::knows)
/// answers for those three and not for the base — which is what
/// `the_servers_identity_is_not_one_of_the_faces_it_serves` in `spike_experts`
/// asserts, so a second spelling goes red rather than merely differing.
pub const MOE_BASE_KEY: &[u8] = b"MoE";

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
/// `NO_IMPLEMENT`: `Router::select` was asked a question this deployment
/// cannot answer, because the constraint names an offer property
/// `moe::Capability` declares no member for. See
/// [`ExpertService::select`] — it is the one refusal on this surface that is
/// about the *contract* rather than about the request or the machine.
pub const NO_IMPLEMENT: &str = "IDL:omg.org/CORBA/NO_IMPLEMENT:1.0";

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
    /// `specialization` and `latency_p50` have no member in this (v1.0)
    /// shape — see the module docs — so they are `None`, which is the
    /// sentence the wire actually supports: *nobody told us*. The v1.1 shape,
    /// [`MeasuredCapability::to_offer`], fills them in.
    ///
    /// They used to be an empty string and `0.0`, and the zero was the worse
    /// of the two. It did not merely fail to match a query; it satisfied
    /// every upper bound, so `latency_p50 < 20` preferred exactly the experts
    /// whose latency nobody had measured. `None` cannot be compared, and the
    /// query reports those offers as unanswerable rather than as answers.
    pub fn to_offer(&self, residency: Residency) -> Offer {
        Offer {
            id: self.id.clone(),
            specialization: None,
            cost: f64::from(self.cost),
            latency_p50: None,
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

/// `moe::MeasuredCapability` — the v1.1 registration shape, corpus/golden/22:
///
/// ```idl
/// struct MeasuredCapability {
///   Capability base;
///   string     specialization;
///   float      latency_p50_ms;
/// };
/// ```
///
/// # Why a new struct and not two more members
///
/// `Capability` is released, and adding the two members to it in place is
/// **BREAKING** by our own `idl-diff` — a CDR member has no tag and no
/// length, so a v1.0 peer would read `specialization`'s bytes as `cost`.
/// PLAN-MOE §4.5 measured that and declined to pay it; D010 A2 gave the
/// version bump its reason. §5.3's answer is a new type that *composes* the
/// released one, reached through new operations (`register_measured`,
/// `heartbeat_measured`) that a v1.0 client never calls. The frozen release
/// this is diffed against is `corpus/evolution/moe/v1.0/moe.idl`; the
/// in-place edit that was refused is `corpus/evolution/moe/v1.1-in-place/`.
///
/// # What the two members mean to the store
///
/// Both go straight into the offer as `Some(..)`, which is the whole point:
/// an offer that arrived this way is *answerable* on `specialization ==` and
/// `latency_p50 <`, and *rankable* under `ORDER BY latency_p50`. Nothing
/// here validates the measurement — a peer that reports `0.0` reports a
/// measured zero, exactly as it would report any other number, and the
/// difference between that and the old placeholder zero is that this one was
/// *said* rather than assumed. The struct has no way to say "unmeasured";
/// that is what registering through v1.0 means.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredCapability {
    /// `Capability base` — the released nine members, unchanged.
    pub base: Capability,
    /// `string specialization` — what the expert is for.
    pub specialization: String,
    /// `float latency_p50_ms` — the median latency somebody measured.
    pub latency_p50_ms: f32,
}

impl MeasuredCapability {
    /// Marshals the struct in declaration order: the nested struct's members
    /// inline (§9.3.2.5 — a struct member is its members, no header), then
    /// the string, then the float.
    pub fn write_to(&self, out: &mut Encoder) {
        self.base.write_to(out);
        out.put_str(&self.specialization);
        out.put_f32(self.latency_p50_ms);
    }

    /// Demarshals what [`MeasuredCapability::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        let base = Capability::read_from(d)?;
        let specialization = d.get_string()?;
        let latency_p50_ms = d.get_f32()?;
        Ok(MeasuredCapability { base, specialization, latency_p50_ms })
    }

    /// The offer this registers: [`Capability::to_offer`] with the two
    /// members the v1.0 shape had no room for filled in.
    pub fn to_offer(&self, residency: Residency) -> Offer {
        let mut offer = self.base.to_offer(residency);
        offer.specialization = Some(self.specialization.clone());
        offer.latency_p50 = Some(f64::from(self.latency_p50_ms));
        offer
    }

    /// The measured shape an offer would be reported as, or `None` when the
    /// offer does not carry both members — an offer that registered through
    /// v1.0 has no measured shape, and inventing one would put the
    /// placeholder back on the wire.
    pub fn from_offer(offer: &Offer, state: Residency, contract_version: &str) -> Option<Self> {
        Some(MeasuredCapability {
            base: Capability::from_offer(offer, state, contract_version),
            specialization: offer.specialization.clone()?,
            latency_p50_ms: offer.latency_p50? as f32,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// moe::GateSignal and moe::Constraints — Router::select's two arguments
// ─────────────────────────────────────────────────────────────────────────────

/// `moe::GateSignal`, corpus/golden/22 line 33:
/// `struct GateSignal { Tensor affinity; unsigned short top_k; };`
///
/// # `affinity` is decoded and deliberately not read
///
/// `Tensor` is `sequence<octet>`, and whether one carries a *handle* or a
/// *payload* is the open decision PLAN-MOE §4.6 records and D006 settled as
/// option E — the decision that keeps `Router::dispatch` unimplemented.
///
/// This sentence named `Expert::process` alongside it until 2026-08-26, and
/// that half was false: [`crate::tenant_service`] serves `process` in two
/// arms, and an omniORB peer calls it under the harness. It is the **second**
/// false restatement of this operation's status in this one file — the other
/// was `router_ior`'s `BAD_OPERATION` — which is a fact about how a paragraph
/// repeated in four places is maintained rather than two typos.
/// [`crate::plane`] is now the one home, and a test computes it from the
/// contracts.
///
/// A `select`
/// that interpreted these bytes as an affinity vector and did arithmetic on
/// them would be making that decision unilaterally, on the operation that was
/// supposed to be the *pure control-plane* half.
///
/// So [`ExpertService::select`] reads `top_k` and nothing else, and
/// `the_affinity_tensor_is_not_read` pins that as a property rather than
/// leaving it as an omission: the same store and the same `top_k` answer
/// identically whatever bytes arrive here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GateSignal {
    /// `Tensor affinity` — the gate network's output. Carried, never read.
    pub affinity: Vec<u8>,
    /// `unsigned short top_k` — at most this many experts come back. Zero
    /// means zero: it is not a sentinel for "no limit", because inventing one
    /// is how `latency_p50 == 0.0` came to mean "infinitely fast".
    pub top_k: u16,
}

impl GateSignal {
    /// Marshals the struct in declaration order.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_octet_seq(&self.affinity);
        out.put_u16(self.top_k);
    }

    /// Demarshals what [`GateSignal::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        let affinity = d.get_octet_seq()?.to_vec();
        Ok(GateSignal { affinity, top_k: d.get_u16()? })
    }
}

/// `moe::Constraints`, corpus/golden/22 line 34:
/// `struct Constraints { CapabilityId required; float max_latency_ms; float
/// max_cost; };`
///
/// # What each member is compared against, and why
///
/// - **`required`** is matched against the offer's `specialization`, not
///   against its `id`. The declared type points at `id` — `CapabilityId` is
///   the id's typedef — but the operation's own shape refutes that reading:
///   `select` returns a *sequence* and `GateSignal` carries a `top_k`, and
///   neither means anything if the constraint already names one expert. That
///   is `resolve`, not selection. §4.3's own worked example is
///   `specialization == 'math'`, which is the question a gate asks.
///   **An empty `required` constrains nothing** — an empty string is not a
///   capability, so there is nothing to match on. That is the one reading of
///   a member here that is not literal, and it is the difference between an
///   operation with an answerable form and one without.
/// - **`max_latency_ms`** is an upper bound on `latency_p99`. The offer
///   carries two latencies and this member names no percentile; p99 is the
///   one `moe::Capability` transports, so it is the one an offer that arrived
///   over this contract actually has. Reading it as p50 would make every wire
///   registration unanswerable on every call — honest, and useless.
/// - **`max_cost`** is an upper bound on `cost`.
///
/// Both bounds are inclusive and both are taken literally, zero included: a
/// `max_cost` of `0.0` is the bound "cost at most nothing", which nothing
/// satisfies, and answering that with an empty sequence is true. Treating it
/// as "unset" would be the placeholder-zero mistake PLAN-MOE §4.5 measured.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Constraints {
    /// `CapabilityId required` — the capability the caller needs, matched
    /// against the offer's specialization. Empty means unconstrained.
    pub required: String,
    /// `float max_latency_ms` — inclusive upper bound on `latency_p99`.
    pub max_latency_ms: f32,
    /// `float max_cost` — inclusive upper bound on `cost`.
    pub max_cost: f32,
}

impl Constraints {
    /// Marshals the struct in declaration order.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_str(&self.required);
        out.put_f32(self.max_latency_ms);
        out.put_f32(self.max_cost);
    }

    /// Demarshals what [`Constraints::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        Ok(Constraints {
            required: d.get_string()?,
            max_latency_ms: d.get_f32()?,
            max_cost: d.get_f32()?,
        })
    }

    /// The §4.3 query text these constraints mean, in the grammar
    /// [`orbweaver_trading::query::Query`] parses.
    ///
    /// Built as *text* on purpose. The trading engine already answers this
    /// question — three-valued matching, unknown-aware ordering, positioned
    /// diagnostics — and its published surface is a query string, so this is
    /// delegation rather than a second matcher that could disagree with the
    /// first. The ordering is `route_freq DESC` because the grammar orders by
    /// a *field* and `route_freq` is the numerator of §6's residency score;
    /// the score itself is not a field, so it cannot be an `ORDER BY`. Ties
    /// break on ascending id inside the engine, so the same store answers the
    /// same way every time.
    ///
    /// Refuses rather than mangles: a `required` carrying a `'` would close
    /// the query's string literal early, and a non-finite bound has no
    /// literal in the grammar at all. Both are `BAD_PARAM` — the argument, not
    /// the store.
    fn to_query_text(&self) -> Result<String, SystemException> {
        let mut clauses: Vec<String> = Vec::new();
        if !self.required.is_empty() {
            if self.required.contains('\'') {
                return Err(system(BAD_PARAM, Completion::No));
            }
            clauses.push(format!("specialization == '{}'", self.required));
        }
        for (field, bound) in [("latency_p99", self.max_latency_ms), ("cost", self.max_cost)] {
            if !bound.is_finite() {
                return Err(system(BAD_PARAM, Completion::No));
            }
            // `{}` on an f64 is the shortest round-tripping decimal and never
            // uses exponent notation, which the query lexer has no rule for.
            clauses.push(format!("{field} <= {}", f64::from(bound)));
        }
        Ok(format!("{} ORDER BY route_freq DESC", clauses.join(" AND ")))
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

/// The four things that must move together, and therefore share one lock.
///
/// See the module docs on sharing: the store's residency and the loader's are
/// two copies of one truth, `refs` is keyed by the same expert ids, and
/// `free_memory` is what the eviction guard reads. A reader holding any three
/// of them without the fourth can see a control plane mid-mirror.
#[derive(Debug)]
struct ExpertState {
    store: OfferStore,
    loader: ExpertLoader,
    refs: BTreeMap<String, Registered>,
    free_memory: u64,
}

/// `moe::ExpertRegistry` and `moe::ExpertLoader`, served together.
///
/// Two interfaces and one servant, because they are two views of one state:
/// registering an expert has to create it in *both* the offer store and the
/// residency machine, and nothing could keep two servants' halves in step
/// without a shared owner. They stay two *objects* — distinct object keys,
/// distinct repository ids, [`SharedDispatch::knows`] answering for both —
/// because the contract declares two interfaces and a client narrows to one
/// of them.
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
    router_key: Vec<u8>,
    /// Constant after construction, so it stays outside the lock: the §6
    /// policy is configuration, not state, and taking a lock to read
    /// configuration is the serialization this batch removed, put back by
    /// habit.
    policy: LoadingPolicy,
    cold_below: u64,
    state: Guarded<ExpertState>,
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
            router_key: key("/router"),
            policy,
            cold_below,
            state: Guarded::new(
                "the expert control plane",
                ExpertState {
                    store: OfferStore::new(),
                    loader: ExpertLoader::new(),
                    refs: BTreeMap::new(),
                    // No snapshot has been reported yet. Zero is the safe
                    // start: it is below every sane low watermark, so the
                    // guard's *other* three conditions still have to hold
                    // before anything is evicted.
                    free_memory: 0,
                },
            ),
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

    /// The `Router` object key.
    pub fn router_key(&self) -> &[u8] {
        &self.router_key
    }

    /// A publishable `ExpertRegistry` reference.
    pub fn registry_ior(&self) -> Ior {
        self.ior_for(EXPERT_REGISTRY_ID, &self.registry_key)
    }

    /// A publishable `ExpertLoader` reference.
    pub fn loader_ior(&self) -> Ior {
        self.ior_for(EXPERT_LOADER_ID, &self.loader_key)
    }

    /// A publishable `Router` reference. `select` is served on it; `dispatch`
    /// answers `NO_IMPLEMENT` — see [`crate::plane`] for which operations
    /// carry a `Tensor` and what this project does about each.
    ///
    /// This sentence said `BAD_OPERATION` until 2026-08-26, four lines from
    /// the code that refutes it and a week after the module docs recorded the
    /// *same* polarity failure being repaired in themselves (2026-08-18 to
    /// 2026-08-19). Repairing one home of a fact is what leaves
    /// the others standing; the status now lives in one place and a test reads
    /// it from there.
    pub fn router_ior(&self) -> Ior {
        self.ior_for(ROUTER_ID, &self.router_key)
    }

    fn ior_for(&self, type_id: &str, key: &[u8]) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key.to_vec(),
                // D009 L2; see the note in `orbweaver-object/src/lib.rs`. The
                // one at 192.0.2.7 below stays empty: it is a test's stand-in
                // reference and never reaches a peer.
                components: vec![orbweaver_giop::codeset::server_component()],
            }],
        }
    }

    /// Reads the offer store. Read-only: every mutation has to go through an
    /// operation that keeps the loader in step.
    ///
    /// A closure and not a `&OfferStore`, because handing out a reference
    /// would mean handing out the lock guard that keeps it alive — and a guard
    /// a caller holds is a lock held for as long as the caller likes, across
    /// whatever it does next. Copy out what you need; the store's own getters
    /// return owned values or `Copy` ones.
    pub fn with_store<R>(&self, f: impl FnOnce(&OfferStore) -> R) -> R {
        self.state.read(|s| f(&s.store))
    }

    /// Reads the residency machine, for the same reason and under the same
    /// rule.
    pub fn with_loader<R>(&self, f: impl FnOnce(&ExpertLoader) -> R) -> R {
        self.state.read(|s| f(&s.loader))
    }

    /// The reference `register_expert` was given for `id` — what a router
    /// hands back when it selects this expert.
    pub fn reference_for(&self, id: &str) -> Option<Ior> {
        self.state.read(|s| s.refs.get(id).map(|r| r.reference.clone()))
    }

    /// The capability this service would report for `id`, with the loader's
    /// residency rather than the one the expert last claimed.
    pub fn capability_of(&self, id: &str) -> Option<Capability> {
        self.state.read(|s| {
            let offer = s.store.get(id)?;
            let registered = s.refs.get(id)?;
            let state = s.loader.status(id).unwrap_or(offer.residency);
            Some(Capability::from_offer(offer, state, &registered.contract_version))
        })
    }

    /// Records the free accelerator memory the control loop observed.
    ///
    /// The loader cannot know this and neither can the store — F3's
    /// [`BatchStats`] doc is explicit that it arrives per window from outside.
    /// A wire `evict` is evaluated against the *last reported* snapshot, so a
    /// control loop that never reports one gets a service that never evicts,
    /// which is the right failure direction.
    pub fn observe_free_memory(&self, bytes: u64) {
        self.state.write(|s| s.free_memory = bytes);
    }

    /// Records what `id` specializes in — the offer property the v1.0
    /// `moe::Capability` declares no member for.
    ///
    /// Out of band for the same reason as [`ExpertService::observe_free_memory`]
    /// and [`ExpertService::record_hit`]: it is a control-plane fact the v1.0
    /// shape carries no room for, so it arrives in-process for an expert that
    /// registered that way. Since v1.1 the wire has a place for it —
    /// `register_measured`/`heartbeat_measured` carry a
    /// [`MeasuredCapability`] — so this is the deployment's fallback for
    /// experts still announcing through `register_expert`, and it is what
    /// makes [`ExpertService::select`] answerable for those.
    ///
    /// Returns whether `id` was registered. Nothing else about the offer
    /// changes — the residency mirror included, since the offer is read,
    /// amended and written back whole.
    pub fn declare_specialization(&self, id: &str, specialization: &str) -> bool {
        self.state.write(|s| {
            let Some(mut offer) = s.store.get(id).cloned() else {
                return false;
            };
            offer.specialization = Some(specialization.to_owned());
            s.store.heartbeat(offer).is_ok()
        })
    }

    /// `moe::Router::select` — the experts that satisfy `qos`, at most
    /// `gate.top_k` of them, best first.
    ///
    /// # It delegates; it does not decide
    ///
    /// The question "which offers satisfy these constraints, in what order" is
    /// [`orbweaver_trading`]'s, and it already answers it for the loading
    /// policy. [`Constraints::to_query_text`] turns the struct into the §4.3
    /// query the engine parses, the engine matches and orders, and this
    /// function only maps the surviving offer ids back to the `Expert`
    /// references [`ExpertService::register_expert`] stored. A second matcher
    /// here would be a second thing to keep in step with §6, and the last
    /// two copies of one truth in this file needed a choke point to stop them
    /// drifting.
    ///
    /// # What it answers when the store cannot answer
    ///
    /// **`NO_IMPLEMENT`, for the whole call — never a shorter list.**
    ///
    /// An offer that registered over this contract carries no
    /// `specialization`, so a `required` constraint is *unanswerable* for it
    /// rather than false — that is what [`orbweaver_trading::query::Truth`]'s
    /// third value is for, and
    /// [`Selection::unanswerable`](orbweaver_trading::query::Selection) is
    /// where the engine puts those offers instead of discarding them. On the
    /// wire there is nowhere to put them: `ExpertSeq` is references and
    /// nothing else, so a sequence that quietly omitted them would say *these
    /// are all the experts that qualify* — which is exactly the sentence the
    /// three-valued matching exists to stop being said. A short answer that
    /// looks complete is worse than no answer.
    ///
    /// So the rule is: **a sequence of references is a complete answer or it
    /// is a refusal.** Any unanswerable offer refuses the call, even when
    /// other offers definitely matched and even when `top_k` was already
    /// filled — the ordering is over all matches, so an offer nobody could
    /// judge might have outranked the ones that came back.
    ///
    /// `NO_IMPLEMENT` and not one of the four exceptions this servant already
    /// uses, because it is a different sentence from all of them:
    ///
    /// | | says | wrong here because |
    /// |---|---|---|
    /// | `BAD_OPERATION` | no such operation | `select` exists and ran |
    /// | `BAD_PARAM` | your argument was bad | it was well formed; a malformed one *is* `BAD_PARAM`, above |
    /// | `NO_PERMISSION` | you may not | nobody is being refused access |
    /// | `TRANSIENT` | try the next window | a window cannot add a member to a struct; retrying for ever is the one thing a caller must not do |
    /// | **`NO_IMPLEMENT`** | **this ORB cannot carry out what you asked** | the property the constraint names has no implementation behind it in this deployment, and PLAN-MOE §4.5 says what closing that costs |
    ///
    /// A caller that wants an answer today drops `required` (an empty one
    /// constrains nothing and every remaining field is answerable), or the
    /// deployment declares specializations out of band with
    /// [`ExpertService::declare_specialization`].
    ///
    /// # What it does not do
    ///
    /// It does not filter on residency: `Constraints` declares no member for
    /// it and inventing one would be policy this contract did not ask for. An
    /// OFFLOADED expert can therefore come back, and dialling it answers
    /// `OBJECT_NOT_EXIST` by F3's design — the caller's cue to `prefetch`. It
    /// also does not dial anything: the references are copied out of the
    /// table under the lock and handed back, so the "no outbound call inside
    /// a lock" rule this module's docs anticipated for `select` is kept by
    /// there being no call at all.
    pub fn select(
        &self,
        gate: &GateSignal,
        qos: &Constraints,
    ) -> Result<Vec<Ior>, SystemException> {
        let text = qos.to_query_text()?;
        // A query text we built ourselves that will not parse is our defect,
        // not the caller's — and `to_query_text` has already refused every
        // argument that could make one.
        let query = Query::parse(&text).map_err(|_| SystemException::internal())?;
        self.state.read(|s| {
            let selection = query.select_reporting(&s.store);
            // Unanswerable *or* unranked — the ordering here is `route_freq`,
            // which every offer carries, so today only the first can happen;
            // the predicate is the rule, not the case.
            if !selection.is_complete() {
                return Err(system(NO_IMPLEMENT, Completion::No));
            }
            let mut chosen = Vec::with_capacity(selection.matched.len().min(gate.top_k.into()));
            for offer in selection.matched.iter().take(gate.top_k.into()) {
                // An offer with no reference would mean the store and the
                // reference table disagree about who is registered. They are
                // written together under one lock, so this is unreachable —
                // and if it ever is reached, saying so is better than
                // returning a list one expert short.
                let Some(registered) = s.refs.get(&offer.id) else {
                    return Err(SystemException::internal());
                };
                chosen.push(registered.reference.clone());
            }
            Ok(chosen)
        })
    }

    /// Records one routing hit against `id`'s decayed counter — §6's feedback
    /// loop, and F4's telemetry when it lands.
    ///
    /// Deliberately not a wire operation: it fires per routed call, and a
    /// per-call operation on this contract is the per-token surface §5
    /// forbids. It arrives in-process from the router instead.
    pub fn record_hit(&self, id: &str) -> bool {
        self.state.write(|s| s.store.add_hit(id))
    }

    /// PREFETCHING → RESIDENT: the weight copy finished.
    ///
    /// No wire operation either, and for a different reason: nothing calls
    /// this from outside because nothing outside performs the copy. Whoever
    /// does — in this repository, the spike standing in for it — reports here.
    pub fn complete_load(&self, id: &str) -> Result<Residency, TransitionError> {
        self.state.write(|s| s.transition(|l| l.complete_load(id)))
    }

    /// A call began on `id`: RESIDENT → ACTIVE, or one more inflight.
    pub fn begin_call(&self, id: &str) -> Result<Residency, TransitionError> {
        self.state.write(|s| s.transition(|l| l.begin_call(id)))
    }

    /// A call on `id` finished.
    pub fn end_call(&self, id: &str) -> Result<Residency, TransitionError> {
        self.state.write(|s| s.transition(|l| l.end_call(id)))
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
    /// One window is one lock section: the four steps below are exactly the
    /// invariant the module docs describe, and a reader between any two of
    /// them would see the drift the choke point exists to prevent.
    pub fn apply_policy(&self, free_memory: u64) -> Vec<Applied> {
        self.state.write(|s| {
            s.free_memory = free_memory;
            let decisions: Vec<Decision> =
                self.policy.decide(&s.store, free_memory, &s.loader.inflight_ids());
            let stats = s.window(&self.policy, self.cold_below);
            let applied = s.loader.apply(&decisions, &stats);
            s.mirror_residency();
            s.store.decay_all();
            applied
        })
    }
}

impl ExpertState {
    /// The window the eviction guard reads, from the last reported snapshot.
    fn window(&self, policy: &LoadingPolicy, cold_below: u64) -> BatchStats {
        BatchStats::from_store(&self.store, policy, self.free_memory, cold_below)
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

    /// `register_expert` (v1.0): the offer carries no specialization and no
    /// median latency, and the store says so with `None`.
    fn register_expert(&mut self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        self.register_offer(reference, cap.to_offer(Residency::Offloaded), &cap.contract_version)
    }

    /// `register_measured` (v1.1): the same registration with both members
    /// filled in. One code path underneath, so the two operations cannot
    /// drift on anything but the offer they build.
    fn register_measured(
        &mut self,
        reference: Ior,
        measured: &MeasuredCapability,
    ) -> Result<(), SystemException> {
        self.register_offer(
            reference,
            measured.to_offer(Residency::Offloaded),
            &measured.base.contract_version,
        )
    }

    fn register_offer(
        &mut self,
        reference: Ior,
        offer: Offer,
        contract_version: &str,
    ) -> Result<(), SystemException> {
        // Checked against both halves before either is touched: a partial
        // registration would leave an offer the loader has never heard of,
        // and `deregister` keys off the reference table that comes last.
        if self.store.get(&offer.id).is_some() || self.loader.status(&offer.id).is_some() {
            return Err(system(BAD_PARAM, Completion::No));
        }
        // PERSISTENT, and not selectable: §4.2's TRANSIENT drops the
        // adaptation state on eviction, this contract has no member to ask
        // for that, and defaulting to the lossy one would make eviction quietly
        // destructive. Adding a member is a contract change (F1), not a
        // default chosen here.
        self.loader.register(&offer.id, Lifespan::Persistent).map_err(|e| refuse(&e))?;
        let id = offer.id.clone();
        self.store.register(offer).map_err(|e| refuse_store(&e))?;
        self.refs
            .insert(id, Registered { reference, contract_version: contract_version.to_owned() });
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

    /// `heartbeat` (v1.0). The two members this shape cannot mention are
    /// **kept from the offer on file**, not reset to `None`: a v1.0 heartbeat
    /// after a v1.1 registration (or after `declare_specialization`) is a
    /// message with no room for the fact, which is not the same message as
    /// "the fact is withdrawn". The first version of this rebuilt the offer
    /// from the capability alone and would have erased a measurement the
    /// moment the expert heartbeated the old way — silently, and only
    /// visible as a router that stopped ranking it.
    fn heartbeat(&mut self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        let (specialization, latency_p50) = match self.store.get(&cap.id) {
            Some(on_file) => (on_file.specialization.clone(), on_file.latency_p50),
            None => (None, None),
        };
        let mut offer = cap.to_offer(Residency::Offloaded);
        offer.specialization = specialization;
        offer.latency_p50 = latency_p50;
        self.heartbeat_offer(reference, offer, &cap.contract_version)
    }

    /// `heartbeat_measured` (v1.1): a fresh measurement replaces the one on
    /// file, and this is the operation a measurement *arrives* by for an
    /// expert already registered.
    fn heartbeat_measured(
        &mut self,
        reference: Ior,
        measured: &MeasuredCapability,
    ) -> Result<(), SystemException> {
        self.heartbeat_offer(
            reference,
            measured.to_offer(Residency::Offloaded),
            &measured.base.contract_version,
        )
    }

    fn heartbeat_offer(
        &mut self,
        reference: Ior,
        mut offer: Offer,
        contract_version: &str,
    ) -> Result<(), SystemException> {
        let Some(registered) = self.refs.get_mut(&offer.id) else {
            return Err(system(BAD_PARAM, Completion::No));
        };
        // A heartbeat re-announces the expert, address included: an expert
        // that moved says so here, and this is what keeps `deregister`'s
        // reference lookup able to find it afterwards.
        registered.reference = reference;
        registered.contract_version = contract_version.to_owned();
        // route_freq is dropped by OfferStore::heartbeat; state comes from the
        // loader. Both are in the table in the module docs.
        offer.residency = self.loader.status(&offer.id).unwrap_or(Residency::Offloaded);
        self.store.heartbeat(offer).map_err(|e| refuse_store(&e))
    }

    // ── the ExpertLoader operations ─────────────────────────────────────────

    fn prefetch(&mut self, id: &str) -> Result<(), SystemException> {
        self.transition(|l| l.request_prefetch(id)).map(|_| ()).map_err(|e| refuse(&e))
    }

    fn evict(
        &mut self,
        id: &str,
        policy: &LoadingPolicy,
        cold_below: u64,
    ) -> Result<(), SystemException> {
        let stats = self.window(policy, cold_below);
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
}

/// Which of the three objects a request addressed. One servant, three object
/// keys, three repository ids — a client narrows to exactly one interface, and
/// an operation from a neighbouring one is `BAD_OPERATION` on this object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    Registry,
    Loader,
    Router,
}

impl Face {
    fn repository_id(self) -> &'static str {
        match self {
            Face::Registry => EXPERT_REGISTRY_ID,
            Face::Loader => EXPERT_LOADER_ID,
            Face::Router => ROUTER_ID,
        }
    }
}

/// The wire operations, each one lock section deep.
///
/// This layer exists so that **taking the lock happens in exactly one place
/// per operation**. `handle` decodes and replies; these open the section and
/// delegate to [`ExpertState`], which does the work with no idea that a lock
/// exists. Nesting two of them would be a torn request and a re-entrant lock,
/// and [`orbweaver_giop::guarded`] refuses the second — so the rule to keep is
/// that nothing in this block calls anything else in this block.
impl ExpertService {
    fn register_expert(&self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        self.state.write(|s| s.register_expert(reference, cap))
    }

    fn deregister(&self, reference: &Ior) -> Result<(), SystemException> {
        self.state.write(|s| s.deregister(reference))
    }

    fn heartbeat(&self, reference: Ior, cap: &Capability) -> Result<(), SystemException> {
        self.state.write(|s| s.heartbeat(reference, cap))
    }

    fn register_measured(
        &self,
        reference: Ior,
        measured: &MeasuredCapability,
    ) -> Result<(), SystemException> {
        self.state.write(|s| s.register_measured(reference, measured))
    }

    fn heartbeat_measured(
        &self,
        reference: Ior,
        measured: &MeasuredCapability,
    ) -> Result<(), SystemException> {
        self.state.write(|s| s.heartbeat_measured(reference, measured))
    }

    fn prefetch(&self, id: &str) -> Result<(), SystemException> {
        self.state.write(|s| s.prefetch(id))
    }

    fn evict(&self, id: &str) -> Result<(), SystemException> {
        self.state.write(|s| s.evict(id, &self.policy, self.cold_below))
    }

    fn pin(&self, id: &str) -> Result<(), SystemException> {
        self.state.write(|s| s.pin(id))
    }

    /// The one read on this surface, and the one a control loop polls: it no
    /// longer waits behind a heartbeat.
    fn status(&self, id: &str) -> Result<Residency, SystemException> {
        self.state.read(|s| s.status(id))
    }

    /// Serves one operation, writing the reply body into `out`.
    ///
    /// Arguments are decoded *before* the lock is taken and the reply is
    /// written *after* it closes, so the section covers the state change and
    /// nothing else.
    fn handle(&self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut args = req.body().map_err(|_| SystemException::marshal())?;
        let face = if req.object_key == self.registry_key {
            Face::Registry
        } else if req.object_key == self.loader_key {
            Face::Loader
        } else if req.object_key == self.router_key {
            Face::Router
        } else {
            // `knows` gates this, so it is unreachable through `Server` — but
            // a direct `dispatch` call has no such gate.
            return Err(SystemException::object_not_exist());
        };

        // Every ORB probes with these before it trusts a narrow, and the
        // answer differs by which of the three objects was addressed.
        match req.operation.as_str() {
            "_is_a" => {
                let want = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(want == face.repository_id() || want == OBJECT_ID);
                return Ok(());
            }
            "_non_existent" | "_not_existent" => {
                out.put_bool(false);
                return Ok(());
            }
            _ => {}
        }

        if face == Face::Router {
            // `Activation dispatch(in Activation x, in CallContext ctx)` is
            // NOT served, and the reason is PLAN-MOE §4.6's: it carries an
            // `Activation`, whose `Tensor` is control-plane-legal only under
            // the reading that it holds a handle rather than a payload. That
            // reading lives in a corpus comment, binds nothing and is enforced
            // by nothing, so serving `dispatch` would commit this project to
            // it by accident. `select` returns references only and needs no
            // such commitment, which is why exactly one of the two is here.
            //
            // It answers `NO_IMPLEMENT`, not `BAD_OPERATION`. D006 recorded the
            // exclusion in a document and the wire went on saying "no such
            // operation" — the answer an oversight gives — so the decision was
            // invisible to every client and indistinguishable from a servant
            // that had simply forgotten. A name `moe::Router` does not declare
            // at all still answers `BAD_OPERATION`.
            if req.operation != "select" {
                return Err(if req.operation == "dispatch" {
                    SystemException::no_implement()
                } else {
                    SystemException::bad_operation()
                });
            }
            let gate = GateSignal::read_from(&mut args).map_err(|_| SystemException::marshal())?;
            let qos = Constraints::read_from(&mut args).map_err(|_| SystemException::marshal())?;
            let experts = self.select(&gate, &qos)?;
            // `sequence<Expert>`: a length then that many object references.
            out.put_u32(experts.len() as u32);
            for expert in &experts {
                put_reference(out, Some(expert)).map_err(|_| SystemException::marshal())?;
            }
            return Ok(());
        }

        if face == Face::Registry {
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
                // v1.1 — the same two operations over the measured shape.
                "register_measured" => {
                    let reference = get_reference(&mut args)
                        .map_err(|_| SystemException::marshal())?
                        .ok_or_else(|| system(BAD_PARAM, Completion::No))?;
                    let measured = MeasuredCapability::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    self.register_measured(reference, &measured)
                }
                "heartbeat_measured" => {
                    let reference = get_reference(&mut args)
                        .map_err(|_| SystemException::marshal())?
                        .ok_or_else(|| system(BAD_PARAM, Completion::No))?;
                    let measured = MeasuredCapability::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    self.heartbeat_measured(reference, &measured)
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

impl SharedDispatch for ExpertService {
    /// Three object keys, one servant — see the type docs. All three are
    /// constants set at construction, so this answers without taking the lock:
    /// a `LocateRequest` cannot be delayed by a heartbeat.
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.registry_key
            || object_key == self.loader_key
            || object_key == self.router_key
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.handle(request, out)
    }
}

/// The `&mut self` shape too, forwarding, so a caller already written against
/// [`Server::serve`](orbweaver_giop::server::Server::serve) keeps working.
impl Dispatch for ExpertService {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::Endian;

    use orbweaver_giop::orb::Orb;
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

    /// The mirror is an invariant across two halves, so it is what
    /// concurrency has to be shown not to have broken.
    ///
    /// Registrations, heartbeats, transitions and status polls all run at
    /// once. Afterwards every expert must be in the store *and* the loader,
    /// with the two agreeing about residency — which is the exact
    /// desynchronisation the module docs record as having happened once
    /// already, silently, when the mirror was not at a single choke point.
    /// One lock over both halves is what makes it impossible rather than
    /// merely fixed, and this is the test that says so under load.
    #[test]
    fn concurrent_registrations_keep_the_store_and_the_loader_in_step() {
        const N: usize = 4;
        const EACH: usize = 6;
        let svc = service();

        std::thread::scope(|scope| {
            for i in 0..N {
                let svc = &svc;
                scope.spawn(move || {
                    for step in 0..EACH {
                        let id = format!("expert-{i}-{step}");
                        svc.register_expert(expert_ref(&id), &cap(&id, 10)).unwrap();
                        // Move it, so the loader and the store must both learn.
                        svc.prefetch(&id).unwrap();
                        svc.complete_load(&id).unwrap();
                        // A heartbeat, which rewrites the offer and must not
                        // rewrite the residency the loader owns.
                        svc.heartbeat(expert_ref(&id), &cap(&id, 20)).unwrap();
                        // And a read from another expert's point of view.
                        assert!(svc.status(&id).is_ok());
                    }
                });
            }
        });

        let total = N * EACH;
        assert_eq!(svc.with_store(|s| s.len()), total, "an offer was lost");
        for (id, state) in svc.with_loader(|l| l.states()) {
            assert_eq!(
                svc.with_store(|s| s.get(&id).map(|o| o.residency)),
                Some(state),
                "{id}: the store and the loader disagree — the mirror tore"
            );
            assert_eq!(state, Residency::Resident, "{id}: every expert finished loading");
        }
        assert_eq!(
            svc.with_store(|s| s.get("expert-0-0").unwrap().mem_footprint),
            20,
            "the heartbeat landed"
        );
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
        let svc = service();
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
        let offer = svc.with_store(|s| s.get("expert-math").cloned()).unwrap();
        // `None`, not a placeholder. The placeholder version of this test
        // asserted `""` and `0.0` and passed while that `0.0` satisfied every
        // `latency_p50 <` bound a query could ask on the wire path.
        assert_eq!(offer.specialization, None, "moe::Capability declares no specialization");
        assert_eq!(offer.latency_p50, None, "…and no p50, which is not the same as fast");
    }

    // ── the v1.1 path: MeasuredCapability ───────────────────────────────────

    fn measured(id: &str, specialization: &str, p50: f32) -> MeasuredCapability {
        MeasuredCapability {
            base: cap(id, 64),
            specialization: specialization.to_owned(),
            latency_p50_ms: p50,
        }
    }

    /// `struct MeasuredCapability { Capability base; string specialization;
    /// float latency_p50_ms; }` — the nested struct is its nine members
    /// inline, then the two new ones, both byte orders. Pinned member by
    /// member with the primitive getters, independently of `read_from`.
    #[test]
    fn measured_capability_members_are_in_the_idls_declaration_order() {
        let m = measured("expert-math", "math", 12.5);
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            m.write_to(&mut e);
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(Capability::read_from(&mut d).unwrap(), m.base, "1 base {endian:?}");
            assert_eq!(d.get_string().unwrap(), "math", "2 specialization {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 12.5, "3 latency_p50_ms {endian:?}");
            assert!(d.get_u8().is_err(), "nothing follows the third member {endian:?}");

            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(MeasuredCapability::read_from(&mut d).unwrap(), m, "round trip {endian:?}");
        }
    }

    /// The registration the contract change exists for, over the wire in
    /// both byte orders: `register_measured` and `heartbeat_measured` reach
    /// the store with both members `Some`, and the matcher can then answer
    /// — and rank — a query on them. Beside it, an expert registered through
    /// v1.0 stays unanswerable on the same query, so the two paths are told
    /// apart by the engine and not by this test's knowledge of which was
    /// which.
    #[test]
    fn a_v1_1_registration_is_answerable_and_rankable_on_both_byte_orders() {
        for endian in [Endian::Big, Endian::Little] {
            let svc = service();
            svc.register_expert(expert_ref("expert-old"), &cap("expert-old", 10)).unwrap();
            let served = Served::start(svc);
            let addr = served.registry.primary().unwrap();
            let key = addr.object_key.clone();
            let mut s = TcpStream::connect((addr.host.as_str(), addr.port)).expect("connects");

            let mut request_id = 0u32;
            let mut call = |op: &str, m: &MeasuredCapability| {
                request_id += 1;
                let reference = expert_ref(&m.base.id);
                let msg = orbweaver_giop::encode_request(
                    Version::V1_2,
                    endian,
                    request_id,
                    &key,
                    op,
                    true,
                    |e| {
                        reference.write_to(e).unwrap();
                        m.write_to(e);
                    },
                )
                .unwrap();
                s.write_all(&msg).unwrap();
                let msg = orbweaver_giop::read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                let reply = orbweaver_giop::decode_reply(msg).unwrap();
                assert_eq!(reply.request_id, request_id, "{endian:?} {op}");
                reply.status
            };
            assert_eq!(
                call("register_measured", &measured("expert-math", "math", 12.0)),
                ReplyStatus::NoException,
                "{endian:?}"
            );
            assert_eq!(
                call("register_measured", &measured("expert-math-b", "math", 30.0)),
                ReplyStatus::NoException,
                "{endian:?}"
            );
            // A fresh measurement for expert-math-b arrives by heartbeat: it
            // is now the faster of the two.
            assert_eq!(
                call("heartbeat_measured", &measured("expert-math-b", "math", 8.0)),
                ReplyStatus::NoException,
                "{endian:?}"
            );
            // A duplicate registration is refused on this path exactly as on
            // the v1.0 one — same code underneath.
            assert_eq!(
                call("register_measured", &measured("expert-math", "math", 1.0)),
                ReplyStatus::SystemException,
                "{endian:?}: a second register_measured is BAD_PARAM"
            );
            drop(s);
            let last = served.registry();
            let svc = served.shutdown(last);

            // Read back *through the matcher*, not by inspecting the offer.
            let q = Query::parse(
                "specialization == 'math' AND latency_p50 <= 20 ORDER BY latency_p50 ASC",
            )
            .unwrap();
            svc.with_store(|store| {
                let sel = q.select_reporting(store);
                let ids: Vec<&str> = sel.matched.iter().map(|o| o.id.as_str()).collect();
                assert_eq!(
                    ids,
                    ["expert-math-b", "expert-math"],
                    "{endian:?}: fastest measured first"
                );
                assert!(sel.unranked.is_empty(), "{endian:?}");
                let unanswerable: Vec<&str> =
                    sel.unanswerable.iter().map(|o| o.id.as_str()).collect();
                assert_eq!(unanswerable, ["expert-old"], "{endian:?}: v1.0 cannot answer");
                assert!(!sel.is_complete(), "{endian:?}: a router asked this refuses");
                let old = store.get("expert-old").unwrap();
                assert_eq!((old.specialization.as_deref(), old.latency_p50), (None, None));
                let b = store.get("expert-math-b").unwrap();
                assert_eq!(b.latency_p50, Some(8.0), "{endian:?}: the heartbeat's measurement");
                assert_eq!(b.specialization.as_deref(), Some("math"));
            });
            // …and once the v1.0 expert is out of the candidate set (it is
            // not maths, so `No` beats `Unknown`), the answer is complete.
            assert!(svc.declare_specialization("expert-old", "code"));
            svc.with_store(|store| {
                let sel = q.select_reporting(store);
                assert!(sel.is_complete(), "{endian:?}");
                assert_eq!(sel.matched.len(), 2, "{endian:?}");
            });
        }
    }

    /// A v1.0 `heartbeat` on a measured offer keeps the measurement: the old
    /// shape has no member for either fact and therefore cannot withdraw
    /// them. Before this, the heartbeat rebuilt the offer from the capability
    /// alone and both went back to `None`.
    #[test]
    fn a_v1_0_heartbeat_does_not_erase_what_it_cannot_mention() {
        let svc = service();
        svc.register_measured(expert_ref("expert-math"), &measured("expert-math", "math", 12.0))
            .unwrap();
        let mut old_shape = cap("expert-math", 99);
        // 0.75, exactly representable: the wire member is `float`, the offer
        // is f64, and 0.9 widens to 0.8999999761581421 — the first run of
        // this test failed on that and not on the servant.
        old_shape.load = 0.75;
        svc.heartbeat(expert_ref("expert-math"), &old_shape).unwrap();
        let offer = svc.with_store(|s| s.get("expert-math").cloned()).unwrap();
        assert_eq!(offer.mem_footprint, 99, "the members the heartbeat carries are updated");
        assert_eq!(offer.load, 0.75);
        assert_eq!(
            offer.specialization.as_deref(),
            Some("math"),
            "…and the ones it cannot are kept"
        );
        assert_eq!(offer.latency_p50, Some(12.0));
        // The out-of-band declaration survives a v1.0 heartbeat by the same
        // rule — it used not to.
        svc.register_expert(expert_ref("expert-code"), &cap("expert-code", 10)).unwrap();
        assert!(svc.declare_specialization("expert-code", "code"));
        svc.heartbeat(expert_ref("expert-code"), &cap("expert-code", 11)).unwrap();
        let offer = svc.with_store(|s| s.get("expert-code").cloned()).unwrap();
        assert_eq!(offer.specialization.as_deref(), Some("code"));
        // A v1.1 heartbeat, by contrast, is how a measurement is *replaced*.
        svc.heartbeat_measured(expert_ref("expert-math"), &measured("expert-math", "math", 15.0))
            .unwrap();
        let offer = svc.with_store(|s| s.get("expert-math").cloned()).unwrap();
        assert_eq!(offer.latency_p50, Some(15.0));
        // And `from_offer` refuses to invent a measured shape for the v1.0 one.
        let unmeasured = svc.with_store(|s| s.get("expert-code").cloned()).unwrap();
        assert!(
            MeasuredCapability::from_offer(&unmeasured, Residency::Offloaded, "moe/1.0").is_none()
        );
        let measured_back = svc.with_store(|s| s.get("expert-math").cloned()).unwrap();
        let back = MeasuredCapability::from_offer(&measured_back, Residency::Offloaded, "moe/1.0")
            .expect("both members on file");
        assert_eq!(back.latency_p50_ms, 15.0);
        assert_eq!(back.specialization, "math");
    }

    // ── Router::select ──────────────────────────────────────────────────────

    fn gate(top_k: u16) -> GateSignal {
        GateSignal { affinity: Vec::new(), top_k }
    }

    /// Constraints that admit everything: no required capability, and bounds
    /// well above what `cap()` registers (latency 42, cost 1.5).
    fn open(top_k: u16) -> (GateSignal, Constraints) {
        (
            gate(top_k),
            Constraints { required: String::new(), max_latency_ms: 1000.0, max_cost: 100.0 },
        )
    }

    /// Three experts with different routing histories, so the ordering claim
    /// has something to order.
    fn routed() -> ExpertService {
        let svc = service();
        for name in ["expert-a", "expert-b", "expert-c"] {
            svc.register_expert(expert_ref(name), &cap(name, 10)).unwrap();
        }
        for _ in 0..3 {
            svc.record_hit("expert-b");
        }
        svc.record_hit("expert-c");
        svc
    }

    fn keys(experts: &[Ior]) -> Vec<String> {
        experts
            .iter()
            .map(|i| String::from_utf8_lossy(&i.primary().unwrap().object_key).into_owned())
            .collect()
    }

    /// The ordering and the truncation, together: best first by `route_freq`,
    /// ties on ascending id, and never more than `top_k`.
    #[test]
    fn select_returns_the_best_experts_first_and_no_more_than_top_k() {
        let svc = routed();
        let (g, qos) = open(10);
        assert_eq!(
            keys(&svc.select(&g, &qos).unwrap()),
            ["expert-b", "expert-c", "expert-a"],
            "route_freq DESC: b (3 hits), c (1), a (0)"
        );
        let (g, qos) = open(2);
        assert_eq!(keys(&svc.select(&g, &qos).unwrap()), ["expert-b", "expert-c"]);
        // Zero means zero. It is not a sentinel for "everything".
        let (g, qos) = open(0);
        assert!(svc.select(&g, &qos).unwrap().is_empty());
    }

    /// The bounds are real bounds, inclusively applied, and a query that
    /// genuinely excludes everything returns an **empty sequence** — which is
    /// the true answer "nothing qualifies", and is a different reply from the
    /// refusal below.
    #[test]
    fn bounds_exclude_and_an_honest_nothing_is_an_empty_sequence() {
        let svc = routed();
        let g = gate(10);
        let admits = Constraints { required: String::new(), max_latency_ms: 42.0, max_cost: 1.5 };
        assert_eq!(svc.select(&g, &admits).unwrap().len(), 3, "the bounds are inclusive");

        let too_fast =
            Constraints { required: String::new(), max_latency_ms: 41.9, max_cost: 100.0 };
        assert!(svc.select(&g, &too_fast).unwrap().is_empty());
        let too_cheap =
            Constraints { required: String::new(), max_latency_ms: 1000.0, max_cost: 1.4 };
        assert!(svc.select(&g, &too_cheap).unwrap().is_empty());
        // The all-zero probe body a sweep sends: a real bound of zero, which
        // nothing satisfies. Answered, not refused.
        assert!(svc.select(&gate(0), &Constraints::default()).unwrap().is_empty());
    }

    /// The finding this operation was written around: a constraint naming a
    /// property the offer cannot answer refuses the **whole call**, rather
    /// than returning the offers that happened to be judgeable.
    ///
    /// Three states, because only the middle one is subtle: nothing knows its
    /// specialization (refuse), *some* do (refuse — a short list would claim
    /// to be complete), all do (answer, and answer with only the matches).
    #[test]
    fn a_constraint_the_offers_cannot_answer_refuses_the_whole_call() {
        let svc = routed();
        let g = gate(10);
        let math =
            Constraints { required: "math".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };

        // None: every offer arrived over a contract with no specialization.
        assert_eq!(
            svc.select(&g, &math).unwrap_err().id,
            NO_IMPLEMENT,
            "an unanswerable constraint is not 'no experts do maths'"
        );

        // Some: expert-b is known to do maths, the other two are unjudged.
        // Returning just expert-b would say "this is every maths expert".
        assert!(svc.declare_specialization("expert-b", "math"));
        assert_eq!(
            svc.select(&g, &math).unwrap_err().id,
            NO_IMPLEMENT,
            "a partial answer that looks complete is the failure mode, not the fix"
        );

        // All: now the question is answerable, and the answer is only the
        // offers that match.
        assert!(svc.declare_specialization("expert-a", "vision"));
        assert!(svc.declare_specialization("expert-c", "math"));
        assert_eq!(
            keys(&svc.select(&g, &math).unwrap()),
            ["expert-b", "expert-c"],
            "route_freq DESC among the maths experts, and no vision expert"
        );
        // …and a capability nobody claims is now a true empty, not a refusal.
        let none = Constraints { required: "cooking".to_owned(), ..math.clone() };
        assert!(svc.select(&g, &none).unwrap().is_empty());

        // The escape hatch a caller has today: drop the constraint the
        // contract cannot carry, and everything else still answers.
        let svc = routed();
        let (g, qos) = open(10);
        assert_eq!(svc.select(&g, &qos).unwrap().len(), 3);
    }

    /// `declare_specialization` amends one property and disturbs nothing else
    /// — the residency mirror included, since a heartbeat is what it rides on.
    #[test]
    fn declaring_a_specialization_leaves_the_rest_of_the_offer_alone() {
        let svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.prefetch("expert-a").unwrap();
        svc.complete_load("expert-a").unwrap();
        svc.record_hit("expert-a");

        assert!(svc.declare_specialization("expert-a", "math"));
        assert!(
            !svc.declare_specialization("expert-ghost", "math"),
            "an unknown id declares nothing"
        );
        let offer = svc.with_store(|s| s.get("expert-a").cloned()).unwrap();
        assert_eq!(offer.specialization.as_deref(), Some("math"));
        assert_eq!(offer.residency, Residency::Resident, "the mirror still agrees with the loader");
        assert_eq!(offer.route_freq, FREQ_SCALE, "the store still owns routing history");
        assert_eq!(offer.mem_footprint, 10);
        assert_eq!(svc.with_loader(|l| l.status("expert-a")), Some(Residency::Resident));
        // …and the p50 the contract also cannot carry is still unknown, which
        // is the point: this closes one gap, not the class.
        assert_eq!(offer.latency_p50, None);
    }

    /// The `Tensor` is decoded and not read. Same store, same `top_k`, four
    /// different affinity blobs — including one large enough to be a payload
    /// rather than a handle — and one answer.
    ///
    /// Stated as a property because the alternative reading of these bytes is
    /// the open decision in PLAN-MOE §4.6: a `select` that quietly started
    /// interpreting them would be making that decision, and this test is what
    /// makes that a change somebody has to notice.
    #[test]
    fn the_affinity_tensor_is_not_read() {
        let svc = routed();
        let qos = open(10).1;
        let baseline = keys(&svc.select(&gate(10), &qos).unwrap());
        for affinity in
            [vec![], vec![0u8], vec![0xff; 3], (0..4096u32).map(|i| i as u8).collect::<Vec<_>>()]
        {
            let g = GateSignal { affinity, top_k: 10 };
            assert_eq!(keys(&svc.select(&g, &qos).unwrap()), baseline);
        }
    }

    /// An argument that cannot be turned into a query literal is the caller's
    /// problem, and it is a different exception from the store's gap.
    #[test]
    fn a_constraint_that_cannot_be_a_query_literal_is_bad_param() {
        let svc = routed();
        let g = gate(1);
        for qos in [
            Constraints { required: "it's math".to_owned(), max_latency_ms: 1.0, max_cost: 1.0 },
            Constraints { required: String::new(), max_latency_ms: f32::NAN, max_cost: 1.0 },
            Constraints { required: String::new(), max_latency_ms: 1.0, max_cost: f32::INFINITY },
        ] {
            assert_eq!(svc.select(&g, &qos).unwrap_err().id, BAD_PARAM, "{qos:?}");
        }
    }

    /// The query text is the delegation, so it is pinned: a member that
    /// silently started comparing against a different field would still
    /// return plausible experts.
    #[test]
    fn the_constraints_become_the_documented_query() {
        let qos = Constraints { required: "math".to_owned(), max_latency_ms: 200.0, max_cost: 2.5 };
        assert_eq!(
            qos.to_query_text().unwrap(),
            "specialization == 'math' AND latency_p99 <= 200 AND cost <= 2.5 \
             ORDER BY route_freq DESC"
        );
        // An empty `required` drops the conjunct rather than matching '' .
        let qos = Constraints { required: String::new(), ..qos };
        assert_eq!(
            qos.to_query_text().unwrap(),
            "latency_p99 <= 200 AND cost <= 2.5 ORDER BY route_freq DESC"
        );
        // Every text this can build is one the engine parses.
        assert!(Query::parse(&qos.to_query_text().unwrap()).is_ok());
        // …including the awkward widenings: an f32 bound printed as f64 must
        // still lex, which rules out exponent notation.
        for bound in [0.1_f32, -0.0, 1e30, 1e-30, f32::MIN, f32::MAX] {
            let qos =
                Constraints { required: String::new(), max_latency_ms: bound, max_cost: bound };
            let text = qos.to_query_text().unwrap();
            assert!(Query::parse(&text).is_ok(), "{bound} produced {text:?}");
        }
    }

    /// The two structs marshal in the IDL's declaration order, decoded with
    /// the primitive getters so a swap in both directions still fails, in both
    /// byte orders.
    #[test]
    fn gate_signal_and_constraints_are_in_the_idls_declaration_order() {
        let g = GateSignal { affinity: vec![1, 2, 3], top_k: 7 };
        let qos =
            Constraints { required: "math".to_owned(), max_latency_ms: 200.5, max_cost: 0.25 };
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            g.write_to(&mut e);
            qos.write_to(&mut e);
            let bytes = e.finish().unwrap();

            let mut d = Decoder::new(&bytes, endian);
            // corpus/golden/22 line 33, then line 34.
            assert_eq!(d.get_octet_seq().unwrap(), &[1, 2, 3], "1 affinity {endian:?}");
            assert_eq!(d.get_u16().unwrap(), 7, "2 top_k {endian:?}");
            assert_eq!(d.get_string().unwrap(), "math", "3 required {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 200.5, "4 max_latency_ms {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 0.25, "5 max_cost {endian:?}");

            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(GateSignal::read_from(&mut d).unwrap(), g, "round trip {endian:?}");
            assert_eq!(Constraints::read_from(&mut d).unwrap(), qos, "round trip {endian:?}");
        }
    }

    // ── a served instance ───────────────────────────────────────────────────

    /// The service on loopback. Clients are used sequentially and `shutdown`
    /// is called with the last one still open — the stop flag is raised
    /// before it drops, so the serve loop observes it after the connection
    /// ends rather than blocking in accept. Sequential use is this test's
    /// choice; since stream E the server would accept overlapping clients.
    struct Served {
        registry: Ior,
        loader: Ior,
        router: Ior,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<ExpertService>>,
    }

    impl Served {
        fn start(mut svc: ExpertService) -> Self {
            let server = Orb::new().server("127.0.0.1:0", b"MoE/registry".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            svc.host = "127.0.0.1".into();
            svc.port = port;
            let (registry, loader, router) =
                (svc.registry_ior(), svc.loader_ior(), svc.router_ior());
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve(&mut svc, || flag.load(Ordering::SeqCst)).unwrap();
                svc
            });
            Served { registry, loader, router, stop, thread: Some(thread) }
        }

        fn registry(&self) -> Connection {
            Connection::connect(&self.registry, T).unwrap()
        }

        fn loader(&self) -> Connection {
            Connection::connect(&self.loader, T).unwrap()
        }

        fn router(&self) -> Connection {
            Connection::connect(&self.router, T).unwrap()
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
        let svc = served.shutdown(ldr);

        assert_eq!(
            svc.with_store(|s| s.get("expert-code").unwrap().mem_footprint),
            60,
            "heartbeat landed"
        );
        // …and 0.9 is not 0.9 on the other side: the contract declares
        // `float`, so the offer's f64 holds what an f32 could carry. Pinned
        // as the f32 widening rather than papered over with a tolerance,
        // because the lossy step is the contract's, not an arithmetic slip.
        assert_eq!(svc.with_store(|s| s.get("expert-code").unwrap().load), f64::from(0.9_f32));

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
        assert_eq!(svc.with_loader(|l| l.status("expert-code")), Some(Residency::Offloaded));
        assert_eq!(
            svc.with_store(|s| s.get("expert-code").unwrap().residency),
            Residency::Offloaded,
            "the store mirrors what the loader actually did"
        );
        assert_eq!(svc.with_store(|s| s.get("expert-math").unwrap().residency), Residency::Active);
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
        let svc = service();
        for name in ["expert-a", "expert-b"] {
            svc.register_expert(expert_ref(name), &cap(name, 10)).unwrap();
        }
        let agree = |svc: &ExpertService, what: &str| {
            for (id, state) in svc.with_loader(|l| l.states()) {
                assert_eq!(
                    svc.with_store(|s| s.get(&id).map(|o| o.residency)),
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
            let svc = service();
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
                let svc = service();
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
        let svc = service();
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
        let svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.prefetch("expert-a").unwrap();
        svc.complete_load("expert-a").unwrap();
        svc.observe_free_memory(10_000);
        let err = svc.evict("expert-a").unwrap_err();
        assert_eq!(err.id, TRANSIENT);
        assert_eq!(svc.with_loader(|l| l.status("expert-a")), Some(Residency::Resident));
    }

    /// Registering twice would reset the residency and the routing history,
    /// and both halves must refuse it — the store's and the loader's.
    #[test]
    fn a_duplicate_registration_refuses_and_leaves_both_halves_untouched() {
        let svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.record_hit("expert-a");
        let err = svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 999)).unwrap_err();
        assert_eq!(err.id, BAD_PARAM);
        assert_eq!(svc.with_store(|s| s.get("expert-a").unwrap().mem_footprint), 10);
        assert_eq!(svc.with_store(|s| s.get("expert-a").unwrap().route_freq), FREQ_SCALE);
        assert_eq!(svc.with_store(|s| s.len()), 1);
    }

    /// `deregister` has only the reference to go on, so the reference table,
    /// the offer and the loader entry must all go — and the heartbeat that
    /// re-announced an address is what keeps the lookup working afterwards.
    #[test]
    fn deregistration_is_by_reference_and_clears_all_three_halves() {
        let svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();

        // A reference to the same object at a different address: §7.2.1 lets
        // comparison say "different", so this is refused rather than guessed.
        let mut moved = expert_ref("expert-a");
        moved.profiles[0].host = "192.0.2.99".into();
        assert_eq!(svc.deregister(&moved).unwrap_err().id, BAD_PARAM);
        assert_eq!(svc.with_store(|s| s.len()), 1, "the refusal changed nothing");

        // The heartbeat is how an expert that moved says so.
        svc.heartbeat(moved.clone(), &cap("expert-a", 10)).unwrap();
        assert_eq!(svc.reference_for("expert-a"), Some(moved.clone()));
        svc.deregister(&moved).unwrap();
        assert!(svc.with_store(|s| s.get("expert-a").is_none()));
        assert_eq!(svc.with_loader(|l| l.status("expert-a")), None, "the loader forgot it too");
        assert_eq!(svc.reference_for("expert-a"), None);
    }

    /// Forgetting an expert with a call inflight would strand the caller.
    #[test]
    fn deregistration_waits_for_an_inflight_call() {
        let svc = service();
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
        let svc = service();
        svc.register_expert(expert_ref("expert-a"), &cap("expert-a", 10)).unwrap();
        svc.pin("expert-a").unwrap();
        assert!(svc.with_loader(|l| l.is_pinned("expert-a")));
        assert!(svc.with_store(|s| s.is_pinned("expert-a")));
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
        drop(ldr);

        let mut router = served.router();
        for (id, want) in [
            (ROUTER_ID, true),
            (OBJECT_ID, true),
            (EXPERT_REGISTRY_ID, false),
            (EXPERT_LOADER_ID, false),
        ] {
            let reply = router.invoke("_is_a", move |e| e.put_str(id)).unwrap();
            assert!(reply.body().unwrap().get_bool().unwrap() == want, "router _is_a {id}");
        }
        // The other half of `Router`, refused with the reason in the module
        // docs — and a neighbouring interface's operation, refused because
        // these are three objects rather than one with a union of operations.
        let err = router.invoke("dispatch", |e| e.put_octet_seq(&[])).unwrap_err();
        assert_eq!(
            exception_id(err),
            orbweaver_giop::server::NO_IMPLEMENT,
            "Router::dispatch is declared and deliberately not served (D006), which is a \
             different fact from a name this interface does not declare"
        );
        let err = router.invoke("status", |e| e.put_str("expert-a")).unwrap_err();
        assert_eq!(exception_id(err), orbweaver_giop::server::BAD_OPERATION, "loader op on router");
        served.shutdown(router);
    }

    /// `select` over a real socket: the arguments decode as corpus/golden/22
    /// declares them and the reply is an `ExpertSeq` — a length then that many
    /// object references, which is the shape a generated client reads.
    #[test]
    fn select_answers_an_expert_seq_on_the_wire() {
        let svc = routed();
        let served = Served::start(svc);
        let mut router = served.router();

        let (g, qos) = open(2);
        let reply = router
            .invoke("select", move |e| {
                g.write_to(e);
                qos.write_to(e);
            })
            .expect("select is served");
        let mut b = reply.body().unwrap();
        let n = b.get_u32().unwrap();
        assert_eq!(n, 2, "top_k truncated the three registered experts");
        let mut got = Vec::new();
        for _ in 0..n {
            let reference = get_reference(&mut b).unwrap().expect("a live reference, not nil");
            assert_eq!(reference.type_id, EXPERT_ID, "the sequence is of moe::Expert");
            got.push(
                String::from_utf8_lossy(&reference.primary().unwrap().object_key).into_owned(),
            );
        }
        assert_eq!(got, ["expert-b", "expert-c"], "route_freq DESC, over the wire");
        assert!(b.is_empty(), "the reply body is the sequence and nothing else");

        // And the refusal, on the wire, for the constraint the contract
        // cannot carry — the one a caller must not retry.
        let g = gate(10);
        let math =
            Constraints { required: "math".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };
        let err = router
            .invoke("select", move |e| {
                g.write_to(e);
                math.write_to(e);
            })
            .unwrap_err();
        assert_eq!(exception_id(err), NO_IMPLEMENT);
        served.shutdown(router);
    }

    /// An object key this service never minted is nobody's.
    #[test]
    fn an_unknown_object_key_is_not_ours() {
        let svc = service();
        assert!(SharedDispatch::knows(&svc, svc.registry_key()));
        assert!(SharedDispatch::knows(&svc, svc.loader_key()));
        assert!(SharedDispatch::knows(&svc, svc.router_key()));
        assert!(!SharedDispatch::knows(&svc, b"MoE"), "the base key alone names none of them");
        assert!(!SharedDispatch::knows(&svc, b"NameService"));
    }
}
