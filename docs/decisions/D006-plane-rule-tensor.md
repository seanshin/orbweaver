# D006 — The plane rule: what a `Tensor` may carry, and whether any check can tell

**STATUS: APPROVED** — drafted 2026-08-14 from the finding written up in
[`PLAN-MOE.md`](../PLAN-MOE.md) §4.6 and cross-referenced from
[`PLAN-SERVICES.md`](../PLAN-SERVICES.md) §8.1, approved the same day by the
user ("승인하고 순서대로 진행"), with the recommendation adopted as written:

- **Option E is adopted.** `Expert::process` and `Router::dispatch` are
  **excluded**, with the reason recorded where `PLAN-SERVICES` §8.1 asks for it.
  Nothing serves them, nothing plans to, and paying a breaking change to bound
  an operation that has never run means writing an unmeasured constant into a
  wire contract to govern traffic nobody has sent.
- **Option A is the return path, not a rejection.** If a consumer ever needs an
  activation-shaped call, it returns as a **new versioned interface** carrying a
  bounded handle type — where §5.3 already prescribes a version bump, so the
  bound costs nothing extra. That path is conditional on generated code
  enforcing bounds, which this decision measured as missing.
- **B and C are premature** by `contract.rs`'s own rule for adding a rule: the
  only action either can offer is reporting that somebody made a claim no check
  can test.

Approval does not make the rule enforceable, and the document says why: a bound
constrains **size, not frequency**, and §3's operative phrase is *never per
token* — a 16-byte handle called once per token is the data plane at full rate,
passing every check any option installs. The one time this project actually held
a "never per token" rule it did so by **removing the API**, held by a
`compile_fail` test. Exclusion is that mechanism; a bound is not.

**상태: 승인됨** — 2026-08-14 승인. E 채택: `process`/`dispatch`를 명시적으로
제외한다. A는 기각이 아니라 **복귀 경로**이며, 활성화 형태의 호출이 필요해지면 새
버전 인터페이스로 돌아온다. 승인해도 규칙이 검사 가능해지지는 않는다 — 상한은
크기를 제약하지 **빈도**를 보지 못하고, "토큰당 금지"를 실제로 지킨 유일한 방법은
API를 없애고 `compile_fail`로 잠근 것이었다.

This is a decision and not a batch because every mechanism that would make the
rule checkable **constrains what a deployment may put in a `Tensor`** — and the
cheapest of them is refused by our own `idl-diff` as a breaking change to a
landed contract. That is a policy question about a published type, which §5.3
says is not settled by whoever commits first.

이것이 배치가 아니라 결정인 이유는, 규칙을 검사 가능하게 만드는 모든 방법이
**배포자가 `Tensor`에 무엇을 넣어도 되는가**를 제약하고, 그중 가장 싼 것조차
우리 자신의 `idl-diff`가 착지된 계약에 대한 파괴적 변경으로 거부하기 때문이다.
공개된 타입에 관한 방침 문제는 먼저 커밋한 사람이 정하지 않는다(§5.3).

---

## What was measured here, and what is quoted / 이 문서가 실측한 것

Everything in this section was re-run in this worktree rather than taken from
§4.6. The commands and their exact output:

**1. Bounding `Tensor` is BREAKING — reproduced.** `sequence<octet>` →
`sequence<octet, 64>` in a scratch copy of `corpus/golden/22`:

```
$ cargo run -q --bin idl-diff -- corpus/golden/22-moe-control-plane.idl 22-bounded.idl
[BREAKING] IDL:moe/Tensor:1.0: bound changed from unbounded to 64 — the bytes are
           unchanged; what changes is what a conformant peer may send or accept,
           so the failure is a refusal rather than a silent misread
refused: 1 change(s) break deployed peers                              (exit 1)
```

Two details worth having, because they change how loud "BREAKING" is here:

- **It is the bound arm, not the catch-all.** `diff.rs:426-452` gives a bound
  change its own verdict and its own sentence, and `diff.rs:504`
  (`a_bound_change_is_breaking_but_not_silent`) pins that the message must
  contain "refusal" and must **not** contain "no way to notice". So this is the
  loud kind: a peer that exceeds the bound is refused, not misread. §5.3's
  measured horror — a peer returning the wrong member and raising nothing — is a
  different class and the differ now says so.
