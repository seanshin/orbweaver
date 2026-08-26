//! `moe::Expert`, served by the thing that owns the expert's weights — the
//! one place [`ExpertLocator`](crate::residency::ExpertLocator) can be mounted, and the answer to *who owns an
//! expert's server*.
//!
//! [`crate::residency::MissPolicy::Activate`] closes D029 §6.1's
//! **Activation / load** leak at the POA: a demand load inside `locate`, so a
//! caller holding only a reference cannot tell an evicted target from a
//! resident one. It was landed and **not adopted** — until this module,
//! `ExpertLocator` had zero references outside `residency.rs`, nothing mounted
//! it on a served POA, and every deployment therefore ran one of the two
//! *refusing* policies. The closure existed and nothing had chosen it.
//!
//! # Who owns an expert's server
//!
//! **Whoever owns the expert's residency owns its server, and the mount is the
//! statement that these are one object.** That is not a preference; it is what
//! the mechanism requires in both directions:
//!
//! - A POA consults a locator only for ids inactive **in that POA**
//!   ([`Poa::dispatch_target`]). Owning the POA without owning the loader
//!   means answering [`Located::Here`](crate::Located::Here) for something you cannot load.
//! - Owning the loader without owning the POA means loading something no
//!   dispatch will ever reach.
//!
//! Neither of the two servants that existed satisfies it, and each fails on
//! its own recorded argument rather than by accident:
//!
//! - **[`crate::expert_service::ExpertService`] owns the loader and serves no
//!   expert.** `register_expert` stores an [`Ior`] *verbatim*, so an expert's
//!   address is chosen by whoever registers it and is generally another
//!   process. Its module docs already refuse the other half — *"an `Expert`
//!   servant on the registry's object would answer for an expert that does not
//!   live here"* — and a demand load taken there would load weights into a
//!   process with nothing to serve them from.
//! - **[`crate::tenant_service::TenantService`] serves experts and owns no
//!   residency.** Its `Kind::Expert` and `Kind::Base` objects are in a plain
//!   map; its only notion of residency is `check_residency`, which is about
//!   *region*, not about load. Giving it a loader would make load state a
//!   tenancy fact, and the shared base at `<base>/shared/base/<model>` belongs
//!   to no tenant — so the one object whose weights are actually shared would
//!   be the one whose residency had no owner.
//!
//! So the mount needed a third thing, and this is it: a servant that owns a
//! [`Poa`], owns the [`ExpertLoader`] for the ids that POA mints keys for, and
//! serves `moe::Expert`'s three declared operations for them. It is
//! deliberately **not** wired into either service. Nothing that exists behaves
//! differently for this module having landed; a deployment adopts the closure
//! by constructing an [`ExpertHost`], and one that does not is exactly as it
//! was.
//!
//! *전문가의 서버는 **그 전문가의 잔류 상태를 소유한 쪽**이 소유하며, 마운트는 이
//! 둘이 하나의 객체라는 선언이다. 레지스트리는 로더를 갖지만 전문가를 서빙하지
//! 않고(`register_expert`가 받는 `Ior`는 다른 프로세스를 가리킨다), 테넌트
//! 서비스는 전문가를 서빙하지만 잔류 상태를 갖지 않는다(공유 베이스는 어느
//! 테넌트에도 속하지 않으므로 그 잔류의 주인이 없어진다). 그래서 세 번째 것이
//! 필요했고, 이 모듈이 그것이다. 기본 동작은 바뀌지 않는다 — 아무것도 이것을
//! 자동으로 마운트하지 않는다.*
//!
//! # The default is [`MissPolicy::Activate`], and that is the adoption
//!
//! Constructing an [`ExpertHost`] chooses the closing policy. The two refusing
//! variants stay reachable through [`ExpertHost::with_miss_policy`] for a
//! deployment that has priced the wait, and choosing one of them is choosing
//! the leak — [`MissPolicy`]'s own rustdoc is where that trade is argued and
//! it is not restated here.
//!
//! # `knows` is residency-independent, and that is a second leak surface
//!
//! A `LocateRequest` (§9.4.5) is the side-effect-free probe, answered from
//! [`SharedDispatch::knows`] one message *before* an invocation. A `knows`
//! that consulted the loader's state would let a caller learn residency
//! without ever invoking — the same property leaking through the cheaper
//! message, which is the class D029's `LocateRequest` subsection closed for
//! *location* on the very same day this row was closed for *load*.
//!
//! So [`ExpertHost::knows`] answers for **every id this host was given**,
//! loaded or not, and the load decision is taken in exactly one place: the POA
//! consulting the locator. A hosted-but-offloaded expert probes `ObjectHere`
//! and then invokes successfully, which is one consistent answer rather than
//! two that disagree.
//!
//! # Which operation can measure the transparency, and which cannot
//!
//! `moe::Capability` has nine members and the fifth is `Residency state`. So
//! **the contract's own `describe()` reports load state**, and D029's row says
//! why that is not a leak: it is one of the two homes where load state is *a
//! value a caller asks for*, a right to be told rather than a side channel.
//!
//! A byte-identity test therefore cannot naively assert that `describe`'s
//! reply is unchanged *because* nothing depends on residency — that reasoning
//! is false here even though the assertion happens to hold. It holds for a
//! sharper reason, which [`ExpertHost::describe_state`] states and a test
//! pins: **the demand load happens in `locate`, before the servant runs**, so
//! by the time `describe` reads the loader the expert is RESIDENT again and
//! the honest report is the same word it was before the eviction. The caller
//! is told the truth, and the truth is unchanged.
//!
//! `process` is the operation that is residency-independent *by construction*
//! — PLAN-MOE §5: no accelerator, no kernel, no weights, so it returns the
//! activation unchanged, exactly as
//! [`TenantService`](crate::tenant_service::TenantService) does — and it is
//! the one a measurement should prefer.
//!
//! # What this does not do
//!
//! - **Time.** A demand-loaded call is slower and a caller with a clock can
//!   tell. In this repository a load is two map writes, so that latency is
//!   real in a deployment and **absent from every test here**. Nothing in this
//!   module measures it and nothing here should be read as having.
//! - **A failed demand load.** See [`ExpertHost::dispatch`]: it is
//!   `OBJECT_NOT_EXIST`, which is the leak put straight back, and it is the
//!   *same open question* as [`MissPolicy`]'s missing deadline — both need an
//!   answer that is neither "here" nor "never existed", and neither has a
//!   client that would act on one. Unreachable in this repository, live in a
//!   deployment.
//! - **Call markers.** `begin_call`/`end_call` are not taken around a
//!   dispatch. They would put the expert ACTIVE for the duration, which the
//!   eviction guard reads as inflight; a host that wants them takes them out
//!   of band, as [`crate::expert_service`] does, and owes the guard a thought
//!   about what an evictable expert means mid-call.
//! - **Forwarding.** [`ExpertLocator`](crate::residency::ExpertLocator) never answers [`Located::Forward`](crate::Located::Forward) —
//!   placement is the trading service's (§4.3) and this host has no view of
//!   another node. The arm is answered `INTERNAL` rather than
//!   `OBJECT_NOT_EXIST` **on purpose**: the second is the exact reply the leak
//!   produces, so a forwarding locator mounted here would look like a
//!   regression of the property instead of like the unfinished work it is.

