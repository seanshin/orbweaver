# D033 — The programme: every open item, what its first completion is, and the order

**STATUS: PROPOSED** — drafted 2026-08-26 on a direction to turn each unfinished
item into something that can be driven to a first completion. It is a
**sequencing decision**, not new work: every item below already has a home in
D026–D032 and is cited, never restated. Not self-approvable: §2 proposes a
definition of *first complete* that decides when a batch may stop.

**상태: 제안** — 2026-08-26, 미완 상태를 하나씩 1차 완료로 갈 수 있는 구성으로
기획하라는 지시에서 작성.

> **Priority zero.** The criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6. Ordering below is by
> what closes a leak, not by what is cheap.

> **Observed 2026-08-26, reported and not acted on.** `D029` is **PROPOSED**,
> and its §6 is what `CLAUDE.md` points every plan document at as priority
> zero, what this document orders itself by, and what four other documents
> cite as their governing rule. **A document whose content is in force as the
> project's standard while its own status says proposed is a state worth
> naming**, and naming it is all a batch may do: the STATUS line is the
> owner's, and this project's approval discipline is the thing that would be
> spent by a batch that promoted a document because it was already being
> obeyed. Nothing here asks for it to change — an owner may well intend a
> criterion to govern while its wording is still open, which is a different
> thing from drift. It is recorded in a place `spikes/decision_status.py`
> reads, so that if the status does change this sentence goes red rather than
> quietly stale.
>
> *2026-08-26 관찰, 보고할 뿐 조치하지 않는다. `D029`는 **제안** 상태인데 그
> §6은 이미 프로젝트의 기준으로 쓰이고 있다. **이름을 붙일 값이 있는 상태이며,
> 이름을 붙이는 것이 배치가 할 수 있는 전부다** — STATUS 줄은 소유자의 것이고,
> 이미 지켜지고 있다는 이유로 문서를 승격시키는 배치가 바로 그 승인 규율을
> 소모한다. 이 문장은 `decision_status.py`가 읽는 자리에 둔다.*

---

## 1. Why a programme document, and why now / 왜 지금 이 문서인가

Measured 2026-08-26: **nine branches wait to merge, five of them WIP sealed
after batches were stopped mid-flight**, and six plan documents (D026–D032) hold
about thirty proposals between them. That is not a shortage of plans. It is a
shortage of **finish lines**.

