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

Not doing (until a consumer appears): real `BindingIterator` lifecycles,
`bind_context`/`destroy` (refused loudly), federation across naming domains
(F5's tenancy work will name the requirement if it materializes).

## 3. CosTrading / 트레이딩 — ◐ decision engine landed, wire deferred

| | |
|---|---|
| Standard | `CosTrading` (Lookup, Register, Admin, Link, Proxy) |
| Consumer | stream F loading/placement (PLAN-MOE §6), later the catalog |
| Today (F2, 2026-08-14) | `orbweaver-trading`: offers, constraint-query subset, loading policy, deterministic trace replay — 37 tests |

**The honest choice about the standard module:** OMG CosTrading is enormous
(five interfaces, federated links, proxy offers, dynamic properties). Nothing
in stream F consumes more than property-constrained lookup over registered
offers, which the engine already does with S4-style positioned query errors.
The **wire surface lands after F3** as the *project* contract
(`moe::ExpertRegistry` from `corpus/golden/22`), served on our POA like F6;
the standard `CosTrading::Lookup::query` facade is deferred until a foreign
trading client is named — the IFR-facade rule (§7) applied to Trading.
Deferral is recorded here so it is a decision, not a drift.

표준 CosTrading 모듈 전체는 소비자가 없다 — 스트림 F가 쓰는 것은 속성 제약
조회뿐이며 그것은 착지되었다. 와이어 표면은 F3 이후 프로젝트 계약으로, 표준
파사드는 외부 트레이딩 클라이언트가 명명될 때. 유예는 표류가 아니라 결정이다.

## 4. CosEvent / 이벤트 — ❌ → F7, oracle design settled by measurement

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
— the push model only; pull is refused loudly. Events are `any`, which
AnyJSON already carries.

Batch unit: the channel × both directions (our supplier → omniORB consumer,
omniORB supplier → our consumer) × disconnect semantics (a dead consumer must
not wedge the channel — bounded buffer, drop-oldest, drops counted and
reported, per the "no silent truncation" rule).

측정이 오라클을 결정했다: omniEvents 픽스처는 없으나 omniORBpy가 CosEventComm
스텁을 싣고 있으므로, F6과 같은 방향 — 채널은 우리가 서빙하고 독립 ORB가 consumer/
supplier로 접속 — 이 가능하다. push 모델만, 컨트롤 플레인 입도만.

## 5. CosLifeCycle & CosProperty / 생명주기·프로퍼티 — F5

`ModelFactory` (create/clone_model/deploy/retire, `corpus/golden/23`) is the
GenericFactory shape with the standard's genericity dropped: typed factories
per contract, because an untyped `create(key, criteria)` is exactly the
stringly-typed surface S4 exists to prevent. `retire` is `ai_effect:
destructive` and rides the existing approval gate. CosProperty stays folded
into the `Manifest` struct until a second consumer exists — the F6 rule: no
standalone service without a named caller. Tenancy isolation tests take the
cross-session capability-table shape already proven in the MCP batches.

## 6. Security — cross-reference only / 교차 참조

CSIv2 wire, delegation policy, credential hygiene and `ai_authz` scopes are
PHASE5 + guard work and stay there; this plan adds nothing on top. The one
service-suite note: none of the services above invent their own authorization
— a naming `bind` or channel `connect` at the MCP boundary passes the same
`Exposure`/`Guarded` gate as any other operation (I1's rule, inherited).

## 7. Interface Repository facade / IFR 파사드 — batch shape for an old line

`orbweaver-registry` is the first-party IFR equivalent (stated since Phase 2).
PLAN §7's old "optional read-only `CORBA::Repository` facade" line gets its
batch shape here: serve `_get_interface`-adjacent lookups (`lookup_id`,
`describe_interface`-equivalent) read-only over the registry, on our POA,
**after F6** (the facade is a named object that wants Naming), with omniORB's
python IRObject client as the cross-ORB oracle. Write operations refused
loudly — the registry is populated from IDL through S4, never over the wire.

## 8. Exclusions, with reasons / 명시적 제외

Absorbed from PLAN-MOE §4 and extended:

| Service | Why not |
|---|---|
| Transaction (OTS) | a graveyard of partial implementations; nothing in the control plane needs cross-object atomicity; honest absence beats a decorative `Current` |
| CosNotification | the heavyweight Event superset (structured events, filters, QoS admin) — plain CosEvent serves every named consumer; revisit only if filtering moves server-side |
| Time / PSS / Concurrency / Collections | no consumer names them; adopt-on-demand |
| Federated Naming / Trading Link | tenancy (F5) may name the requirement; until then out |

## 9. Fixture & dependency policy / 픽스처·의존성 정책

No service in this plan adds a Cargo dependency — pure spec implementation.
Fixtures follow the sslTP-probe precedent: probe first, quote the measured
output, a BLOCKED probe is a valid batch result. Probes on record: omniNames
(present, used since Phase 1), omniORBpy CosNaming/CosEventComm stubs
(present, measured), omniEvents (absent — recorded above), sslTP (absent —
`spikes/tls/PEER-STATUS.md`).

## 10. Sequencing / 순서

| Batch | After | Why that order |
|---|---|---|
| F6 Naming server | — | **landed 2026-08-14** |
| F7 Event channel | F6 | the channel is a named object; discovery wants Naming |
| IFR facade (§7) | F6 | same reason |
| Trading wire (`ExpertRegistry` served) | F3 | the loader/state machine is its first caller |
| F5 LifeCycle/Property | F2 ✅ + F4 | factories bind experts; policy rides the interceptor chain |
| CosEvent → telemetry feedback | F4 + F7 | the §6 feedback loop closes only when both exist |
