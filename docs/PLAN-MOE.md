# PLAN-MOE — the MoE control plane as Orbweaver's first application domain

> Supplement to `PLAN.md` §7, reviewing and adopting
> [`CORBAMoEArchitecture.md`](CORBAMoEArchitecture.md) (2026-08-14).
> `PLAN.md` §7.3의 보완 문서 — MoE 컨트롤 플레인 명세의 검토·채택 기록.

## 1. Review verdict / 검토 판정

The source document is sound where it matters most: the control-plane /
data-plane split (§1.2), expert-granularity objectification (§1.1), and the
residency state machine driven by batch-period statistics rather than tokens
(§5) are exactly the shape a CORBA substrate can carry. **Adopted as the
blueprint for stream F**, with the overrides and corrections below.

원문은 가장 중요한 지점 — 컨트롤/데이터 플레인 분리, expert 입도의 객체화, 토큰이
아닌 배치 주기 통계로 구동되는 상주 상태 머신 — 에서 건전하다. 아래의 재정의·수정과
함께 **스트림 F의 청사진으로 채택**한다.

### Overrides / 재정의 (원문 §12.1)

The document recommends Cap'n Proto or gRPC over implementing GIOP/CDR,
because "reviving IIOP is a toolchain burden". **For this project that
recommendation is void, and measurement says so**: the burden is already paid
— a from-scratch MIT GIOP/CDR core interoperating with omniORB and JacORB in
both directions at three protocol versions, with capability-secure references
the document's own §11 asks for ("IOR 위조 방지") already built at the MCP
boundary. The substrate is `orbweaver-giop` (+ `ssliop`), and the "MCP face"
the document sketches **already exists as `orbweaver-mcp`** — including the
"MCP 툴 핸들 ↔ IOR 바인딩" line, which is precisely our `CapabilityTable`,
with session scoping and expiry the source document does not specify.

원문의 "GIOP 재구현 대신 Cap'n Proto/gRPC" 권고는 이 프로젝트에서 무효다 — 그
비용은 이미 지불되었고 두 독립 ORB로 측정되었다. 원문이 §11에서 요구하는 IOR 위조
방지도 능력 핸들로 이미 존재하며, 세션 종속·만료는 원문에 없는 강화다.

### Corrections found by our own gate / 우리 게이트가 찾은 원문 결함

Extracting the document's IDL (§7, §9.5) into `corpus/moe/` and running it
through S4, omniidl and JacORB's compiler found **the document's contracts do
not compile as written** — two causes:

1. **`Context` is an OMG IDL keyword** (the `context` clause). All three front
   ends reject it. Renamed `CallContext`.
2. **`Residency residency` is the case-insensitive identifier clash** that is
   this project's codified dominant failure — the rule caught an external
   architecture document exactly as it caught our own generators. Member
   renamed `state`.

After the two renames (plus keyword-safe operation names — `register_expert`,
`clone_model`, `get_manifest`, `get_tenant_id`): **S4 0 rejected, omniidl ok,
JacORB ok — measured 2026-08-14.** The corrected files live in `corpus/moe/`
with the findings in their headers; stream F batch 1 promotes them to
`corpus/golden/` and adds negative-corpus cases from the original defects.

원문 IDL은 그대로는 컴파일되지 않는다 — `Context`는 예약어, `residency`는 이
프로젝트가 성문화한 대소문자 무시 충돌. 수정 후 세 프론트엔드 모두 수용(실측).

## 2. What already exists / 이미 있는 것

| MoE 문서 개념 | Orbweaver 실물 | 상태 |
|---|---|---|
| Expert = 분산 객체 + IOR | `orbweaver-object` (references as values, `_is_a`) | ✅ 측정됨 |
| POA ServantActivator 적재기 | `Poa` + `ServantLocator` (`Located::{Here,Forward,Unknown}`) + `residency::ExpertLoader` | ✅ — 상태 머신 F3 착지 |
| Interface Repository | `orbweaver-registry` + SIDL annotations (`ai_desc`… ≈ capability contract) | ✅ |
| MCP 얼굴 + 핸들↔IOR 바인딩 | `orbweaver-mcp` triad + `CapabilityTable` | ✅, 원문보다 강함 |
| PolicyDomain (인가·레지던시·감사) | `Exposure` + `Delegation` + `Caller`/`ai_authz` + audit lines | ◐ — residency 제약만 없음 |
| 테넌트 격리 | per-session capability tables + default-deny exposure | ◐ — Naming/Trading 도메인 스코프는 F5 |
| route_freq 텔레메트리 | `promote::CallStats` — 승격 엔진이 곧 적재 정책의 사촌 | ◐ — 재사용 지점 |
| 인터셉터 체인 | guard의 검사 순서 (정렬은 있으나 공식 체인 아님) | ◐ — F4 |
| Naming | CosNaming client + corbaname | ✅ |
| 언어별 스텁 (원문 §10) | `orbweaver-gen` Rust 백엔드, 오라클 정적=동적 | ◐ — 타 언어는 스트림 B |

