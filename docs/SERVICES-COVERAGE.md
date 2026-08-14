# SERVICES-COVERAGE — what the five served services actually implement

> Measured 2026-08-14 by `spikes/service_sweep.sh`, over the wire, against the
> running servants. Companion to [`PLAN-SERVICES.md`](PLAN-SERVICES.md) (what
> was planned) and [`COMPONENTS.md`](COMPONENTS.md) (what says ✅).
> 2026-08-14 측정. 계획(`PLAN-SERVICES.md`)·상태(`COMPONENTS.md`)와 달리 이
> 문서는 **실제로 와이어에서 응답한 것**만 적는다.

## 1. Why this document exists / 이 문서가 존재하는 이유

`COMPONENTS.md` marks five services ✅ and every one of them implements a
*subset* of what its IDL declares — deliberately, with reasons in the servants'
module docs. Nobody had produced the list. A reader of a ✅ therefore could not
tell a **considered refusal** from an **omission**, and that difference is the
whole distance between an honest ✅ and a misleading one.

`COMPONENTS.md`의 ✅ 다섯 줄은 각각 IDL이 선언한 것의 **부분집합**을 구현하며,
그 이유는 서번트 모듈 문서에 있다. 그러나 목록은 아무도 만들지 않았다. 그래서
독자는 **숙고된 거부**와 **누락**을 구분할 수 없었고, 그 차이가 정직한 ✅과
오해를 부르는 ✅ 사이의 거리 전부다.

## 2. Method / 방법

**The declared operations are read out of IDL, never typed in.** A hand-typed
operation list is a claim about a specification; this is a reading of one.
`spikes/service_sweep.py` runs `omniidl -b dump` as an external program and
parses its text output — clause (b) of the licensing boundary, no import of
anything omniORB ships. Attributes are expanded into the `_get_`/`_set_`
operations they become on the wire (§11.3.7), because that is the name a
request actually carries.

**Probed and found present** (the probe is part of the measurement, not an
assumption): omniORB 4.3.4's own IDL directory at
`/opt/homebrew/share/idl/omniORB`, containing `COS/CosNaming.idl`,
`COS/CosEventComm.idl`, `COS/CosEventChannelAdmin.idl` and `ir.idl`.
`CosEventChannelAdmin.idl` does not compile without `-I` at its own COS
directory — the sweep passes it. The two project contracts are read from
`corpus/golden/22-moe-control-plane.idl` and
`corpus/golden/23-moe-enterprise.idl`, and are read **separately**: each
declares its own `moe::Expert` and golden 22's carries a `delegate` that golden
23's does not, so merging them would invent an operation neither servant's
contract asks for.

**Every classification is a real GIOP 1.2 request, not a reading of source.**
A match table built by reading `match req.operation.as_str()` cannot catch an
operation that is dispatched but broken. The driver speaks GIOP over a socket
with nothing but the Python standard library — deliberately *not* our own
client, because our client and our server were written together and share every
assumption that could hide a fault.

**The probe body is 64 zero bytes.** Not a trick: an *empty* body makes
`Request::body()` fail, every servant maps that to `MARSHAL`, and every
operation would then look present. Sixty-four zeros decode as empty strings,
empty sequences and nil references, so an operation that exists either answers,
raises, or returns `MARSHAL` — and one that does not exist answers
`BAD_OPERATION`, which is the distinction being measured. Real calls with real
arguments are made first, in each service's `walk`, because the probes are
degenerate by design and some have side effects.

선언 목록은 IDL에서 **읽어낸다** — 손으로 타이핑한 목록은 명세에 대한 *주장*이지
명세를 *읽은 것*이 아니다. `omniidl -b dump`를 외부 프로그램으로 실행해 텍스트
출력만 읽는다(라이선스 경계 (b)항). 픽스처는 가정하지 않고 탐지했다:
`/opt/homebrew/share/idl/omniORB`. 프로젝트 계약 두 개는 **따로** 읽는다 —
둘 다 `moe::Expert`를 선언하며 내용이 다르기 때문이다. 분류는 전부 실제 GIOP
1.2 요청이며, 우리 클라이언트가 **아닌** 파이썬 표준 라이브러리로 말한다.
프로브 본문은 0바이트 64개다 — 빈 본문은 모든 연산을 `MARSHAL`로 만들어 존재하는
것처럼 보이게 하므로.