- **One change, at the alias, not one per use site.** `same_identity`
  (`diff.rs:253`, with the reasoning at `:242-252`) compares a named type by its
  repository id, so `Activation`, `GateSignal`, `process`, `dispatch` and
  `select` all stay quiet and only `IDL:moe/Tensor:1.0` reports. Correct, and
  worth knowing before reading the report: one line understates the five
  positions the change actually reaches.

**2. Removing the two operations is also BREAKING — reproduced.**

```
$ cargo run -q --bin idl-diff -- corpus/golden/22-moe-control-plane.idl 22-dropped.idl
[BREAKING] IDL:moe/Expert:1.0: operation "process" removed — a caller that invokes it receives BAD_OPERATION
[BREAKING] IDL:moe/Router:1.0: operation "dispatch" removed — a caller that invokes it receives BAD_OPERATION
refused: 2 change(s) break deployed peers                              (exit 1)
```

So the "give up the rule" option is **not** the free one. Both directions out of
today's state cost a version bump or a recorded `--approve`.

**3. An annotation on the typedef is invisible to `contract-check` — measured,
and this one was a surprise.** A scratch copy with `//@ ai_handle: true` on
`typedef sequence<octet> Tensor;`:

```
$ cargo run -q --bin contract-check -- 22-annotated.idl
1 file(s), 9 type(s) × 32 case(s) × 2 byte orders: 0 property defect(s), 0 contract finding(s)
```

Zero findings — including no `contract/unknown-annotation`, which is the rule
whose entire job is to say "you wrote an `ai_*` key nobody reads"
(`contract.rs:668-684`). The key **does** reach the registry:
`register_annotations` is called for type definitions
(`registry/src/lib.rs:647`) and `annotations_of` explicitly matches
`Definition::Typedef` (`:922`). But `contract::check` iterates only
`Entry::Interface` (`contract.rs:205`), so no rule in the checker ever looks at a
type's annotations. A typo — `ai_handel` — on a typedef is silent today.

**4. A foreign compiler accepts the bounded form.** `omniidl -b dump` on the
bounded copy exits 0 and echoes `typedef sequence<octet, 64> Tensor;`. The
conformance oracle has no objection to the syntax; it has no opinion on the rule.

**측정 요약.** `Tensor`에 상한을 거는 것은 BREAKING이지만 **조용한 부류가 아니라
거절**이며(전용 판정과 전용 문장, `diff.rs:426`), 별칭 한 곳에서 한 줄로만
보고된다. 두 연산을 **지우는 것도** BREAKING이다 — 나가는 두 방향 모두 값을
치른다. 그리고 typedef에 붙인 `//@ ai_handle`은 레지스트리에는 들어가지만
`contract-check`가 인터페이스만 순회하므로 **오탈자 검사조차 발화하지 않는다.**

---

## The question / 문제

`PLAN-MOE` §3 states the rule that defines the whole stream: **the data plane
stays out of CORBA permanently.** Can that be written as a predicate over a
contract — something a program can evaluate to *accept* or *refuse* — and if it
can only be approximated, which approximation, at what price?

§3의 철칙 — **데이터 플레인은 영구히 CORBA 밖에 있다** — 을 계약에 대한 술어로,
즉 프로그램이 수락/거절로 평가할 수 있는 형태로 쓸 수 있는가? 근사만 가능하다면
어느 근사를, 어떤 값을 치르고 택할 것인가?

---

## 1. Stating the rule precisely enough to be checkable / 규칙을 술어로

### P3 — the rule as meant, and why no predicate expresses it

*For every value crossing a control-plane operation, the bytes are a reference
to data rather than the data.*

**This is not a predicate over a contract, and no amount of tooling makes it
one.** A contract declares types. Sixty-four octets are the same declaration
whether they spell a POSIX shared-memory name or sixteen `float32` activations,
and the difference lives in the *referent* — which is not in the contract, not on
the wire, and not in any artifact a gate can read. Any check we build evaluates a
property of the declaration and then we *interpret* it as evidence about the
referent. That interpretive step is where every option below leaks.

**P3는 계약에 대한 술어가 아니며, 도구를 아무리 붙여도 술어가 되지 않는다.**
64바이트는 공유메모리 이름이든 `float32` 열여섯 개든 똑같은 선언이고, 차이는
*지시 대상*에 있는데 지시 대상은 계약에도 와이어에도 없다. 어떤 검사든 선언의
성질을 재고 그것을 지시 대상에 관한 증거로 *해석*할 뿐이며, 아래 모든 대안이
새는 곳이 바로 그 해석 단계다.

