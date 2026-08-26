# Orbweaver — working instructions

AI-driven CORBA/IDL interface automation. An MIT ORB core written against the
published OMG wire specification, plus an AI specification pipeline on top.

**Priority zero — what "a finished ORB" means.** Set by the project owner
2026-08-26. Its home is `docs/decisions/D029-what-a-complete-orb-would-mean.md`
§6 and it is **not restated here**: read it there. In one line, so you know
whether your work touches it — *the ORB is complete when there is no leak in
the transparency that a caller can invoke any target holding only a reference,
knowing nothing of its location, backend, language or load state, and that
this survives targets being added, removed, moved, loaded or evicted at
runtime.* Every plan document is subordinate to it and each records how its own
work bears on it. **Transparency is not confirmed, it is hunted** — a proposal
that closes a leak outranks one that adds a capability.

*0순위 기준의 집은 D029 §6이다. 여기서 다시 적지 않는다 — **투명성은 확인하는 것이
아니라 구멍을 사냥하는 것**이며, 구멍을 막는 제안이 기능을 더하는 제안보다 앞선다.*

Plan: `docs/PLAN.md` (EN) · `docs/PLAN.ko.md` (KO) · Phase 0 results: `docs/PHASE0.md`

---

## The operating model: batch → oracle → repair → codify

**Work the whole set at once, verify the whole set at once, fix by root cause,
then make the cause impossible.** Never item-by-item.

**전체를 한 번에 작업하고, 한 번에 검증하고, 근본원인 단위로 고치고, 그 원인이
다시 발생하지 못하게 만든다.** 건건이 처리하지 않는다.

This is not a style preference. Phase 0 measured it: 20 IDL files generated in
one pass produced 7 failures, and **all 7 had a single root cause**. Item-by-item
work would have produced 7 separate patches and never surfaced the rule.
Batching made the cause visible; one fix took the batch from 65% to 100%.

이것은 취향이 아니다. Phase 0이 측정했다: 20건 일괄 생성 → 실패 7건 → **7건 전부
동일한 근본원인**. 건건이 고쳤다면 패치 7개가 나왔을 뿐 규칙은 드러나지 않았다.

### The four steps

1. **Batch** — produce every item in the set in one pass. **Do not consult the
   oracle mid-pass.** Peeking contaminates the first-pass measurement and, worse,
   lets you patch symptoms one at a time so the shared cause never appears.
   *일괄 생성 중에는 오라클을 보지 않는다. 중간에 보면 1차 통과율이 오염되고
   공통 원인이 드러나지 않는다.*

2. **Oracle** — run every deterministic check across the whole batch at once.
   Then **cluster the diagnostics by root cause, not by item.** The output of
   this step is a list of causes with their affected items, never a list of items.
   *진단을 항목이 아니라 근본원인으로 묶는다.*

3. **Repair** — one fix per root cause, applied across every affected item.
   If a "fix" only helps one item, it is not a root-cause fix; find the real
   cause or record it as a genuine one-off with a reason.
   *원인 하나당 수정 하나. 한 항목에만 듣는 수정은 근본 수정이 아니다.*

4. **Codify** — turn every confirmed cause into something permanent: a lint
   rule, a prompt constraint, a corpus case, or a rule in this file. A cause
   that is only fixed will come back; a cause that is codified cannot.
   *확인된 원인은 반드시 영구 산출물로 바꾼다. 고치기만 한 원인은 돌아온다.*

Repeat until a round yields no new root causes. Report the first-pass rate and
the round count separately — they measure different things.

### Reporting a batch

Always state: batch size, first-pass rate, root causes found (with affected
counts), what was codified. Never report only the final number — the first-pass
rate is the signal about the generator, the round count is the signal about the
oracle.

---

## Hard rules

### Licensing boundary — non-negotiable / 라이선스 경계 — 타협 불가

