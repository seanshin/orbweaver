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
declaration. That is precisely why it was invisible. `moe-estate.json` declares
the union, and `orbweaver-test`'s seed gate checks the invariant no fixture
checks today — that every offer is placed on a declared node.

*다섯 중 둘은 나눌 모집단이 아예 없다. 겹치는 것은 `spike_tenants`와
`spike_experts` 둘뿐이며, **세 가지로 어긋난다**: `vision`은 한쪽에서 일부러
없는 능력이고 다른 쪽에서는 등록·고정된 상주 전문가다; 노드 이름공간이 서로소이며
한쪽은 미선언 노드를 기본 거부한다; `MoE` 키 베이스가 다르고 한쪽은 자기 레지스트리
키와 충돌한다. 배치 노드를 쓰는 세 픽스처가 **서로소인 세 이름공간**을 쓴다 —
별개 프로세스라 아무것도 빨갛지 않았고, 그래서 보이지 않았다.*

---

## What is **not** claimed here

The five fixtures were **not migrated onto this seed**, so the byte-identity
oracle D026 §5 S1 names was **not run**. All five live in `orbweaver-object`,
`orbweaver-registry` and `orbweaver-giop`, and the batch that produced this
directory was scoped to `corpus/state/`, `orbweaver-test` and new files under
`spikes/`. The disagreements above were established **by reading**, which
establishes that the populations differ and does *not* establish that migrating
them preserves any fixture's output. That measurement is still owed.

**A seeded population must not become the only population** (D026 §3). Nothing
here retires an ad-hoc case; the property tests and `wire-fuzz` exist because a
fixed population is a fixed set of paths.

*다섯 픽스처는 이 시드로 **이전되지 않았고**, 따라서 바이트 동일성 오라클은
**돌지 않았다** — 셋 다 이 배치의 범위 밖 크레이트에 있다. 위의 불일치는 **읽어서**
확인한 것이며, 이전이 출력을 보존하는지는 아직 측정되지 않았다.*