use std::collections::BTreeMap;

use orbweaver_cdr::Encoder;
use orbweaver_giop::Ior;
use orbweaver_giop::guarded::Guarded;
use orbweaver_giop::server::{LocateStatus, Request, SharedDispatch, SystemException};

use crate::expert_service::{Capability, EXPERT_ID};

use crate::residency::{BatchStats, ExpertLoader, MissPolicy, Residency, TransitionError};

use crate::tenant_service::{Activation, CallContext};
use crate::{Lifespan, ObjectId, OrbPoa, Poa, Target, UnknownIdPolicy, put_reference};

/// The members of `moe::Capability` an expert host *reports*, which is all of
/// them except the one the loader owns.
///
/// `state` is absent by construction rather than by convention. The authority
/// table in [`crate::expert_service`]'s module docs settles that the loader
/// owns residency and the wire copy is a report; a struct that carried a
/// `state` field here would be a second copy of that truth sitting inside the
/// servant, free to drift from the machine one lock away.
///
/// *`state`가 없는 것은 관례가 아니라 구성이다 — 잔류의 authority는 로더이고,
/// 여기에 필드를 두면 그 진실의 두 번째 사본이 된다.*
#[derive(Debug, Clone, PartialEq)]
pub struct Reported {
    /// `float cost`.
    pub cost: f32,
    /// `float latency_p99_ms`.
    pub latency_p99_ms: f32,
    /// `float load`.
    pub load: f32,
    /// `unsigned long long mem_footprint`.
    pub mem_footprint: u64,
    /// `float route_freq`.
    pub route_freq: f32,
    /// `string placement_node`.
    pub placement_node: String,
    /// `string contract_version`.
    pub contract_version: String,
}

