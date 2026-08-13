# Phase 4 / Stream B — static generation

Stream B of `docs/PLAN.md` §7.3, first batch. The batch unit the plan names:
one backend target across the **whole golden corpus** at once — generate every
stub, compile every stub, run against the fixture — with §8's oracle, *static
result equals dynamic result*.

계획 §7.3의 스트림 B, 1차 배치. 일괄 단위는 계획이 명시한 대로 "백엔드 타깃 하나 ×
golden 말뭉치 전체"이고, 오라클은 §8의 *정적 결과 = 동적 결과*다.

---

# Batch 1: the Rust backend

```
static generation — stubs from the registry, oracle: static equals dynamic
  ok   77 item(s) generated from the golden corpus plus the fixture
  ok   every generated stub compiles outside the workspace
  ok   static bytes equal dynamic bytes: Ragged, wstring, any, sequence, both orders
  ok   the generated stub calls omniORB: 10/10 cases, both byte orders
```

## The dynamic path is the reference implementation

Generation starts only now because there had to be something trustworthy to be
equal to. The dynamic path is the one verified against two independent ORBs, so
a generated stub is correct exactly when it produces the same bytes for the
same values — and that is what the oracle compares, byte for byte, in both byte
orders, before any call is made.

## A generated file contains names, never rules

Every marshalling decision in a stub is a call into `orbweaver_gen::rt`, so the
wire knowledge exists once. Phase 3 measured what duplicating it costs (the
`wstring` BOM failure came from re-implementing instead of reusing), and a code
generator is a machine for duplicating things — the discipline has to be
structural. `rt` reuses `WideCodec` for wide text and the dynamic `Value` path
for `any`, because an `any` is dynamic by definition and a static mirror of it
would be a second implementation of the same rules.

생성된 파일에는 **이름과 순서**만 있고 규칙은 없다. 모든 마샬링 결정은 `rt` 호출이며,
와이어 지식은 한 곳에만 존재한다. Phase 3이 중복의 값을 측정했고, 코드 생성기는
중복을 만드는 기계이므로 규율은 구조여야 한다.

## The oracle found a registry bug on its first run

The generator's union test produced a Rust enum with **two variants named `s`**
— impossible code. The cause was upstream: the registry expands
`case 2: case 3:` into two cases but computed `default_index` against the
**unexpanded** AST list, so any multi-label branch before the default shifted
it onto the wrong case.

Every consumer inherited that: the dynamic invoker selects default branches
from this index, and the TypeCode we encode for peers carries it. It survived
because the existing test **asserted the buggy semantics** — `default_index ==
1, "index of the default *branch*"` — which is how a wrong implementation
outlives its own test suite. The test now asserts the expanded index and says
why, and a regression test pins the shifted case.

오라클이 첫 실행에서 레지스트리 버그를 찾았다. 다중 라벨 전개 후 `default_index`가
전개 **전** 목록 기준으로 계산되어 잘못된 case를 가리켰고, 동적 호출기의 default
분기 선택과 피어에게 보내는 TypeCode가 모두 물려받았다. 기존 테스트가 **버그의
의미론을 그대로 단언**하고 있었기에 살아남았다 — 틀린 구현이 자기 테스트를 통과하는
전형적 경로다.

## Skips cascade, with the reason attached

A struct whose member is a `fixed` typedef must not generate: it would compile
against a type that was never written, moving the failure to the consumer's
compiler with the §4.4 reason lost on the way. Representability is checked
transitively, so `Amount` → `Invoice` → `Billing` all skip, each naming
`fixed<9,2>` and the plan section. Constants are a named non-goal of this batch
(the registry records the type, not the value) and are reported the same way.

## What the generated crate proves by existing

`gen-corpus` writes a crate deliberately **outside** the workspace, with path
dependencies only on the published crates. Compiling it proves the stubs stand
on the public surface alone. The oracle binary inside it is a fixed template,
never generated — a test the generator writes for itself proves nothing.

## Scope

Rust only; Python is the next backend batch. User exceptions decode as errors,
not yet as typed values. Bounded strings/sequences are not enforced at the type
level. The wide-character codec is pinned to GIOP 1.2 + UTF-16 (the dynamic
default) rather than taken per-connection. The promotion engine and I1/I4
integration batches (stubs through the guard, identity preserved across
promotion) are separate batches, as §7.4 requires.