### P2 — the size reading, which is checkable and refuses the wrong things

*For every operation of an interface marked control-plane, every `sequence` and
`string` transitively reachable from its parameters, return type and raised
exceptions carries a declared bound, and the maximum encoded size is ≤ N.*

Checkable in principle — `Registry` already holds every `TypeCode` — but a
maximum-encoded-size function is **not total** over the constructs this very
contract uses. Four have no finite maximum by construction: `TypeCode::ObjRef`
(an IOR is a string plus profiles), `TypeCode::Any`, `TypeCode::Recursive`, and
plain unbounded `string`, which `corpus/golden/22` uses seven times (`dtype`,
`shape`, `request_id`, `trace_id`, `placement_node`, `contract_version`, and
`CapabilityId` itself).

The consequence is decisive: **P2 refuses `Router::select`**, whose return type
is `ExpertSeq` = `sequence<Expert>` — an unbounded sequence of unbounded object
references. §4.6 calls `select` "pure control plane" and its absence "a gap
rather than a decision". A predicate that refuses the one operation the plan
calls unambiguously legal is not a formalization of the plan; it is a different
rule wearing the plan's name.

### P2′ — the narrowed form, which is the only one actually proposable

*For every operation of a control-plane interface, every `sequence<octet>`
transitively reachable from its signature has a nonzero bound ≤ N.*

Total, implementable against today's `Registry` with no new data, and it fires on
exactly what this document is about. Its blast radius in the landed contract,
counted by hand:

| position | reached via |
|---|---|
| `Expert::process` parameter `x` | `Activation.data` |
| `Expert::process` return | `Activation.data` |
| `Router::dispatch` parameter `x` | `Activation.data` |
| `Router::dispatch` return | `Activation.data` |
| **`Router::select` parameter `g`** | **`GateSignal.affinity`** |

**That last row corrects §4.6, and the correction matters.** §4.6 splits the
three operations as "`select` returns `ExpertSeq` — references, nothing else"
against "`dispatch` and `process` are the ones that would carry an
`Activation`". True of `select`'s *return* and false of its *parameter*:
`GateSignal { Tensor affinity; unsigned short top_k; }`
(`corpus/golden/22-moe-control-plane.idl:33`) is a parameter of
`select(in GateSignal g, in Constraints qos)` (`:55`), and an affinity vector is
exactly the shape of an activation — it is the gate's routing logits. So the
honest split is not two operations against one. **All three touch a `Tensor`;
`select` touches it in one direction instead of two.** The operation §4.6 files
as a pure-control-plane gap is the one whose data-plane exposure nobody noticed,
which is a small illustration of why a rule kept in prose gets applied unevenly.

**§4.6에 대한 정정.** `select`는 "참조만 돌려준다"가 맞지만 그것은 반환값 이야기
이고, 파라미터 `GateSignal`은 `Tensor affinity`를 나른다 — 게이트의 라우팅
로짓이며 형태상 활성값 그 자체다. 따라서 정직한 분할은 2 대 1이 아니다. **세
연산 모두 `Tensor`에 닿으며, `select`는 양방향이 아니라 한 방향으로 닿는다.**
§4.6이 순수 컨트롤 플레인의 *공백*으로 분류한 연산이 바로 그 노출을 아무도
알아채지 못한 연산이라는 사실이, 산문으로만 있는 규칙이 왜 고르지 않게 적용되는
지를 그대로 보여준다.

**What P2′ cannot see, stated before it is recommended by anybody:**
`sequence<float>`, `sequence<double>`, `sequence<long>`, an unbounded `string`
carrying base64, and `Any`. Every one of them moves an activation as well as an
octet sequence does, and the last two are in the vocabulary of every contract we
will ever write. P2′ is a rule about **one spelling** of the data plane, not
about the data plane. An author who wants a megabyte across and meets P2′ needs
one edit — `typedef sequence<float> Tensor;` — and the check goes quiet.

---

## 2. Why nothing catches it today / 오늘 아무것도 잡지 못하는 이유

Verified by running the gates, not by assuming, because "no gate catches it" is
the class of claim that is embarrassing to assert and cheap to check.

- **`omniidl`** accepts both forms (measurement 4). It is a syntax and
  type-system oracle and has no concept of a plane.
- **`contract-check`** reports 0 findings on the landed file and 0 on the
  annotated variant (measurement 3). It visits `Entry::Interface` only
  (`contract.rs:205`), so it cannot read a property of a *type*, which is where
  this property lives.
