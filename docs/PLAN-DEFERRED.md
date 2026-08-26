# PLAN-DEFERRED — the excluded services, designed enough to resume

> Companion to [`PLAN-SERVICES.md`](PLAN-SERVICES.md) §8 (Exclusions) and
> [`PLAN-MOE.md`](PLAN-MOE.md) §4. Written 2026-08-13.
> `PLAN-SERVICES.md` §8의 제외 표를 **각 항목의 설계 스케치**로 펼친 문서.
> 제외가 "잊었다"가 아니라 "재개할 만큼은 설계해 두었다"를 뜻하게 하는 것이 목적.

## 0. What this document is / 이 문서의 성격

PLAN-SERVICES §8 excludes seven service areas in four table rows — **fifteen
areas in six rows since 2026-08-26**, when §13–§20 gave a reason and a trigger
to the eight services D023 §1 found carrying neither an implementation nor an
exclusion. (This sentence counts that table, so the two move together; it is
amended rather than rewritten because what it says about *four rows* was true
when this file was written and is what the next paragraph argues against.) A
table row is enough to record a decision and **not** enough to resume from: a future
batch that reads "no consumer names them; adopt-on-demand" starts from a blank
page, re-derives the reasoning, and — the real cost — re-derives it *worse*,
because the reasons that were obvious the day the row was written are gone.

This document gives each exclusion four things and nothing more:

1. **What it is** — the standard's actual surface, so nobody re-reads the OMG
   spec to find out what was declined.
2. **Why deferred** — the argument, not the verdict.
3. **The concrete trigger** — an *observable event*, not a feeling. "If we need
   it" is not a trigger; "the first configuration with two naming domains
   behind one MCP face" is.
4. **A v1 design sketch** — one paragraph, enough that a future batch starts
   from a shape.

**This document adopts nothing, plans no batch, and adds no dependency.** By
construction: every chapter's v1 is first-party spec work in the
PLAN-SERVICES §1 mold, and the two chapters that would touch a dependency
(PSS, non-repudiation) point at paths already cleared by D003 and D002 rather
than opening new ones. Nothing here has a batch number; a chapter graduates by
moving into PLAN-SERVICES with a batch unit and a named oracle, and that move
is the adoption, not this file.

이 문서는 **아무것도 채택하지 않고, 배치를 계획하지 않으며, 의존성을 추가하지
않는다.** 각 장의 v1은 PLAN-SERVICES §1 규칙대로 명세 기반 1st-party 작업이고,
의존성에 닿는 두 장(PSS, 부인방지)은 D003·D002가 이미 정리한 경로를 가리킬 뿐이다.
어떤 장에도 배치 번호가 없다. 한 장은 배치 단위와 오라클을 갖고 PLAN-SERVICES로
옮겨감으로써 졸업하며, **그 이동이 채택**이지 이 파일이 아니다.

PLAN-SERVICES §8이 제외하는 것은 네 행의 일곱 영역이었고, **2026-08-26부터 여섯 행의
열다섯 영역**이다 — D023 §1이 구현도 제외도 없다고 찾아낸 여덟 서비스에 §13–§20이 사유와
방아쇠를 주었다. 위 영어 문장은 그 표를 세는 문장이므로 함께 움직이며, 다시 쓰지 않고
**덧붙여** 고쳤다: *네 행*이라는 말은 이 파일이 쓰이던 날에 참이었고, 바로 그것이 다음
문단이 반박하는 대상이기 때문이다.

### The triggers, in one table / 방아쇠 한눈에

