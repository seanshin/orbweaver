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
JacORB ok — measured 2026-08-14.** The corrected files were staged in
`corpus/moe/` with the findings in their headers, and **stream F batch 1 landed
them the same day** (`f4b3868`): they are now
`corpus/golden/22-moe-control-plane.idl` and
`corpus/golden/23-moe-enterprise.idl`, their headers carrying the provenance the
side directory used to hold, and the two original defects were minted as
negative corpus — `corpus/negative/n11-keyword-context.idl` (`struct Context`,
the OMG keyword) and `corpus/negative/n12-enum-member-case-clash.idl`
(`Residency residency`). **`corpus/moe/` no longer exists**: the corpus is the
home, and a side directory would rot. *This paragraph said the files "live in
`corpus/moe/`" and that batch 1 "promotes them" — present and future tense
about a directory deleted by the commit that closed the batch.*

원문 IDL은 그대로는 컴파일되지 않는다 — `Context`는 예약어, `residency`는 이
프로젝트가 성문화한 대소문자 무시 충돌. 수정 후 세 프론트엔드 모두 수용(실측).
수정본은 `f4b3868`으로 착지했다 — golden 22·23, 원래 결함 둘은 negative
n11·n12, `corpus/moe/`는 삭제. 코퍼스가 집이고 곁가지 디렉터리는 썩는다.
*이 문단은 그 디렉터리가 아직 있고 승격이 아직 남은 것처럼 적혀 있었다.*

## 2. What already exists / 이미 있는 것

