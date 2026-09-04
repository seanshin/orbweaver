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

### 1.0 The order, and what reviewing this plan changed about it

> **Every repair in §1 has landed as of 2026-08-31, and re-reading the criterion
> afterwards found one more instance under the Backend row** — measured, pinned,
> and asked in [`D039`](decisions/D039-what-a-servant-with-no-home-answers-for.md).
> That is §0's method working rather than a gap in it: the list here comes from
> the criterion at run time, so closing everything on it is the moment to read
> the criterion again, not the moment to stop.
>
> The five rows today, as the harness prints them: **1 held, 3 named floor,
> 1 open leak.**
>
> *§1의 수리는 전부 착지했고, 그 뒤에 기준을 다시 읽자 백엔드 행 아래에서 한
> 사례가 더 나왔다 — 재고, 고정하고, D039로 물었다. 목록을 다 비운 순간은 멈출
> 때가 아니라 기준을 다시 읽을 때다.* This block exists because the table below and
> the sections under it went on reading as future work for two days after
> `81cc546` closed rows 1 and 4 — while §L3's own text referred to L1's change
> in the past tense, so the document disagreed with itself. It was found by
> starting the batch and being stopped by the roster scan, which is the second
> time this project has paid for *a planning pass spent on finished work*. **A
> plan is where what landed against it gets written down**; nothing compiles a
> table of intentions, so the only thing that keeps one true is landing the
> record with the batch.
>
> *2026-08-31 기준 §1의 수리는 전부 착지했고, 남은 것은 소유자를 기다리는 결정
> 둘이다. 이 문단이 있는 이유는 아래 표가 `81cc546` 이후 이틀 동안 미래의 일처럼
> 읽혔기 때문이다 — 같은 문서의 §L3는 L1의 변경을 과거형으로 적고 있었으니
> 문서가 스스로와 어긋나 있었다. 배치를 시작했다가 명단 스캔에 막혀서 알았다.*

The first draft ordered §1 by *how open the leak is*, which put L1 first because
its measurement already exists and is green. Reviewing it found that L1's work
is **a design question, not a change** — the trait default has nothing to check
against and the one value it can be changed to is known to produce a vacuous
green. Ranking it first because it looked small was ranking it on an unchecked
guess about its size.

The order below is what survived that:

| | | why here |
|---|---|---|
| 1 | **L1's decision** — where the key set lives | **Done.** [`D036`](decisions/D036-what-a-servant-answers-for-a-key-nobody-activated.md) approved 2026-08-29, option A. |
| 2 | **L5** — measure the killed target | **Done 2026-08-29.** The one item measurement confirmed was independent, and it was: a second arm on an existing fixture pair, landed without touching anything else. |
| 3 | **L3** — the activation row's subject | Steps 1–2 done 2026-08-30. **Step 3 downgraded** on sizing: it adds a deployment shape and no measurement, and priority zero ranks a leak above a capability. |
| 4 | **L1 + L2 as one batch** | **Done 2026-08-29 in `81cc546`**, as one batch exactly as this row asked. The default is deleted, 22 implementations state their answer, and both L2 spikes moved their check into `knows` via `Poa::serves`. |
| 5 | **L4** — a call travelling the other way through the seam | **Done 2026-08-31.** D038 approved, option A; the seam is re-entrant and a foreign servant invokes a reference it was handed. This was the largest item in §1 and is the last leak in it. |
| 6 | **L6's decision** | **Done 2026-08-31.** D037 approved, option C, and §5's three conditions landed with it: the floor stated in the contract, asserted by a test with its own control, and the reason recorded as a reason. |

**Two items are decisions and one is a measurement.** Only three of the six are
repairs. A plan that reported *"six leaks"* as one number would be saying a
number that means three different things, which §5 refuses.

**L3 and L5 swapped places, and that is §4.5's fourth entry firing immediately.**
The draft called L3 *"independent of every other item"*. It is not, and the way
it is not was found by reading the code before starting rather than while:

* The counted `activation` row is fed by `leak_leg activation` →
  `what_a_caller_can_tell_about_load.rs`, which defines **its own** `ExpertHost`
  and leaves `knows` at the default `true` **on purpose** — the file says why,
  and the reason is good: *"the object's existence is the POA's decision here,
  not a second one taken in front of it."*
* The production mount `crates/orbweaver-object/src/expert_host.rs` is a
  **different servant with the same name**, and it **does** override `knows`.
  It has its own test, `a_mounted_expert_host_across_an_eviction.rs`, which runs
  under `cargo test --workspace` and does **not** count toward the row.
* So the row is measured against a servant that inherits the permissive
  default — and the roster names that exact file as one of the 22. **L3's
  subject is inside L1's population**, and L1's change alters what the
  activation row measures.

Neither file names the other. That is not a refusal and not a deferral — it is
a **silence**, between two servants sharing a name, one of which the ledger
counts and the other of which a deployment would use. Saying which is which is
the cheap half of L3 and comes first.

*L3와 L5가 자리를 바꿨고, 이것은 §4.5의 네 번째 항목이 곧바로 발동한 것이다.
초안은 L3를 "모든 항목과 독립"이라 적었다. 아니다 — 계수된 `activation` 행은
**테스트가 자기 안에 정의한** `ExpertHost`로 측정되고 그것은 기본값 `knows`를
상속하며(그 이유는 파일에 적혀 있고 타당하다), 같은 이름의 **프로덕션 마운트는
`knows`를 오버라이드하는 다른 서번트**다. 즉 **L3의 대상이 L1의 모집단 안에
있다.** 두 파일은 서로를 언급하지 않는다 — 거절도 유예도 아닌 **침묵**이다.*

*초안은 **구멍이 얼마나 열려 있는가**로 정렬했고 그래서 L1이 첫째였다. 검토가
찾은 것은 L1의 작업이 **변경이 아니라 설계 질문**이라는 것이다 — 작아 보인다는
확인되지 않은 추측으로 순위를 매긴 것이었다. 여섯 중 셋만 수리이고, 둘은 결정,
하나는 측정이다.*

### L1 — Backend: `Dispatch::knows` accepts a key nobody activated

> **DONE 2026-08-29 (`81cc546`), with L2 in the same batch.** The default is
> deleted and `knows` is required. **It closes nothing and D036 says so** — a
> servant that writes `true` leaks exactly as one that inherited it did — so
> what follows below is kept as written rather than edited into hindsight, and
> the row stays open.
>
> **What landed on 2026-08-31 is the part this section only flagged.** Its
> *Watch for* said D029's Backend cell quotes a dated figure with no date and
> *"fix the cell in the same change, or it will be quoted again."* It was not
> fixed in that change and it was quoted again — by the harness, which reads
> that cell at run time, on every run for two days. The cell now describes the
> leak that exists, and its figure is computed: the population is *a `knows`
> whose body never reads the key* (20 of 82 implementations), and the half of it
> a build emits is **0** and is now an assertion rather than an observation.
> The probe/request hunt, which had gone vacuous when D036 emptied the set it
> was scoped to, is live again over the new population and finds the one
> deliberate fixture.
>
> *L1은 2026-08-29에 L2와 한 배치로 착지했다. **아무것도 닫지 않으며 D036이 그렇게
> 적는다** — 그래서 아래 본문은 사후 시점으로 고쳐 적지 않고 그대로 둔다. 2026-08-31에
> 착지한 것은 이 절이 *Watch for*로 지목만 해 두었던 부분이다: D029 셀의 날짜 없는
> 수치를 같은 변경에서 고치지 않았고, 하네스가 그 셀을 실행 시점에 읽으므로 이틀
> 동안 모든 실행에 다시 인용되었다.*

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
That reason is **weaker than it was**, and the review of this plan on 2026-08-29
had to make it precise rather than let the first draft's *"every remaining
inheritor is a test fixture"* stand. The roster computed from the tree that day
reads *81 `Dispatch`/`SharedDispatch` impls, 59 override `knows`, 22 inherit it*,
and the 22 classify as:

| | |
|---|---|
| 20 | test fixtures — 11 below `src/server.rs`'s line-2200 `#[cfg(test)]`, 9 under `tests/` |
| 2 | `spikes/estate/servant.rs` and `spikes/e2e/servant.rs`, which are **not** test fixtures — they are L2 |

So the objection's landing site is not empty; it is **two files, and they are
the two L2 already names.** L1 and L2 are one batch.

**The work is not a default flip, and the first draft of this plan said it was.**
`fn knows(&self, _object_key: &[u8]) -> bool { true }` is a trait default with
no POA and no active object map in scope, so there is no
`USE_ACTIVE_OBJECT_MAP_ONLY` for it to be changed *to*. The only value it can be
changed to is `false` — **and that experiment has already been run and is
recorded in CLAUDE.md**: the leak test *stayed green* under a blanket `false`,
because a server that serves nothing answers both keys identically too. The
obvious repair produces a vacuous green, which is the failure mode this project
names most often.

**The design question is answered: [`D036`](decisions/D036-what-a-servant-answers-for-a-key-nobody-activated.md), approved 2026-08-29 — option A.** `knows` becomes **required**: the default is deleted so the gap is unrepresentable rather than detectable, with L2's two spikes in the same batch and the existing gate kept. D036 says plainly that **no candidate closes the leak** — what A buys is that the next production servant cannot leak by omission, which is how this one arrived. The approval accepts D036 §6.3's trap explicitly: **A's evidence is a compile error at 22 sites, not a test going red**, and `a_key_nobody_activated.rs` stays green across the change by construction.

**The shape of the question**: the default needs
something to check against, which means either servants declare their keys, or
the trait gains a required method, or `Server` keeps the active object map
itself and `knows` consults it. Each of those touches every implementation. This
is **not the small item the first draft ranked first** — see §1.0.

**How it lands.** With `a_key_nobody_activated.rs`'s existing controls shown red
against whatever the new default is, with the roster re-read rather than
re-typed, **and with the anti-vacuity companion that the blanket-`false`
experiment proves is required** — a counted demonstration that an activated key
and an unactivated one *can* be told apart, beside the assertion that a caller
cannot tell anything else.

**Watch for.** D029's backend cell quotes *"26 of the workspace's 72"* with no
date beside it, and the computed roster says 22 of 81 today. The figure is a
dated reading being read as a current one — the class this project calls *a
floor is not a figure*. Fix the cell in the same change, or it will be quoted
again.

*L1 — 측정이 이미 있고 초록인 유일한 구멍이다. 이 계획서의 초안은 남은 22를
"전부 테스트 픽스처"라고 적었고, 검토가 그것을 정확하게 만들었다: 20은 픽스처지만
2는 아니며, 그 둘이 바로 L2다 — 반대 이유가 지목한 착지 지점은 비어 있지 않고,
**두 파일이며 L1과 L2는 한 배치다.**

그리고 **작업은 기본값 뒤집기가 아니다 — 초안은 그렇게 썼다.** 트레이트 기본값의
스코프에는 POA도 활성 객체 맵도 없으므로 `USE_ACTIVE_OBJECT_MAP_ONLY`로 바꿀
대상 자체가 없고, 바꿀 수 있는 값은 `false`뿐인데 **그 실험은 이미 돌았고
CLAUDE.md에 기록돼 있다**: 일괄 `false` 아래에서 누출 테스트는 **초록으로
남았다** — 아무것도 서비스하지 않는 서버는 두 키에 똑같이 답하기 때문이다.
명백한 수정이 공허한 초록을 만든다.

그러므로 이것은 변경이기 이전에 **설계 질문**이고, 초안이 첫째로 놓았던 작은
항목이 아니다 — §1.0을 보라. D029 셀의 "72 중 26"은 날짜 없이 인용된 옛 측정이며
같은 변경에서 고친다.*

### L2 — Backend: two servants check the key in the wrong hook

> **DONE 2026-08-29 (`81cc546`), inside L1's batch and said out loud, which is
> what this section asked for.** Both servants call `orbweaver_object::Poa`
> `::serves` from `knows` — the read-only half of `dispatch_target`, which is
> why the two paths had diverged: `knows` takes `&self` and `dispatch_target`
> takes `&mut self`, so the request-path check could not be asked from `knows`
> at all.
>
> *L2는 L1의 배치 안에서, 이 절이 요구한 대로 **조용히 섞이지 않고** 착지했다. 두
> 경로가 갈라져 있던 이유는 가변성이었다.*

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

**The work, in the order the review found it.**

1. **Say which servant is which — and the draft got this half wrong.** Two
   `ExpertHost`s share a name: the mount here, whose `knows` answers for its
   own keys, and the one inside `what_a_caller_can_tell_about_load.rs`, which
   answers `true` for every key on purpose. This section said *"neither file
   names the other"*. **That is false in one direction**:
   `a_mounted_expert_host_across_an_eviction.rs` has named the isolated fixture
   since it was written, and explains the distinction — *"a test-private
   servant … which no deployment could run because it lives in a `tests/`
   file"* — better than this plan did. The silence was one-way, and the missing
   direction was written on 2026-08-29. **Done.**
2. **Decide which one the row should be measured against — and the answer was
   *both*.** The isolated servant exists so the POA takes the existence
   decision alone, which is a real property; the mount is what a deployment
   runs, and its file already measured this row through it with four controls
   of its own. What was wrong was not which one measured the property but which
   one **counted**: the mounted file ran under `cargo test --workspace` and
   declared nothing, so the ledger's `activation` row was fed by the isolated
   fixture alone — *a row measured only by a servant no deployment can run is a
   row measured beside the question rather than on it.* It is a harness group
   declaring `bears_on activation` now, and the row reads **2 groups** where it
   read 1. **Done 2026-08-29.**
3. **Wire one existing binary to the mount** — **downgraded 2026-08-30, after
   sizing it.** The step was justified as *the difference between a leak closed
   in a type and a leak closed where a caller could reach it*. **That difference
   was already gone when the sentence was written**:
   `a_mounted_expert_host_across_an_eviction.rs` binds a real server and dials
   it over a socket (`Connection::connect`, then `process`, `describe` and
   `delegate` as GIOP calls), so a caller does reach it, and since step 2 the
   ledger counts it.

   What step 3 would still add is a **deployment shape** — `spike_experts`
   serving the mount beside `ExpertService`, routed by `knows`, which would be
   a satisfying demonstration of the method D036 just made required. It adds no
   measurement. **Priority zero ranks a leak above a capability, and this is
   the capability**, so it stays named and unscheduled rather than done because
   it was on a list. It becomes worth doing the moment something needs an
   expert hosted in a running deployment; nothing does today.

   *크기를 재고 강등했다(2026-08-30). "타입에서 닫힌 것과 호출자가 닿는 자리에서
   닫힌 것의 차이"라는 근거는 그 문장을 쓸 때 이미 사라져 있었다 — 마운트
   테스트는 진짜 서버를 띄우고 소켓으로 다이얼한다. 남는 것은 **배포 모양**이지
   측정이 아니며, 0순위는 구멍을 기능보다 앞에 둔다.*

Ordered after L1 because step 2's answer could have changed under L1's change.
It did not — but the ordering was right for a reason that survives: this test's
`ExpertHost` was one of the 22 inheritors, so L1 rewrote a line of it, and the
sentence *"`knows` is left at its default"* in its rustdoc had to stop being
true in the same batch.