impl Reported {
    /// The `Capability` this host answers `describe()` with, given the
    /// residency the **loader** holds right now.
    fn with_state(&self, id: &str, state: Residency) -> Capability {
        Capability {
            id: id.to_owned(),
            cost: self.cost,
            latency_p99_ms: self.latency_p99_ms,
            load: self.load,
            state,
            mem_footprint: self.mem_footprint,
            route_freq: self.route_freq,
            placement_node: self.placement_node.clone(),
            contract_version: self.contract_version.clone(),
        }
    }
}

/// The POA, the loader and the reported members — one lock over all three.
///
/// The same reason [`crate::expert_service`] gives: the POA's activation set
/// and the residency map are two copies of one truth kept in step by
/// [`ExpertLoader::reconcile`], and a reader that saw one without the other
/// would see the mid-mirror state the choke point exists to prevent.
#[derive(Debug)]
struct HostState {
    poa: Poa,
    loader: ExpertLoader,
    reported: BTreeMap<String, Reported>,
}

/// A server for the experts whose weights this process owns.
///
/// See the module docs for the ownership argument, the `knows` decision and
/// what this does not do.
#[derive(Debug)]
pub struct ExpertHost {
    miss: MissPolicy,
    state: Guarded<HostState>,
}

impl ExpertHost {
    /// A host whose references point at `host:port`, over a POA named
    /// `poa_name`.
    ///
    /// The POA is `Persistent` — so a key carries no incarnation and the
    /// reference a caller holds is the reference a redeployment would mint —
    /// and `AskLocator`, which is §15.3.8.6's `USE_SERVANT_MANAGER` and the
    /// policy without which no [`MissPolicy`] could matter at all. Both are
    /// fixed rather than offered: a host built `Reject` would silently be a
    /// host with no closure in it, which is the state this module exists to
    /// end.
    ///
    /// `host` is separate from the bind address — Phase 0 assumption D.
    pub fn new(host: &str, port: u16, poa_name: &str) -> Self {
        let orb = orbweaver_giop::orb::Orb::new();
        let poa = orb
            .create_poa(poa_name, EXPERT_ID)
            .with_lifespan(Lifespan::Persistent)
            .with_unknown_id(UnknownIdPolicy::AskLocator)
            .publish_at(host, port);
        Self {
            miss: MissPolicy::Activate,
            state: Guarded::new(
                "an expert host",
                HostState { poa, loader: ExpertLoader::new(), reported: BTreeMap::new() },
            ),
        }
    }

    /// Runs a different [`MissPolicy`].
    ///
    /// The default is [`MissPolicy::Activate`] and it is the one that closes
    /// the leak. The two refusing variants are here so that a deployment that
    /// wants one **says so**, at the point where the trade is taken, rather
    /// than inheriting it from an unmounted locator.
    #[must_use]
    pub fn with_miss_policy(mut self, miss: MissPolicy) -> Self {
        self.miss = miss;
        self
    }

