# Changelog / 변경 이력

Measurements live in [`docs/COMPONENTS.md`](docs/COMPONENTS.md); this file
records what changed and, where it matters, what it changes on the wire.

측정은 `COMPONENTS.md`에, 여기에는 무엇이 바뀌었는지와 — 중요한 경우 — 그것이
와이어에서 무엇을 바꾸는지를 적는다.

---

## Unreleased

### ⚠ Mapping changed / 매핑 변경

- **AnyJSON v1.1: a type describes itself structurally** (D008, approved
  2026-08-18). `_t` keeps its v1 name for a type whose identity fits in one —
  `"double"` is still `"double"` and every v1 document still reads and still
  reproduces the same CDR — and becomes an object where v1 could say nothing,
  or where the name lost something the wire keeps: `string<5>` and `string`
  were one word to v1 and are two TypeCodes to a peer. The same representation
  is now also a **value**, so `::CORBA::TypeCode` crosses.

  Measured before the change: `to_json` wrote `{"_t":"IDL:gc12/Tagged:1.0",…}`
  and `from_json` **refused that same document**. Not a limitation — an
  asymmetry, which puts the failure on the return leg in the caller rather than
  at the boundary that produced it. And `tk_TypeCode` had no `Value` variant at
  all, so §8's *static equals dynamic* oracle was not weaker for the operations
  only the static path handled; it was **inapplicable**.

  **`ir-subset` went from 18 generated + 10 skipped to 28 + 0.** The ten
  included `InterfaceDef` itself — the skip propagated up through every
  container until `describe_interface`, the operation the IFR facade exists
  for, could not be generated. The MCP bridge speaks the same mapping, so an
  **Interface Repository is now readable through the agent path**, asserted by
  repository id over the real contract in both byte orders.

  **Upgrading:** nothing to do for a v1 producer or consumer; the change is
  additive and the compatibility claim is tested, not asserted. A Python client
  gains `_rt.TypeCode` — the document, unread: enough to receive one, hand it
  back and inspect its `kind`, not enough to marshal a value *described* by
  one, which would mean Python deciding CDR questions in a package that
  deliberately contains no wire.

  AnyJSON v1.1: `_t`가 이름 하나에 담기는 타입은 v1 그대로, 그 밖에는 구조가
  된다. **추가적**이므로 v1 문서는 전부 그대로 읽히며 그 주장은 시험된다.
  변경 전 실측: 매핑이 자기가 쓴 문서를 자기가 거부했고, `tk_TypeCode`에는
  `Value`가 없었다. **ir-subset 18+10 → 28+0**, 그 열에는 `InterfaceDef` 자신이
  들어 있었다.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **A union `TypeCode`'s case labels are aligned and byte-order-normalised.**
  A label is the discriminator marshalled in its own type. It was read with
  `get_bytes` and written with `put_bytes` — neither of which knows the
  endianness, and neither of which aligns — so it carried the byte order of
  whatever stream produced it. Our encode and decode agreed with each other in
  any order, which is why 1200 tests were green while, against the fixture peer
  on this host:

  - a `long long` discriminated union **could not be decoded at all**, and said
    so as `"string length must include the NUL"` — pointing four fields past
    the fault, at a string, because an unaligned 8-byte read shifted everything
    after it;
  - a `long` discriminated union decoded and then missed **every** branch, in
    both stream orders, with a refusal that blamed the caller's discriminator.

  `UnionCase::label` is now always big-endian and conversion happens at the
  wire. Both cases are pinned as the bytes omniORB actually wrote, and the
  regression test also **re-encodes them back to the peer's bytes** — the check
  our own round trip could not perform, because it agreed with itself.

  **Upgrading:** anything holding a `UnionCase::label` built by hand for a
  little-endian stream was already wrong on the wire and is now wrong in the
  type; build labels big-endian. Nothing else changes.

  union 레이블은 판별자를 그 타입으로 마샬링한 것인데, 정렬 없이 원바이트로
  읽고 썼기에 **온 스트림의 바이트 순서를 그대로** 지녔다. 우리끼리는 일치했으므로
  1200건이 초록이었다: `long long` union은 **디코드조차 되지 않았고**(진단은 네
  필드 뒤의 문자열을 가리켰다), `long` union은 **모든 분기를 빗나가며** 호출자를
  탓했다. 이제 레이블은 항상 빅엔디언이고 변환은 와이어에서 일어난다. 피어가
  실제로 쓴 바이트를 고정했고, **그 바이트로 되돌려 인코딩되는지**까지 검사한다.

- **AnyJSON writes a union's case labels as values, not base64.** They were
  base64 for exactly as long as their byte order was unknowable — one commit.
  An enum discriminator's labels now read `"label":"GREEN"` rather than four
  bytes that say nothing. A label that will not decode falls back to
  `{"_raw": <base64>}`, tagged, because a malformed TypeCode is its producer's
  problem and a renderer that refuses to render it hides the evidence.

### Known limits / 알려진 한계