| Chapter | Observable trigger |
|---|---|
| §1 CosNotification | filtering becomes an **isolation** requirement, not a bandwidth one — a consumer must not *receive* what it filters out (F5 tenancy); or F7's split drop counters and a named consumer's discard-on-receipt rate **together** (§1, restated 2026-08-20 when the instrument was built) |
| §2 Transaction / OTS | a multi-object change must survive a **failure** all-or-nothing, and compensation is unacceptable because an observer could act irreversibly on the intermediate state |
| §3 Time Service | a **peer** resolves `TimeService` from our Naming server and we must answer |
| §4 PSS | a pilot peer that is **already a PSS client** and expects storage-homed objects |
| §5 Concurrency Control | the first configuration with **two `ExpertLoader`s over an overlapping id space** |
| §6 CosCollections | a foreign client expecting `CosCollection` by name; or a result set that genuinely cannot be bounded |
| §7 Federated Naming / Trading Links | the first configuration with **more than one naming/trading domain behind one MCP face** |
| §8 Security Service beyond CSIv2 | four separate triggers, one per part — see §8; the earliest is F5 tenancy wanting policy by **domain membership** rather than per call |
| ~~§10 CosEvent supplier-side pull~~ | **FIRED 2026-08-25** — graduated to `PLAN-SERVICES` §4.1. The row stays struck rather than deleted: a trigger table that silently loses its fired rows cannot be read as a record of what this file's triggers have ever done, and this is the first one |
| §11 CosEvent `destroy` | a caller model in the event servant — the operation is unauthenticated and ends the channel for every other client |
| §12 CosNaming chaining to a foreign context | a federation requirement (§7's trigger, seen from the naming servant) — chaining is *possible* today, which is a reason it can be built, not a reason to |
| §13 CosRelationships | the first operation that must answer an **inverse** question (which models are over this base, which compositions bind this expert); or the first operation that **destroys** a target — either one ends "the graph only grows" |
| §14 CosContainment | the first object destroyed while something else still names it — §13's second trigger seen from the lifetime-bound end |
| §15 CosReference | the first decision that must know **how many** holders name a target rather than that it exists — eviction counts calls in flight, never compositions |
| §16 CosCompoundLifeCycle | a **second** operation traversing the same three roles: one traversal is a behaviour, two are a policy |
| §17 CosObjectIdentity | the first caller that must **refute** identity — one that acts on a `false` from a test documented as confirm-only |
| §18 CosExternalization | the `ExpertLoader` blob must be read by something that did not write it — a second loader, another host, or a loader version change |
| §19 CosQuery | an operation whose parameter is a query **in a language the caller names**; or a foreign client expecting `QueryEvaluator` and speaking SQL-92 or OQL-93 |
| §20 CosLicensingManager | a registry entry whose use is metered for somebody who is not the operator (**the weakest trigger of the eight — §20 says so at the site**); or the first requirement that a granted seat be revocable mid-call |
| §21 Portable Interceptors | a service context that must survive a **reply** — today a reply cannot carry one and an inbound reply's are discarded on read; or the first policy the MCP chain enforces that must also apply to a call **not** made through the bridge. D018 §3.3's guess — *a foreign client that expects to register one* — is recorded at the site as **wrong on inspection** |
| §22 BiDirectional GIOP | the first callback consumer that **cannot listen at all** — an endpoint with no server of its own, not a firewalled one. The nominal trigger (*a consumer whose callbacks cannot be dialled*) is **unreachable in this tree and §22 says so at the site**, per D015 §3.4 |

---

## 1. CosNotification — the Event superset / 알림 서비스

**What it is.** OMG Notification is CosEvent plus four things: **structured
events** (`StructuredEvent` = an `EventHeader` with a fixed part
`{domain_name, type_name, event_name}` and a variable part of QoS properties,
then `filterable_data` as name/value pairs, then `remainder_of_body` as an
`any`); **filter objects** attached to admin and proxy objects, evaluating a
constraint grammar against `filterable_data`, grouped AND/OR by admin;
**QoS administration** at channel, admin, proxy and per-message level
(`EventReliability`, `ConnectionReliability`, `Priority`, `Timeout`,
`OrderPolicy`, `DiscardPolicy`, `MaxEventsPerConsumer`); and **batching
proxies** (`SequenceProxyPushSupplier` delivering `EventBatch`). Plus an event
type repository and mapping filters, which are the parts even Notification
implementations tend to skip.

**Why deferred.** PLAN-SERVICES §8 states it: plain CosEvent serves every named
consumer. The reason is more specific than "smaller is better" — Notification's
centre of gravity is *server-side filtering*, and today every consumer of
control-plane events sits behind the MCP boundary, where the guard chain
already filters by authorization. Adding a second filtering point that does not
share the first one's policy is how two filters disagree.

**Trigger.** Filtering becomes an **isolation** requirement rather than a
bandwidth one. The distinction is the whole trigger: client-side filtering is
adequate when receiving-then-discarding is merely wasteful, and inadequate the
moment receiving *is* the leak. F5 tenancy makes that concrete — if tenant A's
residency transitions travel to tenant B's consumer process and are discarded
there, we have shipped tenant A's data to tenant B and called it filtering.
Second, weaker trigger, **restated 2026-08-20 when its instrument was built**:
the old wording — *"F7's bounded buffer reports a measured drop rate
attributable to fan-out of events no consumer wanted"* — was **circular**, and
building the instrument is what showed it. F7 now splits its drops by cause
(`dropped_overflow`, `unrelayable`, `dropped_on_disconnect`,
`dropped_on_failure_disconnect`, `dropped_at_stop`, summing to `dropped`, with
`fanned_out` — per-proxy copies — as the denominator a *rate* needs). So it can
report a **back-pressure** drop rate (`dropped_overflow / fanned_out`) and the
fan-out multiplication (`fanned_out / accepted`). It cannot report that the
fan-out was *unwanted*: `CosEventComm` has no subscription predicate, so
nothing in the servant knows what a consumer wanted — and that knowledge is
exactly what this chapter's filters would introduce.

The trigger is therefore two observations, one from each side: **F7 reports a
sustained non-zero `dropped_overflow / fanned_out`, and a named consumer
reports discarding on receipt a material share of what it is delivered.** Only
the second half is the thing server-side filtering fixes, and it can only be
counted where the discarding happens.

두 번째, 더 약한 방아쇠, **2026-08-20 계측기를 만들며 다시 씀**: 예전 문장은
**순환**이었고, 계측기를 지어 보고서야 드러났다. F7은 이제 드롭을 원인별로
나누므로 **배압** 드롭률과 팬아웃 배수는 보고할 수 있지만, 팬아웃이 *원치 않은*
것이었는지는 보고할 수 없다 — `CosEventComm`에 구독 술어가 없어 소비자가 무엇을
원했는지 아는 곳이 서번트에 없고, 그 지식이야말로 이 장의 필터가 도입할 것이다.
따라서 방아쇠는 양쪽에서 하나씩 두 관찰이다: **F7이 0이 아닌 배압 드롭률을 지속
보고하고, 동시에 이름 있는 소비자가 수신 후 폐기 비율을 보고할 것.**

**Relation to F7 — superset, so F7's channel becomes its transport core.**
This is the load-bearing sentence for a future batch: Notification is not a
replacement for F7's channel, it is a layer over it, and F7's push-model
`EventChannel`, its bounded buffer, its drop-oldest policy and its
counted-and-reported drops all survive unchanged as the delivery engine.

**v1 sketch.** Keep F7's channel; add exactly three things and refuse the rest
loudly. (a) `StructuredEvent` as a typed shape converted to and from the
existing `any` — AnyJSON already carries the payload, so this is a mapping, not
a new marshalling path. (b) A `Filter` that is the **existing
`orbweaver-trading` constraint evaluator** pointed at `filterable_data` instead
of at offer properties: one evaluator with two callers, following F3's
precedent of reusing `Residency`/`policy::Decision` rather than defining them a
second time, and inheriting S4-style positioned constraint errors for free.
(c) Exactly two QoS knobs — `DiscardPolicy` and `MaxEventsPerConsumer` — chosen
because F7 must implement both anyway, so v1 QoS is *naming what already
exists* rather than new machinery. Event type repository, mapping filters,
sequence/batch proxies, typed↔structured conversion, per-message QoS override:
all `BAD_OPERATION`, per the F6 refuse-loudly rule. Oracle: probe first, in the
sslTP/omnievents tradition — whether omniORBpy ships `CosNotification` stubs is
**unverified and must be measured before the batch is planned**; `brew info
omnievents` already returned "No available formula" (PLAN-SERVICES §4), so a
BLOCKED probe is a plausible and valid result, in which case the oracle is
two of our own processes plus a hand-written independent consumer.

**요지.** Notification의 무게중심은 **서버측 필터링**이고, 오늘의 필터링 지점은
MCP 경계의 guard 체인 하나다 — 정책을 공유하지 않는 두 번째 필터는 불일치의
시작이다. 방아쇠는 필터링이 대역폭 문제가 아니라 **격리** 문제가 되는 순간이다:
테넌트 A의 이벤트가 테넌트 B 프로세스에 도착한 뒤 버려진다면 그것은 필터링이
아니라 유출이다. F7과의 관계는 **상위집합** — F7 채널이 그대로 전송 코어가 되고,
v1은 셋만 얹는다: `StructuredEvent` 매핑, **`orbweaver-trading`의 제약 평가기를
`filterable_data`에 재사용**하는 필터, 그리고 F7이 어차피 구현해야 하는 두 QoS
(`DiscardPolicy`·`MaxEventsPerConsumer`). 나머지는 시끄럽게 거부. 오라클은 먼저
프로브 — omniORBpy의 `CosNotification` 스텁 유무는 **미측정**이다.

---

## 2. Transaction / OTS — why it is a graveyard / 트랜잭션

**What it is.** `CosTransactions` — `Current` (begin/commit/rollback on the
thread), `Control`/`Coordinator`/`Terminator`, `Resource` and
`RecoveryCoordinator` for two-phase commit, `TransactionalObject` as a marker,
and a transaction context propagated in a service context on every request,
with `OTSPolicy`/`InvocationPolicy` deciding who must carry it.

**Why it is a graveyard**, in the order the failures actually bite:

1. **2PC is a durability protocol, and we deliberately have no durable store.**
   D003 Part B deferred storage because nothing measured today needs it. A
   coordinator without a presumed-abort log that survives its own crash is not
   implementing 2PC; it is demonstrating the happy path. Building OTS would
   therefore silently re-open a decision D003 closed with a reason.
2. **Heuristic outcomes are unresolvable by construction.** `HeuristicMixed`
   and `HeuristicHazard` exist precisely because the protocol admits states no
   protocol step can repair; the standard's answer is to report them to an
   operator. A control plane whose entire discipline is "every failure has a
   named cause and a codified fix" would be adopting a surface whose defined
   behaviour is to hand a human an unnamed one.
3. **No oracle.** Every interop claim in this project is "our X against their
   X, both directions". omniORB ships no OTS; whether JacORB's historical OTS
   is usable as a fixture is **unverified** (not probed — and probing belongs
   to the batch, not to this sketch). OTS would be the only major surface with
   no reference peer, which is exactly the position D002 refused to take for
   crypto, for the same reason: we could not claim it was verified.
4. **It holds locks across the network for the duration.** The control plane's
   latency budget is the data plane's; a coordinator that blocks participants
   through prepare is the shape PLAN-MOE's §11 prefetch discipline exists to
   avoid.

**Trigger.** A multi-object state change that must be all-or-nothing across a
**failure** (not merely across a bug), *and* where compensation is
unacceptable because some observer could act irreversibly on the intermediate
state. Both halves are required — the second is what compensation cannot buy.

**The narrow alternative, argued: idempotency keys plus compensation at the
MCP layer.** The operations that would want atomicity here are control-plane
compositions — `ModelFactory.deploy` → `register_expert` → `bind`, or `retire`
→ unbind → evict. Four facts make a saga the better fit than 2PC. They are
**few** (a handful of compositions, not an open set). They are **already
funnelled through one chokepoint**, the guard/interceptor chain, so there is
exactly one place to record intent. They **already have a destructive-approval
gate**, so the expensive human step is spent once, up front, rather than on a
heuristic outcome afterwards. And each step is **individually idempotent or
trivially made so** — `bind`/`unbind` and `deploy`/`retire` are already named
inverses in the contracts we have. So: a caller-supplied idempotency key on
every mutating MCP tool call; `(key, request-hash, outcome)` recorded in the
audit ledger; a repeat replays the recorded outcome instead of re-executing; a
failed composition runs the recorded inverses in reverse order. What this buys
over 2PC: no locks held across the wire, no heuristic states, and failure is
**visible in the audit ledger** — a first-party artifact the harness already
reads and pins by string equality — rather than in a coordinator's private
log. What it does **not** buy, said plainly rather than glossed: **isolation.**
A reader between step 2 and step 3 sees a half-composed model. The honest
mitigation is a design constraint, not an atomicity claim: a composed model is
not published into Naming or Trading until its last step succeeds, so the
intermediate state is unreachable by anyone who did not initiate it. That is
enforceable and testable; "atomic" would not have been.

**v1 sketch.** `IdempotencyKey` on the MCP tool envelope, validated like any
other argument and refused if malformed. A `Saga` recorder in the interceptor
chain (the same chain D004 targets) holding steps and their named inverses. No
new wire surface, no `Current`, no service context, no dependency. Oracle — and
it is a good one, which is the point: a fault-injection harness that kills the
process after each step *k* of an *n*-step composition and asserts the
externally observable state equals either the before-state or the after-state,
for every *k*. That is *n*+1 deterministic cases per composition; batch unit is
one composition, whole set at once.

**요지.** OTS가 묘지인 이유는 넷: (1) 2PC는 내구성 프로토콜인데 **D003이 영속
저장소를 의도적으로 유예**했다 — 로그 없는 코디네이터는 해피패스 시연이다,
(2) `HeuristicMixed`/`HeuristicHazard`는 **구조적으로 해결 불가**한 상태이며
표준의 답은 "사람에게 알려라"다 — 모든 실패에 이름과 성문화된 수정을 요구하는
규율과 정면으로 충돌한다, (3) **오라클이 없다** — OTS 피어가 없다(JacORB OTS
가용성은 **미측정**), (4) prepare 구간 내내 네트워크 락을 쥔다. 좁은 대안은
**멱등키 + 보상(사가)을 MCP 계층에서**: 대상 조합은 소수이고, 이미 인터셉터
체인이라는 단일 관문을 지나며, 파괴적 승인 게이트가 이미 있고, 각 단계에 이미
이름 붙은 역연산이 있다. 2PC 대비 얻는 것 — 네트워크 락 없음, 휴리스틱 상태 없음,
실패가 **감사 원장에 보인다**. 얻지 못하는 것 — **격리**. 완화책은 원자성 주장이
아니라 설계 제약이다: 마지막 단계 전에는 Naming/Trading에 공개하지 않는다.

---

## 3. Time Service — trivial to serve, and that is the trap / 시간 서비스

**What it is.** `CosTime` — `TimeService::universal_time`,
`secure_universal_time`, `new_universal_time`; a `UTO` (Universal Time Object)
carrying time, **inaccuracy** and time-displacement factor; a `TIO` (Time
Interval Object) with `overlaps`/`spans`; and `CosTimerEvent`
(`TimerEventService`, `TimerEventHandler`) which fires an event at a time.

**Why deferred.** It is genuinely trivial to serve — `universal_time()` is a
clock read converted to 100-nanosecond units since the OMG epoch, an afternoon
of work — and that is exactly why the reason for declining it has to be better
than "no consumer". It is.

**The architectural argument.** Verified by reading the code:
`crates/orbweaver-object/src/residency.rs` and the whole of
`crates/orbweaver-trading` contain **no clock read at all** — no
`SystemTime::now`, no `Instant::now`. The residency machine's `apply()` takes a
slice of decisions plus a `BatchStats` **window**, and the source comment says
the type name is the reminder that a window is the unit; `BatchStats` is
derived per window from the offer store rather than cached, so the guard cannot
decide against a stale reading. This is not an oversight to be corrected by a
time service; it is the property that makes the trading engine's deterministic
trace replay possible, and therefore the property the oracle stands on. **The
moment any policy can call `universal_time()`, a trace stops replaying and a
deterministic oracle becomes a flaky one.** A service that is trivial to build
and whose mere availability erodes the harness's foundation is a bad trade at
any price. Honest absence, with a reason stronger than absence of demand.

**Trigger.** A **peer** requires it: a legacy client in a pilot resolves
`TimeService` from our Naming server and we must answer. That is the only
clean trigger. A `CosTimerEvent`-driven consumer of ours would also do it, but
that is a smell to be argued with first — every timer in a control plane of
this shape should be a window boundary.

**v1 sketch.** `universal_time` and `new_universal_time` only.
`secure_universal_time` **refused loudly** — we can make no security claim
about our clock, and the CORBA-shaped honest answer is `TimeUnavailable`
rather than returning the ordinary time under a name that promises more.
Inaccuracy reported honestly rather than fabricated as zero: a UTO claiming
zero inaccuracy is precisely the decorative dishonesty this project rejects, so
v1 reports the platform's coarsest defensible bound and says so in the
contract. `TIO`, `CosTimerEvent` and time-displacement handling: out. And one
codified guard, because the danger here is not the service but its
availability — **the crate that serves time is one nothing in the control plane
may depend on**, enforced by the dependency direction the way F3 enforced
`orbweaver-trading` depending on nothing of ours, not by a comment asking
nicely.

**요지.** 만들기는 정말 쉽다 — 그래서 거절 사유가 "소비자 없음"보다 나아야 한다.
실측: `residency.rs`와 `orbweaver-trading` 전체에 **시계 읽기가 하나도 없다**.
`apply()`는 결정 슬라이스와 `BatchStats` **윈도**를 받고, 윈도가 단위라는 것이
타입 이름의 요지다. 이 무시계 설계가 결정적 트레이스 재현을 가능하게 하고, 그
재현이 오라클의 토대다. **어떤 정책이든 `universal_time()`을 호출할 수 있게 되는
순간 트레이스는 재현되지 않는다.** 방아쇠는 **피어**가 우리 Naming에서
`TimeService`를 resolve하는 경우뿐. v1은 `universal_time`/`new_universal_time`만,
`secure_universal_time`은 시끄럽게 거부(시계에 대해 보안 주장을 할 수 없다),
부정확도는 0으로 위조하지 않고 정직하게 보고, 그리고 **시간 크레이트에는 컨트롤
플레인이 의존하지 못하게** 의존 방향으로 강제한다.

---

## 4. PSS / Persistent State — the CORBA-shaped wrong answer / 영속 상태

**What it is.** The CORBA 3 Persistent State Service: **PSDL**, a separate
language (`storagetype`, `storagehome`, abstract storage types, key
declarations) with its own compiler; storage objects addressed by pid; a
connector/session/transaction model; and a mapping from PSDL onto a datastore.
It is CORBA's answer to "how does a servant's state outlive the process".

**Why deferred, and why it is strictly worse than the pre-cleared path.**
D003 Part B already deferred durable storage — nothing measured today is
blocked by the absence of a database — and pre-cleared **PostgreSQL + pgvector
as a separate-process fixture** with `tokio-postgres` + the `pgvector` crate
as the only Cargo additions, licences verified from shipped tarballs. PSS is
the CORBA-shaped alternative to that path and it loses on four counts:

1. **PSS is a second language.** PSDL has its own front end, its own type
   system and its own mapping rules. `orbweaver-idl` — one front end, in full
   oracle agreement — was the centrepiece of an eleven-week phase (PLAN §7.2,
   Phase 2); PSS proposes doing that again to reach a store we can already
   reach with a socket.
2. **PSS has no oracle.** There is no PSS peer among our fixtures (whether
   omniORB implements PSS at all is **unverified**, and the honest prior is
   that PSS is in OTS's implementation graveyard). The pgvector path's oracle
   is trivial by comparison: `CREATE EXTENSION vector;` succeeds and
   `SELECT '[1,2,3]'::vector;` round-trips — D003 wrote the probe already.
3. **PSS solves the wrong half.** D003's actual requirement is a *catalog*:
   contract metadata plus vectors for semantic search, judged by a frozen
   query benchmark. That is a query problem. PSS is an object-persistence
   mapping with no query story worth the name — pgvector exists precisely
   because the query is a vector query, and PSS has nothing to say about it.
4. **PSS entangles persistence with the object model.** `pipeline::register` is
   deliberately outside the POA as the durable store's seam. PSS puts storage
   *under* the servant, which would turn the residency machine's `PERSISTENT`
   lifespan blob — today an opaque `Vec<u8>` preserved across evict/reload,
   with the opacity being the entire content of the TRANSIENT/PERSISTENT
   distinction — into a storage object, and drag the POA into a storage
   decision it currently does not participate in.

**Trigger.** A pilot peer that **is already a PSS client** and expects our
objects to be storage-homed — the only scenario in which the CORBA shape is
the requirement rather than an aesthetic. Whether that has ever happened to
anyone is **unverified**; no survey was done, and this sketch does not pretend
one was.

**v1 sketch.** If triggered: **do not implement PSDL.** Implement the
observable half only — servants whose state loads and stores through the
existing `ExpertLoader` blob seam, backed by D003's pre-cleared store, with
`_get_pid`-shaped identity if a client demands it. PSS-compatible enough for a
client that only calls the storage-object interface; no PSDL compiler, ever.
Everything else refused loudly, and the refusal is easy to defend because the
alternative is a second language front end.

**요지.** D003 Part B가 이미 영속 저장소를 유예하고 **PostgreSQL + pgvector**
경로를 사전 정리했다. PSS는 그 자리의 CORBA식 대안이며 네 가지로 진다: (1)
**PSDL은 별도 언어** — 프론트엔드를 한 번 더 만들어야 한다, (2) **오라클이 없다**
(pgvector 쪽 오라클은 `CREATE EXTENSION vector` 한 줄), (3) **문제의 반대쪽을
푼다** — D003의 요구는 질의(벡터 검색)인데 PSS에는 질의 이야기가 없다, (4)
영속성을 객체 모델에 **얽는다** — `pipeline::register`는 일부러 POA 밖의
이음매이고, 잔류 머신의 `PERSISTENT` 블롭은 불투명한 채로 있어야 그 구분에 내용이
있다. 방아쇠는 **이미 PSS 클라이언트인 피어**뿐. v1은 PSDL을 만들지 않는다 —
`ExpertLoader` 블롭 이음매 위의 관측 가능한 절반만.

---

## 5. Concurrency Control / 동시성 제어

**What it is.** `CosConcurrencyControl` — `LockSetFactory`, `LockSet`, five
lock modes (intention-read, read, upgrade, intention-write, write) with a
compatibility matrix, and `TransactionalLockSet` whose locks are released by a
transaction coordinator.

**Why deferred.** Three reasons, and the third is the one that matters.
First, the interesting half is `TransactionalLockSet`, which needs the
coordinator §2 declines to build — Concurrency Control is OTS's companion and
inherits its deferral. Second, the non-transactional half is a distributed
mutex service, and the spec has **no lease**: a lock held by a process that
dies is a lock held forever. Third and decisively, **our actual concurrency
requirement is already met and is narrower than the service**: the residency
machine's inflight counter is the guard against evicting a busy expert
(`EVICT` on `ACTIVE` is a named guard refusal, `NoInflight`, not a missing
edge), and it is in-process state on a single POA — deterministic, testable,
incapable of network failure. Adopting a lock service would replace a counter
that cannot fail with a protocol that can.

**Trigger.** Two ORB processes serving the same expert id — precisely: **the
first configuration with two `ExpertLoader`s over an overlapping id space.**
That is an observable event (a deployment topology, not a feeling), and it is
the moment residency stops being one POA's private business. F5 tenancy or a
multi-instance deployment batch are the plausible ways it arrives.

**v1 sketch.** Not `CosConcurrencyControl`. A lease:
`residency::Lease { id, holder, expires_at, fence_token }`, served by whichever
process owns the id space, with the **fence token checked at the point of
use** so a stale holder's write is *refused* rather than merely late — the
standard fix for the standard failure the OMG service does not address. Lock
modes collapse to one (exclusive residency ownership), because the five-mode
matrix exists for transactional data and we are arbitrating a single
boolean per id. The standard facade only if a foreign client names it, per the
IFR-facade rule (PLAN-SERVICES §7). One honest complication recorded now so it
is not discovered later: **this is the only deferred chapter whose v1 needs a
clock**, since a lease is an expiry. It must therefore argue with §3's
no-clock discipline rather than inherit it — the likely resolution is that
lease expiry is a *window count*, not a wall-clock instant, which keeps the
trace replayable, but that is a claim for the batch to prove, not one to
assume here.

**요지.** 흥미로운 절반(`TransactionalLockSet`)은 §2가 거절한 코디네이터를
필요로 하고, 나머지 절반은 **리스가 없는** 분산 뮤텍스다 — 죽은 프로세스의 락은
영원한 락이다. 결정적인 이유는 셋째: 우리의 실제 요구는 이미 충족되어 있고 더
좁다 — 바쁜 expert의 축출을 막는 것은 단일 POA의 인프로세스 inflight 카운터이며,
`ACTIVE`에서의 `EVICT`는 `NoInflight`라는 **이름 붙은 가드 거부**다. 실패할 수
없는 카운터를 실패할 수 있는 프로토콜로 바꾸는 거래다. 방아쇠는 **겹치는 id
공간에 `ExpertLoader`가 둘인 첫 구성**. v1은 락셋이 아니라 **펜스 토큰이 있는
리스** — 사용 시점에 토큰을 검사해 낡은 보유자의 쓰기를 늦게가 아니라 **거부**
한다. 정직한 단서: 이 장의 v1만 시계를 필요로 하므로 §3의 무시계 규율과
논쟁해야 한다(유력한 해법은 만료를 **윈도 수**로 두는 것 — 배치가 증명할 주장).

---

## 6. CosCollections / 컬렉션

**What it is.** `CosCollection` — a large IDL library of collection interfaces
(`KeySet`, `Map`, `Bag`, `Sequence`, `PriorityQueue`, and a taxonomy of
restricted variants), each with iterator objects, comparator/operations
objects, and a factory hierarchy.

**Why deferred.** The clearest "no consumer" case in the suite, and also the
clearest case of a service that language runtimes made obsolete. Everything we
need crosses the wire as an IDL `sequence<T>` or a `struct` and lands as a Rust
`Vec`/`BTreeMap`; what CosCollections adds is **remote** collections with
**remote iterators**, and a remote iterator is a round-trip per element — the
chatty shape the latency discipline throughout PLAN and PLAN-MOE rejects. Worth
recording that the project already met this exact trade-off and chose the same
way: F6's `list` returns a nil `BindingIterator` with the under-reporting
documented, rather than serving iterator lifecycles no caller has.

**Trigger.** A foreign client that expects `CosCollection` interfaces by name —
no other trigger is credible for the standard itself. A second, weaker trigger
argues for the *shape* rather than the standard: a result set that genuinely
cannot be bounded, where paging becomes necessary and a cursor becomes the
honest answer. Today's largest set is the golden corpus — **44 distinct
interface names across its 36 files** (measured 2026-08-25 by name, not by
declaration), still trivially bounded. It read "roughly thirty" while the
number was growing under it, which changes nothing about the verdict and is
exactly why the measurement now carries its date.

**v1 sketch.** Not `CosCollection`. One first-party paging shape, reused
everywhere rather than invented per call site: `(items, next_cursor)` where the
cursor is an **opaque, session-scoped, expiring, entropy-backed** string in the
`CapabilityTable` mold — the same properties, for the same reason, so a cursor
cannot be replayed across sessions any more than a handle can, and the existing
transcript-leak test shape applies unchanged. One shape serving the catalog
listing, Naming's `list`, and Trading's query. The standard facade only on
demand, read-only, per the IFR-facade rule.

**요지.** 스위트에서 가장 명확한 "소비자 없음" 사례이자, 언어 런타임이 낡게 만든
서비스다. 우리가 쓰는 컬렉션은 전부 `sequence<T>`/`struct`로 건너와 `Vec`/
`BTreeMap`이 된다. CosCollections가 더하는 것은 **원격 컬렉션과 원격 반복자**이며,
원격 반복자는 원소당 왕복 — PLAN 전체의 지연 규율이 거부하는 형태다. 프로젝트는
이 거래를 이미 한 번 만났고 같은 선택을 했다(F6의 `list`는 nil `BindingIterator`
와 과소보고 문서화). 방아쇠는 **이름으로 `CosCollection`을 기대하는 외부
클라이언트**, 혹은 경계 지을 수 없는 결과 집합. v1은 표준이 아니라 **하나의
페이징 형태** — `(items, next_cursor)`, 커서는 능력 핸들과 같은 성질(세션 종속·
만료·엔트로피)의 불투명 문자열.

---

## 7. Federated Naming / Trading Links / 연합 네이밍·트레이딩 링크

**What it is.** Naming federation is binding, inside our naming graph, a
context served by *another* ORB, so a `resolve` crosses domains. The mechanism
is already latent: a `NamingContext` binding holds an IOR, and an IOR may name
anything — F6's server already serves nested contexts as distinct object keys.
What is absent is the *policy*: loop protection, trust, and what a cross-domain
failure means. Trading links are the explicit version — `CosTrading::Link`,
`LinkAttributes`, `follow_rule`/`default_follow_rule` and a hop-count policy,
so a `query` with no local match propagates to linked traders.

**Why deferred.** PLAN-SERVICES §2 records federation as "not doing (until a
consumer appears)" and §8 says tenancy (F5) may name the requirement. **F5
landed on 2026-08-14 and evaluated this trigger in code** — `tenant_service.rs`,
"one graph, per-tenant keys … this is the other shape, so the trigger has not
fired" — the one place the tree answered a question this file asks; it is
cited here so the answer has a home a reader can find. It has
not.

**Trigger — and the precision is the point of this chapter.** F5 tenancy is
the trigger **only in one of its two possible shapes**, and a future batch that
does not notice which shape it is in will build federation it does not need or
skip federation it does.

- If tenancy is realized as **one naming graph with per-tenant `Exposure`** —
  the shape the existing default-deny exposure model and per-session capability
  tables already imply — then federation is **not** required. Isolation is an
  authorization property, and adding a second isolation mechanism repeats §1's
  two-filters mistake.
- If tenancy is realized as **one naming/trading domain per tenant, in separate
  processes** — which becomes the shape the moment a tenant requires that their
  catalog not be co-resident with another's, or a deployment places tenants in
  separate pods (PLAN's R7 IOR-rewriting territory) — then federation **is**
  required, because one MCP face must resolve across domains it does not serve.

**The observable trigger is therefore: the first configuration with more than
one naming/trading domain behind one MCP face.** For Trading specifically it is
that same event *plus* a query that must return offers from a domain the local
trader does not hold — which is a distinct event, because separate domains do
not by themselves imply cross-domain selection.

**v1 sketch.** *Naming*: a `foreign` binding kind that is an ordinary
`NamingContext` IOR plus a recorded domain label; `resolve`/`resolve_str`
crosses **at most one hop** and refuses deeper with a named error. Loop
protection by **hop budget, not cycle detection** — a budget is testable in one
case and cycle detection over graphs we do not serve is not testable at all.
The crossing is a `Guarded` operation like any other, so a cross-domain resolve
produces an authorization decision and an audit line rather than a transparent
redirect; that single choice is what keeps federation from becoming a hole in
the tenancy model it was adopted to serve. *Trading*: `Link` with `follow_rule`
restricted to `local_only` and `if_no_local`, hop budget of one, and a merged
result set that **labels every offer with its origin domain**, so the §6
loading policy can never mistake a foreign offer for a local one. Oracle: the
F6 both-directions rule, which federation satisfies naturally — two of our own
servers in separate processes for one direction, and **omniNames as the second
domain** for the other, so the cross-domain claim is measured against an
independent ORB rather than against ourselves.

**요지.** 메커니즘은 이미 잠재해 있다 — 바인딩은 IOR을 담고 IOR은 어디든 가리킬
수 있으며 F6은 중첩 컨텍스트를 별개 객체 키로 서빙한다. 없는 것은 **정책**(루프
보호·신뢰·실패 의미)이다. 방아쇠의 정밀함이 이 장의 요점: F5 테넌시는 **두 가지
형태 중 하나일 때만** 방아쇠다. *하나의 네이밍 그래프 + 테넌트별 `Exposure`*면
연합은 불필요하고(격리는 인가 속성이며, 두 번째 격리 기구는 §1의 실수 반복),
*테넌트별 별도 도메인·별도 프로세스*면 필요하다. 관측 가능한 방아쇠 =
**하나의 MCP 얼굴 뒤에 네이밍/트레이딩 도메인이 둘 이상인 첫 구성**. v1: 홉 예산
1(순환 탐지가 아니라 **예산** — 예산은 시험 가능하고 남의 그래프 순환 탐지는
불가능), 도메인 경계 통과는 감사 라인이 남는 `Guarded` 연산, 트레이딩 결과는
**원산 도메인 라벨**을 달아 적재 정책이 외부 오퍼를 로컬로 오인할 수 없게 한다.
오라클은 두 번째 도메인을 **omniNames**로 두어 양방향을 만족시킨다.

---

## 8. Security Service beyond CSIv2 / CSIv2 너머의 보안 서비스

**What PHASE5 has today** (measured; the CSIv2-advertisement half of this
paragraph is owned by the harness's identity group in `spikes/run_checks.sh`,
which goes FAIL the day a peer advertises and nothing measures it — the
citation said `COMPONENTS.md` until 2026-08-25, by which time that document
held no such sentence, the fact having moved to the gate that can check it):
CSIv2 wire — SAS
service context, GSSUP, mechanism lists — unit-tested in both byte orders;
delegation policy, default-deny with recorded reasons; structurally enforced
credential hygiene (`audit_line` takes a `&Caller` and an `&Assertion`, so
there is no argument that *could* carry a password); `@ai_authz` scopes
enforced at the MCP boundary; the `Caller` seam. Plus D002's approved SSLIOP
path for transport identity. And the honest measurement: **neither fixture
advertises CSIv2 at all.**

**What the full OMG Security Service adds on top.** Naming it is most of this
chapter's value, since "Security Service" is otherwise a phrase that could mean
anything:

| Part | What it adds beyond PHASE5 |
|---|---|
| `SecurityLevel1/2` `Current` | application code can *ask* about the caller's privilege attributes mid-call, instead of being gated before the call |
| `PrincipalAuthenticator`, `Credentials` | a credential **lifecycle** — acquisition, refresh, `set_privileges`. We deliberately have a seam that carries an identity, not a store that holds material |
| POA security policies (`AccessPolicy`, `SecInvocationCredentialsPolicy`, `QOPPolicy`) | required quality-of-protection and access rules **per object**, rather than per connection |
| **Domain managers** | objects belong to security policy domains and inherit policy by membership — genuinely the piece F5 tenancy will want a shape for |
| `AuditDecision` / `AuditChannel` | audit as **objects** with policy expressed as a selector, rather than as our code path |
| Non-repudiation (`NRService`) | evidence generation and verification — effectively a signing service |
| Delegation modes | a spectrum (none / simple / composite / traced); we implement default-deny plus a recorded reason |

**Why deferred.** Three reasons in decreasing obviousness. **No peer** —
PHASE5 measured that neither fixture advertises even CSIv2, so the layer above
it has less chance still of a reference implementation to oracle against; we
would be the only speaker of a protocol nobody answers, which is the position
D002 refused for crypto and for the same stated reason. **We already have a
policy framework** — `Exposure` + `Delegation` + `ai_authz` + the guard chain
occupy the ground CORBASec Level 2 occupies, in a shape our oracles check, and
two policy frameworks are worse than one less-standard one (the §1 argument
again, in its most consequential form). And **CORBASec's own history**: the
most-specified, least-implemented part of CORBA, whose shipped subset was
mostly the part that maps to transport security — which D002 already covers.

**Triggers, split because the parts do not arrive together.**

1. **Domain managers** — F5 tenancy needing policy inherited by object
   *membership* rather than evaluated per call. Observable when the per-call
   `Exposure` check must be identical across a set of objects whose membership
   changes at runtime. This is the earliest of the four.
2. **`Current::get_attributes`** — the first generated servant that must branch
   on caller attributes *inside* application code rather than being gated
   before entry.
3. **Non-repudiation** — a pilot requiring that a destructive approval be
   provable to a third party later. Today's audit line is a string in a ledger
   we control, which is exactly what a third party cannot rely on.
4. **The rest** — a named foreign client that speaks it.

**v1 sketch.** Not CORBASec. Each trigger gets the narrowest first-party answer
in the vocabulary that already exists. (1) A `PolicyDomain` that is a **set
label** on registry entries, with the `Exposure` decision resolved and cached
per domain — the mechanism exists, the *noun* is what is missing. **The noun is
no longer free** (found 2026-08-25): `moe::enterprise::PolicyDomain` has existed
since F5 tenancy (`IDL:moe/enterprise/PolicyDomain:1.0`, `tenant_service.rs`)
meaning a *residency and placement* domain — unrelated to a CORBASec security
policy domain, and served 3 of 3 over the wire. No trigger has fired, so §9
imposes no obligation to correct the sketch; recorded here so that the day it is
used, the collision is a known cost and not a surprise. (2) `Caller`
gains a read-only attribute map, populated from the same token exchange stream
C is already building for OAuth2/JWT, handed to servants as a borrow and never
as a mutable store, so the hygiene property stays structural. (3) Audit lines
get an **append-only hash chain** — each line carries the hash of the previous
— which makes tampering detectable with **no key and no dependency**; only if a
third party must verify independently does a detached signature over the chain
head follow, using D002's already-cleared crypto. That order is the whole
design: the chain is free and solves detection, the signature costs key
management and solves only attribution, so shipping them together would pay for
the expensive half before knowing it is needed. (4) A read-only
`SecurityLevel1` facade on demand, per the IFR-facade rule.

**요지.** PHASE5가 가진 것: CSIv2 와이어(양 바이트 순서 시험), 기본 거부 위임,
구조적 자격증명 위생, `ai_authz` 스코프, `Caller` 이음매, D002의 SSLIOP 경로 —
그리고 정직한 실측: **두 픽스처 모두 CSIv2를 광고하지 않는다.** 전체 보안
서비스가 더하는 것은 `Current::get_attributes`, 자격증명 생애주기, POA별 보안
정책, **도메인 매니저**, 객체로서의 감사, **부인방지**, 위임 모드 스펙트럼이다.
유예 사유 셋: **피어가 없다**(CSIv2조차 답하지 않는데 그 위층은 더더욱 — D002가
암호에 대해 거부한 것과 같은 위치), **정책 프레임워크가 이미 있다**(두 개는 하나
보다 나쁘다), **CORBASec의 역사**(가장 많이 명세되고 가장 적게 구현된 부분).
방아쇠는 넷으로 갈라지며 가장 이른 것은 F5의 **도메인 멤버십 기반 정책**이다.
v1은 CORBASec이 아니라 기존 어휘의 최소 답: (1) 레지스트리 항목의 **집합 라벨**
로서의 `PolicyDomain`, (2) 스트림 C의 토큰 교환이 채우는 `Caller`의 읽기 전용
속성 맵, (3) 감사 라인의 **추가 전용 해시 체인**(키도 의존성도 없이 변조 탐지) —
제3자 검증이 필요할 때에만 체인 머리에 분리 서명. 이 **순서**가 설계의 전부다:
체인은 공짜로 탐지를 풀고 서명은 키 관리를 대가로 귀속만 푼다.

---

## 10. CosEvent — the supplier side of pull / 이벤트 — pull의 공급자 쪽

> **GRADUATED 2026-08-25 → `PLAN-SERVICES.md` §4.1.** The first chapter to
> leave this file under §9. What stays here is what §9 says stays: the reason
> it was deferred, and **the v1 sketch, corrected**, so the disagreement the
> trigger produced is recorded rather than quietly lost. Everything
> forward-looking — batch unit, named oracle — is in `PLAN-SERVICES` §4.1,
> because §0 is explicit that the move *is* the adoption and this file is not.
>
> *졸업 2026-08-25 → `PLAN-SERVICES.md` §4.1. §9 아래 이 파일을 떠난 첫 장이다.
> 여기 남는 것은 §9가 남으라고 한 것 — 유예 사유와, **정정된 v1 스케치**다.*

**What it is.** `SupplierAdmin::obtain_pull_consumer` and the
`ProxyPullConsumer` it would return: the channel *pulls* from a supplier —
`PullSupplier::pull` is specified to block until the supplier has something.
The consumer side of pull (`obtain_pull_supplier`, `pull`, `try_pull`) is
**served** since 2026-08-18: it holds events in the same bounded deque the push
path uses, moved by the same knob, dropped oldest-first into the same counter
(`event_server.rs`, `spikes/service_sweep.sh`).

**Why deferred, re-measured 2026-08-18.** The original reason was two claims —
"the same unbounded buffer this module avoids, for no named consumer" — and
only the second survived measurement: the consumer half needed no new buffer.
What holds for the supplier half is different: there the *channel* is the
puller and would hold a thread per connected supplier on somebody else's clock,
with no bound it owns — for no named supplier, since nothing in this workspace
is a `PullSupplier` (grep, 2026-08-19: only the servant's own proxy and the
sweep's note that the interface is client-implemented). Answered
`NO_IMPLEMENT` with this reason; measured in the generated coverage block.

**Un-defer trigger — fired 2026-08-25.** A named `PullSupplier` in this
workspace. It fired on the project owner's request that the four models be
creatable, under D023 §2's proposed rule that the owner naming a consumer fires
a trigger; D021 §2 carries the argument, which is that a deferral phrased *"until
something asks"* is called the moment something asks, and that the opposite
reading makes this particular trigger **unreachable by construction** — nobody
writes a pull supplier against a channel that cannot obtain a pull consumer. It
is now satisfied literally too: `event_server::PullSupplierServant` is one.

**v1 sketch — wrong, and recorded as wrong.** It read *"one thread per
connected supplier with a per-supplier deadline the channel owns."* What landed
is **one** round-robin thread calling `try_pull`, never the blocking `pull`.
The sketch's whole design followed from assuming the blocking call: with a
non-blocking one there is nothing for the extra threads to wait on, and a fixed
thread count is a bound **the channel owns** where a per-connection count is one
a *client* sets — which was the sketch's own stated objection, arriving through
the mechanism it proposed. The deadline it wanted turned out to be the existing
`DEFAULT_PUSH_TIMEOUT`. What it did not foresee is the cost that replaced it: an
interval the channel has to invent (`DEFAULT_SOURCE_POLL`, 100 ms), which is a
real price and is named rather than hidden. `MAX_CONSECUTIVE_FAILURES` and
`disconnected_for_failure` carried over as the sketch expected; failures land in
a new `pull_failures`, and **no drop cause joined the split**, because a
`ProxyPullConsumer` holds no queue.

**무엇.** 채널이 공급자에서 *당기는* 쪽. 소비자 쪽 pull은 2026-08-18부터 서빙.
**왜 유예.** 원래 사유는 두 주장이었고 하나만 측정을 견뎠다; 공급자 쪽은 채널이
남의 시계에 스레드를 하나씩 붙잡는 일이며 이 작업공간에 `PullSupplier`인 것이
없었다.

**방아쇠 — 2026-08-25 발화.** 네 모델을 생성 가능하게 하라는 소유자의 요청으로
당겨졌다(D023 §2). 논거는 D021 §2에 있다: "무언가 물을 때까지"라는 유예는 물은
순간 판돈이 불린 것이며, 반대로 읽으면 이 방아쇠는 **구조적으로 도달 불가능**
해진다 — pull consumer를 얻을 수 없는 채널을 상대로 pull supplier를 쓰는 사람은
없기 때문이다. 이제 문자 그대로도 충족된다: `PullSupplierServant`가 그것이다.

**v1 스케치 — 틀렸고, 틀린 것으로 기록한다.** "공급자마다 스레드 하나"가 아니라
`try_pull`을 도는 **하나의** 라운드로빈 스레드다. 스케치의 설계 전체가 *막는*
호출을 가정한 데서 나왔다. 막지 않는 호출에서는 여분의 스레드가 기다릴 것이
없고, 고정 스레드 수는 **채널이 소유하는** 한계인 반면 연결당 수는 *클라이언트*가
정하는 한계다 — 그것이 스케치 자신의 반대 이유였는데, 스케치가 제안한 기구를
통해 되돌아왔다. 원하던 데드라인은 이미 있던 `DEFAULT_PUSH_TIMEOUT`이었다.
예견하지 못한 것은 그것을 대신한 비용이다: 채널이 발명해야 하는 간격
(`DEFAULT_SOURCE_POLL`, 100 ms). 실제 대가이고, 숨기지 않고 이름한다.

## 11. CosEvent — `destroy` / 이벤트 — `destroy`

**What it is.** `EventChannel::destroy`: disconnect every proxy, invalidate
every object key other references still name, stop the relay.

**Why deferred, re-measured 2026-08-18.** The original reason — outbound
`disconnect_*` calls from inside a servant whose failures have nowhere to go —
is answered by `orbweaver_giop::guarded` since concurrent dispatch landed. What
remains is sharper: it is an **unauthenticated remote operation that ends the
channel for every other client**, and this servant has no notion of who is
calling (no `Caller`, no service context reaches it — `event_server.rs`,
2026-08-19). `ChannelHandle::stop` is the in-process path. Answered
`NO_IMPLEMENT` with this reason.

**Un-defer trigger.** A caller model in the event servant — the moment CSIv2
identity or the bridge's `Caller` reaches a servant (stream C, D010 B2), this
becomes an authorization decision like any other. **v1 sketch:** `destroy`
allowed to the principal that created the channel, `NO_PERMISSION` to others,
each disconnect through `guarded` with its failure counted.

**무엇.** 채널을 끝내는 원격 연산. **왜 유예.** 원래 사유는 `guarded`가 답했고,
남은 것은 더 날카롭다 — 누가 부르는지 모르는 서번트가 모두의 채널을 끝내는
인증 없는 연산. **방아쇠.** 이벤트 서번트에 호출자 모델이 닿는 순간.

## 12. CosNaming — chaining to a foreign context / 네이밍 — 외부 컨텍스트로의 연쇄

**What it is.** `bind_context` with a context served by *another* ORB, and a
`resolve` that follows it: the naming servant would make an outbound call.

**Why deferred, re-measured 2026-08-18.** `bind_context`, `rebind_context` and
`destroy` are **served** for contexts this server serves (the earlier
"contexts live as long as the process" was true only because nothing removed a
key; measured against omniNames). What is deferred is narrower: a foreign
context. It is *possible* today — that is a reason the work can be done, not a
reason to do it — and the servant deliberately holds no `Connection`, which is
now a test (`naming_no_outbound_call.rs`) rather than a sentence.

**Un-defer trigger.** §7's: more than one naming domain behind one MCP face —
seen from the servant. **v1 sketch:** §7's loop protection and hop count,
plus a per-context "foreign" mark in the catalogue so a `resolve` that will
leave the process is visible before it does.

**무엇.** 다른 ORB가 서빙하는 컨텍스트로의 연쇄. **왜 유예.** 자기 컨텍스트에
대한 세 연산은 서빙된다; 외부 컨텍스트만 남았고, 가능하다는 것은 지을 수 있다는
이유이지 지어야 할 이유가 아니다 — 서번트에 `Connection`이 없다는 것은 이제
테스트다. **방아쇠.** §7의 것.

## The eight that had no chapter — §13–§20 / 장이 없던 여덟

Added 2026-08-26. D023 §1 cross-checked all twenty-one CORBAservices against
the three plan documents and found a third row beside *served* and *deferred
with a reason*: eight services with **neither an implementation nor an
exclusion**. Re-measured the same day by grepping both plan documents for each
name — **zero hits for all eight**, and the two that look like hits are not:
`Query` appeared only as the trader's constraint query, D003's vector query and
§4's "no query story", and `Reference` only as an ordinary object reference.
This file's premise is that an exclusion carries a reason; these eight had no
reason because nobody had decided anything about them.

**They were absent together because five of them are one specification.**
`CosObjectIdentity`, `CosRelationships`, `CosContainment` and `CosReference`
are four modules of the *Relationship Service*, and `CosCompoundLifeCycle` is
the half of the *Life Cycle Service* built on top of it. That is the root cause
and it is worth naming: this was not eight oversights but one document nobody
opened, which is exactly the shape the batch discipline exists to surface. The
remaining three — `CosExternalization`, `CosQuery`, `CosLicensingManager` —
are independent and are absent for three unrelated reasons.

**Nothing here is owed an implementation.** D023 §6 is the fence and it is this
section's whole discipline: *no service in §1's third row becomes an
implementation because it appeared here.* Each gets a reason and a trigger, and
every v1 sketch below refuses the standard interface. D018 §4.1's sentence is
the reason the fence exists — *writing a thing down makes it feel owed* — and a
plan document is exactly how that discipline breaks.

*2026-08-26 추가. D023 §1이 찾은 세 번째 줄 — 구현도 제외도 없는 서비스 여덟.
같은 날 두 계획 문서를 이름별로 다시 grep 했고 **여덟 전부 0건**이었다. 걸린 것처럼
보이는 둘도 아니다: `Query`는 트레이더의 제약 질의·D003의 벡터 질의로만, `Reference`는
평범한 객체 참조로만 나온다. **여덟이 함께 없었던 것은 그중 다섯이 한 명세이기
때문이다** — `CosObjectIdentity`·`CosRelationships`·`CosContainment`·`CosReference`는
관계 서비스의 네 모듈이고, `CosCompoundLifeCycle`은 그 위에 지어진 생명주기 서비스의
절반이다. 누락 여덟 건이 아니라 아무도 열지 않은 문서 하나였다. 나머지 셋은 서로
무관한 이유로 없다. **여기 있는 어떤 것도 구현을 빚지지 않는다** — D023 §6이 울타리이고,
아래 모든 v1 스케치는 표준 인터페이스를 거부한다.*

---

## 13. CosRelationships — the relationships exist and the service does not / 관계 서비스

**What it is.** The Relationship Service's core: relationships and roles as
**first-class objects**, not as pointers held by the things they relate.
`RelationshipFactory::create` takes named roles and returns a `Relationship`
object; each `Role` object holds its `related_object` and answers
`get_other_related_object` / `get_other_role`, so **every relationship is
navigable from both ends**. `RoleFactory` declares `min_cardinality`,
`max_cardinality` and `related_object_types`, and `check_minimum_cardinality`
is the integrity operation; `link` / `unlink` are mutators on the *role* rather
than on either related object. Level two is `CosGraphs` — `Node`, `Traversal`,
`TraversalCriteria` — traversal over the graph the roles form.

**Why deferred, and the measurement is in the tree rather than in this file.**
`crates/orbweaver-object/src/tenant_service.rs` (module documentation, *The
three relationships a manifest holds*, landed 2026-08-25) writes the role,
cardinality and integrity rule for each of `Manifest::base_model` (N:1,
immutable), `::experts` (1:N, append-only) and `::policy_domain` (N:1,
replaceable), and opens by recording that the standard has a service for
exactly this and that the module implements none of it. Two of its measurements
are this chapter's reason, and they are cited rather than restated because that
module is their home — a rule whose home is somebody else's document drifts
from the code on the next change:

- **Dangling is impossible rather than detected.** Creation materialises its
  target, mutation requires an existing one, and no operation destroys a
  target, so the graph only grows. Referential integrity — the thing a `Role`
  object exists to enforce — has nothing to find here.
- **Nothing on the wire navigates any of the three.** The relationships have no
  inverse role and no navigation operation, which that module names as *the
  half `CosRelationship` would have carried*.

So the service's two contributions are, today, one unenforceable and one
unbuilt. Adopting it would mint an object with its own key and its own
lifecycle per link, to supply a back-pointer nobody follows — and it would move
the integrity rules out of the code that enforces them and into a service that
would have to be told about them, which is the direction that batch refused on
purpose.

**Trigger — two, both observable in this tree without anyone naming a
consumer.** (1) **The first operation that must answer an inverse question**:
given a base, which models are over it; given a `PolicyDomain`, which models it
governs; given an expert, which compositions bind it. Observable as a declared
operation in a contract. (2) **The first operation that destroys a target.**
The moment one exists, "the graph only grows" stops being true and the
integrity check that has nothing to find acquires something.

**v1 sketch.** Not `CosRelationships`. The inverse is an **index, not an
object**: the tenant service already holds every manifest, so the reverse
direction is *derived* from that one copy rather than stored a second time
where it can disagree with the first — one fact, one home. Roles stay implicit
and cardinality stays the rule the module already documents, that `bind_expert`
appends to a set with no maximum while `set_policy` replaces a member that is
always exactly one, which is why they are two operations with two `ai_authz`
scopes rather than one `update`. The standard's `Relationship` and `Role`
objects only if a foreign client names them, per the IFR-facade rule
(`PLAN-SERVICES` §7). **One honest note, recorded because it argues against
this chapter's own verdict:** that module also records a finding — the `create`
path does not apply `bind_expert`'s two integrity checks — pinned as a
measurement of today's behaviour and deliberately not repaired. A relationship
service would have made those two paths one path. That is a real argument *for*
the service, it is written here rather than omitted, and it is not enough:
making the two paths agree is a batch in the crate, not a service.

**요지.** 관계 서비스의 핵심은 관계와 역할이 **일급 객체**라는 것이다 — 양쪽 끝에서
탐색 가능하고, 카디널리티와 타입 제약을 `Role`이 강제한다. 유예 사유는 이 파일이 아니라
나무에 있다: `tenant_service.rs`의 모듈 문서(2026-08-25 착지)가 세 관계의 역할·
카디널리티·무결성 규칙을 적었고, 그 두 측정이 곧 사유다 — **댕글링은 탐지되는 것이
아니라 불가능하다**(생성이 대상을 만들고, 변경이 존재를 요구하고, 무엇도 대상을 파괴하지
않으므로 그래프는 자라기만 한다), 그리고 **와이어의 어떤 연산도 셋 중 무엇도 탐색하지
않는다**(역방향 역할도 탐색 연산도 없으며, 그것이 `CosRelationship`이 날랐을 절반이다).
따라서 서비스의 두 기여는 오늘 하나는 강제 불가이고 하나는 미착공이다. 방아쇠는 둘:
**역방향 질문에 답해야 하는 첫 연산**, 그리고 **대상을 파괴하는 첫 연산**. v1은 표준이
아니라 **객체가 아닌 색인** — 역방향을 저장하지 않고 같은 자료에서 파생한다. 정직한
단서 하나를 기록한다: 같은 모듈이 `create`가 `bind_expert`의 무결성 검사 둘을 적용하지
않는다는 **발견**을 고정해 두었고, 관계 서비스라면 두 경로가 하나였을 것이다 — 이 장의
판정에 반대하는 논거이므로 빼지 않고 적는다. 그래도 충분하지 않다. 두 경로를 맞추는 것은
서비스가 아니라 크레이트 안의 배치다.

---

## 14. CosContainment — the lifetime bound is already structural / 포함 관계

**What it is.** A standard *specialization* of §13, in the same document:
`ContainsRole` and `ContainedInRole`, one-to-many, with the contained end
exactly one. Naming a relationship "containment" buys one thing a bare
reference does not have — the contained object's lifetime is bounded by its
container's, so destroying the container is **defined** over the contained set.

**Why deferred.** The bound it names is already enforced, and enforced more
strongly than a service could. Objects here live in the tenant service's own
maps; an entry's lifetime *is* the object's lifetime, and no wire operation can
outlive its owner. Rust ownership is the containment enforcement and a compiler
checks it, where `check_minimum_cardinality` is a call somebody has to remember
to make. And the operation containment exists to define has no instance:
`retire` takes out the model and nothing it points at, and nothing in the
contract destroys a `PolicyDomain` or an `EnterpriseExpert` at all
(`tenant_service.rs`, same module documentation as §13).

**Trigger.** The first object destroyed while something else still names it —
§13's second trigger seen from the lifetime-bound end. Concretely, an operation
that removes a `PolicyDomain` or an expert from the tenant service's maps. That
single event makes three things true at once: the graph stops only growing, a
dangling reference becomes possible for the first time, and *cascade or refuse*
becomes a decision somebody has to write down. Observable in this tree.

**v1 sketch.** Not `CosContainment`. A destroying operation states its
propagation in its own contract — refuse while referenced (`BAD_INV_ORDER`, the
answer `PLAN-SERVICES` §3 already uses for "no such edge") or cascade with the
cascade enumerated — and §13's derived index is what makes *while referenced*
answerable at all. Two roles as objects only on a foreign client's demand.

**요지.** §13의 표준 특수화이며, 포함이 참조보다 더하는 것은 **수명 종속** 하나다 —
컨테이너를 파괴하면 포함된 것들에 대해 무엇이 일어나는지가 정의된다. 유예 사유: 그
종속은 이미 강제되어 있고, 서비스보다 강하게 강제된다 — 객체는 테넌트 서비스의 맵 안에
살고 항목의 수명이 곧 객체의 수명이며, 이것을 검사하는 것은 잊을 수 있는 호출이 아니라
컴파일러다. 그리고 포함이 정의하려는 연산에 사례가 없다: `retire`는 모델만 거두고 그것이
가리키는 것은 건드리지 않으며, 계약의 무엇도 `PolicyDomain`이나 익스퍼트를 파괴하지
않는다. 방아쇠는 **다른 것이 아직 이름하고 있는데 파괴되는 첫 객체** — 그 한 사건이
세 가지를 동시에 참으로 만든다(그래프가 자라기만 하는 것이 끝나고, 댕글링이 처음으로
가능해지고, *연쇄냐 거부냐*가 누군가 적어야 할 결정이 된다). v1은 표준이 아니라
파괴하는 연산이 자기 계약에 전파 규칙을 적는 것이며, §13의 파생 색인이 "아직 참조 중"을
답할 수 있게 하는 것이다.

---

## 15. CosReference — the shape is present three times over / 참조 관계

**What it is.** The other standard specialization in the same document:
`ReferencesRole` / `ReferencedByRole`, many-to-many, and — the distinction from
§14 that is the whole reason the spec ships both — **neither end's lifetime is
bounded by the other's**.

**Why deferred.** The shape is present three times and the service is absent
for §13's reason. `Manifest::experts` is exactly a `ReferencesRole`: a model
references many experts by capability id, many models of many tenants name the
same base, and many of a tenant's models may name one domain
(`tenant_service.rs`, as §13). Each is a **string, never a reference**, so a
holder of a manifest can read all three names and reach none of the objects —
which is the same fact `COMPONENTS.md`'s gap row reports from the other end,
that `bind_expert` and `set_policy` take references no operation of the contract
returns. The link has no object because nothing asks the far end a question.

**Trigger.** The reference count becomes load-bearing: the first decision that
must know **how many** holders name a target rather than merely that it exists.
The nearest concrete instance is eviction, and the precision matters — the
residency machine refuses `EVICT` on `ACTIVE` with the named guard refusal
`NoInflight` (§5), which counts *calls in flight*, not *compositions that bind
the expert*. An expert with no inflight call and three manifests binding it is
evictable today and that is **correct**, because eviction is not destruction.
It becomes this chapter's trigger the day a decision must tell "nobody is
calling it" apart from "nobody is composing it". Observable in the residency
machine's own inputs, and it is not a claim that anything is wrong today.

**v1 sketch.** Not `CosReference`. §13's derived index answers *how many*, and
the count is a number the deciding operation reads — not a `ReferencedByRole`
object that has to be kept in step with the manifests it summarises, which
would be a second copy of a fact whose first copy is authoritative. If the
count must ever be enforced rather than reported, it is enforced where the
decision is made, the way `NoInflight` already is.

**요지.** 같은 문서의 다른 표준 특수화이며, §14와 갈리는 지점은 **어느 쪽 수명도 상대에
종속되지 않는다**는 것이다. `Manifest::experts`가 정확히 `ReferencesRole`이고, 같은
모양이 base와 domain에서 세 번 반복된다 — 그런데 셋 다 **참조가 아니라 문자열**이므로
매니페스트를 든 쪽은 이름 셋을 다 읽고도 객체에는 하나도 닿지 못한다. `COMPONENTS.md`의
공백 행이 반대편에서 보고하는 것과 같은 사실이다. 방아쇠는 **참조 수가 결정을 지탱하게
되는 순간** — 대상이 존재하는지가 아니라 **몇이** 이름하는지를 알아야 하는 첫 결정이다.
가장 가까운 사례는 축출이고 정밀함이 중요하다: `ACTIVE`에서의 `EVICT`를 막는
`NoInflight`는 *진행 중인 호출*을 세지 *조합*을 세지 않는다. 진행 중 호출이 없고 세
매니페스트가 묶고 있는 익스퍼트는 오늘 축출 가능하며 그것이 **옳다** — 축출은 파괴가
아니기 때문이다. "아무도 부르지 않는다"와 "아무도 조합하지 않는다"를 구분해야 하는 날
이 장의 방아쇠가 된다. 오늘 무엇이 틀렸다는 주장이 아니다. v1은 §13의 파생 색인이
그 수를 답하는 것이며, 별도의 역할 객체를 두어 매니페스트와 보조를 맞추는 일이 아니다.

---

## 16. CosCompoundLifeCycle — the criteria are decided and were unwritten / 복합 생명주기

**What it is.** The Life Cycle Service's half that defines `copy`, `move` and
`remove` **over a relationship graph**: `Node`, `Role`, `Relationship` and
`PropagationCriteriaFactory`, with a per-role traversal criterion of *deep*
(copy the target too), *shallow* (drop the link), *none* or *inhibit*. Plus
`CosLifeCycleContainment` and `CosLifeCycleReference`, which fix the criteria
for §14's and §15's relationships.

**Why deferred.** The operation exists and its criteria are already decided —
they were merely unwritten until 2026-08-25. `clone_model` *is*
`CosCompoundLifeCycle::copy`, and `tenant_service.rs` measured what it does
with each of the three roles: all three are traversed with **reference**
semantics and none with *deep* or *shallow* — the base is neither followed nor
copied, no adapter is duplicated, and the clone joins the source's policy
domain rather than getting one of its own. So this service's contribution here
would be to make a fixed choice configurable, and there is exactly **one**
traversing operation, so there is nothing for a criterion to disagree with. A
criteria factory serving one caller is machinery in the place of a decision.

This is also the paragraph `PLAN-SERVICES` §5 was missing and D023 §3's R2
asked for: the standard's `copy`/`move` are defined over relationships and ours
are not, and what ours do instead is now measured and written where it is
enforced.

**Trigger.** A **second** operation that traverses the same three roles. One
operation's traversal is a behaviour; two operations' traversals are a policy,
and the moment the two differ somebody has to name the criteria to say why.
`retire` is the visible candidate — it is *shallow* on all three today, and a
`retire` that must reclaim a base nothing else is over would be the first
*deep* removal in the tree. Observable as a diff between two operations.
A weaker second trigger, **flagged rather than dressed up**: a caller wanting a
clone with different semantics than the one `clone_model` fixes. That one's
subject is outside this project in D023 §2's sense — in practice the owner is
the only party who could bring it — so it is recorded as the weaker half and
the first trigger is the one to watch.

**v1 sketch.** Not `CosCompoundLifeCycle`. Propagation stays a property of each
operation's contract, named in the documentation beside the code that performs
it the way `clone_model`'s three now are, plus a **test per role per
operation**, so a criterion that changes turns a test red instead of leaving a
sentence stale — which is the shape the existing
`clone_model_traverses_all_three_relationships_by_reference` already takes.
`PropagationCriteria` as data only if a second operation genuinely needs a
different criterion for the same role, which is a question its batch can answer
and this file cannot.

**요지.** 생명주기 서비스에서 `copy`/`move`/`remove`를 **관계 그래프 위에** 정의하는
절반이며, 역할마다 *deep*/*shallow*/*none*/*inhibit* 전파 기준을 붙인다. 유예 사유:
연산은 이미 있고 기준도 이미 정해져 있었다 — 다만 2026-08-25까지 적히지 않았을 뿐이다.
`clone_model`이 곧 그 `copy`이고, 측정 결과 세 역할 **전부 reference 의미**로 순회하며
*deep*도 *shallow*도 아니다(base를 따라가지도 복사하지도 않고, 어댑터를 복제하지 않고,
클론은 원본의 정책 도메인에 합류한다). 따라서 이 서비스의 기여는 고정된 선택을 설정
가능하게 만드는 것뿐인데, 순회하는 연산이 **하나뿐**이라 기준이 어긋날 상대가 없다.
이것은 `PLAN-SERVICES` §5에 빠져 있던 문단이기도 하다 — 표준의 `copy`/`move`는 관계
위에 정의되고 우리 것은 아니며, 대신 무엇을 하는지가 이제 강제되는 자리에 적혀 있다.
방아쇠는 **같은 세 역할을 순회하는 두 번째 연산**이다. 하나의 순회는 동작이고 둘의
순회는 정책이다. 더 약한 두 번째 방아쇠(다른 의미의 클론을 원하는 호출자)는 D023 §2의
뜻에서 주체가 이 프로젝트 밖이므로 **약한 쪽이라고 표시해** 둔다. v1은 표준이 아니라
전파를 각 연산의 계약에 적고 **역할×연산마다 테스트**를 두는 것이다 — 기준이 바뀌면
문장이 낡는 대신 테스트가 빨개진다.

---

## 17. CosObjectIdentity — we serve the weaker thing, on purpose / 객체 동일성

**What it is.** The smallest module in the suite, and it too lives in the
Relationship Service document: one interface, `IdentifiableObject`, with a
readonly `constant_random_id` — an `unsigned long` attribute **of the object** —
and `is_identical(other)`. It exists precisely because `CORBA::Object`'s own
identity operations are deliberately too weak to build a relationship graph on.

**Why deferred — and the reason is that what we have is the weaker thing, not
this.** `orbweaver-object` serves `_is_equivalent` and `_hash`
(`is_equivalent`, `reference_hash`, `crates/orbweaver-object/src/lib.rs`), and
they are not what this module declares:

| | ours | `CosObjectIdentity` |
|---|---|---|
| `_is_equivalent` vs `is_identical` | may answer `false` for two references that *do* denote one object — it confirms identity and can never refute it, which the doc comment records against the specification rather than leaving to intuition | must both confirm **and** refute |
| `_hash` vs `constant_random_id` | FNV-1a over the **first profile's** host, port and object key — a hash of the *reference*, documented as existing to bucket references and not to compare them | an attribute of the *object*, constant for its lifetime |

The gap is not theoretical. `reference_hash` reads a profile, so two references
that denote one object through different profiles hash differently — and D013
§2.2 is the measurement that this situation is real rather than hypothetical:
omniORB's `_is_equivalent` answered **true** for two independently created
proxies to one object while each still paid its own location forward. Our own
answer comes from comparing one profile's host, port and key, so it too can say
`false` where `is_identical` would have to say `true`. That is the standard's
licence being used, not a defect.

The second reason is that nothing needs the stronger operation: the confirm-only
guarantee is exactly right for the lookups that use it. **Whether any current
caller acts on a `false` is unmeasured by this chapter** — reading the call
sites is the trigger's first step, not a claim this file makes.

**Trigger.** The first caller that must **refute** identity: one that
deduplicates references, keys a map by reference, or decides from a `false`
that two references are different objects. Observable by reading the call sites
of `is_equivalent`, and it needs nobody to name a consumer.

**v1 sketch.** Not `IdentifiableObject`. The near-term answer is naming, and it
is **not this file's to do**: D023 §3's R3 puts the doc comments gaining the
standard's vocabulary — that `_is_equivalent` is CORBA's confirm-only test,
that `is_identical` is the stronger operation this project does not serve, and
that `_hash` is not `constant_random_id` — in a batch with a crate footprint,
following D019 step 2's shape of naming and routing rather than new behaviour.
If a caller ever must genuinely refute, the answer is **not** a random id the
client is asked to trust: the POA already mints object keys, two references to
one object share one key, and so the honest identity test is key equality
decided at the server. `IdentifiableObject` as a facade only if a foreign
client names it.

**요지.** 스위트에서 가장 작은 모듈이며 이것 역시 관계 서비스 문서에 산다 —
`IdentifiableObject` 하나에 `constant_random_id`(객체의 속성)와 `is_identical`뿐이다.
`CORBA::Object`의 동일성 연산이 관계 그래프를 짓기에는 **일부러** 약하기 때문에 존재한다.
유예 사유는 우리가 가진 것이 바로 그 **약한 쪽**이라는 것이다: `is_equivalent`는 같은
객체를 가리키는 두 참조에 `false`를 답할 수 있고 — 동일성을 **확인**할 수는 있어도
**반박**할 수는 없으며, 문서 주석이 이를 명세에 대고 적어 두었다 — `reference_hash`는
첫 프로파일의 호스트·포트·키에 대한 FNV-1a, 즉 *참조*의 해시이며 비교가 아니라 버킷팅을
위해 있다고 적혀 있다. 간극은 관념이 아니다: D013 §2.2가 그 상황이 실재함을 측정했다 —
omniORB의 `_is_equivalent`는 독립 생성된 두 프록시에 **참**을 답하면서도 각자 자기 포워드를
치렀다. 두 번째 사유는 더 강한 연산을 필요로 하는 것이 없다는 것이며, **오늘의 호출자 중
`false`에 근거해 행동하는 것이 있는지는 이 장이 측정하지 않았다** — 호출 지점을 읽는 것이
방아쇠의 첫 걸음이지 이 파일의 주장이 아니다. 방아쇠는 **동일성을 반박해야 하는 첫
호출자**다. v1은 표준 인터페이스가 아니라 이름 붙이기이고, 그 이름 붙이기는 **이 파일의
몫이 아니다** — D023 §3의 R3가 크레이트 발자국을 가진 배치에 두었다. 정말로 반박이
필요해지면 답은 클라이언트가 믿어야 하는 난수 id가 아니라 **서버에서의 오브젝트 키 동등성**
이다. POA가 이미 키를 발행하고, 한 객체를 가리키는 두 참조는 한 키를 공유한다.

---

## 18. CosExternalization — the blob is opaque and that is the content / 외부화

**What it is.** `CosExternalization` — `Stream::externalize` / `internalize`
with `begin_context` / `end_context`, plus `StreamFactory` and
`FileStreamFactory`; and `CosStream` — `Streamable` (`external_form_id`,
`externalize_to_stream`, `internalize_from_stream`) and `StreamIO`, a **typed
per-element** read/write interface (`write_string`, `write_object`,
`read_long`, …). With compound externalization it writes down a whole
relationship graph. It is CORBA's answer to "record this object graph so
something else can reconstitute it."

**Why deferred.** Three reasons.

1. **`StreamIO` is a remote call per element.** Externalizing a struct of ten
   members is ten round trips to a stream object — the same chatty shape §6
   rejects for remote iterators, refused for the same latency reason. CDR
   already writes a value graph down in one buffer, and AnyJSON already carries
   one across the agent boundary.
2. **The one place we persist an object's state is opaque on purpose.** §4
   records that the residency machine's `PERSISTENT` lifespan blob is a
   `Vec<u8>` preserved across evict and reload, *with the opacity being the
   entire content of the TRANSIENT/PERSISTENT distinction*; `residency.rs` says
   the same at the field itself. `Streamable` is precisely the demand that it
   stop being opaque — that every object publish a typed external form and an
   `external_form_id` others may read. Adopting it spends the distinction to
   buy a format nobody is waiting for.
3. **`external_form_id` is a second identity scheme for types.** We have one
   and it is checked against an oracle — repository ids, diffed against
   `omniidl` (`corpus/pragma/`, the `repository-ids` binary). A parallel format
   identifier maintained by hand beside it is the classifier defect CLAUDE.md
   names: two lists of the same fact, drifting silently, with nothing that
   compiles either.

**Trigger.** The blob must be read by something that did not write it —
precisely: a second implementation of the `ExpertLoader` blob seam, a
requirement that a `PERSISTENT` blob move between hosts, or a loader version
change that must read the previous version's blob. Any one of those makes a
**format** owed, and a format others must read is what `Streamable` names.
Observable as a second loader or a deployment topology, not as a request.

**v1 sketch.** Not `CosExternalization`. If a format becomes owed it is **one
format with one home**, the loader's, versioned, with the version in the blob's
leading bytes so a loader that cannot read one **refuses loudly rather than
misparsing** — `orbweaver-giop`'s own promise, and the property D027 §3 refuses
to trade for convenience. No `StreamIO`: the unit is a whole blob, never an
element, because the per-element round trip is the reason this chapter exists.
Compound externalization over a relationship graph is §13's and §16's question
and inherits their answers rather than reopening them here.

**요지.** 객체 그래프를 스트림에 적어 다른 곳에서 되살리는 CORBA의 답이며, `Streamable`과
**원소 단위 타입 인터페이스** `StreamIO`가 핵심이다. 유예 사유 셋: (1) **`StreamIO`는
원소마다 원격 호출**이다 — 멤버 열 개짜리 구조체를 외부화하면 왕복 열 번이며, §6이 원격
반복자에 대해 거부한 바로 그 형태다. CDR은 이미 값 그래프를 버퍼 하나에 적고, AnyJSON은
이미 그것을 에이전트 경계 너머로 나른다. (2) **객체 상태를 영속화하는 단 한 곳이 일부러
불투명하다** — §4가 적었듯 `PERSISTENT` 블롭의 **불투명함 자체가 TRANSIENT/PERSISTENT
구분의 내용 전부**인데, `Streamable`은 정확히 그것을 그만두라는 요구다. (3)
**`external_form_id`는 타입에 대한 두 번째 신원 체계**다 — 우리에게는 오라클에 대고 검사되는
하나(리포지터리 id, `omniidl`과 대조)가 이미 있고, 그 옆에 손으로 유지되는 형식 식별자는
CLAUDE.md가 이름한 분류자 결함이다. 방아쇠는 **그 블롭을 쓰지 않은 무언가가 읽어야 할 때** —
두 번째 로더, 호스트 간 이동, 또는 이전 버전 블롭을 읽어야 하는 로더 버전 변경. v1은 표준이
아니라 **집 하나짜리 형식 하나**이며, 선두 바이트에 버전을 두어 읽을 수 없는 로더가
오파싱 대신 **시끄럽게 거부**하게 한다. `StreamIO`는 없다 — 단위는 원소가 아니라 블롭
전체다. 왕복이 이 장이 존재하는 이유이기 때문이다.

---

## 19. CosQuery — a filter is not a calculator / 질의 서비스

**What it is.** `CosQuery` plus `CosQueryCollection`: `QueryEvaluator` with
`evaluate(query, ql_type, params)`, `QueryableCollection`, and a `QueryManager`
that `create`s `Query` objects with `prepare` / `execute` / `get_status` /
`get_result`. Crucially, **the service defines no query language of its own.**
`QueryLanguageType` and its subtypes — `SQLQuery`, `SQL_92Query`, `OQL`,
`OQL_93`, `OQL_93_Basic` — are empty marker interfaces used as type tags, and
`ql_types` is the readonly attribute by which a server declares *which of those
named languages* it speaks. Results come back as `CosQueryCollection`
collections, which is §6's remote-collection-with-iterator shape.

**Why deferred — and this is the sentence D023 §5 asked for, written from the
two grammars rather than from intuition.**

> **The trader's query is not `CosQuery`, and the reason is not that ours is
> smaller.** `CosQuery` defines no language: implementing `QueryEvaluator`
> means declaring through `ql_types` that you speak SQL-92 or OQL-93, so there
> is no version of the interface behind which our constraint language could be
> served — it is not one of the languages the tags admit. And the distance is a
> difference in kind rather than in size. Our constraint is a **predicate over
> one offer at a time**: eight productions, six comparison operators, three
> connectives, exactly one built-in (`EXIST`), **no arithmetic of any kind**,
> evaluated over a **closed set of ten fixed struct fields** rather than names
> looked up in a property bag. It decides which of the stored offers come back
> and it can compute nothing. SQL-92 and OQL-93 are **expression languages over
> collections**: their select list constructs values that were never stored,
> they join across collections, they aggregate, and they nest queries inside
> queries. Ours has one collection, no join, no aggregate and no subquery — the
> single nested form, `WITH`, is evaluated against the *same one offer*. Even
> projection is outside the grammar: the wire's `SpecifiedProps` is a separate
> parameter naming the same ten fields, applied after the predicate has already
> chosen whole offers, and what comes back is offers with their properties,
> never a constructed tuple. The two languages answer different questions —
> *which of the things I stored match* against *what value can I compute from
> what I stored*. **A trader's constraint is a filter; `CosQuery` is a
> calculator.** They are not the same service at two sizes, and the trader's
> query growing will not turn it into this one.

Three further reasons, shorter. The result shape is a `CosQueryCollection` with
an iterator, which is §6's chapter and inherits its verdict. A `Query` object
with `prepare`/`execute`/`get_status`/`get_result` is a **stateful remote
cursor**, which is server-held per-client state with a lifecycle nobody
requires. And accepting a query language over the wire means accepting a
*parser* over the wire, with all the refusal-quality obligations S4 carries, in
a language we do not otherwise implement.

**Trigger.** An operation whose parameter is a query **in a language the caller
names** — the moment a client submits query text and we must answer *which
language is that*, `ql_types` is the question being asked and the service is the
shape of the answer. D003's catalog is the plausible route: a vector-and-metadata
search a client composes rather than one we compose for it. Second, weaker and
the same class as §6's: a foreign client expecting `QueryEvaluator` by name
**and** speaking SQL-92 or OQL-93, since one without the other is not this
trigger. Both observable without the owner naming a consumer.

**v1 sketch.** Not `CosQuery`. If a client must submit query text, it submits
**the constraint language we already have**, named as itself and versioned, with
S4-style positioned errors — which the engine already produces and which every
refusal in the grammar already carries. `ql_types` would then honestly report a
language that is not SQL-92 and not OQL-93, which is precisely why the standard
interface is the wrong wrapper: it exists to let a client *choose* between two
languages it already knows, and offering a third under that name would be the
decorative dishonesty this project rejects elsewhere. Paging, if a result set
ever exceeds its bound, is §6's one `(items, next_cursor)` shape and not a
`Query` object.

**요지.** `CosQuery`는 **자기 질의 언어를 정의하지 않는다** — `QueryLanguageType`의
하위 타입들은 빈 표지 인터페이스이고, `ql_types`는 서버가 SQL-92냐 OQL-93이냐를
선언하는 속성이다. D023 §5가 요구한 문장은 이것이다: **트레이더의 질의는 `CosQuery`가
아니며, 이유는 우리 것이 더 작아서가 아니다.** 그 인터페이스를 구현한다는 것은 표지가
허용하는 두 언어 중 하나를 말한다는 뜻이므로, 우리 제약 언어를 그 뒤에 세울 수 있는
판본은 없다. 그리고 거리는 크기가 아니라 **종류**의 차이다. 우리 제약은 **한 번에 오퍼
하나에 대한 술어**다 — 생성 규칙 여덟, 비교 연산자 여섯, 접속사 셋, 내장 함수 정확히
하나(`EXIST`), **산술은 일절 없음**, 그리고 프로퍼티 가방의 이름이 아니라 **고정된 구조체
필드 열 개의 닫힌 집합** 위에서 평가된다. 저장된 오퍼 중 무엇이 돌아올지를 정할 뿐 아무것도
계산하지 못한다. SQL-92와 OQL-93은 **컬렉션 위의 식 언어**다 — select 목록이 저장된 적
없는 값을 구성하고, 컬렉션을 조인하고, 집계하고, 질의 안에 질의를 중첩한다. 우리에게는
컬렉션이 하나, 조인도 집계도 부질의도 없다(유일한 중첩 형태 `WITH`는 *같은 오퍼 하나*에
대해 평가된다). 사영조차 문법 밖이다 — 와이어의 `SpecifiedProps`는 술어가 이미 오퍼
전체를 고른 뒤에 같은 열 개 이름에 적용되는 별개 파라미터이고, 돌아오는 것은 구성된
튜플이 아니라 프로퍼티를 단 오퍼다. 두 언어는 다른 질문에 답한다 — *저장한 것 중 무엇이
맞는가* 대 *저장한 것으로 무엇을 계산할 수 있는가*. **트레이더의 제약은 필터이고
`CosQuery`는 계산기다.** 한 서비스의 두 크기가 아니며, 트레이더의 질의가 자란다고 저것이
되지 않는다. 짧은 사유 셋 더: 결과가 반복자 달린 컬렉션이라 §6의 판정을 물려받고,
`Query` 객체는 아무도 요구하지 않는 **상태 있는 원격 커서**이며, 와이어로 질의 언어를
받는다는 것은 와이어로 **파서**를 받는다는 뜻이다. 방아쇠는 **호출자가 언어를 지목하는
질의를 파라미터로 받는 연산** — 그 순간 `ql_types`가 곧 물어지는 질문이 된다. v1은
표준이 아니라 이미 있는 제약 언어를 자기 이름으로 버전과 함께 받는 것이다.

---

## 20. CosLicensingManager — the mechanism is here and the principal is not / 라이선싱

**What it is.** `LicenseServiceManager` hands out a producer-specific licence
service, and on it `start_use`, `check_use` and `end_use`. A use is started
against a **producer's** licence, periodically re-checked, and released;
`check_use` may answer with an action of *continue* or *terminate* plus a
notification, so a licence can **end a use already in progress**, and
`ChallengeData` carries the producer's authentication of the request.

**Why deferred.** Two reasons that point in opposite directions, which is why
the chapter was worth writing rather than being a "no consumer" row.

1. **The mechanism is already here, and the service's subject is not.** The MCP
   boundary's interceptor chain has named `SEAT_QUOTA` since F4 and
   `orbweaver-mcp::quota` fills it: a budget of `limit` calls counted against a
   `Scope` — the whole bridge, a caller, a caller's interface, a caller's
   operation — with a stated `Renewal`, refused as `QuotaExhausted` and reaching
   a stub as `TRANSIENT` when it renews. `start_use` and `end_use` are seat
   acquisition and release, and we have them. What differs is the **principal**:
   licensing meters use on behalf of a producer *who is not the operator*,
   while a quota is the operator's own limit on the operator's own resource.
   `ChallengeData` exists because the producer does not trust the deployment,
   and that mistrust is the entire service. Same mechanism, different party,
   and the party is the subject.
2. **`check_use` wants a clock, and this stack refuses one twice.** A periodic
   re-check that can terminate a use in progress is a timer. `quota.rs` records
   that nothing in that crate reads a clock — windows advance only when the host
   opens one, which is why a per-window, a per-batch and a whole-process budget
   are the same code and all three replay identically — and it names a gate as
   the worst possible place for a non-reproducible answer. §3 declines the Time
   Service to protect the same property in the trading engine. A licence check
   on our own clock would be the first clock inside a gate.

**Trigger, and it is the weakest of the eight — said here rather than dressed
up.** The nominal trigger is a registry entry whose use must be metered for
somebody who is not the operator: a model, base or expert supplied under terms
the deployment must enforce and report. That is observable as a registry entry
with an external licensor — but it is a **commercial fact about a deployment**,
so in practice the owner is the only party who could bring it, which by D023
§2's own diagnosis makes it a trigger whose subject is outside this project.
Recorded as observable in principle and unreachable in practice, which is worth
more than a trigger that reads well and cannot fire.

The second trigger is sharper and lives in the tree: **the first requirement
that a granted seat be revocable mid-call** — that something already dispatched
be stopped because its entitlement ended. That is `check_use`'s *terminate*,
and the quota chain has no such thing: it decides before the call and never
during it. Observable as a requirement on the interceptor chain.

**v1 sketch.** Not `CosLicensingManager`. A licensor label on the registry
entry beside the existing `Scope`, so metering stays **one mechanism with two
configurations** rather than two mechanisms that can disagree — the §1 argument
about two filters, in its cheapest form. The report is the audit ledger, which
already carries a line per permitted and refused call; what makes it evidence a
third party can check is §8's append-only hash chain, and the order of that
argument carries over unchanged — the chain is free and solves detection, a
detached signature costs key management and solves only attribution, so the
expensive half waits until somebody must verify independently. No `check_use`
timer: if revocation is ever owed it arrives as a window the host opens, which
keeps the replay property that made the quota trustworthy in the first place.
The standard's three operations only if a foreign client names them.

**요지.** 생산자의 라이선스에 대고 사용을 시작·주기적 재확인·해제하는 서비스이며,
`check_use`는 *계속*이나 *종료*로 답할 수 있어 **이미 진행 중인 사용을 끝낼 수** 있다.
유예 사유 둘은 서로 반대 방향을 가리키며, 그래서 이 장은 "소비자 없음" 한 줄이 아니라
쓸 가치가 있었다. (1) **기구는 이미 있고, 서비스의 주체가 없다** — F4 이래 인터셉터
체인의 `SEAT_QUOTA`를 `orbweaver-mcp::quota`가 채운다: `Scope`(브리지 전체·호출자·
호출자의 인터페이스·호출자의 연산)에 대고 세는 `limit` 예산, 명시된 `Renewal`,
`QuotaExhausted` 거부, 갱신되면 스텁에는 `TRANSIENT`. `start_use`/`end_use`는 좌석의
획득과 해제이고 우리에게 있다. 다른 것은 **주체**다 — 라이선싱은 **운영자가 아닌
생산자**를 대신해 사용을 계량하고, 쿼터는 운영자가 자기 자원에 두는 자기 한계다.
`ChallengeData`가 존재하는 이유는 생산자가 배포처를 믿지 않기 때문이며, 그 불신이
서비스의 전부다. 같은 기구, 다른 당사자, 그리고 당사자가 곧 주제다. (2) **`check_use`는
시계를 원하고 이 스택은 시계를 두 번 거부한다** — `quota.rs`는 그 크레이트가 시계를 읽지
않으며 윈도는 호스트가 열 때만 넘어간다고 적고(그래서 세 가지 구성이 같은 코드이고 셋 다
동일하게 재현된다), 재현 불가능한 답이 있기에 가장 나쁜 곳으로 **게이트 안**을 이름한다.
§3이 트레이딩 엔진의 같은 성질을 지키려 시간 서비스를 거절한 것과 같다. **방아쇠는 여덟
중 가장 약하며, 꾸미지 않고 여기 적는다**: 운영자가 아닌 누군가를 위해 계량되어야 하는
레지스트리 항목 — 관측 가능하지만 **배포의 상업적 사실**이므로 실제로는 소유자만이 가져올
수 있고, D023 §2의 진단대로 주체가 프로젝트 밖인 방아쇠다. 원리상 관측 가능하고 실무상
도달 불가라고 기록하는 편이, 잘 읽히지만 당겨질 수 없는 방아쇠보다 낫다. 더 날카로운 두
번째 방아쇠는 나무 안에 있다: **이미 부여된 좌석을 호출 도중에 회수해야 하는 첫 요구** —
`check_use`의 *종료*이며, 쿼터 체인에는 그런 것이 없다(호출 전에 정하고 호출 중에는 결코
정하지 않는다). v1은 표준이 아니라 기존 `Scope` 옆의 라이선서 라벨이며, 계량을 **두
기구가 아니라 한 기구의 두 구성**으로 남긴다. 보고는 감사 원장이고, 제3자가 검사할 수 있게
만드는 것은 §8의 추가 전용 해시 체인이며 그 논증의 **순서**가 그대로 넘어온다.

---

## The two ORB features that had no chapter — §21–§22 / 장이 없던 두 ORB 기능

Added 2026-08-26. D018 §3.3 listed three absences carrying no decision at all —
`def_kind`, the POA policies, and these two — and put these two third. **The
first two landed and the third did not**, which D029 §3.3 re-measured today.
Re-measured again before writing: this file's four `interceptor` hits (§2
twice, §20 twice) are all the MCP boundary's own chain, and `bidirectional`,
`BiDir` and `BI_DIR` appear in it nowhere at all.

**D018 §3.3's own sentence needs one correction, and it is the reason this
section exists rather than a footnote.** It said the two are *"in **no** plan
document"*. That is not what a grep says. `PLAN.md` §1 excludes *bidirectional
GIOP (needed for callback-style systems behind firewalls; revisit after v1)*
from v1 scope, and `PLAN.md` §2.1's mechanism table lists **Portable
Interceptors** with *"guardrails: authorization, dry-run, approval, audit
logging, tracing"* as their value — both in the Korean twin as well. So neither
was unmentioned; both were **mentioned in a form that cannot be resumed from**,
which is §0's whole argument one layer up: a scope line records a decision and
a motivation row records an intention, and neither carries a trigger. What was
true, and what D029 §3.3 measured precisely, is that `PLAN-DEFERRED` had no
chapter for either.

**They are not services, which is why they were missed twice.** §13–§20's batch
swept the twenty-one CORBAservices against three plan documents and could not
have caught these: an interception chain and a transport mode are **ORB
features**, so they sit in neither `PLAN-SERVICES` §8's exclusion table nor
D023 §1's service map, and every instrument this project owns for finding an
un-reasoned absence was pointed at the service list. That is the root cause,
and it is not §13–§20's: those eight were absent together because five of them
were one specification nobody opened; these two are absent because **the census
had the wrong unit.**

**They add no rows to `PLAN-SERVICES` §8.** That table's column is *Service*
and these are not services, so §0's sentence counting its rows is deliberately
untouched by this batch — the two documents' inventories still agree because
neither document gained an inventory item.

> **Priority zero, set 2026-08-26.** This section is subordinate to the ORB
> completion criterion, whose home is
> [`D029`](decisions/D029-what-a-complete-orb-would-mean.md) §6: *no leak in
> the transparency that a caller can invoke any target holding only a
> reference, without knowing its location, backend, language or load state, and
> that this survives targets being added, removed, moved, loaded or evicted at
> runtime.* The criterion is stated there and **not restated here** — what is
> recorded below is only how these two chapters bear on it.
>
> *0순위 기준의 집은 D029 §6이며 여기서 다시 적지 않는다. 아래에 적는 것은 이 두
> 장이 그 기준에 **어떻게 닿는지**뿐이다.*

> **How these two bear on it, and they are not equals.** §22 is a
> **location-transparency item**: bidirectional GIOP is the second answer to
> the problem `spikes/nat_rewrite.sh` answers, and §22's job is to say which
> half of D029 §6.1's Location row this project holds and which half it does
> not. §21 closes **none** of §6.1's five leaks — it is a capability, and §6's
> criterion ranks a capability below a leak, which is why D029 §5's re-ordering
> already puts O3 fourth. Writing them in as peers would be the mistake §6
> exists to prevent; they share a section because they were *missed* together,
> not because they weigh the same.

**Neither is owed an implementation.** D023 §6's fence and D018 §4.1's sentence
apply here exactly as they applied to the eight: *writing a thing down makes it
feel owed*, and a plan document is how that discipline breaks. Both v1 sketches
below open by refusing the standard's interface, and §21's refuses it twice —
once for the interface and once for the registration model.

*2026-08-26 추가. D018 §3.3이 "결정이 아예 없는 부재" 셋 가운데 이 둘을 세 번째로
놓았고, **앞의 둘은 착지했고 셋째는 하지 않았다**(D029 §3.3이 오늘 재측정). 쓰기 전에
다시 쟀다: 이 파일의 `interceptor` 네 건은 전부 MCP 경계의 자체 체인(§2 두 번, §20 두
번)이고 `bidirectional`·`BiDir`·`BI_DIR`은 아예 없다. **D018 §3.3의 문장 하나는
고쳐야 하며, 그것이 이 절이 각주가 아니라 절인 이유다** — "**어떤** 계획 문서에도
없다"고 했으나 grep은 다르게 말한다. `PLAN.md` §1은 v1 범위에서 *양방향 GIOP(방화벽
뒤 콜백형 시스템에 필요; v1 이후 재검토)*를 제외하고, §2.1의 메커니즘 표는 **포터블
인터셉터**를 *"가드레일: 인가, dry-run, 승인, 감사로그, 트레이싱"*이라는 가치와 함께
싣는다 — 한국어 쌍둥이에도 똑같이 있다. 즉 둘은 언급되지 않은 것이 아니라 **재개할 수
없는 형태로 언급**되어 있었고, 이는 §0의 논지를 한 층 위에서 되풀이한다: 범위 한 줄은
결정을 기록하고 동기 표의 한 행은 의도를 기록하지만, 어느 쪽도 방아쇠를 싣지 않는다.
참인 문장은 D029 §3.3이 정확히 측정한 것 — `PLAN-DEFERRED`에 두 장이 없었다 — 이다.
**서비스가 아니어서 두 번 놓쳤다**: §13–§20 배치는 스물한 개 CORBAservices를 훑었고
이 둘을 잡을 수 없었다. 인터셉션 체인과 전송 방식은 **ORB 기능**이라 `PLAN-SERVICES`
§8의 제외 표에도 D023 §1의 서비스 지도에도 없고, 이유 없는 부재를 찾는 이 프로젝트의
모든 계기가 서비스 목록을 향해 있었다. 근본원인이며 §13–§20의 것과 다르다: 그 여덟은
다섯이 한 명세였기에 함께 없었고, 이 둘은 **인구조사의 단위가 틀렸기** 때문에 없다.
따라서 **§8에 행을 더하지 않는다** — 그 표의 열은 *서비스*이고 이 둘은 아니므로 §8의
행을 세는 §0의 문장은 일부러 건드리지 않는다. 0순위 기준의 집은 D029 §6이며 여기서
다시 적지 않는다. **둘은 대등하지 않다**: §22는 **위치 투명성** 항목으로
`spikes/nat_rewrite.sh`가 답하는 문제의 다른 쪽 답이며, §6.1의 Location 행에서 우리가
쥔 절반과 쥐지 못한 절반을 말하는 것이 그 장의 일이다. §21은 다섯 구멍 중 **아무것도**
막지 않는다 — 기능이며, 기준은 기능을 구멍보다 뒤에 둔다(D029 §5가 O3을 네 번째로 둔
이유). 대등하게 적는 것이야말로 §6이 막으려는 실수다. 한 절에 있는 이유는 함께
*놓쳤기* 때문이지 무게가 같아서가 아니다. **어느 쪽도 구현을 빚지지 않는다** — D023
§6의 울타리와 D018 §4.1의 문장이 그대로 적용되며, 아래 두 v1 스케치는 표준
인터페이스를 거부하고 §21은 두 번 거부한다(인터페이스 한 번, 등록 모델 한 번).

---

## 21. Portable Interceptors — one discipline, two chains that cross / 포터블 인터셉터

**What it is.** CORBA 3.4 ch. 21. Three interceptor kinds, registered per-ORB
through an `ORBInitializer` at initialization and **immutable afterwards**:
`ClientRequestInterceptor` (`send_request`, `send_poll`, `receive_reply`,
`receive_exception`, `receive_other`), `ServerRequestInterceptor`
(`receive_request_service_contexts`, `receive_request`, `send_reply`,
`send_exception`, `send_other`), and `IORInterceptor`, whose
`establish_components` runs at POA creation and adds `TaggedComponent`s to
every IOR that POA mints. Three supporting pieces carry the actual weight.
`RequestInfo` exposes the request — operation, arguments, exceptions, and
**`get_request_service_context` / `add_request_service_context`**, which is how
OTS propagates a transaction, how CSIv2 carries a security token and how
`SendingContextRunTime` finds its peer. `PortableInterceptor::Current` gives
each request numbered **slots**, allocated at ORB init, so a thread-local set
before a call can be read by an interceptor and pushed into a service context.
A `Codec` from `CodecFactory` encapsulates the values that go into one. And a
client interceptor may raise **`ForwardRequest`**, redirecting the call — a
`LOCATION_FORWARD` the *ORB* originates rather than the servant. The ending
interception points run only for the starting points that completed.

**Why deferred — because D018 §3.3's question has an answer, and the answer is
no.** D018 asked whether ours and the standard's are the same idea at different
scopes. Read against `crates/orbweaver-mcp/src/interceptor.rs` — which already
claims a relationship in its own words, *"the CORBA portable-interceptor shape,
adapted to this call path"*, `send_request`/`receive_request` collapsing into
`Interceptor::before` and `send_reply`/`send_exception` into
`Interceptor::after` *"because this chain sits on one side of one call"* — the
honest verdict is that **they share a discipline and differ in both of the
things that decide what a chain is for: what it may read, and what it may do.**
"Different scopes" is the wrong picture, because neither's coverage contains
the other's. The scopes **cross**.

1. **Ours is not on the ORB's request path at all.** `orbweaver-giop` has no
   interception seam: `Invoker` (`crates/orbweaver-giop/src/lib.rs:1550`) is
   three methods with no hook, and `Dispatch`
   (`crates/orbweaver-giop/src/server.rs:625`) is the servant, not a seat in
   front of it. The chain is entered from `orbweaver-mcp`'s `Guarded` and
   `Bridge::invoke`, so **a call issued straight through `orbweaver_giop`
   passes no stage**, while the standard's chain is per-ORB and sees every
   request there is. In the other direction ours reads the contract, the
   caller's `ai_authz` scopes, the host's approval and the arguments as an
   AnyJSON document on both paths — facts that live above IDL and never reach
   the wire, which no `ServerRequestInterceptor` can see. Two chains, each
   holding what the other cannot.

2. **The standard's chain exists to modify the message; ours is built so that
   it cannot.** `add_request_service_context` is the capability everything
   above rides on. Measured here: there is **no API to attach an arbitrary
   service context to an outbound request** — the only production writer is the
   codeset arm, inline in two private methods (`lib.rs:2506`, `mux.rs:1058`);
   a reply **cannot carry one at all**, `encode_reply` hard-writing an empty
   list (`server.rs:435`); and an inbound reply's contexts are read and thrown
   away (`skip_service_contexts`, `lib.rs:1526`). Above that, `CallContext`
   deliberately carries no `Connection`, for the three reasons the module
   states: a stage that can dial can time out and stops being reproducible, can
   be hung by the very target the caller is being protected from, and — the
   decisive one — a stage that can send is a caller past its own gate. **A
   chain that may not touch the message is not a smaller portable interceptor.
   It is a different object.**

3. **Two of the three kinds have no counterpart, and the third is not an
   interceptor question here.** `IORInterceptor::establish_components` is how
   the standard gets a component into an IOR; we publish components from the
   servant side with no interceptor anywhere near it
   (`codeset::server_component`, CSIv2's mech list). `PortableInterceptor::
   Current`'s slots are the opposite of a `CallContext` whose whole design is
   that a stage is a function of the contract and the request; `PICurrent`,
   `PolicyCurrent` and `CodecFactory` exist in this tree only as string
   literals in `orb.rs`'s `RESERVED_OBJECT_IDS`, a list whose own doc says it
   **gates nothing** and that this ORB supplies none of them. And
   **`ForwardRequest` has zero occurrences in the repository**: `LOCATION_
   FORWARD` and `_PERM` are originated by the *servant* through
   `Dispatch::redirect` and followed client-side up to `MAX_FORWARD_HOPS = 8`,
   so the transparency the standard's exception serves is held without a chain
   that can raise one.

4. **What is genuinely shared is the discipline, and it was taken on purpose.**
   `Chain::run` calls `before` in registration order, stops at the first
   `Outcome::Refuse`, and calls `after` in reverse over the stages that ran —
   the refuser included, a stage that did not run never — which the module
   names as CORBA's own rule for portable interceptors, and which is why the
   observers sit outermost. Ordered stages under stable names, insertable by a
   deployment. That much is one idea. It is the **mechanism**, not the scope.

5. **And the standard's chain wants a clock, which this stack has refused
   twice.** Interception points are where every ORB measures latency; D004
   fixes the trace record's `ts` as coming from the caller and says *there is
   no clock in the interceptor chain and this decision does not add one*,
   because replay determinism is what makes the ledger evidence. That is not a
   reason the standard is wrong. It is another reason the two are not one chain
   at two scopes.

**Trigger — two, and D018 §3.3's guess is not one of them.** D018 supposed *a
foreign client that expects to register one*. Recorded here as **wrong on
inspection**: portable interceptors are registered in the ORB the registering
program links, so a foreign client registers them in its own ORB and never in
ours. There is no operation by which a peer could ask. The two that replace it:

- **A service context that must survive a reply.** Today an inbound request's
  contexts are parsed and exposed to a servant (`Request::service_contexts`,
  `server.rs:286`) and only `code_sets()` reads them, while a reply's are
  impossible to write and discarded on read. The first peer whose protocol
  needs a context to come *back* — the shape OTS, CSIv2's `SAS` and
  `SendingContextRunTime` all have — fires it. Observable **in this tree,
  against the omniORB and JacORB fixtures the harness already runs**, and it
  needs nobody to name a consumer. Note what it is: a **marshalling** gap that
  the standard happens to expose through interceptors, which is why the sketch
  below answers it without one.
- **The first policy the MCP chain enforces that must also apply to a call not
  made through the bridge.** Today policy coverage and the bridge are the same
  set, so the question never arises. The day they differ, the gate has to move
  onto the request path, and a gate on the request path is a
  `ServerRequestInterceptor` whatever it is called. Observable as a requirement
  on `orbweaver-giop`, in this tree, with nobody named.

**v1 sketch.** Not `PortableInterceptor` — and not `ORBInitializer` either,
which is the second refusal and the one that matters more. Neither trigger's
answer is a port of ch. 21. **For the first**, the answer is not an interceptor
at all: `encode_reply` gains a context list and `decode_reply` keeps one, so a
context we do not understand is **preserved** exactly the way §9.7.2 already
makes us preserve a `TaggedComponent` we do not understand — the same rule
applied to the other carrier, testable against a peer's own bytes rather than
against our own reading of them. **For the second**, one insertion point in
`Dispatch` before the servant, taking the same `CallContext` the MCP chain
takes, so the result is **one chain with two entry points** rather than two
chains that can disagree — §1's and §20's argument about two filters, in a
third place. Registration stays a builder on the `Server`, not an
`ORBInitializer`: the standard's immutable-after-init rule buys a determinism
that our connection-less, clock-less stages already have by construction, so
adopting the ceremony would buy nothing and cost an ORB-lifetime coupling. No
slots — a per-request mutable scratch space is the one addition that would make
a stage stop being a function of its inputs. And **no `ForwardRequest`**, which
is the sharpest refusal here: letting a *gate* originate a forward hands the
policy layer a way to move a call, which is the "gate becomes a caller" that
the connection-less `CallContext` exists to prevent — and `Dispatch::redirect`
already gives that power to the servant, which is the place that should have it.

**How this bears on priority zero (D029 §6).** It closes **none** of §6.1's
five leaks. The one clause that touches the table is `ForwardRequest`, a
location-transparency mechanism, and that transparency is already held without
it. What this chapter *does* owe §6 is a leak that is **not** one of the five,
named here so it is not mistaken for one: whether a call is gated depends on
which API the caller used, because policy coverage tracks the bridge rather
than the ORB. That is a leak in the guardrail, not in the transparency, and it
is the second trigger above.

**요지.** CORBA 3.4 21장. `ORBInitializer`로 **ORB마다** 등록되고 이후 불변인 세
종류 — 클라이언트 요청 인터셉터(5지점), 서버 요청 인터셉터(5지점), 그리고 POA 생성
시점에 IOR에 `TaggedComponent`를 얹는 `IORInterceptor`. 무게를 지는 것은 셋이다:
`RequestInfo`의 **`add_request_service_context`**(OTS의 트랜잭션 전파, CSIv2의 보안
토큰이 타는 길), 요청마다 번호 붙은 **슬롯**을 주는 `PortableInterceptor::Current`,
그리고 서번트가 아니라 **ORB가** 일으키는 `LOCATION_FORWARD`인 **`ForwardRequest`**.
**유예 사유는 D018 §3.3의 물음에 답이 있고 그 답이 "아니오"라는 것이다.** 우리 체인은
스스로를 *"이 호출 경로에 맞춘 CORBA 포터블 인터셉터 형태"*라 적고 `send_request`/
`receive_request`가 `before`로, `send_reply`/`send_exception`이 `after`로 접힌 이유를
*"이 체인은 한 호출의 한쪽에 앉아 있기 때문"*이라 말한다. 정직한 판정은 **둘이 규율을
공유하고, 체인의 용도를 정하는 두 가지 — 무엇을 읽을 수 있는가와 무엇을 할 수 있는가 —
에서 갈린다**는 것이다. "범위가 다른 같은 생각"은 틀린 그림이다. 어느 쪽의 적용 범위도
다른 쪽을 포함하지 않으며, 범위는 **교차한다**. (1) **우리 체인은 ORB의 요청 경로 위에
있지 않다** — `orbweaver-giop`에 인터셉션 자리가 없고(`Invoker`는 훅 없는 세 메서드,
`Dispatch`는 서번트 자신), 체인은 `orbweaver-mcp`의 `Guarded`와 `Bridge::invoke`에서만
들어간다. 즉 `orbweaver_giop`로 곧장 낸 호출은 **어떤 스테이지도 지나지 않는다**. 반대
방향으로는 계약·`ai_authz` 스코프·승인·양쪽 경로의 AnyJSON 인자를 읽는데, 이것들은 IDL
위에 살며 와이어에 닿지 않으므로 어떤 서버 요청 인터셉터도 볼 수 없다. (2) **표준의
체인은 메시지를 고치려고 있고, 우리 것은 고칠 수 없게 지어져 있다** — 임의의 서비스
컨텍스트를 요청에 붙이는 **API가 없고**(유일한 프로덕션 기록자는 코드셋 갈래),
`encode_reply`는 빈 목록을 못 박아 쓰며 들어온 리플라이의 컨텍스트는 읽고 버린다.
그 위에서 `CallContext`는 **일부러 `Connection`을 싣지 않는다**: 다이얼할 수 있는
스테이지는 시간 초과할 수 있어 재현성을 잃고, 보호 대상인 바로 그 타깃에 매달릴 수 있으며,
결정적으로 **보낼 수 있는 스테이지는 자기 게이트를 지나친 호출자**다. 메시지를 못 건드리는
체인은 작은 포터블 인터셉터가 아니라 **다른 물건**이다. (3) 세 종류 중 둘은 대응물이 없고
하나는 여기서 인터셉터의 문제가 아니다 — 컴포넌트는 서번트 쪽에서 발행하고, 슬롯은 계약과
요청의 함수라는 `CallContext`의 설계와 정반대이며(`PICurrent`·`PolicyCurrent`·
`CodecFactory`는 "아무것도 통제하지 않는다"고 스스로 적은 예약 이름 목록의 문자열일
뿐이다), **`ForwardRequest`는 저장소 전체에 0건**이고 포워드는 서번트가
`Dispatch::redirect`로 일으켜 클라이언트가 8홉까지 따라간다. (4) **진짜로 공유하는 것은
규율이고, 그것은 의도적으로 가져온 것이다** — 등록 순서로 들어가 첫 거부에서 멈추고, 실행된
스테이지에 대해서만 역순으로 나오는 규칙을 모듈이 *CORBA 포터블 인터셉터의 규칙*이라 이름
한다. 그것은 **기구**이지 범위가 아니다. (5) 표준의 체인은 **시계**를 원하고 이 스택은 두
번 거부했다 — D004는 `ts`를 호출자에게서 받으며 *체인 안에 시계가 없고 이 결정이 시계를
더하지 않는다*고 적는다. **방아쇠는 둘이며 D018의 추측은 그중 없다** — *등록하려는 외부
클라이언트*는 **검토 결과 틀렸다**: 인터셉터는 등록하는 프로그램이 링크한 ORB에 등록되므로
외부 클라이언트는 자기 ORB에 등록하지 우리 것에 하지 않는다. 대신 (a) **리플라이에서 살아
남아야 하는 서비스 컨텍스트** — 오늘 요청 쪽은 파싱되어 서번트에 노출되지만 리플라이 쪽은
쓸 수 없고 읽고 버려진다. 하네스가 이미 돌리는 omniORB·JacORB 픽스처로 **이 나무 안에서**
관측 가능하며 아무도 지명할 필요가 없다. 정체는 **마샬링** 간극이고, 그래서 아래 스케치는
인터셉터 없이 답한다. (b) **브리지를 거치지 않은 호출에도 적용돼야 하는 첫 정책** — 오늘은
정책 범위와 브리지가 같은 집합이라 물음이 서지 않는다. 갈라지는 날 게이트는 요청 경로로
내려가야 하고, 요청 경로 위의 게이트는 이름이 무엇이든 서버 요청 인터셉터다. **v1은
`PortableInterceptor`가 아니고 `ORBInitializer`도 아니다**(둘째 거부가 더 중요하다).
첫 방아쇠의 답은 인터셉터가 아예 아니다: `encode_reply`가 컨텍스트 목록을 갖고
`decode_reply`가 그것을 보존하여, 이해하지 못하는 컨텍스트를 §9.7.2가 이미 이해하지 못하는
`TaggedComponent`에 요구하는 그대로 **보존**한다 — 같은 규칙을 다른 운반체에 적용한 것이고,
우리 판독이 아니라 **피어의 바이트**로 검증된다. 둘째의 답은 `Dispatch` 안의 삽입 지점
하나이며, MCP 체인이 받는 것과 **같은 `CallContext`**를 받아 **두 체인이 아니라 입구가 둘인
한 체인**으로 남긴다(§1과 §20의 "필터 둘" 논증, 세 번째 자리). 등록은 `ORBInitializer`가
아니라 `Server`의 빌더로 남는다 — 불변성이 사는 결정성을 우리 스테이지는 연결도 시계도 없어
이미 갖고 있으므로, 그 의례는 아무것도 사지 않고 ORB 수명 결합만 치른다. 슬롯 없음. 그리고
**`ForwardRequest` 없음** — 게이트가 포워드를 일으키게 하는 것은 정책 계층에 호출을 옮길
길을 주는 것이고, 연결 없는 `CallContext`가 막으려는 "게이트가 호출자가 된다"가 바로
그것이다. `redirect`가 이미 그 힘을 **서번트**에게 주며, 그쪽이 가져야 할 자리다.
**0순위(D029 §6)와의 관계**: §6.1의 다섯 구멍 중 **아무것도** 막지 않는다. 표에 닿는 유일한
조항인 `ForwardRequest`가 섬기는 투명성은 그것 없이 이미 지켜진다. 이 장이 §6에 빚지는 것은
**다섯에 속하지 않는 구멍** 하나를 이름하는 일이다 — 호출이 게이트를 지나는지가 호출자가 쓴
API에 달려 있다는 것(정책 범위가 ORB가 아니라 브리지를 따라간다). 투명성이 아니라
가드레일의 구멍이며, 위의 둘째 방아쇠가 그것이다.

---

## 22. BiDirectional GIOP — we hold one half of location transparency / 양방향 GIOP

**What it is.** GIOP 1.2's answer to the callback problem. The client side sets
`BiDirPolicy::BidirectionalPolicy` and sends a **`BI_DIR_IIOP` service
context** on a request, carrying a `BiDirIIOPServiceContext` — a sequence of
`ListenPoint {host, port}`. Once it is sent and accepted, the server may issue
**requests** back down the connection the client opened, and the client
dispatches them. Without it a GIOP connection is one-directional at the
*request* level: replies come back, requests do not, and a server that must
call an object the client handed it **dials a fresh connection** to the address
in that object's IOR — which is precisely the address a firewalled or
NAT-ed client does not have.

**Why deferred. Not "no consumer" — the callback is in this tree and it
dials.** The event channel's push side calls `Connection::connect(&job.
consumer, timeout)` (`crates/orbweaver-giop/src/event_server.rs:1645`) and
invokes `push` on it (`:1674`); the module states the shape itself — *this
server acts as a client* (`:208`) — and records which references are dialled
(`:374`: unlike a `PullConsumer`, a `PushConsumer` **is**). Connections are
cached per delivery thread by proxy object key (`:1691`); the inbound
connection is never one of them. So the reverse direction exists as a shape and
is served by dialling, which is exactly the mechanism bidirectional GIOP
replaces. Three reasons it stays deferred anyway:

1. **The reverse direction is refused structurally, not left half-built.** A
   client connection's reader accepts only `Reply` and `CloseConnection` and
   **poisons the connection** on anything else (`lib.rs:2601`, `mux.rs:1343`);
   the server loop answers anything outside `LocateRequest` / `Request` /
   `CancelRequest` / `CloseConnection` / `MessageError` with a `MessageError`
   and closes (`server.rs:1455`). **An inbound `Request` on a client connection
   — the exact traffic this feature creates — is a fault today**, by
   construction. That is the honest measure of the distance, and it is also why
   nothing here is silently half-supporting the feature.
2. **The defect it repairs cannot be produced on this machine.** D015 §3.4,
   checked rather than repeated: *"No docker here, no second host … It cannot
   be closed here."* `spikes/nat_rewrite.sh` measures the forward direction by
   dialling and is explicit at its own site about what it does not show — its
   publish-map assertion notes *"unmeasured here: whether anything answers
   there. That needs the cluster"*, and its Docker and Kubernetes probes are
   counted skips carrying *"it has never executed anywhere — do not read it as
   evidence"*. On one machine on loopback **every callback IOR is dialable**,
   so the leak is invisible by construction rather than by neglect.
3. **The standard's own answer carries a hazard that needs a policy we would
   have to invent.** A client can claim a `ListenPoint` for a host it does not
   own, and a server that believes the claim will send requests intended for
   that host down the claiming client's connection. omniORB and JacORB both
   gate the feature behind an explicit policy for that reason, which means the
   cheap-looking half (decode a context) and the expensive half (trust it) are
   not the same batch.

**Which half of location transparency we hold — against D029 §6.1's Location
row.** R7 rewrites **the server's** IOR so that a client can dial it from where
the client actually is; the target there is the server. Bidirectional GIOP is
the same question with the target being **the client**: the reference the
client handed the server has to be dialable from where the *server* is. So the
forward half is measured and the reverse half is served by dialling and has
never been tested against a peer that cannot be dialled. **Recorded as a
finding and not repaired here**: §6.1's Location row reads on the forward half
only, and the reverse half is not named in it — a decision's facts live in the
decision, so this file names the gap rather than editing it.

**Trigger — two, and the first is unreachable in this tree.**

- **Nominal, and it cannot fire here.** *A consumer whose callbacks cannot be
  dialled.* D018 §3.3 supposed this and named D015 §3.4 as the reason it cannot
  be measured; **checked, and the claim holds** — the missing fixture is a NAT
  between two hosts, §3.4 is class B with fixtures absent, and it says in its
  own words that it cannot be closed here. Recorded as observable in principle
  and **unreachable in practice**, at the site, on §20's precedent: that is
  worth more than a trigger that reads well and cannot fire. This is the class
  D023 §2 diagnoses — every deferral's trigger has a subject outside this
  project — and the door it proposes, *the owner naming a consumer fires a
  trigger*, is **PROPOSED and not approved**. It is cited here and relied on by
  neither of this chapter's triggers, because the second one needs nobody.
- **Sharper, in this tree, and it needs nobody.** *The first callback consumer
  that cannot listen at all* — not a firewalled endpoint, an endpoint with no
  server of its own. Every consumer this project has ever pushed to runs one:
  `spikes/event_consumer.py` is an omniORB `PushConsumer` servant with its own
  RootPOA and POAManager. D015 §3.5 records the one process class here that
  cannot — *"Python is clients only"*. Observable as the first
  `connect_push_consumer` whose caller has no POA to put a servant in.

**And that second trigger has two answers, of which this chapter is the
second-best — so this deferral gets *stronger* with time.** D030 L1 gives a
non-Rust process a servant seam: a listener and a POA. If L1 lands, a Python
consumer can be dialled and this trigger never fires for the only subject it
has, leaving only the nominal trigger, which is unreachable here. Bidirectional
GIOP would instead let a client be a target **with no listener at all** — a
different answer, cheaper for the consumer and dearer for everyone's trust
model. A deferral that ages into a firmer deferral is unusual enough to write
down, because the reflex on re-measurement is to read a still-unfired trigger
as evidence the chapter is owed sooner.

**v1 sketch.** Not `BiDirPolicy`, and — the load-bearing refusal —
**not bidirectionality as a mode**. Two steps, in this order, because the first
is worth having whether or not the second is ever built.

(a) **Decode and record; never act.** The `BI_DIR_IIOP` context read off an
inbound request, named as an id beside `SERVICE_ID_CODE_SETS` and
`SERVICE_ID_SAS`, and surfaced through the seam that already exists
(`Request::service_contexts`, `server.rs:286`), so a capture from a peer that
sends one is readable rather than opaque. This is the same *preserve what we do
not understand* rule §9.7.2 already imposes on a `TaggedComponent`, and it is
§21's first trigger seen from the other end — which is the argument for the two
chapters sharing a section and not for them being one chapter.

(b) **Reverse dispatch only behind an explicit per-connection opt-in**, naming
which listen points are accepted and refusing any that the connection's own
peer address does not corroborate. The threshold is a policy an operator has
and the crate must not choose — `SEAT_QUOTA`'s line, in a second place. What it
must not become is a global mode: the one-direction message guards in
`lib.rs:2601` and `server.rs:1455` are a **correctness** property today, and
relaxing them everywhere would trade a loud fault for a silent acceptance.

**How this bears on priority zero (D029 §6).** It would close the reverse half
of §6.1's **Location** row — *the caller must not be able to tell where the
target runs* — for the case where the target is the process that opened the
connection. **It does not follow that it should be built**: that transparency
leaks only for a target that cannot be dialled, and D030 L1 closes exactly that
for the only such target this tree has. A leak with a cheaper closure already
proposed is not an argument for the dearer one.

**요지.** GIOP 1.2가 콜백 문제에 내놓은 답. 클라이언트가
`BiDirPolicy::BidirectionalPolicy`를 걸고 요청에 **`BI_DIR_IIOP` 서비스 컨텍스트**를
실어 `ListenPoint {host, port}`의 시퀀스를 보내면, 그 뒤로 서버는 **클라이언트가 연
연결로 요청을 되쏠 수** 있다. 그것이 없으면 GIOP 연결은 *요청* 수준에서 단방향이다 —
리플라이는 돌아오지만 요청은 가지 않으며, 클라이언트가 건넨 객체를 불러야 하는 서버는
그 객체 IOR의 주소로 **새 연결을 건다**. 방화벽·NAT 뒤 클라이언트에게 없는 바로 그
주소다. **유예 사유는 "소비자 없음"이 아니다 — 콜백은 이 나무 안에 있고 다이얼한다**:
이벤트 채널의 push 쪽은 `Connection::connect(&job.consumer, …)`
(`event_server.rs:1645`)로 걸고 `push`를 부르며(`:1674`), 모듈이 스스로 *이 서버는
클라이언트로 행동한다*(`:208`)고 적고 어떤 참조가 다이얼되는지 기록한다(`:374` —
`PullConsumer`와 달리 `PushConsumer`는 **다이얼된다**). 연결은 배달 스레드마다 프록시
오브젝트 키로 캐시되며(`:1691`) 들어온 연결은 결코 그중에 없다. 그래도 유예하는 이유
셋: (1) **역방향은 반쯤 지어진 것이 아니라 구조적으로 거부된다** — 클라이언트 연결의
리더는 `Reply`와 `CloseConnection`만 받고 나머지에 **연결을 오염**시키며
(`lib.rs:2601`, `mux.rs:1343`), 서버 루프는 다섯 종류 밖의 메시지에 `MessageError`로
답하고 닫는다(`server.rs:1455`). **클라이언트 연결로 들어온 `Request` — 이 기능이 만드는
바로 그 트래픽 — 는 오늘 결함이다.** 거리를 재는 정직한 자이며, 아무것도 조용히 반쯤
지원하고 있지 않다는 뜻이기도 하다. (2) **고치려는 결함을 이 기계에서 만들 수 없다** —
D015 §3.4를 되풀이하지 않고 확인했다: *"여기엔 도커도 두 번째 호스트도 없다 … 여기서
닫을 수 없다."* `spikes/nat_rewrite.sh`는 정방향을 다이얼로 재고 무엇을 못 보이는지
자기 자리에서 밝힌다 — publish-map 단언은 *"여기서 측정되지 않음: 거기서 무언가 답하는지.
그건 클러스터가 필요하다"*라 적고, 도커·쿠버네티스 프로브는 *"어디서도 실행된 적 없다 —
증거로 읽지 말 것"*을 단 skip이다. 한 기계 루프백에서는 **모든 콜백 IOR이 다이얼 가능**
하므로 구멍은 방치가 아니라 구성상 보이지 않는다. (3) **표준의 답 자체가 우리가 발명해야
할 정책을 요구하는 위험을 진다** — 클라이언트는 자기 것이 아닌 호스트의 `ListenPoint`를
주장할 수 있고, 그 주장을 믿는 서버는 그 호스트로 갈 요청을 주장한 클라이언트의 연결로
보낸다. omniORB와 JacORB가 둘 다 명시적 정책 뒤에 두는 이유이며, 값싸 보이는 절반(컨텍스트
해독)과 비싼 절반(그것을 신뢰)이 같은 배치가 아니라는 뜻이다. **우리가 쥔 위치 투명성의
절반은 어느 쪽인가 — D029 §6.1의 Location 행에 대고.** R7은 **서버의** IOR을 고쳐
클라이언트가 실제로 있는 자리에서 걸 수 있게 한다(대상이 서버다). 양방향 GIOP은 대상이
**클라이언트**인 같은 물음이다 — 클라이언트가 건넨 참조가 *서버가 있는 자리*에서 걸려야
한다. 즉 정방향 절반은 측정되었고, 역방향 절반은 다이얼로 서빙되며 걸 수 없는 피어를 상대로
시험된 적이 없다. **고치지 않고 발견으로 기록한다**: §6.1의 Location 행은 정방향 절반만
읽고 있고 역방향 절반은 그 행에 이름이 없다 — 결정의 사실은 결정에 살므로, 이 파일은 간극을
이름할 뿐 고쳐 쓰지 않는다. **방아쇠는 둘이고 첫째는 이 나무에서 당겨질 수 없다.** 첫째
(명목): *콜백을 걸 수 없는 소비자*. D018 §3.3이 그렇게 추측하며 D015 §3.4를 이유로 들었고,
**확인했고 그 주장은 성립한다** — 없는 픽스처는 두 호스트 사이의 NAT이고, §3.4는 픽스처가
없는 class B이며 여기서 닫을 수 없다고 스스로 적는다. §20의 선례대로 **원리상 관측 가능,
실무상 도달 불가**라고 자리에서 적는다. 이는 D023 §2가 진단한 부류다 — 모든 유예의
방아쇠는 주체가 이 프로젝트 밖에 있다 — 그리고 그것이 제안하는 문(*소유자가 소비자를
지명하면 방아쇠가 당겨진다*)은 **승인이 아니라 제안**이다. 여기서는 인용할 뿐이며 이
장의 두 방아쇠 어느 쪽도 그것에 기대지 않는다. 둘째는 아무도 필요 없기 때문이다. 둘째(더 날카롭고, 나무 안이며, 아무도 필요 없다):
*아예 들을 수 없는 첫 콜백 소비자* — 방화벽 뒤가 아니라 **자기 서버가 없는** 종단점. 지금까지
push한 소비자는 전부 서버를 돌린다(`spikes/event_consumer.py`는 자기 RootPOA와 POAManager를
가진 omniORB `PushConsumer` 서번트다). 그렇게 할 수 없는 유일한 프로세스 부류를 D015 §3.5가
적는다 — *"파이썬은 클라이언트 전용"*. 서번트를 놓을 POA가 없는 호출자의 첫
`connect_push_consumer`로 관측된다. **그리고 그 둘째 방아쇠에는 답이 둘 있고 이 장은 차선이며,
따라서 이 유예는 시간이 갈수록 *강해진다*.** D030 L1은 비-Rust 프로세스에 서번트 자리(리스너와
POA)를 준다. L1이 착지하면 파이썬 소비자는 걸 수 있게 되고 이 방아쇠는 자기 유일한 주체를
잃어 명목 방아쇠만 남으며, 그것은 여기서 도달 불가다. 양방향 GIOP은 대신 **리스너 없이**
클라이언트를 대상이 되게 한다 — 소비자에게 싸고 모두의 신뢰 모형에 비싼 다른 답이다. 재측정
때의 반사는 아직 당겨지지 않은 방아쇠를 "더 빨리 빚졌다"는 증거로 읽는 것이므로, 굳어지는
유예는 적어 둘 값이 있다. **v1은 `BiDirPolicy`가 아니며, 무게를 지는 거부는 **양방향을
모드로 만들지 않는다**는 것이다.** 두 단계이고 순서가 있다. (a) **해독하고 기록하되 결코
행동하지 않는다** — 들어온 요청에서 `BI_DIR_IIOP` 컨텍스트를 읽어 `SERVICE_ID_CODE_SETS`·
`SERVICE_ID_SAS` 옆에 이름을 주고 이미 있는 자리(`Request::service_contexts`,
`server.rs:286`)로 노출하여, 그것을 보내는 피어의 캡처가 불투명이 아니라 읽히게 한다.
§9.7.2가 이해 못 하는 `TaggedComponent`에 이미 부과하는 *이해 못 하는 것을 보존하라*와 같은
규칙이고, §21의 첫 방아쇠를 반대편에서 본 것이다 — 두 장이 한 절을 쓰는 근거이지 한 장이 될
근거는 아니다. (b) **역방향 디스패치는 연결마다 명시적 옵트인 뒤에만** — 어떤 listen point를
받아들이는지 이름하고, 연결 자신의 피어 주소가 뒷받침하지 않는 것은 거부한다. 그 문턱은
운영자가 가진 정책이고 크레이트가 골라서는 안 된다(`SEAT_QUOTA`의 선을 두 번째 자리에서).
**전역 모드가 되어서는 안 된다**: `lib.rs:2601`과 `server.rs:1455`의 단방향 가드는 오늘
**정확성** 성질이며, 그것을 전부 풀면 시끄러운 결함을 조용한 수용과 맞바꾸게 된다.
**0순위(D029 §6)와의 관계**: §6.1 **Location** 행의 역방향 절반 — *호출자는 대상이 어디서
도는지 알 수 없어야 한다* — 을, 대상이 연결을 연 프로세스인 경우에 대해 막는다. **그렇다고
지어야 한다는 결론은 나오지 않는다**: 그 투명성은 걸 수 없는 대상에 대해서만 새고, 이 나무에
있는 유일한 그런 대상에 대해서는 D030 L1이 더 싸게 막는다. 더 싼 닫음이 이미 제안된 구멍은
더 비싼 닫음의 근거가 아니다.

---

## 9. How a chapter graduates / 한 장이 졸업하는 법

A chapter leaves this file when its trigger fires — **the trigger, as written,
observed**, not a persuasive case that it nearly fired. Graduation is one
change: the chapter moves into `PLAN-SERVICES.md` with the two things this file
deliberately withholds, a **batch unit** and a **named oracle**, and the
fixture is probed before the batch is planned (probe first, quote the measured
output, a BLOCKED probe is a valid result — the sslTP and omnievents
precedent). If a trigger fires and the chapter's v1 sketch turns out wrong, the
sketch is corrected **in the same change** that supersedes it, with the reason,
so this file records what the trigger actually taught rather than quietly
losing the disagreement. A sketch that was wrong and is recorded as wrong is
worth more than one that was never tested.

한 장은 **적힌 그대로의 방아쇠가 관측될 때** 이 파일을 떠난다("거의 당겨졌다"는
논증이 아니라). 졸업은 한 번의 변경이다: 이 파일이 일부러 보류한 두 가지 —
**배치 단위**와 **이름 붙은 오라클** — 을 갖고 `PLAN-SERVICES.md`로 이동하며,
배치 계획 전에 픽스처를 먼저 프로브한다(측정 출력을 인용하고, BLOCKED도 유효한
결과다). 방아쇠가 당겨졌는데 스케치가 틀렸다면, 그것을 대체하는 **같은 변경
안에서** 사유와 함께 고친다 — 틀렸고 틀렸다고 기록된 스케치는 한 번도 시험되지
않은 스케치보다 가치 있다.
