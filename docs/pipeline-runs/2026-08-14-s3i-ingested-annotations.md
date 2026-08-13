# Pipeline run — S3i, annotating interfaces nobody here wrote (2026-08-14)

The first batch through **S3i**: SIDL annotation for interfaces that arrived by
*ingestion* rather than by generation. `crates/orbweaver-registry/src/ingest.rs`
landed the wire half and stated the gap in its own documentation:

> **No SIDL annotations exist on the wire.** The IR carries no `ai_effect`,
> `ai_authz` or `ai_desc`, so every ingested operation arrives with an empty
> annotation map. The guard's destructive-approval gate and scope checks have
> nothing to key on […]

That is the broken link this run closes. An ingested interface could not be
exposed to an agent safely — not because a switch was off, but because the gates
are keyed on annotations that do not exist. S3i writes them, and **the honesty
constraint is the substance of the stage, not its plumbing**: an inferred
annotation is a claim about somebody else's service, made by a model reading
names and types, and keying an authorization decision on a guess is exactly the
failure the whole trust boundary exists to prevent.

## Setup

| | |
|---|---|
| Subjects | 19 interfaces / 51 operations, ingested from `corpus/golden/{09,10,11,16,19,22,23,24,25}` |
| Ingestion | `RepositoryServer` facade on loopback → `registry::ingest::ingest` — **our own IR served over a real socket**, labelled a self-consistency stand-in wherever it prints |
| Producer | `claude -p` (Claude Code CLI), via a two-argument wrapper in the `CommandStage` mould |
| Model | self-reported by the CLI; not independently verified |
| Prompt | `orbweaver_forge::infer::S3I_PROMPT`, written to disk by the crate and passed as `FORGE_PROMPT` |
| Gate | `orbweaver_forge::infer::gate` — 10 rules, each also a prompt constraint |
| Command | `sidl-infer --idl <files> --producer <w> --out <dir> --max-rounds 3 --json` |

The subjects are **ingested**, not loaded: the IDL is served by the facade and
consumed over IIOP, so every entry comes back `Origin::Ingested` with an empty
annotation map. The stand-in is our encoder against our decoder and is not a
cross-ORB claim; what it reproduces faithfully is the only thing S3i depends on
— that the wire carries no annotations, whoever is on the other end.

`sidl-infer` reports under `S3 annotate` because S3i **is** the annotation
stage: same position, same kind of output, a different input medium. S3 and S3i
are alternatives and never both, which is why S3i has no sixth `StageId`.

## Round 1 — verbatim

```
Evidence floor (deterministic, no model): 26/51 operation(s) (51%) carry a name
the checker can read nothing into.

S3 annotate: 19 item(s)
  first-pass: 13/19 valid (68%) — after round 1, before any repair
  rounds: 3 used, 3 allowed
    round 1: [si/effect-without-evidence] 4 item(s): IDL:fault25/Vault:1.0, IDL:gc24/Gauge:1.0, IDL:moe/ExpertLoader:1.0, IDL:moe/enterprise/ModelFactory:1.0
    round 1: [si/evidence-not-in-subject] 3 item(s): IDL:moe/enterprise/ComposedModel:1.0, IDL:moe/enterprise/EnterpriseExpert:1.0, IDL:moe/enterprise/ModelFactory:1.0
    round 2: [si/evidence-not-in-subject] 1 item(s): IDL:moe/ExpertLoader:1.0
    round 3: [si/effect-without-evidence] 1 item(s): IDL:moe/ExpertLoader:1.0
  result: NOT all valid — 1 item(s) still failing after 3 round(s):
    IDL:moe/ExpertLoader:1.0: rejected by the stage gate

unknown rate: 33/47 operation(s) (70%)
```

Exit code 1 — **the batch did not converge.** The round limit was spent, one
item was still failing, and that is the headline rather than a footnote.

### The numbers, stated separately per §5.1

