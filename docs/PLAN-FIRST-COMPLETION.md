# What stands between here and a first complete ORB

**Written 2026-08-29.** Subordinate to priority zero, whose home is
`docs/decisions/D029-what-a-complete-orb-would-mean.md` §6 and which is not
restated here. Every item below records how it bears on that criterion, and the
order is that criterion's: **a proposal that closes a leak outranks one that
adds a capability.**

This document plans. It does not restate `COMPONENTS.md`'s remaining-work
column, and it does not restate any decision's status — those have homes and
`spikes/decision_status.py` checks the restatements against them.

**Where the list comes from.** Not from judgement about what feels unfinished:
the open leaks are the `unmeasured, per D029 §6.1 — where it leaks today` cells
that the harness reads out of D029 at run time and printed on 2026-08-29, and
the counted `SKIPPED`s are that run's own. A plan assembled from memory is a
plan about the last thing somebody looked at.

*2026-08-29 작성. 0순위에 종속되며 그 집은 D029 §6이고 여기서 다시 적지 않는다.
항목마다 그 기준에 어떻게 닿는지를 적고, 순서는 그 기준의 순서다 — **구멍을 막는
제안이 기능을 더하는 제안보다 앞선다.** 목록은 판단이 아니라 하네스가 실행 시점에
D029에서 읽어 찍은 셀에서 온다. 기억으로 조립한 계획은 마지막으로 누가 들여다본
것에 대한 계획이다.*

---

## 0. The state this plans from / 이 계획이 딛는 상태

Measured 2026-08-29, one harness run and one CI run, both green:

| | |
|---|---|
| harness | all measured checks green; **16** counted `SKIPPED` |
| transparency measured this run | location, backend, language, activation, lifecycle — 20 groups, 0 red |
| `cargo test --workspace` | 2007 tests, exit 0 (2026-08-28) |
| CI | three jobs green on `fafd5ea` |

Each figure carries the date it was measured, because a figure in prose drifts
in silence and a gate stays green over the drift.

*각 수치는 측정 날짜를 단다 — 산문의 수치는 조용히 어긋나고 게이트는 그 위에서
초록으로 남기 때문이다.*

---

## 1. Open leaks, in priority-zero order / 열린 구멍, 0순위 순서로

### L1 — Backend: `Dispatch::knows` accepts a key nobody activated

**The leak.** A servant that inherits the default `knows` answers for any
object key at its endpoint. A caller fabricates a key, is answered, and has
learned a backend fact: this endpoint holds one undifferentiated servant rather
than a POA with an active object map. CORBA 3.4 §15.3.8.6's own default
(`USE_ACTIVE_OBJECT_MAP_ONLY`) would have said `OBJECT_NOT_EXIST` and told it
nothing.

**Why it is first.** It is the only open leak whose measurement already exists
and is green: `crates/orbweaver-giop/tests/a_key_nobody_activated.rs`, **10
tests** across 3 GIOP versions × 2 byte orders (run 2026-08-29). Nothing has to
be built to see the change land or fail.

That figure was **9** in the first draft of this section, copied from D029's
cell — which is the same defect this item's *Watch for* names, committed one
paragraph after naming it, in a document whose §0 says a figure carries its
date. `spikes/plan_numbers.py` is what caught it: it flags every hand-typed
count no rule computes, and it is a report rather than a gate for the reason
CLAUDE.md gives, so it only helps somebody who runs it.

**What changed since the cell was written.** The reason recorded for not
changing it was that the repair lands in crates the GIOP crate does not own.
**That reason has been overtaken.** The roster computed from the tree on
2026-08-29 reads *81 `Dispatch`/`SharedDispatch` impls, 59 override `knows`, 22
inherit it, of which 3 check the key in another hook* — and the production
inheritors went to zero on 2026-08-28 (`6034243`). Every remaining inheritor is
a test fixture. **The landing site the objection named is empty.**

**The work.** A `default_knows_policy()` that the default consults, defaulting
to the specification's, with the permissive behaviour available where a fixture
wants it. Lands with `a_key_nobody_activated.rs`'s existing controls shown red
against the new default, and with the roster re-read rather than re-typed.

**Watch for.** D029's backend cell quotes *"26 of the workspace's 72"* with no
date beside it, and the computed roster says 22 of 81 today. The figure is a
dated reading being read as a current one — the class this project calls *a
floor is not a figure*. Fix the cell in the same change, or it will be quoted
again.