> **Re-measured 2026-08-26 14:07, and the shape changed more than the count.**
> **Eight** branches wait, and **all eight are WIP-sealed** — not five of nine.
> They are two kinds and the difference matters: **five** still say *"WIP
> preserved: batch stopped by the owner mid-flight, committed unreviewed so
> nothing is lost"* — the same five §3's 0.3 is about, untouched — and **three**
> say *"WIP sealed before worktree cleanup (already-merged batch)"*, which is
> residue of work that did land, not work waiting to. So the four branches that
> were merely *waiting* merged (§3's 0.2), and the debt §3 calls 0.3 is
> unchanged at five. **The count is measured before this batch's own branch
> exists**; committing this makes it nine again, which is the reason a branch
> count is a poor gate and is quoted here with a timestamp rather than pinned.
>
> *2026-08-26 14:07 재측정 — 아홉 중 다섯이 아니라 **여덟 전부**가 WIP다. 다만
> 두 종류다: 중단된 다섯(0.3의 그 다섯, 그대로)과 이미 병합된 배치의 잔여 셋.
> 단순히 대기하던 넷은 병합되었다. 이 배치 자신의 브랜치를 세기 전의 숫자다.*

Every one of those proposals says what to build. **None says what makes it
done**, so a batch either overruns or stops where it happens to stop — and five
did stop mid-flight today, leaving partial state that is *not a result*.

*계획이 모자란 것이 아니라 **결승선**이 모자란다.*

## 2. What "first complete" means / "1차 완료"의 정의

**An item is first-complete when a gate would go red if it regressed, and the
thing it does not yet do is written down where the next reader will find it.**

Three clauses, each refusing a common way to declare victory:

1. **A gate, not a demo.** A spike that ran once is not first completion; a
   group in the harness or a test in `cargo test` is. This project has measured
   nine ways a green gate can measure nothing — the gate needs its negative
   control, and *the control is the finish line*.
2. **A stated remainder.** Not "done" but "done to here, and here is the rest,
   named." A leak found and recorded advances the criterion; a leak silently
   left does not. D029 §6.1's table is where a transparency's remainder lives.
3. **No partial landing of an atomic thing.** If half of it turns the other
   half red, it lands whole or not at all — D016 §4's class, and D019 step 4
   was scoped by exactly this rule.

*게이트(시연이 아니라), 이름 붙은 잔여, 그리고 원자적인 것의 반쪽 착지 금지.*

## 3. Stage 0 — the debt that blocks measurement / 측정을 막는 부채

**Nothing below can be trusted until this is clear**, because every later
oracle runs on the merged tree.

| | Item | First complete when |
|---|---|---|
| 0.1 | the frozen harness run finishes | its verdict is read on a tree that did not change under it — and if it did, the run is discarded and repeated, no exceptions |
| 0.2 | four committed branches merge | `cargo check --workspace --all-targets` clean **on the merged tree**, which is where six breaks were found today |
| 0.3 | five WIP branches triaged | each is **re-measured from scratch or abandoned**; a stopped batch's partial state is not a result and must not be merged as one |
| 0.4 | records land | `records_keep_up.py` green |

**0.3 is the one that will be tempting to skip.** Those five contain real work —
the ledger, the gates, the seam, two spec gaps, Event E3 — and none of it was
verified. Merging unverified work because it looks finished is how the day's
six merge breaks would become seven.

> **Status corrected 2026-08-26, later the same day. 0.1 and 0.2 are done; 0.3
> is not, and its sentence above is now wrong about its own contents.**
>
> - **0.1 — done, and the verdict was red before it was green.** The frozen run
>   finished and was read: five failures over two causes, both repaired, with
>   the finding that the causes were the coordinator's own. What the run said
>   and what the repair was is in `CHANGELOG.md`'s Unreleased section, not
>   restated here.
> - **0.2 — done.** The four committed branches merged; the merge breaks are
>   what commissioned `spikes/crossing_facts.py`, D028 §4 M2, which is a
>   **report and not a gate** by its own instruction.
> - **0.3 — not done, the five are still sealed, and the paragraph above
>   mis-states what they hold.** Each of the five was opened and diffed against
>   `main` on 2026-08-26; **four of the five are now redundant** — the ledger
>   (D031, `spikes/transparency.py`, `run_checks.sh`), the gates
>   (`differential.sh` and the corpus gate), the seam (`py_bridge.rs`,
>   `python_rt.py`) and Event E3 (`event_channel_by_name*.rs`) all landed on
>   `main` by other routes the same day and arrived **measured**, where these
>   copies are not. That makes those four **cheaper and more dangerous than
>   written**: cheaper because the work exists, more dangerous because a
>   redundant branch that looks finished is exactly the one somebody merges.
>   **The fifth is the exception and it is the one worth naming.** It holds
>   `crates/orbweaver-giop/tests/locate_forward_and_reply_contexts.rs`, ~500
>   lines, plus ~200 lines of `server.rs` — **the two spec gaps, which are
>   §5's 2.3 and 2.4, and no part of it is on `main`**: no file of that name
>   exists there. So 0.3's real content is *one* branch of unverified work with
>   no landed equivalent, and it is unverified work against two items this
>   document schedules. Re-measure or abandon still stands, and it now resolves
>   differently per branch rather than as one verdict.
> - **0.4 — this batch.**
>
> *2026-08-26 정정 — 0.1과 0.2는 완료, 0.3은 미완이며 위 문장은 그 내용물에
> 대해 틀렸다: 다섯 중 **넷**(원장·게이트·심·E3)은 다른 경로로 `main`에
> 착지했으므로 검증되지 않은 **사본**이다 — 끝난 것처럼 보이는 중복 브랜치가
> 바로 누군가 병합하는 것이다. **다섯째가 예외이며 이름을 붙일 값이 있다**:
> §5의 2.3과 2.4인 두 규격 공백을 쥐고 있고 `main`에는 그중 아무것도 없다.
> 따라서 재측정이냐 폐기냐는 하나의 판정이 아니라 브랜치별로 갈린다.*

## 4. Stage 1 — the gates everything else needs / 나머지가 딛는 게이트

These are prerequisites, and **each pays off today with no language attached.**

| | Item | Home | First complete when |
|---|---|---|---|
| 1.1 | `corpus/services/` enters the differential and the corpus gate | D032 B1 | a divergence in that directory turns a gate red; `ir-subset.idl`'s JacORB rejection is **recorded** rather than structurally unrecordable |
| 1.2 | the sweep enrols by contract, not filename | D032 B2 | `coverage_tables.py --check` green **with the trader visible**; every service measured from a fixture's IDL says so in the output |
| 1.3 | the leak ledger | D031 H1–H2 | the harness prints, per transparency, measured / red / **named unmeasured** — and the empty case reads as *unmeasured*, never as passing |

**1.2's payoff is immediate:** `CosTrading::Lookup` is on the wire and invisible
to `SERVICES-COVERAGE` §8 — the one thing today's trading batch could not
finish, and it is a wiring problem now that the contract exists.

> **Corrected 2026-08-26 later the same day. All three of Stage 1 moved, and
> one of them exposes a defect in how its own finish line was written.**
>
> - **1.1 — first-complete.** D032 B1 landed; the divergence that was
>   structurally unrecordable is recorded. D032 §2's first finding carries the
>   verification.
> - **1.2 — the finish line as written is met, and the item is not.** The
>   trader is visible in `SERVICES-COVERAGE` §8 and `service_sweep.py` prints
>   a `#SOURCES` row naming each service's origin, which is both clauses of the
>   "first complete when" cell above. **But D032 B2, the proposal this row
>   points at, is half done** — the sweep still names its inputs literally, so
>   the *next* first-party contract is invisible for the reason the trader was.
>   **This is a finding about §2, not about the trader:** a finish line can be
>   satisfied by less than the proposal it was written for, and this one was,
>   because it named an observable symptom rather than the mechanism. Recorded
>   rather than repaired — rewriting the cell now would hide that it happened.
>   The green half is **not measured by me**: `coverage_tables.py --check`
>   needs a live sweep and this batch did not hold the harness lock, so §8's
>   content is read from the document and the check's verdict is unmeasured
>   here rather than reported green.
> - **1.3 — first-complete, and its first reading refutes the prediction that
>   commissioned it.** D031 line 74 expected the ledger to print *"four
>   transparencies partly measured and one not."* It prints **three declaring
>   and two with none**. The instrument's status, the recompute command and the
>   ledger's seven negative controls are in `docs/COMPONENTS.md`'s D029 §6
>   section and are not restated here. **The prediction being wrong in the
>   pessimistic direction is the result**, and it is the ledger working: a
>   reading assembled by hand was one transparency too generous about itself.
>
> *2026-08-26 정정 — 1.1 완료. 1.2는 **쓰인 결승선은 충족되었으나 항목은
> 아니다**: D032 B2는 절반이고, 결승선이 기전이 아니라 관찰 가능한 증상을
> 지명했기 때문이다. 이는 트레이더가 아니라 §2에 대한 발견이므로 고치지 않고
> 기록한다. `--check`의 판정은 이 배치가 재지 않았다. 1.3 완료 — 그 첫 판독은
> 넷/하나가 아니라 **셋/둘**이며, 예측이 낙관적이었다는 것이 곧 결과다.*

## 5. Stage 2 — the transparency closers / 투명성을 닫는 것

Ordered by D029 §6.1. **These are the only items that move the criterion**;
everything else in this document is what lets them be measured.

| | Item | Transparency | First complete when |
|---|---|---|---|
| 2.1 | leak tests, one per transparency | all five | each is a test that fails when the leak is reintroduced; where a transparency cannot yet be tested, the test exists as a **counted SKIPPED naming what it waits on** |
| 2.2 | the ORB can stop what it handed out | lifecycle stability | a peer mid-call observes a stated outcome when shutdown lands; **not `run()`** — if the design cannot separate stopping from an event loop, that is a finding that stops the batch |
| 2.3 | `LocateReply OBJECT_FORWARD` carries an IOR | location | a client asking `LocateRequest` for a moved object is forwarded, both byte orders |
| 2.4 | reply service contexts: ignore but preserve | (correctness, §9.7.2) | a context sent by a peer survives the round trip; **no attachment API** — that is `PLAN-DEFERRED` §21's question |
| 2.5 | the servant seam | language | omniORB's client calls a **non-Rust servant** behind our ORB, both byte orders |

**2.1 comes first** and the reason is stated in D031 §4 H4: a leak test with
nowhere to report lands as one more green group.

> **Corrected 2026-08-26 later the same day. 2.5 landed; 2.1 has its first
> instance; 2.3 and 2.4 are on an unmerged branch and this table listed none
> of that.**
>
> - **2.5 — landed, and it is the only item here that has moved the
>   criterion.** Where the Language transparency now stands is
>   [`D029`](D029-what-a-complete-orb-would-mean.md) §6.1's Language row and
>   §6.1.1, and is **not restated here**. Read against this row's own finish
>   line, the honest reading is *met in substance, short in one clause*: the
>   seam is measured against a generated Rust servant across GIOP versions and
>   **both byte orders**, and by omniORB's client — but the peer leg is
>   omniORB's native order only, which the batch named **unmeasured** rather
>   than counted. This row asked for a foreign client at both byte orders and
>   got one at one, for a reason that is a property of the peer. Left as
>   written, because a row edited to match what was achieved stops being a
>   finish line.
> - **2.1 — not complete, and it has its first real instance.** Event E3 is a
>   leak test for Location in this row's sense: it fails when the leak is
>   reintroduced, and it was **shown to** — three negative controls, each run,
>   each moving the counter, including one that removes the shutdown so the
>   pre-move reference still works. Its remainder is named rather than implied:
>   four leaks that stay open, recorded in D029 §6.1's Location material. The
>   row itself stays ❌ — `docs/COMPONENTS.md` says so and that is where the
>   instrument's status lives — because two transparencies still have no
>   declaring group at all.
> - **2.3 and 2.4 — this is where §3's 0.3 finding lands.** Both were built and
>   neither is on `main`: they sit on the one sealed branch with no landed
>   equivalent. **They are not unstarted and they are not done**, and that is a
>   third state this table has no column for — which is precisely what makes
>   0.3 worth triaging before Stage 2 is planned around them.
>
> *2026-08-26 정정 — 2.5 착지(무엇이 닫혔는지는 D029 §6.1과 §6.1.1). 다만 이
> 행이 요구한 "양쪽 바이트 순서의 외부 피어" 중 피어 다리는 omniORB의 네이티브
> 순서뿐이며 그것은 **미측정으로 명명**되었다. 행은 고치지 않는다 — 달성한 것에
> 맞춰 고친 행은 더 이상 결승선이 아니다. 2.1은 미완이나 E3라는 첫 실례가 있고
> 부정 대조군 셋이 실제로 붉어졌다. 2.3과 2.4는 만들어졌으나 `main`에 없다 —
> 이 표에 열이 없는 세 번째 상태다.*

## 6. Stage 3 — C and Java, named essential by the owner / 소유자가 필수로 지명

The direction is recorded; the preconditions are **measured and not met**.
Neither language starts before Stage 1 and 2.5, and the two are not in the same
position.

| | Item | First complete when |
|---|---|---|
| 3.1 | the acceptance suite is parameterised by language | today's `Python client target` group becomes its **first instance** and produces byte-identical results as one — the migration's oracle |
| 3.2 | `28-target-keywords.idl` covers C and Java | each language's reserved words are **executed** by an emitter, the way `yield` was found missing from the Rust list |
| 3.3 | **Java** target | driven by 3.1's suite against **JacORB as a runtime peer** — which already exists, in three harness groups, both directions |
| 3.4 | **C** peer | *this is C's first batch and it is not an emitter.* No C ORB exists here — omniORB is C++. The candidate that fits this project's licence position and its own precedent is a **hand-written C peer speaking GIOP**, as `ssliop_peer.py` was for TLS: the peer a binding needs is one that speaks the protocol, not another ORB |
| 3.5 | **C** target | driven by 3.1's suite against 3.4's peer |

**Java is unblocked by our own work only.** C is blocked by an absent peer, and
that asymmetry must not be smoothed over: starting C's emitter before 3.4 would
produce a target measured against itself, which D030 §3 refuses by name.

## 7. What is deliberately not in this programme / 일부러 넣지 않은 것

Named so their absence is a decision rather than an oversight:

- **The D006 rule-level conflict** — `PLAN-SERVICES` §1 rule 2 and D006 option E
  point opposite ways at `Expert::process`, and the census that would have
  inverted D006 is four rather than zero. **That is the owner's decision**, not
  a batch, and it constrains what a deployment may put in a `Tensor`.

  > **Updated 2026-08-26 — still not in this programme, and now cited rather
  > than described.** The re-run and what it settles live in
  > [`D006`](D006-plane-rule-tensor.md)'s *Amendment 2026-08-26* and in
  > `orbweaver_object::plane::TENSOR_BEARING`, which a test computes from the
  > contracts so the next operation to carry a `Tensor` joins the census on the
  > next `cargo test`. Two things belong here because they change what a future
  > batch may assume. The question that batch was given — *does
  > `Router::dispatch` become served?* — was answered **no, it stays refused**,
  > and **the answer was not the finding**: the finding was that `dispatch`'s
  > stated reason was false in two places in the source, under a green harness.
  > And the batch re-measured the neighbours of the shape it was handed, which
  > is the only reason the count is four rather than one — CLAUDE.md's *scope
  > it to the rule, not to the keyword*, holding for a third time. D006's
  > STATUS is untouched and remains the owner's.
  >
  > *2026-08-26 갱신 — 여전히 이 프로그램 밖이며, 이제 서술이 아니라 인용이다.
  > 주어진 질문의 답은 **아니오, 계속 거부**였고 **답은 발견이 아니었다** —
  > 발견은 그 거부의 명시된 사유가 초록 하네스 아래에서 거짓이었다는 것이다.
  > 넷이라는 수는 그 배치가 주어진 형태의 이웃까지 다시 쟀기 때문에 나왔다.*
- **A second ORB core in any language.** D030 §4, unchanged. §6 is bindings.
- **D027's E1–E3, D026's S1 migration, D028's M1/M3/M4.** All real, all
  cited in their homes, none of them closing a transparency or unblocking C and
  Java. They resume after Stage 2.
- **Anything measured by a demo.** §2's first clause.

## 8. What this document does not claim / 주장하지 않는 것

It does not claim the order is the only one — 1.3 and 2.1 could swap, and the
argument is in D031 §4 H4 rather than here. It does not claim the five WIP
branches are salvageable: §3's 0.3 says re-measure or abandon, and abandoning
five batches' partial work is a real possible outcome. It does not claim
Stage 3 is reachable this week; it claims the preconditions are now **listed and
measurable**, which they were not this morning. And it does not claim §2's
definition is easy to hold — the clause that will hurt is *a gate, not a demo*,
because a demo is what a finished batch feels like.