- **Batch size:** 19 interfaces, 51 operations.
- **First-pass rate:** 13/19 (**68%**) — the statement about the producer.
- **Round count:** 3 used of 3 allowed, **not converged** — the statement about
  the gate.
- **Unknown rate:** 33/47 (**70%**) over the 18 items that reached a verdict;
  the failing item's 4 operations are excluded, so this denominator is not 51.
- **Root causes:** two.

## Root causes, clustered

| # | Cause | Items | What it was |
|---|---|---|---|
| A | `si/effect-without-evidence` | 4 | **the gate refusing a correct answer** |
| B | `si/evidence-not-in-subject` | 3 | the producer quoting text the subject does not contain |

### A is the interesting one, and it is a defect in the checker

The rule said: if the operation's name contains no verb the checker recognises,
an effect claim is refused and `unknown` is the only accepted answer. Applied to
the batch, it refused this:

```json
{"operation": "evict", "effect": "destructive", "authz": "moe.expert_loader.evict",
 "evidence": "\"evict(in CapabilityId id) -> void\" — the name 'evict' names a
 removal, and the only input is \"CapabilityId id\"."}
```

`moe::ExpertLoader::evict` **destroys everything routed to an expert.** The
producer read it correctly. The checker refused it because `evict` is not on a
26-word list — and this project has already written down, in
`crates/orbweaver-forge/src/annotate.rs`, that this exact operation is the one a
verb heuristic cannot catch:

> Two of that batch's twenty — `moe::ExpertLoader::evict` among them, which
> destroys everything routed to an expert — contain no mutating verb, so
> `mutating_verb` would never have found them.

So the rule turned a known limitation of a heuristic into a **refusal of the
right answer**, and the item oscillated: round 2 complied with `unknown` and
tripped B while rewording its evidence; round 3 went back to `destructive` and
tripped A again. Reproduced deterministically on the single interface
(`--seed IDL:moe/ExpertLoader:1.0`), 0/1 first-pass, same three rounds.

The cause underneath is one sentence: **a word list is evidence of presence and
never of absence.** "The checker recognises no verb" is not "the name says
nothing" — and asserting it is the identical error this module's own
documentation denounces, committed by the module.

Adding `evict` to the list would have been the item-by-item patch. The next
interface says `quiesce`, `drain`, `decommission`.

## Repair — one fix per cause

**A → the rule was rewritten to check the thing a checker can actually check.**
`si/effect-without-evidence` became `si/unnamed-verb`, and it now refuses only a
claim that **points at nothing in the operation's own name**. A `destructive`
claim quoting the word it reads — `the name 'evict' names a removal` — is
*kept*. Alongside it, `apply` writes a new mark, `inferred_basis`, computed by
the checker from the signature and never taken from the producer:

| `inferred_basis` | means |
|---|---|
| `declined` | the stage claimed no effect |
| `recognised-verb: "delete"` | the name contains a verb the checker knows |
| `unrecognised-verb — …` | the claim is the producer's reading alone; a human should read the name themselves |

This is strictly more information than the refusal produced, and it is honest in
both directions: the claim survives, and the reviewer is told exactly how much
of it the machine could corroborate.

**B needed no code change.** The rule is correct — a proposal citing a parameter
the description does not contain is resting on something invented — it fired, it
produced a repair prompt, and the repair round fixed every affected item. That
is the loop working rather than a finding.

## Codify

- `si/unnamed-verb` is in `infer::RULES`, which pairs every rule with a phrase
  `S3I_PROMPT` must contain; `every_rule_is_a_prompt_constraint_and_a_check`
  fails if either half goes missing, so this cause cannot come back silently.
- The prompt now states the limitation to the producer in as many words: *"The
  checker keeps a short list of mutating verbs and it is INCOMPLETE: 'evict',
  'quiesce', 'drain' and 'retire' are not on it."*