- **`idl-diff`** compares two contracts. It has nothing to say about one, and it
  is the tool that *refuses* the fix rather than one that could apply it.
- **The marshaller — and this is the one that must be corrected, because §4.6's
  option sketch calls the enforcement free.** It is free on one of the project's
  two paths and absent on the other:
  - **Dynamic path: enforced, both directions.** `check_bound`
    (`dynamic/src/lib.rs:594`) is called on encode (`:539`) and on decode
    (`:718`), with `bounds_are_enforced_in_both_directions` (`:988`) pinning it.
  - **Static generated path: the bound is dropped.** `rust_type`
    (`gen/src/lib.rs:164`) maps `TypeCode::Sequence { element, .. }` to
    `Vec<{element}>` — the bound is discarded at the pattern — and
    `impl<T: Cdr> Cdr for Vec<T>` (`gen/src/rt.rs:526`) writes `self.len()` with
    no check at all. Its decode side calls `validate_count`, which bounds the
    length against **the bytes actually present**, not against a declared bound:
    it is the allocation-safety measure from the Phase 0 audit, and it is not
    this.

  So a generated Rust stub for a `sequence<octet, 64>` will happily send a
  megabyte. **The enforcement is missing on exactly the path a latency-sensitive
  deployment would choose**, which is the path a person who wants to inline an
  activation is already on.

**마샬러가 "공짜로" 강제한다는 서술은 절반만 참이다.** 동적 경로는 인코딩·디코딩
양방향에서 상한을 강제하지만(`check_bound`), 정적 생성 경로는 `rust_type`에서
상한을 **버리고** `Vec<T>`의 인코더에 길이 검사가 아예 없다. 즉 지연에 민감한
배포가 고를 바로 그 경로에서 강제가 사라진다 — 활성값을 인라인하고 싶은 사람이
이미 서 있는 경로다.

The rule therefore lives in exactly two places: one sentence in `PLAN-MOE` §3 and
the trailing comment on `corpus/golden/22-moe-control-plane.idl:13`
(`// reference-carrying; never inlined`). Neither binds anything. This is the
same shape D005 found for the authorization scope — *the token is not lost
through carelessness; there is structurally nowhere for it to go* — and it is
worth naming the repeat, because two instances of one shape is a pattern about
this project's artifacts rather than two unlucky files.

---

## Options considered / 검토한 대안

Each with what it costs and what it forbids, because an option stated without its
cost is advocacy. Every one of them is measured against `contract.rs`'s rule for
adding a rule (`contract.rs:12-16`): **name the consumer that will act on it and
what that consumer will do.**

### A — bound the type (`typedef sequence<octet, N> Tensor;`)

**Consumer and action:** `orbweaver-dynamic`'s marshaller, which refuses an
over-long value on encode and on decode (`check_bound`). Named, real, tested.

**What it costs.** A BREAKING verdict on a landed contract (measurement 1), so
§5.3 requires a new version of the type or a recorded `--approve`. And a second
cost §4.6 did not price: **the consumer only covers half the project.** The
static generated path drops the bound entirely (§2 above), so adopting A without
also fixing `gen` buys enforcement for dynamic callers and a documented number for
everyone else. That fix is small — carry the bound into the generated codec — but
it is real work that A silently assumes.

**What it forbids.** It puts a number in a published contract. A future
accelerator whose handle scheme does not fit — a URI to a segment plus an offset
plus a generation counter is easily past 64 bytes — outgrows it, and *changing*
the number is BREAKING again in both directions: `loosening_a_bound_is_breaking_for_the_receiver`
(`diff.rs:520`) pins that relaxing is as breaking as tightening, for the other
party. So A is not one breaking change; it is one now and one per accelerator
generation.

**And the number has no measurement behind it.** Nothing in this repository has
ever produced a handle. The `64` in §4.6's sketch is a plausible-looking figure
somebody typed while writing the sketch, not a measured size, and adopting it
would make an unmeasured constant a wire contract.

### B — a SIDL annotation (`//@ ai_handle: true` on the octet sequence)

**Consumer and action — and this is where B is weakest.** The rule for adding a
rule wants a consumer that *acts*. The honest candidates:
`contract-check` (reports a finding), and the MCP exposure gate (refuses to
expose an operation whose reachable octet sequence carries no such claim). Both
are real actions. But both act on **the presence of the claim, never on its
truth** — the annotation is an assertion by the author that the bytes are a
handle, and nothing anywhere tests the assertion. §2.2's whole argument is that
an annotation is an input to a decision; here it would be an input to a decision
about whether somebody *said* the right thing.

