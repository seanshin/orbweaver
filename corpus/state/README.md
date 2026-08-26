# `corpus/state/` — runtime populations, as data

D026 §5 S1. `corpus/` elsewhere holds **contracts**; this directory holds the
**populations a contract is exercised over** — offers a trader ranks, names a
naming graph binds, tenants, nodes, policy domains. Nothing here is IDL and
nothing here is compiled.

*`corpus/`의 다른 곳은 **계약**을 담고, 이 디렉터리는 **그 계약이 시험되는
모집단**을 담는다 — 트레이더가 순위를 매기는 오퍼, 네이밍 그래프가 묶는 이름,
테넌트, 노드, 정책 도메인. 여기에 IDL은 없고, 컴파일되는 것도 없다.*

---

## The format, and why it is not AnyJSON

**Plain JSON documents, whose scalar spellings are AnyJSON v1's.** Not AnyJSON
documents — AnyJSON *scalars*.

Two readers, sharing no code:

| | reader | cost |
|---|---|---|
| Rust | `orbweaver_dynamic::json::Json::parse` | already a dependency of `orbweaver-test`; the workspace has no serde and gains none |
| Python | stdlib `json` | none |

AnyJSON was the candidate to beat, and it loses here for four reasons — none of
them about JSON being easier.

1. **AnyJSON is a mapping of a value that already has a `TypeCode`.** Its
   signature is `to_json(tc: &TypeCode, v: &Value, …)`. A seed exists *before*
   any `TypeCode` — it is the input from which a fixture builds typed values.
   Encoding one would mean minting a `TypeCode` per population shape, which
   means writing IDL for the seeds, which is exactly the "second corpus of
   contracts" D026 §3 forbids.
2. **A seed encoded in AnyJSON would be measured by the thing it measures.**
   `prop.rs` already asserts AnyJSON round-trips. If the population were
   AnyJSON, a mapping defect would corrupt the population *and* the expectation
   together, in the same direction, and the round trip would stay green over
   it. A seed has to be read by a reader with no stake in the outcome.
3. **The Python half would become a second implementation of our own normative
   mapping.** AnyJSON v1 is a specification (`docs/PLAN.md` §4.5), not a
   convention. A Python AnyJSON reader shares the *mapping* even where it shares
   no *code*, and S1b's entire argument is that the two readers must be
   independent. Python's `json` has no stake in anything of ours.
4. **`Json::Object` is a `BTreeMap`** and Python's is a `dict`. Neither reader
   can depend on member order, so the format cannot grow an ordering convention
   that only one of them honours.

What AnyJSON *does* win is scalar spelling, so we take it rather than inventing
a second convention that could drift from it:

- **64-bit integers are JSON strings** (`"mem_footprint": "1048576"`). A JSON
  number is a `double` everywhere that matters and loses digits past 2^53.
- **Enumerators cross by name** (`"residency": "RESIDENT"`), never by ordinal.
- **Absent is `null`, and `null` is not a zero.** `"latency_p50": null` means
  nobody measured it — the distinction `orbweaver-trading` was changed to
  preserve, after an unmeasured latency of `0.0` matched every upper bound and
  a router preferred exactly the experts nobody had timed.

A value read from here that later becomes a CORBA `Any` therefore needs no
translation, and the two conventions cannot drift apart.

*AnyJSON은 `TypeCode`를 이미 가진 값의 매핑이다. 시드는 `TypeCode`가 생기기
**전에** 존재하므로 계층이 맞지 않고, 시드를 AnyJSON으로 넣으면 **재려는 대상이
시드를 재게 된다**. 파이썬 쪽 리더는 우리 규범 매핑의 두 번째 구현이 되어, 두
리더가 독립이어야 한다는 S1b의 논지를 무너뜨린다. 다만 **스칼라 표기는**
AnyJSON의 것을 그대로 쓴다 — 64비트 정수는 문자열, 열거자는 이름, 부재는
`null`이며 `null`은 0이 아니다.*