    /// The policy this host runs.
    pub fn miss_policy(&self) -> MissPolicy {
        self.miss
    }

    /// Takes an expert onto this node: registered, loaded, and activated on
    /// the POA.
    ///
    /// RESIDENT and not OFFLOADED, because an expert whose weights have just
    /// been placed on the node *is* on the node; a host that started
    /// everything offloaded would be describing a different deployment and
    /// would make the first call of every reference a demand load.
    pub fn host_expert(&self, id: &str, reported: Reported) -> Result<(), TransitionError> {
        self.state.write(|s| {
            s.loader.register(id, Lifespan::Persistent)?;
            s.loader.request_prefetch(id)?;
            s.loader.complete_load(id)?;
            s.loader.reconcile(&mut s.poa);
            s.reported.insert(id.to_owned(), reported);
            Ok(())
        })
    }

    /// Evicts a hosted expert and brings the POA's activation set back in
    /// line.
    ///
    /// The [`ExpertLoader::reconcile`] is not optional and not a tidy-up:
    /// without it the POA keeps serving the id from its active map, the
    /// locator is never consulted, and a test of this property would pass
    /// while measuring nothing.
    pub fn evict(&self, id: &str, window: &BatchStats) -> Result<Residency, TransitionError> {
        self.state.write(|s| {
            let out = s.loader.evict(id, window)?;
            s.loader.reconcile(&mut s.poa);
            Ok(out)
        })
    }

    /// The residency the loader holds for `id`, read out of band.
    ///
    /// For a test or an operator. A *caller* learns this from `describe()`,
    /// which is the contract's own answer and not this.
    pub fn residency(&self, id: &str) -> Option<Residency> {
        self.state.read(|s| s.loader.status(id))
    }

    /// The reference a caller holds for `id`, or `None` if this host was never
    /// told where it publishes.
    ///
    /// Minted by the POA, so it carries the POA's key and nothing about
    /// residency — which is the whole of what "holding only a reference" has
    /// to mean for the row to be about anything.
    pub fn reference(&self, id: &str) -> Option<Ior> {
        self.state.read(|s| s.poa.reference(&ObjectId::from_name(id)))
    }

    /// The ids this host was given, loaded or not.
    pub fn hosted(&self) -> Vec<String> {
        self.state.read(|s| s.reported.keys().cloned().collect())
    }

    /// Why `describe()`'s `Residency state` member is the same word before and
    /// after an eviction under [`MissPolicy::Activate`], returned as data so a
    /// test asserts against it rather than against a comment.
    ///
    /// `state` is the one member of `moe::Capability` that reads the loader,
    /// so the naive reading — *"the reply is identical because nothing in it
    /// depends on residency"* — is **false** for this operation. The true
    /// reason is ordering: [`Poa::dispatch_target`] consults the locator
    /// *before* the servant runs, the locator's `Activate` arm ends RESIDENT,
    /// and so the residency `describe` reports is the residency the expert
    /// has by the time anyone can ask. The caller is told the truth and the
    /// truth is unchanged.
    ///
    /// Under a refusing policy the servant is never reached at all, so there
    /// is no reply for this to be about.
    pub fn describe_state(miss: MissPolicy) -> Option<Residency> {
        match miss {
            MissPolicy::Activate => Some(Residency::Resident),
            MissPolicy::Refuse | MissPolicy::RefuseAndPrefetch => None,
        }
    }

    /// The id `key` names, if this POA minted it.
    fn id_of(state: &HostState, key: &[u8]) -> Option<String> {
        state.poa.parse_key(key).and_then(|id| id.as_str().map(str::to_owned))
    }
}