**What it costs.** Three things, not one.
1. **A ninth key in `VOCABULARY`** (`contract.rs:120`, eight keys today),
   governed by `PLAN.md` §2.2. Every key added is a key the guard, the MCP face
   and the generated tests may have to reason about.
2. **Plumbing, because the natural home is the blind spot** (measurement 3). The
   claim belongs on the typedef — that is where the existing comment is and where
   it is written once rather than at each of the five positions. A typedef
   annotation reaches the registry and is read by nobody, and the checker cannot
   even flag a typo in it. So B is one rule *plus* teaching `contract::check` to
   visit `Entry::Type`. Small; not zero; and exactly the "there is nowhere for it
   to go" cost D005 charged option C.
3. **The alternative home is worse.** Parameters *are* visited
   (`contract.rs:472`), so `//@ ai_handle` on each parameter is checkable today —
   at the price of restating a property of the type at five positions that must
   agree, which is a synchronization problem invented to avoid a plumbing job.

**What it forbids.** Nothing. It is a claim, and it forbids only silence.

### C — a naming or typing convention (`typedef sequence<octet, 64> TensorHandle;`)

**Consumer and action:** whichever checker is taught the name. Cheap to write.

**What it costs, and why it is the weakest of the five.** It needs the same
plumbing as B (the checker must visit `Entry::Type`) and buys less, because a
name is outside any governed vocabulary and will therefore be spelled three ways
within a year — `TensorHandle`, `TensorRef`, `HandleSeq` — each of which is
invisible to a checker that knows one of them. Conventions decay, and a decayed
convention is worse than an absent one because the checker stays green while the
meaning drains out of it. This project has already measured that documenting a
rule does not enforce it: the case-insensitive clash rule caught "two corpus files
and two fixtures, one written by someone who had just described the rule in that
same file's header" (`CLAUDE.md`).

**What it forbids.** Nothing, and it does not even make a claim — it makes a
spelling.

### D — do nothing, and rely on review

Stated concretely, because "do nothing" is an option only when its cost is on the
page.

The rule is currently invisible to **every gate this project has**: `omniidl`,
`sidl-validate`, `contract-check`, `idl-diff`, the property/fuzz suite, the S1–S4
pipeline gates. All measured or read above; none has a concept of a plane. The
rule's total representation is one sentence in a plan and one trailing comment on
one corpus line.

There is a specific, near-term path by which it gets broken. F2's wire surface
already serves `moe::ExpertRegistry` and `moe::ExpertLoader` **out of this very
contract** (`PLAN-MOE` §3, landed 2026-08-15). The first person who wants
`process` served finds it already declared, already in a golden file, already
compiled by three front ends, with nothing between them and a megabyte. They will
not be defying a rule; they will not encounter one. D is the option that chooses
to let the rule be discovered in an incident rather than in a document, and it is
the one option whose cost is paid entirely by someone who was not in this
conversation.

**D의 정직한 요약: 이 규칙은 이 프로젝트의 모든 게이트에 보이지 않는다.** 표현
전체가 계획서 문장 하나와 코퍼스 주석 한 줄이다. 그리고 F2가 **바로 이 계약에서**
`ExpertRegistry`/`ExpertLoader`를 이미 서빙하므로, `process`를 서빙하고 싶은
첫 사람은 이미 선언되어 있고 이미 골든이며 이미 세 프런트엔드가 통과시킨 연산을
발견할 뿐이다. 그는 규칙을 어기는 것이 아니라 규칙을 만나지 않는다.

### E — give up the rule for these two operations and declare them excluded

**Shape:** `Expert::process` and `Router::dispatch` are declared out of scope for
the CORBA control plane, with the reason written where §8.1 asks for it. The
activation crosses by whatever the data plane is; the ORB never sees it.

**What it costs:** measured, not assumed — removing them is BREAKING
(measurement 2), so the concrete act is a **new version of `Expert` and `Router`
without them**, which is §5.3's prescribed path for evolution, rather than an
in-place deletion. Until that version exists the declarations stay, unserved, and
the honest interim state is a `BAD_OPERATION` **with a written reason**, which is
precisely what §8.1 asks of the other eleven absences.

