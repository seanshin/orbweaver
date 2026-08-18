# D008 — How AnyJSON says what a value's type is

**STATUS: APPROVED** — drafted 2026-08-18 from measurements taken in
`crates/orbweaver-dynamic/tests/self_description.rs` and from the skip list
`gen-python` prints for `corpus/services/ir-subset.idl`, approved the same day
by the user, with the recommendation adopted as written: **option B**, a
structural TypeCode, additive as **AnyJSON v1.1**, with **option D's symmetry
taken as its first step** so no partial implementation can leave the mapping
writing what it refuses to read. A is refused for depending on shared state
the wire format exists to avoid; C is refused for being exactly as opaque as
§4.5's own table says `_t` must not be. Approved after
[`D007`](D007-python-wire-seam.md), as this document required.

**상태: 승인됨** — 2026-08-18 작성·승인, 권고안 그대로 채택: **B안**(구조적
TypeCode), 추가적 **AnyJSON v1.1**, 그리고 **첫 단계로 D안의 대칭성**. A안은
와이어 형식이 피하려던 공유 상태에 기대므로, C안은 §4.5의 표가 `_t`에 요구하는
바로 그 자기서술을 포기하므로 기각. 이 문서가 요구한 대로 D007 다음에 승인되었다.

---

## The measurement / 실측

Two facts, both executed rather than read:

```
to_json(any, Any(Tagged, {...}))  →  {"_t":"IDL:gc12/Tagged:1.0","_v":{"name":"x"}}
from_json(that same document)     →  unknown type "IDL:gc12/Tagged:1.0";
                                     only primitives may cross in an any
encode(tk_TypeCode, <any Value>)  →  expected a value of type typecode, got a long
```

The first is worse than a limitation. **The mapping writes a document it
refuses to read**, so an agent receives an `any` it cannot send back, and the
failure appears on the return leg rather than at the boundary that produced it.
The encode side names the type by repository id (`type_name`), the decode side
resolves through `named_type`, and `named_type` knows fifteen primitives.

The second has no `Value` variant at all: `tk_TypeCode` (kind 12) encodes and
decodes in `orbweaver-giop`, `orbweaver-gen` carries it as `rt::TypeCodeVal`,
and the dynamic path — the reference implementation that §8's *static equals
dynamic* oracle compares against — cannot represent it. Where the static path
is the only one that works, the oracle is not weaker; it is inapplicable.

Measured consequence, `gen-python` over `corpus/services/ir-subset.idl`:
**18 items generated, 10 skipped**, every skip naming `::CORBA::TypeCode`, and
the skips propagate upward through containers until `InterfaceDef` itself is
skipped — *`describe_interface`, the operation the IFR facade exists for*. The
MCP bridge speaks the same mapping, so the same ten items are what an agent
cannot read out of an Interface Repository.

측정된 사실 둘: 매핑이 **자기가 쓴 문서를 자기가 거부한다**(에이전트는 되돌려
보낼 수 없는 `any`를 받는다), 그리고 `tk_TypeCode`에는 `Value` 변종이 아예 없어
정적 경로만 되는 타입이 생긴다 — 오라클이 약해지는 게 아니라 **적용 불가**가 된다.
결과: ir-subset 28건 중 10건 스킵, 전파되어 `InterfaceDef`까지, 즉
`describe_interface`가 생성되지 않는다.

---

## The question / 문제

§4.5 gives `any` the form `{"_t": <TypeCode repr>, "_v": ...}` and never says
what `<TypeCode repr>` is. Today it is a bare name string. The question is what
it should be, and the same answer has to serve a second slot: a `TypeCode`
appearing as a **value** in its own right, which the IFR's descriptions are
made of.

---

## The options / 선택지

### A. A repository id, resolved through the registry

`_t` stays a string; the decode side consults a registry to turn
`IDL:gc12/Tagged:1.0` back into a `TypeCode`.

| | |
|---|---|
| Format change | none — documents already look like this |
| Cost | `anyjson` gains a registry parameter; today it takes only `&dyn References` for handles |
| Fails on | anonymous types (an unnamed `sequence<long>` has no repository id — `type_name` already emits `"<anonymous>"` for them), and `tk_TypeCode` as a value, which a name cannot express |
| The deeper objection | it makes the mapping depend on **shared state between sender and receiver**. CDR carries the full TypeCode inside an `any` precisely so that it does not. A JSON mapping that needs both ends to hold the same registry has given up the property the `any` exists for |

