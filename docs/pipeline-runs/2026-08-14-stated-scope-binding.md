# Stated-scope binding — D005 option C, landed (2026-08-14)

`docs/decisions/D005-contract-stability.md` is APPROVED and its first adopted
item is option C: **a scope-shaped literal token the requirement states must
survive to the `//@ ai_authz` S3 emits, checked by string equality with no model
in the loop.** This record is that change and its measurement.

The finding it answers is Cause A(3) of
[`2026-08-14-end-to-end.md`](2026-08-14-end-to-end.md): a regeneration over an
unchanged requirement with unchanged prompts passed every gate 1/1 and asked for
`parkinglot.barrier.open` where the requirement says `gate:operate`. An identity
provider issuing the stated scope against such a contract refuses **every
legitimate caller**, the refusal is well-formed and correctly audited, and it
reads as a permissions misconfiguration rather than a generation defect. It was
caught only because the *names* drifted too; a regeneration that kept every name
and changed only `ai_authz` would have passed all eight hops green.

Footprint: `crates/orbweaver-forge/` and this file. No other crate, no
`spikes/run_checks.sh`, no corpus file.

## What was built

| | |
|---|---|
| The predicate | `ingest::scope_shaped` — what "scope-shaped" means, with its non-claims in its own doc comment |
| The binding | `ingest::Brief::stated_scopes()` — operation name → the literal token |
| The rule | `annotate::RULES[11]` = `s3/authz-not-the-stated-scope`, a prompt constraint **and** a check |
| The check | `annotate::stated_scope_findings`, `annotate::check_against_brief` |
| The channel | `Stage::begin_item`, `CommandStage::with_briefs`, `annotate::stated_scopes_block` |
| The obligation | `s3/unanswered-question`, Advice — D005's compensating instrument, as far as a crate can carry it |

### Where the binding lives, and why there

**In the brief, not in the requirement text.** `Brief::stated_scopes()` returns a
pair only when all three of these hold:

1. S1 recorded the token in `OperationSketch::authz`;
2. the token is `scope_shaped`;
3. the token occurs **verbatim in `Brief::requirement`**, which S1 copies from
   the requirement.

Condition 1 is why the token is correctable: the brief is the artifact a human
can edit *before any IDL exists*, and deleting or fixing an `authz` there is a
decision somebody made and a file somebody can read. Re-extracting from the
requirement text on every run would put the binding beyond correction — a wrong
demand would then be un-overridable except by editing the requirement, which is
the one input that is supposed to be fixed.

Condition 3 is why it is a binding on a *stated* token rather than on a model's
opinion. S1 is free to record a permission it composed for an operation the
requirement scopes only in prose; that record is useful, is read by S2, and
binds nothing.

S1's prompt gained one constraint to keep the token bindable: `"authz"` is the
permission **as the requirement states it, copied verbatim**. A token S1 tidies
into a house style is a token nothing downstream can bind, and the tidying would
be invisible.

### What "scope-shaped" is, and what it does not claim

Two or more `[a-z][a-z0-9_-]*` segments joined by `:` or `.`, ASCII, at most 100
bytes. Accepts `gate:operate`, `bank.transfer.write`, `echo:blob`,
`accounts.write`, `parkinglot.barrier.open`, `api.v2`. Rejects `admin` (no
separator), `GATE:OPERATE` and `Gate:operate` (upper case), `gate/operate` (a
slash), `gate:` and `:operate` (an empty segment), `1.0` and `v1.0` (versions),
`읽기:권한` (non-ASCII), `gate:operate x` (a phrase).

Stated as non-claims, because a rule advertised beyond its reach is a rule
people learn to skim:

- **It does not claim to recognise every scope.** Bare words, upper-case
  conventions, slash-separated permissions and non-ASCII tokens are all real and
  all invisible to it. A requirement whose permission is `운영자 권한` gets no
  binding at all. That is silence, not a pass.
- **It does not claim every token it accepts is a scope.** `config.yaml`,
  `api.example.com` and `main.rs` are scope-shaped. This is why the predicate is
  never used alone: a filename must *also* have been recorded by S1 as an
  operation's permission and appear in the requirement before it binds anything.
- **It does not claim the scope is correct.** A model that invents a plausible
  permission for an operation the requirement never mentioned writes a token
  nobody stated, and nothing here looks at that.
- **It does not claim to know which operation should carry the token when
  identifiers move.** S2 may rename, and the rule's primary form is deliberately
  name-independent (below).

### What the check does — two ways to fire, one rule, one finding per token

- **absent** — the token is the value of no `ai_authz` anywhere in the file.
  This is the measured drift, and it is caught however the operations were
  renamed, because the token is compared against the file's whole set of scopes
  rather than against one operation. This is the form that matters: it survives
  S2's rename, which D005 refuses to revoke.