## 3. Stream F — MoE control plane / 스트림 F

Follows §7.3's format: every batch has a unit and a deterministic oracle. The
data plane stays out of CORBA permanently (원문 철칙 1 그대로); hit-rate and
latency claims are oracle-checked against **deterministic routing traces**, not
against live accelerators this repo does not have — stated now so nobody
reports an unmeasurable number later.

- **F1 — contracts into the corpus.** ✅ *landed 2026-08-14: golden 22/23, negative n11/n12, a fourth divergences entry, and a generator-hygiene bug (closure shadowed by a parameter named `e`) caught and fixed.* Unit: `corpus/moe/*.idl` → golden corpus
  + negative cases minted from the original `Context`/`residency` defects.
  Oracle: S4 + differential (three front ends) + gen-corpus compile.
  *Status: validation half already measured green (above).*
- **F2 — Trading Service** ✅ *decision engine landed 2026-08-14 (`orbweaver-trading`, 37 tests); the wire surface followed on 2026-08-15 as the project contract — `orbweaver-object::expert_service` serves `moe::ExpertRegistry`/`ExpertLoader` from `corpus/golden/22`, with `apply_policy` as the one place the store and F3's machine meet. The standard `CosTrading::Lookup` facade stays deferred: PLAN-SERVICES §3.* Original scope: Offer store (Capability
  properties), constraint-query subset (`specialization == 'math' AND
  latency_p99 < 200`), deterministic ordering. Oracle: property-query tests
  over fixture offers; the §6 loading policy (`score = route_freq ×
  affinity ÷ mem_footprint`, watermarks, LFU eviction) replayed over recorded
  traces with pinned outcomes.
- **F3 — residency state machine on the POA.** ✅ *landed 2026-08-14:
  `orbweaver-object::residency`, 20 tests (34 in the crate) — the 4×5
  transition table pinned answer by answer, each of the guard's four
  conditions proven necessary by flipping it alone, PERSISTENT state surviving
  an evict/reload cycle, and a two-window `Decision` list applied end to end
  against the real §6 policy.* `ExpertLoader` over
  `ServantLocator`; transitions only OFFLOADED→PREFETCHING→RESIDENT(→ACTIVE
  marker)→OFFLOADED; **no token-period transitions by construction** (the API
  simply has no per-call hook — held by a `compile_fail` doc test, not only by
  documentation). Oracle: transition tests incl. the eviction
  guard (`inflight == 0`), PERSISTENT state preservation.
  Two decisions worth their own line. **A request for an OFFLOADED expert is
  refused (`OBJECT_NOT_EXIST`), never served by a synchronous load** —
  demand-loading inside `locate` is precisely the latency §11 has prefetch
  exist to hide, and it would hold a dispatch thread for the whole copy; the
  miss instead requests a prefetch that the *next* window completes. And
  `Located::Here` activates an id permanently, so eviction has to be
  reconciled onto the POA (`ExpertLoader::reconcile`) or an evicted expert
  keeps being served out of the active map.
- **F4 — the interceptor chain, formalized.** The guard's checks become an
  ordered, extensible chain: authn → quota → safety → telemetry → audit (원문
  §4.5 순서), with telemetry feeding `CallStats`/`route_freq`. Oracle: order
  pinned by tests; every existing guard test must pass unchanged.
- **F5 — enterprise composition.** `ComposedModel`/`ModelFactory` over the
  registry; tenancy = per-domain `Exposure` + capability tables; residency
  constraint joins `Delegation::decide`. Oracle: cross-tenant invisibility
  tests in the exact shape of the existing cross-session handle tests;
  `ModelFactory.retire` is `ai_effect: destructive` and needs approval.

Integration points (§7.4 style): **IF1** F2's selection must return capability
handles at the MCP face, never IORs (transcript-leak test reused). **IF2**
F4's telemetry and stream B's promotion stats are one store, not two.

## 4. Core CORBA services coverage / CORBA 필수 서비스 커버리지

> Deepened into a dedicated suite plan: [`PLAN-SERVICES.md`](PLAN-SERVICES.md)
> (2026-08-14). The table below is the audit snapshot; the suite plan is the
> living document. F6 landed 2026-08-14 with both oracle directions measured.
> 이 표는 실사 시점의 스냅숏이며, 서비스 스위트의 살아있는 계획은
> `PLAN-SERVICES.md`다. F6은 양방향 오라클 실측과 함께 착지했다.