This project ships MIT. omniORB, ACE/TAO and JacORB are **LGPL/GPL/DOC and are
fixtures, never dependencies.**

- **Never** `import`, link, vendor, copy from, or redistribute any part of them.
- They may only be (a) run as separate-process wire peers over TCP, and
  (b) invoked as external programs whose text output we read (`omniidl` as a
  conformance oracle).
- `cargo tree` must stay free of them. Anything under `crates/` is original work
  written against the OMG specification.
- CI images containing them are built or pulled inside CI and **never published**
  as project artifacts — publishing is redistribution.

**Amended 2026-08-12 (D001, approved).** MIT for everything we write. Where a
component is **data we cannot originate** — a character mapping table, a
timezone database — permissive-with-attribution is accepted, disclosed in
`NOTICE`, and recorded under `docs/decisions/`.

The distinction is the point, not a loophole. Logic defined by a published
specification we implement ourselves and owe nobody for; that is why the ORB
core is first-party. A mapping table is somebody's compilation of facts with no
specification to implement from, so retyping it produces the same derived work
rather than an original one — *a table derived from an incompatibly-licensed
source is not laundered by being retyped.*

우리가 쓰는 것은 MIT. **우리가 원저작할 수 없는 데이터**는 귀속 표시 조건의 관대
라이선스를 허용하되 `NOTICE`에 공개하고 결정으로 기록한다. 로직과 데이터의 구분이
핵심이며 빠져나갈 구멍이 아니다.

Currently accepted under this clause: `encoding_rs` for EUC-KR, behind the
default-on `euc-kr` feature. `--no-default-features` removes it and the
obligation. Both configurations are tested by `run_checks.sh`.

Before adding any dependency, check its licence against this rule — and check
the provenance of its *data*, not only its declared licence. A crate declaring
MIT over a table it does not account for is a worse position than an honestly
disclosed BSD-3-Clause.

### IDL rules the compiler enforces / 컴파일러가 강제하는 IDL 규칙

- **Identifier clashes are case-insensitive.** A member, parameter or operation
  may not share a name with a type or an enclosing scope, ignoring case.
  `Position position`, `Value value`, `module inventory { interface Inventory }`
  and `struct Version { unsigned long version; }` are all illegal. This is
  natural naming in every other language, which is exactly why it is the
  dominant generation failure. *가장 흔한 실패 원인.*
  Run `cargo run -q --bin sidl-validate -- <files>` before the oracle — it
  catches this class with an actionable message. (The regex lint
  `spikes/idl_lint.py` this rule used to name retired in Phase 2 batch 2, when
  semantic analysis subsumed it; the instruction outlived the file by several
  phases, which is its own small lesson about codifying a command instead of a
  capability.) Documenting the rule does not prevent it:
  it has since caught two corpus files and two fixtures, one written by someone
  who had just described the rule in that same file's header.
- **A target language's reserved words are the generator's problem, not the
  contract's.** `yield`, `lambda` and `None` are legal IDL and reserved
  somewhere. Every emitter escapes them, and until
  `corpus/golden/28-target-keywords.idl` existed no emitter's escaping had ever
  been *executed* — the Rust list was missing `yield`, so `fn yield()` was
  emitted and did not compile. Adding a target means adding its keyword list to
  that file's coverage. *대상 언어의 예약어는 계약이 아니라 생성기의 문제다.*
- `TypeCode` must be qualified as `::CORBA::TypeCode`.
- v1 wire support excludes `valuetype`, abstract interfaces and `fixed`. The
  parser accepts them; the wire does not. See `docs/PLAN.md` §4.4.
- SIDL v1 uses **structured comments** (`//@ ai_desc: ...`), not IDL 4
  `@annotation` — deployed compilers reject the latter (Phase 0 assumption C).

### Wire and test rules / 와이어·테스트 규칙

- **Compare decoded values, never raw buffers.** CDR padding content is
  undefined by the specification and omniORB does not zero it, so byte-for-byte
  message comparison against a reference ORB produces false failures.