*L3 — 마운트는 **있고 취해지지 않았다**. 새 기능이 아니라, *타입에서 닫힌 구멍*과
*호출자가 닿는 자리에서 닫힌 구멍*의 차이다.*

### L4 — Language: a reference arriving is a handle the far side cannot invoke

What is left under this row is one thing and it is stated as one: a reference
*arriving* at a foreign servant is a handle it cannot invoke, which needs a
call travelling the other way through `orbweaver_gen::seam`.

**The `SKIPPED` half is done (2026-08-30) and the leak half is not.** That leg
waited on *a Python servant mountable in a server the test owns*, which
`orbweaver_gen::pychild::SeamChild` now is;
`crates/orbweaver-gen/tests/what_a_caller_can_tell_about_a_language.rs` swaps
Rust for Python behind one reference on one open connection, with its control
run by `leak_controls.sh`. Counted skips went 16 to 15 and every one of the five
transparencies is `MEASURED` by a leak leg with a live caller.

**What is left is the leak itself, and its decision is
[`D038`](decisions/D038-a-call-travelling-the-other-way.md)** — read its status
there; this plan does not restate it. What this plan carries is the order the
work lands in, below.

Sizing it changed what it is. It is not *a second message kind*; it is that
`Answerer::ask` is strict request/response and every way of letting the far side
invoke makes the seam **re-entrant** — the child sends a request while the
parent waits for a reply, and the parent must answer it from inside its own
`dispatch_body`. Three consequences that are properties rather than details: a
**deadlock shape that does not exist today** becomes reachable; **all three**
implementations of the protocol change; and the handle table, which is §4.7's
enforcement point, stops being read-only.

D038 recommends the nested-request candidate with three invariants that do not
move — the far side never learns an address, the handle table stays the
boundary, `ask`'s error contract is unchanged — and one rule the deadlock
forces: *the nested call is made on a connection the servant owns, never on the
one the request arrived on*. It refuses the *record it as a floor* candidate for
a reason worth keeping: **nothing about being written in another language makes
invoking impossible**, so filing it as a floor would be filing missing work as a
property, and a row that does that has stopped measuring.

It is the largest item in §1, and D038 asks for the owner's answer **before the
work starts** rather than after: the protocol has three implementations and a
re-entrancy no test currently reaches.