### The classification, and its one honest limit / 분류와 그 한계

| verdict | what the wire showed | what makes it that verdict |
|---|---|---|
| **served** | a reply, a user exception, or `MARSHAL` | the operation name resolved and its argument decoder ran |
| **refused, with a reason** | `NO_PERMISSION`, or `BAD_OPERATION` **and** a reason written in the servant or the plan | quoted below, so a reader can judge it |
| **absent** | `BAD_OPERATION` and no reason written anywhere | nobody decided this; it is a gap |

**The wire cannot tell the last two apart.** A refused pull operation and a
forgotten one both answer `BAD_OPERATION`. So the wire supplies the fact and
the documents supply the reason, and an operation with the fact and no reason
is exactly what this batch was run to find.

Two limits, stated rather than discovered later. `MARSHAL` proves an operation
is *dispatched*; it does not prove it is *correct* — the operations that got a
real call with real arguments are the ones quoted in each section, and the rest
rest on `spike-names`/`spike-events`/`spike-ifr`/`spike-experts`/`spike-tenants`.
And `_is_a`/`_non_existent` are `CORBA::Object` pseudo-operations, not part of
any service's declared count; both answered on all fourteen objects addressed.

와이어는 마지막 두 판정을 구분하지 **못한다** — 거부된 pull 연산과 잊힌 연산은
둘 다 `BAD_OPERATION`이다. 그러므로 사실은 와이어가, 이유는 문서가 댄다. 사실만
있고 이유가 없는 연산 — 그것이 이 배치가 찾으려던 것이다. 한계 둘: `MARSHAL`은
*디스패치*를 증명할 뿐 *정확성*을 증명하지 않으며, `_is_a`/`_non_existent`는
어느 서비스의 선언 수에도 넣지 않았다(주소 지정한 14개 객체 전부에서 응답함).

## 3. CosNaming — 10 of 14 on the context, 0 of 3 on the iterator

Object addressed: the root `NamingContextExt` published by
`spike-names --hold`. Source: `COS/CosNaming.idl`.

| Operation | Interface | Verdict | Wire | Reason, quoted |
|---|---|---|---|---|
| `resolve` | NamingContext | served | `NotFound` on a bad name, the bound reference on a good one | — |
| `bind` | NamingContext | served | `MARSHAL` on the probe; real bind measured by `spike-names` | — |
| `rebind` | NamingContext | served | `MARSHAL` | — |
| `unbind` | NamingContext | served | `InvalidName` | — |
| `new_context` | NamingContext | served | a fresh context reference | — |
| `bind_new_context` | NamingContext | served | `InvalidName` on the empty name | — |
| `list` | NamingContext | served | `list(100)` → 2 bindings + a **nil** iterator | "a truncated `list` under-reports — a caller that wants the full set passes a large `how_many`" |
| `to_string` | NamingContextExt | served | `InvalidName` on the empty name | — |
| `to_name` | NamingContextExt | served | `to_name('a/b')` → `[('a',''),('b','')]` | — |
| `resolve_str` | NamingContextExt | served | `resolve_str('spike/Echo')` → object key `Echo`; `'nope'` → user `NotFound` | — |
| `bind_context` | NamingContext | refused | `BAD_OPERATION` | "They would bind a *foreign* context, and resolving through one means chaining the call over the wire — not v1 work." |
| `rebind_context` | NamingContext | refused | `BAD_OPERATION` | same sentence (the module docs name both) |
| `destroy` | NamingContext | refused | `BAD_OPERATION` | "contexts live as long as the process, and an unbound context stays reachable by its key" |
| **`to_url`** | NamingContextExt | **absent** | `BAD_OPERATION` | **nothing. The string does not occur anywhere in the repository.** |
| `next_one` | BindingIterator | refused | unreachable — no iterator object exists | "Real iterators need servant lifecycle, which is POA work." |
| `next_n` | BindingIterator | refused | unreachable | same |
| `destroy` | BindingIterator | refused | unreachable | same |

**Declared 17 · served 10 · refused with a reason 6 · absent 1.**