- **Test both byte orders.** An encoder that only works native-endian passes
  every local test and fails in the field.
- **Alignment origin matters.** A GIOP message aligns from the first byte of its
  12-byte header; an encapsulation restarts alignment at its own first byte.

### Harness rules / 하네스 규칙

Each of these produced a phantom failure during Phase 0. They will recur.

- **Wait loops must sleep.** `for i in $(seq 1 500); do [ -f f ] && break; done`
  finishes in microseconds and does not wait at all. This caused the initial
  assumption A failure; the protocol was correct the whole time.
- **Never pipe into `grep -q`** when the producer matters, and know that there
  are **two** independent ways such a pipeline lies. `grep -q` exits on first
  match and SIGPIPEs upstream (141). And under `set -o pipefail` — which
  `run_checks.sh` sets on line 9 — a pipeline's status is its *rightmost
  non-zero* exit, so **a producer that fails makes the pipeline fail, and an
  `if` reads that as "no match".** Measured 2026-08-25, in this harness's own
  first three gates: `cargo test --workspace --quiet 2>&1 | grep -q "^error"`
  printed **`ok cargo test --workspace` over a red workspace** for as long as
  it had existed — its FAIL branch required `cargo test` to *pass* while
  printing an error line — and `cargo tree --workspace | grep -qiE
  "omniorb|jacorb"` could not report a forbidden dependency, because finding
  one is exactly when SIGPIPE fires. **The licence boundary this file calls
  non-negotiable had a gate that could not go red.** A short producer fits the
  64 KB pipe buffer and never sees SIGPIPE, which is why hand-checking it
  always looked fine. Capture to a variable, then match with a **herestring**
  (`grep -q … <<<"$out"`) — never `printf … | grep -q`, which this file called
  the sanctioned form and which **has the same defect**: capturing the output
  first saves the *data*, and `grep -q` still exits early, still SIGPIPEs the
  `printf`, and `pipefail` still turns that into "no match". Swept the same
  day: **76 of them in this harness**, and the one that mattered was the
  concurrent-dispatch group's own `printf '%s' "$cd_out" | grep -q "^test
  result: FAILED"` over three crates' test output — non-deterministically, by
  where in the output the failure fell. A group whose whole argument is *"five
  runs, because one green run is not evidence"* could not see a failing run
  when the failure came early. Also **read the producer's own exit status
  first** — a producer that could not run at all is an unmeasured check, which
  is a failure and never a pass. A `grep` without `-q` reads its whole input,
  never SIGPIPEs, and is safe in a pipeline; only the early-exit forms (`-q`,
  `-m1`, `head`) are the hazard. *두 가지 방식으로 거짓말한다 — SIGPIPE와
  `pipefail`. 변수로 캡처해도 파이프면 똑같이 거짓말한다; herestring을 쓴다.
  "다섯 번 도는 이유는 한 번의 초록이 증거가 아니기 때문"이라던 그룹이 바로 그
  형태 때문에 실패한 실행을 못 보고 있었다.*
- **Never edit a script while it is running.** `bash` reads a script
  incrementally, so an edit that shifts byte offsets can make a running shell
  resume at the wrong place. Done 2026-08-25 — three gate repairs written into
  `run_checks.sh` while a 43-minute run of it was in flight. Nothing visibly
  broke, and that is the problem: **whether the run was affected cannot be
  established after the fact**, so its verdict stopped being evidence and the
  run had to be repeated. Wait for the lock, or copy the script. *실행 중인
  스크립트를 편집하지 않는다. 영향 여부를 사후에 증명할 수 없으므로 그 실행의
  판정은 증거가 되지 못한다.*
- **A completed client `connect` does not mean the server can accept yet.**
  On macOS loopback a non-blocking single `accept()` misses fresh connections
  ~5% of the time (measured 25/500 in stream E batch 2). Accept-side checks
  wait with a sleeping, deadline-bounded loop — the same class as the wait
  rule above.