**What it forbids:** it forbids the design in which a router hands an activation
to an expert over an ORB. That is a real loss only if something needs it, and
nothing in this repository does — there is no accelerator, no fused kernel, no
RDMA (`PLAN-MOE` §5), and the source architecture's own rule is that payloads
cross as references.

**Its virtue:** it is the only option that does not require a rule nobody can
check. The other four all end at "and then somebody must not lie about what is in
the bytes."

---

## Recommendation / 권고

**Adopt E now; name A as the shape of the return path; reject B and C as
premature; do not arrive at D by omission.**

1. **E is the change to make, and it is cheap precisely because nothing needs
   the operations.** Nothing in the tree serves `process` or `dispatch`, nothing
   plans to, and the design they belong to does not exist here. Paying a breaking
   change (option A) to constrain an operation that has never run means adopting
   an unmeasured constant into a wire contract to govern traffic nobody has sent.
   **A bound that nothing serves is a number nobody has tested.**
2. **A is the return path, not the rejected option.** If a consumer ever needs an
   activation-shaped call over the ORB, it comes back as a **new versioned
   interface** with a bounded `TensorHandle` — and at that point the bound costs
   nothing extra, because a new version is not an in-place edit and §5.3 already
   prescribes it. Landing A today pays that price early and separately; landing it
   with the version pays it once. **The condition on that return is that the
   `gen` gap is closed first** — a bound the static path drops is documentation
   with a number in it.
3. **B and C are premature, by their own governing rule.** `contract.rs`'s rule
   demands a consumer that acts; the only action either can offer is "report that
   somebody made a claim". Extending §2.2's vocabulary to record an assertion no
   check can test is how a checker fills up with style opinions one plausible
   rule at a time — which is the failure `contract.rs:18-95` documents itself
   resisting, twice.
4. **D is refused explicitly**, so that "nothing changed" is a decision with a
   name on it rather than the residue of a document nobody finished reading.

**권고: 지금은 E. A는 기각이 아니라 복귀 경로. B·C는 시기상조. D를 방치로
선택하지 말 것.** E인 이유는 아무도 그 두 연산을 필요로 하지 않기 때문이며,
한 번도 실행된 적 없는 연산을 제약하려고 파괴적 변경을 치르는 것은 아무도 보낸
적 없는 트래픽을 규율하려고 측정되지 않은 상수를 와이어 계약에 넣는 일이다.
**아무도 서빙하지 않는 상한은 아무도 시험해 보지 않은 숫자다.** 활성값을 나르는
호출이 정말 필요해지는 날, 그것은 `TensorHandle`을 가진 **새 버전 인터페이스**로
돌아오며 — 그때 상한은 추가 비용이 아니다. 단, 그 복귀의 조건은 `gen`의 구멍을
먼저 막는 것이다: 정적 경로가 버리는 상한은 숫자가 적힌 문서일 뿐이다.

---

## What measurement would confirm or refute this / 무엇이 이 권고를 판정하는가

The recommendation rests on one falsifiable claim: **no consumer needs an
activation to cross an ORB boundary, so the cheapest correct move is to stop
declaring that it may.** Three measurements settle it, in increasing cost.

1. **The consumer census, and it is cheap.** Enumerate every call site in this
   repository and every interaction in `docs/CORBAMoEArchitecture.md` that
   requires an activation — not a handle — to cross a process boundary through
   the ORB. **If the count is zero, E is right and A was premature.** If it is
   nonzero, E is wrong today and the recommendation inverts: A lands with the
   bound the census's largest case needs. This can be run now, by reading, and it
   should be run before this document is approved.
2. **The handle size, which decides A's number and currently has no measurement
   at all.** When the data-plane simulator §5 makes performance measurable,
   record the encoded size of the handle the simulator actually needs — the
   segment name, the offset, the length, the generation counter. **If the p99
   handle exceeds 64 bytes, the `64` in §4.6's sketch was wrong**, and choosing it
   today would have bought a breaking change followed by a second one. That
   outcome confirms E's ordering directly.
3. **The loudness claim, which this document quotes and did not verify.**
   `diff.rs`'s sentence — a bound change fails as a *refusal* rather than a
   silent misread — is verified for our dynamic marshaller by
   `bounds_are_enforced_in_both_directions` and is **unverified against a foreign
   peer.** Bound `Tensor` at 64 in a fixture, serve it over the dynamic path, and
   have an omniORB client send 65 octets. A `MARSHAL`-class exception confirms
   the claim; a truncation or a silent accept refutes it, and would move a bound
   change out of the loud class and into §5.3's silent one — which would weaken
   A considerably and strengthen E.