*L1 — 유일하게 **측정이 이미 있고 초록인** 구멍이므로 첫째다. 바꾸지 않은 이유로
기록된 것("수리가 GIOP 크레이트가 소유하지 않는 크레이트에 착지한다")은 이미
뒤집혔다: 프로덕션 상속자는 2026-08-28에 0이 되었고 남은 22는 전부 테스트
픽스처다 — **반대 이유가 지목한 착지 지점이 비어 있다.** D029 셀의 "72 중 26"은
날짜 없이 인용된 옛 측정이며, 같은 변경에서 고친다.*

### L2 — Backend: two servants check the key in the wrong hook

`spikes/e2e/servant.rs` and `spikes/estate/servant.rs` do check the object key,
but in `dispatch_body` rather than in `knows`. So their **probe** path answers
`ObjectHere` for a key their own POA calls `Unknown` — the request/probe
disagreement that the `serve_one` reorder closed for a *moved* key, still open
for an *unknown* one.

Two files, and the roster already counts them (*"of which 3 check the key in
another hook"*). Smaller than L1 and in the same family, so it lands with it or
immediately after — but **not silently inside it**: a batch scoped to L1 that
also fixes these should say so, because a fix that only helps the item it was
handed is the thing this project's operating model refuses.

*L2 — 같은 계열이고 더 작다. L1과 함께 또는 바로 뒤에 착지하되, **조용히 섞지
않는다.***

### L3 — Activation: the mount is available and not taken

`crates/orbweaver-object/src/expert_host.rs` answers the ownership question —
*whoever owns an expert's residency owns its server* — with a servant that owns
a Persistent `AskLocator` POA and the `ExpertLoader` for the ids it mints keys
for, defaulting to `MissPolicy::Activate`. **Nothing constructs one.** The four
`spike_*` binaries and both services build none, so no deployment in this tree
behaves the way the closed leak says a deployment does.

**The work.** Wire one existing binary to it, so the property is exercised in a
served shape rather than only in a unit test. This is not a new capability: it
is the difference between *a leak closed in a type* and *a leak closed where a
caller could reach it*.

*L3 — 마운트는 **있고 취해지지 않았다**. 새 기능이 아니라, *타입에서 닫힌 구멍*과
*호출자가 닿는 자리에서 닫힌 구멍*의 차이다.*

### L4 — Language: a reference arriving is a handle the far side cannot invoke

What is left under this row is one thing and it is stated as one: a reference
*arriving* at a foreign servant is a handle it cannot invoke, which needs a
call travelling the other way through `orbweaver_gen::seam`.

It also carries the run's one language `SKIPPED`: *no leak test changes language
under a live caller yet; it waits on a Python servant mountable in a server the
test owns.* One piece of work retires both — which is the reason to take it
before anything that retires neither.

*L4 — 남은 것은 하나이며, 그 하나가 계수된 `SKIPPED` 하나도 함께 물러나게 한다.*

### L5 — Lifecycle: the second floor, a target *killed* rather than stopped

`Orb::shutdown` says §9.4.10's goodbye; a killed process leaves a reset, and a
caller can tell those apart. D034 and O1 measure the stop from a peer's own
socket; nothing measures the kill.

Named in D029 as a second floor rather than a leak, so **what is owed here is a
measurement, not a repair** — and possibly the finding that it is a floor too.
That distinction is the work: a floor that is measured and named costs nothing
to leave open, and one that is assumed is the same as unmeasured.

*L5 — 여기서 빚진 것은 수리가 아니라 **측정**이며, 그것이 바닥이라는 결론도
결과다. 측정되어 이름 붙은 바닥은 열어두는 데 비용이 없고, 가정된 바닥은
미측정과 같다.*

### L6 — Location: `moe::Router::select` hands out N addresses at once

`select` returns `ExpertSeq` — N object references, each an `Ior` stored
verbatim from `register_expert` and marshalled inline with host, port and object
key. A caller learns where every candidate expert runs.
`corpus/golden/22`'s own comment beside the operation already says so, and
§4.7's bearer-address rule is the authority half of the same fact.

**Recorded, not changed, and it needs a decision before it needs code**:
`select` is served and has consumers, so the choice is between changing a served
contract and accepting the leak with its reason written down. Do not open this
one by writing a patch — open it by writing the decision.

*L6 — 코드보다 **결정**이 먼저 필요하다. 이미 서비스되고 소비자가 있으므로,
계약을 바꾸는 것과 이유를 적고 받아들이는 것 사이의 선택이다. 패치를 써서 열지
않는다.*

---

## 2. The decision path already approved / 이미 승인된 결정 경로

D035's §8 records an ordering the owner set, and its step 1 landed on
2026-08-28. **This plan does not restate that decision's status** — read it
there. What this plan adds is only where its remaining steps sit against the
leaks above:

- Its step 2 is a **TAO fixture**, under the same terms as omniORB and JacORB —
  a separate-process wire peer and an external program whose output is read,
  **never a dependency**. Two rows move for one fixture: it is what could refute
  the step after it, and it retires the differential's standing `tao_idl` skip.
  **If it will not stand up, that is a result** and is what should be recorded.
- Its step 3 is conditional on step 2 existing. A cheap experiment with no
  possible refutation is not cheap.

Against §1: the TAO fixture closes no leak. It **buys refutability** for a step
that would otherwise be green because both ends are ours — which is why it sits
here rather than above L1.

*§1 대비: TAO 픽스처는 구멍을 닫지 않는다. 그것이 사는 것은 **반증 가능성**이며,
그래서 L1 위가 아니라 여기에 있다.*

---

## 3. Instrument debt — what makes the above measurable / 계기 부채

These close no leak. They are here because every item in §1 is judged by an
instrument, and an instrument that cannot go red makes §1 unfalsifiable.

1. **The ledger renderer truncates a cell at an embedded pipe.** The lifecycle
   cell ends `17 of this workspace's 63 serve sites pass ` in the harness's
   output; in D029 the sentence is *"pass `|| false` — fixable rather than
   fixed."* The renderer cut at the backtick-pipe. **Never conclude from a
   truncated read** — applied to the instrument that reports priority zero.
   Small, and it is the reader that must be fixed, not the sentence.
2. **17 of 63 serve sites pass `|| false`** (D029's figure, dated 2026-08-27 by
   the cell it sits in). Fixable rather than fixed, and a re-measurement is owed
   before the number is quoted again — a crude grep on 2026-08-29 returns
   different totals, which measures the method and not the tree.
3. **`guarded::Section` on the event-server registry mutex.** Extracted from the
   E3 branch judgement as worth doing and not landed.
4. **The tighter §4 bound** — *fetches nothing more* rather than *asks at most
   once more*, by re-reading the predicate where the outcome is recorded. The
   trigger is written into `event_server.rs`'s §4 docs; it was not taken because
   the supplier had already handed the event over.
5. **Harness split.** The top three groups took 49% of one run's wall clock and
   36 groups took under 2s each (measured 2026-08-27). A split is a change to
   how work is scheduled, not to what is measured, so it lands only with a
   demonstration that the same groups still run.
6. **Eight unlanded worktree branches**, of which one has been judged (E3 — not
   landed, with two things extracted from it) and seven have not. An unjudged
   branch is not a debt until somebody asks what is in it; it becomes one when a
   plan is written without knowing.

*이것들은 구멍을 닫지 않는다. §1의 모든 항목이 계기로 판정되고, 빨개질 수 없는
계기는 §1을 반증 불가능하게 만들기 때문에 여기 있다.*

---

## 4. What this plan deliberately excludes / 의도적으로 제외한 것

- **The Time Service.** Its chapter is `PLAN-DEFERRED.md` §3 and its clockless
  argument is now gated by a harness group, so the deferral is measured rather
  than asserted. Nothing in §1 waits on it.
- **D035's option A.** Deferred by the owner; not re-opened here.
- **Anything that adds a capability while a leak in §1 is open**, unless it is
  the only way to measure that leak. That is not a rule this plan invents — it
  is priority zero, and this section exists so that the exclusion is written
  down rather than left to be inferred from what is absent.

*여기 없는 것을 부재로 추론하게 두지 않으려고 이 절이 있다.*

---

## 5. How each item lands / 각 항목이 착지하는 방식

Unchanged from the operating model and repeated here only as the checklist this
plan will be held to:

- one fix per root cause, applied across every affected item;
- a new harness group lands **with its negative control in the commit
  message** — the command that was run to make it red, and what it printed;
- a control **lifts** the code it controls and never restates it, and the
  subject it strips is **synthesised**, never a live name;
- a figure in prose carries the date it was measured, or comes from a script
  that writes it;
- an unmeasured check is a failure, never a pass.

**One thing this plan owes that the model does not already say.** Five items in
§1 are leaks and one (L5) is a measurement; L6 is a decision. Reporting them as
one count would say a number that means three different things. Each is reported
as what it is.

*하나의 수로 합쳐 보고하면 세 가지 다른 것을 뜻하는 수가 된다. 각각을 그것인
채로 보고한다.*