- **An unmeasured check is a failure, never a pass.** If a fixture will not
  start, increment the failure counter. A harness that reports green on an
  unmeasured assumption is worse than no harness.
- **A new harness group lands with its negative control in the commit
  message** — the command that was run to make it red, and what it printed
  (D010 §7.2). Five gates were green while measuring nothing in one week, and
  every one was found by a negative control, none by review; the fifth was
  written by the person who had just recorded the other four — a probe that
  grepped its marker out of a traceback echoing the source line. Probes use
  exit codes, not markers.
- **A peer's bytes are recorded with provenance and re-taken live.** A
  convention both ends apply cannot be refuted by a round trip, and a
  convention one end applies on read can hide the other end's defect on
  write; twelve wire changes in v0.5.0 were found this way and none by a test
  we could have written from the specification alone. One test per capture
  decodes it and re-encodes back to the peer's bytes; the harness re-takes
  every capture from the live fixture (`spikes/*_capture.py`).
- **Our own counters are not what a peer saw, and a two-process fixture must
  never let its own exit code vouch for the peer's.** Measured 2026-08-26
  (D034 §5.1): with the ORB's shutdown deliberately broken to drop in-flight
  work, `spike-orb-shutdown` printed `servers_stopped:1, went_quiet:true,
  serve_returned_ok:true` and **exited 0** — every number this side keeps said
  the shutdown was clean — while the peer on the other end of the same socket
  got a TCP reset and not one octet of GIOP. A lifecycle checked from its own
  counters is green on exactly the build it exists to refuse. Print both,
  verdict from the peer. And **a reset is an observation, not a failure to
  measure**: that peer's first draft filed one under `UNMEASURED` and exited 3,
  so the strongest refutation it could produce would have read as an unmeasured
  check rather than a failure — found by running the control, not by reading it.
  *우리 카운터는 피어가 본 것이 아니다. 자기 카운터로 검사하는 생애주기는 그것이
  거절하려는 바로 그 빌드에서 초록이 된다. 리셋은 측정 실패가 아니라 관측이다.*
- **A class-B claim lands as a counted `SKIPPED` group naming its fixture,
  never as a `note` and never as `ok`** (D010 §2).
- **A batch scoped to a keyword will fix a keyword; scope it to the rule.**
  Twice in one release the defect handed to a batch was one instance of a
  production-wide divergence: a signature takes `param_type_spec` (ten
  divergences from the oracle, eight closed by one function — a `fixed`-only
  fix would have closed three) and a constant takes `const_type` (seven
  shapes, one cause, two of seven). Both agents re-measured the neighbours of
  the shape they were given, which is why the count is known at all.
- **A record written by a script that fails is not a record.** A `python3 -
  <<EOF` whose anchor has drifted exits non-zero, and the `git commit` on the
  next line runs anyway: twice this release a records commit carried only part
  of what its message claimed, and `records_keep_up.py` read eighteen commits
  behind while the failure looked like a false alarm. Check the writer's exit
  status before staging, and read the gate's complaint as true until measured
  otherwise. The verdict line counts
  SKIPPED; prose after an `ok` is not counted and reads as coverage.

### Where a fact lives / 사실이 사는 곳

Every fact has one home. A document that **restates** another document's fact
drifts from it on the next change, silently, because nothing compiles a
sentence. Measured 2026-08-18: ten stale decision-status claims and four stale
remaining-work lists across five documents, produced by nothing worse than
decisions being approved and work landing.

*사실마다 집은 하나다. 다른 문서의 사실을 **다시 적은** 문장은 다음 변경에서
조용히 어긋난다 — 문장을 컴파일하는 것은 없기 때문이다.*