- **misplaced** — an operation whose name still matches the brief's carries a
  *different* scope. This is D005's decisive case: every identifier kept, only
  `//@ ai_authz` changed, all eight hops green today.

Silent when the brief is absent (S3 run alone over hand-written IDL), when the
recorded token is not scope-shaped, when the requirement does not state it, and
when the file's syntax is already broken.

### The channel — S3's second input

D005 names this as option C's honest cost, and it is one method with a default
plus one constructor:

- `Stage::begin_item(&mut self, id)` — a default no-op the batch loop calls
  before each item in both phases, so a stage that needs a second artifact can
  find it. No existing implementation changed.
- `CommandStage::with_briefs(&workspace)` — S3 reads `<id>.brief.json` beside
  its `.idl` input. `forge-pipeline` wires it for S3 and nothing else.
- The producer is handed the scopes as a block appended to the prompt file it
  already reads, so `spikes/e2e/producer.sh` needed no change. The prompt
  *constraints* remain the crate constant — `--print-prompt s3` still prints
  exactly that, because a first-pass rate is a measurement of the constant, and
  the block is per-item data the constant refers to.

## Measurement

Per §5.1, batch → oracle → repair → codify, with first-pass rate and round count
reported separately.

### The deterministic batch — fully measured

Written in one pass with no oracle consulted mid-pass: predicate, binding, rule,
prompt constraints (S1 and S3), plumbing, and thirteen tests. Then the whole
change was gated at once.

- **Batch size:** one change, 13 new tests, 4 files touched under
  `crates/orbweaver-forge/` plus one new test file.
- **First pass:** `cargo test -p orbweaver-forge` **140/140** and
  `cargo clippy --workspace --all-targets` **0 warnings**, both on the first run.
  `cargo fmt --check` produced the round's only cause.
- **Rounds: 2 used.** Round 1 cause: **rustfmt disagreed with three
  hand-wrapped expressions** — one cause, three sites, fixed by `cargo fmt`.
  Round 2 clean.
- Read this first-pass rate for what it is: it measures a change written by hand,
  not a generator. The generator-facing number is the live batch below.

Two hazards were caught while writing rather than by the oracle, and both are
recorded because the difference between them is the point:

1. **A `prompt_phrase` that spanned a line break in `S3_PROMPT`.** The codify
   test `every_rule_is_a_prompt_constraint_and_a_check` would have caught it as
   a red test — which is what that test is for, and evidence it still works.
2. **The token scanner shaving `Gate:operate` into `ate:operate`.** Nothing in
   the project would have caught this: a capitalised neighbour would have
   produced a token the requirement does not contain. Codified as
   `ingest::tests::the_scan_finds_a_stated_token_and_leaves_prose_alone`.

### The false-positive rate — the number that decides whether the rule survives

A check that fires on every contract because the requirement mentioned a word is
a check people route around. Measured, and reproducible from
`cargo test -p orbweaver-forge --test stated_scopes -- --nocapture`:

| item set | items | fires | correct |
|---|---:|---:|---|
| `corpus/requirements/inputs/*.txt` (the frozen benchmark) | 20 | **0** | yes — none of the twenty states a scope-shaped token at all, so the rule **cannot** fire on any of them whatever S1, S2 and S3 write |
| the recorded parking contract, real bytes from `spikes/e2e/recorded/` | 1 | 0 | yes |
| the same contract, live-regenerated through S3 today | 1 | 0 | yes |
| the drifted contract, reconstructed from the run record's table | 1 | 1 | yes — `gate:operate` absent |
| the recorded contract with only `ai_authz` changed | 1 | 1 | yes — the case that passes all eight hops today |
| unit fixtures: token kept but operation renamed; token not scope-shaped; token S1 composed; no brief at all; token on the wrong operation | 5 | 1 | yes — only the last one, and it is the misplaced form |

**False positives: 0 of 29 items. Over the frozen requirement corpus: 0/20.**

The 0/20 is also a limit worth stating plainly rather than celebrating: **the
frozen benchmark contains no requirement that states a scope**, so it cannot
exercise this rule at all. The corpus addition that would fix it —
one requirement stating a permission token literally — is outside this change's
footprint and is recorded below as a finding.

### The live batch — n = 1, indicative

One S3 call through the committed producer (`spikes/e2e/producer.sh`,
`claude -p`), over the recorded brief and draft, with the new prompt block and
the new gate:

```
range: S3 annotate → S4 validate over 1 item(s), 2 repair round(s) allowed per stage
S3 annotate: 1 item(s)
  first-pass: 1/1 valid (100%) — after round 1, before any repair
  rounds: 1 used, 2 allowed
    round 1: no causes
  result: all 1 item(s) valid
S4 validate: 1 item(s)
  first-pass: 1/1 valid (100%) — after round 1, before any repair
```