| MoE 문서 개념 / Concept in the source document | Orbweaver 실물 / What exists here | 상태 / Status |
|---|---|---|
| Expert = 분산 객체 + IOR | `orbweaver-object` (references as values, `_is_a`) | ✅ 측정됨 |
| POA ServantActivator 적재기 | `Poa` + `ServantLocator` (`Located::{Here,Forward,Unknown}`) + `residency::ExpertLoader` | ✅ — 상태 머신 F3 착지 |
| Interface Repository | `orbweaver-registry` + SIDL annotations (`ai_desc`… ≈ capability contract) | ✅ |
| MCP 얼굴 + 핸들↔IOR 바인딩 | `orbweaver-mcp` triad + `CapabilityTable` | ✅, 원문보다 강함 |
| PolicyDomain (인가·레지던시·감사) | `Exposure` + `Delegation` + `Caller`/`ai_authz` + audit lines; 계약의 `PolicyDomain`은 선언 오퍼레이션 **셋**(`authorize`, `check_residency`, `audit`)이고 셋 다 `tenant_service.rs`가 서빙한다. 16/16은 golden 23 **계약 전체**의 수치다 — 다섯 인터페이스(`Expert` 2, `EnterpriseExpert` 3, `PolicyDomain` 3, `ComposedModel` 4, `ModelFactory` 4), `SERVICES-COVERAGE` §7 | ◐ — `Delegation::decide`의 residency 항만 없음 |
| 테넌트 격리 | per-session capability tables + default-deny exposure + F5 `tenant_service.rs`(2026-08-14) | ✅ — F5가 *인가* 모양을 택했다(한 그래프, 테넌트별 키); 도메인 모양은 PLAN-DEFERRED §7의 방아쇠로 남음 |
| route_freq 텔레메트리 | `promote::CallStats` — 승격 엔진이 곧 적재 정책의 사촌 | ◐ — IF2 재사용은 착지(`Bridge::invoke` → `CallStats`, F4의 텔레메트리 인터셉터); MoE 오퍼 저장소의 `route_freq`는 여전히 자체 카운터이고 `CallStats`가 먹이지 않는다 |
| 인터셉터 체인 | `interceptor::Chain::standard` — 등록 순서 audit → telemetry → exposure → scopes → approval. 등록 순서는 행위 순서가 아니다: 게이트는 들어가며 §4.5의 1→2→3(exposure·scopes·approval), 관측자는 나오며 4→5(telemetry, 마지막 말은 audit). 이름은 있으나 표준 스택이 채우지 않는 좌석 셋 — `SEAT_QUOTA`는 점유자가 **있고**(`quota::Quota`, `Chain::quota`가 설치) 기본값으로 넣지 않는다: 한계값은 운영자만 가진 숫자이며 스택이 고를 수 있는 두 숫자가 다 틀리다(무제한은 거부하지 않는 단계, 0은 아무것도 답하지 않는 다리). `SEAT_SAFETY_CONTENT`는 점유자를 **싣지 않는다** — 계약을 읽는 절반은 `STAGE_APPROVAL`이 채우고, 인자를 읽는 절반의 *규칙*은 배포자의 것이다. `SEAT_EXPIRY`도 같은 이유로 `Chain::expiry`(이 크레이트에는 시계가 없다) | ✅ — F4 착지 2026-08-14; 순서는 `the_standard_stack_registers_every_named_stage_in_order`가 고정한다. 이 칸은 `exposure → scopes → quota → approval → safety → telemetry → audit`라고 적혀 있었다 — 표준 스택에 없는 두 좌석을 점유자로 세고, 관측자 둘을 게이트 안쪽에 두었다 |
| Naming | CosNaming client + corbaname | ✅ |
| 언어별 스텁 (원문 §10) | `orbweaver-gen` Rust 백엔드(스텁 + 스켈레톤), 오라클 정적=동적 · **두 번째 타깃 착지**: Python 클라이언트 — `src/python.rs`, `gen-python` 바이너리, `tests/python_target.rs`(생성 코드를 실제로 **실행**해 golden 코퍼스 전체에서 Rust 매핑과 대조, 양쪽 바이트 순서). 씸은 D007(**승인됨** 2026-08-18, v1은 A안: AnyJSON을 말하는 로컬 브리지 프로세스, 새 의존성 없음) | ◐ — Python은 **클라이언트 전용**이다(모듈 헤더의 명시적 범위 경계: Python 서번트는 브리지가 역방향으로 Python을 호출해야 하고, 서빙 절반은 Rust `skeleton`이 이미 답한다). 남은 ◐는 그 외 언어이며, 여기에는 "타 언어는 스트림 B"라고만 적혀 있었다 |

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
  `orbweaver-object::residency`, 20 tests (34 in the crate **on that day** —
  a crate-wide figure inside a module record, which is the half that rots: 84
  today) — the 4×5
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
- **F4 — the interceptor chain, formalized.** ✅ *landed 2026-08-14
  (`crates/orbweaver-mcp/src/interceptor.rs`).* The guard's checks became an
  ordered, extensible chain. Planned scope was §4.5's numbering — authn → quota
  → safety → telemetry → audit — and what `Chain::standard` registers is
  **audit → telemetry → exposure → scopes → approval**, because registration
  order is not acting order: the gates act on the way *in* in §4.5's order
  1 → 2 → 3, the observers act on the way *out* in order 4 → 5, so an observer
  that must see every call — including a refused one — has to be registered
  outside every gate. Two of the five §4.5 numbers are **named empty seats** in
  the standard stack rather than occupants: `SEAT_QUOTA` has a first-party
  occupant (`quota::Quota`, installed by `Chain::quota`) that is not built in
  because the limit is a number only an operator has, and
  `SEAT_SAFETY_CONTENT`'s argument-reading half ships no occupant because the
  rule is a deployment's — `STAGE_APPROVAL` fills the contract-reading half.
  Telemetry feeds `CallStats`/`route_freq`. Oracle: registration order pinned by
  `the_standard_stack_registers_every_named_stage_in_order`, the chain's verdicts
  pinned against `Exposure::check_call` case by case by
  `the_chain_and_check_call_answer_alike`; every existing guard test passes
  unchanged. *This entry listed the planned §4.5 order as though it were the
  shipped one, and credited the order pin to the verdict-equivalence test.*
- **F5 — enterprise composition.** ✅ *landed 2026-08-14 as `tenant_service.rs`, 16/16 served (PLAN-SERVICES §10); the residency term in `Delegation::decide` is the open half.* `ComposedModel`/`ModelFactory` over the
  registry; tenancy = per-domain `Exposure` + capability tables; residency
  constraint joins `Delegation::decide`. Oracle: cross-tenant invisibility
  tests in the exact shape of the existing cross-session handle tests;
  `ModelFactory.retire` is `ai_effect: destructive` and needs approval.

