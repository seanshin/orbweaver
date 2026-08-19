# D010 — What remains, and which of it cannot be measured here

**STATUS: PROPOSED** — drafted 2026-08-19 from `docs/COMPONENTS.md`'s gap
columns, the "reported and not fixed" lines of every commit since v0.4.0, and
`SERVICES-COVERAGE.md`; **revised the same day after review checked each
class-A claim against the code and found two wrong and one proposal
unworkable as written.** Not adopted. It sequences work; approval commits to
the order and to the split in §2, not to any implementation.

> **What review changed.** A1 said `LOCATION_FORWARD_PERM` needed an
> `orbweaver-giop` change; the encoder already emits status 4 and has tests for
> it, so the whole gap is one return type in `orbweaver-gen` — a smaller batch
> with a different oracle. A4 quoted the Python round-trip count from a
> commit four batches old (73/104); it is 78/132 today. And §7.1's gap-symbol
> check was **measured before being proposed as a gate**: 11 of the 17
> symbols in today's gap columns "exist" in their crate, almost all
> legitimately, so as drafted it would have been the fourth gate this week
> that is green-or-red while measuring nothing. It is now a *report*, not a
> gate, with the measurement that demoted it.

**상태: 제안됨** — 2026-08-19 작성. `COMPONENTS.md`의 공백 열, v0.4.0 이후 모든
커밋의 "보고만 하고 고치지 않음" 줄, `SERVICES-COVERAGE.md`에서 뽑았다. **채택
아님.** 순서와 §2의 구분을 결정할 뿐 구현을 결정하지 않는다.

---

## 1. Why this document, and why now / 왜 지금 이 문서인가

The session that produced v0.4.0 → today landed 41 non-merge commits, and its
own record shows two things a plan must not ignore:

- **Four "gaps" were already closed** when someone went to close them (F5,
  `knows()`, `LOCATION_FORWARD`, `SEAT_SAFETY_CONTENT`). Each was a stale row
  in a document nobody had re-read.
- **Four gates were green and measuring nothing** (a grep that caught its own
  comment, a fuzzer's static allocation model, two CSIv2 targets reached zero
  times in 50 k cases, a naming comparison agreeing by mutual destruction). Each
  was found by a negative control, not by review.

So progress was wrong in **both directions**, and a plan written from memory
would carry both errors forward. This one is written from the current gap
columns, and every row below either names the measurement that would close it
or says why none exists here.

The single number "about 80 %" was given yesterday with the caveat that most of
the remainder is not unbuilt but **unmeasurable on this machine**. §2 makes that
split the organising principle rather than a caveat.

지난 세션의 기록은 계획이 무시하면 안 되는 두 가지를 보여준다: **"공백" 넷이
이미 닫혀 있었고**, **게이트 넷이 초록이면서 아무것도 재지 않았다.** 진행률은
양방향으로 틀려 있었다. 이 문서는 기억이 아니라 현재의 공백 열에서 쓰였고, 모든
행이 닫을 측정을 이름 붙이거나 왜 그 측정이 여기 없는지를 말한다.

---

## 2. The split that organises everything below / 모든 것을 가르는 구분

| Class | Meaning | What closes it |
|---|---|---|
| **A — buildable and measurable here** | an oracle exists on this machine | a batch |
| **B — buildable, but the oracle is absent here** | the work can be done; its *verification* needs a fixture, key or host we do not have | a batch **plus** an honest SKIP until the fixture appears — never a pass |
| **C — deliberately deferred with a trigger** | `PLAN-DEFERRED` style: not built until an observable event | nothing, until the trigger fires; building it early is the defect |
| **D — a claim in a document, not code** | a row that cannot be tested against | restate, mark aspiration, or remove |

A batch that lands a class-B item and reports green has committed the error
this project's honesty rules exist for. The harness already models this
(`SKIPPED — unmeasured, not passing`, currently 3 groups); every B row below
must land as a fourth, fifth, sixth such group, not as an `ok`.