(`spikes/decision_status.py` refused a first wording of that sentence — it said
*"approved before it is started"* beside the decision's identifier and the gate
read it as a claim about **that decision's status**. Third time in three days
that decision-status vocabulary has been used here for something that is not a
decision's status; the gate has caught all three.)

**The plan for what is left has its own document, made 2026-09-02:**
[`PLAN-SEAM-JAVA.md`](PLAN-SEAM-JAVA.md). D038 was **APPROVED 2026-08-31**, so
the precondition this section names — *the owner's answer before the work
starts* — is met, and this is no longer blocked. **This section does not restate
that plan**; what belongs here is only how large the item turned out to be.

**Measured 2026-09-02, before planning.** Two of the protocol's three
implementations already carry the invoke direction — Rust (`seam.rs`'s
`resolve_nested`, `nested_refusal`, `PROTOCOL_VERSION = "2"`) and Python
(`python_rt.py`'s `ObjectRef.invoke`). **Java carries none of it**: a grep for
the envelope names in `java_rt.java` returns no line. So §1's largest item is
**one implementation**, and the two that exist are its specification — smaller
than this section had been carrying, because the work already done was done and
not because the sizing changed.

**And the plan's first step is not the obvious one.** Java was never enrolled in
`the_seam_is_one_protocol.rs`, whose `BINDINGS` array still holds a single row,
so Java publishes no protocol document and reads its envelope key as a hardcoded
literal — the defect that test exists to prevent. Nothing could go red about it.
The reasoning is in the plan document's §2; the shape is the one this repository
measured twice on the same day, **two gates green with each scoped to a place
while the rule is about a claim**.

*L4 — `SKIPPED` 절반은 2026-08-30에 끝났다(다섯 투명성 전부 살아 있는 호출자로
`MEASURED`). 남은 것은 테스트가 아니라 **프로토콜 추가**다: 도착한 참조를 호출하려면
반대 방향으로 가는 호출이 필요한데 seam에 그 메시지가 없다. §1에 남은 가장 큰 항목이다.*

### L5 — Lifecycle: the second floor, a target *killed* rather than stopped

`Orb::shutdown` says §9.4.10's goodbye; a killed process leaves a reset, and a
caller can tell those apart. D034 and O1 measure the stop from a peer's own
socket; nothing measures the kill.

**DONE 2026-08-29.** `spikes/orb_shutdown.sh` drives both arms from one fixture:
the graceful one calls `Orb::shutdown` on the servant's entry signal, the other
SIGKILLs itself at that same instant with the servant still held. Killed: one
`reset`, no reply, no goodbye. Stopped: a reply to request 1, then
`CloseConnection`.

**The measurement is the 2×2, not the two matching runs.** Each fixture is also
driven against the *other* expectation and required to be refuted — a run where
the peer expects what it gets proves nothing on its own, which is this project's
anti-vacuity rule read backwards: here the claim is that a caller *can* tell, so
what must be shown is that the two answers differ.

Two things the doing changed. **The arm was `abort()` first, and four hand runs
left five `spike-orb-shutdown-*.ips` crash reports** in
`~/Library/Logs/DiagnosticReports` — a harness group that files a crash report
on every run trains its reader to ignore crash reports, which is the opposite of
what the previous day established. SIGKILL leaves none and is the more faithful
model besides: a signal no handler can catch is what *killed* means. And it is
spawned (`kill -9 $$`) rather than called, because `unsafe_code = "forbid"` is a
workspace rule and `libc::kill` would need it.

The row does not move. A caller can tell, which is what a floor means; what
changed is that the floor can no longer stop being true without something going
red.

*L5 — 완료(2026-08-29). 측정은 맞는 짝 둘이 아니라 **2×2**다: 기대를 엇갈리게 건
두 실행이 반증되어야 한다 — 피어가 받을 것을 기대하는 실행은 그 자체로 아무것도
증명하지 않기 때문이다. 주장이 *구별할 수 있다*이므로 보여야 할 것은 두 답이
다르다는 것이다. 처음엔 `abort()`였고 손으로 네 번 돌린 것이 크래시 리포트 다섯
개를 남겼다 — 매 실행마다 크래시 리포트를 쌓는 그룹은 독자에게 크래시 리포트를
무시하도록 가르친다. 행은 움직이지 않는다: 호출자가 구별할 수 있다는 것이 바닥의
뜻이고, 바뀐 것은 그 바닥이 이제 조용히 참이 아니게 될 수 없다는 것이다.*

### L6 — Location: `moe::Router::select` hands out N addresses at once

`select` returns `ExpertSeq` — N object references, each an `Ior` stored
verbatim from `register_expert` and marshalled inline with host, port and object
key. A caller learns where every candidate expert runs.
`corpus/golden/22`'s own comment beside the operation already says so, and
§4.7's bearer-address rule is the authority half of the same fact.

**Its decision is [`D037`](decisions/D037-what-a-selection-hands-back.md)** —
read its status there. The shape of the answer is **a named floor**, not for
comfort but because the candidate that would close the row needs
`Router::dispatch`, which the project has declined to serve on separate grounds,
and the candidate that narrows it buys **displacement, which D035 has already
ruled is not closure**.

Its §2 is the part worth reading twice: whether a caller may know *which*
experts exist is not the question — it may, and a control-plane gate whose
callers cannot is not a gate. What does not carry across is **addresses**. Load
state has two contract homes where it is a value a caller asks for; an address
has none, so learning it is not being told, it is reading a fact off the
marshalled form of something given for another purpose.

§5 lists what acceptance must include to be worth more than silence, and §6.4
names the cost honestly: a criterion whose rows are mostly named floors is
measuring the shape of the repository rather than the transparency.

*L6 — 코드보다 **결정**이 먼저 필요하다. 이미 서비스되고 소비자가 있으므로,
계약을 바꾸는 것과 이유를 적고 받아들이는 것 사이의 선택이다. 패치를 써서 열지
않는다.*

---

## 1.9 What is left when §1 is empty / §1이 비었을 때 남는 것

**Re-derived 2026-09-01 from the tree, not from this document's memory** — which
is §0's method, and the third time this session it has produced something a
remembered list would have missed. §1's repairs are done; what follows is a
different kind of work and is ranked by priority zero all the same.

| | measured 2026-09-01 |
|---|---|
| the five rows | **1 held, 3 named floor, 1 open leak** (`transparency.py --statuses`) |
| harness | all measured checks green; **10** counted `SKIPPED` |
| decisions | **39 total — 14 APPROVED, 24 PROPOSED** |

*2026-09-01, 문서의 기억이 아니라 트리에서 다시 유도했다 — §0의 방법이고, 이번
세션에 그것이 기억된 목록이 놓쳤을 것을 찾아낸 세 번째다.*

---

### A. The one open leak's one instance — D039 / 유일한 열린 구멍

`backend` is the only row standing at `open leak`, and the roster pins its
deployable population at exactly one: `seam::ForeignServant`, whose `knows`
answers `true` when it has no home.
[`D039`](decisions/D039-what-a-servant-with-no-home-answers-for.md) recommends
**A — the servant is told which key it serves** — and is **not
self-approvable** for D036's reason: it changes what a public type answers a
peer for.

**It blocks nothing else and nothing else blocks it.** Ranked first because
priority zero ranks a leak above everything here, and because it is the only
item on this list that would move a row.

*유일한 `open leak` 행의 유일한 배포 사례. D039의 권고는 A이며, 그 상태는 그
문서에서 읽는다 — 여기서 다시 적지 않는다. (첫 판은 여기에 결정 어휘를 썼고
게이트가 거절했다: 이 문서가 D039의 상태를 주장하는 것으로 읽혔기 때문이다.
그 게이트가 결정 어휘의 오용을 거절한 것은 이번이 다섯 번째다.)*

---

### B. The decision backlog, and the thing nobody computes / 결정 적체

**24 of 39 decisions sit at `PROPOSED`, and no instrument distinguishes the two
kinds inside that number.** (Still true of the *instrument*: the separation
below was made by reading, and `decision_backlog.py` reports the signals a
person needs rather than the answer.) `spikes/decision_status.py` checks that every
*restatement* of a status matches its decision — it cannot check whether a
decision that says `PROPOSED` has in fact been decided, and that is a different
question with a different answer per document.

At least one is demonstrably the second kind. **D034** is `PROPOSED`, and its
question — *what a caller holding a reference sees when the ORB stops* — was
answered by the owner through D035's amended ordering, with the work landed
(`spikes/orb_shutdown.sh`, and §1's L5 done 2026-08-29). Its content is treated
as settled by D029's Lifecycle cell, which is a named floor **because of that
answer**.

So the backlog holds at least three populations, and until 2026-09-02 nobody had separated them:

1. **awaiting a genuine decision** — D037 and D038 were these until 2026-08-31,
   and D039 is one now;
2. **settled in fact, awaiting only a status edit** — D034 looks like this;
3. **superseded** — a question a later decision answered differently.

**The classification, made 2026-09-02.** One pass, each document read against
the tree. The evidence column says what was *checked*, never what was inferred
from a recommendation resembling something in `crates/`. Recompute the
mechanical half with `python3 spikes/decision_backlog.py`, whose `R`/`T` columns
are the two signals that separate *operated on* from *written about*: `R` = cited
by `CLAUDE.md`'s working rules, `T` = a `spikes/` script names the decision's own
file, so a run reads it.

**Settled in fact — the tree depends on the content, and no residue was found.**
Rejecting any of these would break something that is running.

| | evidence checked |
|---|---|
| **D010** | `R T`. `CLAUDE.md` carries §2's class split and §7.2's negative-control rule as working rules; a `spikes/` script reads the file. D016 §6 records the entire class-A programme executed with every commit verified, and nothing in class C built |
| **D029** | `R T`. `CLAUDE.md` names §6 the criterion's home and calls it priority zero; **the harness reads §6.1 at run time** and prints the ledger every run. This is the sharpest instance in the backlog |
| **D034** | `R`. `CLAUDE.md` carries §5.1 as a hard rule (*our own counters are not what a peer saw*); `spikes/orb_shutdown.sh` landed; D029's lifecycle row is a named floor **because of** its answer |
| **D033** | `T`. A script reads the file; its Stage 3 — C and Java, *named essential by the owner* — is complete: both binding grids 8 of 8, 0 red |
| **D031** | H2's ledger is computed and printed by every harness run and H4 landed; `run_checks.sh` cites it ten times |
| **D032** | All four items B1–B4 record landing; `binding_suite.sh` and `binding_words.rs` execute its rule, and the acceptance grid it defines is what both languages are measured by |
| **D028** | M1 and M2 landed; M2's rule is in `CLAUDE.md` as the `pub(crate)` rule, and `crossing_facts.py` runs |

**Executed, with a residue that is work rather than a decision.** Substantially
built; what is left is buildable and waits on nobody.

| | landed | residue |
|---|---|---|
| **D020** | Stage A (recorded in the document), Stage B — `IdAssignment` is a real policy in `policy.rs` | Stages C/D — POAManager, the standard names |
| **D021** | E2, E3, E4 — `event_channels_in_one_server.rs`, `spikes/event_by_name.sh` | E1, gated on §2's trigger |
| **D022** | `T`, T1; `lookup.rs` and `trading_server.rs` carry the surface | T2, T3 |
| **D023** | R3; phase 2 the document calls *already running* | phase 3, named as later |
| **D024** | `orbweaver-console` exists, with the ORB administration surface | §5's agent tools |
| **D025** | P1 — both inert keys reach `infer.rs` and `annotate.rs` — and P4 | P2, P3 |
| **D030** | L1 (the Python servant, 2026-08-30), L2, L4 | L3, the developer tools |

**In effect by default, and the evidence is an absence.** Both recommend
*building nothing*, so **the empty tree cannot testify** — it looks identical
whether the recommendation was adopted or nobody acted. What can testify is the
affirmative half, and it did:

| | evidence checked |
|---|---|
| **D012** | No cap outside tests; `COMPONENTS.md` documents the limit and cites D012 by name |
| **D013** | No identity map anywhere in `crates/`; `COMPONENTS.md` documents the boundary and cites D013 as *recommending not building* |

**Awaiting a genuine decision — population 1.** Something here waits on an answer.

| | what waits |
|---|---|
| **D039** | The criterion's one `open leak` has one deployable instance, and §5's recommendation A is unbuilt. §1 of this plan ranks it first |
| **D015** | The release half **landed** — v0.7.0 was cut 2026-08-26, the day after this was drafted. *"Then ask for a pilot"* is unanswered, and D016 §6 asks it again |
| **D016** | A question document: its §6 asks the owner two things — D010's status, and a pilot. Nothing but an answer can settle it |
| **D018** | *Which absence does a consumer meet first* — nothing in the tree presumes an answer, and its §4.1 says what must not happen before one |
| **D026** | S1, S2 and S4 landed (the harness ages every skip). **S3 is exactly the unanswered part**: the two paid fixtures still stand as counted `SKIPPED`, and §6 proposes spending money and CI minutes |
| **D017** | §5 costs harness time on every run, which the document says is a budget the owner holds |
| **D011** | Two clauses shipped — publish nothing, and `TelemetrySink` exists. The third did not happen as written: §10's row was resolved by **graduating** (`PLAN-SERVICES` §4, 2026-08-25), not closed as *deliberately not built*. Whether that satisfies the recommendation is the owner's reading, not mine |

**Could not classify from the tree — 1 of 24.** **D014.** Its `acted` marker is
set and its W1 items name defects, but D016 was drafted the same day and
rearranges the same work by footprint, so which document the work landed *under*
is not decidable from the tree. Recorded as unclassified rather than guessed.

**Two things this pass got wrong first, both worth keeping.** A keyword scan for
supersession raised a false hit against the priority-zero decision; reading the
context showed the matched words were *"leaks once instead of N times"* and
*"not §8's original C then B"*, inside a later decision amending **its own**
§8 ordering. **A keyword cannot read direction.** (Said this way on purpose:
the first draft of this paragraph put the two document numbers either side of
the word, and `decision_status.py` read the narration as a claim and went red —
correctly, since a gate cannot tell a reported false positive from an
assertion.) And for a *build-nothing* decision the first instinct was to read the empty
tree as adoption, which is this repository's own lesson that a green meaning
*nothing occurred* is indistinguishable from one meaning *the property held*.

*2026-09-02에 만든 분류. 24건 중 — **사실상 확정** 7건(트리가 내용에 의존하며,
거부하면 도는 것이 깨진다), **실행되었고 남은 것은 결정이 아니라 작업** 7건,
**부재가 증거인 것** 2건(아무것도 짓지 말라는 권고라 **빈 트리는 증언할 수 없다** —
증언하는 것은 긍정 쪽 절반이고, 그것은 착지했다), **진짜로 열려 있는 것** 7건,
**트리만으로 분류할 수 없는 것** 1건(D014, 추측하지 않고 미분류로 기록). 이 패스가
처음에 틀린 두 가지: 키워드는 **방향을 읽지 못한다**(D029가 D035에 의해 대체되었다는
오탐), 그리고 빈 트리를 채택으로 읽으려 한 것 — **아무 일도 일어나지 않은 초록과
성질이 지켜진 초록은 똑같이 보인다**.*

**What is mine to do and what is not.** Producing the classification is mine:
one pass, each document read against the tree, and a table saying which
population it is and on what evidence. **Approving is not** — that is what
`PROPOSED` means, and a classification that approved anything would be the
author's convenience wearing a decision's coat.

**Why this ranks above the cells below.** Everything in this repository is
chosen by reading these documents. A backlog where a settled decision and an
open one are spelled identically is the same defect as the Backend cell's stale
figure, one layer up — and this session has now paid for that class four times.

*39개 중 24개가 `PROPOSED`이고, 그 수 안의 두 종류를 가르는 계기가 없다.
`decision_status.py`는 **다시 적힌** 상태가 결정과 맞는지 검사할 뿐, `PROPOSED`라고
적힌 결정이 사실은 결정되었는지는 검사할 수 없다 — 다른 질문이다. D034가 그
두 번째 부류로 보인다. **분류는 내 몫이고 승인은 아니다.***

#### B.1 The classification, done 2026-09-01 / 분류 결과

`spikes/decision_backlog.py` reports the computable signals; the judgement below
is a person's and **approving is not part of it**.

**The pattern is one pattern, not three populations.** Nearly every `PROPOSED`
decision has its recommendation standing in the tree: D015 recommended cutting a
release and **seven have been cut**; D011's in-process fan-out ships in
`event_server.rs`; D012 and D013 recommended *building nothing* and are cited as
limits in `COMPONENTS.md`; D020's POA policies are types in eight files. Of the
24, **18 record in their own text that they were acted on** — a `Result,
appended`, a `Corrected`, an `amended`.

So the backlog is not mostly *undecided work*. It is mostly **work that went
ahead while the document's status never moved**, which is a different thing and
a smaller one — but it means the marker no longer separates *decided* from
*not*, and that is what makes reading these documents unreliable.

**The sharpest instance is D029, and it is the criterion itself.** `CLAUDE.md`
opens by saying priority zero was *"set by the project owner 2026-08-26"* and
names D029 §6 as its home. D029 says `STATUS: PROPOSED`. Five row standings, this
plan, and the harness ledger every run all rest on it.

`decision_status.py` cannot see this, and correctly: it compares **restatements**
of a status against the decision, and `CLAUDE.md` does not restate a marker — it
says the owner set the direction, which is a sentence about a different thing.
The gap is real and neither instrument was wrong.

**What this does not claim.** That any of the 24 *should* be `APPROVED`. A
recommendation standing in the tree is evidence that it was acted on, not that
it was approved, and the difference is exactly what a status marker exists to
record. Whether work proceeding ahead of an approval is a problem here or the
normal way this project moves is the owner's reading, not mine — and it is the
one question in this section I cannot answer by measuring.

**What would be wrong to do.** Editing 24 markers to match the tree. That would
make the gate green and destroy the only record of which decisions the owner
actually took, which is worse than the drift: a status that agrees with the code
by construction has stopped being evidence about anything.

*계산 가능한 신호는 스크립트가, 판단은 사람이 적는다 — **승인은 그 판단에 들어
있지 않다.** 부류는 셋이 아니라 하나였다: 대부분이 *결정되지 않은 일*이 아니라
**문서의 상태가 움직이지 않은 채 진행된 일**이다. 24개 중 18개가 자기 본문에
착지를 기록한다. 가장 날카로운 사례는 D029 자신이다 — `CLAUDE.md`는 0순위가
*"소유자가 정했다"*고 적고, 문서는 `제안`이라 적는다. **24개 표지를 트리에 맞춰
고치는 것은 틀린 수리다**: 게이트는 초록이 되고, 소유자가 실제로 내린 결정의
유일한 기록은 사라진다.*

---

### C. The two C cells — cheaper than they were called, and bounded / C 칸 둘

**Correction of record: these do not need a C runtime.** They were described
that way in this session's reporting and it was wrong. `spikes/c_peer.c` (1688
lines) already exists, speaks GIOP from the published specification, links no
ORB, and has **both roles** — it connects and it binds/listens — with
`--request-endian`, `--reply-endian` and `--giop` under the caller's control. So
`servant × c` is the shape `servant × omniorb` already has with `c_peer` as the
driver, and `client × c` is that with `c_peer` serving. Shell work over a
fixture the harness already gates.

**And what they buy is bounded by `spikes/bindings/AXES`, which decided it when
the peer landed and refused to answer it by declaring:**

> `independent` refutes coding errors and does NOT satisfy clause 6.

The C peer shares no code with `crates/`, so an error on our side is not
mirrored on the other — real evidence, and more than `self` can offer. It shares
the same reading of the same specification by the same process, and *a
convention both ends apply cannot be refuted by a round trip.* Clause 6 is
already met for Java in both directions, so these cells add evidence and close
no clause.

*기록 정정: C 런타임이 필요하지 않다. 이 세션의 보고가 그렇게 말한 것은 틀렸다.
`c_peer.c`는 이미 있고 두 역할을 다 한다. 다만 사는 것은 AXES가 이미 정해 두었다 —
`independent`는 코딩 오류를 반증하지 클라우즈 6을 충족하지 않는다.*

---

### D. The claims that are conditions, not work / 작업이 아니라 조건인 것

**Re-measured 2026-09-03.** Five `SKIPPED`s wait on something absent from
this machine rather than on anything undone — and the list moved twice this
week by *building* rather than by waiting: omniORBpy `sslTP` and JacORB SSL
were conditions until `spikes/tls/setup.sh` and `spikes/jacorb/SslServer.java`
made them fixtures, and multipass was "absent" in prose while installed in
fact. What is left: docker (here — **present on CI**, where the container probe
now runs and passes), a cluster for the k8s probe, `VOYAGE_API_KEY`, an
`ORBWEAVER_IDP_URL` issuer for CSIv2, and the live S1–S3 pass PLAN §8 puts on a
**per-release** cadence. The count the verdict prints is the honest one.

**One condition is a choice rather than an absence, and it is the owner's.**
`ci.yml` pins no Rust toolchain and there is no `rust-toolchain.toml`, so CI
lints with its runner image's stable (1.98 on 2026-09-03) while this machine
lints with 1.95. Two CI pushes went red that day on lints the local clippy
does not emit — a local green was never evidence about CI's clippy, and only
raising `rust-version` to its true 1.88 unlocked enough MSRV-gated lints on both
sides to show the sets differed. Pinning makes *clippy is green* mean one thing
on every machine and makes each Rust release a manual edit; not pinning is
what produced the two reds. Neither is free, and this document does not pick.

*2026-09-03 재측정. 다섯이 남았고, 이번 주에 목록은 기다려서가 아니라 **지어서** 두
번 움직였다 — SSL 둘은 픽스처가 되었고, multipass는 산문에서만 없었다. 그리고 부재가
아니라 **선택**인 조건이 하나 있다: CI가 Rust 툴체인을 고정하지 않아 CI는 1.98로,
이 머신은 1.95로 lint한다. 로컬 초록은 CI의 clippy에 대한 증거였던 적이 없다. 고정은
매 릴리스마다 손을 타고, 미고정은 오늘 빨강 둘을 냈다. 소유자의 몫이다.*

*열 중 여섯은 이 머신에 없는 것을 기다리지, 하지 않은 일을 기다리지 않는다. 계수되고
나이가 붙어 있으며 지금 그대로가 정확하다.*

---

### E. TAO in CI — the owner's, and measured / TAO를 CI에

Not a leak and not a skip: the `tao_idl` column is retired wherever the fixture
is built, and CI is where it is not. The cost is measured rather than estimated —
the differential job takes **57 seconds** and the build takes **~3 minutes and
532 MB**, so it multiplies that job by roughly four on every push, and caching a
DOC-licensed tree is a licensing judgement `spikes/tao/setup.sh` declines to make
alone. The refusal and its cost are in that script's header; the choice is §2's
to record when it is made.

---

### F. What is buildable now, as parallel lanes / 지금 지을 수 있는 것 — 병행 차선으로

**Derived 2026-09-04 from the tree, not from this document's memory** (§0's
method). `transparency.py --statuses` prints 1 held, 3 named floor, 1 open
leak; the open leak's item is row 1 above and waits where it waited. Rows 4–6
are done. What remains buildable without an owner's answer is **§3's items 2,
3 and 5** and nothing else: §3.1 is fixed, §3.4's trigger has not fired (the
module's own docs say the choice is not its alone to make), and every residue
in §B's *executed with residue* table is a capability, which §4 excludes while
a §1 row stands open. This section adds only footprints and ordering — each
lane's substance stays home in §3.

*트리에서 유도했다(2026-09-04), 이 문서의 기억이 아니라 — §0의 방법. 원장은
1 held, 3 named floor, 1 open leak을 찍고, 그 열린 구멍은 위 1행이며 기다리던
자리에서 기다린다. 4–6행은 끝났다. 소유자의 답 없이 지을 수 있는 것은 **§3의
2·3·5번**뿐이다: §3.1은 고쳐졌고, §3.4의 방아쇠는 당겨지지 않았으며(그 모듈의
문서가 스스로 혼자 정할 일이 아니라고 적는다), §B의 *잔여가 있는 실행* 표의
잔여는 전부 기능이라 §1의 행이 열려 있는 동안 §4가 제외한다. 이 절은 발자국과
순서만 더한다 — 각 차선의 내용의 집은 §3이다.*

| lane | home | footprint | how it runs |
|---|---|---|---|
| **A** | §3.3 — `guarded` on the event server's registry mutex | `crates/orbweaver-giop/src/event_server.rs`, one file: the one registry `Mutex` (one by that file's own sharing argument) and its lock sites | worktree agent; local oracle `cargo test -p orbweaver-giop` + clippy, the harness's event groups at landing — **done 2026-09-04** (`fea8749`, landed `b054b36`, harness green) |
| **B** | §3.2 — the repair half of the unstoppable serve sites | the sites `spikes/serve_sites.py --list` prints — **21 of 85 on 2026-09-04**: fifteen files in five crates plus `spikes/e2e/servant.rs` and `spikes/estate/servant.rs`. No file shared with lane A, nothing in lane C's script | worktree agent; oracle is `serve_sites.py` re-run plus each touched fixture's own harness group at landing. D029's lifecycle-cell figure moves **in the same change** — that cell's own rule — **done 2026-09-04** (`8ca4c69`, landed `6549b68`): 9 defects with one cause, 12 refusals recorded at their sites, and the caution below fired exactly as written |
| **C** | §3.5 — the harness split | `spikes/run_checks.sh` itself | **the coordinator's lane, serial, on `main`** — the gate is not edited by the work it judges, and the machine-wide lock is the coordinator's. Its first commit is the demonstration §3.5 already requires (the same groups still run), not the split |