Integration points (§7.4 style): **IF1** F2's selection must return capability
handles at the MCP face, never IORs (transcript-leak test reused). **IF2**
F4's telemetry and stream B's promotion stats are one store, not two.

§7.3의 형식을 따른다 — 모든 배치는 단위 하나와 결정적 오라클 하나를 가진다.
데이터 플레인은 영구히 CORBA 밖에 남고(원문 철칙 1 그대로), 적중률·지연시간
주장은 이 저장소에 없는 실제 가속기가 아니라 **결정적 라우팅 트레이스**에 대해
오라클로 검증한다 — 나중에 아무도 측정 불가능한 숫자를 보고하지 않도록 지금
적어 둔다.

- **F1 — 계약을 코퍼스로.** ✅ *2026-08-14 착지: golden 22·23, negative
  n11·n12, 네 번째 divergences 항목, 그리고 생성기 위생 결함 하나(`e`라는 이름의
  파라미터가 클로저를 가림)를 잡아 고쳤다.* 단위: MoE IDL → golden 코퍼스,
  그리고 원래의 `Context`/`residency` 결함에서 주조한 negative 케이스.
  오라클: S4 + differential(프론트엔드 셋) + gen-corpus 컴파일.
- **F2 — 트레이딩 서비스.** ✅ *결정 엔진 2026-08-14 착지(`orbweaver-trading`,
  37 tests); 와이어 표면은 2026-08-15에 프로젝트 계약으로 뒤따랐다 —
  `orbweaver-object::expert_service`가 `corpus/golden/22`의
  `moe::ExpertRegistry`/`ExpertLoader`를 서빙하며, `apply_policy`가 오퍼
  저장소와 F3의 상태 머신이 만나는 유일한 지점이다. 표준 `CosTrading::Lookup`
  파사드는 유예로 남는다: PLAN-SERVICES §3.* 원래 범위: 오퍼 저장소(Capability
  프로퍼티), 제약 질의 부분집합, 결정적 정렬. 오라클: 픽스처 오퍼에 대한 속성
  질의 테스트와, 기록된 트레이스 위에서 결과를 고정해 재생한 §6 적재 정책
  (`score = route_freq × affinity ÷ mem_footprint`, 워터마크, LFU 축출).
- **F3 — POA 위의 레지던시 상태 머신.** ✅ *2026-08-14 착지:
  `orbweaver-object::residency`, 20 tests(그날의 크레이트 전체 34 —
  모듈 기록 안에 든 크레이트 수치이며 썩는 쪽이다: 오늘은 84) — 4×5 전이표를 답
  하나하나 고정하고, 가드의 네 조건 각각을 혼자 뒤집어 필요함을 증명하고,
  PERSISTENT 상태가 축출/재적재 주기를 살아남고, 두 창짜리 `Decision` 목록을
  실제 §6 정책에 대해 끝에서 끝까지 적용했다.* `ServantLocator` 위의
  `ExpertLoader`; 전이는 OFFLOADED→PREFETCHING→RESIDENT(→ACTIVE 표지)→OFFLOADED
  뿐이며, **토큰 주기 전이는 구성상 없다** — API에 호출당 훅이 아예 없고, 이는
  문서가 아니라 `compile_fail` 문서 테스트가 붙든다. 오라클: 축출 가드
  (`inflight == 0`)를 포함한 전이 테스트와 PERSISTENT 상태 보존. 줄을 따로 받을
  결정이 둘. **OFFLOADED expert에 대한 요청은 거부되며(`OBJECT_NOT_EXIST`)
  동기 적재로 서빙되지 않는다** — `locate` 안에서의 요구 적재는 §11이 프리페치를
  두어 숨기려는 바로 그 지연이고, 복사가 끝날 때까지 디스패치 스레드를 붙잡는다.
  미스는 대신 프리페치를 요청하고 그것을 *다음* 창이 완료한다. 그리고
  `Located::Here`는 id를 영구 활성화하므로 축출은 POA로 화해되어야 하며
  (`ExpertLoader::reconcile`), 그러지 않으면 축출된 expert가 활성 맵에서 계속
  서빙된다.