impl SharedDispatch for ExpertHost {
    /// Every id this host was given, **regardless of residency**.
    ///
    /// See the module docs: a `knows` that consulted the loader would answer a
    /// `LocateRequest` differently for an evicted expert, and the caller would
    /// have learned the load state from the probe without spending an
    /// invocation. One decision, taken once, in the POA.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.state
            .read(|s| Self::id_of(s, object_key).is_some_and(|id| s.reported.contains_key(&id)))
    }

    /// `ObjectHere` for a hosted expert, offloaded or not.
    ///
    /// Spelled out rather than left to the default so that the property is
    /// stated where a reader looks for it. The default composition over
    /// [`SharedDispatch::knows`] gives the same answer, and a `knows` that
    /// grew a residency test would break both together — which is why the two
    /// are one function and not two.
    fn locate(&self, object_key: &[u8]) -> LocateStatus {
        if self.knows(object_key) { LocateStatus::ObjectHere } else { LocateStatus::UnknownObject }
    }

    /// `moe::Expert`'s three declared operations, over the POA and the
    /// locator.
    ///
    /// # What a caller sees when the demand load fails
    ///
    /// `OBJECT_NOT_EXIST` — the same reply an unregistered id gets, and
    /// therefore **the leak put straight back**: the caller can tell, and what
    /// it is told is false, because the object does exist. Three things about
    /// that, none of them "it is fine":
    ///
    /// 1. **In this repository the branch is unreachable.** Under one lock,
    ///    [`ExpertLocator`](crate::residency::ExpertLocator) reaches `complete_load` only from OFFLOADED or
    ///    PREFETCHING, and both edges are unguarded, so the `Err` arm cannot
    ///    be taken. It is live in a deployment where the copy can fail.
    /// 2. **It is the same open question as the missing deadline.**
    ///    [`MissPolicy`]'s rustdoc records that an `Activate` with a real
    ///    weight copy needs a ceiling and that the hard half is what the
    ///    expiry *answers*, because `OBJECT_NOT_EXIST` reinstates the leak.
    ///    A load that fails and a load that times out want the same reply, and
    ///    the candidates are the same two: `TRANSIENT`, or a [`Located`](crate::Located)
    ///    variant carrying a retry-after. Neither is added here, for the
    ///    refusal's own standard — there is no client that acts on either, and
    ///    adding one without such a client would be decoration.
    /// 3. **It is a refusal a caller should be able to retry**, which is what
    ///    distinguishes it from case (a): an id nobody registered is
    ///    `OBJECT_NOT_EXIST` correctly and forever, and that is pinned by
    ///    test. Conflating the two is the defect; the fix is a status, and the
    ///    status is the decision above.
    ///
    /// *요구 적재가 실패하면 호출자는 `OBJECT_NOT_EXIST`를 본다 — 구멍이 그대로
    /// 되돌아온다. 이 저장소에서는 그 분기에 도달할 수 없고, 배포에서는 도달한다.
    /// 그리고 이것은 `MissPolicy`가 적어 둔 **마감 시한 만료가 무엇을 답하는가**와
    /// 같은 하나의 질문이다.*
    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut args = request.body().map_err(|_| SystemException::marshal())?;
        let operation = request.operation.as_str();

        self.state.write(|s| {
            let HostState { poa, loader, reported } = s;
            let mut locator = loader.locator(self.miss);
            let id = match poa.dispatch_target(&request.object_key, Some(&mut locator)) {
                Target::Active(id) => id,
                Target::Unknown => return Err(SystemException::object_not_exist()),
                // See the module docs: INTERNAL and not OBJECT_NOT_EXIST, so
                // that an unfinished forward cannot be mistaken for the leak.
                Target::Forward(_) => return Err(SystemException::internal()),
            };
            let name = id.as_str().ok_or_else(SystemException::object_not_exist)?;
            let Some(caps) = reported.get(name) else {
                // The POA minted the key but this host was never given the
                // expert. `knows` already refuses these; a servant that
                // indexed a map here would panic instead.
                return Err(SystemException::object_not_exist());
            };

            match operation {
                // `Capability describe()`. The `state` member is read from the
                // loader — the authority — and not from `caps`.
                "describe" => {
                    let state = loader.status(name).ok_or_else(SystemException::internal)?;
                    caps.with_state(name, state).write_to(out);
                    Ok(())
                }
                // `Activation process(in Activation x, in CallContext ctx)`.
                // Returned unchanged: PLAN-MOE §5, no data plane in this
                // repository, exactly as `TenantService` answers it. That is
                // also what makes it the operation a transparency measurement
                // should use — its reply depends on the argument and on
                // nothing else.
                "process" => {
                    let x =
                        Activation::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let _ctx = CallContext::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    x.write_to(out);
                    Ok(())
                }
                // `Expert delegate(in Capability need)`. The hosted expert
                // that answers to `need.id`, or a nil reference.
                //
                // **Residency-independent on purpose.** A `delegate` that
                // handed back only *loaded* experts would be this row's leak
                // at one remove — and worse than the direct one, because the
                // reference it withholds is exactly the kind D029 names as
                // never having gone through `Router::select`, so no filter
                // anywhere else would have covered it.
                "delegate" => {
                    let need =
                        Capability::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let found = reported
                        .contains_key(&need.id)
                        .then(|| poa.reference(&ObjectId::from_name(&need.id)))
                        .flatten();
                    put_reference(out, found.as_ref()).map_err(|_| SystemException::marshal())
                }
                _ => Err(SystemException::bad_operation()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn reported() -> Reported {
        Reported {
            cost: 1.5,
            latency_p99_ms: 12.0,
            load: 0.25,
            mem_footprint: 1 << 30,
            route_freq: 3.0,
            placement_node: "node-a".to_owned(),
            contract_version: "1.0".to_owned(),
        }
    }

    fn host() -> ExpertHost {
        let h = ExpertHost::new("127.0.0.1", 4011, "experts");
        h.host_expert("expert-a", reported()).expect("a fresh id");
        h
    }

    fn pressure(id: &str) -> BatchStats {
        BatchStats { memory_pressure: true, cold: BTreeSet::from([id.to_owned()]) }
    }

    /// The adoption, as a property: a host built the ordinary way runs the
    /// policy that closes the leak.
    #[test]
    fn the_default_miss_policy_is_the_one_that_closes_the_leak() {
        assert_eq!(host().miss_policy(), MissPolicy::Activate);
    }

    /// The `LocateRequest` probe cannot tell. Without this, a caller learns
    /// residency one message before an invocation and the closure buys
    /// nothing.
    #[test]
    fn a_hosted_expert_probes_the_same_offloaded_or_resident() {
        let h = host();
        let key = h.reference("expert-a").expect("published").primary().expect("one").object_key.clone();
        assert!(matches!(h.locate(&key), LocateStatus::ObjectHere), "resident");
        h.evict("expert-a", &pressure("expert-a")).expect("unpinned, idle, cold");
        assert_eq!(h.residency("expert-a"), Some(Residency::Offloaded), "the eviction took");
        assert!(matches!(h.locate(&key), LocateStatus::ObjectHere), "offloaded — and the same");
    }

    /// A key this POA never minted is not this host's, under every policy.
    #[test]
    fn an_unhosted_key_is_unknown() {
        let h = host();
        assert!(!h.knows(b"experts/expert-nobody"), "minted here, never given");
        assert!(!h.knows(b"SomeOtherPoa/expert-a"), "not this POA's key");
    }

    /// The reference is residency-free: the same bytes before and after.
    #[test]
    fn the_reference_does_not_change_across_an_eviction() {
        let h = host();
        let before = h.reference("expert-a").expect("published");
        h.evict("expert-a", &pressure("expert-a")).expect("evicted");
        assert_eq!(h.reference("expert-a"), Some(before));
    }

    /// `describe_state` is the file's claim about ordering, not a restatement
    /// of the enum.
    #[test]
    fn describe_reports_resident_under_activate_and_nothing_under_a_refusal() {
        assert_eq!(ExpertHost::describe_state(MissPolicy::Activate), Some(Residency::Resident));
        assert_eq!(ExpertHost::describe_state(MissPolicy::Refuse), None);
        assert_eq!(ExpertHost::describe_state(MissPolicy::RefuseAndPrefetch), None);
    }
}
