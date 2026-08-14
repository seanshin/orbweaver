# Diffing a regeneration against what is registered — D005 option B, landed, and the benchmark repaired (2026-08-14)

`docs/decisions/D005-contract-stability.md` is APPROVED with the order **C
first, then B, framed by D, A rejected**. C landed in
[`2026-08-14-stated-scope-binding.md`](2026-08-14-stated-scope-binding.md). This
record is **B** — `orbweaver_forge::validate_against`, which already wraps
§5.3's differ and which the pipeline simply never called, wired into the S4
gate — plus the repair of a gap that record found in the benchmark itself.

Footprint: `crates/orbweaver-forge/`, `corpus/requirements/` and this file. No
other crate, no `spikes/run_checks.sh`.

## What was built

| | |
|---|---|
| The meaning of "registered" | `pipeline::Registered` — a directory of contracts the run is pointed at, with the three candidates argued in its doc comment |
| The absence of one | `pipeline::Baseline::{None, Contract, Unreadable}` — a first generation is silence; an unreadable baseline is a failure |
| The gate | `pipeline::ValidateStage::against` — S4 calls `validate_against` instead of `validate` where a contract is registered |
| The record | `pipeline::DiffOutcome` + `ValidateStage::outcomes` — what each item was compared against, and whether the comparison ran at all |
| Option D's frame | `ValidateStage::superseding`, `pipeline::record_supersede`, `<out>/superseded.tsv` |
| The seam | `forge-pipeline --registered <dir> [--supersede <reason>]` |
| The benchmark repair | `corpus/requirements/inputs-v2/` — the frozen twenty verbatim plus six requirements that state a permission |

## The judgement this change had to make: what "registered" means

D005 accepts B **"on the record that the registry of record does not exist
yet"**, and PLAN §5.3 says the same in its own words: *"'released' currently
means the file `idl-diff` is pointed at rather than a contract read from a
registry of record."* D003-B deferred durable storage. So the change had to pick
a meaning. Three candidates, and the argument is in
`ValidateStage`/`Registered`'s doc comments as well as here:

1. **The S5 catalog within a run — rejected.** `register()` builds its
   `Registry` *after* S4, out of the very artifacts S4 has just gated. Diffing
   an item against it compares a contract with itself and can never report a
   change; and `register`'s own documentation says the rows do not persist. **A
   baseline a run creates cannot constrain that run.**
2. **Nothing until a store exists — rejected.** That is D005's option E taken by
   omission. The differ, its verdicts, `validate_against` and `idl-diff` are all
   built and tested; withholding them until storage lands protects nobody, and
   the harm B answers is the one the 2026-08-14 end-to-end run actually measured
   (a servant that stopped compiling, an exposure allowlist resolving to
   `allow=0`).
3. **A directory of contracts the run is pointed at — adopted.**
   `--registered <dir>` resolves `<id>.sidl.idl`, or `<id>.idl` where nothing
   annotated it — the same resolution `Workspace::gated_artifact` already uses,
   so **a previous run's `--out` directory is a usable registry of record** with
   no second layout to learn. It is exactly what `idl-diff` already means by
   "released", so B introduces no second meaning of the word. It is auditable:
   every outcome names the file it was compared against. And it is the seam the
   durable store plugs into — when D003-B lands, `Registered::contract` is the
   only body that changes.

**A first generation is silence, not a refusal.** An id with nothing registered
under it has nothing to diff against, so nothing is demanded and the item passes
exactly as it would have before this change — asserted against the
baseline-free gate's own verdict, not merely described
(`a_first_generation_has_no_baseline_and_is_not_refused_for_it`). But the
absence is **counted and printed**, because "compared and clean" and "never
compared" must not look alike:

Real bytes from a two-item run — one regenerated over a registered contract, one
never registered before, no model involved:

```
S4 validate: 2 item(s)
  first-pass: 1/2 valid (50%) — after round 1, before any repair
    round 1: [evolution/BREAKING] 1 item(s): R01
  result: NOT all valid — 1 item(s) still failing after 1 round(s):
    R01: rejected by the stage gate
=== s4 R01
[evolution/BREAKING] 1 occurrence(s)
  IDL:m/Ledger:1.0: removed — the repository id is the contract identity, so removing
  or renaming it makes every existing reference to it unresolvable
  publish a new version of the interface instead of editing the released type in place
S4 §5.3: compared 1 item(s) against /tmp/ob-demo/reg; 1 had no registered contract
         (first generation — nothing to diff, nothing demanded); 1 carried a breaking change
S4 §5.3: annotations are not compared — a regeneration that keeps every identifier and
         changes only //@ ai_authz is compatible here (D005 option C covers that, at S3)
```

R02 — the item with nothing registered under it — passed without a word about
its baseline, which is the silence this section argues for; the run's *only*
statement about it is the count.

And a contract that *is* registered but cannot be read is an **error**, not
silence — the harness rule (*an unmeasured check is a failure, never a pass*)
applied to the gate itself, under `evolution/registered-unreadable`.

## What option B cannot see, stated plainly

**The differ reads no annotations.** It compares bases, operations, attributes,
`TypeCode`s and constant types and values, and never touches
`OperationSig::annotations`, where `ai_authz` lives. A regeneration that keeps
every identifier and changes only `//@ ai_authz: gate:operate` produces **zero
changes** and passes this gate. Measured on the recorded parking bytes, both
halves in one test: B silent, C refusing
(`option_b_is_blind_to_the_scope_that_option_c_binds`). By §5.3's own logic a
scope is not a wire change at all, so the table has no row for it and this
change does not add one. **That is why C landed first and why B does not
subsume it: the two see disjoint halves of the same regeneration, and only C
sees the half that fails silently in production.**

One near-miss, found while measuring and worth more than the headline. The
recorded contract states its scope **twice** — as `//@ ai_authz` and as
`const string GATE_OPERATE_PERMISSION = "gate:operate"`. The differ *does*
compare constant values, so a regeneration that moves both produces a
`constant value changed` change — which §5.3 calls *conditionally breaking* and
this crate maps to a **warning**, so the item still lands. The honest statement
of B's reach is therefore narrower than "it warns about scopes": it sees a scope
only when that contract's author chose to model it as a constant, which is a
property of one contract's style rather than a capability of the gate, and even
then it does not refuse (`a_scope_that_is_also_an_idl_constant_warns_and_still_lands`).

## D005's routine-approval warning, measured rather than repeated

D005: *"a full regeneration renames everything, so the gate fires on every id,
every time. An approval that is always given stops being a signal."* Measured on
the recorded contract with the module and interface renamed the way the second
run renamed them:

**One rename produced 12 breaking changes in a single item** — every type,
exception, typedef, constant and the interface itself, each `removed` because
the repository id is the contract identity. Reproducible:

```sh
cargo test -p orbweaver-forge --test registered_diff -- --nocapture a_full_regeneration
```

Nothing in this change makes that approval thoughtful. What it does is make it
**leave evidence**: `--supersede <reason>` downgrades the refusals and writes
every change the reason covered to `<out>/superseded.tsv`, one row per change.
An empty reason is not a declaration and is ignored; `--supersede` without
`--registered` is refused outright rather than accepted as a flag that does
nothing.

## Measurement

Per §5.1: batch → oracle → repair → codify, first-pass rate and round count
reported separately.

### The deterministic batch — fully measured

Written in one pass with no oracle consulted mid-pass: the `Registered`/
`Baseline`/`DiffOutcome` types, the gate, the supersede record, the CLI flags,
the six new requirements, and the tests. Then the whole change was gated at
once.

- **Batch size:** one change; 15 new tests (12 in `tests/registered_diff.rs`,
  2 in `tests/stated_scopes.rs`, and the CLI seam test inside the first); 6 new
  corpus requirements plus 20 copies; 5 source files touched.
- **First pass:** `cargo clippy --workspace --all-targets` **0 warnings** and
  `cargo test -p orbweaver-forge` **all green** on the first run —
  including every assertion about the six new requirements, which were written
  before the scanner was run over any of them. `cargo fmt --check` produced the
  round's only cause.