- **F4 — 인터셉터 체인의 형식화.** ✅ *2026-08-14 착지
  (`crates/orbweaver-mcp/src/interceptor.rs`).* 가드의 검사들이 순서 있고 확장
  가능한 체인이 되었다. 계획된 범위는 §4.5의 번호 —
  authn → quota → safety → telemetry → audit — 였고, `Chain::standard`가 실제로
  등록하는 것은 **audit → telemetry → exposure → scopes → approval**이다.
  등록 순서는 행위 순서가 아니기 때문이다: 게이트는 들어가며 §4.5의 1→2→3으로,
  관측자는 나오며 4→5로 행위한다. 그래서 거부된 호출까지 **모든** 호출을 봐야
  하는 관측자는 모든 게이트 바깥에 등록되어야 한다. §4.5의 다섯 번호 중 둘은
  표준 스택에서 점유자가 아니라 **이름 붙은 빈 좌석**이다: `SEAT_QUOTA`는
  일급 점유자(`quota::Quota`, `Chain::quota`가 설치)가 있지만 기본 내장되지
  않는데 한계값은 운영자만 가진 숫자이기 때문이고, `SEAT_SAFETY_CONTENT`의
  인자를 읽는 절반은 점유자를 싣지 않는데 그 규칙은 배포자의 것이기 때문이다 —
  계약을 읽는 절반은 `STAGE_APPROVAL`이 채운다. 텔레메트리는
  `CallStats`/`route_freq`를 먹인다. 오라클: 등록 순서는
  `the_standard_stack_registers_every_named_stage_in_order`가, 체인의 판정은
  `Exposure::check_call`과의 사례별 대조로
  `the_chain_and_check_call_answer_alike`가 고정한다. 기존 가드 테스트는 전부
  그대로 통과한다. *이 항목은 계획된 §4.5 순서를 출하된 순서인 양 적었고, 순서
  고정의 공을 판정 동치 테스트에 돌렸다.*
- **F5 — 엔터프라이즈 합성.** ✅ *2026-08-14에 `tenant_service.rs`로 착지,
  16/16 서빙(PLAN-SERVICES §10); `Delegation::decide`의 레지던시 항이 열린
  절반이다.* 레지스트리 위의 `ComposedModel`/`ModelFactory`; 테넌시는 도메인별
  `Exposure` + 능력 테이블; 레지던시 제약은 `Delegation::decide`에 합류한다.
  오라클: 기존 세션 간 핸들 테스트와 정확히 같은 모양의 테넌트 간 비가시성
  테스트, 그리고 `ai_effect: destructive`라 승인이 필요한 `ModelFactory.retire`.

통합 지점(§7.4 형식): **IF1** F2의 선택은 MCP 얼굴에서 능력 핸들을 반환해야
하며 IOR을 반환해서는 안 된다(전사 유출 테스트 재사용). **IF2** F4의
텔레메트리와 스트림 B의 승격 통계는 저장소 둘이 아니라 하나다.

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
and it was not, at the time, worth paying — nothing outside this repository
serves `moe::Capability`, but the same is true of every contract on the day
before someone deploys it, and a project that edits released types when it is
convenient has no §5.3 at all. **The unknown-aware query was the answer until
a version bump had a reason of its own.**

`moe::Capability`에 두 멤버를 넣는 것은 우리 도구가 **BREAKING으로 거부**한다.
당시에는 값을 치를 이유가 없었으므로, 답은 계약 확장이 아니라 미지값을 아는
질의였다. 편할 때 released 타입을 고치는 프로젝트에는 §5.3이 아예 없는 것과
같다.

### 4.5.1 The version bump, paid the §5.3 way / 버전 인상 — §5.3의 방식으로

**Closed 2026-08-19 (D010 A2).** The reason arrived: a latency-ordered router
over v1.0 offers had *no* measured candidate, and "unknown sorts last" is the
right answer to the wrong question — it keeps an unmeasured expert from being
first, but when nothing is measured the fastest is still one of them. So the
contract moved to **moe v1.1, additively**: `corpus/golden/22` gains