B류를 초록으로 보고하는 배치는 이 프로젝트의 정직성 규칙이 막으려는 바로 그
실수를 저지른 것이다. 하네스는 이미 이것을 모델링한다(`SKIPPED — unmeasured,
not passing`, 현재 3그룹). 아래 모든 B 행은 `ok`가 아니라 네 번째, 다섯 번째
그런 그룹으로 착지해야 한다.

---

## 3. Class A — buildable and measurable here / 여기서 짓고 잴 수 있는 것

Ordered by what a defect would cost, not by size.

### A1. `LOCATION_FORWARD_PERM` is unreachable from a skeleton — *stream B, `orbweaver-gen` only*
- **State, corrected twice.** The first draft said this needed an
  `orbweaver-giop` change; review said it did not, because `server.rs` already
  encodes status 4. Both were half right (found by the batch, 2026-08-19): the
  encoder could write it and no `Dispatch` could ask for it — `rt::Dispatch`
  is `orbweaver_giop::server::Dispatch` re-exported, `Served::Forward` carried
  a bare `Ior`, and `Server::handle_request` mapped every forward to status 3.
  The generated skeleton's own doc comment said so; the review read
  `rt::Dispatch` as a gen type. The batch was giop (a `Forward` type, a
  defaulted `redirect`, the status mapping with the 1.0/1.1 downgrade,
  `Connection::forwarded()`) then gen (the servant hook, the generated
  dispatch, re-bless), in that order.
- **Why it matters.** §7.4's I4 is where a moved object lives; without PERM
  every client re-forwards on every call, and a servant that has moved an
  object for good has no way to say so.
- **Batch (as landed, 680aa41).** giop: `Forward { Temporary(Ior),
  Permanent(Ior) }` and a defaulted `redirect` beside the existing `forward`,
  so every servant that implements `forward` keeps compiling and keeps meaning
  temporary; gen: the servant trait's `redirect`, the generated
  `Dispatch::redirect`, the emitted corpus re-blessed.
- **Oracle (measured, and not the one first proposed).** The raw reply
  status off the wire — 4 at 1.2, 3 below — from a generated skeleton, both
  byte orders, beside a temporary servant reading 3. The request count at the
  old address, which the draft named, **cannot go red**: it is 1 under both
  statuses for our client and for omniORB 4.3.4 (measured), because both move
  to the forwarded endpoint and stay. What omniORB measurably does is follow
  our status 4. The discriminating peer oracle is fallback-on-failure of the
  forwarded-to address (temporary → re-asks the original, permanent → does
  not), which needs a second server at a second address and is not built.
- **Codify.** The PERM arm in `object_identity.rs`; the byte comparison against
  the hand-written naming servant does not change, because naming never
  forwards — a review note, so nobody expects it to.

### A2. Trading's `moe::Capability` cannot answer `specialization` or `latency_p50` — *stream F*
- **State.** The three-valued matcher already treats an unpopulated field as
  *unanswerable* rather than false. But the wire contract declares neither
  field, so wire-registered offers can never populate them, and adding them is
  **BREAKING by our own `idl-diff`** (measured, PLAN-MOE §4.5).
- **Why it matters.** A latency-ordered router prefers the experts nobody has
  measured, and D006 §7 lists this exact query class as the return path.
- **Batch.** A **versioned** `Capability` — `moe::Capability` v1.1 or a sibling
  interface — carrying both fields, per §5.3: a released type is not editable
  in place. `idl-diff` gates the change; the trading engine's matcher gains two
  answerable fields; the loader publishes them.
