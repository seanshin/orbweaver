# Pipeline run — the split pipeline, S1–S5 (2026-08-13)

The first batch through the pipeline as **five stages** rather than one prompt
with a validator behind it. The 2026-08-13 run measured a single model call per
requirement (`gen_claude.sh` → IDL) against S4; this one measures S1 ingest,
S2 synthesize and S3 annotate **separately**, each with its own producer, its
own gate, its own first-pass rate and its own round count.

The point of the split is attribution. The previous run could say the batch was
100%; it could not have said *which stage* was wrong if it had not been, because
there was one stage. This run reports four numbers where there was one.

## Setup

| | |
|---|---|
| Requirements | the 20 Phase 0 assumption-B originals (R01–R20), unchanged |
| Inputs | `corpus/requirements/inputs/R01.txt` … `R20.txt` |
| Producer | one wrapper for all three stages → `claude -p` (Claude Code CLI 2.1.229) |
| Model | `claude-fable-5` (self-reported by the CLI; not independently verified) |
| Prompts | `orbweaver_forge::{ingest::S1_PROMPT, synthesize::S2_PROMPT, annotate::S3_PROMPT}`, passed to the wrapper as `FORGE_PROMPT` |
| Gates | S1 `ingest::gate`, S2 `synthesize::gate`, S3 `annotate::check_against`, S4 `validate` |
| Command | `forge-pipeline --requirements corpus/requirements/inputs --ingest <w> --synthesize <w> --annotate <w> --out <dir> --max-rounds 3 --register` |
| Cross-checks | `omniidl -bdump` (conformance oracle), `contract-check` (S7, `orbweaver-test`) |

