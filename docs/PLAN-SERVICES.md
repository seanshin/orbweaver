# PLAN-SERVICES — the core CORBA services, planned as a suite

> Supplement to `PLAN.md` §7 and companion to [`PLAN-MOE.md`](PLAN-MOE.md).
> Written 2026-08-14, after the services audit found CosNaming half-planned
> and CosEvent absent from every document. Batch numbers F2/F5/F6/F7 are
> PLAN-MOE's; this document deepens them and adds what they lacked.
> `PLAN.md` §7의 보완이자 `PLAN-MOE.md`의 자매 문서 — 서비스 실사가 찾은 공백
> (Naming 절반, Event 전무)을 서비스 단위 계획으로 메운다.

## 1. The rules that govern every service / 모든 서비스를 지배하는 규칙

1. **First-party, from the OMG specification.** A CosService is logic defined
   by a published spec — exactly the category CLAUDE.md says we implement
   ourselves and owe nobody for. No service brings a dependency.
2. **Only the subset a named consumer needs.** Every operation implemented
   must name who calls it (the harness, stream F, the MCP bridge). An
   operation with no consumer is surface to get wrong. Omitted operations are
   **refused loudly** (`BAD_OPERATION`), never half-served — F6 set the
   precedent (`bind_context`/`destroy` refused, `list`'s nil iterator
   documented as under-reporting).
3. **Both directions, where a reference peer exists.** Our client against the
   reference implementation AND the reference client against our server. F6
   measured both for Naming; every service chapter names its peer or states
   honestly that none is available.
4. **Exception bytes mirror what our client already expects.** The client
   half was verified against omniNames first, so the server half must produce
   the shapes that client decodes — one wire truth, approached from both ends.

규칙 요지: OMG 명세로부터 1st-party 구현, **이름 붙은 소비자**의 부분집합만,
가능한 곳은 **양방향** 오라클, 생략 연산은 시끄럽게 거부(반쪽 서비스 금지).

**Whether rule 2 is being kept is measured, not asserted.**
[`SERVICES-COVERAGE.md`](SERVICES-COVERAGE.md) (2026-08-14) drives every
operation each service's IDL declares against the running servant and reports,
per operation, whether it is served, refused with a reason quoted from the
servant, or refused with no reason written anywhere. The last category is the
one this rule exists to keep empty; it held **12 of 107** declared operations
on 2026-08-14 and **0 of 106** on 2026-08-19 — the current number is the
generated block `SERVICES-COVERAGE.md` §8, never this sentence. Re-run it
with `./spikes/service_sweep.sh`.

규칙 2가 지켜지고 있는지는 주장이 아니라 **측정**된다 —
[`SERVICES-COVERAGE.md`](SERVICES-COVERAGE.md)(2026-08-14)가 선언된 모든 연산을
실행 중인 서번트에 걸어보고 서빙/이유 있는 거부/이유 없는 거부로 분류한다.
마지막 칸이 비어 있게 하는 것이 이 규칙의 목적이며, 거기에는 **선언 107개 중
12개**(2026-08-14), **선언 106개 중 0개**(2026-08-19)가 있었다 — 현재
숫자는 `SERVICES-COVERAGE.md` §8의 생성 블록이며 이 문장이 아니다.
`./spikes/service_sweep.sh`로 다시 잰다.
(이 문단은 날짜도 살아 있는 집도 없이 **현재형으로 "선언 107개 중 12개"라고만
적고 있었다** — 영어 쌍둥이가 두 수치를 모두 날짜와 함께 적고 생성 블록을 지목한
바로 그 자리에서. 수를 적는 법의 본보기는 위 영어 문단이다.)

## 2. CosNaming / 네이밍 — ✅ both halves landed and measured

| | |
|---|---|
| Standard | `CosNaming`, `NamingContextExt` (INS) |
| Consumers | harness, MCP reference acquisition, F7's channel discovery, IFR facade |
| Client (Phase 1) | resolve, resolve_str, corbaname — verified against omniNames |
| Server (F6, 2026-08-14) | resolve/bind/rebind/unbind/bind_new_context/new_context/list + Ext string surface; nested contexts as distinct object keys |

Measured, both directions: our client round-trips bind/AlreadyBound/
NotFound{why, rest_of_name}/nested resolution against our server, and
**omniORB's python client resolved `spike/Echo` against our server and decoded
our `NotFound` user-exception bytes** — the first "their client, our server"
claim in the project. Serving user exceptions at all required
`Dispatch::dispatch_body`, which every existing servant inherits unchanged.

Not doing (until a consumer appears): real `BindingIterator` lifecycles
(the interface is declared and probed against no object — §8 of the coverage
document names it unmeasured), federation across naming domains (F5 evaluated
PLAN-DEFERRED §7's trigger *in code* and found the other shape —
`tenant_service.rs`, "one graph, per-tenant keys"). `bind_context`,
`rebind_context` and `destroy` are **served** since 2026-08-18 (this sentence
said "refused loudly" and §8.1.1 said "moved to `NO_IMPLEMENT`" while the wire
served all three — one fact, three homes, three answers, found by the plan
review of 2026-08-19; the wire is the home). What is still deferred is
narrower than "federation": a `bind_context` whose argument is a **foreign**
context answers `NO_IMPLEMENT`, and the reason and trigger for that live in
**PLAN-DEFERRED §12** (chaining to a foreign context), not in §7 — this
paragraph named only §7, which is the trading-link/naming-federation chapter,
so a reader chasing the refusal landed a chapter away from its reason. The
membership test is `local_context_key` in `naming_server.rs`: a profile at the
host and port this server publishes **and** a key the tree currently holds,
because the key alone would adopt somebody else's object and the address alone
would accept a key `destroy` has retired. Measured in-crate (a dummy foreign
reference gets `NO_IMPLEMENT` while an undeclared name still gets
`BAD_OPERATION`) and from a peer (`naming_lifecycle_from_a_peer.rs` asserts the
`bind_context_foreign` row is `NO_IMPLEMENT`); the servant holds no
`Connection`, which is `naming_no_outbound_call.rs` rather than a sentence.