- Two tests pin both directions —
  `a_claim_that_points_at_nothing_in_the_name_is_refused` and
  `a_claim_that_names_the_word_it_reads_is_kept_and_marked`.
- `inferred_basis` is a column in the worksheet, so the recovered claims are
  visible as the rows most worth a reviewer's time.

## Round 2 — after the repair, verbatim

```
S3 annotate: 19 item(s)
  first-pass: 18/19 valid (95%) — after round 1, before any repair
  rounds: 2 used, 3 allowed
    round 1: [si/evidence-not-in-subject] 1 item(s): IDL:moe/enterprise/EnterpriseExpert:1.0
    round 2: no causes
  result: all 19 item(s) valid

unknown rate: 32/51 operation(s) (63%)
```

Exit code 0. The loop terminated on *"a round yields no new root causes"*, not
on a round limit.

| | Round 1 | Round 2 |
|---|---|---|
| First-pass | 13/19 (68%) | **18/19 (95%)** |
| Rounds used / allowed | 3 / 3, **not converged** | 2 / 3, converged |
| Unknown rate | 33/47 (70%) | **32/51 (63%)** |
| Causes | `si/effect-without-evidence` ×4, `si/evidence-not-in-subject` ×3 | `si/evidence-not-in-subject` ×1 |

### What the repair recovered

Six `destructive` claims that round 1's rule would have forced to `unknown`:

| Interface | Operation | Proposed scope |
|---|---|---|
| `fault25::Vault` | `forget` | `vault.entries.delete` |
| `fault25::Vault` | `rotate` | `vault.rotate` |
| `gc24::Gauge` | `scale_all` | `gc24.gauge.write` |
| `moe::ExpertLoader` | `evict` | `moe.expert_loader.evict` |
| `moe::enterprise::ModelFactory` | `deploy` | `moe.models.deploy` |
| `moe::enterprise::ModelFactory` | `retire` | `moe.models.retire` |

Six of the nineteen `destructive` claims — **32%** — came from words no verb
list in this repository contains. Every one is marked `unrecognised-verb`.

### The final distribution

```
effect:  19 destructive, 32 unknown          (51 operations)
basis:   32 declined
         13 recognised-verb ("bind" ×2, "clear", "create" ×2, "drop",
            "register" ×2, "reset" ×2, "set_", "store", "update")
          6 unrecognised-verb
```

**`si/ungating-claim` never fired, in either round.** Not one proposal across
102 model outputs claimed `read_only`, `idempotent` or `safe`. As in the
2026-08-13 record, the prompt constraint prevented the combination rather than
the check catching it, which is the more valuable of codification's two jobs —
but it is one sample, and the check is what makes it a guarantee rather than a
tendency.

## The design, and the argument for it

### Marking: an inference never occupies a key a gate reads

`orbweaver_mcp::policy` keys on `ai_effect` and `ai_authz`. S3i writes
`inferred_effect`, `inferred_authz` and `inferred_desc`, plus `inferred_source`,
`inferred_evidence`, `inferred_basis` and `inferred_status`. The consequence is
deliberate and is **measured, not assumed**
(`an_inferred_scope_enforces_nothing_at_the_policy_gate`): with the interface
allowlisted, `Exposure::check_call` on an operation carrying
`inferred_authz: legacy.tracks.admin` and `inferred_effect: destructive`
**succeeds** — no caller, no approval.

That is the design, not a hole in it. Two reasons:

1. **`Provenance` must be answerable from the annotation map alone.** A bare
   `ai_effect: destructive` cannot say whether a person wrote it. The marks are
   annotations rather than a side table precisely so they survive a `Registry`
   clone, a JSON round trip and a description handed to an agent — measured in
   `marks_round_trip_through_the_registry`.
2. **An enforced guess is worse than no guess.** An inferred `ai_authz` would
   make the bridge demand a permission name a model invented about somebody
   else's service. That looks like a control and is theatre: the deployment's
   identity provider means nothing by `moe.expert_loader.evict`, because nobody
   in that deployment chose it.