`to_url` is the finding. It is the one `NamingContextExt` operation the project
already has the machinery for — `crate::naming` parses `corbaname:` URLs on the
client side, and `to_url` is the operation that *produces* one — and it is the
only operation in CosNaming refused by the servant and mentioned by no document,
no test and no plan.

주소 지정 객체: `spike-names --hold`가 발행한 루트 `NamingContextExt`.
**선언 17 · 서빙 10 · 이유 있는 거부 6 · 부재 1.** 발견은 `to_url`이다 —
클라이언트 쪽에 `corbaname:` 파서가 이미 있어 기계장치는 갖춰져 있는데,
서번트는 거부하고 어떤 문서·테스트·계획도 그것을 언급하지 않는다.

## 4. CosEvent — 9 of 12 reachable, and the whole pull half refused as designed

Objects addressed: the channel from `spike-events --hold`, then
`for_consumers`, `for_suppliers`, `obtain_push_supplier` and
`obtain_push_consumer` walked over the wire — five objects. Source:
`COS/CosEventChannelAdmin.idl` + `COS/CosEventComm.idl`.

| Operation | Object | Verdict | Wire | Reason, quoted |
|---|---|---|---|---|
| `for_consumers` | EventChannel | served | a `ConsumerAdmin` reference | — |
| `for_suppliers` | EventChannel | served | a `SupplierAdmin` reference | — |
| `obtain_push_supplier` | ConsumerAdmin | served | a `ProxyPushSupplier` reference | — |
| `obtain_push_consumer` | SupplierAdmin | served | a `ProxyPushConsumer` reference | — |
| `connect_push_consumer` | ProxyPushSupplier | served | `MARSHAL` on the probe; `BAD_PARAM` on nil, per §2.3.6 | — |
| `disconnect_push_supplier` | ProxyPushSupplier | served | reply (idempotent) | — |
| `connect_push_supplier` | ProxyPushConsumer | served | reply | — |
| `push` | ProxyPushConsumer | served | before connect → user `Disconnected`; after → reply | — |
| `disconnect_push_consumer` | ProxyPushConsumer | served | reply | — |
| `destroy` | EventChannel | refused | `BAD_OPERATION` | "Destroying a channel means calling `disconnect_push_consumer` back on every attached consumer — outbound invocations, from inside a servant, whose failures have nowhere to go — and then invalidating object keys that other references still name." |
| `obtain_pull_supplier` | ConsumerAdmin | refused | `BAD_OPERATION` | "Pull inverts the flow control: the channel would have to *hold* events until somebody asks, which is the same unbounded buffer this module spends its bounded queue avoiding, for no named consumer." |
| `obtain_pull_consumer` | SupplierAdmin | refused | `BAD_OPERATION` | same |
| `connect_pull_consumer`, `pull`, `try_pull`, `disconnect_pull_supplier` | ProxyPullSupplier + PullSupplier | refused | `BAD_OPERATION` — probed against the push proxy, since no pull proxy can be obtained | same |
| `connect_pull_supplier`, `disconnect_pull_consumer` | ProxyPullConsumer + PullConsumer | refused | `BAD_OPERATION` | same |

**Declared 18 · served 9 · refused with a reason 9 · absent 0.**

This is the cleanest of the five: every operation not served has a reason
written in the servant, and the ratio is exactly what PLAN-SERVICES §4 scoped.
The pull interfaces are refused *by construction* — no `ProxyPullSupplier`
object can be obtained at all — so their operations were probed against the
push proxies, which is the strongest statement the wire can make about an
operation nothing can address.

주소 지정 객체 다섯. **선언 18 · 서빙 9 · 이유 있는 거부 9 · 부재 0.**
다섯 중 가장 깨끗하다: 서빙하지 않는 모든 연산에 서번트가 쓴 이유가 있다.
pull 인터페이스는 객체 자체를 얻을 수 없으므로, push 프록시에 프로브를 던져
"어떤 객체도 이 연산을 답하지 않는다"는 진술을 측정으로 만들었다.

## 5. Interface Repository — 8 of 44, and 6 refusals nobody wrote down

Objects addressed: the root `Repository` from `spike-ifr --hold`, and the
`InterfaceDef` for `IDL:gc10/Both:1.0` obtained through `lookup_id`. Source:
`ir.idl`. Sixty-two probes over two objects; forty-four *distinct* operations,
because `Container` and `IRObject` are inherited by both.

