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

---

# Batch 5: contract evolution

`docs/PLAN.md` §5.3 sets out which changes a deployed peer survives. Until now
that table was a set of claims. This batch turns it into a tool that refuses
releases, and — more importantly — checks the claims against a peer that was
built before the change.

```
contract evolution — §5.3 verdicts against a peer that predates the change
  ok   the swapped struct members are flagged BREAKING before release
  ok   omniORB answered the swapped call with the WRONG member, no exception
  ok   an added operation on an un-updated server gives BAD_OPERATION
  ok   after the additive release, old and new clients are both served
  ok   idl-diff refuses the breaking revision (exit 1)
  ok   idl-diff accepts the additive-only revision
```

## The dangerous case is the quiet one

`spikes/evolution_v2.idl` differs from v1 by swapping two `long` members of one
struct and adding one operation. Both edits look harmless in review. Against an
omniORB servant compiled from v1, a client encoding per v2 called
`first({px:11, py:22})` and received **22** — the other member's value. No
exception, no warning, no log line. A caller has no way to detect this.

That is the whole argument for the gate. A breaking change that raised `MARSHAL`
would merely be an outage; this one returns plausible numbers indefinitely.

위험한 쪽은 시끄러운 변경이 아니라 **조용한 변경**이다. 구조체 멤버 두 개를 맞바꾼
v2 클라이언트가 v1 서버를 호출하자 예외 없이 **다른 멤버의 값**이 돌아왔다. CDR은
멤버를 위치로만 인코딩하고 태그도 길이도 싣지 않으므로, 수신자가 이를 알아챌 방법이
없다. protobuf·JSON·Avro의 직관이 여기서 정확히 반대로 작동한다.

## Four verdicts, because "breaking" alone is useless advice

A differ that answers *breaking* to everything can only ever say "change
nothing". §5.3's severities are therefore distinguished:

| Verdict | Meaning | Example |
| --- | --- | --- |
| `compatible` | nobody acts | a new type or interface; a gained base |
| `server-first` | safe if servers roll out first | an added operation or attribute |
| `conditionally breaking` | wire-legal, meaningless to old receivers | an appended enumerator; an added union case |
| `BREAKING` | deployed peers misread or fail | any struct member edit; any signature change; any rename |

`idl-diff` exits non-zero on the bottom two. `--approve <reason>` overrides it
and prints the reason next to the findings, so the decision travels with the
diff rather than living in a chat log — approval records responsibility, it does
not make a change safe.

*server-first* earns its own row because the constraint is on **rollout order**,
not on the change: the harness shows the same added operation returning
`BAD_OPERATION` from an un-updated server and answering correctly after the
release, with the un-recompiled v1 client unaffected throughout.

## One edit must be one finding

The first run reported the single swapped pair **three times** — once truthfully
against the struct, and twice as *"operation `first` changed the type of
parameter 0"*. Both extra lines were derived noise, and worse than noise: they
send a reviewer looking for an edit to `first` that nobody made, and they bury
the one real finding.

The cause was comparing parameter and member types **structurally**. A named
type is its repository id and nothing else, so type *references* are now
compared by identity while the type's own entry carries the structural change.
A test pins both halves: one edit yields one finding, and a parameter re-pointed
at a genuinely different type is still caught.

한 번의 수정은 **하나의 지적**이어야 한다. 첫 실행은 구조체 멤버 교체 하나를 세 번
보고했고, 그중 둘은 아무도 건드리지 않은 연산을 가리켰다. 명명된 타입의 정체성은
저장소 ID뿐이므로, 타입 *참조*는 정체성으로 비교하고 구조적 변경은 타입 자신의
항목에서만 보고하도록 고쳤다.

## A defect the harness surfaced by accident

Building the registry alone compiles `orbweaver-giop` without `euc-kr`, and a
diagnostic helper became dead code in that configuration — invisible until now
because the licence check ran `--no-default-features` behind an exit-status test,
and warnings cannot fail one. That check now runs with `-D warnings`.

Turning warnings into errors then exposed a real bug in Batch 4. `Poa::new`
derived its process incarnation from the address of a temporary `Box`, with a
comment explaining that avoiding the clock kept tests deterministic. The
temporary was freed before the next call, so the allocator returned the same
address and two POAs created in sequence **shared an incarnation** — precisely
the staleness the field exists to detect. It had passed only because the
allocator happened to vary the address; a rebuild stopped it doing so.