### Approval: the only transition, and it is a human act with a name on it

`infer::approve` promotes `inferred_X` → `ai_X`, refuses an empty approver
(`ApproveError::NoApprover`), and **keeps every `inferred_*` key**, writing
`inferred_status: approved by "ops-lead@example" on 2026-08-14`. The mark
travels for as long as the annotation exists — in the registry, in what an agent
reads, in an audit line. Approving `destructive` hands the operation to the
approval gate rather than through it; approving `unknown` leaves the human in
the loop, because the policy gate treats an unrecognised `ai_effect` as needing
approval.

### The asymmetry: a value that closes a gate may be inferred, one that opens a gate may not

`destructive` is proposable. `read_only`, `idempotent` and `safe` are refused at
inference (`si/ungating-claim`) **and** at approval (`ApproveError::Ungating`) —
both ends, because the worksheet is a text file and a rule enforced at one end
is a rule with a way around it.

The argument is that inference errors are not symmetric. A wrong `destructive`
costs a human one approval click. A wrong `read_only` **removes** the approval
gate: `policy::destructive_effect` returns `None` for it and an agent calls the
operation with no human in the loop. Since the evidence is a name, only one of
those two errors may be reachable. A person who knows the service can author
`read_only` themselves — that is a different act from approving a machine's
guess, and it is the act that carries the responsibility.

### Visible, not merely default-off

`worksheet()` emits **one row per ingested operation**, including operations no
proposal covers — which is the state an ingested interface starts in, and
exactly the state a default-off design loses (nothing is set, so nothing is
wrong, so nothing is shown). `exposure_refusal()` is the single question an
allowlisting step asks, and it refuses per interface rather than per call,
because a refusal arriving at call time arrives after the tool was already
advertised to an agent. `unmarked_gate_keys_on_an_ingested_entry_are_reported`
covers the one shape the design forbids: an ingested entry carrying `ai_*` keys
with no provenance mark — a remote description in a reviewed contract's clothes.

**추론 주석은 남의 서비스에 대한 주장이지 사실이 아니다.** 그래서 게이트가 읽는
키(`ai_effect`, `ai_authz`)를 절대 차지하지 않고, 게이트를 **여는** 값은 제안도
승인도 할 수 없으며(양쪽 끝에서 거부), 근거가 없으면 답은 `unknown`이다. 승인은
이름과 날짜가 붙은 사람의 행위이며, 승인 후에도 `inferred_*` 표시는 남는다.
승인 전 상태는 "설정되지 않음"이 아니라 워크시트의 **한 줄**로 보인다.

## What an inference cannot know

Stated plainly, because the run above cannot be read correctly without it:

- **The name says nothing about whether the operation writes to a database, and
  no amount of prompting fixes that.** `settle` may post a ledger row or format
  a string. `get_report` may bill the caller. `ping` may reset a watchdog that
  fails a cluster over. 63% of this batch's operations got `unknown` because
  `unknown` is the true answer, not because the producer gave up.
- **A `destructive` claim is a guess in the safe direction, not a finding.** The
  six `unrecognised-verb` rows are the producer's reading of an English word.
  `retire` reads as destructive and could be a status flag.
- **The IR and the object are different peers.** Nothing binds a repository's
  description to what the server holding the object implements
  (`registry::ingest`), so even the *signature* the inference reads is the IR's
  claim rather than a fact.
- **Lying by omission stays invisible.** An IR reporting four of five operations
  is indistinguishable from an interface with four. S3i annotates what it was
  shown.
- **The unknown rate measures the producer's willingness to decline, not the
  interface.** A different model on the same 51 operations would produce a
  different rate. One run, one model, no variance estimate.

## Honesty caveats