| Operation | Interface | Verdict | Wire |
|---|---|---|---|
| `lookup_id` | Repository | served | `IDL:gc10/Both:1.0` → an `InterfaceDef` reference |
| `_get_def_kind` | IRObject | served | `5` = `dk_Interface` on the entry, `dk_Repository` on the root |
| `_get_id` | Contained | served | `'IDL:gc10/Both:1.0'` |
| `_get_name` | Contained | served | `'Both'` |
| `_get_absolute_name` | Contained | served | `'::gc10::Both'` |
| `describe_interface` | InterfaceDef | served | name `'Both'`, id `'IDL:gc10/Both:1.0'` |
| `_get_base_interfaces` | InterfaceDef | served | 2 bases |
| `is_a` | InterfaceDef | served | `is_a('IDL:gc10/Nameable:1.0')` → `true` |
| 25 mutating operations | all five interfaces | refused | `NO_PERMISSION`, **before target resolution** |
| `lookup`, `contents`, `describe`, `_get_defined_in`, `_get_containing_repository` | Container / Contained | refused | `BAD_OPERATION` |
| **`get_canonical_typecode`, `get_primitive`** | Repository | **absent** | `BAD_OPERATION` |
| **`lookup_name`, `describe_contents`** | Container | **absent** | `BAD_OPERATION` |
| **`_get_version`** | Contained | **absent** | `BAD_OPERATION` |
| **`_get_type`** | IDLType | **absent** | `BAD_OPERATION` |

The 25 mutating refusals, quoted: *"the registry is populated from IDL through
S4, never over the wire… A writable IFR would be a second ingestion path with
none of those gates on it, so it is refused at the servant rather than left to
deployment configuration."* And on the choice of exception: *"`BAD_OPERATION`
would have been the wrong answer: it says 'no such operation', and a client
would reasonably retry against a different reference. `NO_PERMISSION` says the
operation exists and the answer is no."* Measured true without exception —
`is_mutating` fires on every `create_*`, every `_set_*`, `destroy` and `move`,
25 distinct operations, on both objects, with no argument decoded first.

The five documented `BAD_OPERATION`s, quoted: *"`Container::contents`/`lookup`,
`Contained::describe`, `_get_defined_in` and `_get_containing_repository` are
**not** served… `describe_interface`'s `defined_in` member already carries the
containing module's repository id, which is what a client wanted
`_get_defined_in` for."*

**Declared 44 distinct · served 8 · refused with a reason 30 · absent 6.**

The six absences are the finding. `_get_version` is the sharpest: its *write*
half `_set_version` is refused `NO_PERMISSION` — "the operation exists and the
answer is no" — while its *read* half answers "no such operation", on a servant
whose registry knows the version, because it is part of every repository id it
already parses. A read-only facade that refuses the write and denies the read
has the two backwards.

객체 둘, 프로브 62회, **서로 다른 연산 44개 · 서빙 8 · 이유 있는 거부 30 ·
부재 6.** 변경 연산 25개 전부가 인자를 읽기도 전에 `NO_PERMISSION`이라는 것은
측정으로 확인되었다. 발견은 부재 6개이며, 그중 `_get_version`이 가장 날카롭다 —
*쓰기* 쪽 `_set_version`은 "연산은 있고 답은 아니오"(`NO_PERMISSION`)인데
*읽기* 쪽은 "그런 연산 없음"(`BAD_OPERATION`)이다. 레지스트리는 이미 모든
리포지터리 ID에서 버전을 파싱하고 있다. 읽기 전용 파사드가 두 방향을 거꾸로
답하고 있는 셈이다.