**The prompts are crate constants, not shell text.** `FORGE_PROMPT` names a file
the pipeline writes from the crate, so the prompt a measurement used is
versioned with the checker that graded it — the previous run record proposed
exactly this rule ("a first-pass rate is meaningless without the prompt that
produced it") and it is now mechanical rather than a convention.

## Round 1 — S1 → S5, verbatim

```
range: S1 ingest → S4 validate over 20 item(s), 3 repair round(s) allowed per stage
S1 ingest: 20 item(s)
  first-pass: 18/20 valid (90%) — after round 1, before any repair
  rounds: 2 used, 3 allowed
    round 1: [s1/name-clash] 1 item(s)
    round 1: [s1/no-operations] 1 item(s)
    round 2: no causes
  result: all 20 item(s) valid
S2 synthesize: 20 item(s)
  first-pass: 19/20 valid (95%) — after round 1, before any repair
  rounds: 2 used, 3 allowed
    round 1: [enclosing-scope-clash] 1 item(s)
    round 1: [identifier-case-clash] 1 item(s)
    round 2: no causes
  result: all 20 item(s) valid
S3 annotate: 20 item(s)
  first-pass: 20/20 valid (100%) — after round 1, before any repair
  rounds: 1 used, 3 allowed
    round 1: no causes
  result: all 20 item(s) valid
S4 validate: 20 item(s)
  first-pass: 20/20 valid (100%) — after round 1, before any repair
  rounds: 1 used, 1 allowed
    round 1: no causes
  result: all 20 item(s) valid
S4 gated 20 annotated file(s) and 0 unannotated draft(s)
S5: registered 20 item(s); 22 exposable interface(s), every one exposed=no
```

Exit code 0. Wall time **1445 s** (24 min 5 s) for 60 sequential model calls
plus 4 repairs.

### The numbers, stated separately per §5.1

| Stage | Batch | First-pass | Rounds used / allowed | Causes in round 1 |
|---|---|---|---|---|
| S1 ingest | 20 | **18/20 (90%)** | 2 / 3 | `s1/name-clash` ×1, `s1/no-operations` ×1 |
| S2 synthesize | 20 | **19/20 (95%)** | 2 / 3 | `enclosing-scope-clash` ×1, `identifier-case-clash` ×1 |
| S3 annotate | 20 | **20/20 (100%)** | 1 / 3 | none |
| S4 validate | 20 | **20/20 (100%)** | 1 / 1 | none |
| S5 register | 20 | — | — | 22 exposable interfaces, every one `exposed=no` |

Nothing was dropped: every stage converged, so every item reached S4.

### Cross-checks after round 1

- **omniidl:** 20 pass, 0 fail over the annotated files. Structured comments are
  comments, so a SIDL file the reference compiler rejects is a file whose IDL is
  wrong; none was.
- **`contract-check` (S7):** 20 files, 152 types × 32 cases × 2 byte orders —
  **0 property defects, 1 contract finding.**

## Root causes, clustered

Four causes, none of them shared across stages — which is itself the result
worth reading, because a single-prompt pipeline would have reported all four as
"the model produced bad IDL".

| # | Cause | Stage | Items | What it was |
|---|---|---|---|---|
| A | `s1/name-clash` | S1 | 1 | a brief naming a field for the entity it carries — the Phase 0 dominant cause, **caught before any IDL existed** |
| B | `s1/no-operations` | S1 | 1 | a brief that described data and asked for no calls |
| C | case-insensitive clashes | S2 | 1–2 | `enclosing-scope-clash` and `identifier-case-clash`, the Phase 0 cause again, this time in the IDL |
| D | `contract/oneway-not-idempotent` | **S3's gate, missing** | 1 | R13's `oneway submit` declared `ai_idempotent: false`; S3's gate had nothing to say and S7's checker did |

**A and C are the same underlying rule at two different stages, and that is the
argument for the split in one line.** Phase 0 measured this cause seven times out
of seven in generated IDL. It is now caught at S1 on the *reading* — a field
named for its entity is visible in the brief — and at S2 on the IDL. A run where
it fires at S1 and a run where it fires at S2 are different failures with
different fixes, and before this change they were the same number.

**D is the interesting one.** It is not a failure of the model: it is a gate
that did not know something. `contract-check` in `orbweaver-test` reads the same
SIDL vocabulary the MCP policy gate reads, and it flagged a combination S3's own
checker never looked at — a one-way call that also declares retry unsafe, which
leaves a client that lost its connection with no correct move at all. The
independent oracle earning a finding the stage's own gate missed is what an
independent oracle is for.

Cause C's item count is `1–2` rather than exact, and that is a defect in the
report rather than in the run: `BatchReport::causes` carried rule → **count**,
so nothing recorded whether one file broke two rules or two files broke one
each. §5.1 says the oracle step's output is "a list of causes with their
affected items". It now is: `causes` carries the ids, `BatchReport::affected`
reads them back, and the printed line ends `1 item(s): R07`. Found by writing
this document and failing to answer a question it should have been able to
answer.

## Repair and codify

One fix per cause, and the only cause needing a code change was D:

- **D → `s3/oneway-not-idempotent`, in the prompt *and* in the check.** The
  `annotate::RULES` roster names both halves for every rule S3 enforces, and
  `every_rule_is_a_prompt_constraint_and_a_check` fails if either half is
  missing — so this cause cannot come back silently; it would have to come back
  as a red test.
- **A, B, C** needed no codification: they are already rules, they fired, they
  produced repair prompts, and the repair round fixed every affected item. That
  is the loop working rather than a finding.
- **The report shape** (causes carrying their item ids) is codified in the type,
  not in a convention.

## Round 2 — S3 alone, after codifying D

The whole point of stage isolation, exercised: re-running S3 costs 20 model
calls, not 60, and S1 and S2 are not touched.

```
forge-pipeline --annotate <w> --out <dir> --from s3 --to s4 --max-rounds 3 --register
```

```
range: S3 annotate → S4 validate over 20 item(s), 3 repair round(s) allowed per stage
S3 annotate: 20 item(s)
  first-pass: 19/20 valid (95%) — after round 1, before any repair
  rounds: 2 used, 3 allowed
    round 1: [s3/missing-ai_authz] 1 item(s)
    round 2: no causes
  result: all 20 item(s) valid
S4 validate: 20 item(s)
  first-pass: 20/20 valid (100%) — after round 1, before any repair
  rounds: 1 used, 1 allowed
    round 1: no causes
  result: all 20 item(s) valid
S4 gated 20 annotated file(s) and 0 unannotated draft(s)
S5: registered 20 item(s); 22 exposable interface(s), every one exposed=no
```

Exit code 0. Wall time **398 s** (6 min 38 s) — a sixth of the wall time and a
third of the model calls of a full re-run, which is what "runnable alone" buys.

### Cross-checks after round 2

- **omniidl:** 20 pass, 0 fail.
- **`contract-check`:** 20 files, 152 types — **0 property defects, 0 contract
  findings.** The finding that opened this round is gone: R13's `submit` now
  declares `ai_idempotent: true`.
- The new rule `s3/oneway-not-idempotent` **did not fire**. The prompt
  constraint prevented the combination rather than the check catching it, which
  is the codify step doing the more valuable of its two jobs.

### Round count and first-pass rate, separately

- **Rounds:** 2 (round 1 found four causes, round 2 found none). The batch loop
  terminated on "a round yields no new root causes", not on a round limit.
- **First-pass rates** are per stage and per round and are listed above. There
  is no single first-pass number for this run, deliberately: the four numbers
  are the deliverable.

## Honesty caveats

- **Same model family throughout — the numbers are indicative.** The producer is
  a Claude model, the gates and their fix-hints were written by a Claude model,
  and this report's author is a Claude model. Per the honesty rules and PLAN §8,
  90/95/100/100 are indicative figures, not a clean benchmark, until the frozen
  benchmark with a hold-out subset and an independent harness exists.
  `contract-check` and `omniidl` are the two checks in this document that were
  not written for this run: the first is a peer crate with its own rules, the
  second is a foreign compiler.
- **One run each, no variance estimate.** S3 scored 100% in round 1 and 95% in
  round 2 **over the same drafts**. The new rule did not fire in round 2, so the
  difference is run-to-run variance in which annotations the model omitted, not
  a regression the change caused. One sample per cell; nothing here supports a
  claim about the difference between 95% and 100%.
- **Round 2 is conditional on round 1's drafts.** It re-annotated the S2 output
  round 1 produced. Its S3 number is a statement about annotating *those* twenty
  files.
- **The S1 numbers measure a schema as much as a reading.** Both S1 failures
  were structural (a name clash, an empty operation list). No measurement here
  says whether a brief that *parses* read the requirement correctly — that is
  what the `open_questions` field and a human reader are for, and no human read
  these twenty briefs.
- **The wrapper is not committed.** It lives in the run's scratch directory; the
  recommended committed form is in the "Harness" section below and was
  deliberately not applied, since `spikes/` was outside this change's footprint.
- **The first attempt at this batch failed 20/20 with `producer-error`** before a
  single model call happened: an apostrophe inside `${FORGE_PROMPT:?…}` made the
  wrapper a syntax error. Recorded because the pipeline got it right — the
  failure was counted under `producer-error`, distinct from "the model produced
  something invalid", which is the distinction that made it two minutes to
  diagnose instead of a hunt through the outputs.

## Artifacts

- Briefs, drafts and SIDL: the run's `--out` directory (not committed; the
  Phase 0 set stays in `corpus/requirements/generated/`).