표준은 `CosNaming`과 INS의 `NamingContextExt`. 소비자는 하네스, MCP의 참조 획득,
F7의 채널 발견, IFR 파사드. 클라이언트 절반(Phase 1)은 resolve/resolve_str/
corbaname이며 omniNames에 대고 검증했고, 서버 절반(F6, 2026-08-14)은
resolve/bind/rebind/unbind/bind_new_context/new_context/list과 Ext 문자열
표면이며 중첩 컨텍스트는 서로 다른 오브젝트 키로 산다.

**양방향으로 측정했다.** 우리 클라이언트가 우리 서버에 대고
bind/AlreadyBound/NotFound{why, rest_of_name}/중첩 해석을 왕복하고,
**omniORB의 파이썬 클라이언트가 우리 서버에서 `spike/Echo`를 해석하고 우리가 보낸
`NotFound` 사용자 예외 바이트를 디코딩했다** — 프로젝트 최초의 "그들의 클라이언트,
우리의 서버" 주장이다. 사용자 예외를 서빙하는 것 자체가 `Dispatch::dispatch_body`를
필요로 했고, 기존 서번트 전부가 그것을 변경 없이 물려받는다.

소비자가 나타날 때까지 하지 않는 것: 진짜 `BindingIterator` 수명 주기(인터페이스는
선언되어 있으나 어떤 객체에도 프로브되지 않았다 — 커버리지 문서 §8이 이를 미측정으로
적는다), 네이밍 도메인 간 연합(F5가 PLAN-DEFERRED §7의 방아쇠를 *코드 안에서*
평가했고 다른 모양을 찾았다 — `tenant_service.rs`, "그래프 하나, 테넌트별 키").
`bind_context`, `rebind_context`, `destroy`는 2026-08-18 이후 **서빙된다**(이
문단은 "시끄럽게 거부"라고, §8.1.1은 "`NO_IMPLEMENT`로 옮겼다"고 적고 있었으나
와이어는 셋 다 서빙하고 있었다 — 사실 하나에 집 셋, 답 셋. 2026-08-19 계획 검토가
찾았고, 집은 와이어다). 아직 유예된 것은 "연합"보다 좁다: 인자가 **외부**
컨텍스트인 `bind_context`는 `NO_IMPLEMENT`로 답하며, 그 이유와 방아쇠는 §7이 아니라
**PLAN-DEFERRED §12**(외부 컨텍스트로의 연쇄)에 산다 — 이 문단은 §7만 지목하고
있었고 §7은 트레이딩 링크/네이밍 연합의 장이므로, 거부의 이유를 좇는 독자는 한 장
떨어진 곳에 도착했다. 판정은 `naming_server.rs`의 `local_context_key`다: 이 서버가
공표하는 호스트·포트의 프로파일 **그리고** 트리가 지금 들고 있는 키. 키만으로는
남의 객체를 제 것인 양 채택하게 되고, 주소만으로는 `destroy`가 거둔 키까지
받아들이기 때문이다. 크레이트 안에서(가짜 외부 참조는 `NO_IMPLEMENT`를 받고, 선언
자체가 없는 이름은 여전히 `BAD_OPERATION`을 받는다) 그리고 피어에서
(`naming_lifecycle_from_a_peer.rs`가 `bind_context_foreign` 행이 `NO_IMPLEMENT`임을
단언한다) 측정했다. 서번트가 `Connection`을 들지 않는다는 것은 문장이 아니라
`naming_no_outbound_call.rs`다.

## 3. CosTrading / 트레이딩 — ✅ engine and project-contract wire both landed

