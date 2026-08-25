# D017 — Finding these without a human, and the wave that lands

**STATUS: PROPOSED** — drafted 2026-08-25, while the day's twelfth batch was in
the harness. D016 lists the defects that day measured; this proposes the
practices that would have found them **without anyone going looking**, and the
process change the day's own scale argued for. Not self-approvable: §5 costs
harness time on every run, which is a budget the user owns.

**상태: 제안** — 2026-08-25 작성. D016이 그날 측정한 결함을 열거한다면, 이
문서는 **아무도 찾으러 가지 않아도 그것들이 드러나게 하는 관행**과, 그날의
규모가 스스로 요구한 공정 변경을 제안한다.

---

## 1. The two numbers this document is written from / 이 문서가 딛는 두 숫자

**Four gates, in one day, were reporting green over things they had never
read.** The bilingual decision gate had not compared eleven of thirteen Korean
halves — including D003's, the file whose split is the reason the gate exists.
The harness's own first three gates could not go red, one of them the licence
boundary this project calls non-negotiable. The same defect sat in 76 more
places in the form the rules file had called *sanctioned*. And the formatting
check lived only in CI, so "landed through the harness" had never included it.

**Eleven of the fourteen defects in D016 are one class**: a fact with more than
one home and no compiler behind any of them.

Neither number was produced by review. Every one came from **running a thing
and watching what it did** — a control that made a gate red, a build control
that added a variant and watched four call sites fail to compile, a fixture
driven through the real path. The lesson is not "look harder". It is that the
looking has to be done by something that runs.

*이 문서가 딛는 두 숫자: 하루에 게이트 넷이 읽은 적 없는 것 위에서 초록을
보고했고, 결함 열넷 중 열하나가 한 계급이다. 어느 것도 검토로 나오지 않았다 —
전부 **무언가를 실행하고 그것이 무엇을 하는지 본 것**에서 나왔다.*

## 2. The gate that has never been red / 한 번도 빨간 적 없는 게이트

A gate is a claim about the future: *if this breaks, I will say so.* Nothing in
this project tests that claim except a human remembering to write a negative
control, and the memory has failed at least six times — five recorded in
`CLAUDE.md` before today, four more today.

**Proposal.** Each harness group records, beside itself, the last time it was
observed red and by what. A group that has never been observed red is not
thereby wrong — most gates should be green most of the time — but it is a
**suspect**, and the harness should be able to print the list. Two forms, and
the cheap one first:

- **A report** (`spikes/never_red.py`): every `fail_total=$((fail_total+1))`
  site in `run_checks.sh` beside the commit that last touched it and whether a
  negative control is recorded in that commit's message. D010 §7.2 already
  requires the control to land in the commit message, so the data exists in
  git; nothing reads it. This is a morning's work and finds the groups whose
  controls were never written.
- **A rehearsal** (harder, and the real answer): a mode in which the harness
  runs each group against a deliberately broken input and asserts it goes red.
  That is a fixture per group, which is why it is not proposed wholesale — but
  it is exactly what `spikes/half_reply.sh` and the `bilingual_drift`
  transformation did for their own batches, and both found something.

**Start with the report.** Its first output is a measurement nobody has:
*how many of this harness's gates have a recorded control at all?*

*게이트는 미래에 대한 주장이다 — "이것이 깨지면 내가 말하겠다". 그 주장을
시험하는 것은 사람의 기억뿐이었고, 그 기억은 최소 여섯 번 실패했다. 한 번도
빨간 적 없는 그룹은 틀린 것이 아니라 **용의자**다.*

## 3. Finding a second home without a human noticing / 두 번째 집을 자동으로 찾기

Today's eleven were found by five separate sweeps, each commissioned after
someone noticed one instance. That does not scale and it is not repeatable.
What the instances have in common is mechanical:

| Shape | Today's instances | What a script can see |
|---|---|---|
| A **sentence** retyped | the four refusal heads (12 literals), `_DEFERRED`/`_UNMARSHALLABLE`, the deferral heads | a string literal ≥ N words appearing in two crates |
| A **spelling** retyped | seven `TypeCode` namers, four repository-id readers, `a_free_low_port` in two crates | a function whose body is textually near-identical to another's |
| A **table** retyped | `MUTATING_VERBS` ×3, the ungating set ×3, `PREDECLARED_CORBA` (before it became a constant) | an array/slice literal whose elements match another's, in another crate |
| A **classifier** keyed on someone else's sentence | `LexError::rule`, `deferred_wire_gaps`, `def_kind`'s comment | a `starts_with`/`contains` over a literal that also appears in another crate |
| A **comment** asserting another crate's behaviour | six in `orbweaver-registry`, four in `orbweaver-forge` | a comment naming a symbol from another crate — `spikes/comment_symbols.py`, already sketched by the registry batch |