The reasoning was the error, not the code: distinctness, not determinism, is
what the field is for. It is now seeded from the clock and the pid and advanced
by a counter, and the two-POA test is joined by one that mints sixty-four.

Batch 4의 실제 버그가 여기서 드러났다. `Poa::new`가 임시 `Box`의 주소로 incarnation을
만들었는데, 그 임시 객체는 즉시 해제되어 할당자가 같은 주소를 재사용했다. 연속으로
만든 두 POA가 **같은 incarnation을 공유**했다 — 이 필드가 막으려던 바로 그 상황이다.
"결정론을 위해 시계를 피한다"는 주석의 **추론 자체가 틀렸다.** 필요한 것은 결정론이
아니라 유일성이다.

## Scope

The differ compares two IDL files. It does not yet read a *released* contract
from a registry of record, so "released" currently means "the file you point it
at"; wiring it to a stored baseline, and to `@ai_since` versioned interfaces, is
Phase 4 work. Value types and `fixed` are still absent, so their evolution rules
are unwritten rather than wrong.

---

# Batch 6: differential conformance, made permanent

The oracle has been `omniidl` on a laptop since Phase 0. That measures one
thing well and another thing not at all: whether we agree with omniORB, and
nothing about whether the corpus means the same to any other compiler.

```
differential conformance — every front end on every corpus file
  56 file(s) through: omniidl jacorb_idl + orbweaver
  ok   our front end matches the corpus everywhere the oracles uphold it
  ok   no unexplained divergence between 2 independent front ends
  note 3 recorded divergence(s), see corpus/divergences.tsv:
       12-any-typecode.idl — omniidl=accept jacorb_idl=reject
       n02-identifier-clash.idl — omniidl=reject jacorb_idl=accept
       n10-operation-name-clash.idl — omniidl=reject jacorb_idl=accept
```

## Two oracles separate two findings that looked identical

With one oracle there is exactly one kind of disagreement and it is always
ours. With two, `spikes/differential.sh` separates:

- **our front end against a consensus** — we are wrong, as `CLAUDE.md` has said
  since Phase 0;
- **the oracles against each other** — the *corpus file* is wrong, because it
  does not mean the same thing to every deployed compiler. Agreeing with either
  oracle cannot surface this, which is why one oracle could never have found it.

A third check falls out for free: the verdict each file is filed under (accept
for `golden/`, reject for `negative/`) is now compared against the consensus
instead of assumed. A golden file that every compiler rejects is a broken
fixture, and quietly agreeing with the compilers would have hidden it.

## The second oracle paid for itself on its first run

Three divergences, two causes, and both are worth knowing:

**JacORB 3.9 does not enforce the case-insensitive identifier rule.** It accepts
`struct T { Position position; }` and `Blob blob(in unsigned long size)` — the
two shapes of the failure that dominated Phase 0 and that this project has
tripped over five times. CORBA 3.4 §7.2.3 makes identifiers collide
case-insensitively and omniidl rejects both. We follow the specification.

This is not a reason to loosen; it is a reason the strictness matters. IDL that
builds cleanly under JacORB can fail under omniORB, which is exactly the kind of
late, confusing breakage the generator pipeline exists to prevent.

**JacORB 3.9 cannot resolve `::CORBA::TypeCode`.** It reports `Undefined name:
gc12.Bag.CORBA.TypeCode` — it looked an absolute scope up relative to the
enclosing one. `CLAUDE.md` requires the qualified spelling because the
unqualified one is itself a case clash, so the corpus file stays as written.

두 번째 오라클이 첫 실행에서 값을 했다. **JacORB 3.9는 대소문자 무시 식별자 충돌
규칙을 강제하지 않는다** — Phase 0을 지배했고 이 프로젝트가 다섯 번 걸려 넘어진 바로
그 규칙이다. 이는 규칙을 느슨하게 할 이유가 아니라, 엄격함이 왜 중요한지에 대한
근거다. JacORB에서 깨끗하게 빌드되는 IDL이 omniORB에서 실패할 수 있다.

## A divergence must be explained, and the explanation must expire