---

## The rule this directory exists to serve

D026 §4: **a fixture states where its population came from, and a population
that more than one fixture uses has one home.** The corollary is load-bearing:
a fixture may still invent a population, and says so. What is forbidden is the
*silent second copy*.

*픽스처는 자기 모집단의 출처를 밝히고, 둘 이상이 쓰는 모집단은 집이 하나다.
발명은 여전히 허용되며 — 다만 그렇다고 말한다. 금지되는 것은 **조용한 두 번째
사본**이다.*

---

## Measured 2026-08-26: what the five fixtures' populations did

The first job of this directory was a question, not coverage: for the five
fixtures D026 §1 counts, **do their populations already disagree?** They were
read in full and compared. The answer has three parts.

**Two of the five have no population to share.** `spike_ifr` does not invent
one at all — it loads `corpus/golden/10-inheritance.idl` and
`19-realistic-service.idl` into a `Registry`, so its population already has a
home and its "4 seeding calls" are file loads. `spike_events`' population is
`ulong` values (`0..19`, `1000..1005`, `2000..2004`) and object keys, with no
named entity in it. `spike_names`' naming graph is real and unique to it.

**Only `spike_tenants` and `spike_experts` overlap, and they disagree — three
ways, none of which anything was checking.**

| | `spike_tenants` | `spike_experts` |
|---|---|---|
| `vision` | a capability that deliberately **does not exist** — `authorize("svc-acme","vision")` is asserted `false` | `expert-vision` is registered, pinned, and `Resident` |
| placement nodes | declares `gpu-eu-1`, `gpu-us-1`; refuses undeclared nodes **default-deny** | every expert is placed on `gpu-04` |
| the `MoE` key base | root key `MoE`, a strict prefix of every derived key | root key `MoE/registry`, **colliding with its own derived registry key**, while the service derives from base `MoE` |

Smaller, same cause: the capability `code` costs `2.0` in one and `1.5` in the
other, and `IDL:moe/Expert:1.0` is a retyped string literal in `spike_tenants`
where `spike_experts` uses the `EXPERT_ID` constant — agreeing today by luck,
which is the data-shaped form of CLAUDE.md's *"a sentence many layers say"*.

**Three fixtures model MoE placement over three disjoint node namespaces** —
`gpu-eu-1`/`gpu-us-1` (tenants), `gpu-04` (experts), `node-a` (the trading
fixture). Nothing is red in the product: they are separate processes with
separate services, so no node is ever checked against another fixture's
declaration. That is precisely why it was invisible.

*다섯 중 둘은 나눌 모집단이 아예 없다. 겹치는 것은 `spike_tenants`와
`spike_experts` 둘뿐이며, **세 가지로 어긋난다**: `vision`은 한쪽에서 일부러
없는 능력이고 다른 쪽에서는 등록·고정된 상주 전문가다; 노드 이름공간이 서로소이며
한쪽은 미선언 노드를 기본 거부한다; `MoE` 키 베이스가 다르고 한쪽은 자기 레지스트리
키와 충돌한다. 배치 노드를 쓰는 세 픽스처가 **서로소인 세 이름공간**을 쓴다 —
별개 프로세스라 아무것도 빨갛지 않았고, 그래서 보이지 않았다.*

---

## Decided 2026-08-27: the two node domains

D028 §4 M3 asks whether `gpu-eu-1` and `gpu-04` are one domain spelled twice or
**two domains**, and says plainly that the honest outcome may be two. It is
two. The argument is mechanical, not a matter of taste, and it is about
**authority and closure** rather than about the names.