```idl
struct MeasuredCapability { Capability base; string specialization; float latency_p50_ms; };
interface ExpertRegistry { /* v1.0 unchanged */
  void register_measured(in Expert e, in MeasuredCapability measured);
  void heartbeat_measured(in Expert e, in MeasuredCapability updated_measured);
};
```

— a new struct that *composes* the released one, and two server-first
operations a v1.0 client never calls. The pair the gate re-measures lives on
disk: `corpus/evolution/moe/v1.0/moe.idl` is the frozen release, golden 22 is
the proposed revision, and `corpus/evolution/moe/v1.1-in-place/moe.idl` is the
edit above with the members added in place, kept as the negative control:

```
$ idl-diff corpus/evolution/moe/v1.0/moe.idl corpus/golden/22-moe-control-plane.idl
[server-first] IDL:moe/ExpertRegistry:1.0: operation "heartbeat_measured" added — …
[server-first] IDL:moe/ExpertRegistry:1.0: operation "register_measured" added — …
[compatible] IDL:moe/MeasuredCapability:1.0: added — nothing deployed refers to it yet
accepted: nothing here breaks a deployed peer            (exit 0)

$ idl-diff corpus/evolution/moe/v1.0/moe.idl corpus/evolution/moe/v1.1-in-place/moe.idl
[BREAKING] IDL:moe/Capability:1.0: member "latency_p50_ms" added — …
[BREAKING] IDL:moe/Capability:1.0: member "specialization" added — …
refused: 2 change(s) break deployed peers               (exit 1)
```

What the engine and the servant do with it, measured:

- **The matcher.** `orbweaver_trading::query::Selection` gains `unranked` —
  offers that satisfy every conjunct and carry no value for the `ORDER BY`
  field — and `is_complete()`, the router rule as a predicate: a sequence is a
  complete answer or it is a refusal. An unmeasured offer is no longer sorted
  last; it is set aside and named. Before the change the same tests were red
  (`a_router_ordering_by_latency_cannot_prefer_an_unmeasured_expert`,
  `an_unknown_ordering_key_is_unranked_not_last`).
- **The servant.** `register_measured`/`heartbeat_measured` are served on
  `moe::ExpertRegistry`; the offer arrives with both members `Some`, and a
  query on them is answerable and rankable in both byte orders
  (`a_v1_1_registration_is_answerable_and_rankable_on_both_byte_orders`). A
  v1.0 `heartbeat` on a measured offer **keeps** the two members — a message
  with no room for a fact cannot withdraw it; before this it erased them,
  and erased an out-of-band `declare_specialization` too.
- **`spike-experts` windows 4–5.** Over v1.0 offers, "the fastest maths
  expert" is refused (`ranked [], set aside ["expert-math"]`); after one
  `register_measured` it is still refused, naming the unmeasured one; after
  `heartbeat_measured` for the other, `ranked ["expert-math-b",
  "expert-math"]` and the pick is the one measured faster. A bound nothing
  meets is an honest empty answer, not a refusal.

Still true and unchanged, as the record of what landed against this plan:
`Router::select` orders by `route_freq`, which every offer carries, so the wire
operation never sets anything aside for ranking; `Constraints` is released and
still binds `max_latency_ms` to p99. What that leaves open on the wire is
current status, not plan history, and its home is `COMPONENTS.md`'s trading row
— read the gap column there. *A sentence naming the terms of the open half was
retyped here from that column, which is the drift this project has measured
before: a fact has one home.*

