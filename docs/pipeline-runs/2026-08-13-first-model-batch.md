# Pipeline run — first real model batch (2026-08-13)

The first batch with a real generator through `forge-pipeline` (the §5.1
orchestrator landed in the previous wave). Until this run, the loop's
mechanics were tested only with scripted fakes and the crate said so
plainly; this run replaces that honesty note with a measurement.

## Setup

| | |
|---|---|
| Requirements | the 20 Phase 0 assumption-B originals (R01–R20) |
| Inputs | `corpus/requirements/inputs/R01.txt` … `R20.txt` |
| Generator | `spikes/gen_claude.sh` → `claude -p` (Claude Code CLI 2.1.229) |
| Model | `claude-fable-5` (self-reported by the CLI; not independently verified) |
| Gate | S4 (`orbweaver_forge::validate`: parse + semantics + registry + wire rules) |
| Command | `forge-pipeline --requirements corpus/requirements/inputs --generator spikes/gen_claude.sh --out /tmp/forge-out --max-rounds 3` |

**The inputs are the Phase 0 originals, not new texts.** The 20 Korean
requirement sentences were extracted verbatim from the frozen table in
`corpus/requirements/README.md` into one `.txt` file each (id = row number).
No requirement was reworded, added, or dropped.

## BatchReport (verbatim)

```
batch: 20 item(s)
first-pass: 20/20 valid (100%) — after round 1, before any repair
rounds: 1 used, 3 allowed
  round 1: no causes
result: all 20 item(s) valid
```

Exit code 0. Wall time **520.68 s** (~26 s per item, sequential).

## The numbers, stated separately per §5.1

- **Batch size:** 20
- **First-pass rate:** 20/20 (**100%**) — measured after round 1, before any
  repair. This is the statement about the generator.
- **Round count:** 1 used of 3 allowed. This is the statement about the
  oracle: no repair round was needed, so none ran.
- **Root causes found:** none. Round 1 produced zero error-level findings
  across the batch.
- **Persistent failures:** none; nothing to analyse under the failure clause.

**Cross-check against the conformance oracle.** All 20 generated files were
also run through `omniidl -bdump` (the Phase 0 oracle) after the batch:
**20 pass, 0 fail.** On this batch S4 and omniidl agree, which is evidence
about S4's gate on these files, not a proof of equivalence.

## Why 100% is not a refutation of Phase 0's 65% — read this before quoting

Phase 0 measured 65% first-pass → 100% after one repair round. This run
measured 100% first-pass on the same requirement texts. The two numbers are
**not directly comparable**, for three reasons in decreasing order of weight:

1. **The prompt carries the codified rule.** Phase 0's dominant root cause —
   case-insensitive identifier clashes, all 7 of its failures — was codified
   into CLAUDE.md, and `gen_claude.sh` quotes that rule (with its examples)
   in every generation prompt, as the task for this run specified. Phase 0's
   generator had no such warning. So this run does not re-measure the naive
   generator; it measures the generator *after* the codify step. That the
   dominant cause produced zero occurrences is the codify loop doing what it
   claims — visible in the outputs, e.g. R03 (a target tracker with a
   position, the exact `Position position` trap) chose `GeoPoint location`,
   and the smoke-test output spontaneously renamed `ping()` to `send_ping()`
   inside `interface Ping`, citing the rule.
2. **Different gate in the loop.** Phase 0 gated on omniidl; this run gated
   on S4, with omniidl only as a post-hoc cross-check (which agreed, 20/20).
3. **Probably a different model version.** Phase 0's generator model was not
   pinned in the repo; this run used whatever `claude -p` resolves to today
   (self-reported `claude-fable-5`).

## Honesty caveats

- **Same model family throughout — the number is indicative.** The generator
  is a Claude model; S4's fix-hints were written by a Claude model; this
  report's author is a Claude model. Per the honesty rules and PLAN §8, 100%
  is an indicative figure, not a clean benchmark, until the frozen benchmark
  with a hold-out subset and an independent harness exists.
- **The repair path is still unmeasured with a real model.** Because round 1
  was clean, `run_batch`'s repair rounds and `gen_claude.sh`'s `$2` branch
  (repair prompt appended verbatim per §3.3) never executed against a real
  generator in this run. Their mechanics remain covered only by the crate's
  scripted-fake tests. A 100% first pass is good news that leaves the
  self-repair loop's real-model behaviour exactly as unmeasured as before.
- **Sequential, single run.** One pass, items generated in name order, no
  retries; no variance estimate. 20 items is the same n as Phase 0.
- `spikes/idl_lint.py`, which CLAUDE.md names as the pre-oracle lint, is not
  present on this branch, so it was not run; the identifier-clash class it
  targets is checked by S4 itself (`identifier-case-clash`,
  `enclosing-scope-clash`, `inherited-clash` rules).

## Codify (proposed, not applied)

No new root cause emerged, so there is nothing to codify from a failure.
Two things are worth codifying from the *absence* of one:

1. **The prompt constraint is now load-bearing — pin it.** The 65→100 delta
   is (most plausibly) the CLAUDE.md case-clash rule quoted in the prompt.
   `gen_claude.sh` is therefore part of the measured system: prompt changes
   should be treated like oracle changes — versioned, and any first-pass
   number reported alongside the prompt that produced it. Proposed rule:
   *a first-pass rate is meaningless without the generator prompt hash next
   to it.*
2. **The repair loop needs an adversarial batch to be measured at all.** If
   the codified prompt keeps first-pass at 100%, the §3.3 repair path will
   never run in production batches and will rot unmeasured. Proposed: a
   deliberately trap-laden requirement set (negative-corpus style — naming
   traps the prompt does *not* warn about, e.g. reserved words, `TypeCode`
   qualification, unions with clashing discriminator names) whose purpose is
   to force round 2, so the repair path gets a real-model measurement.

## Artifacts

- Generated IDL: `/tmp/forge-out/R01.idl` … `R20.idl` (not committed, per
  the run plan; `corpus/requirements/generated/` keeps the Phase 0 set).
- Inputs: `corpus/requirements/inputs/` (committed with this note).
- Generator wrapper: `spikes/gen_claude.sh` (committed with this note).