| | |
|---|---|
| Standard | `CosTrading` (Lookup, Register, Admin, Link, Proxy) |
| Consumer | stream F loading/placement (PLAN-MOE §6), later the catalog |
| Engine (F2, 2026-08-14) | `orbweaver-trading`: offers, constraint-query subset, loading policy, deterministic trace replay — 37 tests on 2026-08-14 (`spikes/plan_numbers.py` prints today's) |
| Wire (2026-08-15) | `orbweaver-object::expert_service`: `moe::ExpertRegistry` + `moe::ExpertLoader` from `corpus/golden/22`, on our POA-side `Server` — 17 tests on 2026-08-15 (`spikes/plan_numbers.py` prints today's) plus `spike-experts` |

**The honest choice about the standard module:** OMG CosTrading is enormous
(five interfaces, federated links, proxy offers, dynamic properties). Nothing
in stream F consumes more than property-constrained lookup over registered
offers, which the engine already does with S4-style positioned query errors.
The wire surface therefore landed as the *project* contract
(`moe::ExpertRegistry` from `corpus/golden/22`), served on our POA like F6;
the standard `CosTrading::Lookup::query` facade is still deferred until a
foreign trading client is named — the IFR-facade rule (§7) applied to
Trading. Deferral is recorded here so it is a decision, not a drift.

What the servant is, precisely: **one servant, two objects.** Registering an
expert has to create it in the offer store *and* in F3's residency machine,
and nothing could keep two servants' halves in step; but the contract declares
two interfaces and a client narrows to one, so they stay distinct object keys
with distinct repository ids. The join is `apply_policy(free_memory)` — a
heartbeat updates the offer, the §6 policy decides over the store, and its
`Decision`s drive the loader — and that is the whole reason the batch was
worth doing as one piece.

Two consequences worth stating rather than discovering later. **No operation
declares `raises`**, so every refusal is a system exception chosen to be
actionable (`BAD_PARAM` unknown, `BAD_INV_ORDER` no such edge, `NO_PERMISSION`
pinned, `TRANSIENT` the window may differ); inventing a user exception would
emit bytes the generated client has no branch for. And **`moe::Capability`
carried no `specialization` and no `latency_p50`** — closed 2026-08-19 the
§5.3 way as `MeasuredCapability` + `register_measured`/`heartbeat_measured`
(PLAN-MOE §4.5.1 is the home; `corpus/evolution/moe/` holds the frozen pair).

Not measured, and not claimed: only our own client has called this. No foreign
MoE peer exists, so unlike §2 there is no "their client, our server" direction
to report.

표준 CosTrading 모듈 전체는 소비자가 없다 — 스트림 F가 쓰는 것은 속성 제약
조회뿐이며 그것은 착지되었다. 와이어 표면은 프로젝트 계약(`corpus/golden/22`의
`moe::ExpertRegistry`/`ExpertLoader`)으로 착지했고, 표준 파사드는 외부 트레이딩
클라이언트가 명명될 때까지 계속 유예된다. 유예는 표류가 아니라 결정이다.

서번트의 정체: **서번트 하나, 오브젝트 둘.** 등록은 오퍼 스토어와 F3 상주
머신 양쪽을 동시에 만들어야 하므로 소유자가 하나여야 하지만, 계약이 인터페이스
두 개를 선언하므로 오브젝트 키와 리포지터리 ID는 분리된다. 접점은
`apply_policy(free_memory)` — 하트비트가 오퍼를 갱신하고, §6 정책이 스토어
위에서 결정하고, 그 `Decision`이 로더를 구동한다. 이것이 이 배치를 한 덩어리로
묶은 이유다.

기록해 둘 두 가지. **어떤 연산도 `raises`를 선언하지 않으므로** 모든 거부는
시스템 예외이며, 행동 가능하도록 골랐다(`BAD_PARAM` 미등록, `BAD_INV_ORDER`
없는 간선, `NO_PERMISSION` 핀, `TRANSIENT` 다음 윈도우 재시도). 사용자 예외를
발명하면 생성된 클라이언트가 해석할 분기가 없는 바이트가 나간다. 그리고
**`moe::Capability`에는 `specialization`도 `latency_p50`도 없었다** —
2026-08-19, §5.3의 방식으로 닫혔다. 릴리스된 `Capability`를 제자리에서 고치는
것은 우리 `idl-diff`가 BREAKING으로 판정하므로(CDR 멤버에는 태그도 길이도 없다),
그것을 **합성하는** `MeasuredCapability`(`base` + `specialization` +
`latency_p50_ms`)와 새 연산 `register_measured`/`heartbeat_measured`가
`corpus/golden/22`에 들어왔다. 집은 [`PLAN-MOE.md`](PLAN-MOE.md) §4.5.1이고,
`corpus/evolution/moe/`가 얼린 짝 — `v1.0`과 거절된 `v1.1-in-place` — 을 갖고
있다. 이 문단은 그 뒤로도 현재형으로 "없다 … F1의 계약 문제"라고, 즉 닫힌 것을
열린 것으로 적고 있었다.

측정되지 않았고 주장하지도 않는 것: 우리 클라이언트만 호출했다. 외부 MoE 피어가
없으므로 §2와 달리 "그들의 클라이언트, 우리의 서버" 방향은 보고할 것이 없다.

## 4. CosEvent / 이벤트 — ✅ all four models served (17 of 18), `PLAN-DEFERRED` §10 graduated here 2026-08-25

| | |
|---|---|
| Standard | `CosEventComm` (PushConsumer/PushSupplier), `CosEventChannelAdmin` |
| Consumers | residency transitions (F3), telemetry batches (F4 → CallStats), audit fan-out |
| Granularity rule | control-plane events only — **never per token**, the same clock discipline as the loading policy |

**Fixture probe (measured 2026-08-14):** `brew info omnievents` → *"No
available formula"* — no reference event-channel implementation is available
as a fixture. But omniORBpy ships the `CosEventComm` stubs (`import
CosEventComm` succeeds), so the F6 oracle direction transfers: **we serve the
channel first-party, and omniORB's python acts as the independent push
consumer/supplier against it.** Scope v1: `EventChannel` +
`ConsumerAdmin::obtain_push_supplier` + `SupplierAdmin::obtain_push_consumer`
+ `ProxyPushConsumer::push(any)` / `ProxyPushSupplier::connect_push_consumer`
— the push model, since 2026-08-18 the **consumer half of pull**
(`obtain_pull_supplier`, `pull`, `try_pull`), and since 2026-08-25 the
**supplier half** as well (§4.1), which is all four models. Only `destroy`
remains `NO_IMPLEMENT`, with its reason in `event_server.rs`'s header and its
chapter at PLAN-DEFERRED **§11** — it wants a caller model, because `destroy`
ends the channel for every other client. §10 was the supplier side of pull and
graduated into §4.1 above; the two were always two chapters with two different
triggers, and this sentence named §10 for both until 2026-08-18.
Events are `any`, which AnyJSON already carries.

Batch unit: the channel × both directions (our supplier → omniORB consumer,
omniORB supplier → our consumer) × disconnect semantics (a dead consumer must
not wedge the channel — bounded buffer, drop-oldest, drops counted and
reported, per the "no silent truncation" rule).

### 4.1 The supplier side of pull — graduated from `PLAN-DEFERRED` §10, 2026-08-25

**The first chapter to graduate under §9**, so it is also the template. The
trigger — *"a named `PullSupplier` in this workspace"* — fired on the project
owner's request that the four models be creatable, under D023 §2's rule that
**the owner naming a consumer fires a trigger**. It is now satisfied literally
as well: `event_server::PullSupplierServant` is one, published from the crate
rather than hidden in a test module where the trigger could not have seen it.

**Batch unit** (what §9 says this file owes and `PLAN-DEFERRED` withholds): the
three operations `SupplierAdmin::obtain_pull_consumer`,
`ProxyPullConsumer::connect_pull_supplier` and
`PullConsumer::disconnect_pull_consumer` × both byte orders × the failure
directions (a supplier that never answers, one that answers `Disconnected`,
one that disappears). Not four pieces of work: the whole gap between two
served models and four is the channel acting as a **pull consumer of a
supplier**, and all three operations carry that one shape.

**Named oracle:** `spikes/event_pull_supplier.py` — an omniORBpy
`CosEventComm::PullSupplier` that our channel dials and calls `try_pull` on,
with an omniORB `PushConsumer` *and* an omniORB pull consumer both receiving
what was fetched. Both byte orders via `spike-events --source-endian`. This is
the mirror of §4's existing direction: there our channel is called by an ORB we
did not write, here it is a **client** of one.

Measured 2026-08-25 (`service_sweep.sh`): probes 28, dispatched 27,
`NO_IMPLEMENT` 1 — **17 of 18 declared operations**, the remaining one being
`destroy` (`PLAN-DEFERRED` §11, trigger unfired). One `EventChannelServer` also
now holds several named channels (`create_channel`), which needed no wire
interface and no factory.

*§9 아래 **처음 졸업한 장**이므로 템플릿이기도 하다. 방아쇠는 네 모델을 생성
가능하게 하라는 소유자의 요청으로 당겨졌다(D023 §2 — 소유자가 소비자를 지명하면
방아쇠가 당겨진다). 이제 문자 그대로도 충족된다: `PullSupplierServant`가 그것이며,
방아쇠가 볼 수 없는 테스트 모듈이 아니라 크레이트에서 공개된다.*

***배치 단위***: 연산 셋 × 양쪽 바이트 순서 × 실패 방향들. 네 조각의 일이 아니다 —
서빙되는 두 모델과 네 모델 사이의 간극 전체가 **채널이 공급자로부터 당기는** 한
가지 모양이고, 연산 셋이 모두 그것을 나른다.

***이름 붙은 오라클***: `spikes/event_pull_supplier.py` — 우리 채널이 다이얼해
`try_pull`을 부르는 omniORBpy `PullSupplier`. §4의 기존 방향의 거울이다: 거기서는
우리가 쓰지 않은 ORB가 우리 채널을 부르고, 여기서는 우리 채널이 그것의
**클라이언트**다. 2026-08-25 측정: 선언 18개 중 **17개 서빙**, 남은 하나는
`destroy`(§11, 미발화).

측정이 오라클을 결정했다: omniEvents 픽스처는 없으나 omniORBpy가 CosEventComm
스텁을 싣고 있으므로, F6과 같은 방향 — 채널은 우리가 서빙하고 독립 ORB가 consumer/
supplier로 접속 — 이 가능하다. 컨트롤 플레인 입도만이라는 규칙은 그대로이고,
범위는 push 모델, 2026-08-18 이후의 **pull 소비자 쪽**
(`obtain_pull_supplier`, `pull`, `try_pull`), 그리고 2026-08-25 이후의
**pull 공급자 쪽**(§4.1)이며 — 이것이 네 모델 전부다. `NO_IMPLEMENT`로 남는 것은
`destroy` 하나이고 이유는 `event_server.rs` 헤더에, 장은 **PLAN-DEFERRED §11**에
있다 — 그것은 호출자 모델을 원한다. `destroy`가 다른 모든 클라이언트에 대해
채널을 끝내기 때문이다. §10은 pull 공급자 쪽이었고 위 §4.1로 졸업했다. 둘은
언제나 방아쇠가 다른 두 장이었으나, 이 문단은 2026-08-18까지 둘 다 §10이라고
적고 있었다.

## 5. CosLifeCycle & CosProperty / 생명주기·프로퍼티 — F5

`ModelFactory` (create/clone_model/deploy/retire, `corpus/golden/23`) is the
GenericFactory shape with the standard's genericity dropped: typed factories
per contract, because an untyped `create(key, criteria)` is exactly the
stringly-typed surface S4 exists to prevent. `retire` is `ai_effect:
destructive` and rides the existing approval gate. CosProperty stays folded
into the `Manifest` struct until a second consumer exists — the F6 rule: no
standalone service without a named caller. Tenancy isolation tests take the
cross-session capability-table shape already proven in the MCP batches.

`ModelFactory`(create/clone_model/deploy/retire, `corpus/golden/23`)는
GenericFactory의 모양에서 표준의 일반성을 덜어낸 것이다 — 계약마다 타입이 붙은
팩토리. 타입 없는 `create(key, criteria)`야말로 S4가 막으려고 존재하는 문자열
표면이기 때문이다. `retire`는 `ai_effect: destructive`이며 기존 승인 게이트를 탄다.
CosProperty는 두 번째 소비자가 생길 때까지 `Manifest` 구조체 안에 접힌 채로 둔다 —
F6의 규칙, 즉 이름 붙은 호출자 없는 독립 서비스는 없다. 테넌시 격리 테스트는 MCP
배치에서 이미 입증된 세션 간 능력 테이블 모양을 그대로 가져간다.

## 6. Security — cross-reference only / 교차 참조

CSIv2 wire, delegation policy, credential hygiene and `ai_authz` scopes are
PHASE5 + guard work and stay there; this plan adds nothing on top. The one
service-suite note: none of the services above invent their own authorization
— a naming `bind` or channel `connect` at the MCP boundary passes the same
`Exposure`/`Guarded` gate as any other operation (I1's rule, inherited).

CSIv2 와이어, 위임 정책, 자격 증명 위생, `ai_authz` 스코프는 PHASE5와 가드 작업의
몫이며 거기 그대로 둔다 — 이 계획은 그 위에 아무것도 얹지 않는다. 서비스 묶음에
대해 적을 것은 하나뿐이다: 위 서비스 가운데 어느 것도 **자기만의 인가를 발명하지
않는다**. MCP 경계에서의 네이밍 `bind`나 채널 `connect`는 다른 모든 연산과 똑같이
`Exposure`/`Guarded` 게이트를 통과한다(I1의 규칙을 물려받는다).

## 7. Interface Repository facade / IFR 파사드 — ✅ landed, both directions

`orbweaver-registry` is the first-party IFR equivalent (stated since Phase 2).
PLAN §7's old "optional read-only `CORBA::Repository` facade" line gets its
batch shape here: serve `_get_interface`-adjacent lookups (`lookup_id`,
`describe_interface`-equivalent) read-only over the registry, on our POA,
**after F6** (the facade is a named object that wants Naming), with omniORB's
python IRObject client as the cross-ORB oracle. Write operations refused
loudly — the registry is populated from IDL through S4, never over the wire.

| | |
|---|---|
| Standard | `CORBA::Repository`, `Contained`, `IRObject`, `InterfaceDef` |
| Consumers | a foreign DII client; integration's conformance harness |
| Landed | `orbweaver-registry::ifr::RepositoryServer` + `spike-ifr` |
| Served | `lookup_id`; `_get_id`/`_get_name`/`_get_absolute_name`/`_get_version`/`_get_def_kind`; `describe_interface`, `_get_base_interfaces`, `is_a`; `_is_a`/`_non_existent` |

It sits in `orbweaver-registry`, not `orbweaver-giop`: the facade needs the
registry's facts, and the dependency edge runs registry → giop. One object key
per registry entry, derived reversibly from the repository id, so a stored
reference survives a restart and the server holds no per-reference state.

Measured, both directions: our client round-trips the whole walk against our
server (`spike-ifr`, 13 checks), and **omniORB's python client — narrowing via
the `omniORB.ir_idl` stubs it ships — decoded our `FullInterfaceDescription`
for `gc10::Both` and `tms::TrackManager`**, including parameter modes,
`OP_ONEWAY`, a raised exception's members, a `tk_alias`→`tk_sequence`→
`tk_struct` return type, and `CORBA.NO_PERMISSION` from `create_module`.

Not doing (until a consumer appears): `Container::contents`/`lookup`/
`lookup_name`/`describe_contents`, `Contained::describe`, `_get_defined_in`/
`_get_containing_repository`, `Repository::get_canonical_typecode`/
`get_primitive`, `IDLType::_get_type` — all **`NO_IMPLEMENT`** as of
2026-08-14. Every mutating operation is `NO_PERMISSION` permanently, not
pending: a writable IFR would be a second ingestion path with none of S4's
gates on it.

**Why the deferrals stopped answering `BAD_OPERATION`.** They answered it until
§8.1 was written, and that is exactly what made §8.1 necessary: `BAD_OPERATION`
says "no such operation", which is byte-for-byte what an operation nobody
thought about says, so the only thing separating a decision from a gap was a
sentence in a document the client cannot read. `NO_IMPLEMENT` is the
specification's answer for an operation that exists and has no implementation
here, which is what a deferral *is*. Three refusals, three facts, on the wire:
`NO_PERMISSION` is policy, `NO_IMPLEMENT` is deferred, `BAD_OPERATION` is "not
an operation of the object you addressed — try another reference". The reasons
stay written down, because the wire says *that* an operation is deferred and
never *why*.

유예 연산이 `BAD_OPERATION`을 그만둔 이유: 그 답은 "그런 연산 없음"이며, 아무도
생각해보지 않은 연산의 답과 바이트 단위로 같다. `NO_IMPLEMENT`는 "연산은 있고
여기 구현이 없다"는 명세의 답이고, 그것이 유예의 정의다. 이제 와이어에서
`NO_PERMISSION`(정책) · `NO_IMPLEMENT`(유예) · `BAD_OPERATION`(그런 연산 없음)
셋이 구분된다. 이유는 여전히 문서에 적는다 — 와이어는 *유예됐다*까지만 말하고
*왜*는 말하지 않으므로.

## 8. Exclusions, with reasons / 명시적 제외

Absorbed from PLAN-MOE §4 and extended. Each row is sketched — what it is, the
concrete trigger that would un-defer it, and the v1 we would build — in
[`PLAN-DEFERRED.md`](PLAN-DEFERRED.md) (2026-08-13), so "excluded" means
"designed enough to resume" rather than "forgotten".
아래 각 행은 [`PLAN-DEFERRED.md`](PLAN-DEFERRED.md)에 방아쇠와 v1 스케치까지
펼쳐져 있다 — 제외가 "잊음"이 아니라 "재개 가능"을 뜻하도록.

| Service | Why not |
|---|---|
| Transaction (OTS) | a graveyard of partial implementations; nothing in the control plane needs cross-object atomicity; honest absence beats a decorative `Current` |
| CosNotification | the heavyweight Event superset (structured events, filters, QoS admin) — plain CosEvent serves every named consumer; revisit only if filtering moves server-side |
| Time / PSS / Concurrency / Collections | no consumer names them; adopt-on-demand |
| Federated Naming / Trading Link | tenancy (F5) may name the requirement; until then out |
| Relationship / Containment / Reference / CompoundLifeCycle / ObjectIdentity | **added 2026-08-26**, and they are not "no consumer" — the relationships, the traversal and the identity test all exist in the tree and the standard's names for them do not. Five modules of two specifications; reasoned and triggered in [`PLAN-DEFERRED`](PLAN-DEFERRED.md) §13–§17 |
| Externalization / Query / Licensing | **added 2026-08-26** — three unrelated reasons, one each: a per-element remote stream against a blob that is opaque on purpose, a language service whose type tags admit only SQL-92 and OQL-93, and a metering mechanism we have under a principal we do not. [`PLAN-DEFERRED`](PLAN-DEFERRED.md) §18–§20 |

행별 이유. **트랜잭션(OTS)** — 반쪽 구현들의 무덤이고, 컨트롤 플레인에서 객체 간
원자성을 필요로 하는 것이 없다. 장식용 `Current`보다 정직한 부재가 낫다.
**CosNotification** — 구조화 이벤트·필터·QoS 관리를 얹은 Event의 무거운 상위
집합이며, 이름 붙은 소비자 전부는 평범한 CosEvent로 충분하다. 필터링이 서버 쪽으로
옮겨갈 때에만 다시 본다. **Time / PSS / Concurrency / Collections** — 이름을 대는
소비자가 없다. 필요해지면 그때 채택한다. **연합 네이밍 / 트레이딩 링크** — 테넌시
(F5)가 요구를 명명할 수 있으나, 그전까지는 범위 밖이다.
**관계 / 포함 / 참조 / 복합 생명주기 / 객체 동일성**(2026-08-26 추가) — "소비자 없음"이
아니다. 관계도, 순회도, 동일성 검사도 **나무 안에 이미 있고 표준의 이름만 없다**. 두
명세의 다섯 모듈이며 사유와 방아쇠는 [`PLAN-DEFERRED`](PLAN-DEFERRED.md) §13–§17에 있다.
**외부화 / 질의 / 라이선싱**(2026-08-26 추가) — 서로 무관한 세 사유가 하나씩이다:
일부러 불투명한 블롭을 상대로 한 원소 단위 원격 스트림, 표지가 SQL-92와 OQL-93만
허용하는 언어 서비스, 그리고 기구는 있으나 주체가 없는 계량. §18–§20.

## 8.1 Operations absent without a reason, 2026-08-14 — 12건 / 이유 없는 부재

**The reading below is 2026-08-14's, and every row carries its own date.** On
that day `SERVICES-COVERAGE.md` probed all 107 declared operations over the
wire and found **12 answering `BAD_OPERATION` with no reason written
anywhere**; the rows record what happened to each since. The current count is
the generated block `SERVICES-COVERAGE.md` §8 and is not restated here — the
heading and this sentence used to be undated and present-tense over dated rows,
which reads as today's number and had stopped being one. The wire cannot
distinguish a considered refusal from a forgotten one, so a
`BAD_OPERATION` nobody wrote a sentence about is a gap by definition. This
section is that sentence, written now — and where the honest answer is "nobody
decided", it says so rather than inventing a rationale after the fact.

| Absent | Verdict |
|---|---|
| `NamingContextExt::to_url` | ~~A gap~~ — **served 2026-08-14**, and measured against two producers: omniORB resolved a URL ours built, and omniNames was compared over 14 argument pairs. |
| `Repository::get_canonical_typecode`, `get_primitive` | **Deferred, reason now recorded**: both hand out `TypeCode`s the registry never stored — a canonical form and the primitives table — so serving them means minting type information rather than reporting it, which is the one thing a read-only facade must not do. **Answers `NO_IMPLEMENT` since 2026-08-14.** |
| `Container::lookup_name`, `describe_contents` | **Deferred, same class as §7's five**: they enumerate a container's contents, and `describe_interface` already carries what a client wanted from them. Listed with the others rather than left absent. **Answers `NO_IMPLEMENT` since 2026-08-14 — and so do §7's five, which is what "same class" now means on the wire and not only on paper.** |
| `Contained::_get_version` | **Was a defect, not a deferral. Fixed 2026-08-14: it is served.** Its write half `_set_version` answered `NO_PERMISSION` — "the operation exists and the answer is no" — while its read half said "no such operation", on data the registry already parses out of every repository id. Backwards by `ifr.rs`'s own argument. The read now answers the version from the id and the write is still refused. |
| `IDLType::_get_type` | **Deferred with the same reason as `describe_interface`'s absence used to have**: it returns a `TypeCode`, and until recently the registry loaded `::CORBA::TypeCode` as `void`. That is fixed, so this one is now merely unimplemented rather than unimplementable. **Answers `NO_IMPLEMENT` since 2026-08-14.** |
| `moe::Router::select`, `dispatch` | **`select` served 2026-08-14**, delegating to the trading engine and refusing the *whole call* with `NO_IMPLEMENT` when a constraint names a field a wire-registered offer cannot answer — a sequence of references is a complete answer or it is a refusal. `dispatch` is **excluded by D006 (approved 2026-08-14)**, together with `Expert::process`: both carry an `Activation`, nothing serves them, and bounding an operation that has never run would write an unmeasured constant into a wire contract to govern traffic nobody has sent. The return path is a new versioned interface carrying a bounded handle, where §5.3 already prescribes the version bump. Originally: **split, and only half undecided** — reasoned in [`PLAN-MOE.md`](PLAN-MOE.md) §4.6. `select` returns references only, is pure control plane, and its absence is a **gap**. `dispatch` (and `Expert::process`) carry an `Activation`, which is control-plane-legal only under the reading that `Tensor` holds a handle rather than a payload — a reading that lives in a corpus comment, binds nothing, and is enforced by nothing. Committing to it is the decision. |

**부재 12건에 대한 문장 — 2026-08-14 판독이며, 아래 각 행은 자기 날짜를
달고 있다.** 지금의 수는 `SERVICES-COVERAGE.md` §8의 생성 블록이며 여기에 다시
적지 않는다(제목과 첫 문장은 날짜 없는 현재형으로 날짜 붙은 행들 위에 서 있었고,
그것은 오늘의 수로 읽히지만 이미 오늘의 수가 아니었다).
와이어는 숙고된 거부와 잊힌 거부를 구분하지 못하므로,
아무도 문장을 쓰지 않은 `BAD_OPERATION`은 정의상 공백이다. `to_url`과
`_get_version`은 **결함**이고, 나머지는 이유를 붙여 유예하며, `moe::Router`는
**미결정**이라고 적는다 — 사후에 근거를 지어내지 않는다.

**2026-08-14 갱신.** `_get_version`은 **서빙되고**(읽기는 답하고 쓰기는 계속
`NO_PERMISSION`), IFR의 유예 연산 10개는 `BAD_OPERATION`이 아니라
`NO_IMPLEMENT`로 답한다 — 이제 와이어 자체가 유예와 누락을 구분한다. §7 참조.
나머지 항목(`to_url`, `moe::Router`)은 이 배치의 범위 밖이며 그대로 남는다.

### 8.1.1 The rule the IFR found, applied everywhere / 그 규칙을 전면 적용

**2026-08-18.** §7's answer was right and was only in one servant. Every other
deliberate non-implementation still said `BAD_OPERATION` — *no such operation*
— which is what an oversight says, so the decision lived in a document the
client cannot read. It is now the wire's job in all five services:

| answer | means |
|---|---|
| `NO_PERMISSION` | the operation exists and the answer is no, as policy |
| `NO_IMPLEMENT` | the operation is declared and this servant does not implement it, on purpose |
| `BAD_OPERATION` | this interface does not declare that name at all |

Moved to `NO_IMPLEMENT` on 2026-08-18: the event channel's supplier-side pull
and its `destroy`; and — then re-examined the same day — CosNaming's
`bind_context`, `rebind_context`, `destroy` and the consumer side of pull,
which turned out to be *possible* rather than deferred and are **served**
(§2, §4; the generated coverage block is the home); and
`moe::Router::dispatch`, whose exclusion **D006 approved on 2026-08-14 while
the servant went on saying "no such operation"** — a decision recorded in
prose and contradicted on the wire, which is the exact failure §8.1 exists to
name, committed by the section that names it.

`moe::Expert` (as `corpus/golden/22` declares it) is **claimed by no object in
the control plane, deliberately.** The registry *stores* expert references and
the experts themselves are served elsewhere — `corpus/golden/23`'s
`EnterpriseExpert` and the shared `::moe::Expert` both answer — so an `Expert`
servant beside the registry would be a second, weaker implementation of
something already served. `SERVICES-COVERAGE.md` observed that this was
"defensible by design … but that sentence is nowhere written". This is the
sentence. It is an unserved *interface*, which the sweep now reports as its own
fact rather than as five missing operations.

**전면 적용.** §7이 찾은 답은 옳았고 서번트 하나에만 있었다. 나머지의 의도적
미구현은 여전히 `BAD_OPERATION`(*그런 연산 없음*) — 누락이 내는 답 — 이었으므로,
결정은 클라이언트가 읽을 수 없는 문서에만 있었다. 이제 다섯 서비스 전부에서
와이어가 그 일을 한다. 특히 `moe::Router::dispatch`는 **D006이 2026-08-14에
제외를 승인했는데 서번트는 계속 "그런 연산 없음"이라고 답하고 있었다** — 산문에
기록된 결정을 와이어가 부정한 것으로, §8.1이 이름 붙이려던 실패를 §8.1 자신이
저지른 셈이다. `moe::Expert`는 컨트롤 플레인의 어떤 객체도 **의도적으로**
자처하지 않으며(레지스트리는 참조를 저장할 뿐, 익스퍼트는 다른 곳에서 서빙된다),
그 문장이 어디에도 없다던 지적에 대한 답이 이 문단이다.

## 9. Fixture & dependency policy / 픽스처·의존성 정책

No service in this plan adds a Cargo dependency — pure spec implementation.
Fixtures follow the sslTP-probe precedent: probe first, quote the measured
output, a BLOCKED probe is a valid batch result. Probes on record: omniNames
(present, used since Phase 1), omniORBpy CosNaming/CosEventComm stubs
(present, measured), omniORBpy **IR stubs** (present as `omniORB.ir_idl` —
`hasattr(CORBA, "Repository")` is `False` on a bare `import CORBA` and `True`
after the extra import; that one line is what made §7's oracle possible),
omniEvents (absent — recorded above), sslTP (absent —
`spikes/tls/PEER-STATUS.md`).

이 계획의 어떤 서비스도 Cargo 의존성을 추가하지 않는다 — 순수한 명세 구현이다.
픽스처는 sslTP 프로브의 선례를 따른다: 먼저 프로브하고, 측정된 출력을 인용하고,
BLOCKED 프로브도 유효한 배치 결과로 친다. 기록에 남은 프로브: omniNames(있음,
Phase 1부터 사용), omniORBpy의 CosNaming/CosEventComm 스텁(있음, 측정됨),
omniORBpy의 **IR 스텁**(`omniORB.ir_idl`로 있음 — 맨 `import CORBA`만 하면
`hasattr(CORBA, "Repository")`가 `False`이고 그 임포트를 더하면 `True`가 된다.
§7의 오라클을 가능하게 한 것이 그 한 줄이다), omniEvents(없음 — 위에 기록),
sslTP(없음 — `spikes/tls/PEER-STATUS.md`).

## 10. Sequencing / 순서

| Batch | After | Why that order |
|---|---|---|
| F6 Naming server | — | **landed 2026-08-14** |
| F7 Event channel | F6 | **landed** (push both ways; consumer-side pull 2026-08-18) — the channel is a named object; discovery wants Naming |
| IFR facade (§7) | F6 | **landed** — same reason |
| Trading wire (`ExpertRegistry` served) | F3 | **landed 2026-08-15** (§3) — the loader/state machine is its first caller |
| F5 LifeCycle/Property | F2 ✅ + F4 | **landed 2026-08-14** as `tenant_service.rs`, and this row never grew the marker the F6 and IFR rows have — measured 16/16 in `SERVICES-COVERAGE.md` §7. What was missing was the *direction*: 2026-08-18 an omniORB client calls all sixteen through its own stubs (what F5 still lacks is `COMPONENTS.md`'s gap column, not this table's — a clause restating it stood here) |
| CosEvent → telemetry feedback | F4 + F7 | **both exist since 2026-08-18 and nothing publishes** a control-plane event into the channel — re-measured 2026-08-25: no use of the event servant anywhere in `orbweaver-mcp`, `orbweaver-object` or `orbweaver-trading`. The design note this row asked for now exists — [`docs/decisions/D011-control-plane-events.md`](decisions/D011-control-plane-events.md), STATUS **PROPOSED**, drafted 2026-08-19 — and it answers "what is published, what is not (the §5 trust boundary)"; this cell went on saying "needs a short design note first", one document behind. D011 deliberately does not edit this row: its §11 writes out, unapplied, what this cell becomes under each option, so the batch that closes the row applies it in one edit |

순서의 이유, 행별로. **F6 네이밍 서버** — 선행 없음, **2026-08-14 착지**.
**F7 이벤트 채널** — F6 다음. **착지**(push는 양방향, consumer 쪽 pull은
2026-08-18). 채널은 이름 붙은 객체이고, 발견에는 네이밍이 필요하다.
**IFR 파사드(§7)** — F6 다음, **착지**, 같은 이유. **트레이딩 와이어
(`ExpertRegistry` 서빙)** — F3 다음, **2026-08-15 착지**(§3). 로더/상태 머신이
그 첫 호출자다. **F5 라이프사이클/프로퍼티** — F2 ✅와 F4 다음,
`tenant_service.rs`로 **2026-08-14 착지**. 이 행은 F6·IFR 행이 가진 표시를 끝내
달지 못했는데, `SERVICES-COVERAGE.md` §7에서 16/16으로 측정되어 있었다. 빠져
있던 것은 *방향*이며, 2026-08-18, omniORB 클라이언트가 자기 스텁으로 열여섯
연산을 모두 호출했다(F5에 아직 없는 것은 `COMPONENTS.md`의 공백 열 소관이지 이
표의 소관이 아니다 — 그것을 다시 적은 절이 여기 서 있었다).
**CosEvent → 텔레메트리 되먹임** — F4와 F7 다음. **양쪽 다 2026-08-18 이후
존재하지만 아무것도 발행하지 않는다**. 2026-08-25 재측정: `orbweaver-mcp`,
`orbweaver-object`, `orbweaver-trading` 어디에서도 이벤트 서번트를 쓰지 않는다.
이 행이 요구한 설계 노트는 이제 존재한다 —
[`docs/decisions/D011-control-plane-events.md`](decisions/D011-control-plane-events.md),
상태 **제안**, 2026-08-19 작성 — 그리고 "무엇을 발행하고 무엇을 발행하지 않는가
(§5의 신뢰 경계)"에 답한다. 이 칸은 그동안에도 "짧은 설계 노트가 먼저 필요하다"고
적고 있었다 — 문서 하나만큼 뒤처진 채로. D011은 이 행을 의도적으로 고치지 않는다.
그 §11이 선택지별로 이 칸이 무엇이 되는지를 **미적용 상태로** 써 두었으므로, 행을
닫는 배치가 한 번의 편집으로 적용한다.