- **A decision's status lives in `docs/decisions/D00N-*.md` and nowhere else.**
  Every other mention is checked against it by `spikes/decision_status.py` in
  the harness. Dated records — `docs/pipeline-runs/`, `PHASE*.md`, released
  CHANGELOG sections — state what was true at a date and are out of scope by
  construction: editing them to match today would falsify them, not repair them.
- **`PLAN` records what was planned and what landed against it; `COMPONENTS`
  records current status and what is still missing.** PLAN does not restate
  COMPONENTS' remaining-work column. It did, and all three items it named had
  landed while it still named them — the cost was a planning pass spent on
  finished work, which no test can go red on. *계획서는 상태를 다시 적지 않는다.*
- **A record lands with its batch, not after it.** `COMPONENTS.md` states what
  is measured now and `CHANGELOG.md` states what changed; a script cannot check
  either for truth, so `spikes/records_keep_up.py` checks the only thing it
  can — whether they were opened at all — and fails past ten commits. Measured
  2026-08-18: they had gone **thirty-nine commits**, six of them wire-behaviour
  changes, while three `COMPONENTS.md` rows became false. *배치는 기록과 함께
  착지한다.*
- **A bilingual fact is one fact in two languages: edit both or neither.**
  D003's approval overwrote the head of its own PROPOSED block and left the
  tail, so the file said APPROVED in English and 제안 in Korean four lines
  apart — and every document that had copied it copied the English half.
  **This rule alone did not stop it.** Swept 2026-08-25: four more instances,
  every one the English half re-measured and the Korean left asserting the
  pre-measurement fact — a sweep's Korean saying it had *not* re-measured
  while the English said it had, and two counts frozen at a figure their own
  English twin had already superseded. So the rule now has a script:
  `spikes/bilingual_drift.py` blames each section and reports where the two
  halves were last edited days apart, because *the rule is an invariant about
  commits* — one fact, one change, both languages. Its first design compared
  **date literals** and its negative control killed it: 34 findings of which 4
  were real, and the threshold that suppressed the noise removed exactly those
  4. **A check tuned until it is quiet, tested only against a tree with no
  defect in it, is the green-while-measuring-nothing class with better
  manners** — tune against the defect, then confirm the quiet.
  *규칙만으로는 막히지 않았다. 규칙은 커밋에 대한 불변식이므로 이제 스크립트가
  있다. 첫 설계는 부정 대조군이 죽였다 — 조용해질 때까지 조인 검사는 찾으려던
  것만 정확히 걸러낸다.*
- **A sentence many layers say is a fact, and `pub(crate)` is how a fact
  escapes its home.** A refusal, a limit, a diagnostic head that more than one
  layer must give in the same words belongs to one function, and that function
  has to be reachable from every layer that owes it — including the ones in
  other crates. Measured 2026-08-24: the two heads for the four constructs the
  wire cannot carry were `pub(crate)` in `orbweaver-dynamic`, so
  **twelve literals in two other crates** wrote them again, and one had gone
  false — `prop.rs` told a contract-check reader that `from_json` answers
  `"cannot cross yet"` for a `fixed`, three days after that layer stopped
  saying it. Nothing was red: the pin that existed was scoped to a crate and
  the fact is scoped to the workspace. **A pin whose scope is narrower than its
  fact's is a pin that will go green over the drift.** The gate for this class
  computes the expected text by calling the same function, so a layer that
  keeps a literal fails the moment the wording changes rather than at the next
  reading. *여러 계층이 말하는 문장은 사실이며, `pub(crate)`은 사실이 자기 집을
  빠져나가는 경로다. 고정의 범위가 사실의 범위보다 좁으면 그 고정은 어긋남 위에서
  초록으로 남는다.*