- **`_rt.py` reads only a named type in an `any`'s `_t`.** The Rust half reads
  and writes the structural form; the Python half refuses it by name, with the
  decision cited, rather than accepting the document and marshalling `_v` as
  something else — the same rule the Rust side follows, applied to whichever
  implementation is behind.

### Added / 추가

- **A decision's status is checked, not just written.**
  `spikes/decision_status.py` reads the authoritative status out of each
  `docs/decisions/D00N-*.md` and holds every other mention to it, in the
  harness. Dated records — `pipeline-runs/`, `PHASE*.md`, released sections of
  this file — are out of scope by construction: they state what was true on a
  date, and editing them to match today would falsify them rather than repair
  them. The gate also refuses a citation to a decision that does not exist.

  결정의 상태는 `docs/decisions/`에 한 번만 산다. 나머지 언급은 하네스가 그것과
  대조한다. 날짜가 붙은 기록(실행 기록·PHASE·릴리즈된 절)은 그 시점의 사실이므로
  범위 밖이다 — 오늘 기준으로 고치는 것은 수리가 아니라 위조다.

### Fixed / 수정

- **Ten stale status claims, and the four remaining-work lists no gate can
  see.** Four decisions the user approved on 2026-08-14 — D003, D004, D005,
  D006 — were still being called open in five documents. `PLAN.md` §7.2 still
  listed three Phase 4 items as remaining — a servant that cannot raise a system exception,
  a missing server-side static-equals-dynamic oracle, and no Python target —
  when all three had landed, and stream E carried an un-struck second copy of
  its own last three items through three passes. Nothing here was a code
  defect and nothing could go red: the measured cost was **a planning pass
  spent proposing work that was already finished.** §7.2 no longer restates a
  remaining-work list at all; it links the `COMPONENTS.md` row that owns one.

  다섯 문서가 이미 승인된 결정 넷을 PROPOSED로 불렀고, §7.2는 이미 착지한 세
  항목을 남은 일로 열거하고 있었다. 코드 결함은 하나도 없고 빨개질 수 있는
  것도 없었다 — 실측된 비용은 **이미 끝난 일을 다음 작업으로 제안한 계획 한
  번**이다.

- **`D003` said APPROVED in English and 제안 in Korean, four lines apart.** The
  approval edit overwrote the head of its own PROPOSED block and left the tail,
  in the file that is the source of truth — and every document that copied it
  copied the English half. The gate now requires a decision's status markers to
  agree with each other before it checks anyone against them.

- **The gate's own two blind spots, found by running it.** It read documents
  line by line, so a claim whose reference and status word fell either side of
  a markdown wrap was invisible — `PLAN-MOE.md` passed while saying the wrong
  thing, and only its Korean twin was caught, because that one happened not to
  wrap. It also attached every status word to every decision named in the same
  sentence, which reported two correct sentences as drift. Passages are now
  built per paragraph, and a status word binds to the decision most recently
  named before it. First run 6 findings, after both repairs 10, false positives
  0. It then caught the first draft of this very entry, which quoted the old
  status without naming the new one — the entry was reworded rather than the
  gate loosened.

---

## v0.4.0 — 2026-08-14

The release a **consumer-shaped input** produced. Thirteen legacy contracts
that include each other, four `#pragma prefix` styles, an acquired company's
prefix inheriting our base, one file with no prefix at all, and nothing
annotated anywhere — the shape an estate actually has, rather than the shape
`corpus/` has. It found eight root causes in one pass, **six of which no test
in this repository could ever have gone red on**, because every corpus file is
self-contained and every corpus file is annotated.

Two of them change what a deployment does. They lead.

이번 릴리즈는 **소비자 모양의 입력**이 만들었다. 서로 include 하는 13개의 레거시
계약, prefix 스타일 넷, 주석은 어디에도 없음. 근본원인 8건이 한 번에 나왔고 그중
**6건은 이 저장소의 어떤 테스트도 빨갛게 될 수 없던 것**이다. 코퍼스 파일은 전부
self-contained이고 전부 주석이 달려 있기 때문이다.

### ⚠ Behaviour changed / 동작 변경

