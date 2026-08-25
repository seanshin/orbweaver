# SERVICES-COVERAGE — what the five served services actually implement

> Measured by `spikes/service_sweep.sh`, over the wire, against the running
> servants. **§8 is generated from the sweep and diffed by the harness**; §3–§7
> are the first reading (2026-08-14) with its dated re-measurements, kept for
> the reasons quoted there — the part no sweep can produce. Companion to
> [`PLAN-SERVICES.md`](PLAN-SERVICES.md) (what was planned) and
> [`COMPONENTS.md`](COMPONENTS.md) (what says ✅).
> `service_sweep.sh`가 와이어에서 측정. **§8은 스윕이 생성하고 하네스가 diff한다**;
> §3–§7은 첫 판독(2026-08-14)과 날짜 붙은 재측정이며, 스윕이 만들 수 없는
> 부분 — 인용된 이유 — 때문에 남긴다. 이 문서는 **실제로 와이어에서 응답한
> 것**만 적는다.

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
any service's declared count; both answered on every object the sweep
addressed. The count is deliberately not restated here — this sentence read
"all fourteen objects addressed" while the sweep grew a fourth MoE object (§6)
and a real `ProxyPullSupplier` (§4) underneath it, and nothing compiles a
sentence. §8 is the home: the pseudo-operations are probed once per object and
counted apart there, inside each service's `Probes` total.

와이어는 마지막 두 판정을 구분하지 **못한다** — 거부된 pull 연산과 잊힌 연산은
둘 다 `BAD_OPERATION`이다. 그러므로 사실은 와이어가, 이유는 문서가 댄다. 사실만
있고 이유가 없는 연산 — 그것이 이 배치가 찾으려던 것이다. 한계 둘: `MARSHAL`은
*디스패치*를 증명할 뿐 *정확성*을 증명하지 않으며, `_is_a`/`_non_existent`는
어느 서비스의 선언 수에도 넣지 않았다(스윕이 주소 지정한 모든 객체에서 응답함).
객체 수는 여기에 다시 적지 않는다 — 이 문장이 "14개"라고 말하는 동안 스윕은 MoE
객체 하나(§6)와 진짜 `ProxyPullSupplier`(§4)를 더 얻었다. 집은 §8이며, 의사
연산은 객체마다 한 번씩 프로브되어 각 서비스의 `Probes` 합계 안에서 따로 계수된다.

## 3. CosNaming — the first reading, 2026-08-14 / 첫 판독

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