| | **A — the declared estate** | **B — reported placement** |
|---|---|---|
| where it is written | `TenantService::declare_node`, out of band | `moe::Capability.placement_node`, a member of `corpus/golden/22` |
| who says it | the operator | the expert, about itself |
| closed? | **yes** — `check_residency` refuses a name it does not list | **no** — any string an expert sends is legal |
| what reads it | `PolicyDomain::check_residency`, against the manifest's region | the offer store, and the trader as an opaque value |
| seeded members | `gpu-eu-1`, `gpu-us-1`; `gpu-nowhere` deliberately outside | `node-a`, `gpu-04` |

**Why they cannot be merged in either direction.** Domain A must be decidable
or its default-deny is not a refusal — `TenantState.nodes`' own comment says a
region is *"a deployment fact the contract has no member for, so it arrives out
of band rather than being guessed from a node's name."* Domain B cannot be
closed, because the thing being admitted is the thing that writes the name:
making `heartbeat` check `placement_node` against the estate would turn a
report into **admission control**, which is a change to what the contract
*does* and not to what a seed *says*. And nothing in the product compares one
to the other today, which is why nothing was red.

So **the disagreement was never a defect**, and closing it costs no line of
fixture code — the cheaper answer D028 predicted. What it costs is this
section, because *no document said they were two*. Within domain B, `node-a`
and `gpu-04` are two **deployments**, not two spellings: nothing claims they
are the same machines and nothing needs to.

**What changed, then.** The first version of `moe-estate.json` listed the union
of both under one `nodes` key, and `every_offer_is_placed_on_a_declared_node`
gated on it. That gate passed — and what it was really asserting was the
*one-domain answer*, compiled in as if it were bookkeeping, over a list built
to make it pass. It is replaced by three that are true of the model:
`every_offer_reports_a_node_the_seed_states` (a typo gate inside domain B),
`every_declared_node_has_a_region` (domain A), and
`the_population_keeps_the_two_node_domains_distinguishable` — which fails if
every reported node also happens to be declared, because a seed in which the
two coincide cannot tell the two-domain answer from the one-domain answer, and
the next reader re-merges the lists as a tidy-up. That is how the union got
written the first time.

*D028 §4 M3의 물음에 대한 답은 **두 도메인**이다. 근거는 이름이 아니라 **권한과
폐쇄성**이다: A(선언된 에스테이트)는 운영자가 대역 외로 선언하며 **닫혀** 있어야
한다 — 그래야 기본 거부가 거부다. B(보고된 배치)는 전문가가 계약 멤버로 자기
자신에 대해 쓰는 값이라 **닫을 수 없다** — 들여보낼 대상이 곧 이름을 쓰는
주체이므로, 대조를 강제하면 보고가 **입회 통제**로 바뀐다. 이는 시드가 *말하는*
바가 아니라 계약이 *하는* 바를 바꾸는 일이다. 따라서 **불일치는 애초에 결함이
아니었고**, 픽스처 코드는 한 줄도 바뀌지 않는다 — 비용은 이 절뿐이며, 잘못된 것은
값이 아니라 **둘이 다른 도메인이라고 적은 문서가 없었다는 사실**이다. B 안에서
`node-a`와 `gpu-04`는 두 철자가 아니라 **두 배치(deployment)**다.*

*바뀐 것은 시드다. 첫 판은 둘의 합집합을 `nodes` 하나에 적었고 게이트가 그 위에서
초록이었다 — 그 게이트가 실제로 주장하던 것은 **한 도메인이라는 답**이었으며,
통과하도록 만든 목록 위에서 통과하고 있었다. 이제 셋으로 나뉘며, 그중
`the_population_keeps_the_two_node_domains_distinguishable`는 보고된 노드가 모두
선언된 노드이기도 하면 실패한다 — 둘이 일치하는 시드는 두 답을 구별할 수 없고,
다음 독자는 목록을 정리하며 다시 합친다. 처음 합집합이 쓰인 경위가 정확히 그것이다.*

---

## Decided 2026-08-27: the two worlds `vision` lives in

D028 §4 M3's third finding, and *"probably not a defect at all"* is right.