**Lane cautions, each one already paid for once:**

- **Lane B is scoped to the rule, never to the 21** — *a server a fixture
  starts is stoppable by the route its own teardown uses*. Some of the 21 are
  plausibly refusals: `orb_stops_what_it_handed_out.rs`'s two sites sit inside
  the test that measures stopping, where a deliberately unstoppable contrast
  arm is exactly what an anti-vacuity companion looks like. A refusal is
  recorded where it sits, not converted. If clustering finds the 21 are mostly
  refusals, the lane's honest yield is recorded reasons rather than
  conversions — a result, not a failure. The count is a lower bound and the
  classifier says so itself.
- **Lane C starts only while no other lane is landing** — never edit the
  harness while it runs, and a split mid-wave would change the gate under the
  branches it is about to judge.
- **Landing is serial and the coordinator owns it**: one branch merged at a
  time, the full harness per merge, agents never run `run_checks.sh` (local
  oracle only) and report a recommended group without applying it.
- **Coordinator pre-step, lock free:** `spikes/reclaim.sh` beside its own
  measurement — `target/debug/deps` stood at 456k files / 17G at 2026-09-04's
  close, and the 858k incident cost a 50-minute `cargo test`. Housekeeping,
  not a lane; no threshold is a gate.

**Not scheduled, and why, so absence cannot be read as oversight:** the §B
residues stay named where they live (their homes are the decision documents;
this section does not restate them); §3.4 keeps its unfired trigger; the two
transients (`os error 35` on a GIOP 1.0 ping, one red `GIOP 1.1 against
JacORB` in harness 99) are observation during lane C's landing runs rather
than lanes — *did not reproduce in N runs* stays a valid result; and §D's
conditions and §E wait where they wait.

