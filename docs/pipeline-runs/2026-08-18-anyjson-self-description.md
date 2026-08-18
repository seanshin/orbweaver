# 2026-08-18 — AnyJSON says what a type is

> Batch 2 of the order agreed on 2026-08-18. Decisions: [`D008`](../decisions/D008-anyjson-self-description.md)
> (approved, option B) after [`D007`](../decisions/D007-python-wire-seam.md)
> (approved, option A), in that order because D008 depends on the seam's
> version discipline having a home.

## 1. The defect, measured before anything was changed

Read from code first and then **executed**, because a code reading is a claim:

```
to_json(any, Any(Tagged, {...}))  →  {"_t":"IDL:gc12/Tagged:1.0","_v":{"name":"x"}}
from_json(that same document)     →  unknown type "IDL:gc12/Tagged:1.0";
                                     only primitives may cross in an any
encode(tk_TypeCode, <any Value>)  →  expected a value of type typecode, got a long
```

The first turned out worse than the reading suggested. The mapping did not
refuse a constructed `any` — it **wrote a document it then refused to read**.
An agent gets an `any` it cannot send back, and the failure lands on the return
leg, in the caller, rather than at the boundary that produced it. The encode
side named the type by repository id; the decode side resolved through a table
of fifteen primitives.

The second is an absence rather than a bug: no `Value` variant for
`tk_TypeCode` at all, in the crate that §8's *static equals dynamic* oracle
uses as the reference. Where only the static path works, that oracle is not
weaker — it is **inapplicable**.

**Consequence, counted:** `gen-python` over `corpus/services/ir-subset.idl`
generated **18 items and skipped 10**, every skip naming `::CORBA::TypeCode`,
propagating up through containers until `InterfaceDef` itself was skipped —
*`describe_interface`, the operation the IFR facade exists for*.

## 2. What the second implementation taught the first

The structural form's union labels were written as a **value** first, which is
what a language-neutral mapping should carry and what the Python runtime
already uses (`label in case[0]`, values, not bytes). It was reverted on a
measurement:

```
big-endian labels    / Big stream    -> ok, 8 bytes
big-endian labels    / Little stream -> ok, 8 bytes
little-endian labels / Big stream    -> REFUSED: no branch of U matches …
little-endian labels / Little stream -> REFUSED: no branch of U matches …
```

A union TypeCode's case labels are stored in **the byte order of the stream
they were decoded from** — `typecode.rs` reads them with `get_bytes` and writes
them with `put_bytes`, neither of which knows the endianness — and the TypeCode
does not record which that was. Turning those bytes into a number means
guessing. So labels cross as base64: exact, honest about carrying something the
mapping cannot yet interpret, and replaceable by a value the day the wire
defect is fixed.

**That wire defect is real and was deliberately not fixed here.** Our own
encode/decode is self-consistent, which is why nothing was red; a little-endian
peer's labels miss every branch, and the refusal blames the caller's
discriminator. It is `orbweaver-giop` work, and smuggling it into an AnyJSON
batch is exactly what §7.3's stream E note warns against. Recorded in
`COMPONENTS.md`'s giop gap column as the next batch's material.

## 3. What was built

- `Value::TypeCode(Box<TypeCode>)`, with CDR encode/decode reusing
  `orbweaver_giop::typecode` rather than re-deriving it.
- `tc_to_json`/`tc_from_json`: the structural form, **additive** — a type whose
  identity fits in a name keeps its v1 name, the object form appears only where
  v1 said nothing or where the name lost something the wire keeps.
- The `_t` slot and the `tk_TypeCode` value slot share one representation,
  because they are the same question asked twice.
- Python: a `_rt.TypeCode` holder — the document, unread. Enough to receive,
  relay and inspect; not enough to marshal a value *described* by one, which
  would mean Python deciding CDR questions in a package that contains no wire.

## 4. Two defects the work introduced, caught by the gates it wrote