**2026-08-19 종결 (D010 A2).** 이유가 생겼다: 지연시간 순 라우터가 v1.0 오퍼만
가진 상태에서 "미지값은 마지막에 정렬"은 옳은 답이지만 질문이 틀렸다 — 아무도
측정하지 않았을 때 가장 빠른 것은 여전히 그중 하나다. 그래서 계약은 **v1.1로,
추가만으로** 올라갔다: released `Capability`를 *합성하는* 새 구조체
`MeasuredCapability`와, v1.0 클라이언트는 호출하지 않는 server-first 오퍼레이션
둘. 게이트가 재측정하는 쌍은 디스크에 있다 — `corpus/evolution/moe/v1.0`이 동결된
릴리스, golden 22가 제안, `v1.1-in-place`가 제자리 수정 음성 대조군(exit 1).
매처는 `Selection::unranked`와 `is_complete()`를 얻었고(순위를 매길 수 없는
오퍼는 마지막이 아니라 따로 명명된다), 서번트는 두 오퍼레이션을 양쪽 바이트
순서로 서비스하며, v1.0 `heartbeat`는 자기가 언급할 수 없는 두 멤버를 지우지
않는다. `spike-experts` 4·5번 창이 거부→부분 측정 거부→완전한 답을 순서대로
보인다. `Router::select`는 `route_freq`로 정렬하므로 와이어 동작은 변하지 않았다.
와이어에서 아직 열려 있는 것은 계획 이력이 아니라 **현재 상태**이고, 그 집은
`COMPONENTS.md`의 트레이딩 행이다 — 여기서 다시 적지 않는다. 사실의 집은 하나다.

## 4.6 Why `Router` is in no plan — the plane rule and its escape hatch / `Router`가 어느 계획서에도 없던 이유 — 평면 규칙과 그 탈출구

The coverage sweep found `moe::Router::select` and `dispatch` declared in a
landed contract, served by nothing, and named in no plan — not even in the
exclusions. Reading the contract against §3's rule explains it, and the
explanation is worth more than the omission.

§3 says **the data plane stays out of CORBA permanently**. The contract
declares:

```idl
typedef sequence<octet> Tensor;      // reference-carrying; never inlined
struct Activation { Tensor data; string dtype; string shape; };

interface Expert { Activation process(in Activation x, in CallContext ctx); };
interface Router { Activation dispatch(in Activation x, in CallContext ctx); };
```

Those two operations are control-plane-legal **only** under the reading that
`Tensor` carries a *handle* — a shared-memory name, an accelerator buffer id —
rather than the activation itself. That reading exists in a comment in
`corpus/golden/22` and **nowhere else**: not in this document, not in
`PLAN-SERVICES`, not in `ARCHITECTURE`. And nothing enforces it. A
`sequence<octet>` will carry a megabyte as cheerfully as a sixteen-byte handle,
so the rule that defines this whole stream is, today, a sentence in a plan and
a comment in a corpus file.

That is why the two are unimplemented, and the honest split is:

- **`Router::select`** returns `ExpertSeq` — references, nothing else — and its
  absence was a **gap** rather than a decision. **Closed 2026-08-14**: it
  delegates to the trading engine rather than reimplementing selection, and
  when a constraint names a field a wire-registered offer cannot answer it
  refuses the *whole call* with `NO_IMPLEMENT`. A shorter list would have said
  *these are all the experts that qualify*, which is the sentence the offer
  store's three-valued matching exists to prevent; a sequence of references is
  a complete answer or it is a refusal.

  **Corrected by D006, which measured what this section asserted.** `select` is
  not free of the plane question: it *takes* a `GateSignal`, and a `GateSignal`
  holds `Tensor affinity` — the gate's routing logits. So all three operations
  touch a `Tensor`; only `select`'s **return** is references-only. The
  operation this section filed as pure control plane is the one whose exposure
  nobody had noticed, which is a fair illustration of why a rule that lives in
  prose is not a rule.
- **`Router::dispatch` and `Expert::process`** carry an `Activation` in *both*
  directions, which is the difference of degree that separates them from
  `select`. **Excluded by D006, approved 2026-08-14** — and the reason approval
  changes less than it looks: a bound constrains size, not frequency, so no
  mechanism any option offered could see a 16-byte handle called once per token,
  which is the data plane at full rate. Exclusion is the mechanism this project
  has actually made hold before (F3 removed the API and a `compile_fail` test
  keeps it removed); a bound is not. Serving them means either committing to the handle reading in a
  document that binds, or declaring them excluded. **Committing needs a
  decision**, because it constrains what a deployment may put in a `Tensor`,
  and a rule nobody can check is a rule that will be broken by whoever needs a
  quick result.