*차선 주의사항 — 전부 한 번씩 값을 치른 것들이다: **B차선의 범위는 21이 아니라
규칙이다**(픽스처가 띄운 서버는 그 픽스처 자신의 해체 경로로 멈출 수 있어야
한다). 21 중 일부는 거절일 수 있다 — `orb_stops_what_it_handed_out.rs`의 두
지점은 멈춤을 재는 테스트 안에 있고, 일부러 멈출 수 없게 만든 대조 팔은
반공허성 동반자의 모양 그대로다. 거절은 그 자리에 기록하지 변환하지 않으며,
군집화가 21의 대부분을 거절로 밝히면 이 차선의 정직한 산출은 변환이 아니라
기록된 이유들이다 — 그것은 실패가 아니라 결과다. **C차선은 다른 차선이 착지하지
않는 동안에만** 시작한다 — 도는 하네스는 편집하지 않는다. **착지는 직렬이고
코디네이터의 몫이다**: 한 번에 한 브랜치, 병합마다 전체 하네스, 에이전트는
`run_checks.sh`를 돌리지 않는다. 사전 단계로, 락이 비었을 때
`spikes/reclaim.sh` — `target/debug/deps`가 2026-09-04 마감에 456k 파일 / 17G였고,
858k 사건은 `cargo test` 50분을 치렀다. 예약하지 않은 것과 그 이유도 적는다 —
부재가 간과로 읽히지 않도록: §B의 잔여는 제 집(결정 문서)에 이름 붙은 채 남고,
§3.4는 방아쇠가 당겨지지 않았으며, 두 과도 현상은 차선이 아니라 C차선 착지
실행 중의 관찰이고(*N번 실행에서 재현되지 않았다*는 유효한 결과다), §D의
조건들과 §E는 기다리던 자리에서 기다린다. — 갱신 2026-09-04: A·B 차선은
착지했다(날짜와 커밋은 표의 칸에); B차선에서는 위 주의사항이 적힌 그대로
발동해 21 중 12가 거절로 판명되었다.*

