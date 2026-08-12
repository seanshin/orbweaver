# Phase 2 — IDL compiler, registry and object model

> In progress. Reproduce with `./spikes/run_checks.sh`
> Phase 1 closed: [`PHASE1.md`](PHASE1.md)

---

# Batch 1: the IDL front end

`orbweaver-idl` exists to be **ours**: MIT, and able to carry SIDL semantics
that deployed compilers reject. Phase 0 assumption C established that omniidl
refuses IDL 4 `@annotation`, and the structured-comment fallback is only viable
because we own the parser — this is the batch that makes that true rather than
promised.

`orbweaver-idl`은 **우리 것**이어야 한다. Phase 0 가정 C에서 배포된 컴파일러가
IDL 4 `@annotation`을 거부함이 확인됐고, 구조화 주석 폴백이 성립하는 이유가
파서를 우리가 소유하기 때문이다. 이 배치가 그 약속을 사실로 만든다.

## Correctness is agreement, not taste

The acceptance criterion is not a reading of the grammar. `omniidl` accepts
every file in `corpus/golden/` and rejects every file in `corpus/negative/`, so
**anywhere we differ, we are wrong.** The corpus is the specification of this
crate's behaviour, and the harness enforces it:

```
orbweaver-idl — our parser against the oracle
  ok   accepts all 21 golden files and the 20-file benchmark
  ok   rejects the syntactic negatives, including unescaped keywords
```

인수 기준은 문법 해석이 아니라 **오라클과의 일치**다. 다른 곳이 있으면 우리가
틀린 것이다.

## Comments are not noise here

A lexer normally drops comments. This one must not: SIDL lives in
`//@ key: value`, so discarding comments would discard the meaning layer the
whole project rests on. Annotations attach to the *next* token, which keeps a
declaration bound to what was written above it no matter how the parser later
reorders anything.

## What the corpus caught

`n08-reserved-word.idl` — `long interface;` — was accepted at first. The lexer
emits keywords as plain identifiers, and it was also stripping the leading
underscore that escapes one, so the parser could not tell `interface` from
`_interface`. The escape is now a flag on the token, and an unescaped keyword
used as a name is an error that names the fix.

렉서가 키워드 이스케이프용 밑줄을 버리고 있어 `interface`와 `_interface`를 구별할
수 없었다. 이제 이스케이프 여부를 토큰에 남긴다.

Two grammar details worth pinning, both of which fail far from their cause:

- **`>>` must be split when it closes nested generics.** `sequence<sequence<long>>`
  lexes its tail as one shift operator, and treating that as a single closing
  bracket loses a nesting level.
- **A parameter needs a direction.** Omitting `in` is a common generation
  mistake, so the message says to write one rather than reporting an
  unexpected token.

## Scope of this batch

Parsed and represented: modules, interfaces with inheritance and forward
declarations, structs, unions with multiple labels and `default`, enums,
exceptions, typedefs with array dimensions, constants with the full expression
grammar, valuetypes, natives, attributes, operations with `raises` and
`context`, and every builtin type including `fixed`, `sequence` and bounded
strings.

Not this batch: name resolution, the semantic checks the oracle applies (the
eight semantic negatives are excluded from the parser test by name rather than
quietly passing), the SIDL vocabulary itself, and code generation.
