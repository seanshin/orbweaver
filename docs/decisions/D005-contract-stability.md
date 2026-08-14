# D005 — Contract stability across regenerations: what S2 is allowed to choose

**STATUS: APPROVED** — drafted 2026-08-14 from the finding in
[`docs/pipeline-runs/2026-08-14-end-to-end.md`](../pipeline-runs/2026-08-14-end-to-end.md)
(*Cause A*), approved the same day by the user ("승인하고 순서대로 진행"), with
the recommendation adopted as written:

1. **Option C first** — a scope-shaped literal token the requirement states must
   survive to the `//@ ai_authz` S3 emits, checked by string equality with no
   model, codified in `annotate::RULES` where
   `every_rule_is_a_prompt_constraint_and_a_check` forces both halves to exist.
   S1's `Brief.authz` already records the token, so the check is cheap and the
   drift it catches is the one no gate in this project can currently see.
2. **Then option B** — `orbweaver_forge::validate_against`, which already wraps
   §5.3's differ and which the pipeline simply never calls, wired into the S4
   gate against the registered contract. Accepted on the record that the
   registry of record does not exist yet and that the differ reads no
   annotations.
3. **Framed by option D** — a regeneration over a registered contract is an
   explicit, reasoned act rather than a repeat.
4. **Option A rejected** — pinning names in the brief revokes S2's rename, which
   is today's second line of defence against the project's dominant failure, and
   buys no stability anyway because S1 regenerates too.

What approval does **not** buy, restated because it is the document's own
warning: stabilising regeneration converts the only signal this project has ever
produced that a reading was a *choice* rather than a fact into silence. The
compensating instrument is a person reading the brief's `open_questions` before
registration, and that obligation belongs to whichever change lands these
options.

**상태: 승인됨** — 2026-08-14 승인. C를 먼저(요구사항의 리터럴 스코프 토큰이
애노테이션까지 살아남는지 모델 없이 검사), 그다음 B(§5.3 differ를 S4 게이트에),
D로 틀을 잡고, A는 기각. 승인이 사주지 **않는** 것: 재생성을 안정화하면 "이 독해는
사실이 아니라 선택이었다"는 유일한 신호가 침묵으로 바뀐다. 보완 장치는 등록 전에
사람이 `open_questions`를 읽는 것이며, 그 의무는 이 선택지들을 구현하는 변경에
붙는다.

This is a decision and not a bug report because every mechanism that would
prevent the finding constrains **what S2 is allowed to choose**. That is a
statement about the pipeline's contract with its users — whether "run it again"
means "get the same thing" — and not an implementation detail that the next
batch may settle by committing first.

이것이 버그 수정이 아니라 결정인 이유는, 이 현상을 막는 모든 방법이 **S2가 무엇을
고를 수 있는가**를 제약하기 때문이다. 그것은 "다시 돌리면 같은 것이 나오는가"라는
파이프라인과 사용자 사이의 약속에 관한 문제이지, 먼저 커밋한 사람이 정하면 되는
구현 세부가 아니다.

## What was measured, and what it is not / 측정된 것과 아닌 것

`E2E_MODEL=1 ./spikes/end_to_end.sh` re-ran **S1 ingest, S2 synthesize and S3
annotate against the same requirement file, the same producer and the same
crate-constant prompts.** Every stage passed 1/1 again, exactly as the first
run did. The contract was different:

| | first run (recorded) | second run |
|---|---|---|
| module | `ParkingFacility` | `ParkingLot` |
| interface | `ParkingControl` | `GateControl` |
| operations | `get_floor_occupancy` / `get_gate_status` / `open_entry_gate` | `get_floor_availability` / `get_barrier_state` / `open_entry_barrier` |
| floor identity | `typedef long FloorLevel` (signed; basements negative) | `typedef string FloorLabel` |
| shape | one floor per call | a sequence of every floor per call |
| **authorization scope** | **`gate:operate`** | **`parkinglot.barrier.open`** |

Nine downstream checks failed (hops 3–7) and all nine had this one cause. The
requirement states `gate:operate` in prose, on purpose.

**Limits of the evidence, stated before anything is argued on it:**

- **n = 1 regeneration.** Two samples of one requirement. There is no rate here,
  no variance estimate, and nothing that supports a claim about how often any of
  this happens. Every number in this document is a count of two runs.