- **An unannotated operation is now refused, where it used to be allowed.**
  The bridge asked `annotations.get("ai_effect")?`, so a *misspelled* effect
  required approval and a *missing* one did not. Expose the estate's twelve
  interfaces with no scopes and the answer was **`allow=76, refuse=0`** —
  `SHUTDOWN`, `purge`, `void_invoice` and a writable attribute's setter among
  them. `COMPONENTS.md` recorded this hole for *ingested* contracts; it was a
  property of **any** unannotated one, which is what every legacy estate is.
  And S4 advised fifty-two times over that estate that the bridge *"must assume
  it needs approval"* — measured false. One part of the system stated the safe
  rule while the mechanism did the opposite.

  Closed means **refused**, with its own `Denied::EffectUnstated`. Not
  approval-required: an approval is a human saying yes to a *described* call,
  nobody can say yes to a call whose effect nobody stated, and one `--approve`
  would otherwise unlock a whole estate at once. Not not-exposed: the operator
  did expose it, and saying otherwise sends them into the allowlist to debug a
  contract problem.

  **Upgrading:** a contract that annotates its operations is unaffected
  (`end_to_end.sh` still reads `allow=2 need_scope=1`, unmoved). A contract
  that does not needs either `//@ ai_effect:` on its operations or the
  exposure's new **`--assume-effect <value>`** — the operator's single
  declaration of what that estate's silence means, run through the same
  recognition a contract's own value gets. Not seventy-six approvals. Every
  allow resting on it says so: `assumed: true`, `effect_stated_by: "exposure"`,
  and a startup line counts the silence.

  Absent and unrecognised are now deliberately different, in the direction that
  costs something: a typo reaches a human, because somebody was annotating and
  got one wrong; a silence goes back to the contract, because there is nothing
  there to say yes to.
  *주석 없는 오퍼레이션은 이제 거부된다. 침묵은 허가가 아니다.*

- **A `CloseConnection` between fragments is neither corruption nor a clean
  retry.** Both existing answers were false statements about the stream.
  `Desynchronized` says the peer is broken and the connection is corrupt, about
  a well-framed orderly goodbye — the client gives up on a service that was
  merely restarting. But `ConnectionClosed` hands the caller §13.5.1's promise
  that outstanding messages *were not processed and may be safely resent*, and
  that promise is about messages **without replies**. A peer that had already
  sent the leading piece of a reply demonstrably processed that request;
  re-sending a non-idempotent operation on that promise runs it twice, silently,
  on a pooled connection.

  So the retry fact is **per caller, not per connection**.
  `Error::InterruptedMidReassembly` carries what arrived and what was
  outstanding, `Fault::unsent` is false for the cut call and true for every
  other waiter, and `Error::is_orderly_close()` decides teardown against
  corruption on a value rather than on a message string. At GIOP 1.1
  `FragmentUnsupported` wins and the close is left *unread* — 1.1's refusal is
  permanent for that peer and a close is retryable, so preferring the close
  sends the pool round to hear the same thing again. A `MessageError` mid-chain
  is a report rather than corruption, but §9.4.8 gives it no body, so it names
  nothing and makes **no** request re-sendable. The serving loop stopped
  answering a goodbye with a `MessageError`, and stopped answering a
  `MessageError` with a `MessageError`, which was a two-ORB loop.

  **Unmeasured, and named as such:** the shape needs a peer to shut down
  between two writes of one reply, and neither fixture exposes that window.
  What *is* measured is the regression — omniORB 4.3.4's real 1.2 fragments
  still reassemble.

- **`describe_interface` reports the resolved surface.** Ten of twelve estate
  interfaces inherit; the agent was shown 11 operations, the guard judged 13,
  the servant served 13. Nothing was allowed that policy forbade — but the
  description an agent plans against was not the surface it could reach, and a
  reviewer reading it did not know the rest was there. Three walks became one,
  public, so nothing has an excuse to write a fifth.

### Added / 추가

- **`#include` resolves.** Eleven of thirteen estate files failed our own S4
  while `omniidl` accepted all thirteen, and the ~90 diagnostics were one
  thing: a name declared in an included file reported as undeclared. Quoted
  includes resolve against the including file's own directory, angled against
  `-I`, and the working directory is **never** searched — a validator run from
  a build directory would otherwise resolve differently from the same validator
  run from the source tree, and the difference would surface as a repository id
  rather than as an error. Once-only by canonical path, so **guards are not
  required** (the estate measured that real IDL in the wild has none), with
  non-blocking advice when an unguarded file repeats, because a deployed
  compiler rejects what we accept and being quietly laxer is worse than saying
  so. Cycles name the loop; a missing file lists every path searched; a
  diagnostic reports the included file's own line with the chain.

  The prefix rule across a file boundary was **measured, not reasoned** — two
  probes through `omniidl -Wbinline` say the includer's prefix does not enter
  and the included file's does not escape. Thirteen roots, 49 ids, all agreeing
  with `omniidl`. Full C preprocessing stays out and is **refused rather than
  skipped**: skipping `#if` compiles every arm at once, which is a silent
  misparse.

  `sidl-validate` takes `-I` and resolves before validating, which moves the
  estate's per-file acceptance from **2/13 to 13/13**. One of the original two
  passed *because* its include was silently skipped — the defect wearing a pass.
  *`#include`가 해석된다. 코퍼스 파일이 전부 self-contained이라 여섯 페이즈 동안
  아무 테스트도 빨갛게 되지 않았다.*