- **Rounds: 2 used.** Round 1 cause: **rustfmt disagreed with four hand-wrapped
  expressions** — one cause, four sites, fixed by `cargo fmt`. This is the same
  single cause the 2026-08-14 option C batch reported; it is a property of
  writing Rust by hand against this project's width, not a new defect. Round 2
  clean.
- Read this first-pass rate for what it is: it measures a change written by
  hand, not a generator.

One hazard was caught by measuring rather than by reasoning, and it changed a
claim: **the recorded contract also states its scope as an IDL constant.** The
first draft of this record said flatly that B cannot see a scope change. Running
the rename test printed `IDL:ParkingFacility/GATE_OPERATE_PERMISSION:1.0` among
the affected ids, which prompted the constant test above and the narrower
sentence in §*What option B cannot see*. The over-broad claim would have passed
every check in the project.

### The gate itself

| | |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `RUSTFLAGS="-D warnings" cargo test -p orbweaver-forge` | **154 passed**, 0 failed |
| `cargo test --workspace` | **1139 passed**, 0 failed |

## The benchmark could not exercise option C. It can now.

The option C record's own finding: *"`corpus/requirements/` cannot exercise this
rule. None of the twenty requirements states a scope-shaped token."* Its
false-positive measurement was **0 of 20** — a zero over a set that structurally
cannot produce a one, which measures **absence, not precision**.

**The frozen set was not touched.** `corpus/requirements/inputs/` is frozen
before generation on purpose and its twenty items are the denominator every
assumption-B number ever reported is over; adding to it would silently change a
figure earlier records quote while leaving those records unchanged.
`corpus/queries/` set the precedent for exactly this situation — `search-v1.tsv`
stayed frozen and `search-v2.tsv` was added beside it, containing every v1 line
**verbatim** plus the new cases, with both run and both reported.

`corpus/requirements/inputs-v2/` is that shape: `R01`–`R20` copied byte for byte
(a test asserts it, so a divergence cannot happen silently) plus **six** new
requirements, each naming the permission the way a requirement in its domain
would name it, written in one pass in the register the twenty use.

### The finding: three of six state a permission the rule cannot see

```
R21.txt: ["pharmacy.prescription.write"]
R22.txt: ["grid:breaker_control"]
R23.txt: ["billing.refund.approve"]
R24.txt: []
R25.txt: []
R26.txt: []
corpus/requirements/inputs-v2: 3/26 requirement(s) contain a scope-shaped token (0/20 frozen, 3/6 new)
```

| item | how the requirement states the permission | bound |
|---|---|---|
| R21 병원 처방 | `pharmacy.prescription.write` | yes |
| R22 배전 계통 | `grid:breaker_control` | yes |
| R23 결제 환불 | `billing.refund.approve` | yes |
| R24 영상 반출 | Korean prose — *보안 책임자 권한* | **no** |
| R25 학사 성적 | `ROLE_REGISTRAR` | **no** |
| R26 창고 로봇 | `warehouse/robot/estop` | **no** |

**The three that do not fire are the more valuable half.** `ingest::scope_shaped`
recognises one lexical convention — two or more lower-case ASCII segments joined
by `:` or `.`. Korean prose, the upper-case `ROLE_` convention every Spring
codebase uses, and slash-separated ACL paths are all ordinary ways to state a
permission and all invisible to it. A project whose house style is any of those
three gets **no binding at all** from option C, and gets it silently. The rule's
documentation already claimed this limit; the corpus now demonstrates it at a
rate — **50% of naturally-phrased permission statements in this set are outside
the rule's reach.**

It is recorded rather than patched. Widening the predicate to accept
`ROLE_REGISTRAR` accepts every upper-case constant in every requirement, and the
false demands would be overridden by hand until the rule was routed around —
which is the failure mode the option C record's false-positive measurement
exists to prevent. n = 6, one author, so the 50% is a property of this set and
not a rate about requirements in general.

**What v2 does not yet have:** generated IDL. `corpus/requirements/generated/`
still holds v1's twenty and nothing was generated for v2 here — see UNMEASURED.

## UNMEASURED, with reasons