- **The generator, the gates, the earlier record and this document are all the
  same model family.** Per PLAN §8 and the honesty rules, everything
  model-facing here is **indicative**. The two checks in that run written by
  nobody involved were `omniidl` (a foreign compiler) and `contract-check` (a
  peer crate) — and **both accepted both contracts.**
- **The second contract was not committed.** `spikes/e2e/recorded/` holds the
  first run's `PARKING.brief.json`, `PARKING.idl`, `PARKING.sidl.idl` and
  `pipeline.log`. The second run's artifacts exist only as the six-row table
  above, transcribed into the run record. **The `long` → `string` claim and the
  scope claim cannot be re-read from the tree**; they can only be re-produced by
  running the producer again, which by the nature of the finding will produce a
  third contract rather than the second one. Anyone re-deriving this document's
  premises should expect to measure the *class*, not to reproduce the *instance*.
- **What is verified from the tree**, and cited by file and line throughout
  below: the stage wiring, every gate's rule set, the `Brief` shape, the S2 and
  S3 prompts, and the differ's public API. Those are code and were read.

**증거의 한계:** 재생성 표본 1건이므로 비율이 아니다. 생성·게이트·기록·이 문서가
모두 같은 모델 계열이므로 수치는 **지시적**이며, 외부 검사 두 개(`omniidl`,
`contract-check`)는 **두 계약을 모두 통과시켰다.** 두 번째 계약의 산출물은
커밋되지 않았고 실행 기록의 표로만 남아 있다 — 즉 사례는 재현할 수 없고 부류만
측정할 수 있다. 반면 아래에 파일·행으로 인용한 코드 사실은 트리에서 직접 읽은
것이다.

## The question / 문제

May two runs of S1–S3 over one unchanged requirement, with unchanged prompts,
produce two different contracts — and if not, which stage loses which freedom?

같은 요구사항·같은 프롬프트로 S1–S3를 두 번 돌렸을 때 서로 다른 계약이 나와도
되는가 — 안 된다면 어느 단계가 어떤 자유를 잃는가?

Today the answer is *yes, freely*, and it is written into the prompt rather than
left implicit. `S2_PROMPT` (`crates/orbweaver-forge/src/synthesize.rs:39`) grants
the freedom in two sentences:

> `- Cover the brief. Every operation and entity in it should be reachable in the
>   IDL; if you deliberately merge or rename one, keep the meaning.`

> `Write the types the brief's shapes describe — the shapes are in plain words on
>  purpose, and choosing the IDL type is your decision, not the brief's.`

Both sentences are deliberate and both are load-bearing. The rename clause is
how S2 escapes the project's dominant failure — the case-insensitive identifier
clash — when S1 hands it a clashing name. The type clause is the entire reason
S2 exists as a stage: the brief carries *shapes in plain words* and turning a
shape into `long` or `string` is the synthesis. Revoking either is a real loss,
which is why this is a decision and not a patch.

## The two harms are different failures and need different answers / 두 피해

### Harm 1 — identifier drift / 식별자 표류