- **A Python client target.** The point of a second target language is not the
  language — it is that a second target is the only thing separating the IDL
  mapping from what happened to be convenient in Rust. It paid immediately: the
  **Rust** emitter's keyword list was missing Rust's own reserved words, so a
  contract naming `yield` emitted `fn yield()` and did not compile. No
  emitter's escaping had ever been *executed*, because no corpus file named a
  target language's reserved word until `28-target-keywords.idl` did.

  The seam is a **local process, not FFI**: generated Python renders arguments
  as AnyJSON v1 and hands them to `orbweaver-py-bridge`. No new dependency, no
  second wire implementation, `cargo tree` unchanged. **D007 is PROPOSED**, not
  adopted, and compares it against a PyO3 extension module (a new dependency
  class *and* an `unsafe_code` question at the boundary) and pure-Python
  CDR/GIOP. Mapping implemented against the OMG specification and *compared*
  against `omniidl -bpython` — 129 scope names and 100 operation names agree,
  and no name exists on our side that `omniidl` does not also produce.
  Comparing is not deriving.

  Measured: 28 golden contracts and 12 services generate, import and execute —
  73 values and 100 calls round-tripped, both byte orders, 0 divergences; a
  generated Python client completed 12 real calls against the omniORB fixture.

- **D005 option B — a regeneration is diffed against what is registered.**
  "Registered" is a directory the run is pointed at, which is what a previous
  run's output already is. The S5 catalog was rejected because `register()`
  builds it *after* S4 out of the artifacts S4 just gated, so an item would be
  diffed against itself. An absent contract is **counted and printed**, never
  silence; a registered-but-unreadable one is an error.

  What B cannot see is measured rather than asserted: a regeneration keeping
  every identifier and changing only `//@ ai_authz` is compatible by §5.3 and
  passes, with option C refusing it in the same test. That is why C landed
  first, and why the harness runs both.

- **Declared bounds are enforced by generated code**, at the same refusal point
  the dynamic path uses, and the §8 oracle gained the reading that would have
  caught the gap: for a value that violates its contract, the two paths must
  write the same bytes and reach the same verdict. Byte equality over valid
  values never could — which is how the divergence survived until D006 measured
  it while arguing about something else.
- **D005 option C**: a scope-shaped token the requirement states verbatim must
  survive to the `//@ ai_authz` S3 emits, checked by string equality with no
  model. Finding recorded rather than smoothed: **none of the twenty frozen
  requirements states a scope literally**, so the benchmark cannot exercise the
  rule at all — and of six naturally-phrased new ones, **three do not trip it
  either**. Korean prose, `ROLE_REGISTRAR` and `warehouse/robot/estop` are all
  ordinary ways to state a permission and all invisible to the rule, so a
  project with any of those house styles gets no binding and gets it silently.
  Not patched: accepting `ROLE_REGISTRAR` accepts every upper-case constant.
- **D005 and D006 approved.** `Expert::process` and `Router::dispatch` are
  excluded; the bound is their return path via a new versioned interface.
- **The content interceptor seat can read the arguments.** The chain was *not*
  moved after mapping, which is the fix people reach for: a chain there answers
  a mapping error before a policy refusal, turning failures into an oracle for
  the shape of operations the caller may not call. `CallContext` carries the
  agent's arguments unmapped instead. The static path supplies `None`, which is
  §4.7's bypass wearing a safety label and is now in the module docs rather
  than left to be discovered.
- **IF2's two stores are joinable** (`CallStats::merge`) and deliberately not
  joined: a static call is evidence a path *already has* a stub, so folding it
  in would have the promotion policy recommend promoting it again.

### Fixed / 수정

- **A gate found only what it could not load.** `Workspace::load` resolves
  `<id>.sidl.idl` **or** `<id>.idl` — a documented fallback for a contract no
  annotation stage ever saw — but discovery globbed only the annotated suffix,
  so the fallback was unreachable and `--only s4` over a directory of legacy
  `.idl` found nothing. The estate's workaround was to rename, which then made
  the run's attribution line report a contract with **no annotations in it** as
  annotated, because that line asked the filename; and because `gen-corpus`
  derives its module name from the file stem, the rename put a pipeline-stage
  suffix into a generated module name (`f_ESTATE_sidl`). Three symptoms, one
  cause — a name treated as evidence about content. The third needed no fix of
  its own.
- **The fix hint mangled qualified names**, printing `qualify it with
  ``Module::::```. The cause was in the *parser*, not the printer: `scoped_name`
  gave a scoped name the span of its **first token**, so
  `::MFS::Common::StringList` sliced to `"::"`. Three sites had it; the estate
  saw one. Splitting `unknown-scoped-name` off `unknown-name` makes the advice
  right as well as the text — "qualify it with its module" is meaningless for a
  name that is already qualified.
- **`validate(&str)` was the corpus's self-contained shape written into an
  API.** Handing it a resolved unit's *text* refuses it: thirteen files' guard
  directives are still in there, and four `#ifndef` blocks in one string is
  conditional compilation rather than an include guard. `validate_unit` takes
  the unit.