`corpus/divergences.tsv` records each one with the reason we side with one
oracle. An entry exempts a file from failing, never from being reported — the
three above are printed on every run.

The registry is checked in both directions, and both directions were tested by
temporarily breaking them rather than assumed to work: an **unrecorded**
divergence fails and names the file, and a **recorded divergence that stops
happening** also fails, because an exemption that no longer describes reality
silently covers whatever moves into its place.

등록은 실패 면제일 뿐 보고 면제가 아니다. 미등록 불일치도, **더 이상 발생하지 않는
등록**도 실패한다. 현실을 설명하지 못하는 면제는 그 자리에 들어오는 다른 문제를
조용히 덮기 때문이다. 두 방향 모두 일부러 깨뜨려 발동을 확인했다.

오라클이 하나면 불일치는 언제나 우리 잘못이다. 둘이면 **우리가 틀린 경우**와
**말뭉치 파일이 이식 가능하지 않은 경우**가 분리된다. 후자는 어느 한쪽과 일치하는
것만으로는 절대 드러나지 않는다.

## An absent oracle is a failure, not a skip

`--require omniidl,tao_idl` makes a missing compiler fatal. CI passes it; a
laptop usually has one oracle and the harness says so, incrementing the
unmeasured counter rather than printing a green line. This is the Phase 0
harness rule applied to the oracle itself.

## Gating fmt and clippy found three lies in the code

None of the three was a crash. All three told the next reader something untrue
about the protocol, which is the more expensive kind of defect in a codebase
whose whole claim is that it implements a published specification correctly.

- **`Connection::invoke` wrapped its reply handling in a loop where every branch
  returned.** The structure announced that some messages could be skipped and
  the read retried. Nothing may do that until request multiplexing exists: with
  one outstanding request, a message that is not our reply means our accounting
  is wrong, and reading past it compounds the error instead of recovering.

- **`handle_request` branched on GIOP version to compute a reply header length,
  with the same value in both arms.** It read as though the 1.0/1.1-versus-1.2
  service-context reordering had been accounted for. It has not been: both come
  to 12 bytes *only* because the context list we emit is empty. The constant now
  says so, and says what would break it.

- **`is_supported`** is explicitly `#[allow]`ed rather than incidentally warned
  about, because folding its `cfg!` arm into `matches!` would hide that EUC-KR
  support is a build-time question.

`rustfmt.toml` pins the house style — `max_width = 100`, `use_small_heuristics
= "Max"` — and the workspace now carries `clippy::all` as a warning, promoted to
an error in CI.

fmt·clippy를 게이트로 걸기 위해 먼저 깨끗하게 만드는 과정에서 **프로토콜에 대해
사실이 아닌 말을 하는 코드 세 곳**이 나왔다. 셋 다 크래시는 아니지만, 공개 명세를
정확히 구현했다고 주장하는 코드베이스에서는 더 비싼 종류의 결함이다.

## What CI runs

Three jobs, each on a throwaway `ubuntu-latest` runner. omniORB, TAO and JacORB
are `apt`-installed or downloaded **into the runner** and never published as an
artifact — publishing would be redistribution, which §10 forbids.

| Job | What it establishes |
| --- | --- |
| `rust` | fmt, `clippy -D warnings`, tests under `-D warnings`, the attribution-free build, and `cargo tree` free of ORB fixtures |
| `differential` | our front end against **omniidl and JacORB's IDL compiler**, both required |
| `interop` | the full harness: both directions, both peers, GIOP 1.0/1.1/1.2 |

TAO is not installed: Ubuntu packages no `tao-idl`, which the workflow's first
run established rather than assumed. The script still picks it up if a runner
has it. Installs are best-effort and the harness scripts are the gate, because
an apt step that aborts the job reports a wrong package name and nothing about
the code — which is precisely what the first run did, twice, for one reason:
**I guessed two package names instead of checking them.**

apt 설치는 의도적으로 best-effort이고 판정은 하네스 스크립트가 한다. 중단된 apt
단계는 패키지 이름이 틀렸다는 것만 알려주고 코드에 대해서는 아무것도 알려주지 않는다
— 첫 실행이 정확히 그랬다. 두 job의 실패, 원인은 하나: **패키지 이름을 확인하지 않고
추측했다.**