- **A floor is not a figure.** A gate pinned as `>= N` proves the property it
  was built for — nothing regressed — and proves *nothing* about the count, so
  every sentence that quotes `N` as if it were today's measurement drifts
  upward in silence as the corpus grows, and the gate stays green over the
  drift because green is all it was ever going to say. Measured 2026-08-25,
  two instances in one sweep: `COMPONENTS.md` said the AnyJSON leg crosses
  `5248/5248` (floor 5248, **actual 6016**) and that the Python sweep crosses
  `172 values / 137 calls` (floor 170/137, **actual 182/139**) — and the
  harness's own comment beside that floor said `170 / 137` too, so the row and
  the gate agreed with each other and both disagreed with the run. A floor
  keeps its rationale in the comment; a figure in prose carries **the date it
  was measured**, or comes from a script that writes it
  (`spikes/coverage_tables.py`). Where a document and a gate quote the same
  number, say which one is the floor. *하한은 수치가 아니다. `>= N` 고정은
  퇴행 없음을 증명할 뿐 개수에 대해서는 아무것도 증명하지 않으므로, `N`을
  오늘의 측정처럼 인용한 문장은 조용히 어긋나고 게이트는 그 위에서 초록으로
  남는다. 산문의 수치는 측정 날짜를 달거나, 그것을 쓰는 스크립트에서 온다.*
- **A classifier is a sentence too.** Code that decides *which class a thing
  belongs to* by matching a hand-written substring of a sentence some other
  function owns is the same defect wearing the counting half's coat, and it
  fails the same way: silently, when the sentence changes for a good reason.
  Swept 2026-08-24 across twelve crates — five instances, three of them silent
  and one **already losing in the product**: `LexError::rule` classified by a
  retyped prefix that one of its own three construction sites did not carry, so
  a fixed-point literal too long to parse filed under `parse` and never
  received the fix hint written for it. Ask the owner: either the owning crate
  publishes the marker (`orbweaver_cdr::IMPLAUSIBLE_LENGTH`) or the classifier
  computes it by calling the function that writes the sentence. Where the
  constant becomes shared there is **nothing left to test** — the drift is
  impossible rather than detectable — and a negative control there comes back
  green, which is a reason to record the fact rather than to add a test.
  *분류자도 문장이다. 소유 크레이트가 표지를 공개하거나, 분류자가 문장을 쓰는 함수를
  호출해 표지를 계산한다. 상수를 공유하게 되면 남는 테스트는 없다 — 어긋남이 탐지
  대상이 아니라 불가능해지기 때문이다.*
- **A cascade whose catch-all *clears* what its mapper *refuses* is a hole the
  shape of the gap between two lists.** `orbweaver-gen`'s `rust_type` ends in
  `other => Err(..)` and its walker `representable` ended in `_ => Ok(())`, so
  every construct in the gap was skipped at its declaration and **emitted at
  every container that named it** — `pub sealed: ...::gp34::Envelope` for an
  `Envelope` nothing declares. Two lists of "what the wire cannot carry", one
  per emitter, each maintained by hand against a mapper that already knew.
  Measured 2026-08-25: the Rust half at least failed to compile; the Python
  half **was not red at all**, writing `("ref", "IDL:gp34/Envelope:1.0")` for a
  class its package never defines, which the caller discovers at the first
  call. Exhaustiveness at the leaf does not survive a walker that is permissive
  at the node: the walker must **ask the mapper** at every node rather than
  keep its own list, which makes the gap unrepresentable instead of detectable.
  Note which half of this was found by a compiler and which by nothing — the
  target with the weaker type system is where the same defect goes quiet.
  *매퍼가 거부하는 것을 캐스케이드의 catch-all이 통과시키면, 두 목록 사이 틈
  모양의 구멍이 생긴다. 잎에서의 전수성은 노드에서 관대한 순회를 견디지 못한다 —
  순회가 자기 목록을 갖는 대신 **매퍼에게 물어야** 한다. 어느 쪽 절반이 컴파일러에
  잡혔고 어느 쪽이 아무것에도 안 잡혔는지 보라.*

### Honesty rules / 정직성 규칙