- **Oracle.** `idl-diff` exit 0 on the additive revision and exit 1 on the
  in-place edit (both already in the harness's contract-evolution group);
  `spike-experts` selecting by `latency_p50` and refusing to prefer an
  unmeasured expert — the negative control D006 §7 describes.
- **Codify.** The corpus gains the versioned contract; PLAN-MOE §4.5's row moves
  from "unanswerable" to measured.

### A3. The dry-run's *mapping* half, and a static call's `arguments: None` — *stream A×D, three crates*
- **State.** The chain runs before arguments are decoded, so a dry run predicts
  policy and not marshalling, and the static path hands the content seat
  `arguments: None`. Named as "a three-crate `Invoker::invoke` change" and left.
- **Why it matters.** The content seat now reads argument values (this session
  fixed the ledger leak that made that safe). A deployment reading the guard
  row is told, correctly, not to assume a content stage sees a static call's
  payload — but a static call *is* the promoted path, so the payload the seat
  most needs to see is the one it does not.
- **Batch.** Thread decoded arguments through `Invoker::invoke` for the static
  path (`orbweaver-gen` stubs → `orbweaver-mcp` chain), and give the dry run a
  marshalling prediction from the same `TypeCode`s.
- **This is the one item in this document that widens what a stage can see,
  and it is ordered accordingly.** Three constraints, each with the failure it
  prevents named:
  1. **The leak test extends to the static path *before* the seat sees anything
     there.** `an_argument_a_content_stage_saw_cannot_reach_the_ledger` was
     written for the dynamic path after a PIN reached the audit ledger; the
     static path must have the same test red-then-green in the *same* commit,
     or the batch has re-opened the hole this session closed on the other path.
  2. **The gate does not move.** Refusal still precedes anything sent. If
     threading arguments through `Invoker::invoke` would make a stub encode
     before the chain runs, the batch stops and reports — that is §4.7's bypass
     in compiled form, and I1's transcript-leak test is what would catch it.
  3. **The dry run's marshalling prediction must not decode a live payload.** A
     prediction is synthesised from `TypeCode`s and the caller's declared
     values; it must never be a real call with the wire disconnected, because
     a real call reaches the content seat and the ledger.
- **A note on why this is class A at all.** It sounds like it belongs beside
  the identity work in class B, but every oracle it needs exists here: the
  leak test, I1's transcript test, and a `Bounded<String, 8>` whose refusal
  point both paths already share.
- **Oracle.** The existing leak test over a *static* call; I1's transcript-leak
  test unchanged; a dry-run of an operation with a `string<8>` argument of nine
  characters predicting `MARSHAL` where today it predicts `allow`.
- **Codify.** The guard row's caveat deleted; the leak test's static arm in the
  harness group that already carries the dynamic one.

### A4. `_rt.py` refuses an `any` carrying a constructed type — *stream B, `orbweaver-gen`*
- **State.** D008's D-symmetry: the Python half reads only a named `_t` and
  refuses v1.1's structural form **citing the decision** rather than guessing.
  Correct, and a limit a Python client meets on the first `any` with a struct
  in it.
- **Batch.** `_rt.py` maps a structural `_t` to a descriptor — the *reverse* of
  what `python.rs::descriptor` already does — and the cross-implementation
  round-trip oracle over `corpus/golden` gains the constructed-`any` cases the
  Rust side already passes.
- **Oracle.** The existing `python_target.rs` sweep, which counts values crossed
  to Python and back with 0 divergences. **Today: 78 values / 132 calls over
  golden, 35 / 46 over services** (measured 2026-08-19; the draft quoted
  73/104 from a four-batch-old commit, which is the kind of number this
  document exists to stop quoting). The count goes up and divergences stay at
  0, or the batch has failed.
- **Codify.** The skip list shrinks and the sweep pins the new count.

### A5. `SERVICES-COVERAGE.md` §4 and §2 are stale — *class D, but cheap and A-adjacent*
- **State.** §4 (CosEvent) records `BAD_OPERATION` where the code has said
  `NO_IMPLEMENT` for a day, and its counts predate the pull model. §2's naming
  row says 13 of 16.
- **Batch.** Re-run `service_sweep.sh --raw`, rewrite the two sections from the
  output, and — the codify step — **make the sweep emit the tables** so the
  document is generated from the measurement rather than transcribed. That is
  the same move `records_keep_up.py` made one level up.
- **Oracle.** `diff` between the emitted table and the committed one, in the
  harness.

### A6. The Python servant direction — *stream B, `orbweaver-gen`, D007*
- **State.** "Python is clients only" — a Python servant needs the bridge to
  call *back into* Python, a second protocol direction D007 named and did not
  build.
- **Why it matters, and why it is last in class A.** It is a whole second
  wire — the bridge would have to accept requests, not only send them — and no
  consumer here has asked for it. It is A only because every oracle it needs
  (omniORB driving a servant, the cross-implementation round trip) exists.
- **Recommendation.** Do not start it until a consumer names it. Recorded here
  so that "Python is clients only" reads as a decision rather than an
  omission.

---

## 4. Class B — buildable, oracle absent here / 지을 수 있으나 여기서 잴 수 없는 것

Every one of these lands as a **SKIPPED harness group** with the missing
fixture named in the skip line — the pattern `tao_idl`, `VOYAGE_API_KEY` and
docker already follow. None may report `ok`.

### B1. The synonym class of `search_interfaces` — *stream D, `orbweaver-mcp`*
- **State.** D003-A landed the wrapper, cache format and lexical∪vector union.
  The frozen v1 set scores 0/10 on synonyms with the offline stand-in, which is
  *"a plumbing number a token-overlap embedder cannot beat by construction"*.
- **Missing.** A real embedding model, i.e. `VOYAGE_API_KEY` (or the local
  model D003 pre-cleared).
- **When it appears.** The harness arm flips from SKIPPED to a measured score;
  I3's injection class runs against the real model. **The number is not
  predicted here.**

### B2. Identity through a real provider — *stream C, `orbweaver-identity`*
- **State.** CSIv2 wire, GSSUP, mech lists, delegation policy, the `Caller`
  seam, the token→`Caller` exchange as a *trait this project does not
  implement*, and a scope audit. **Nothing has been through a real identity
  provider**; both fixtures advertise no CSIv2 at all.
- **Missing.** A peer that speaks CSIv2 (neither omniORB nor JacORB does as
  installed) and an OIDC/JWT issuer.
- **When it appears.** The per-peer CSIv2 claim in the catalogue becomes
  measured for that peer; R17's re-establishment on token expiry becomes
  testable. Until then the deliberately-empty verifier stays empty — a verifier
  wrong in the accepting direction interoperates perfectly.

### B3. SSLIOP against a peer — *stream C*
- **State.** Built behind an off-by-default feature; in-process rustls tests
  green; **peer proof BLOCKED** because brew's omniORBpy ships no `sslTP`.
- **Missing.** An omniORB build with SSL, or JacORB's SSL transport configured.
  `spikes/tls/PEER-STATUS.md` names the unblock path.

### B4. Deployment — *stream D*
- **State.** R7's IOR rewriting built and measured against constructed failures;
  **the container probe has never executed** (no docker), no rewritten IOR has
  been put in front of a foreign ORB, no real routing domain.
- **Missing.** docker on the machine that runs the harness, and a second host.

### B5. GIOP 1.1 wide text, and a peer that shuts down mid-reply — *stream E*
- **State.** Two things this session **measured to be unmeasurable here**:
  omniORBpy cannot unmarshal its own 1.1 `wchar` output, so it is not an oracle
  for 1.1 wide text; and `InterruptedMidReassembly`'s shape needs a peer to
  close between two writes of one reply, which neither fixture exposes.
- **Missing.** A second peer for the first: JacORB *may* serve, but **nothing
  in the tree drives JacORB at GIOP 1.1 at all** — every JacORB group in the
  harness runs at its default 1.2, and its `giop_minor_version` property has
  never been set here. So the first step is not a wide-char test; it is a
  JacORB-at-1.1 fixture, after which the wide-char question is one more call.
  A controllable peer for the second — one that can be told to close between
  two writes of a reply — which neither installed ORB is.

### B6. TAO — *streams B/E, and PLAN §8*
- **State.** Named in §8 as a peer since v0.2; `tao_idl` absent; nothing
  round-trips against TAO. Yesterday's plan batch made §8's row honest (two
  peers, both directions) and moved TAO to aspiration A6 in PLAN §11.
- **Missing.** TAO installed. Then: a third column in every interop group.

---

## 5. Class C — deferred with a trigger, none fired / 방아쇠 달린 유예, 발화 없음

`PLAN-DEFERRED.md`'s eight chapters plus three from this session, listed so a
reader can see them **without** reading them as unfinished:

| Item | Trigger | Where the reason lives |
|---|---|---|
| CosNotification, OTS, Time, PSS, Concurrency, Collections, Federated Naming, Security beyond CSIv2 | each has an observable trigger in `PLAN-DEFERRED` §1–§8 | that document |
| CosEvent **supplier**-side pull | a named `PullSupplier` in this workspace | `event_server.rs` header, rewritten 2026-08-18 |
| CosEvent `destroy` | an authenticated caller model | same |
| CosNaming chaining to a foreign context | a federation requirement (also PLAN-DEFERRED §7) | `naming_server.rs` header, rewritten 2026-08-18 |
| A non-empty `char` conversion list | a peer that cannot reach UTF-8 — **probed, none exists**, and offering it was measured to *lower* what JacORB sends | D009 §8 row 4, `codeset.rs` |
| The remote `DynAny` interface | a caller that holds a component reference across calls | `dynany.rs` |
| A durable catalog store | a pilot that needs durability (D003-B) | D003 |

**Building any of these before its trigger is the defect, not the omission.**
This session showed it twice: the empty conversion list is *safer* than a
populated one against a real peer, and `NO_IMPLEMENT` for a considered
deferral is worth more to a client than a half-served operation.

**방아쇠 전에 짓는 것이 결함이지 누락이 아니다.** 이 세션이 두 번 보여줬다: 빈
변환 목록이 실제 피어에 대해 채운 목록보다 *안전했고*, 숙고된 유예의
`NO_IMPLEMENT`가 반만 서빙된 연산보다 클라이언트에게 값졌다.

---

## 6. Class D — claims in documents that cannot be tested / 시험 불가한 문서 주장

Yesterday's plan batch fixed five rows and **named five more, deliberately
untouched**. They are the remainder of this class:

| Row | Defect | Recommendation |
|---|---|---|
| §11 *contract tests auto-generated ≥ 80 %* | no run computes the ratio | mark **none** in the Instrument column now; restate when the generator counts what it emits |
| §11 *IDL first-pass rate* / *within three rounds* | "first-pass rate" has been plural since the stages split | restate per stage against `forge-pipeline`'s existing output |
| §5's *Automation target* column (95/90/80/100/100/85/90) | seven percentages, no instrument for any | either give each an instrument or move the column to aspirations — a table of seven untestable numbers is worse than no column |
| §8 *CDR encoding — byte-identical against reference ORBs* | **contradicts CLAUDE.md's wire rule** (padding is undefined; omniORB does not zero it) | resolve the contradiction: the harness compares decoded values plus recorded peer bytes re-encoded, and the row should say that |
| §12 action 3 *TAO, omniORB and JacORB containers* | historical planning text in tension with §8's restated matrix | annotate as historical, or align with B6 |

And one that is neither PLAN nor COMPONENTS: **`SERVICES-COVERAGE.md`** is a
measured document maintained by hand. A5 makes the sweep emit it.

---

## 7. Two things this session made structurally likely to recur / 재발이 구조적으로 예상되는 것 둘

Not gaps in the product — gaps in the *process* the record shows, each with a
codification proposal.

### 7.1 A stale gap column
Four this session (five, counting the idl row found while drafting this).
`records_keep_up.py` now fails past ten commits without the records being
*opened*, which is the crude half. The precise half — *is the gap column still
true* — has no instrument.

The draft proposed one: extract the backticked symbols from each gap column,
grep the crate, and flag "this gap names a thing that now exists". **Measured
before proposing it as a gate, as the decision-status gate was:** today's gap
columns name 17 symbols, and **11 of them exist in their crate** — nearly all
legitimately (`fixed`, `Dispatch::forward`, `Error::InterruptedMidReassembly`
are named *because* they exist and the gap is about what they cannot yet do).
A 65 % false-positive rate is not a gate; it is the fourth check this week that
would be red-or-green while measuring nothing, and people learn to skip those.

**Revised proposal:** `spikes/gap_symbols.py` exists as a **report the
coordinator reads when writing a plan**, not a harness gate. It prints, per gap
row, the symbols it names and whether each exists, so that the person
re-reading the row has the fact in front of them. Batch 1 builds it as that.
If a wording convention later lets a gap say "does not exist yet" in a
machine-readable way — a `~~strikethrough~~` for closed items already does half
of this — the report can become a gate for that subset, with its false-positive
rate re-measured then.

### 7.2 A gate that is green and measures nothing
Four this session, all found by a negative control. `CLAUDE.md`'s harness rules
say *an unmeasured check is a failure*; they do not say *a check must be shown
to fail*. **Proposed rule** for the harness section: **a new harness group
lands with its negative control in the commit message** — the command that was
run to make it red, and what it printed. This session did that in every
landing message; the rule makes it a requirement rather than a habit.

---

## 8. Recommended order / 권고 순서

Class A first, ordered by cost-of-defect; class D's cheap rows folded in where
they touch a batch; class B as fixtures appear; class C never, until triggered.

| # | Batch | Class | Oracle |
|---|---|---|---|
| 1 | **A5** coverage document emitted by the sweep + §7.1's `gap_symbols.py` | D→A | diff in the harness; false-positive rate measured first |
| 2 | **A1** `LOCATION_FORWARD_PERM` | A | reply status byte off the wire, both byte orders; omniORB following it (landed 680aa41 — the count is not an oracle) |
| 3 | **A3** static-path arguments and dry-run mapping | A | leak test over a static call; a `string<8>` dry-run predicting `MARSHAL` |
| 4 | **A2** versioned `Capability` | A | `idl-diff` both directions; router refusing an unmeasured expert |
| 5 | **A4** structural `_t` in `_rt.py` | A | golden crossings up, divergences 0 |
| 6 | **§6**'s five rows | D | none — restate/mark/remove with argument |
| — | **B1–B6** | B | each a SKIPPED group naming its fixture; measured the day it appears |
| — | **A6** Python servants | A | not until a consumer names it |

Batches 2–5 have footprints (`gen`, `mcp`+`gen`+`guard`, `trading`+`object`,
`gen`) — and after review's correction to A1, **three of the four touch
`orbweaver-gen`**. They cannot run as one wave the way the service wave did.
Two waves: A1 + A2 first (disjoint: `gen` and `trading`+`object`), then A3 + A4
after A1 lands (both touch `gen`, and A3's static-path change is what A4's
generated stubs would carry). Batch 1 is the coordinator's, for the same
reason yesterday's record work was.

배치 2–5는 풋프린트가 갈리므로 서비스 웨이브처럼 병행 하나로 돌리고 직렬로
착지시킨다. 배치 1은 어제의 기록 작업과 같은 이유로 코디네이터의 몫이다.

---

## 9. What approval means / 승인의 의미

1. **The split in §2 becomes the rule for reporting.** A batch names its class
   in the landing message, and a class-B batch that reports `ok` is a defect.
2. **§7's two proposals become work items** — `gap_symbols.py` in batch 1 **as
   a report, not a gate** (its false-positive rate was measured at 65 % and
   that number is what demoted it), and the negative-control rule in
   `CLAUDE.md`'s harness section. **This document holds itself to that rule**:
   every class-A batch above names its oracle, and A1's review correction is
   itself the negative control on the draft — a claim about the code was
   checked against the code and found wrong before anyone built on it.
3. **Nothing in class C is authorised.** Approval of this document is not
   approval of any deferred chapter; each has its own trigger and, where one
   exists, its own decision.
4. **The order in §8 is a recommendation**, and a measurement in an early batch
   may reorder later ones — the union-label batch reordered a whole wave, and
   that was correct.

승인은 (1) §2의 구분을 보고 규칙으로 삼고, (2) §7의 제안 둘을 작업 항목으로 만들며,
(3) **C류의 어떤 것도 승인하지 않고**, (4) §8의 순서를 권고로 둔다 — 이른 배치의
측정이 뒤 순서를 바꿀 수 있고, 그것은 옳았다.