> **Acted on 2026-08-14 and since re-measured by this same sweep**, which now
> runs in `run_checks.sh`: IFR reports **probes 66 · dispatched 28 ·
> `NO_PERMISSION` 38 · `BAD_OPERATION` 0**. The six absences this section found
> are gone — not by serving everything, but by making a deferral answer
> differently from an oversight, which is the distinction §2 says the wire
> cannot make on its own. `BAD_OPERATION` still means "nobody decided", and it
> now has no instances in this service.
>
> The original acting note follows. `ifr.rs`
> now serves `_get_version` (the write half is still `NO_PERMISSION`), and the
> ten operations it defers — `contents`, `lookup`, `lookup_name`,
> `describe_contents`, `describe`, `_get_defined_in`,
> `_get_containing_repository`, `get_canonical_typecode`, `get_primitive`,
> `_get_type` — answer **`NO_IMPLEMENT`** instead of `BAD_OPERATION`, so the
> distinction §2 says the wire cannot make is one the wire now makes for this
> service. The table above is the state the sweep measured; the new answers are
> covered by `ifr.rs`'s own tests and by `spike-ifr`, and a re-run of
> `./spikes/service_sweep.sh` is what would turn them back into a measurement
> here. It has not been run: this batch had no omniORB fixture available
> (`omniidl` absent), and an unmeasured check is not a pass.
>
> **2026-08-14 조치, 이 스윕으로 재측정되지 않음.** `_get_version`은 서빙되고,
> 유예 연산 10개는 `NO_IMPLEMENT`로 답한다 — §2가 "와이어는 구분하지 못한다"고
> 적은 그 구분을 이 서비스에서는 와이어가 한다. 위 표는 스윕이 측정한 상태이며,
> 새 답은 단위 테스트와 `spike-ifr`가 검증한다. `service_sweep.sh` 재실행이
> 이것을 다시 *측정*으로 만들 것이나, 이 배치에서는 omniORB 픽스처가 없어
> 실행하지 못했다.

## 6. MoE control plane (`corpus/golden/22`) — 7 of 12

Objects addressed: `moe::ExpertRegistry` and `moe::ExpertLoader`, held open by
`spikes/svc-hold` (see §9). Source: `corpus/golden/22-moe-control-plane.idl`.

| Operation | Interface | Verdict | Wire |
|---|---|---|---|
| `register_expert` | ExpertRegistry | served | a real registration replied; a duplicate is `BAD_PARAM` |
| `deregister` | ExpertRegistry | served | `MARSHAL` on the probe |
| `heartbeat` | ExpertRegistry | served | a real heartbeat replied |
| `prefetch` | ExpertLoader | served | `MARSHAL` on the probe (oneway in the IDL, answered here) |
| `evict` | ExpertLoader | served | after a `pin`, `BAD_INV_ORDER` |
| `pin` | ExpertLoader | served | reply |
| `status` | ExpertLoader | served | `status('expert-sweep')` → Residency ordinal `0` = OFFLOADED |
| **`describe`, `process`, `delegate`** | `moe::Expert` | **absent** | `BAD_OPERATION` on both served objects |
| **`select`, `dispatch`** | `moe::Router` | **absent** | `BAD_OPERATION` on both served objects |

**Declared 12 · served 7 · refused with a reason 0 · absent 5.**