- **Same model family throughout — the numbers are indicative.** The producer is
  a Claude model, the gate and its fix-hints were written by a Claude model, and
  this report's author is a Claude model. Per the honesty rules and PLAN §8,
  68/95 and 70/63 are indicative figures, not a clean benchmark.
- **The ingestion peer is our own facade.** Real ingestion machinery over a real
  socket, but our encoder against our decoder. A JacORB-served batch would test
  the wire half; it would not change anything S3i does, because the annotation
  map is empty either way. Not run here, and therefore **not claimed**.
- **`contract-check` was not run over S3i's output, and cannot be.** The S1/S3
  batch checked S3's output with it because S3 emits `.idl` text. S3i emits JSON
  proposals for interfaces that have no IDL file anywhere — that is the premise
  of ingestion — so there is nothing for `contract-check` to read. The
  equivalent independent check is `orbweaver-mcp`'s real policy gate, which
  `an_inferred_scope_enforces_nothing_at_the_policy_gate` drives directly. Said
  plainly rather than substituted for: this run has **one** independent oracle,
  where the S1/S3 run had two.
- **One run per cell.** Round 1 and round 2 are single passes over the same
  subjects. 68% → 95% follows a rule change that removed a rule which refused
  correct answers, so the direction is explicable, but nothing here supports a
  claim about the size of the difference.
- **The failing round-1 item was diagnosed by reproduction**, not by inference:
  `--seed IDL:moe/ExpertLoader:1.0` reproduced 0/1 first-pass and the same
  three-round oscillation, and the rejected artifact and repair prompt are what
  the diagnosis was read from. `sidl-infer` now persists both for every item it
  could not pass, which it did not do when round 1 ran — that gap is why the
  reproduction was needed.
- **The wrapper is not committed.** `spikes/` was outside this change's
  footprint. The recommended form is below and was deliberately not applied.

## Harness — recommended, not applied

`spikes/` and `run_checks.sh` were outside this change's footprint, so nothing
below was added. The deterministic half needs no model and belongs in the gate:

```sh
# S3i: ingestion through the facade, the marking, the exposure refusal, and the
# proof that an inferred scope enforces nothing at the real MCP policy gate.
# No model, no network beyond loopback.
hr "forge S3i — inferred annotations for ingested interfaces"
RUSTFLAGS="-D warnings" cargo test -q -p orbweaver-forge || fail "forge S3i tests"

# Every ingested interface must be refused for exposure until a human approves.
# A run that ingests and reports nothing awaiting review is a run whose marking
# silently stopped working — so an empty worksheet here is a FAILURE, not a pass.
sheet=$(mktemp -d)
cargo run -q -p orbweaver-forge --bin sidl-infer -- \
    --idl corpus/golden/22-moe-control-plane.idl --out "$sheet" >/dev/null 2>&1 \
    || fail "sidl-infer could not ingest the golden corpus"
rows=$(grep -vc '^#' "$sheet/inferred.todo.tsv" || true)
case "$rows" in
    0|"") fail "no worksheet rows: ingested operations awaiting a human went unreported" ;;
    *)    pass "$rows ingested operation(s) reported as awaiting a human" ;;
esac
grep -q 'approved=no' "$sheet/inferred.todo.tsv" \
    || fail "worksheet rows are not marked unapproved"
rm -rf "$sheet"
```

The model-facing half should stay out of `run_checks.sh` — it is ~20 minutes and
an API key — and run as a recorded batch like this one, with its numbers landing
here beside the prompt that produced them.

## Artifacts

- Proposals, rejected artifacts, repair prompts and `inferred.todo.tsv`: the
  run's `--out` directory (not committed).
- Prompt: `sidl-infer --print-prompt` reproduces it exactly; it is
  `orbweaver_forge::infer::S3I_PROMPT`, versioned with the checker that grades
  it.
- Subjects: reproducible with `sidl-infer --idl <files>` and no `--producer`,
  which runs the deterministic half and prints the model-facing numbers as
  UNMEASURED.