---

### The order, and why / 순서와 이유

| | | why here |
|---|---|---|
| 1 | **D039** | the only item that would move a criterion row; awaits the owner |
| 2 | **B, the classification** | ~~everything else is chosen by reading these documents, and two kinds of `PROPOSED` are spelled the same~~ — **done 2026-09-02**, and its own §B holds the table. It came out a repair rather than a report: 16 of 24 are executed in fact, so the ranking below stands |
| 3 | **C, the two cells** | ~~cheap, and honest about buying evidence rather than coverage~~ — **done**: both grids run 8 cells, 0 skipped, 0 red, and the C cells are in both |
| 4 | **L4's third implementation** | D038 was approved 2026-08-31, so §1's largest item is unblocked and is now **one implementation** — Rust and Python carry the invoke direction, Java carries none of it. The only §1 item left that is work rather than a decision. Its plan is [`PLAN-SEAM-JAVA.md`](PLAN-SEAM-JAVA.md), whose first step is a **red test** rather than a feature: Java was never enrolled in the seam's protocol-agreement test — **done 2026-09-02/03** (`add6ba9`, `3c91c4f`): Java is enrolled, its `pinned` difference list emptied in one day, the invoke direction landed, and the work found a leak on the way (`2dda795`, a Java servant could not tell its objects apart) |
| 5 | **the grid's unread versions** | ~~client GIOP 1.0 in both languages, python client 1.1, python servant 1.0 — coverage, not transparency~~ — **worked 2026-09-03 and it was not coverage.** Python's 1.1 was a missing loop in one cell (now `spikes/lib/giop_versions.sh`, shared). **1.0 is an interoperability defect**: `orbweaver_dynamic::invoke` requires a wide codec before marshalling anything, so every call on a GIOP 1.0 connection is refused, `ping()` included. Its own batch — **done 2026-09-03** (`bf408d9`): `invoke` refuses only what the signature in hand needs, a 1.0-only peer is callable, and both grids run 8 cells with 0 red |
| 6 | **the NAT probe's record** | [`PLAN-NAT-PROBE.md`](PLAN-NAT-PROBE.md), 2026-09-03. CI's `NAT rewriting` group costs 202s against 10s here; the container probe's header says it has never run; the harness cannot say whether it did, because it reads a skip *count* where the script prints *which*. S1 makes the harness say — and the plan's own first draft cited a log line as proof that turned out to be another group's, recorded there rather than smoothed over — **done 2026-09-03** (`b438d3b`): both lanes merged; the probe had been failing under a false skip, and the declared MSRV was false |
| 7 | **D and E** | conditions and a costed choice, not work |

**What would make this plan wrong.** If the classification in B finds that the
24 are all genuinely open, then B is a report rather than a repair and C should
have gone first. That is checkable by doing B, which is the argument for doing
it before ranking anything under it — the same mistake §1.0 records making about
L1, where ranking on an unchecked guess about size put the wrong item first.

**Answered 2026-09-02, and the plan is not wrong.** B was done and came out a
repair: **16 of 24 are executed in fact** — 7 the tree depends on, 7 executed
with a residue that is work rather than a decision, 2 in effect by default — and
**7 genuinely await an answer**, with 1 (D014) recorded as unclassifiable from
the tree rather than guessed. Had it gone the other way the ranking would have
had to change; it did not, so C keeps its place.

*순서 갱신(2026-09-02): 2번(B)과 3번(C)은 **끝났다**. 새 4번은 **L4의 세 번째
구현** — D038이 2026-08-31에 승인되어 §1의 가장 큰 항목이 풀렸고, 이제 남은 것은
**구현 하나**다(Rust와 Python은 반대 방향을 싣고 있고 Java에는 한 줄도 없다).
결정이 아니라 작업인, §1에 남은 마지막 항목이다. 새 5번은 그리드가 스스로
미측정이라 적은 GIOP 버전들 — 투명성이 아니라 커버리지이므로 L4 아래에 둔다.
1번(D039)은 그대로다: 기준의 행을 움직일 유일한 항목이고, 소유자를 기다린다.*

*이 계획이 틀리는 경우: B의 분류가 24개 전부 진짜로 열려 있다고 밝히면 B는 수리가
아니라 보고이고 C가 먼저였어야 한다. 그것은 B를 해 보면 확인된다 — §1.0이 L1에
대해 기록한, 크기에 대한 확인되지 않은 추측으로 순위를 매긴 실수와 같은 부류다.*