> **Re-measured 2026-08-14, after `to_url` landed: absent 0.** The client half
> already parsed `corbaname:`; the producing half now exists, was measured
> against **two** producers — omniORB's own client resolved a URL ours built,
> and omniNames was run as a second producer over 14 argument pairs (11
> identical, 3 differing only in hex-digit case, each parser reading the
> other's) — and one behaviour changed *because* of that comparison rather
> than despite it: an empty name returns the bare `corbaname:<addr>` form, as
> omniNames does.

`to_url` is the finding. It is the one `NamingContextExt` operation the project
already has the machinery for — `crate::naming` parses `corbaname:` URLs on the
client side, and `to_url` is the operation that *produces* one — and it is the
only operation in CosNaming refused by the servant and mentioned by no document,
no test and no plan.

주소 지정 객체: `spike-names --hold`가 발행한 루트 `NamingContextExt`.
**선언 17 · 서빙 10 · 이유 있는 거부 6 · 부재 1.** 발견은 `to_url`이다 —
클라이언트 쪽에 `corbaname:` 파서가 이미 있어 기계장치는 갖춰져 있는데,
서번트는 거부하고 어떤 문서·테스트·계획도 그것을 언급하지 않는다.

## 4. CosEvent — the first reading, and the pull half since / 첫 판독과 그 뒤의 pull

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
On 2026-08-14 the pull interfaces were refused *by construction* — no
`ProxyPullSupplier` object could be obtained at all — so their operations were
probed against the push proxies, which was the strongest statement the wire
could make about an operation nothing can address.

**Since 2026-08-18 that sentence is half true, and it went on saying the whole
thing for a week.** The consumer half of pull is served and the sweep obtains
the real proxy: §8 shows `obtain_pull_supplier`, `connect_pull_consumer`,
`pull`, `try_pull` and `disconnect_pull_supplier` dispatched against a genuine
`ProxyPullSupplier`, not against a push proxy standing in for one. The
`ProxyPullConsumer` probe still goes to a push proxy, and there that is still
the honest thing — `obtain_pull_consumer` answers `NO_IMPLEMENT`, so the
interface has no object, and an operation nothing can address is still an
operation the channel does not have.

주소 지정 객체 다섯. **선언 18 · 서빙 9 · 이유 있는 거부 9 · 부재 0.**
다섯 중 가장 깨끗하다: 서빙하지 않는 모든 연산에 서번트가 쓴 이유가 있다.
2026-08-14 당시 pull 인터페이스는 객체 자체를 얻을 수 없었으므로, push 프록시에
프로브를 던져 "어떤 객체도 이 연산을 답하지 않는다"는 진술을 측정으로 만들었다.

**2026-08-18부터 그 문장은 절반만 참이며, 이레 동안 전부인 양 적혀 있었다.**
pull의 소비자 쪽은 서빙되고 스윕은 진짜 프록시를 얻는다 — §8에서
`obtain_pull_supplier`, `connect_pull_consumer`, `pull`, `try_pull`,
`disconnect_pull_supplier`가 실제 `ProxyPullSupplier`에 디스패치된다.
`ProxyPullConsumer` 프로브는 여전히 push 프록시로 가며, 거기서는 그것이 정직하다 —
`obtain_pull_consumer`가 `NO_IMPLEMENT`이므로 그 인터페이스에는 객체가 없고,
주소 지정할 수 없는 연산은 채널에 없는 연산이다.

## 5. Interface Repository — the first reading, and 6 refusals nobody had written down / 첫 판독

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

> **Re-measured 2026-08-14: `BAD_OPERATION` 0.** `_get_version` is served and
> the ten deferrals answer `NO_IMPLEMENT`, so a considered deferral no longer
> looks like an oversight on the wire — which is the distinction this report
> opens by saying the wire cannot make on its own.

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
> runs in `run_checks.sh`: IFR reports **probes 66 · dispatched 14 ·
> `NO_PERMISSION` 38 · `BAD_OPERATION` 0** — 14 dispatched, 38 refused and 14
> deferred is the 66. This line said *dispatched 28* until 2026-08-25, counting
> `NO_IMPLEMENT` as dispatch; the History section below caught it — *"the IFR's
> served count was overstated by 14 … a facade that answers 14 operations read
> as 28"* — and this quotation of the number stayed wrong while the correction
> sat in the same file. The six absences this section found
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
> **2026-08-14 조치, 그 뒤 같은 스윕으로 재측정됨** — 이제 `run_checks.sh`에서
> 돌며 IFR은 **프로브 66 · 디스패치 14 · `NO_PERMISSION` 38 · `BAD_OPERATION` 0**을
> 보고한다. 이 머리 문장은 2026-08-25까지 "이 스윕으로 재측정되지 않음"이라고
> 적혀 있었다 — 스무 줄 위 영문 쌍둥이가 "그 뒤 같은 스윕으로 재측정됨"이라고
> 말하는 동안, 한 사실이 한 문서 안에서 두 언어로 정반대를 말한 것이다.
> 아래는 원래의 조치 기록이다. `_get_version`은 서빙되고,
> 유예 연산 10개는 `NO_IMPLEMENT`로 답한다 — §2가 "와이어는 구분하지 못한다"고
> 적은 그 구분을 이 서비스에서는 와이어가 한다. 위 표는 스윕이 측정한 상태이며,
> 새 답은 단위 테스트와 `spike-ifr`가 검증한다. `service_sweep.sh` 재실행이
> 이것을 다시 *측정*으로 만들 것이나, 이 배치에서는 omniORB 픽스처가 없어
> 실행하지 못했다.

## 6. MoE control plane (`corpus/golden/22`) — the first reading / 첫 판독

Objects addressed: `moe::ExpertRegistry` and `moe::ExpertLoader`, held open by
the harness-only holder `spikes/svc-hold` on 2026-08-14 (removed 2026-08-19 — both spikes have `--hold`; see §9). Source: `corpus/golden/22-moe-control-plane.idl`.

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

> **Re-measured 2026-08-14 with a Router object published: `select` dispatches,
> `dispatch` answers `BAD_OPERATION`.** That is the intended state rather than a
> remaining gap — `select` returns references and is served; `dispatch` carries
> an `Activation` and is the open decision D006 recommends excluding. The sweep
> now probes a fourth object (`moe-router.ior`) and says so when none is
> published, instead of quietly probing Router against servants that never
> claimed it.

The two servants serve their two interfaces completely — the module's claim
(*"the two interfaces below are served exactly as declared there, with no
operation added and none half-served"*) is measured true, 7 of 7. What no
document stated **when this was written, on 2026-08-14,** was the other half of
the contract. `moe::Expert` is defensible by design: the registry *stores*
expert references and the experts are served elsewhere, so an `Expert` servant
here would be wrong — *"but that sentence is nowhere written"*. And
`moe::Router` had *"no defence recorded at all: it is declared in the contract,
named in no plan, and answered by nothing."*

**Both complaints are closed: every clause above that asserted an absence is
now false, and only the plain facts — `Expert` is defensible, `Router` is
declared in the contract — still stand.**
The `Expert` sentence is written: `PLAN-SERVICES.md` §8.1.1 quotes this
paragraph's own words back — *"defensible by design … but that sentence is
nowhere written. This is the sentence"* — and the sweep now reports the
interface as *claimed by no object*, its own fact, rather than as five missing
operations. `Router` keeps only *declared in the contract*: its defence is
recorded (`PLAN-MOE.md` §4.6, *"Why `Router` is in no plan — the plane rule and
its escape hatch"*), `dispatch`'s exclusion is D006, **APPROVED 2026-08-14 —
the same day this section reported that no defence existed anywhere**, and it
is answered by something: §8 has `select` served and `dispatch` answering
`NO_IMPLEMENT` since 2026-08-18. A decision approved and a document written do
not reach the wire or the neighbouring section on their own; that gap is what
this paragraph measured without knowing it.

**선언 12 · 서빙 7 · 이유 있는 거부 0 · 부재 5.** 두 서번트는 자기 인터페이스
두 개를 완전히 서빙한다(7/7, 모듈 문서의 주장은 측정으로 참). **2026-08-14 이 절을
쓸 당시** 문서화되지 않은 것은 계약의 나머지 절반이었다. `moe::Expert`는 설계상
변호 가능하지만(레지스트리는 참조를 *저장*할 뿐 익스퍼트는 다른 곳에서 서빙된다)
"그 문장이 어디에도 없고", `moe::Router`는 "변호조차 기록되어 있지 않다"고 적었다.

**둘 다 닫혔다 — 위에서 부재를 주장한 절은 모두 거짓이 되었고, 남은 것은
`Expert`가 변호 가능하다는 것과 `Router`가 계약에 선언되어 있다는 사실뿐이다.**
`Expert`의 그 문장은 `PLAN-SERVICES.md` §8.1.1이 이 문단의 지적을 그대로 인용하며
쓴다("그 문장이 어디에도 없다 — 이 문단이 그 문장이다"). 스윕도 그것을 "연산 다섯
개 누락"이 아니라 "어떤 객체도 자처하지 않는 인터페이스"라는 자기 사실로 보고한다.
`Router`의 이유는 `PLAN-MOE.md` §4.6("`Router`가 어느 계획서에도 없던 이유")에
기록되어 있고, `dispatch`의 제외는 **D006 — 이 절이 "변호가 없다"고 적은 바로 그
날인 2026-08-14에 승인** — 이며, §8에서 `select`는 서빙되고 `dispatch`는
2026-08-18부터 `NO_IMPLEMENT`로 답한다. 승인된 결정과 쓰인 문서가 저절로 와이어나
옆 절에 닿지는 않는다는 것 — 이 문단이 모르고 측정한 것이 그것이다.

## 7. MoE enterprise (`corpus/golden/23`) — the first reading / 첫 판독

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

**When this was written on 2026-08-14, the only service of the five whose ✅
needed no qualification** — and it stopped being the only one on 2026-08-18,
when CosNaming joined it: §8 reads 14 declared, 14 served. "Only" was a dated
word left in the present tense for a week. The module's claim
— *"The contract is corpus/golden/23 and nothing else: every operation it
declares is served, including the two `EnterpriseExpert` inherits from
`::moe::Expert`, and nothing it does not declare exists on the wire"* — is
measured true in both directions: 16 of 16 served, and every operation from a
*neighbouring* interface answered `BAD_OPERATION` on every object, which is what
makes these five distinct objects rather than one with a union of operations.

**선언 16 · 서빙 16 · 이유 있는 거부 0 · 부재 0.** **2026-08-14 당시** 다섯 중
유일하게 단서가 필요 없는 ✅였고, 2026-08-18 CosNaming이 합류하면서 "유일하게"는
끝났다(§8: 선언 14 · 서빙 14). 날짜에 묶인 말이 현재형으로 이레를 더 남아 있었다.
모듈 문서의 주장이 양방향으로 참임이 측정되었다 — 16/16 서빙,
그리고 *이웃* 인터페이스의 연산은 모든 객체에서 `BAD_OPERATION`.

## 8. Measured now — generated by the sweep / 지금의 측정 — 스윕이 생성

The block between the markers is written by
`./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py --write`
and checked by the harness with `--check`: when the wire and this file
disagree, the harness fails with the diff, and the fix is to regenerate —
never to edit the block. Every number in this document that a script can
compute lives here and nowhere else; the counts that used to sit in the §3–§7
headings were restated facts and went stale within four days of being typed
(D010 A5). *Declared* is per `interface::operation`; *served* is "at least one
object dispatched it"; the last list is what the sweep **did not measure**,
which used to be silent.

마커 사이 블록은 스윕이 쓰고 하네스가 `--check`로 대조한다 — 어긋나면 diff와
함께 실패하고, 고치는 법은 재생성뿐이다. 스크립트가 셀 수 있는 숫자는 모두
여기에만 산다. 마지막 목록은 스윕이 **재지 않은 것**이며, 전에는 침묵했다.

<!-- BEGIN generated by spikes/coverage_tables.py from service_sweep.sh --raw; edit the sweep, not this block -->

### CosNaming — 14 declared, 14 served / 선언 14, 서빙 14

| Interface | Operation | Answer, per object probed | Class |
|---|---|---|---|
| `CosNaming::NamingContextExt` | `to_string` | NamingContextExt (root) → `user InvalidName` | served |
| `CosNaming::NamingContextExt` | `to_name` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContextExt` | `to_url` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContextExt` | `resolve_str` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContext` | `bind` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContext` | `rebind` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContext` | `bind_context` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContext` | `rebind_context` | NamingContextExt (root) → `MARSHAL` | served |
| `CosNaming::NamingContext` | `resolve` | NamingContextExt (root) → `user InvalidName` | served |
| `CosNaming::NamingContext` | `unbind` | NamingContextExt (root) → `user InvalidName` | served |
| `CosNaming::NamingContext` | `new_context` | NamingContextExt (root) → `reply` | served |
| `CosNaming::NamingContext` | `bind_new_context` | NamingContextExt (root) → `user InvalidName` | served |
| `CosNaming::NamingContext` | `destroy` | NamingContextExt (root) → `user NotEmpty` | served |
| `CosNaming::NamingContext` | `list` | NamingContextExt (root) → `reply` | served |

_`CORBA::Object` pseudo-operations probed and counted apart: `_is_a`, `_non_existent`._

### CosEvent — 18 declared, 14 served / 선언 18, 서빙 14

| Interface | Operation | Answer, per object probed | Class |
|---|---|---|---|
| `CosEventChannelAdmin::EventChannel` | `for_consumers` | EventChannel → `reply` | served |
| `CosEventChannelAdmin::EventChannel` | `for_suppliers` | EventChannel → `reply` | served |
| `CosEventChannelAdmin::EventChannel` | `destroy` | EventChannel → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CosEventChannelAdmin::ConsumerAdmin` | `obtain_push_supplier` | ConsumerAdmin → `reply` | served |
| `CosEventChannelAdmin::ConsumerAdmin` | `obtain_pull_supplier` | ConsumerAdmin → `reply` | served |
| `CosEventChannelAdmin::SupplierAdmin` | `obtain_push_consumer` | SupplierAdmin → `reply` | served |
| `CosEventChannelAdmin::SupplierAdmin` | `obtain_pull_consumer` | SupplierAdmin → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CosEventChannelAdmin::ProxyPushSupplier` | `connect_push_consumer` | ProxyPushSupplier → `MARSHAL` | served |
| `CosEventComm::PushSupplier` | `disconnect_push_supplier` | ProxyPushSupplier → `reply` | served |
| `CosEventChannelAdmin::ProxyPushConsumer` | `connect_push_supplier` | ProxyPushConsumer → `MARSHAL` | served |
| `CosEventComm::PushConsumer` | `push` | ProxyPushConsumer → `reply` | served |
| `CosEventComm::PushConsumer` | `disconnect_push_consumer` | ProxyPushConsumer → `reply` | served |
| `CosEventChannelAdmin::ProxyPullSupplier` | `connect_pull_consumer` | ProxyPullSupplier → `MARSHAL` | served |
| `CosEventComm::PullSupplier` | `pull` | ProxyPullSupplier → `user Disconnected` | served |
| `CosEventComm::PullSupplier` | `try_pull` | ProxyPullSupplier → `user Disconnected` | served |
| `CosEventComm::PullSupplier` | `disconnect_pull_supplier` | ProxyPullSupplier → `reply` | served |
| `CosEventChannelAdmin::ProxyPullConsumer` | `connect_pull_supplier` | ProxyPushConsumer → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CosEventComm::PullConsumer` | `disconnect_pull_consumer` | ProxyPushConsumer → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |

_`CORBA::Object` pseudo-operations probed and counted apart: `_is_a`, `_non_existent`._

### IFR — 44 declared, 9 served / 선언 44, 서빙 9

| Interface | Operation | Answer, per object probed | Class |
|---|---|---|---|
| `CORBA::Repository` | `lookup_id` | Repository (root) → `MARSHAL` | served |
| `CORBA::Repository` | `get_canonical_typecode` | Repository (root) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Repository` | `get_primitive` | Repository (root) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Repository` | `create_string` | Repository (root) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Repository` | `create_wstring` | Repository (root) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Repository` | `create_sequence` | Repository (root) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Repository` | `create_array` | Repository (root) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Repository` | `create_fixed` | Repository (root) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `lookup` | Repository (root) → `NO_IMPLEMENT`; InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Container` | `contents` | Repository (root) → `NO_IMPLEMENT`; InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Container` | `lookup_name` | Repository (root) → `NO_IMPLEMENT`; InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Container` | `describe_contents` | Repository (root) → `NO_IMPLEMENT`; InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Container` | `create_module` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_constant` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_struct` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_union` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_enum` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_alias` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_interface` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_value` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_value_box` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_exception` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_native` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Container` | `create_abstract_interface` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::IRObject` | `_get_def_kind` | Repository (root) → `reply`; InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::IRObject` | `destroy` | Repository (root) → `NO_PERMISSION`; InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::InterfaceDef` | `_get_base_interfaces` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::InterfaceDef` | `_set_base_interfaces` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::InterfaceDef` | `is_a` | InterfaceDef (gc10::Both) → `MARSHAL` | served |
| `CORBA::InterfaceDef` | `describe_interface` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::InterfaceDef` | `create_attribute` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::InterfaceDef` | `create_operation` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Contained` | `_get_id` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::Contained` | `_set_id` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Contained` | `_get_name` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::Contained` | `_set_name` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Contained` | `_get_version` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::Contained` | `_set_version` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::Contained` | `_get_defined_in` | InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Contained` | `_get_absolute_name` | InterfaceDef (gc10::Both) → `reply` | served |
| `CORBA::Contained` | `_get_containing_repository` | InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Contained` | `describe` | InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `CORBA::Contained` | `move` | InterfaceDef (gc10::Both) → `NO_PERMISSION` | refused (`NO_PERMISSION`) |
| `CORBA::IDLType` | `_get_type` | InterfaceDef (gc10::Both) → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |

_`CORBA::Object` pseudo-operations probed and counted apart: `_is_a`, `_non_existent`._

### MoE enterprise — 16 declared, 16 served / 선언 16, 서빙 16

| Interface | Operation | Answer, per object probed | Class |
|---|---|---|---|
| `moe::enterprise::ModelFactory` | `create` | ModelFactory → `MARSHAL` | served |
| `moe::enterprise::ModelFactory` | `clone_model` | ModelFactory → `MARSHAL` | served |
| `moe::enterprise::ModelFactory` | `retire` | ModelFactory → `MARSHAL` | served |
| `moe::enterprise::ModelFactory` | `deploy` | ModelFactory → `MARSHAL` | served |
| `moe::enterprise::ComposedModel` | `get_manifest` | ComposedModel → `reply` | served |
| `moe::enterprise::ComposedModel` | `infer` | ComposedModel → `MARSHAL` | served |
| `moe::enterprise::ComposedModel` | `bind_expert` | ComposedModel → `MARSHAL` | served |
| `moe::enterprise::ComposedModel` | `set_policy` | ComposedModel → `MARSHAL` | served |
| `moe::enterprise::PolicyDomain` | `authorize` | PolicyDomain → `MARSHAL` | served |
| `moe::enterprise::PolicyDomain` | `check_residency` | PolicyDomain → `MARSHAL` | served |
| `moe::enterprise::PolicyDomain` | `audit` | PolicyDomain → `MARSHAL` | served |
| `moe::enterprise::EnterpriseExpert` | `get_tenant_id` | EnterpriseExpert → `reply` | served |
| `moe::enterprise::EnterpriseExpert` | `base` | EnterpriseExpert → `reply` | served |
| `moe::enterprise::EnterpriseExpert` | `adapter_delta` | EnterpriseExpert → `reply` | served |
| `moe::Expert` | `describe` | EnterpriseExpert → `reply`; shared ::moe::Expert → `reply` | served |
| `moe::Expert` | `process` | EnterpriseExpert → `MARSHAL`; shared ::moe::Expert → `MARSHAL` | served |

_`CORBA::Object` pseudo-operations probed and counted apart: `_is_a`, `_non_existent`._

### MoE control plane — 14 declared, 10 served / 선언 14, 서빙 10

| Interface | Operation | Answer, per object probed | Class |
|---|---|---|---|
| `moe::ExpertRegistry` | `register_expert` | ExpertRegistry → `MARSHAL` | served |
| `moe::ExpertRegistry` | `deregister` | ExpertRegistry → `MARSHAL` | served |
| `moe::ExpertRegistry` | `heartbeat` | ExpertRegistry → `MARSHAL` | served |
| `moe::ExpertRegistry` | `register_measured` | ExpertRegistry → `MARSHAL` | served |
| `moe::ExpertRegistry` | `heartbeat_measured` | ExpertRegistry → `MARSHAL` | served |
| `moe::ExpertLoader` | `prefetch` | ExpertLoader → `MARSHAL` | served |
| `moe::ExpertLoader` | `evict` | ExpertLoader → `MARSHAL` | served |
| `moe::ExpertLoader` | `pin` | ExpertLoader → `MARSHAL` | served |
| `moe::ExpertLoader` | `status` | ExpertLoader → `MARSHAL` | served |
| `moe::Router` | `select` | Router → `MARSHAL` | served |
| `moe::Router` | `dispatch` | Router → `NO_IMPLEMENT` | deferred (`NO_IMPLEMENT`) |
| `moe::Expert` | `describe` | ExpertRegistry → `BAD_OPERATION`; ExpertLoader → `BAD_OPERATION` | not dispatched (`BAD_OPERATION`) |
| `moe::Expert` | `process` | ExpertRegistry → `BAD_OPERATION`; ExpertLoader → `BAD_OPERATION` | not dispatched (`BAD_OPERATION`) |
| `moe::Expert` | `delegate` | ExpertRegistry → `BAD_OPERATION`; ExpertLoader → `BAD_OPERATION` | not dispatched (`BAD_OPERATION`) |

_`CORBA::Object` pseudo-operations probed and counted apart: `_is_a`, `_non_existent`._

### Totals / 합계

| Service | Declared | Served | Deferred `NO_IMPLEMENT` | Refused `NO_PERMISSION` | Not dispatched `BAD_OPERATION` | Probes | Unmeasured |
|---|---:|---:|---:|---:|---:|---:|---:|
| CosNaming | 14 | 14 | 0 | 0 | 0 | 16 | 0 |
| CosEvent | 18 | 14 | 4 | 0 | 0 | 28 | 0 |
| IFR | 44 | 9 | 10 | 25 | 0 | 66 | 0 |
| MoE enterprise | 16 | 16 | 0 | 0 | 0 | 28 | 0 |
| MoE control plane | 14 | 10 | 1 | 0 | 3 | 21 | 0 |
| **total** | **106** | **63** | **15** | **25** | **3** | **159** | **0** |

_Declared_ counts each `interface::operation` once however many objects it was probed on; an operation is _served_ if any object dispatched it. `MARSHAL` on a 64-zero-byte probe proves dispatch, not correctness (§9). / _선언_은 객체 수와 무관하게 `인터페이스::연산`을 한 번씩 센다.

### Interfaces no object claimed / 어떤 객체도 주장하지 않는 인터페이스

| Service | Interface | Reported |
|---|---|---|
| MoE control plane | `moe::Expert` | declared, claimed by no object probed |

### Declared, probed against no object — unmeasured / 선언되었으나 프로브한 객체 없음 — 미측정

- **CosNaming** — 1 interface(s): `CosNaming::BindingIterator` (3 declared operation(s))
- **IFR** — 21 interface(s): `CORBA::ModuleDef` (0 declared operation(s)), `CORBA::ConstantDef` (5 declared operation(s)), `CORBA::TypedefDef` (0 declared operation(s)), `CORBA::StructDef` (2 declared operation(s)), `CORBA::UnionDef` (5 declared operation(s)), `CORBA::EnumDef` (2 declared operation(s)), `CORBA::AliasDef` (2 declared operation(s)), `CORBA::NativeDef` (0 declared operation(s)), `CORBA::PrimitiveDef` (1 declared operation(s)), `CORBA::StringDef` (2 declared operation(s)), `CORBA::WstringDef` (2 declared operation(s)), `CORBA::FixedDef` (4 declared operation(s)), `CORBA::SequenceDef` (5 declared operation(s)), `CORBA::ArrayDef` (5 declared operation(s)), `CORBA::ExceptionDef` (3 declared operation(s)), `CORBA::AttributeDef` (5 declared operation(s)), `CORBA::OperationDef` (11 declared operation(s)), `CORBA::ValueMemberDef` (5 declared operation(s)), `CORBA::ValueDef` (19 declared operation(s)), `CORBA::ValueBoxDef` (2 declared operation(s)), `CORBA::AbstractInterfaceDef` (0 declared operation(s))

<!-- END generated -->

### History: the first totals, and the disagreements / 이력: 첫 합계와 불일치

> Dated. What follows was true when written and is kept as the record of how
> the numbers moved; today's numbers are the generated block above.
> 날짜 붙은 기록. 오늘의 숫자는 위 생성 블록이다.


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

#### CosNaming, same day / 같은 날 CosNaming

`bind_context`, `rebind_context` and `destroy` are served. Two of the three
deferral reasons turned out to be **descriptions of the servant rather than
obstacles**: contexts lived as long as the process *because* nothing removed a
key, and `bind_context` of a context this dispatch already serves is a map
insert, not a call over the wire. It was also the only way the already-served
`new_context` produced anything reachable.

Chaining — binding a **foreign** context — stays deferred, and its reason is
rewritten rather than repeated. It is implementable now (`guarded` makes the
lock question answerable), and that is a reason it is *possible*, not a reason
to do it. Measured what it would buy: omniNames chains both `resolve` and
`bind`, and against an undialable context turns a naming `resolve` into
`TRANSIENT` after a TCP connect.

The peer drove it: **20 labelled rows compared as a whole**, all matching, with
every expected value measured against omniNames 4.3.4 first. Two deliberate
divergences from omniNames, both peer-visible: it type-checks neither rebind,
and it accepts any reference for `bind_context`. JacORB is **unmeasured, not
passing** — no fixture on this machine.

The structural property the naming module rests on is now checked rather than
asserted: the servant contains no `Connection`, `Pool`, `Mux`, `invoke`,
`TcpStream` or `connect(`, and all 16 operations run under
`guarded::complaints_about` with nothing held. That is what a federated
`bind_context` would spend.

**One negative control changed a test.** The lock sweep *passed* with a
violation planted in `Tree::destroy`, because `destroy` at a populated root
stops at `NotEmpty` and never reaches the removal. Rows are dispatched where
they succeed now, and each must return a value.

`bind_context`·`rebind_context`·`destroy`가 서빙된다. 세 사유 중 둘은 **서번트에
대한 서술이지 장애물이 아니었다** — 컨텍스트가 프로세스만큼 산 것은 *아무것도 키를
지우지 않았기 때문*이다. 연쇄는 유예를 유지하되 사유를 다시 썼다: 이제 가능하다는
것은 *가능하다*는 이유이지 *해야 한다*는 이유가 아니다. **음성 대조군 하나가
테스트를 바꿨다** — 심어 둔 위반을 통과시켰는데, `destroy`가 `NotEmpty`에서 멈춰
제거에 닿지 않았기 때문이다.

#### Re-measured again, later on 2026-08-18 / 재재측정

The pull half of CosEvent moved, and the sweep had to move with it.

| Service | probes | served | `NO_PERMISSION` | `NO_IMPLEMENT` | `BAD_OPERATION` |
|---|---:|---:|---:|---:|---:|
| CosEvent | 28 | **24** | 0 | **4** | 0 |

The consumer side of pull is served: `obtain_pull_supplier`,
`connect_pull_consumer`, `pull`, `try_pull`, `disconnect_pull_supplier`. The
deferral's reason was *"the same unbounded buffer this module spends its
bounded queue avoiding, for no named consumer"*, and **only the second clause
survived measurement** — a `ProxyPullSupplier` holds events in the same bounded
deque, moved by the same knob, dropped oldest-first into the same counter.

The supplier side stays deferred with a **rewritten** reason: there the channel
is the puller, `PullSupplier::pull` is specified to block until the supplier
has something, and the channel would hold a thread per connected supplier on
somebody else's clock — for no named supplier, since nothing here is one.
`destroy` also stays, and its reason sharpened: the outbound half is answered
by `guarded`, but it remains an **unauthenticated remote operation that ends
the channel for every other client**, and this servant does not know who is
calling.

**The sweep was measuring the wrong object.** It probed the pull operations
against a *push* proxy, because no pull proxy could be obtained when that code
was written. The moment they started being served, that reported the whole
`ProxyPullSupplier` interface as **unserved** — a false absence produced by
asking the wrong reference. It obtains the real proxy now. The
`ProxyPullConsumer` probe still goes to a push proxy, and there that is the
honest thing: `obtain_pull_consumer` is still refused, so the interface has no
object, and an operation nothing can address is still an operation the channel
does not have.

**No peer verified any of this.** `brew info omnievents` still reports *"No
available formula"*, and omniORBpy ships no `ProxyPullSupplier` stubs, so there
is not even a half-peer. The oracle is CORBA 3.4 plus hand-built GIOP clients,
with the limit that arrangement always has: it proves we do what we read, not
what another ORB does.

CosEvent의 pull 소비자 쪽이 서빙된다. 유예 사유 두 절 중 **하나만 측정을
견뎠다.** 스윕은 **잘못된 객체를 재고 있었고**, 그래서 서빙되기 시작한 순간
인터페이스 전체를 미서빙으로 보고했다 — 잘못된 참조에 물어 만든 거짓 부재다.
**어떤 피어도 이것을 검증하지 않았다.**

#### Re-measured 2026-08-18 / 재측정

The table above is the 2026-08-14 measurement and stays as it was taken. The
same sweep, re-run:

| Service | probes | served | `NO_PERMISSION` | `NO_IMPLEMENT` | `BAD_OPERATION` |
|---|---:|---:|---:|---:|---:|
| CosNaming | 16 | 13 | 0 | 3 | **0** |
| CosEvent | 28 | 19 | 0 | 9 | **0** |
| Interface Repository | 66 | 14 | 38 | 14 | **0** |
| MoE enterprise | 28 | 28 | 0 | 0 | **0** |
| MoE control plane | 19 | 12 | 0 | 1 | **6** |

**Absences: zero.** Not because the operations were implemented — most were
not — but because the wire now says which fact it means, per
[`PLAN-SERVICES.md`](PLAN-SERVICES.md) §8.1.1: `NO_IMPLEMENT` for a declared
operation this servant does not implement on purpose, `BAD_OPERATION` only for
a name the interface does not declare. The sweep decides this instead of a
reader cross-referencing a document, and **fails** on a `BAD_OPERATION` from an
object that claims the interface.

Three things that changes in the numbers above:

1. **The IFR's served count was overstated by 14.** `NO_IMPLEMENT` was being
   counted as *dispatched*, so a facade that answers 14 operations read as 28.
   The count was wrong in the direction that flatters, which is the direction
   worth checking.
2. **The six remaining `BAD_OPERATION`s are correct** and are not absences:
   they are `moe::Expert` operations probed against a registry and a loader
   that never claimed to be Experts. That is reported now as its own fact — an
   interface claimed by no object — with the reason written in §8.1.1 rather
   than left as five missing operations.
3. **`moe::Router::dispatch` was a real absence and had been one since D006.**
   The decision to exclude it was approved 2026-08-14 and recorded in prose
   while the servant went on answering "no such operation". The gate found it
   on its first green run; nothing else could have.

위 표는 2026-08-14 측정이며 그대로 둔다. 재실행 결과 **부재 0건** — 연산이
구현되어서가 아니라 와이어가 어느 사실인지 말하게 되었기 때문이다. 셋이 달라졌다:
① IFR의 서빙 수치는 **14만큼 부풀려져 있었다**(`NO_IMPLEMENT`를 서빙으로 계수),
② 남은 `BAD_OPERATION` 6건은 옳은 답이며 부재가 아니다(자처한 적 없는 인터페이스),
③ **`moe::Router::dispatch`는 진짜 부재였고 D006 이후 줄곧 그랬다** — 제외 결정은
산문에 있었고 서번트는 계속 "그런 연산 없음"이라 답했다. 게이트가 첫 초록 실행에서
찾았고, 다른 무엇도 찾을 수 없었다.

#### Where the measurement disagrees with the documents / 문서와의 불일치

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

- **`spike-experts` and `spike-tenants` had no `--hold`** on 2026-08-14, so this
  sweep worked around it with a harness-only holder (`spikes/svc-hold`). Both
  binaries have `--hold` now and `spikes/service_sweep.sh` uses it; the holder
  crate was orphaned and referenced by no script, and was removed on 2026-08-19
  (plan review).

- **`MARSHAL` proves dispatch, not correctness.** On 2026-08-14, twelve of the fifty served
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
./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py           # §8's block
./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py --check   # the harness's diff
./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py --write   # regenerate §8
```

Needs `brew install omniorb` for the IDL only — the sweep runs `omniidl` as an
external program and imports nothing. If the IDL directory is missing the sweep
reports `BLOCKED` and exits non-zero, because an unmeasured check is a failure
and never a pass.

IDL만을 위해 omniORB가 필요하며, `omniidl`은 외부 프로그램으로 실행할 뿐
아무것도 import 하지 않는다. IDL 디렉터리가 없으면 `BLOCKED`을 보고하고
0이 아닌 코드로 끝난다 — 측정되지 않은 검사는 통과가 아니라 실패이므로.