- In `spike_tenants`, `vision` is a capability **no tenant holds and no domain
  grants**. `authorize("svc-acme", "vision")` is asserted `false`, under the
  label *"…and only for what was granted"*. The word has to be in the
  vocabulary and out of every grant, or the refusal refuses nothing.
- In `spike_experts`, `expert-vision` is **registered, pinned and `Resident`**,
  and `evict` on it answers `NO_PERMISSION` rather than `TRANSIENT`.

Those are two different questions about two different objects. *May this
subject use this capability under this policy domain* is a tenancy question;
*is this expert process resident on this host* is a placement question. An
expert can be loaded and running while no tenant is authorized to route to it —
that is not a contradiction, it is what a control plane and an authorization
plane being separate **means**.

**What was wrong is that nothing said so**, and nothing could go red about it.
That is D023's third row again — absent by accident rather than by decision —
one scale down. It is now absent **by decision**: `ungranted_capability` names
the word, and `the_ungranted_capability_is_granted_by_nobody` fails the day a
grant appears, which is the day `spike_tenants`' `ok` line stops being true.
Nothing in either fixture changed.

*`vision`은 결함이 아니다. 한쪽은 **어떤 테넌트도 갖지 않고 어떤 도메인도 부여하지
않는 능력**이고(그래야 거절이 거절이다), 다른 쪽은 **등록·고정된 상주 전문가**다.
서로 다른 두 객체에 대한 서로 다른 두 물음 — *이 주체가 이 정책 도메인에서 이
능력을 쓸 수 있는가*(테넌시)와 *이 전문가 프로세스가 이 호스트에 상주하는가*(배치).
어떤 테넌트도 라우팅 권한이 없는 채로 전문가가 적재되어 도는 것은 모순이 아니라,
제어 평면과 인가 평면이 분리되어 있다는 뜻 그 자체다. **잘못된 것은 그렇다고 말한
문서가 없었다는 것**이며, 이제 `ungranted_capability`가 그 단어를 지목하고 게이트가
부여가 생기는 날 — `spike_tenants`의 `ok` 한 줄이 참이기를 그치는 바로 그 날 —
빨개진다. 두 픽스처의 코드는 하나도 바뀌지 않았다.*

---

## Migrated 2026-08-27, and what the migration could **not** do

The byte-identity oracle D026 §5 S1 names has now been **run**. Two fixtures
were migrated, three were not, and the reason three were not is structural
rather than scheduling — which is the finding this section exists for.

| fixture | migrated | byte-identity |
|---|---|---|
| `spike_tenants` | **yes** — tenants, regions, capabilities, costs, adapter deltas, policy domains, the grant, declared nodes | **identical** |
| `spike_experts` | **partly** — the reported node and the capability vocabulary; its four experts stay invented, and its module docs say so | **one added line**, explained below |
| `spike_names` | no — **blocked**, see below | identical (untouched) |
| `spike_ifr` | not applicable — its population is `corpus/golden/*.idl` and already has a home | identical (untouched) |
| `spike_events` | not applicable — `ulong`s and object keys, no named entity | **not comparable**, see below |

**The one deliberate difference.** `spike_experts` prints one new line naming
the node its experts report. It is there because a seeded value that reaches no
output cannot be shown reaching anything: `placement_node` is reported into the
offer store and printed by nothing, so the first run of that fixture's negative
control — the seed changed under it — **came back green**. A control that
cannot fail is not a control.

### The blocker: the loader is above every fixture that owes it

`orbweaver-test` depends on `orbweaver-giop`, `orbweaver-registry` and
`orbweaver-dynamic`, so it sits **above** every crate the five fixtures live
in. A fixture cannot `use orbweaver_test::state` without a dependency cycle,
and Cargo has no bin-only dependency — a `dev-dependency` reaches tests,
examples and benches, and **not** a `[[bin]]`.

