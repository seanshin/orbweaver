# PLAN-DEFERRED — the excluded services, designed enough to resume

> Companion to [`PLAN-SERVICES.md`](PLAN-SERVICES.md) §8 (Exclusions) and
> [`PLAN-MOE.md`](PLAN-MOE.md) §4. Written 2026-08-13.
> `PLAN-SERVICES.md` §8의 제외 표를 **각 항목의 설계 스케치**로 펼친 문서.
> 제외가 "잊었다"가 아니라 "재개할 만큼은 설계해 두었다"를 뜻하게 하는 것이 목적.

## 0. What this document is / 이 문서의 성격

PLAN-SERVICES §8 excludes seven service areas in four table rows. A table row
is enough to record a decision and **not** enough to resume from: a future
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
| §10 CosEvent supplier-side pull | a named `PullSupplier` in this workspace — something that *is* one, whose clock the channel would then hold a thread on |
| §11 CosEvent `destroy` | a caller model in the event servant — the operation is unauthenticated and ends the channel for every other client |
| §12 CosNaming chaining to a foreign context | a federation requirement (§7's trigger, seen from the naming servant) — chaining is *possible* today, which is a reason it can be built, not a reason to |

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
honest answer. Today's largest set is a corpus of roughly thirty interfaces.

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

**What PHASE5 has today** (from `COMPONENTS.md`, measured): CSIv2 wire — SAS
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
per domain — the mechanism exists, the *noun* is what is missing. (2) `Caller`
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

**Un-defer trigger.** A named `PullSupplier` in this workspace. **v1 sketch:**
one thread per connected supplier with a per-supplier deadline the channel
owns; a supplier that blocks past it is disconnected with the same
`disconnected_for_failure` accounting the push path has.

**무엇.** 채널이 공급자에서 *당기는* 쪽. 소비자 쪽 pull은 2026-08-18부터 서빙.
**왜 유예.** 원래 사유는 두 주장이었고 하나만 측정을 견뎠다; 공급자 쪽은 채널이
남의 시계에 스레드를 하나씩 붙잡는 일이며 이 작업공간에 `PullSupplier`인 것이
없다. **방아쇠.** 이름 붙은 `PullSupplier` 하나.

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