1. **`short_name` emitted four names `named_type` refused** (`any`, `typecode`,
   `void`, `null`) — the mapping writing what it cannot read, reintroduced one
   table apart and two hours after the document arguing against it. Pinned by
   `short_name_and_named_type_are_inverses`, enumerated rather than spot-checked
   because a spot check is what missed it.
2. **The oracle did not cover the new type.** `witness` in `python_target.rs`
   returned `None` for `TypeCode`, so the 28 items *generated* and their values
   never crossed. Green, and measuring nothing. Fixed with a constructed
   witness — a primitive one would have crossed as the same name string v1
   already used and proved nothing.

## 5. Measurements

| | before | after |
|---|---:|---:|
| `gen-python` over `ir-subset.idl` | 18 generated, **10 skipped** | **28 generated, 0 skipped** |
| cross-implementation round trip, `corpus/services` | 12 values / 12 calls | **21 / 21**, 0 divergences |
| cross-implementation round trip, `corpus/golden` | 73 values / 100 calls | **73 / 104**, 0 divergences |
| `cargo test --workspace` | 1198 | **1205** |

The agent path is asserted separately, over the real contract **by repository
id** rather than over a TypeCode written in the test: all five IFR descriptions
cross and return byte-identically in both byte orders, and the test also
asserts a structural TypeCode is present in the document — so a contract that
quietly lost its `::CORBA::TypeCode` members could not pass by round-tripping
nothing.

## 6. What was scoped out, with reasons

- **An `any` carrying a constructed type, from Python.** The Rust half reads
  and writes v1.1's `_t`; `_rt.py` reads only a named type and **refuses the
  structural form with a message naming D008**, rather than accepting the
  document and marshalling `_v` as something else. Building it untested was the
  alternative, and there is no corpus material for it today.
- **Union labels as values** — blocked on the wire defect in §2.
- **The wire defect itself** — a `orbweaver-giop` batch, named and recorded.

## 7. 한국어 요약 / Korean summary

**결함(실행으로 재현).** ① 매핑이 구성 타입을 담은 `any`를 **쓰고 나서 읽기를
거부**했다 — 거부가 아니라 비대칭이므로 실패가 반환 구간에서 터진다. ②
`tk_TypeCode`에는 `Value` 변종이 아예 없어, 정적 경로만 되는 타입에 대해 §8
정적=동적 오라클이 약해지는 게 아니라 **적용 불가**였다. **결과: `ir-subset`
28건 중 10건 스킵, 전파되어 `InterfaceDef`까지 — `describe_interface`가
생성되지 않았다.**

**두 번째 구현이 첫 번째에게 가르친 것.** union 레이블을 값으로 쓰려다 실측에
막혔다: 레이블은 **디코드한 스트림의 바이트 순서 그대로** 저장되고 TypeCode는
그 순서를 기록하지 않는다. 리틀엔디언 피어의 레이블은 **모든** 분기를 빗나가며,
거부 메시지는 레이블이 아니라 호출자의 판별자를 탓한다. 그래서 레이블은 base64로
건넌다 — 추측하지 않는 정확한 표현. **이 와이어 결함은 여기서 고치지 않았다**:
giop 작업이고, AnyJSON 배치에 밀어 넣는 것이 §7.3 스트림 E 주석이 경고하는 바로
그것이다.

**작업이 만든 결함 둘, 스스로 쓴 게이트가 잡았다.** ① `short_name`이 내보내는
이름 넷을 `named_type`이 거부했다 — 이 문서가 반대한 바로 그 비대칭이 표 하나
건너, 두 시간 뒤에 재발. ② 오라클이 새 타입을 덮지 않았다: 28건이 생성되고 값은
건너지 않았다. 초록이면서 아무것도 재지 않는 상태였다.

**측정.** ir-subset 18+10 → **28+0**, 서비스 코퍼스 교차 왕복 12/12 → **21/21
발산 0**, 골든 73/100 → **73/104 발산 0**, 워크스페이스 1198 → **1205**.