The prompt file the producer read carried
`SCOPES THE REQUIREMENT STATES … open_entry_gate: gate:operate`, and the emitted
contract carries `//@ ai_authz: gate:operate`.

**Read the 1/1 as nothing.** A batch of one is not a rate; it says that nothing
went wrong once. The producer, the gates and this record are all the same model
family, so per PLAN §8 it is **indicative**. What the run does establish
deterministically is that the channel is connected end to end: prompt file →
producer → artifact → gate.

## UNMEASURED, with reasons

- **Any rate for the model under the new prompt.** One requirement, one call.
  No variance, no hold-out, no second producer. D005's own §*What measurement
  would confirm or refute this* asks for N ≥ 20 regenerations; that is a budget
  to approve, not something to discover inside this change.
- **Whether S1 copies a stated token verbatim under the amended `S1_PROMPT`.**
  Zero live S1 runs here. The constraint is written and checked as a prompt
  constraint only; its effect on S1's output is unmeasured.
- **Whether the rule ever fires on the frozen benchmark.** It cannot: 0/20
  requirements state a scope. The rule's true-positive behaviour is measured
  only on the parking material and on unit fixtures.
- **The second run's actual bytes.** D005 records that they were never
  committed and that re-running produces a third contract. The drifted fixture
  in `tests/stated_scopes.rs` is **reconstructed from the run record's six-row
  table** and is labelled as such in the file. It reproduces the class, never
  the instance.
- **Anything a foreign oracle would say.** `omniidl` and `contract-check`
  accepted both contracts in the original run and neither reads a brief; nothing
  here changes that, and no foreign check was re-run for this change.
- **The end-to-end driver under the new gate.** `spikes/` is outside this
  footprint, so `spikes/end_to_end.sh` was not re-run; the S3 path it replays
  passes no brief and is therefore unaffected by construction, which is an
  argument and not a measurement.

The quality gate itself was measured: `cargo fmt --check` clean,
`cargo clippy --workspace --all-targets` 0 warnings,
`RUSTFLAGS="-D warnings" cargo test -p orbweaver-forge` 140 passed,
`cargo test --workspace` 1090 passed / 0 failed.

## What this does not fix

Stated in the same breath as the number, because the number invites the wrong
conclusion:

- **It binds one *stated* token.** A requirement that names no permission, or
  names it in prose, gets nothing. Most requirements are that requirement —
  0/20 of the benchmark states one.
- **It cannot see an invented scope on an operation the requirement never
  mentioned.** The contract may carry any number of scopes nobody asked for;
  only the stated one is bound.
- **It is a consistency check and not a correctness check.** No determinism
  check catches a *consistent* misreading. A stable, reproducible,
  gate-passing contract that reads the requirement wrongly is untouched, and
  D005 warns that stabilising regeneration makes such a misreading **durable**
  while removing the disagreement that used to expose it.
- **It does nothing about the other two harms.** Identifier drift, the exposure
  allowlist and the servant that stops compiling are option B's subject, and B
  is the next batch.

### The obligation D005 attached to this change

D005: *"the compensating instrument cannot be a green check — it has to be that
a brief's open questions get **read**, by a person, before the contract is
registered. That obligation belongs in whatever change lands these options."*

What a crate can do is small and is done: while the brief is in hand — S3 is the
last stage that holds one, since S4 and S5 see only IDL — every unanswered
`open_question` is carried into S3's report as `s3/unanswered-question` at
Advice. The recorded parking brief has ten, and the test asserts all ten travel.
It blocks nothing and proves nothing. **The instrument is the person; this only
makes the questions impossible not to see at the last stage before
registration.** A gate that claimed to discharge this obligation would be the
green check D005 says it must not be.

## Findings, not changes

Outside this footprint, reported precisely and not made:

1. **`corpus/requirements/` cannot exercise this rule.** None of the twenty
   requirements states a scope-shaped token. One added requirement that names a
   permission literally — the parking sentence is a ready-made model — would
   give the frozen benchmark a case where the binding is live, and would make
   the 0/20 above a measurement of *precision* rather than of *absence*. Corpus
   additions belong with the change that motivates them; this change's footprint
   excludes `corpus/`, so it is named here instead of landing silently later.
2. **`spikes/end_to_end.sh` replays S3 without a brief**, so its S3 hop does not
   exercise the binding. Pointing the replay at `spikes/e2e/recorded/
   PARKING.brief.json` would make hop 2 measure the rule on the real recorded
   material.
3. **Option B is unlanded**, as D005 orders it: `validate_against` is still not
   called by any pipeline gate, and the differ still does not read annotations —
   so a scope drift remains invisible to `idl-diff` even after this change.

## Harness — recommended, not applied