- `spike_tenants`, `spike_experts` (`orbweaver-object`) — reachable, because
  `orbweaver-object → orbweaver-dynamic` is cycle-free. They include
  `crates/orbweaver-test/src/state.rs` by `#[path]`: **one file, two
  compilations, no second copy to drift.** That satisfies D026 §4 and is a
  workaround for the graph, not a design.
- `spike_names`, `spike_events` (`orbweaver-giop`) and `spike_ifr`
  (`orbweaver-registry`) — **not reachable at all.** `orbweaver-dynamic`
  depends on `orbweaver-giop` and `orbweaver-registry`, so the JSON parser the
  loader needs is *above* them too. There is no placement of the loader that
  those crates can see, and only `spike_names` has a population to share
  (`naming-graph.json`, still built inline in Rust).

**The structural fix, for whoever takes this next:** the binding constraint is
that `orbweaver_dynamic::json` sits above `orbweaver-giop`. Move that parser
down to `orbweaver-cdr` — which every crate in the workspace already depends
on — and the loader can live somewhere all five fixtures can name, at which
point `spike_names` becomes migratable and the `#[path]` includes become plain
`use`. Nothing in this batch's footprint could do that.

### `spike_events` cannot be measured by this oracle

Its output is **not a function of its inputs**. Ten runs of the *unchanged*
binary, with nothing in this batch touching it:

- 9/10 — `dropped=3 (on_failure_disconnect=3) fanned_out=32`
- 1/10 — `dropped=2 (on_failure_disconnect=2) fanned_out=31`

A race between the third push failure disconnecting the channel and the last
event being queued. **It is not diagnosed** and is not claimed to be — measured
2026-08-27 on one machine, 1 in 10. The consequence is that the byte-identity
oracle cannot be applied to that fixture, and a later migration would have been
blamed for the race.

Note how nearly this was missed: the oracle's own determinism check was **two
consecutive runs**, and both landed on the 1/10 variant, so it reported *"the
oracle is deterministic"*. Two runs is not evidence.

**A seeded population must not become the only population** (D026 §3). Nothing
here retires an ad-hoc case; the property tests and `wire-fuzz` exist because a
fixed population is a fixed set of paths, and each migrated fixture's module
docs name what it still invents and why.

*바이트 동일성 오라클이 **돌았다**. 둘은 이전되었고(`spike_tenants` 완전,
`spike_experts` 공유분만) 셋은 아니며, 아닌 이유는 일정이 아니라 **구조**다:
로더의 집인 `orbweaver-test`가 다섯 픽스처가 사는 모든 크레이트보다 **위에**
있어서 픽스처가 순환 없이 이름을 부를 수 없고, Cargo에는 바이너리 전용 의존성이
없다(`dev-dependency`는 `[[bin]]`에 닿지 않는다). `orbweaver-object`의 둘은
`orbweaver-dynamic`을 통해 닿으므로 `#[path]`로 **파일 하나를 두 번 컴파일**한다 —
사본이 없으므로 어긋남이 불가능하다. `orbweaver-giop`·`orbweaver-registry`의 셋은
**아예 닿지 못한다**: 필요한 JSON 파서가 그들보다 위에 있다. **구조적 해법**은
`orbweaver_dynamic::json`을 모두가 의존하는 `orbweaver-cdr`로 내리는 것이며, 그때
`spike_names`가 이전 가능해지고 `#[path]`는 평범한 `use`가 된다.*

*`spike_events`는 이 오라클로 잴 수 없다 — 출력이 입력의 함수가 아니다. 손대지
않은 바이너리를 열 번 돌려 두 가지 결과가 나왔다(9/10과 1/10). **진단되지 않았고**
진단되었다고 주장하지 않는다. 하마터면 놓칠 뻔했다: 오라클의 결정성 확인이 **연속
두 번**이었고 둘 다 소수 쪽에 떨어져 "결정적"이라고 보고했다. 두 번은 증거가 아니다.*