None of these needs to be clever. Each is a report that lists candidates for a
human to judge, in the shape `gap_symbols.py` and `plan_numbers.py` already
have — *a report, not a gate*, because every one of them has false positives by
construction (three copies of a keyword list may be three different facts).

**Order by what today cost most**: the comment sweep first (ten stale comments
in two crates, two of them already losing in the product), then the retyped
table, then the retyped sentence. The spelling one is the hardest and the one
D016 §4 A2 is already fixing by hand.

*오늘의 열하나는 각각 누군가가 한 사례를 알아챈 뒤 발주된 스윕 다섯에서 나왔다.
그것은 규모가 커지지 않고 반복되지도 않는다. 다섯 모양 전부 기계가 볼 수 있다.*

## 4. The wave that lands / 착지하는 웨이브

2026-08-25 ran twelve batches and **landing became the constraint**: serial
merges at one harness run each, and a run went 16 → 55 minutes as the corpus
and the suite grew. Three changes, each measured that day:

1. **Group merges by risk, not one by one.** All ten pending branches merged
   cleanly against `main` and against each other — verified with
   `git merge-tree` before touching the working tree, which costs seconds.
   Merging in three risk groups turned ten harness runs into three. The cost is
   stated rather than hidden: a red group names three or four batches instead
   of one, and their footprints are disjoint, so a bisect is two runs at worst.
2. **`ORBWEAVER_CONCURRENCY_RUNS=1` for intermediate landings**, the full five
   for the group that touches the wire and for the last run of a session. The
   knob already exists; using it is a deliberate, stated weakening — *"five
   runs, because one green run is not evidence"* is the group's own argument,
   and a batch that introduces a concurrency regression would then be caught at
   the end rather than at itself.
3. **Check the unlanded branches before commissioning a batch.** D014's W1-c
   was written from a gap `main` really had and a worktree branch had already
   closed — better, because that branch had been handed the word *constants*
   and generalised to *the catalogue shows what a contract declares*. One batch
   wasted. `git worktree list` is part of verifying a gap.

And the discipline that made the day's own mistakes cheap: **a record lands
with its batch**. Four merges went in without theirs and `records_keep_up.py`
said so, correctly, at twelve commits behind — the gate this project wrote
after getting it wrong twice, working.

*열두 배치를 돌린 날 **착지가 제약이 되었다.** 위험도별 묶음 병합, 중간 착지의
동시성 1회, 그리고 발주 전 미착지 브랜치 확인.*

## 5. What this costs, and what it cannot fix / 비용과 한계

§2's report and §3's five reports are cheap to run and cheap to ignore, which
is their weakness: this project already has three reports that are not gates
(`gap_symbols`, `plan_numbers`, `bilingual_drift`) and the discipline that
makes them useful is a human reading them before planning. Adding five more
without a habit attached would be five more things nobody reads. **So each one
lands with the place it is read named** — before a batch is commissioned, in
the sequence `CLAUDE.md` already gives for `gap_symbols.py`.

The rehearsal mode in §2 is the only proposal here that would cost harness
time, and it is the only one that turns a suspicion into a measurement. It is
proposed as a direction, not a batch, because a fixture per group is the same
work as `spikes/half_reply.sh` × the number of groups.

None of this reaches the class that has no textual shape: a comment that is
false about behaviour nobody can grep for, a gate whose fixture silently
stopped starting, a claim that was never true. Those still need somebody to run
the thing and watch. What §2 and §3 buy is that **the mechanical half stops
consuming the attention that the unmechanical half needs.**

## 6. Order / 순서

1. `spikes/comment_symbols.py` — the registry batch already sketched it, and it
   would have found ten stale comments in two crates.
2. `spikes/never_red.py` — the measurement nobody has.
3. The retyped-table and retyped-sentence reports.
4. The rehearsal mode, if §2's report says enough groups lack a control to
   justify it. **That is the decision this document is really asking for**, and
   it cannot be made before the report exists.

Everything in D016 comes first: these are practices, and practices written
while eight measured defects sit unfixed are a way of not fixing them.

*D016이 먼저다. 측정된 결함 여덟이 미수정으로 있는 동안 쓰인 관행은 그것을
고치지 않는 한 방법이다.*