Names change, so **repository ids change**, and a repository id is the contract's
identity (`crates/orbweaver-registry/src/diff.rs:94`: *"the repository id is the
contract identity, so removing or renaming it makes every existing reference to
it unresolvable"*). Consequences measured in the run:

- a committed servant stops compiling against the regenerated trait (hop 3);
- the generated stub, skeleton and client name types that no longer exist;
- the operator's exposure allowlist — `--expose IDL:ParkingFacility/ParkingControl:1.0`
  — and the S5 `exposure.todo.tsv` worksheet name ids that no longer resolve, so
  the bridge exposes **nothing** (`allow=0`, hop 4);
- any registered contract's consumers break in the same way, for the same reason.

**Blast radius: everything downstream. Failure mode: loud, early, and at build
time.** The servant does not compile; the IOR is never published; the dry run
predicts `allow=0` before a socket is opened. Every one of the nine failures
announced itself. The cost is rework, paid by the person who ran the
regeneration, at the moment they ran it.

### Harm 2 — semantic drift / 의미 표류

The same names could have survived and the *meaning* still changed. Two measured
instances:

- **the scope**: `gate:operate` → `parkinglot.barrier.open`;
- **a type**: `typedef long FloorLevel` (signed, so a basement is `-1`) →
  `typedef string FloorLabel`.

**Blast radius: narrow. Failure mode: silent, late, and misattributed.** That
inversion is the whole argument.

Take the scope case, which is the sharp one. The second contract compiles,
annotates, validates, registers, exposes and serves. `omniidl` accepts it;
`contract-check` finds nothing; S4 gates it clean. A deployment whose identity
provider issues the scope the requirement *literally states* — `gate:operate` —
against a contract that demands `parkinglot.barrier.open` **refuses every
legitimate caller.** The refusal is well-formed, correctly audited, and
indistinguishable from a permissions misconfiguration: the operator holds a
scope, the guard reads a scope, they differ. The people who debug it are the
identity team, who will check the IdP, the role mapping and the token, find all
three correct, and have no reason to suspect a generator that reported 1/1 valid
at every stage. The system fails **closed**, which is the safe direction and not
a harmless one — a barrier no operator can open is an outage, and it is an
outage whose evidence points away from its cause.

The type case is the same shape with a luckier instance. `long` → `string`
happens to be loud, because the servant will not compile. The *class* is not
loud: `long` → `unsigned long` for a floor level whose basements are negative
compiles everywhere and wraps on the wire, and CDR encodes by position with no
tag to notice with (PLAN §5.3). One member of this class was proved on the wire
in Phase 2 — a client whose two struct members had been swapped received the
other member's value **with no exception raised**.

### Which is worse, and why / 어느 쪽이 더 나쁜가

**Semantic drift is worse, and this run's own data is the argument.**

1. **Identifier drift's cost is bounded by a compiler.** It is paid in build
   errors, at the moment of the change, by the person who caused it. Semantic
   drift's cost is unbounded in time and paid at run time by somebody who did
   not cause it and cannot see the cause.
2. **Identifier drift is self-announcing; semantic drift produced zero
   findings.** Considered on its own merits the second contract is valid,
   annotated, `omniidl`-clean and `contract-check`-clean. Every gate in this
   project passes it.
3. **The decisive point: the scope drift was only visible because the names
   drifted too.** A regeneration that kept `ParkingFacility::ParkingControl` and
   every operation name, and changed only `//@ ai_authz`, would have passed all
   eight hops of the end-to-end run — `end-to-end: PASS`, green, and wrong. Its
   detection in this measurement was **incidental**. That is the strongest thing
   that can be said about a failure mode: the one time we caught it, we caught it
   by accident.

**의미 표류가 더 나쁘다.** 식별자 표류는 컴파일러가 경계를 그어 주며, 변경한
사람이 그 자리에서 빌드 오류로 비용을 치른다. 의미 표류는 시간상 경계가 없고,
원인을 볼 수 없는 다른 사람이 운영 중에 비용을 치른다. 두 번째 계약은 그 자체로는
유효하고 주석이 붙어 있으며 `omniidl`과 `contract-check`를 모두 통과한다. 결정적
으로, **이번에 스코프 표류가 보인 것은 이름까지 함께 바뀌었기 때문이다** —
이름을 전부 유지한 채 `ai_authz`만 바뀐 재생성이었다면 8개 홉 전부가 초록으로
통과했을 것이다. 유일하게 잡은 한 번이 우연이었다는 것이 이 부류에 대해 할 수
있는 가장 강한 진술이다.

The ranking has a practical consequence and is not a rhetorical flourish: it
says the cheap narrow fix aimed at the scope should land **before** the broad
expensive one aimed at names, and §*Recommendation* argues that ordering
directly.

## Why no gate catches it / 왜 어떤 게이트도 잡지 못하는가

Verified by reading the stages, because "no gate catches it" is the kind of
claim that is embarrassing to assert and easy to check.

- **No stage compares its output to a previous run.** `Workspace`
  (`pipeline.rs:573`) passes artifacts forward and overwrites; nothing stores or
  reads a prior run.
- **No pipeline gate compares its output to a registered contract.** The
  capability exists and is not wired in: `orbweaver_forge::validate_against`
  (`lib.rs:240`) already loads two contracts into registries, runs
  `orbweaver_registry::diff::diff`, and maps `Verdict::Breaking → Error` under
  the rule name `evolution/BREAKING`. Its callers are the `sidl-validate
  --against` CLI and the `idl-diff` binary. `gate_for(StageId::Validate, ..)`
  calls plain `validate(output)` (`pipeline.rs:564`). **The differ is inside
  forge and the pipeline does not call it.**
- **The one input-versus-output check is within a run.** `s3/contract-changed`
  (`annotate.rs:135`, via `contract_changes`, `annotate.rs:716`) demands that
  S3's output be S3's input plus comments. It compares `contract_shape()` maps
  that **strip annotations first** (`annotate.rs:765`), so it is by construction
  blind to a scope.
- **Nothing anywhere checks the requirement text.** The nearest thing is S2's
  `coverage()` (`synthesize.rs:137`), which compares *brief* names to IDL names —
  and it is three ways too weak to be the answer: the findings are
  `Severity::Warning` (`s2/operation-missing`, `synthesize.rs:164`;
  `s2/entity-missing`, `:183`), the match is loose substring containment in
  either direction after stripping non-alphanumerics (`:160`), and `comparable()`
  (`:133`) **skips any name containing a non-ASCII character** — so a Korean
  brief, which is what this run's Korean requirement produced, is exempt entirely.
- **The scope information exists at S1 and has nowhere to go.**
  `OperationSketch::authz: Option<String>` (`ingest.rs:181`) did carry
  `"gate:operate"` — it is on line 113 of the committed
  `spikes/e2e/recorded/PARKING.brief.json`. `Brief::to_prompt()` renders it as
  `[authz: gate:operate]` into **S2's** prompt (`ingest.rs:517`), where
  `S2_PROMPT` forbids S2 from writing any `//@ ai_*` annotation. **S3 never
  receives the brief at all**: `gate_for(StageId::Annotate, ..)` passes S2's
  `.idl` (`pipeline.rs:563`, `input_stage(Annotate) = Synthesize`), and
  `annotate::check_against(before, after)` takes two IDL strings. S3's model
  therefore invents the scope from the IDL, and `s3/missing-ai_authz`
  (`annotate.rs:99`) checks only that the string is **non-empty**
  (`annotate.rs:507`). S1's own gate never checks `authz` either — a mutating
  operation with `authz: None` passes S1 silently.

That last bullet is the useful one: the token is not lost through carelessness.
There is structurally nowhere for it to go.

## Options considered / 검토한 대안

Each with what it costs and what it forbids, because an option stated without
its cost is advocacy.

### A — pin names in S1's `Brief`; make S2's gate refuse a rename

**What exists:** `coverage()` already does the comparison. Raising
`s2/operation-missing` and `s2/entity-missing` from `Warning` to `Error` and
tightening the match from substring containment to equality is a small change.

**What it costs.** It revokes the two S2_PROMPT sentences quoted above. S2 can
no longer rename its way out of a case-insensitive identifier clash — the
project's dominant generation failure, measured 7/7 in Phase 0 and again as
`enclosing-scope-clash` and `identifier-case-clash` in the twenty-item split
run. `s1/name-clash` (`ingest.rs:809`) catches part of that class at S1, but
S2's rename is today's second line of defence and this option removes it. It
also does nothing for a Korean brief unless S1 is separately required to emit
ASCII identifiers, which is a second policy change hiding inside the first.

**What it forbids, and what it does not.** It forbids S2 choosing names. It does
**not** forbid S1 choosing them differently, and S1 regenerates too — so this
moves the non-determinism up one stage rather than removing it. It closes the
loop only if the brief is treated as the durable artifact and is *not*
regenerated, which `ingest.rs:186-189` already contemplates ("edit the brief and
re-run from S2 is a supported operation"). Stated honestly, option A is really
*make the brief, not the requirement, the thing of record* — a defensible policy
that should be argued as itself rather than arrived at sideways.

**On the harm this document ranks first: it does nothing.** `ai_authz` is
written at S3, which never sees the brief.

### B — diff a regeneration against the registered contract; refuse an undeclared breaking change

**What exists:** almost all of it. `diff(old: &Registry, new: &Registry) ->
Vec<Change>` (`diff.rs:82`), `Verdict::{Compatible, ServerFirst,
ConditionallyBreaking, Breaking}` with `blocks_release()` (`diff.rs:44`), the
`idl-diff <released.idl> <proposed.idl> [--approve <reason>]` binary that exits
non-zero on a blocking verdict, and `orbweaver_forge::validate_against`
(`lib.rs:240`) which already wraps the differ for forge and is simply not called
by the pipeline. A rename reads as `removed` (Breaking) + `added` (Compatible) —
pinned by the test `renaming_reads_as_a_removal_and_is_breaking` (`diff.rs:574`).

**What it catches:** the whole of harm 1, correctly, with machinery that is
built, tested, and proved on the wire (PLAN §5.3).

**What it does not catch: the scope.** `diff.rs` compares bases, operations,
attributes, `TypeCode`s and constant types. It never reads
`OperationSig::annotations` (`registry/src/lib.rs:112`), where `ai_authz` lives.
A regeneration that keeps every identifier and changes only the scope produces
**zero changes** from the differ. Extending it to annotations is new work with
its own policy question — by §5.3's own logic a scope is not a wire change at
all, so the table has no row for it and one would have to be argued.

**What it costs.** It needs a registry of record, and PLAN §5.3 already records
that it does not have one: *"'released' currently means the file `idl-diff` is
pointed at rather than a contract read from a registry of record."* And it has a
subtler cost that matters more: **a full regeneration renames everything, so the
gate fires on every id, every time.** An approval that is always given stops
being a signal. Landing B alone trains an operator to type
`--approve "regenerated"` as a reflex — and a scope drift then rides in under
that approval, invisible, because the differ never looked at the scope anyway.

**What it forbids:** nothing about generation. It forbids **registration**. That
is its virtue and its limit: it does not make regeneration stable, it makes an
unstable regeneration refuse to land, and it protects only the second and later
contracts because the first has nothing to diff against.

### C — require a literal token from the requirement to survive to the annotation

**Shape:** a token in the requirement matching a scope-shaped lexical form (say
`[a-z][a-z0-9_]*[:.][a-z0-9._:-]+`) that S1 recorded in
`OperationSketch::authz` must appear **verbatim** in that operation's
`//@ ai_authz`. No model is involved in the check; it is string equality.

**What exists:** S1's half, entirely. `OperationSketch::authz` (`ingest.rs:181`)
and `Brief::requirement` verbatim (`ingest.rs:193`), and the measured brief
carries `"authz": "gate:operate"`. The codify home exists too: `annotate::RULES`
(`annotate.rs:82`) with the test
`every_rule_is_a_prompt_constraint_and_a_check` (`annotate.rs:1140`) forcing a
rule to be both a prompt constraint and a check — the mechanism that made
`s3/oneway-not-idempotent` stick.

**What does not exist: a channel.** As established above, S3 receives the `.idl`
and only the `.idl`. So C is not one rule; it is one rule **plus** a plumbing
change that gives S3 the brief alongside the IDL. The alternative — have S2 emit
the scope so S3 can preserve it — collides head-on with `S2_PROMPT`'s "Do NOT
write //@ ai_*" and with `s3/contract-changed`. The plumbing is the honest cost,
and it is small: a second input artifact for one stage.

**What it forbids:** S3 inventing a scope when the requirement stated one. It
forbids nothing when the requirement states none, which is most requirements.

**Its limits, stated plainly.** It binds only what a *lexical* rule can
recognise. `gate:operate` is recognisable because it is shaped like a scope; a
requirement saying "운영자 권한" with no literal token gets no protection at all,
and a requirement that mentions a colon-shaped token which is *not* the scope
produces a false demand that a human must then override. It says nothing about
module names, operation names, or types. **It would have failed the second run.**

### D — accept non-determinism; make regeneration an explicit, versioned act

**Shape:** `forge-pipeline` refuses to silently overwrite a contract that has
been registered. A regeneration must either land as a new version — a
`ParkingControl_2` in a versioned module, which is exactly what PLAN §5.3
already prescribes for evolution ("versioned interfaces … never by editing
deployed types in place") — or pass an explicit `--supersede <id> --reason
<text>` that is recorded.

**What it costs:** it does not make the second contract *right*, it makes it
*separate*. Two contracts answer one requirement and a human chooses. The
registry grows. The servant author still has to port.

**What it forbids:** the idea that "re-run the pipeline" is an ordinary
repeatable operation. That is a genuine loss and it lands on a property the
project has already measured and valued — the split-pipeline run's headline was
that re-running S3 costs 20 model calls instead of 60. D keeps re-running from
S2 or S3 cheap and makes re-running **from S1 over a registered contract** a
governed act, which is arguably where the line belongs, since S1 is where
identity is fixed.

**Its virtue:** it is the only option honest about what a sampling model is. It
introduces no new doctrine; it applies §5.3's existing doctrine to a case §5.3
did not anticipate, namely a "new version" nobody designed.

**On the scope: it does nothing**, unless combined with C.

### E — do nothing

Stated concretely for a user who regenerates, because "do nothing" has a cost
and the cost is what makes it an option rather than a default:

The pipeline reports 1/1 valid at every stage, twice, and both reports are true.
Their committed servant stops compiling — they will notice that. Their exposure
allowlist silently covers nothing (`allow=0`), so the bridge answers every agent
with no such interface and no error anywhere says why. And their authorization
scope may have changed under a contract that still looks entirely correct, in
which case the system fails closed against every legitimate operator and the
evidence points at the identity provider.

The honest summary is that **the pipeline promises repeatability nowhere and its
users will assume it everywhere**, because every other compiler-shaped tool they
have ever used is deterministic. Choosing E is choosing to let that assumption
be discovered in production rather than in a decision document.

## Recommendation / 권고

**Adopt C first, then B, framed by D. Reject A. Do not choose E by omission.**

1. **C is the change to land first**, because it answers the harm this document
   argues is worse, it is checkable with no model, it has a codify home where
   `every_rule_is_a_prompt_constraint_and_a_check` forces both halves to exist,
   and it would have failed the second run. Its real cost is the plumbing that
   gives S3 the brief, and that cost should be paid explicitly rather than
   worked around by widening what S2 may write.
2. **B is the second batch, not the first**, and the ordering is the substance
   of the recommendation rather than a schedule. B fires on every id of every
   regeneration, so landing it alone makes `--approve "regenerated"` routine, and
   a routine approval is a silence — under which a scope drift the differ cannot
   see would pass anyway. Wire `validate_against` into the S4 gate with the
   registered contract as `released`, and accept the two limits on the record:
   the registry of record does not exist yet (PLAN §5.3's own stated limit), and
   the differ does not read annotations.
3. **D is the frame both land inside.** A regeneration over a registered
   contract is an explicit, reasoned act with a recorded reason, not a repeat.
4. **A is rejected.** It buys name stability by revoking S2's rename — today's
   second line of defence against the project's dominant failure — and it does
   not actually buy stability, because S1 regenerates too. If the underlying idea
   is worth having, it is *make the brief the artifact of record*, and that
   deserves its own decision rather than arriving as a gate severity change.

**권고: C를 먼저, 그다음 B, 둘 다 D의 틀 안에서. A는 기각. E를 방치로 선택하지
말 것.** C가 먼저인 이유는 이 문서가 더 나쁘다고 논증한 피해를 직접 다루고,
모델 없이 검사 가능하며, `annotate::RULES`라는 성문화 자리가 이미 있고, 두 번째
실행을 실제로 떨어뜨렸을 것이기 때문이다. B가 먼저이면 안 되는 이유는 재생성마다
모든 식별자에 대해 발화하므로 `--approve "재생성"`이 관행이 되고, 관행이 된 승인은
침묵이며, differ가 애초에 보지 못하는 스코프 표류가 그 침묵을 타고 통과하기
때문이다. A는 S2의 개명 권한 — 이 프로젝트 최다 실패 원인에 대한 두 번째 방어선 —
을 회수하면서도, S1 역시 재생성되므로 안정성을 사지 못한다.

## What measurement would confirm or refute this / 무엇이 이 권고를 판정하는가

The recommendation rests on a falsifiable claim: **the drift that matters
concentrates in a small number of literal tokens and repository ids rather than
spreading evenly across the contract.** Measure it.

Take one requirement and regenerate S1–S3 **N ≥ 20 times** with prompts and
producer fixed, recording per run: the set of repository ids, the set of
`ai_authz` values, and the chosen type of every parameter the brief marks with
the same `authz`. Three numbers settle the recommendation:

1. **How often the scope drifts when the requirement states one literally.** If
   C's rule would have fired in most drifting runs, C is the right first change.
   If the scope is usually stable and it is the *types* that move, C is cheap
   insurance and the real fix is elsewhere — which refutes the **ordering**, not
   the option.
2. **How often two runs agree on any identifier at all.** High agreement with
   occasional drift makes B a gate and the recommendation stands. Agreement near
   zero makes B a wall rather than a gate, and D's versioned-act framing becomes
   the only workable posture — that would demote B from "second batch" to
   "consequence of D".
3. **How often a run keeps every identifier and still changes the scope.** This
   is the case that passes all eight hops today, and its rate is the entire
   argument for ranking semantic drift above identifier drift. **If it never
   happens across N runs, the ranking in this document is wrong** and B alone
   suffices.

Two honesty conditions on that measurement. It must be run with a second,
non-Claude producer if one becomes available, because a drift rate measured with
one sampler is a property of that sampler and may not be reported as a property
of "models". And N regenerations of one requirement measure one requirement: the
full version is 20 × N over the corpus, at roughly the split-pipeline run's cost
(1445 s for 60 model calls) multiplied by N, which is a real budget and should be
approved as one rather than discovered.

## What none of these options fix / 어떤 대안도 고치지 못하는 것

**None of this makes the contract correct.** A model can produce a stable,
reproducible, gate-passing contract that reads the requirement wrongly, and a
determinism check is a consistency check, not a correctness check. No option
here catches a *consistent* misreading; several of them would make one durable.

The measured run already shows the shape of it: S1 recorded ten open questions —
how a floor is identified, whether the caller's permission arrives as a parameter
or out of band, what `open` returns — **nobody answered them**, S2 chose, and
both sets of choices passed every gate. `open_questions` and a human reader are
the only instruments in the project that address this, and neither is a gate.

There is a cost here that must be said in the same breath as the recommendation:
**disagreement between two runs is currently the only signal this project has
ever produced that a reading was a choice rather than a fact.** Making
regeneration stable converts that loud, incidental signal into silence. If C, B
and D are adopted, the compensating instrument cannot be a green check — it has
to be that a brief's open questions get **read**, by a person, before the
contract is registered. That obligation belongs in whatever change lands these
options, not in a later one.

**어떤 대안도 계약을 *옳게* 만들지 못한다.** 모델은 재현 가능하고 게이트를 모두
통과하면서도 요구사항을 잘못 읽은 계약을 낼 수 있으며, 결정성 검사는 일관성
검사이지 정확성 검사가 아니다. 실제로 S1은 열 개의 미해결 질문을 기록했고 아무도
답하지 않았으며 S2가 골랐고 양쪽 다 통과했다. 더 중요한 것은 **두 실행의 불일치가
지금까지 이 프로젝트가 "그 독해는 사실이 아니라 선택이었다"를 드러낸 유일한
신호였다는 점이다.** 재생성을 안정화하면 그 신호는 침묵이 된다. 따라서 보완 수단은
초록색 검사가 아니라, 등록 전에 사람이 `open_questions`를 **읽는 것**이어야 하며,
그 의무는 이 대안들을 싣는 변경에 함께 들어가야 한다.

## What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is adopted. No crate is modified by this change; its footprint is this
file and one pointer line in `docs/PLAN.md` and `docs/PLAN.ko.md`.

Specifically left open: whether a scope change is a *breaking* change in §5.3's
sense (the table has no row for annotations and this document does not add one);
whether the brief becomes the artifact of record (option A's real proposition,
which needs its own decision); where the registry of record lives, which B
depends on and PLAN §5.3 already names as missing; the three other findings the
same run reported — the MCP serving loop's discarded audit log, `orbweaver-gen`'s
unimplemented constants, and S3i's relationship to any of this — none of which
are this document's subject. Stream A (PLAN §7.3) owns S1–S3 and would own the
work if this is approved.

오늘 채택되는 것은 없다. 스코프 변경이 §5.3의 의미에서 파괴적 변경인지, 브리프를
정본 산출물로 삼을 것인지, B가 의존하는 등록 원본이 어디에 있는지는 모두 열린
채로 둔다. 같은 실행이 보고한 다른 세 발견(감사 로그 미출력, 상수 미생성, S3i)은
이 문서의 주제가 아니다.