- **A file-scope `#pragma prefix` leaks across a splice.** A prefix runs to the
  end of its *file*; after concatenation there is one file, so a deliberately
  prefix-less contract inherited the previous one's prefix and five repository
  ids came out **well-formed and wrong**. Nothing errors on our side — only a
  peer disagrees, and then `_is_a` fails and an `--expose` allowlist names an
  interface no object has, which reads exactly like a permissions mistake. The
  rule is one line, *each file begins with the empty prefix*, and it was
  written nowhere because until a set existed there was nowhere to write it.
- **`spikes/echo.idl` states an effect on every operation.** They are all reads
  — the interface has no state — but "obviously a read" is not something a
  bridge can see. The harness caught it; the fixture was written when silence
  was an allow.

### Known limits / 알려진 한계

- **An agent cannot read an Interface Repository through AnyJSON.** §4.5 has no
  form for `::CORBA::TypeCode`, so `corpus/services/ir_subset` loses ten items
  including `describe_interface` — and the MCP agent path speaks the same
  mapping. Recorded, not fixed: §4.5 is a specification decision, not a Python
  problem.
- **The dry-run survey is correct and hard to read.** Closing the effect gate
  took it from 7,253 bytes of uniform `allow` to 31,957 bytes with real signal,
  and the growth is *entirely* 64 verbatim copies of one `why` sentence. One
  reason per **class** rather than per row would put it near 8 KB with strictly
  more signal. A report shape, not a gate.
- **`--assume-effect` is a real widening and is meant to be.** It is one
  operator declaration covering every silence in an exposure. That is the point
  — the alternative is seventy-six approvals and a human who learns to click
  through them — but it is written into every allow that rests on it so an
  audit can find them.
- The Python target is **clients only**. A Python servant needs the bridge to
  call *back* into Python, which is a second protocol direction.
- Cross-front-end portability of the include semantics is **unmeasured** — TAO
  and JacORB were absent. An `#include` inside a module whose file sets a
  prefix is exercised by no file we have. Hard links to the same content are
  two files to the resolver.

---

## v0.3.0 — 2026-08-14

The wire-behaviour section leads again, for the same reason. Everything in it
was found by a reader we did not write, or by composing two parts that had only
ever been measured apart.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **`::CORBA::TypeCode` loaded as `void`.** The front end predeclares the name
  for checking, the registry resolves against the spec's own definitions where
  it is absent, and it fell through to `void` — silently, for months. An
  operation returning a TypeCode generated `-> ()` and **marshalled nothing at
  a peer expecting one**. `CLAUDE.md` requires that spelling, which is what
  made the gap read as support. Found by the generated-servant batch, which
  could not express `describe_interface` and said so rather than emitting an
  empty reply.
- **A refusal claimed the call had completed.** `Guarded` raised system
  exceptions with a literal `0` — `COMPLETED_YES` — so every refused caller was
  told its call had run. A separate path from the transposed enum fixed in
  v0.2.0, and now that quota refusals are `TRANSIENT`, it was an invitation to
  retry attached to "it already happened".
- **A deferred IFR operation answered "no such operation".** `ifr.rs` refused
  the ten IR operations it defers with `BAD_OPERATION`, which is byte-for-byte
  what an operation nobody thought about answers — so a client could not tell a
  decision from a gap, and `SERVICES-COVERAGE.md` could only tell them apart by
  searching this repository for a written reason. They answer **`NO_IMPLEMENT`**
  now: `NO_PERMISSION` is policy, `NO_IMPLEMENT` is deferred, `BAD_OPERATION` is
  "not an operation of the object you addressed". Affects `contents`, `lookup`,
  `lookup_name`, `describe_contents`, `describe`, `_get_defined_in`,
  `_get_containing_repository`, `get_canonical_typecode`, `get_primitive`,
  `_get_type`.
  *유예 연산은 이제 `NO_IMPLEMENT`로 답한다 — 와이어가 유예와 누락을 구분한다.*

### Added / 추가

- **`Contained::_get_version` is served.** It answered `BAD_OPERATION` while its
  write half `_set_version` answered `NO_PERMISSION` — "the operation exists and
  the answer is no" — on a version the facade parses out of every repository id
  it handles. The two were the wrong way round by `ifr.rs`'s own argument. The
  read answers now; the write is still refused. `corpus/services/ir-subset.idl`
  declares the attribute, so the generated skeleton serves it too and the
  byte-for-byte comparison in `ifr_shape.rs` covers it.
- **Constants carry their value, and are generated.** `Entry::Const` recorded
  the type and not the value, so `orbweaver-gen` skipped every constant — which
  the end-to-end run met concretely when a contract declared its authorization
  scope as a `const string` so a servant could name it, and the servant could
  not. The registry now folds the expression **once, where the names are**
  (`const long OFFSET = MAX_RETRIES * 2;` needs both arithmetic and IDL's
  outward scope resolution), coerces it to the declared type, and stores an
  evaluated `ConstValue`. An expression it cannot fold stores **no value** and
  the generator skips that item with a reason — never a guessed zero. A
  `string` constant is emitted as `&str`, since Rust has no `const String`.
  *상수는 값을 갖는다 — 레지스트리가 한 번 접고, 접을 수 없으면 값을 만들지 않는다.*

