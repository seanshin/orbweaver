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

---

# Batch 2: semantic analysis

The parser accepts anything shaped like IDL. This pass decides whether it
*means* anything — and it retires `spikes/idl_lint.py`, the regex
approximation that had been standing in for it.

**Result: complete agreement with the oracle.** All 21 golden files and the
20-file benchmark are clean; all 10 negatives are rejected, semantic ones
included. The exclusion list that Batch 1 needed is gone.

파서는 IDL처럼 생긴 것을 전부 받는다. 이 패스가 그것이 *의미*를 갖는지 판정하고,
임시 정규식 린트를 은퇴시킨다. **오라클과 완전히 일치한다.**

## The rule took a third shape, and the oracle had to settle it

The identifier rule was already known in two forms. Implementing it properly
turned up a third, and a guess would have been wrong in both directions.

Four oracle queries were needed to find the boundary:

| Input | Oracle |
|---|---|
| `Token issue(); void v(in string token);` | **accepted** |
| `void v(in Token token);` | rejected |
| `Token issue(in string token);` | **accepted** |
| `long position(); Position get();` | rejected |

So a parameter name lives in **its own parameter list**, not in the interface:
it collides with types named *in that list* and not with the return type, nor
with anything another operation mentions. Operation names, by contrast, are in
the interface scope and do collide across operations.

파라미터 이름은 인터페이스가 아니라 **자기 파라미터 목록** 안에 산다. 같은 목록에
등장한 타입과는 충돌하지만 반환형이나 다른 연산과는 충돌하지 않는다. 연산 이름은
반대로 인터페이스 범위에 있어 연산 간에도 충돌한다.

Two of the tests written before asking were **wrong**, in the accepting
direction: `struct S { A a; }` against an `enum A` does clash, and a constant
used as a type reports the clash rather than a type error. Both were corrected
to what the oracle says rather than what seemed reasonable.

## Two implementation bugs the corpus caught

- **Reopened scopes merged differently-cased names.** "A symbol of this kind
  already exists" was read as "this is the definition of that forward
  declaration", which silently unified `struct A` and `struct a` in a reopened
  module. Symbols now record whether they are defined, and completing a forward
  declaration requires the *exact* spelling.
- **Inherited names were invisible to declaration checks**, so a derived
  interface could redeclare a base operation. Declaration now consults
  inherited scopes.

## Cascading diagnostics are suppressed

A reference that resolves to a differently-spelled symbol is the case clash
already reported. Emitting a second diagnostic from the same cause would send
the self-repair loop after the consequence instead of the cause — which is a
real cost, not an aesthetic one, since the loop fixes what it is told about.

연쇄 진단을 억제한다. 같은 원인에서 나온 두 번째 진단은 자가수정 루프를 원인이
아니라 결과로 보낸다.

## Why the regex had to go

`spikes/idl_lint.py` matched syntax, so each new shape of the rule needed a new
pattern — and it missed two of them: struct scopes, then operation names. A
scope tree expresses the rule once. The replacement also catches what a regex
never could: unknown names, duplicate declarations, inherited collisions,
repeated union labels, and reserved words used as identifiers.

Every diagnostic names the fix. `TypeCode` unqualified reports *write
`::CORBA::TypeCode`*; a reserved word reports *write `_interface`*; a case
clash says to rename the member rather than the type, because the type name is
what callers depend on.