- Report what was measured, not what was intended. If something is unmeasured,
  say so and say why.
- When the generator and the evaluator are the same model, label the number
  indicative and say so in the same breath as the number.
- Do not claim a transient is diagnosed until it reproduces and the fix makes it
  stop. "Did not reproduce in N runs" is a valid, honest result.

---

## Commands

```bash
cargo test --workspace          # 1949 tests across twelve crates (measured
                                # 2026-08-26; ~1515 when this line was written,
                                # and a figure in prose carries its date)
./spikes/run_checks.sh          # the harness; exit code is the verdict, one run at a time
```

The harness takes a machine-wide lock (`/tmp/orbweaver-harness.lock`) and kills
fixtures by process group. Two runs at once used to destroy each other's peers
and report failures that were about the scheduling; that cost two diagnoses.
Wait for the lock rather than removing it. *하네스는 머신 전역 락을 잡는다.*

**Gates, in the order they get run:**

```bash
cargo run -q --bin sidl-validate -- [-I <dir>]... <files>.idl   # S4: syntax,
                                # semantics, fix hints; #include resolved first
cargo run -q -p orbweaver-test --bin contract-check -- corpus/golden/*.idl
                                # property (defects) + annotation advice (never gates)
cargo run -q --bin idl-diff -- <released>.idl <proposed>.idl   # §5.3, exit 1 on breaking
omniidl -b dump <file>.idl      # the conformance oracle for one file
./spikes/differential.sh        # two front ends over the corpus, divergences recorded
python3 spikes/decision_status.py  # every restated decision status vs its decision
```

**Measurement tools** — each prints what it could *not* measure:

```bash
cargo run -q --release -p orbweaver-test --bin wire-fuzz -- --cases 50000
                                # panic freedom over the decoders a peer reaches first
cargo run -q --bin repository-ids -- corpus/pragma/*.idl   # ids, to diff against omniidl
./spikes/service_sweep.sh       # every declared operation of the five servants, over the wire
./spikes/end_to_end.sh          # requirement → contract → both halves → guarded call
./spikes/nat_rewrite.sh         # R7: an IOR dialable from where the client actually is
./spikes/estate/run.sh --tsv    # thirteen legacy contracts, ingestion to agent call
./spikes/perm_fallback.sh --expect-temporary reask --expect-permanent stay
                                # the two forward statuses, told apart by behaviour — omniORB and ours
./spikes/orb_shutdown.sh        # what a peer MID-CALL sees when Orb::shutdown stops the
                                # server under it — the peer imports no ORB and builds its
                                # own §9.4 requests, both byte orders. Its exit code is the
                                # measurement; the fixture's own counters are printed beside
                                # it and never allowed to vouch for it (D034 §5.1)
./spikes/jacorb_giop11.sh · ./spikes/jacorb_wchar11.sh · ./spikes/wide_rust.sh
                                # GIOP 1.1/1.2 wide text against JacORB, version asserted from bytes
python3 spikes/union_label_capture.py · python3 spikes/union_default_capture.py
                                # re-take the peer's recorded TypeCode bytes from the live fixture
./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py --check
                                # SERVICES-COVERAGE §8 says what the wire says
cargo run -q --bin gen-python -- --out <dir> <files>.idl   # the second target
cargo run -q -p orbweaver-console --bin orbweaver-console -- catalog <file>.idl --text
python3 spikes/gap_symbols.py   # before planning against a COMPONENTS gap row: what it
                                # names and whether that exists — a report, not a gate
python3 spikes/plan_numbers.py  # every hand-typed count in the plan documents beside
                                # today's computed figure — a report, not a gate
python3 spikes/bilingual_drift.py  # sections whose EN and KO halves were last edited
                                # days apart — a report, not a gate
python3 spikes/entry_cost.py    # what a newcomer must name to serve an object and to
                                # call one, and how short the shortest path is — a
                                # report, not a gate (D027 E4). No threshold for "too
                                # many items to learn" is defensible, which is why it
                                # is not one
```