- **`CosNaming::NamingContextExt::to_url`**, measured against **two** producers:
  omniORB's own client resolved a URL ours built, and omniNames was run as a
  second producer over 14 argument pairs (11 identical, 3 differing only in
  hex-digit case, each parser reading the other's). One behaviour changed
  *because* of that comparison: an empty name returns the bare `corbaname:`
  form, as omniNames does.
- **`moe::Router::select`**, delegating to the trading engine. When a
  constraint names a field a wire-registered offer cannot answer it refuses the
  **whole call** with `NO_IMPLEMENT`: a shorter list would say *these are all
  the experts that qualify*, which is the sentence three-valued matching exists
  to prevent.
- **Constants are generated**, with the value folded once in the registry
  rather than an expression every consumer would have to fold — three folders
  that will disagree, and the one that disagrees silently ships. An expression
  that cannot be folded stores nothing rather than a guessed zero.
- **A deferral answers `NO_IMPLEMENT`, an oversight still answers
  `BAD_OPERATION`.** Ten Interface Repository operations moved, so the
  distinction `SERVICES-COVERAGE.md` opens by saying the wire cannot make is
  one the wire now makes for that service — and `BAD_OPERATION` there is zero.
- **The audit ledger is bounded** and spends one slot on an in-band `ELIDED`
  marker naming what it dropped, because a dropped hour and a quiet hour read
  identically exactly when somebody is reading the log to tell them apart.
  `verify_promotion` refuses a truncated history rather than concluding from a
  gap, and checks before parsing so the marker is not reported as a formatting
  bug.
- **A trace can name what failed**: `CallResult` carries the exception's
  repository id, so D004's `outcome` column stops saying `-` for a failure it
  simply had not been told about.
- **D006** (PROPOSED): the data-plane rule, and the finding that no option can
  enforce it — a bound constrains size, not frequency, and "never per token" is
  the operative phrase.
- **Attribute accessors are gated and visible.** `allow_interface` made
  `_get_balance` callable while the policy resolved scopes only through
  declared operations, so an `ai_authz` written on an attribute bought nothing
  and a `_set_` was never approval-gated — invisible *and* ungated, in the
  instrument built so an operator can see what an agent may reach. An
  `ai_effect` gates the setter only; a scope guards the value and so guards
  both accessors.
- **Token → `Caller` exchange** with the verifier as a trait this crate does
  not implement, and a **scope audit** that reports a contract scope no issued
  token can satisfy as an outage, naming the operations that go dark, before
  any call is made (D005's class).
- **`describe_interface` generated byte-identically** to the hand-written
  Interface Repository facade — 77 cases × 2 byte orders, with omniidl
  accepting the contract and the oracle's own decoder reading the reply back.
- **`docs/SERVICES-COVERAGE.md`** — all 107 declared operations probed over the
  wire, classified served / refused-with-a-reason / absent. **12 answered
  `BAD_OPERATION` with no reason written anywhere**; `PLAN-SERVICES` §8.1 now
  writes the sentence each was missing, including two that turned out to be
  defects and one that is recorded as undecided.
- The fuzz reaches the **text** parsers too (stringified IORs, D004 trace
  lines, the ingestion validators): 450,000 cases × 17 targets across five
  seeds, zero panics, with every target's reach asserted.
- **Dispatch runs concurrently** (`SharedDispatch`). One lock per servant, at
  most one held per thread, nothing blocking inside it — so lock *ordering* is
  vacuous rather than documented, since a deadlock cycle needs a thread holding
  two locks. Enforced structurally: a guard cannot escape its closure, a
  per-thread marker refuses a second section, and dialling asserts nothing is
  held (panicking in debug, so a violation fails the test that wrote it). The
  existing `Dispatch` trait and every generated skeleton are unchanged and
  still serialized. **The finding**: `ServerStats::peak_dispatching` was written
  to witness overlap and could not — it sits outside the servant's own lock and
  counted callers queued for it, reaching N on a *serialized* server. The
  negative control caught it; review had not.
- **Multi-object generated skeletons**: `knows()` required with no default,
  identity as an explicit `Target` argument, and **71 pinned cases answering
  byte-identically to the hand-written Interface Repository facade**, down to a
  minted reference's object key and port — with `ifr.rs` unmodified as the
  oracle.
- **IOR rewriting for NAT and containers** (R7): profiles and
  `TAG_ALTERNATE_IIOP_ADDRESS` rewritten, the object key refused as a target
  because it is identity rather than a route, an undecodable profile preserved
  byte for byte. Both real socket failures constructed. The container probe is
  written and **has never executed** — counted as a skip.
- **The quota interceptor seat**, with the window taken as a host-supplied
  label rather than from a clock, and refusals typed `TRANSIENT` when they
  renew and `NO_PERMISSION` when they do not.
- **The audit ledger leaves the process** in serving mode; it used to be
  drained only under `--dry-run`.
- **`#pragma prefix`/`version`/`ID`** and an S4 report for an explicit ID that
  is not a repository id.
- **D005** (PROPOSED): contract stability, and the argument that semantic drift
  is worse than identifier drift.

### Fixed / 수정

- **An unknown latency is not a fast one.** An offer registered over the wire
  carried `latency_p50: 0.0`, which did not merely fail to match
  `latency_p50 < 20` — it **matched**, so a latency-ordered router preferred
  exactly the experts nobody had measured. Matching is now three-valued and
  unknown sorts after every known value.
- The generator inverted a repository id into a Rust module path, emitting
  `pub mod acme.com` for prefixed IDL.
- **`_get_version` answered "no such operation" while `_set_version` answered
  "you may not"** — backwards by the servant's own argument, on data the
  registry parses out of every repository id it holds.
- **Annotations on types were never checked.** The contract checker walked
  interfaces only, so a typo on a `typedef` was silent — contradicting its own
  premise that an `ai_*` key nobody reads is what it reports.
- **A bound change is no longer reported as the silent class.** `idl-diff`
  described `sequence<octet>` becoming `sequence<octet, 64>` as "the encoded
  form differs, and CDR gives a receiver no way to notice". Both halves are
  false: the bytes are identical, and a peer that exceeds the bound is refused
  loudly. The verdict stays Breaking; the reason now says which kind of
  breaking, because the sentence being borrowed describes §5.3's measured case
  of a peer returning the wrong member and raising nothing.
- The `*.log` ignore swallowed the end-to-end provenance record, which only a
  fresh clone could reveal — the third time a blanket ignore has taken a
  committed artefact.

---

## v0.2.0 — 2026-08-14

Phase 4 substantially landed; five CORBA services served on our own POA; the
specification pipeline split into stages that can be measured one at a time.

### ⚠ Wire behaviour changed / 와이어 동작 변경

Four defects, in three groups, that change the bytes we put on the wire or
what we accept from a peer. Two of the three groups were found by a reader we
did not write — a foreign ORB, and the specification itself where no peer
could serve as one.

- **`completion_status` was transposed.** `COMPLETED_YES` is ordinal 0 per
  §4.11.4 (confirmed against omniORB before changing anything); our
  `Completion` had `No = 0, Yes = 1`. **A servant reporting "the operation did
  not run" reached every foreign ORB as "it ran"** — so a call refused before
  it started looked like a mutation that had happened, and a client that could
  have safely re-sent concluded it must not. Every servant uses the symbolic
  names, so the fix corrects the naming, event, IFR, expert and tenant services
  at once. `MAYBE` was 2 either way, which is why only two of the three were
  wrong and why nothing local caught it: our own client compared against the
  same enum and agreed with itself, including the test that asserted the
  encoded byte equalled `Completion::No as u32` and therefore moved with the
  bug. It now asserts the literal ordinal.
  **재시도 안전성을 결정하는 두 값이 뒤바뀌어 있었다.** 우리 클라이언트는 같은
  enum으로 비교하므로 스스로와는 늘 일치했고, 외부 ORB만이 이견을 낼 수 있었다.

- **Recursive types could not be marshalled at all.** Every non-empty recursive
  value was refused with "expected a value of type an indirection", and nothing
  noticed because the only generator that could have produced such a value was
  the one reporting the arm as unmeasured. Markers now resolve against the
  enclosing type the error path is already standing on; nesting is bounded at
  64 in both directions, because on decode the depth is the sender's choice.

- **Two fragment-reception defects**, found against hand-built §9.4.9 streams
  since no available peer emits fragments: a stray leading `Fragment` was
  returned as a message, and a fragment at a different GIOP version was
  accepted as a continuation (in 1.1 the bytes read as a request id are body,
  so a match would have been coincidence).

### Added / 추가

- **The end-to-end path, measured as one path** (`spikes/end_to_end.sh`, in the
  harness): a fresh requirement → S1–S5 → both generated halves → a servant on
  our POA → an agent-shaped caller through the guard, with a scope refusal
  visible in the transcript. **185 hand-written product lines against 778
  generated.** Composing it produced the release's most useful finding — see
  *Known limits*.
- **`#pragma prefix` / `version` / `ID`** — repository ids now match omniidl on
  a 25-id corpus, prefixes and all. Before this, every legacy IDL file (the OMG
  recommends a reverse-DNS prefix) would have given us a different identity for
  every type than the peer had, while looking correct locally.
- **`orbweaver-console`** — catalog, contract diff and D004 traces as
  self-contained HTML, no web framework and no template engine.
- **D004 tier 1 telemetry** — one span record per decision, no clock, and a
  credential structurally unable to reach a line.
- **S3i** — annotations inferred for ingested contracts, which never occupy a
  key a gate reads until a human approves them.
- **CORBA services on our POA**: CosNaming server, CosEvent push channel
  (bounded queue, dead consumers disconnected with **drops counted**),
  Trading wire surface for the MoE control plane, a read-only Interface
  Repository facade, and LifeCycle/tenancy with the tenant in every object key.
- **Remote IFR ingestion** — JacORB 3.9's Interface Repository served us. A
  contract can now be taken off the wire with no IDL file, with provenance
  marked and **contagious upwards**.
- **Server skeletons** (`orbweaver-gen`), driven by omniORB's own python
  client, with a servant fault surface whose `#[must_use] Raising` cannot
  become a `SystemException` without naming the completion status.
- **Server-side static-equals-dynamic oracle** — 204 reply-byte comparisons,
  three GIOP versions × two byte orders × two reply origins.
- **Concurrent connections** — cap 64, refusal spoken as §9.4.7's
  `CloseConnection`. Dispatch remains serialized and the documentation says so.
- **The guard's interceptor chain** (F4) and **dry-run**: an exposure can be
  read before it is deployed, audited under its own `DRYRUN-` token, and unable
  to diverge from the live gate by construction.
- **S1–S3 as distinct pipeline stages**, each a producer plus the gate that
  judges it, each runnable alone.
- **Property and contract testing** (`orbweaver-test`), including a wire fuzz
  measuring panic-freedom over the decoders a peer reaches before any policy
  runs: 0 panics in 50,000 cases × 10 targets.
- **Vector search** behind `search_interfaces` via an external command (D003-A)
  — no new dependency. The synonym class remains **UNMEASURED** without a key.
- Decisions **D003** (approved) and **D004** (approved): both adopted zero
  Cargo dependencies.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
  [`docs/PLAN-SERVICES.md`](docs/PLAN-SERVICES.md),
  [`docs/PLAN-DEFERRED.md`](docs/PLAN-DEFERRED.md),
  [`docs/PLAN-MOE.md`](docs/PLAN-MOE.md).

### Fixed / 수정

- The generator bound its encoder closure as `e`, so an IDL parameter named `e`
  shadowed it into `e.put(e)`. Generated locals are now `__`-prefixed and the
  corpus keeps a parameter named `e` on purpose.
- `--expose IDL:spike/Echo:1.0` split at the version's dot, so the
  bare-interface form allowlisted an interface nobody had. Found by dry-run on
  its first run.
- The registry's union `default_index` was computed against the unexpanded case
  list, and **the existing test asserted the buggy semantics**.
- `Poa::new` took an incarnation from a freed `Box` address, which the
  allocator then reused, so two POAs could share one.
- The offer store could lag the residency machine, so under memory pressure the
  loading policy returned an empty decision list — silently.

### Changed / 변경

- The harness takes a **machine-wide lock** and kills fixtures by process
  group. Two concurrent runs used to destroy each other's peers and report
  failures that were about the scheduling; that cost two diagnoses.
- `PLAN.md` / `PLAN.ko.md` at **v0.7**; the streams are scope and the status
  lives in `COMPONENTS.md`, which is refreshed after every wave.

### Known limits / 알려진 한계

Stated because an absence that is not written down reads as a feature:

- Nothing in the token exchange has been through a **real identity provider**;
  it is unit-tested against hand-built claims — the same shape as CSIv2 being a
  per-peer claim rather than a feature.
- **No rewritten IOR has been put in front of a foreign ORB**, and port
  translation has never been dialled. R7 is now measured across a real routing
  boundary (a second host), but both ends of that measurement were ours.
- **Multiplexing is refused below GIOP 1.2**, deliberately: a 1.1 `Fragment`
  carries no request id, so a fragmented 1.1 reply is attributable only by
  position. Against a stock omniORB that is a live limit, not a theoretical
  one — it fragments a 1 MB reply at 1.1, and we refuse it.
- A `CloseConnection` arriving **between fragments** surfaces as
  `UnexpectedMessage` rather than as a retryable close.
- Ingested contracts carry no SIDL, so the guard's gates have nothing to key
  on — a second, independent reason exposure stays off.
- The embedding synonym class, the TAO oracle column and the SSLIOP peer proof
  are **unmeasured**, each for a stated reason, and the harness counts them as
  skips rather than passes.
- **The pipeline is not reproducible across runs, and nothing catches it.**
  Re-running S1–S3 on the same requirement with the same prompts passed every
  gate 1/1 again and produced a different contract: different module and
  operation names, a different parameter type, and an authorization scope that
  drifted from the one the requirement literally states. An identity provider
  issuing the stated scope against such a contract refuses every legitimate
  caller. Recorded in `docs/pipeline-runs/2026-08-14-end-to-end.md`; the fix
  needs a decision about what S2 may choose, so it is named rather than
  patched.

---

## v0.1.0 — 2026-08-13

Phases 0–3.5. A from-scratch MIT ORB interoperating with omniORB 4.3.4 and
JacORB 3.9 in both directions at GIOP 1.0/1.1/1.2; IDL 4.2 front end and type
registry in full oracle agreement; POA and object model; dynamic invocation and
AnyJSON; the MCP triad over stdio with default-deny exposure and capability
handles; the S4 validation gate; CSIv2 wire and delegation policy.
