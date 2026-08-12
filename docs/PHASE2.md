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

---

# Batch 3: the type registry

`docs/PLAN.md` §2.1 rests on CORBA already being "a runtime self-describing
type system", and the Interface Repository is the part of that claim this batch
makes true. Parsed IDL becomes queryable metadata: repository ids, an
inheritance graph, operation signatures, `TypeCode`s, and the SIDL annotations
carried through from source.

## Verified against the wire, not against ourselves

Deriving a `TypeCode` from IDL and encoding it with our own encoder proves that
two pieces of our code agree. The question that matters is whether a stock ORB
produces the *same* type description from the *same* IDL:

```
type registry — TypeCode derived from IDL vs the peer's
  ok   omniORB agrees with the TypeCode we derived for spike::Ragged
  ok   JacORB agrees too — two independent derivations of one IDL type
```

`spike::Ragged` is `octet, long, short, double, octet` — the alignment case —
and both peers return byte-identical type descriptions to the one we built from
its IDL.

우리 인코더와 우리 유도기가 일치한다는 것은 우리 코드 두 조각의 합의일 뿐이다.
중요한 질문은 **같은 IDL에서 순정 ORB가 같은 타입 서술을 만드는가**이고, 두 독립
구현이 그렇다고 답했다.

## `_is_a` without a network call

Phase 1 risk R2 recorded that real deployments frequently run no Interface
Repository. A registry populated from IDL works either way, and answering
`_is_a` from our own inheritance graph is faster than asking and available when
the target is unreachable (§4.7). Multiple inheritance, transitivity and the
implicit `CORBA::Object` base are all covered — and a cycle, which is illegal
IDL that the checker rejects, terminates rather than hanging, because a registry
may be loaded from input nobody checked.

## Details that decide whether a call is even possible

- **Inherited operations resolve.** Stopping at the declaring interface would
  report a perfectly valid call as unknown.
- **Union labels take the discriminator's width.** A boolean label is one octet
  and a long label is four; the wrong width shifts every case that follows.
- **Enumerator labels resolve to their ordinal**, which the discriminator's own
  `TypeCode` supplies.
- **Array dimensions nest outermost-first**: `long M[3][4]` is an array of 3
  arrays of 4, not the reverse.
- **A forward declaration must not erase a body** already registered.
- **SIDL annotations reach the registry** — on interfaces, operations and
  individual parameters. An annotation that stops at the AST helps nobody, and
  carrying it is the whole reason for owning the parser.

## Known limits, stated rather than discovered later

- `#pragma prefix` and explicit `typeid` are not honoured, so a repository id is
  `IDL:` plus the qualified name plus `:1.0`. Every fixture here publishes
  exactly that, and a peer must agree with it for `_is_a` to mean anything —
  but a deployment using a prefix will not match until this lands.
- `valuetype` and `native` register as object references. Neither is marshalled
  in v1 (§4.4), and inventing a wire form for them would be worse than not
  having one.
- The peer cross-check covers one struct. Extending it to unions and enums
  needs fixture operations that carry them.

---

# Batch 4: the object model

`docs/PLAN.md` §4.7 argues that references, identity and lifecycle are what make
a *conversation* possible, and that the AI path needs conversations:
`search_interfaces` → `describe_interface` → `invoke_operation` is a workflow in
which something must hold a reference between steps. This batch supplies that.

```
object model — references, identity, LOCATION_FORWARD
  ok   _is_a answered from the inheritance graph, no network lookup
  ok   an object reference survives as a value and is callable
  ok   omniORB followed a LOCATION_FORWARD we emitted
  ok   JacORB followed a LOCATION_FORWARD we emitted
```

## We can now send what Phase 1 could only follow

`LOCATION_FORWARD` was the archetype of Batch 1's cause C2 — we returned it to
callers as though it were a normal reply, so they decoded a marshalled IOR as
their return value. Batch 1 taught us to *follow* one. This batch emits one, and
both peers retry against it transparently, as §9.4.3.2 requires.

The proof needed care. A forwarded `ping()` still returns 42, so a passing call
proves nothing on its own — the server therefore logs each emission and the
harness requires both the successful call *and* the logged forward. An earlier
run looked like success and was not: a raw diagnostic tool had consumed the
forward first, leaving the peer to receive an ordinary reply.

전달된 `ping()`도 42를 돌려주므로 **호출 성공만으로는 아무것도 증명하지 못한다.**
서버가 발행을 기록하고, 하네스는 성공한 호출과 기록된 forward를 **둘 다** 요구한다.
앞선 실행이 정확히 그 함정에 빠졌다 — 진단 도구가 forward를 먼저 소비했고 피어는
평범한 응답을 받았다.

## `_is_a` without a network call

Answered from the registry's inheritance graph (Batch 3), which is faster than
asking and works when the target is unreachable. omniORB's `_narrow` succeeds
against our server, `_is_a` reports correctly for the interface, for
`CORBA::Object`, and negatively for an unrelated id.

## Identity, with its limits documented

- **`_is_equivalent` confirms identity and can never refute it.** §7.2.1 permits
  `false` for two references that do denote the same object, so anything
  treating a `false` as proof of difference is wrong. Said in the doc comment
  and pinned by a test, because it is the kind of thing a reader assumes.
- **`_hash` buckets references; it does not compare them.** Equivalent
  references hash alike, and the converse does not hold.
- **`_interface` returns `NO_IMPLEMENT`** rather than a nil the caller would
  dereference: answering it needs an Interface Repository object we do not
  expose.

## The POA, and a stale-reference hazard

A transient object key carries the process incarnation, so a reference minted by
a previous run is *recognisably* stale rather than silently landing on whatever
now occupies that id — which would be the worst kind of correct-looking bug.
Persistent keys omit it and are reproducible across runs, which is the point of
the policy. A key minted by a different POA is not ours either.

일시적(transient) 객체 키에는 프로세스 incarnation이 들어간다. 이전 실행이 만든
참조가 **조용히 다른 객체에 도달하는 대신 눈에 띄게 낡은 것**이 되도록 하기 위해서다.

## Layering

A new crate, because `_is_a` needs the registry and the registry already depends
on `orbweaver-giop`; putting the object model in `giop` would have made that
circular. The order is now cdr → giop → idl → registry → object.