Fixture setup: `brew install omniorb` (interop peer and oracle only);
`spikes/jacorb/setup.sh` for the second oracle and its Interface Repository.

## Layout

```
crates/orbweaver-cdr/       CDR encode/decode, and the JSON parser — it moved
                            down here 2026-08-26 so fixtures below `dynamic`
                            can read a seed; `orbweaver_dynamic::json` is a
                            re-export and removing it would widen the graph
crates/orbweaver-giop/      GIOP, IOR, TypeCode, Server/Dispatch, naming + event servants
crates/orbweaver-idl/       IDL 4.2 front end, SIDL structured comments
crates/orbweaver-registry/  types as data, the IFR facade, remote IFR ingestion, §5.3 differ
crates/orbweaver-object/    POA, references, MoE residency, tenancy
crates/orbweaver-dynamic/   value marshalling, DII/DSI shape, AnyJSON
crates/orbweaver-trading/   offer store, constraint queries, loading policy
crates/orbweaver-forge/     the S1–S5 pipeline, each stage a producer plus its own gate
crates/orbweaver-mcp/       the agent boundary: triad, handles, interceptor chain, dry-run
crates/orbweaver-gen/       client stubs and server skeletons
crates/orbweaver-test/      property, contract advice, wire fuzz
crates/orbweaver-console/   catalog, contract diff and trace pages — renders, decides nothing
corpus/golden/              must all compile — type-system and CDR coverage
corpus/negative/            must all be rejected — diagnostic quality material
corpus/services/            contracts that exist to be served (identity pragmas live here)
corpus/pragma/              repository-id cases, diffed against omniidl
corpus/requirements/        assumption B benchmark, frozen before generation
corpus/queries/             the frozen search benchmark (v1 stays frozen; v2 is widened)
corpus/annotations/         assumption C probes
corpus/include/             the first multi-file cases — resolution, prefix scope,
                            guards, cycles. Every other corpus file is
                            self-contained, which is why #include was skipped
                            rather than resolved for six phases with nothing red
corpus/divergences.tsv      where the front ends disagree, with which one we follow
spikes/estate/              thirteen legacy contracts that include each other,
                            four prefix styles, nothing annotated — consumer-
                            shaped, and a gate. It gates nothing itself, which
                            is what lets it measure the path
spikes/                     fixtures, servers, the harness, and the measurement scripts
docs/                       ARCHITECTURE (as built) · PLAN(.ko) · COMPONENTS (measured)
                            PLAN-MOE · PLAN-SERVICES · PLAN-DEFERRED · SERVICES-COVERAGE
                            decisions/ · pipeline-runs/ · PHASE0–6
```

## Conventions

- Documents are maintained in **English and Korean**, kept structurally
  symmetric section by section.
- Corpus additions go in with the change that motivated them, never later —
  **and with `./spikes/differential.sh --require omniidl,jacorb_idl --record`,
  which `cargo test --workspace` now insists on.** Measured 2026-08-25: eight
  files had landed without either front end ever comparing them, and the
  harness found seven divergences and a golden file whose generated Rust did
  not compile. The cause was not carelessness — batches are told not to run
  `run_checks.sh`, because it takes a machine-wide lock, and **nobody named the
  standalone gate they should have run instead.** A prohibition without its
  replacement is an instruction to skip the check. So the gate stopped being a
  command to remember: the differential's verdict is checked-in data
  (`corpus/differential-results.tsv`) and an oracle-free test compares the
  corpus against it, which means it runs for everybody rather than for whoever
  runs the harness. *금지에 대체물이 없으면 검사를 건너뛰라는 지시다.*
- Rust: `unsafe_code = "forbid"` at the workspace level. Keep it that way —
  wire parsing is the classic memory-safety hazard and that is why the core is
  Rust in the first place.