- **Every model-facing number.** No producer was run for this change: no
  `ANTHROPIC_API_KEY` and no model call anywhere in it. There is no first-pass
  rate for S1/S2/S3 over `inputs-v2/`, and none is estimated or substituted.
  Both halves of the batch above are deterministic and fully measured; the model
  half is **UNMEASURED**, not approximated.
- **Whether S1 records a stated permission as `authz` for the three new bindable
  requirements.** Option C binds only what S1 recorded *and* the requirement
  states verbatim. The scanner measures the second condition only; the first
  needs a live S1 run. So `inputs-v2/` makes the rule *exercisable* — it does not
  yet show the rule firing end to end on the benchmark.
- **v2's assumption-B rates.** Both sets must be run and both reported when a
  producer is next available; quoting one without the other hides which half
  moved.
- **B against a real regeneration of a real registered contract.** Every
  measurement here uses committed bytes or fixtures. D005's own limits section
  says the second run's artifacts were never committed and that re-running
  produces a third contract, so the *class* is what is measured, never the
  instance.
- **Anything a foreign oracle would say.** `omniidl` and `contract-check` were
  not re-run; neither reads a registered contract and nothing here changes what
  they see.
- **`spikes/end_to_end.sh` under the new gate.** `spikes/` is outside this
  footprint. The driver passes no `--registered`, so its S4 hop is unaffected by
  construction — an argument, not a measurement.

## What this does not fix

- **It does not make regeneration stable.** B makes an unstable regeneration
  *refuse to land*. D005 says this plainly and it is worth repeating beside the
  green numbers.
- **It protects only the second and later contracts.** The first has nothing to
  diff against, by construction.
- **It cannot make an approval mean something.** `--supersede` under a
  twelve-change list is exactly the reflex D005 warns about. All this change buys
  is that the reflex is written down.
- **It is a consistency check, not a correctness check.** A stable,
  reproducible, gate-passing contract that reads the requirement wrongly is
  untouched — and D005 warns that stabilising regeneration makes such a
  misreading *durable* while removing the disagreement that used to expose it.
  The compensating instrument remains a person reading the brief's
  `open_questions` before registration (carried as `s3/unanswered-question`,
  Advice, by the option C change). No gate discharges it.

## Findings, not changes

1. **`corpus/requirements/generated/` has no v2 half.** The twenty generated
   files are v1's. A v2 run with a producer would produce twenty-six; until then
   `inputs-v2/` is inputs only, and the harness checks that read `generated/`
   are unaffected.
2. **§5.3 and this crate disagree about `ConditionallyBreaking`.**
   `Verdict::blocks_release()` includes it; `validate_against` maps it to
   `Severity::Warning`, so it does not refuse. That predates this change and is
   left alone deliberately — changing it would change `sidl-validate --against`
   for every caller — but it is the reason a scope stated as a constant lands.
   Whether a scope change is a breaking change in §5.3's sense is exactly what
   D005 left open.
3. **`spikes/end_to_end.sh` could exercise B for free.** Pointing a second run
   at the first run's output directory with `--registered` would make hop 4
   measure the differ on real regenerated material, with no model needed for the
   comparison half.

## Harness — recommended, not applied

`spikes/run_checks.sh` is outside this footprint, so nothing below was added to
it. The check needs no model, no network and no port, so it belongs in the gate
and may run before the harness lock.

```sh
# D005 option B: a regeneration is diffed against what is registered, and an
# undeclared breaking change is refused. Deterministic, no model, no network.
# Also pins what B *cannot* see: a scope drift is compatible by §5.3.
# Skipping is a FAILURE.
hr "forge — registered-contract diff (D005 option B)"
# Capture, then match: never pipe a producer into `grep -q` (CLAUDE.md).
evolution=$(cd "$ROOT" && cargo test -q -p orbweaver-forge --test registered_diff 2>&1)
case "$evolution" in
    *"test result: ok."*) pass "registered-contract diff: $(printf '%s' "$evolution" \
        | grep -oE '[0-9]+ passed' | head -1)" ;;
    *) fail "registered-contract diff"
       printf '%s\n' "$evolution" | tail -20 ;;
esac
```