Requested review (2026-08-14): does the plan actually cover the classic
service suite the architecture leans on? Audit of PLAN v0.6 + this supplement:

| Service | Today | Plan home | Gap closed by |
|---|---|---|---|
| **CosNaming** | client only (`naming.rs`, resolves against omniNames) | PLAN §4.4 | **F6 (new): first-party Naming *server*** — bind/rebind/resolve/unbind/list on our POA; oracle: omniORB's client resolving against **our** server, and ours against omniNames — both directions, like every other interop claim |
| **Trading** | none | **F2** (this supplement) | offer store + constraint queries + §6 loading policy |
| **Event / Notification** | **absent from every plan document — the review's finding** | **F7 (new): minimal CosEventChannel** — push-style supplier/consumer on our wire, at the granularity the control plane needs (residency transitions, telemetry batches; never per token); oracle: two-process channel with an omniORB-side consumer |
| **LifeCycle** | none | **F5** (`ModelFactory` = GenericFactory pattern) |
| **Property** | none | folded into **F5** (per-tenant `Manifest`/config; a separate CosProperty server is not justified before a second consumer exists) |
| **Security** | CSIv2 wire + delegation + hygiene (PHASE5), guard chain (F4) | PLAN §4.8 | — |
| **Transaction (OTS)** | none | **declared out of scope**: distributed transactions across legacy ORBs are a graveyard of partial implementations, nothing in the control plane needs atomicity across objects, and an honest absence beats a decorative `Current` interface |
| **Time / PSS / Concurrency** | none | out of scope, same reasoning — adopt only when a consumer names a concrete need |

F6/F7 follow the stream rules: one capability × both peers × measured both
directions. 요청 검토의 결론: Naming은 서버 절반이, Event는 전부가 계획에
없었다 — F6·F7로 편입하고, Transaction·Time·PSS는 사유와 함께 명시적 제외로
선언한다. 장식용 인터페이스보다 정직한 부재가 낫다.

## 4.5 The Capability gap, and what closing it would cost / 계약 확장 비용

`moe::Capability` carries no `specialization` and no `latency_p50`, so
`orbweaver-trading`'s constraint queries — the §4.3 half of the control plane —
cannot be satisfied on either field by an expert that registered over the wire.
The Trading batch reported this as a gap. Measuring it found something sharper:
as placeholders, the empty string only lost matches while the **zero won them**,
because `latency_p50 < 20` is satisfied by an unmeasured latency. A router
selecting on latency preferred exactly the experts nobody had timed.

The immediate fix is not a contract change: both fields are now `Option`, a
comparison over an absent field is *unanswerable* rather than false, and an
unknown value sorts after every known one so "the fastest" is never the one
nobody measured.

Extending the contract instead was measured with our own tool rather than
assumed:

```
$ idl-diff corpus/golden/22-moe-control-plane.idl 22-with-the-two-members.idl
[BREAKING] IDL:moe/Capability:1.0: member "specialization" added — a CDR member
           has no tag and no length, so an added one is read as part of
           whatever followed it
[BREAKING] IDL:moe/Capability:1.0: member "latency_p50_ms" added
refused: 2 change(s) break deployed peers        (exit 1)
```

So the two members cannot be added in place: §5.3 requires a new version of the
type or an explicit `--approve` with a recorded reason. That is the right price
and it is not yet worth paying — nothing outside this repository serves
`moe::Capability`, but the same is true of every contract on the day before
someone deploys it, and a project that edits released types when it is
convenient has no §5.3 at all. **The unknown-aware query is the answer until a
version bump has a reason of its own.**

`moe::Capability`에 두 멤버를 넣는 것은 우리 도구가 **BREAKING으로 거부**한다.
지금 값을 치를 이유가 없으므로, 답은 계약 확장이 아니라 미지값을 아는 질의다.
편할 때 released 타입을 고치는 프로젝트에는 §5.3이 아예 없는 것과 같다.

## 5. What this supplement does not claim / 이 보완이 주장하지 않는 것

No accelerator, no fused kernel, no RDMA exists in this repository; the data
plane remains a named external. `Expert.process` payloads cross as references
per the source document's own rule — AnyJSON already refuses to inline what a
handle should carry. Performance targets (§11의 <5% 오버헤드, p99 은닉) become
measurable only when a data-plane simulator exists; until then stream F
reports state-machine and policy correctness, nothing about latency.