**Order update 2026-09-04.** Rows 4–6 are done (the dates and commits are in
their cells), so every numbered row above is now either done or an answer only
the owner can give — the table has stopped ordering buildable work. What is
buildable today without an owner's answer is **§3's instrument debt**, and §F
above holds its lane plan; everything else remaining is a capability §4
excludes while the backend row stands open, a condition (§D), or the owner's
(rows 1 and 7, §B's approvals, §E).

*순서 갱신(2026-09-04). 4–6행이 끝났다(날짜와 커밋은 각 칸에 있다). 이제 위 표의
모든 행은 끝났거나 소유자만이 답할 수 있는 것이다 — 이 표는 지을 수 있는 일의
순서표이기를 멈췄다. 소유자의 답 없이 오늘 지을 수 있는 것은 **§3의 계기 부채**이고,
그 차선 계획은 위 §F에 있다. 그 밖에 남은 것은 backend 행이 열려 있는 동안 §4가
제외하는 기능이거나, 조건(§D)이거나, 소유자의 몫(1·7행, §B의 승인들, §E)이다.*

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

  **DONE 2026-08-31.** It stood up: `spikes/tao/setup.sh` builds `tao_idl`
  4.0.7 from the ACE+TAO 8.0.7 source in about three minutes (Homebrew's `ace`
  formula downloads that tarball and builds only `ace/`, so there is no
  packaged `tao_idl` to install). The differential runs **99 files through
  three front ends** and finds no unexplained divergence; the standing
  `SKIPPED tao_idl absent` is retired wherever the fixture is built, and still
  printed, correctly, where it is not.

  **What running it found is worth more than the fixture.** The `tao_idl`
  column had been written against an absent oracle and had never been
  executed: its verdict function leaked TAO's own exit status — **2** on a
  parse error — into a protocol with room for `0` and `1`, so every correct
  rejection read as a divergence. And it asked the oracle about **IDL 3**,
  TAO's default, while this corpus is IDL 4.2. The first run reported **37**
  unexplained divergences; those two causes account for **29** of them.
  The remaining **8** are real, each narrowed to a rule by probe and recorded
  in `corpus/divergences.tsv` — and three of the probes did not reproduce on
  the first shape tried, which is the only reason the rules are narrower than
  the files.

  *2026-08-31 완료. 픽스처는 섰다. 그리고 **세운 것보다 돌린 것이 더 많이
  찾았다**: `tao_idl` 열은 오라클이 없는 채로 쓰였고 한 번도 실행된 적이 없었다 —
  파스 오류의 종료 코드 **2**가 0/1 프로토콜로 새어 들어가 모든 올바른 거절이
  불일치로 읽혔고, 코퍼스가 IDL 4.2인데 오라클에는 TAO 기본값인 **IDL 3**을
  물었다. 37건 중 29건이 그 둘이었고, 남은 8건은 진짜이며 각각 규칙으로 좁혀
  기록했다.*
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

1. ~~**The ledger renderer truncates a cell at an embedded pipe.**~~ **Fixed
   2026-08-30.** `transparency.py` split each markdown row on every `|`, so a
   cell containing an inline-code pipe was cut in half: the Lifecycle cell
   reached the reader as `17 of this workspace's 63 serve sites pass ` and
   stopped, losing *`|| false` — fixable rather than fixed*. The splitter
   respects backtick spans now. Control: the old splitter, lifted from
   `git show HEAD:`, **run inside the repository** — the first attempt ran it
   from the scratchpad, where `ROOT` resolves elsewhere and it could not find
   D029 at all, which is the same mistake this session made once already and
   would have read as a passing control.

   The sentence was not escaped in D029 to suit the parser: that would make the
   document worse for every other reader to spare this one. *The tool is the
   thing to fix.*
2. **17 of 63 serve sites pass `|| false`** (D029's figure, dated 2026-08-27 by
   the cell it sits in). Fixable rather than fixed, and a re-measurement is owed
   before the number is quoted again — a crude grep on 2026-08-29 returns
   different totals, which measures the method and not the tree.
   **The re-measurement half became `spikes/serve_sites.py` on 2026-08-30, and
   the repair half is done 2026-09-04** (§F lane B, `8ca4c69`): 12 of 85, every
   one a recorded refusal, 0 unexplained. The figure's home is D029's Lifecycle
   cell; it is not restated here.
3. **`guarded::Section` on the event-server registry mutex.** Extracted from the
   E3 branch judgement as worth doing and not landed. **Done 2026-09-04** (§F
   lane A, `fea8749`): a `Section` under the same `Mutex`, the sharing decision
   untouched — and the doc sentence that had promised the tripwire watches this
   lock became true rather than being edited.
4. **The tighter §4 bound** — *fetches nothing more* rather than *asks at most
   once more*, by re-reading the predicate where the outcome is recorded. The
   trigger is written into `event_server.rs`'s §4 docs; it was not taken because
   the supplier had already handed the event over.
5. **Harness split.** The top three groups took 49% of one run's wall clock and
   36 groups took under 2s each (measured 2026-08-27). A split is a change to
   how work is scheduled, not to what is measured, so it lands only with a
   demonstration that the same groups still run.

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

## 4.5 Preconditions, and what would make this plan wrong / 전제조건과 반증

**A precondition, not a work item — and it is discharged.** Eight worktree
branches are unlanded, all from 2026-08-26. Planning §1 without knowing what is
in them risks planning work that already exists, which is a precondition rather
than an item: cheap to discharge, expensive to skip. The first draft filed it as
instrument debt, which was the wrong shelf.

Discharged 2026-08-29:

| branch | verdict |
|---|---|
| `a1591880` | superseded. Its D031 **is in main** and its ledger landed (38 `bears_on` tags). Its Rust `orbweaver-test/src/transparency.rs` was superseded by `spikes/transparency.py`, and by the better answer: the Python reader takes the five names from D029 §6.1 with no fall-back list, where the Rust one would have been a **second home** for them. |
| `a194b897` `a5ba8ca8` `aacf7741` `ae15423a` | nothing to take: every artifact these four carry — `plan_numbers.py`, the language-binding decision document, `CHANGELOG`/`COMPONENTS` and `ifr_walk_peer.py` — is already in main. |
| `a9bb59bf` | superseded. **This is L4's territory** — a Python bridge and servant — and `py_bridge.rs` and `python_rt.py` are in main and have diverged from it (458 lines the branch adds, 173 main has that it lacks). |
| `ac991408` | superseded. **This is L1's territory** — `server.rs` and locate/forward — and `locate_forward_and_reply_contexts.rs` is in main while `server.rs` has moved **493 lines beyond** the branch. |
| `adaa637f` | judged 2026-08-28: **not landed** (E3), with two things extracted from it. |

`spikes/decision_status.py` refused a first wording of that row too — it said
*"superseded"* beside a decision's identifier and the gate read it as a claim
about **that decision's status**, which was the second time in this document
that decision-status vocabulary was used for something that is not a decision.
The row says what it means now.

**What was read and what was not.** Presence, divergence direction and the two
decision documents were read; the branches' small unique deltas (66 lines of
`server.rs` on `ac991408`, for instance) were not read line by line. The
question the precondition asks is narrower than that and is answered:
**no branch closes a §1 leak**, because main's ledger still reports every one of
them open, and main is ahead of both branches that touch their code. A branch
holding a closure would have shown as a closed row or as the fix itself.

*해소됨. 전제조건이 묻는 것은 "어느 브랜치가 §1의 구멍을 닫는가"이고, 답은 **아니오**다
— main의 원장이 여전히 전부 열린 것으로 보고하고, 두 겹치는 브랜치의 코드에서
main이 앞서 있기 때문이다. 각 브랜치의 고유 델타를 한 줄씩 읽지는 않았고, 그렇게
적는다.*

**What would make this plan wrong**, stated so that finding out is a result
rather than an embarrassment:

1. **If closing all six still leaves the ORB incomplete** — and it does. Two
   floors are named in D029 and neither moves: a caller of a removed target must
   be given one address to send a first packet to, and a caller that resolves a
   name learns the address it resolved to. This plan closes leaks *above* the
   floors. Anyone reading it as *"six items and then it is done"* is reading it
   wrong, and that is the reading this paragraph exists to refuse.
2. **If L1's design question has no answer that is not a vacuous green.** The
   blanket-`false` experiment is the warning, not the boundary — but if every
   candidate turns out to be *the server serves nothing*, then the honest
   outcome is a recorded refusal with its reason, not a change.
3. **If a leak cell is stale.** Two figures in the §6.1 table were quoted
   without their dates, and a re-measurement on 2026-08-29 has already overtaken
   one of them — the roster reads 22 of 81 where the cell says *"26 of 72"*.
   This plan is built on those cells; a cell whose figure has moved makes the
   item built on it wrong. §3's first two entries exist because of that.

   `spikes/decision_status.py` refused the first wording of this paragraph:
   it said the figure *"is already superseded"* next to the decision's name, and
   the gate read that as a claim about the **decision's status**. It was right
   to — a reader can misread it the same way, and a document that plans against
   a decision must not look like it is restating one.
4. **If the ordering is wrong because the sizes are wrong.** L1 was reordered
   once already, on discovering its size. L3, L4 and L5 have not been sized by
   anything better than reading the cell — which is exactly the mistake L1's
   reorder corrects. **Size each before starting it, not while.**

*여기 있는 것을 반증하는 방법을 적어둔다 — 알아내는 것이 결과가 되도록, 창피가
되지 않도록. 여섯을 다 닫아도 ORB는 완성되지 않는다: 바닥 둘은 움직이지 않고 이
계획서는 그 **위**의 구멍을 닫는다. 그리고 L1은 크기를 잘못 재서 이미 한 번 순서가
바뀌었다 — 나머지도 시작하기 전에 크기를 잰다.*

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