### B. A structural TypeCode, recursively encoded — *recommended*

`_t` becomes an object for constructed types and **stays a string for
primitives**:

```json
{"_t": {"kind":"struct","id":"IDL:gc12/Tagged:1.0","name":"Tagged",
        "members":[{"name":"name","type":"string"}]}, "_v": {"name":"x"}}
```

and a `TypeCode`-typed value is that same structure standing alone as `_v`.

| | |
|---|---|
| Format change | **additive.** Every v1 document stays valid: a primitive `_t` is still `"double"`. The object form appears only where v1 could express nothing at all, so nothing that works today changes shape |
| Cost | a second sub-format to specify, version and implement twice (Rust and `_rt.py`), including `TypeCode::Recursive` markers — the same problem the CDR path already solved once, and the one place this will be genuinely fiddly |
| Gain | self-contained, no shared registry, anonymous types expressible, one representation serving both slots, and **readable** — an agent that receives one learns the shape |

### C. Base64 of the CDR-encoded TypeCode

`{"_t": "AAAAD...=="}`.

| | |
|---|---|
| Format change | additive, and trivially exact |
| Cost | **opaque.** §4.5's own justification for the `_t` field is "self-description survives the crossing"; a base64 blob survives the crossing and describes nothing to the reader it crossed for. It also embeds CDR, and an endianness, inside a mapping whose purpose is to not be CDR |
| Gain | zero risk of the two implementations disagreeing about type structure — the Python side would never need to understand a TypeCode at all |

C is the cheapest correct answer and is refused on what §4.5 is *for*. It is
worth recording that it would work.

### D. Keep the limit, make it symmetric

Refuse on the encode side too, so the mapping never writes what it cannot read.

| | |
|---|---|
| Cost | the IFR stays unreadable, the ten items stay unreachable, `tk_TypeCode` stays outside the dynamic path, and §8's *static equals dynamic* oracle stays inapplicable to the types only the static path handles |
| Gain | one honest boundary instead of two disagreeing ones, in an afternoon |

D is not a solution and is listed because **it is what happens by default if
this document is not decided**, and because it is strictly better than today.

---

## Recommendation / 권고

**Adopt B. Take D's symmetry immediately as its first step** — the encode side
should refuse anything it cannot read back, from the first commit, so that the
asymmetry cannot survive a partial implementation of B.

Do not adopt A: a mapping that needs a shared registry is a worse `any` than
CDR's, and the project would be reimplementing the problem the wire format
already solved. Do not adopt C: it satisfies the criterion and defeats the
purpose, and the purpose is written into §4.5's own table.

**B안 채택. 그 첫 단계로 D의 대칭성을 즉시 취한다** — 인코드 측이 읽지 못할 것을
쓰지 않게 만들어, B의 부분 구현이 비대칭을 살려 두지 못하게 한다. A안은 공유
레지스트리에 기대므로 CDR이 이미 푼 문제를 다시 만드는 일이고, C안은 기준은
만족시키되 §4.5의 목적을 무산시킨다.

---

## What approval would mean / 승인의 의미

Approving B commits to:

1. **AnyJSON v1.1**, additive, with the compatibility claim tested rather than
   asserted: every v1 document this project can produce today must still parse
   and reproduce identical CDR under v1.1.
2. **Two implementations moving together.** `_rt.py` is the second
   implementation of §4.5 and `orbweaver-py-bridge` speaks it across a process
   boundary, so v1.1 needs a version discipline the seam does not have yet.
   That discipline is [`D007`](D007-python-wire-seam.md)'s subject, which is
   why this decision should not be approved before that one.
3. **A `Value::TypeCode` variant**, which is a public type change in
   `orbweaver-dynamic` and reaches `wire-fuzz`, the property tests and the
   `static equals dynamic` oracle — all of which should then cover it rather
   than skip it.

승인은 (1) 추가적 v1.1과 **시험된** 호환성 주장, (2) 두 구현의 동시 이동 — 이는
D007이 다루는 버전 규율을 전제하므로 **D007보다 뒤에 승인되어야 한다**, (3)
`Value::TypeCode` 공개 타입 변경과 그것을 건너뛰지 않는 오라클을 뜻한다.
