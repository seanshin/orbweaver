# Orbweaver — working instructions

AI-driven CORBA/IDL interface automation. An MIT ORB core written against the
published OMG wire specification, plus an AI specification pipeline on top.

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
- **Never pipe into `grep -q`** when the producer matters. `grep -q` exits on
  first match and SIGPIPEs upstream. Capture to a variable, then match.
- **A completed client `connect` does not mean the server can accept yet.**
  On macOS loopback a non-blocking single `accept()` misses fresh connections
  ~5% of the time (measured 25/500 in stream E batch 2). Accept-side checks
  wait with a sleeping, deadline-bounded loop — the same class as the wait
  rule above.
- **An unmeasured check is a failure, never a pass.** If a fixture will not
  start, increment the failure counter. A harness that reports green on an
  unmeasured assumption is worse than no harness.

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
cargo test --workspace          # ~1200 tests across twelve crates
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
cargo run -q --bin gen-python -- --out <dir> <files>.idl   # the second target
cargo run -q -p orbweaver-console --bin orbweaver-console -- catalog <file>.idl --text
python3 spikes/gap_symbols.py   # before planning against a COMPONENTS gap row: what it
                                # names and whether that exists — a report, not a gate
```

Fixture setup: `brew install omniorb` (interop peer and oracle only);
`spikes/jacorb/setup.sh` for the second oracle and its Interface Repository.

## Layout

```
crates/orbweaver-cdr/       CDR encode/decode
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
- Corpus additions go in with the change that motivated them, never later.
- Rust: `unsafe_code = "forbid"` at the workspace level. Keep it that way —
  wire parsing is the classic memory-safety hazard and that is why the core is
  Rust in the first place.