An honesty condition on all three: measurements 1 and 2 concern a design that
does not exist yet, so both are forecasts until the simulator does. Reporting
either as a property of "the MoE control plane" before then would be reporting an
intention as a measurement.

**판정 측정.** (1) 활성값이 ORB 경계를 넘어야 하는 소비자가 하나라도 있는가 —
0이면 E가 옳고, 1 이상이면 권고는 뒤집힌다. 지금 읽기만으로 가능하며 승인 전에
해야 한다. (2) 실제 핸들 크기 — p99가 64를 넘으면 §4.6의 `64`는 틀린 숫자였고,
오늘 그것을 채택했다면 파괴적 변경을 두 번 치렀을 것이다. (3) "시끄러운 실패"
주장은 우리 동적 마샬러에 대해서만 검증되어 있고 **외부 피어에 대해서는 미검증**
이다 — omniORB 클라이언트가 65바이트를 보낼 때 `MARSHAL`이 뜨는지 봐야 한다.

---

## What none of the options fix / 어떤 대안도 고치지 못하는 것

**1. A bound constrains size; the rule is about meaning.** Sixty-four octets are
sixteen `float32` values. A handle-shaped payload can be a tiny tensor, and
routing a small expert's activation through a 64-byte window is the data plane in
CORBA at full compliance. Nothing proposed here — bound, annotation, convention,
or exclusion of two named operations — distinguishes a handle from a small
activation, because that distinction is not in the contract (§1, P3).

**2. Nothing here can see frequency, and frequency is at least half the rule.**
§3's operative phrase, repeated across F3 and F7, is **"never per token"**. A
16-byte bounded handle invoked once per token *is* the data plane — at full rate,
inside CORBA, passing every check in every option above. Frequency is not a
property of a contract at all; it is a property of a deployment. The only
instrument this project has that could see it is F4's telemetry counting calls
per operation, and that is an observation after the fact, not a gate. **The rule
that defines stream F is, in its most important half, permanently outside the
reach of any contract-level check.** F3 is the one place the project actually
solved this, and it did not solve it with a check: it removed the API — "no
token-period transitions **by construction** (the API simply has no per-call
hook)", held by a `compile_fail` doc test. That is the only enforcement mechanism
on record here that worked, and it is E's mechanism, not A's.

**어떤 대안도 빈도를 보지 못하며, 빈도가 규칙의 절반 이상이다.** §3의 실질
문구는 **"토큰마다는 절대 안 된다"**이다. 16바이트 상한 핸들을 토큰마다 호출하면
그것이 곧 데이터 플레인이며 — 최대 속도로, CORBA 안에서, 위 모든 대안의 모든
검사를 통과하면서. 빈도는 계약의 성질이 아니라 배포의 성질이다. 이 프로젝트가
이 문제를 실제로 푼 유일한 사례는 F3인데, 검사로 푼 것이 아니라 **API를
없앰으로써**(`compile_fail` 독 테스트가 지킨다) 풀었다 — 그것은 A의 방식이
아니라 E의 방식이다.

**3. P2′ sees one spelling.** `sequence<float>`, unbounded `string`, `Any`: one
edit routes around every check any option here would install (§1).

**4. A CORBA-side check is always about the pointer, never the pointee.** That is
the point of a handle — and it means a bounded, annotated, correctly-named,
fully-gated operation still hands over a token that dereferences to a gigabyte.
Whatever discipline governs the pointee is not in this project.

**5. "BREAKING" here is a statement about a hypothetical peer.** Nothing outside
this repository serves `moe::Tensor`. §4.5 met exactly this and refused to use it
as an excuse — *"the same is true of every contract on the day before someone
deploys it, and a project that edits released types when it is convenient has no
§5.3 at all"* — and this document takes the same position for the same reason.
But the honest consequence is that the loudness protecting us here is
**disciplinary, not physical**, and discipline is the thing that fails quietly.

**6. Therefore the rule's real enforcement is social, and this document's honest
claim is smaller than it looks.** Moving the rule from a corpus comment into a
decision makes it *findable* — strictly more than a trailing comment on line 13,
and strictly less than a check. E is recommended not because it enforces the rule
but because it is the only option that **removes the opportunity** instead of
labelling it, which is what F3 did and what actually held.