`spikes/run_checks.sh` is outside this footprint, so nothing below was added to
it. The deterministic half needs no model and no network and belongs in the
gate; the live half does not, for the reason the 2026-08-13 record gave (a model
call and an API key do not belong in a gate).

```sh
# D005 option C: the scope a requirement states, bound to the ai_authz S3 emits.
# Deterministic, no model, no network. Fires on the run record's drifted
# contract and stays silent on the recorded one. Skipping is a FAILURE.
hr "forge — stated-scope binding (D005 option C)"
# Capture, then match: never pipe a producer into `grep -q` (CLAUDE.md).
scopes=$(cd "$ROOT" && cargo test -q -p orbweaver-forge --test stated_scopes 2>&1)
case "$scopes" in
    *"test result: ok."*) pass "stated-scope binding: $(printf '%s' "$scopes" \
        | grep -oE '[0-9]+ passed' | head -1)" ;;
    *) fail "stated-scope binding"
       printf '%s\n' "$scopes" | tail -20 ;;
esac
```

It reads `spikes/e2e/recorded/` and `corpus/requirements/inputs/`, so it must run
from the repository root; it binds no port and starts no fixture, so it needs
nothing from the harness lock and may run before it.

## Reproducing

```sh
cargo test -p orbweaver-forge --test stated_scopes -- --nocapture   # the numbers above
cargo run -q --bin forge-pipeline -- --print-prompt s3              # the constraints, without the per-item block
```

The live batch, for anyone with a producer:

```sh
mkdir -p /tmp/s3ws && cp spikes/e2e/recorded/PARKING.brief.json spikes/e2e/recorded/PARKING.idl /tmp/s3ws/
cargo run -q --bin forge-pipeline -- --out /tmp/s3ws --from s3 --to s4 \
    --annotate "$PWD/spikes/e2e/producer.sh" --max-rounds 2
```

Expect a different contract each time in every respect the rule does not bind —
that is Cause A and it is not fixed — and `//@ ai_authz: gate:operate` in all of
them, or a refusal that names the token.

## 요약 (Korean summary)

이 기록은 영어가 정본이다. `docs/pipeline-runs/`의 선례를 따라 **결론만** 옮긴다.

- **D005 승인 사항 중 C를 구현했다.** 요구사항이 문자 그대로 적은 스코프 토큰이
  S3의 `//@ ai_authz`까지 살아남는지를 **모델 없이 문자열 동일성으로** 검사한다.
  규칙 이름은 `s3/authz-not-the-stated-scope`이며, `annotate::RULES`에 들어가
  프롬프트 제약과 검사 양쪽으로 강제된다.
- **결속이 사는 곳은 브리프다.** S1이 `authz`로 기록했고, 스코프 형태이며,
  `Brief::requirement`에 **그대로** 등장하는 토큰만 결속한다. 브리프는 IDL이
  생기기 전에 사람이 고칠 수 있는 산출물이므로, 잘못된 요구는 사람의 편집으로
  뒤집을 수 있다.
- **오탐 0건.** 고정된 요구사항 코퍼스 20건 중 스코프 형태 토큰을 포함한 것은
  **0건**이므로 규칙은 아예 발화할 수 없다. 전체 29개 측정 항목 중 발화는 3건이며
  모두 의도된 참양성이다. 다만 이는 코퍼스가 이 규칙을 **측정할 수 없다**는 뜻
  이기도 하며, 요구사항 1건 추가가 필요하다(발견으로 기록, 이번 변경 범위 밖).
- **기록된 계약에는 침묵하고, 표류한 계약은 거부한다.** 커밋된 첫 실행 산출물
  (실제 바이트)에는 침묵하고, 실행 기록의 표로 재구성한 두 번째 계약과 **이름을
  전부 유지한 채 스코프만 바꾼** 재생성은 거부한다. 후자는 오늘 8개 홉을 전부
  통과하는 바로 그 경우다.
- **모델 측정은 1건(n=1)이며 지시적이다.** 커밋된 producer로 S3를 한 번 실행해
  1/1 통과, `gate:operate` 유지를 확인했다. 비율이 아니다.
- **고치지 못하는 것.** 요구사항이 스코프를 말하지 않으면 아무것도 결속하지 않고,
  요구사항이 언급조차 하지 않은 연산에 모델이 그럴듯한 스코프를 지어내는 것은
  잡지 못한다. 결정성 검사는 일관성 검사이지 정확성 검사가 아니다.
- **D005가 이 변경에 붙인 의무**는 초록색 검사로 대신할 수 없다. 크레이트가 할 수
  있는 최소한으로, 브리프의 미해결 질문을 S3 리포트에 `s3/unanswered-question`
  (Advice)로 실어 등록 직전 단계에서 보이게 했다. 읽는 것은 사람의 몫이다.