Nothing here is implemented on the strength of this section: it records why the
gap exists and what closing it would require. What *is* now true is that the
absence has a reason, which is what §8.1 of `PLAN-SERVICES` asks of every
`BAD_OPERATION`.

**`Router`가 어느 계획서에도 없던 이유.** 데이터 플레인 금지 규칙 아래에서
`process`/`dispatch`가 합법인 것은 **`Tensor`가 핸들을 나른다는 독해** 아래에서
뿐인데, 그 독해는 코퍼스 파일의 주석에만 있고 강제하는 것은 없다.
`sequence<octet>`는 16바이트 핸들만큼이나 기꺼이 1메가바이트를 나른다. `select`의
부재는 결정이 아니라 **공백**이지만, D006이 이 절을 정정했다 — `select`는 `Tensor
affinity`를 담은 `GateSignal`을 **받으므로** 평면 질문에서 자유롭지 않다. 참조만인
것은 **반환**뿐이다. 산문 속의 규칙은 규칙이 아니라는 예시이기도 하다.

> **Settled as a decision, and it corrects this section.** Whether the plane
> rule can be stated as a predicate over a contract at all, and which of five
> mechanisms should carry it, is
> [`decisions/D006-plane-rule-tensor.md`](decisions/D006-plane-rule-tensor.md)
> (**APPROVED** 2026-08-14): option E, `Expert::process` and `Router::dispatch`
> excluded rather than bounded, `Router::select` left open, and option A — a
> new versioned interface carrying a `TensorHandle` — named as the return path
> rather than a rejection. Two of its findings amend what is written above: the split
> here is not two operations against one — `select` takes `GateSignal`, which
> holds a `Tensor affinity`, so **all three operations touch a `Tensor`** and
> only the return side of `select` is references-only; and a bound is *not*
> enforced by the marshaller for free, because `orbweaver-gen` drops it
> (`gen/src/lib.rs:164`) while `orbweaver-dynamic` enforces it *[true on 2026-08-14 when D006
> was written; the same day's 526b355 made both paths enforce it — `rt::Bounded`,
> `tests/bounds_oracle.rs` — and D006's argument does not depend on it: a bound
> constrains size, not frequency]*. The bound change
> was re-measured as BREAKING, and so is removing the two operations.
> 규칙을 계약에 대한 술어로 쓸 수 있는가와 다섯 기제 중 무엇을 택할 것인가는
> D006(**승인됨**, 2026-08-14)으로 정리되었다: E안 — `Expert::process`와
> `Router::dispatch`를 상한 대신 제외, `Router::select`는 열어 둠, A안(`TensorHandle`을
> 나르는 새 버전 인터페이스)은 기각이 아니라 복귀 경로. 위 서술 두 곳을 정정한다: `select`도
> `GateSignal.affinity`로 `Tensor`에 닿으므로 **세 연산 모두** 해당하며, 상한은
> 마샬러가 공짜로 강제하지 않는다(정적 생성 경로가 상한을 버린다).

## 5. What this supplement does not claim / 이 보완이 주장하지 않는 것

No accelerator, no fused kernel, no RDMA exists in this repository; the data
plane remains a named external. `Expert.process` payloads cross as references
per the source document's own rule — AnyJSON already refuses to inline what a
handle should carry. Performance targets (§11의 <5% 오버헤드, p99 은닉) become
measurable only when a data-plane simulator exists; until then stream F
reports state-machine and policy correctness, nothing about latency.

이 저장소에는 가속기도, 융합 커널도, RDMA도 없다. 데이터 플레인은 이름이 붙은
외부로 남는다. `Expert.process`의 페이로드는 원문 자신의 규칙대로 참조로
건너간다 — AnyJSON은 핸들이 날라야 할 것을 인라인하기를 이미 거부한다. 성능
목표(원문 §11의 <5% 오버헤드, p99 은닉)는 데이터 플레인 시뮬레이터가 생겨야
비로소 측정 가능해진다. 그때까지 스트림 F가 보고하는 것은 상태 머신과 정책의
정확성이며, 지연시간에 대해서는 아무것도 보고하지 않는다.