**규칙의 실제 강제는 사회적이며, 이 문서의 정직한 주장은 보이는 것보다 작다.**
코퍼스 주석에서 결정 문서로 옮기는 것은 규칙을 *찾을 수 있게* 만들 뿐이다 —
13행 끝의 주석보다는 분명히 낫고, 검사보다는 분명히 못하다. E를 권고하는 이유는
그것이 규칙을 강제해서가 아니라, 규칙을 표시하는 대신 **기회를 제거하는** 유일한
대안이기 때문이다.

---

## What was verified, and what was not / 검증된 것과 아닌 것

**Verified in this worktree, 2026-08-14**, by running the named command or
reading the named file: the three `idl-diff` / `contract-check` / `omniidl` runs
quoted in §*What was measured*; `diff.rs`'s bound arm (`:426-452`), its two bound
tests (`:504`, `:520`) and `same_identity` (`:253`); `dynamic/src/lib.rs`'s
`check_bound` (`:594`) and its call sites (`:539`, `:718`) and test (`:988`);
`gen/src/lib.rs:164` dropping the bound and `gen/src/rt.rs:526-546`'s unchecked
`Vec<T>` codec; `contract.rs`'s rule for adding a rule (`:12-16`), `VOCABULARY`
(`:120`, eight keys), the interface-only iteration (`:205`) and `unknown_keys`
(`:668`); `registry/src/lib.rs:647` and `:917-928` carrying typedef annotations
into the registry; and the five `Tensor` positions counted directly from
`corpus/golden/22-moe-control-plane.idl` (`:13`, `:18`, `:33`, `:41`, `:55`,
`:56`).

**Unverified, stated plainly:**

- **Whether a foreign peer refuses an over-bound sequence loudly.** No fixture was
  run. The "refusal, not a misread" sentence is verified for *our* dynamic
  marshaller and quoted, not measured, for omniORB. Measurement 3 above is the
  experiment.
- **Any handle size.** No handle scheme exists in this repository, so the `64` has
  no empirical support of any kind, here or in §4.6.
- **Whether `select`'s affinity vector is small in practice.** The claim that an
  affinity vector is "activation-shaped" is an argument from what a gate does, not
  a measurement of one; no gating trace exists here.
- **Whether the MCP exposure gate could act on an `ai_handle` claim.** Named as a
  plausible consumer for option B from the shape of the exposure path, not traced
  through it.
- **The generator-side fix for the dropped bound.** Called "small" above on the
  strength of reading `rust_type` and the `Vec<T>` codec; not prototyped, and not
  costed against the differential oracle that requires the static and dynamic
  paths to agree byte for byte.

**미검증을 그대로 적는다:** 외부 피어가 상한 초과를 시끄럽게 거절하는지(픽스처
미실행), 실제 핸들 크기(이 저장소에 핸들 체계가 없으므로 `64`는 아무 근거도
없다), `select`의 affinity 벡터가 실제로 작은지(게이팅 트레이스 없음), B의
소비자로 지목한 MCP 노출 게이트가 정말 그 주장에 따라 행동할 수 있는지, 그리고
`gen`의 상한 누락 수정이 "작다"는 평가(읽기만 했고 프로토타이핑하지 않았다).

---

## What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is adopted. No crate, no corpus file and no contract changes; the
footprint is this file and three pointer lines.

Specifically left open: **whether `Router::select` should be served**, which §4.6
files as a gap and which this document only complicates by finding a `Tensor` on
its input side — the gap is still a gap, and closing it is stream F's call, not
this document's; whether `contract::check` should visit `Entry::Type` at all,
which is a checker-scope question that outlives this rule and which options B and
C both depend on; whether the static generated path should carry sequence bounds,
which is a `gen` correctness question that exists whether or not this document is
approved and which the differential oracle should probably have caught already;
what the data plane actually *is* (§5 keeps it a named external and this document
does not name it); and whether `moe::Tensor` should exist as a typedef at all if
no operation may carry one. Stream F owns the work if this is approved.

오늘 채택되는 것은 없다. `select`를 서빙할 것인가, `contract::check`가
`Entry::Type`을 순회해야 하는가, 정적 생성 경로가 시퀀스 상한을 실어야 하는가,
데이터 플레인이 실제로 무엇인가 — 모두 열린 채로 둔다. 다만 마지막 항목 하나는
이 문서가 만든 질문이다: 어떤 연산도 나를 수 없다면 `moe::Tensor`라는 typedef가
존재해야 하는가.