It reads `spikes/e2e/recorded/` and writes only under `$TMPDIR`, so it must run
from the repository root and needs nothing else.

## Reproducing

```sh
cargo test -p orbweaver-forge --test registered_diff -- --nocapture   # option B, and the 12-change count
cargo test -p orbweaver-forge --test stated_scopes -- --nocapture     # option C, and the v2 corpus numbers
```

The gate against a directory, with no model:

```sh
cargo run -q --bin forge-pipeline -- --only s4 --out <workspace> --registered <yesterday's --out dir>
cargo run -q --bin forge-pipeline -- --only s4 --out <workspace> --registered <dir> \
    --supersede "why this regeneration may land"   # writes <workspace>/superseded.tsv
```

## 요약 (Korean summary)

이 기록은 영어가 정본이다. `docs/pipeline-runs/`의 선례를 따라 **결론만** 옮긴다.

- **D005의 B를 구현했다.** 이미 만들어져 있었으나 파이프라인이 부르지 않던
  `validate_against`를 S4 게이트에 연결했다. 등록된 계약이 있으면 재생성을
  §5.3 differ로 비교하고, 선언되지 않은 파괴적 변경은 **거부한다.**
- **"등록"은 실행이 가리키는 계약 디렉터리다.** 내구 저장소는 D003-B에서 미뤄졌고
  PLAN §5.3도 같은 한계를 이미 기록하고 있다. 실행 중의 S5 카탈로그는 S4 **이후에**
  같은 산출물로 만들어지므로 자기 자신과 비교하게 되어 기각했고, 저장소가 생길
  때까지 아무것도 하지 않는 선택은 E를 방치로 고르는 것이라 기각했다. 어제의
  `--out` 디렉터리가 그대로 등록 원본이 된다.
- **등록본이 없으면 침묵한다.** 첫 생성은 비교 대상이 없으므로 거부하지 않는다.
  다만 "비교하지 않았다"는 사실은 세어서 출력한다. 등록본이 있는데 **읽을 수
  없으면** 그것은 침묵이 아니라 실패다.
- **B는 스코프 표류를 보지 못한다.** differ는 애노테이션을 읽지 않으므로
  `//@ ai_authz`만 바뀐 재생성은 §5.3 기준 호환이며 통과한다. 그래서 C가 먼저
  착륙했고 B는 C를 대체하지 못한다. 유일한 예외는 계약이 스코프를 IDL 상수로도
  선언한 경우인데, 그조차 **경고이고 거부가 아니다.**
- **모듈·인터페이스 이름 하나를 바꾸자 한 항목에서 파괴적 변경 12건이 나왔다.**
  D005가 경고한 "관행이 된 승인"의 크기를 실제로 측정한 값이다. 게이트가 그
  승인을 사려 깊게 만들 수는 없고, 대신 `superseded.tsv`에 무엇을 통과시켰는지
  남긴다.
- **얼어 있는 벤치마크는 건드리지 않았다.** `corpus/queries/`의 선례대로
  `inputs-v2/`를 옆에 두었고, v1 스무 건을 바이트 단위로 그대로 포함한다(테스트가
  강제한다). 권한을 명시하는 요구사항 여섯 건을 더했다.
- **여섯 건 중 셋만 규칙이 인식한다.** 산문(*보안 책임자 권한*), 대문자
  `ROLE_REGISTRAR`, 슬래시 `warehouse/robot/estop`은 모두 실제 쓰이는 권한 표기이며
  규칙이 보지 못한다. 이것이 이번 코퍼스 추가의 **핵심 결과**다 — 규칙이 덮는 것은
  표기 관습 하나뿐이다. 예측자를 넓히면 오탐이 늘어 규칙 자체가 우회되므로 고치지
  않고 측정으로 남긴다.
- **모델 측정은 없다(UNMEASURED).** 이번 변경에서 모델 호출은 0건이며, 대체 수치를
  넣지 않았다. 결정적 검사는 전부 측정했다: forge 154건, 워크스페이스 1139건 통과.