The two servants serve their two interfaces completely — the module's claim
(*"the two interfaces below are served exactly as declared there, with no
operation added and none half-served"*) is measured true, 7 of 7. What no
document states is the other half of the contract. `moe::Expert` is defensible
by design: the registry *stores* expert references and the experts are served
elsewhere, so an `Expert` servant here would be wrong — but that sentence is
nowhere written. `moe::Router` has no defence recorded at all: it is declared
in the contract, named in no plan, and answered by nothing.

**선언 12 · 서빙 7 · 이유 있는 거부 0 · 부재 5.** 두 서번트는 자기 인터페이스
두 개를 완전히 서빙한다(7/7, 모듈 문서의 주장은 측정으로 참). 문서화되지 않은
것은 계약의 나머지 절반이다. `moe::Expert`는 설계상 변호 가능하지만(레지스트리는
참조를 *저장*할 뿐 익스퍼트는 다른 곳에서 서빙된다) 그 문장이 어디에도 없고,
`moe::Router`는 변호조차 기록되어 있지 않다.

## 7. MoE enterprise (`corpus/golden/23`) — 16 of 16

Objects addressed: the tenant `acme`'s `ModelFactory`, the `ComposedModel` its
`create` returned over the wire, and the `PolicyDomain`, `EnterpriseExpert` and
shared `::moe::Expert` reached by the documented, reversible key template.
Source: `corpus/golden/23-moe-enterprise.idl`.

| Operation | Object | Verdict | Wire |
|---|---|---|---|
| `create` | ModelFactory | served | a real manifest → a `ComposedModel` reference |
| `clone_model`, `retire`, `deploy` | ModelFactory | served | `MARSHAL` on the probe; measured properly by `spike-tenants` |
| `get_manifest` | ComposedModel | served | tenant `'acme'`, base `'llama-3'` |
| `infer` | ComposedModel | served | a real activation → `BAD_INV_ORDER` (no expert bound yet) |
| `bind_expert`, `set_policy` | ComposedModel | served | `MARSHAL` |
| `authorize` | PolicyDomain | served | `authorize('nobody','math')` → `false` (default-deny) |
| `check_residency` | PolicyDomain | served | `check_residency('gpu-04')` → `true` |
| `audit` | PolicyDomain | served | `MARSHAL` |
| `get_tenant_id` | EnterpriseExpert | served | `'acme'` |
| `base` | EnterpriseExpert | served | `IDL:moe/Expert:1.0`, key `MoE/enterprise/shared/base/llama-3` |
| `adapter_delta` | EnterpriseExpert | served | reply |
| `describe`, `process` | `::moe::Expert`, inherited | served | on the `EnterpriseExpert` **and** on the shared base |

**Declared 16 · served 16 · refused with a reason 0 · absent 0.**

The only service of the five whose ✅ needs no qualification. The module's claim
— *"The contract is corpus/golden/23 and nothing else: every operation it
declares is served, including the two `EnterpriseExpert` inherits from
`::moe::Expert`, and nothing it does not declare exists on the wire"* — is
measured true in both directions: 16 of 16 served, and every operation from a
*neighbouring* interface answered `BAD_OPERATION` on every object, which is what
makes these five distinct objects rather than one with a union of operations.

**선언 16 · 서빙 16 · 이유 있는 거부 0 · 부재 0.** 다섯 중 유일하게 단서가
필요 없는 ✅다. 모듈 문서의 주장이 양방향으로 참임이 측정되었다 — 16/16 서빙,
그리고 *이웃* 인터페이스의 연산은 모든 객체에서 `BAD_OPERATION`.

## 8. Totals, and the disagreements / 합계와 불일치

| Service | Declared | Served | Refused with a reason | Absent |
|---|---:|---:|---:|---:|
| CosNaming | 17 | 10 | 6 | **1** |
| CosEvent | 18 | 9 | 9 | 0 |
| Interface Repository | 44 | 8 | 30 | **6** |
| MoE control plane (golden 22) | 12 | 7 | 0 | **5** |
| MoE enterprise (golden 23) | 16 | 16 | 0 | 0 |
| **total** | **107** | **50** | **45** | **12** |

Twelve of a hundred and seven declared operations are refused by a servant and
explained by nothing. That is the number this document exists to produce.

선언 107개 중 12개가 서번트에게 거부당하지만 어디에도 설명이 없다. 이 문서가
만들어내려던 숫자가 그것이다.

### Where the measurement disagrees with the documents / 문서와의 불일치

1. **`COMPONENTS.md` line 39 says the CosNaming server landed a "full context
   surface + NamingContextExt".** Measured: 10 of 14. `to_url` is
   `BAD_OPERATION`. "Full … NamingContextExt" is wrong by one operation, and it
   is the one operation whose client-side counterpart the project already
   ships.
2. **`PLAN-SERVICES.md` §2 lists the naming refusals as "`bind_context`/
   `destroy` (refused loudly)".** Measured: four refusals, not two. The
   servant's own module docs name `rebind_context` as well; nothing names
   `to_url`.
3. **`PLAN-SERVICES.md` §7 and `ifr.rs` list five not-served IR operations.**
   Measured: eleven distinct. Six — `get_canonical_typecode`, `get_primitive`,
   `lookup_name`, `describe_contents`, `_get_version`, `_get_type` — answer
   `BAD_OPERATION` with no reason in any document.
4. **`_get_version` answers `BAD_OPERATION` while `_set_version` answers
   `NO_PERMISSION`.** By `ifr.rs`'s own argument that is backwards: the write
   half says "the operation exists and the answer is no" and the read half says
   "no such operation", on data the registry demonstrably holds. *Repaired and
   re-measured 2026-08-14: served, with the write half still `NO_PERMISSION`,
   so the pair is no longer backwards.*
5. **`corpus/golden/22` declares twelve operations and `PLAN-SERVICES.md` §3
   accounts for seven.** `moe::Router::select`/`dispatch` are declared in a
   landed contract, served by nothing, and named in no plan — not even in
   §8's exclusions table, which is where a deliberate omission would live.
6. **No disagreement found for CosEvent or for the MoE enterprise service.**
   Both match their documents operation for operation, and both documents'
   claims survive being measured. Stating that is part of the result: two of
   the five ✅ rows are exactly what they say.
7. **`COMPONENTS.md` line 42's "Writes refused `NO_PERMISSION` before target
   resolution"** is measured true across all 25 mutating operations on both
   objects — the strongest single claim in the table, and it holds.

## 9. What could not be measured, and why / 측정하지 못한 것과 그 이유

- **`spike-experts` and `spike-tenants` have no `--hold`.** `spike-names`,
  `spike-events` and `spike-ifr` do; the two MoE spikes open their serving
  windows inside `std::thread::scope` and close them again, so an external
  sweep has no way to address either servant. This sweep works around it with
  `spikes/svc-hold`, a harness-only holder outside the workspace. **Adding
  `--hold` to the two binaries is the right fix and it is a `crates/` change,
  so it is reported here rather than made.**
- **`MARSHAL` proves dispatch, not correctness.** Twelve of the fifty served
  operations are classified served on a `MARSHAL` alone — no real call with
  real arguments was made here: `bind`, `rebind` (CosNaming),
  `connect_push_consumer` (CosEvent), `deregister`, `prefetch` (golden 22),
  `clone_model`, `retire`, `deploy`, `bind_expert`, `set_policy`, `audit`,
  `process` (golden 23). The other thirty-eight answered with a reply or a user
  exception — an answer, not a decode failure — and the ones driven with real
  arguments are quoted in the tables above. Each of the twelve is exercised
  properly by its own spike; this document does not re-prove them and does not
  claim to.
  `_is_a` answered `MARSHAL` on every object for the same reason — a
  zero-length CDR string is malformed — and is a pseudo-operation outside every
  count here.
- **`prefetch` is `oneway` in the IDL and was probed as a two-way call.** The
  servant answered, which is a reply to a request that declared it expected
  one — correct here, but it means the sweep did not measure the oneway path.
- **No cross-ORB direction.** This is our client shape, over raw GIOP, against
  our servers. The "their client, our server" claims for Naming and the IFR are
  `PLAN-SERVICES.md` §2 and §7's, measured there, not re-measured here.

`spike-experts`/`spike-tenants`에 `--hold`가 없다 — 외부 스윕이 두 서번트를
주소 지정할 방법이 없다. 이 스윕은 워크스페이스 밖의 하네스 전용 홀더로
우회했으며, **올바른 수정은 두 바이너리에 `--hold`를 추가하는 것이고 그것은
`crates/` 변경이므로 하지 않고 보고한다.** `MARSHAL`은 디스패치를 증명할 뿐
정확성을 증명하지 않으며, 서빙 50개 중 12개가 `MARSHAL`만으로 분류되었다 —
각자의 스파이크가 검증한다. `prefetch`는 IDL에서 `oneway`인데 양방향으로
프로브했으므로 oneway 경로는 측정되지 않았다. 교차 ORB 방향은 여기서
재측정하지 않았다.

## 10. Reproducing / 재현

```bash
./spikes/service_sweep.sh          # human-readable: real answers and totals
./spikes/service_sweep.sh --raw    # one TSV row per (object, operation)
```

Needs `brew install omniorb` for the IDL only — the sweep runs `omniidl` as an
external program and imports nothing. If the IDL directory is missing the sweep
reports `BLOCKED` and exits non-zero, because an unmeasured check is a failure
and never a pass.

IDL만을 위해 omniORB가 필요하며, `omniidl`은 외부 프로그램으로 실행할 뿐
아무것도 import 하지 않는다. IDL 디렉터리가 없으면 `BLOCKED`을 보고하고
0이 아닌 코드로 끝난다 — 측정되지 않은 검사는 통과가 아니라 실패이므로.