- Inputs: `corpus/requirements/inputs/` (committed previously).
- Prompts: `forge-pipeline --print-prompt s1|s2|s3` reproduces them exactly.

## Harness — recommended, not applied

`spikes/` was outside this change's footprint, so nothing below was added to
`run_checks.sh`. The deterministic half needs no model and belongs in the gate:

```sh
# S1/S3 as stages: structure round-trips, stage isolation, S4 gating,
# and contract-check over S3's output. No model, no network.
hr "forge stages — S1 ingest, S3 annotate, isolation and re-gating"
RUSTFLAGS="-D warnings" cargo test -q -p orbweaver-forge || fail "forge stage tests"

# S3's output must satisfy the S7 checker, not only S3's own gate. Point this
# at a workspace an annotate run left behind; skipping is a FAILURE, not a pass.
if [ -d "${FORGE_OUT:-}" ] && ls "$FORGE_OUT"/*.sidl.idl >/dev/null 2>&1; then
    cc=$(cargo run -q -p orbweaver-test --bin contract-check -- "$FORGE_OUT"/*.sidl.idl 2>&1)
    case "$cc" in
        *"0 contract finding(s)"*) pass "contract-check clean over S3 output" ;;
        *) fail "contract-check findings over S3 output: $cc" ;;
    esac
else
    fail "no annotated output to check — an unmeasured check is a failure"
fi
```

The model-facing half should stay out of `run_checks.sh` (24 minutes and an API
key) and run as a recorded batch like this one, with its numbers landing here.
