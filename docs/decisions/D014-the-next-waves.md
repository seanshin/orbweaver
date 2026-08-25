# D014 — The next waves: five one-crate batches, and what must land first

**STATUS: PROPOSED** — drafted 2026-08-25 from `docs/COMPONENTS.md`'s gap
columns, **each row grep-verified against the tree on the drafting day**
(§7.1 of D010: a gap column goes stale; one row here had — see W1-e).

**상태: 제안** — 2026-08-25에 `docs/COMPONENTS.md` 갭 열에서 기안. **모든 행을
기안 당일 트리에 대해 grep으로 재검증**했다(D010 §7.1: 갭 열은 부패한다;
여기서도 한 행이 이미 부패해 있었다 — W1-e).

---

## 1. What this document is / 이 문서는 무엇인가

Three sessions (2026-08-21 → 25) closed one family of defects — a sentence
with many homes, a classifier reading a fragment of one — and what remains in
the gap columns is **scattered one-crate work**: no two candidates share a
file, none needs a decision that is not already written, and none blocks
another. That is the wave shape. This document fixes the batch boundaries so
that agents can run them in parallel worktrees and the landing stays serial.

It decides **batch boundaries and their oracles only**. It does not restate
decision statuses (they live in `docs/decisions/D00N-*.md`), does not restate
D010's class B/C inventory — class C is current with **no trigger fired**
(all twelve `PLAN-DEFERRED` chapters re-measured 2026-08-25), and of class B
only **B5's first half has closed** (JacORB at GIOP 1.1, `74b0f15`; its second
half is §4 below), while B1–B4 and B6's fixtures are still absent — and quotes
a gap row only together with the verification that it is still true.

*(The parenthetical this paragraph first carried — "still current; its fixtures
are still absent" — **was** the restatement it claimed not to make, and it
copied D010 §4's stale B5 reading into this document's charter while §4 below
scheduled B5's second half, which only makes sense if the first is done. A
sentence that says "I do not restate X" and then restates X in the same breath
is the cheapest possible demonstration of why the rule exists.)*

세 세션이 한 결함 계열을 닫았고, 갭 열에 남은 것은 **흩어진 단일 크레이트
작업**이다: 파일을 공유하는 후보가 없고, 새 결정이 필요한 것도, 서로 막는
것도 없다. 이것이 웨이브의 모양이다. 이 문서는 **배치 경계와 오라클만**
결정한다. 결정 상태를 다시 적지 않고, D010의 B/C 목록을 다시 적지 않으며,
갭 행은 그것이 아직 참이라는 검증과 함께만 인용한다.

## 2. What lands first, serially / 먼저, 직렬로 착지할 것

The **qualified-subject batch is in flight in the main checkout** (2026-08-25):
`orbweaver_dynamic::{valuetype,abstract_interface,native}_subject` put the
repository id into every refusal subject, nine Rust sites and seven Python
sites now build the subject by calling one home, and the oracle pass over the
workspace is running as this is written. It touches `orbweaver-dynamic`,
`orbweaver-gen` (including `python_rt.py`) and `orbweaver-test` — three of the
five footprints below — so **no wave starts until it lands**, or every agent
merges into a moved target.

**한정 주어 배치가 메인 체크아웃에서 진행 중이다.** 다섯 footprint 중 셋을
건드리므로 **이 배치가 착지하기 전에는 웨이브를 시작하지 않는다** — 그렇지
않으면 모든 에이전트가 움직인 과녁에 병합하게 된다.

## 3. Wave 1 — five batches, five footprints / 웨이브 1 — 배치 다섯, footprint 다섯

> **Result, appended 2026-08-25 — the same day, because a plan that lands as a
> forecast is a document nobody re-reads.** W1-c (console constants) and W1-d
> (the line-0 sentinel) ran in parallel worktrees and are committed there
> awaiting serial landing; both found **one root cause each**, and W1-d's cause
> was one the brief had not named — the sentinel had *three* private readers,
> not one, so the fix was an owning `Finding::position()` rather than an
> `if`. W1-e's verification half ran read-only and returned **six** repairs
> where this document had predicted one, including a `COMPONENTS.md` row that
> was a **fifth cell in a four-column table** — dropped by every renderer, so a
> closed gap had been invisible to readers while the source looked complete.
> W1-a and W1-b did not start: their footprints collide with the
> qualified-subject batch of §2, which is the constraint working as written.
>
> *결과를 같은 날 덧붙인다 — 예보로 착지한 계획서는 아무도 다시 읽지 않는다.
> W1-c·W1-d는 병행 실행되어 착지 대기 중이고, W1-d의 원인은 이 문서가 이름하지
> 않은 것이었다(센티널을 읽는 사적 리더가 셋). W1-e는 하나를 예상한 자리에서
> 여섯을 찾았다.*

Every batch below: scope to the **rule**, not the instance handed to you
(CLAUDE.md; measured twice in one release); re-verify the quoted evidence
before writing code; one commit; recommend a harness snippet, **do not edit
`spikes/run_checks.sh`**; report what you could not measure from a worktree
(no JacORB fixture there — the landing machine measures it, 2026-08-24
lesson).

아래 모든 배치: 받은 사례가 아니라 **규칙**으로 범위를 잡고, 인용된 근거를
코드 작성 전에 재검증하고, 커밋은 하나, 하네스 스니펫은 **권고만**(직접 편집
금지), 워크트리에서 잴 수 없던 것은 보고로 남긴다.

### W1-a. A skipped *abstract* interface still owes its name — `orbweaver-gen`

- **Evidence, verified 2026-08-25:** `python.rs` registers a skipped
  interface's name so a reference to it carries the same TypeCode name bytes
  the Rust emitter writes — guarded by `&& !entry.abstract_interface`
  (`src/python.rs`, the skip arm of the emit loop). An abstract interface's
  descriptor `("abstract_interface", id)` reads its name from the same `NAMES`
  table, so a *member* referring to a skipped one crosses with `name: ""`
  where the Rust target writes the name: two targets, one contract, different
  bytes — the exact defect fixed for object references on 2026-08-20.
- **The rule:** anything a *reference or description can name* keeps its name
  registered even when its body is skipped. Re-measure the neighbours: every
  `Err(why)` arm of both emit loops, not only this guard.
- **Oracle:** a `python_target` case — struct member referring to a skipped
  abstract interface, crossed inside an `any`, bytes equal to the Rust
  emitter's. Negative control: remove the registration, watch it name the
  empty string.
- **Footprint:** `crates/orbweaver-gen` (`src/python.rs`, `tests/`).

### W1-b. `void`/`null`/`Principal` refuse as `<anonymous>` — `orbweaver-dynamic`

- **Evidence, verified 2026-08-25:** `anyjson.rs:1056` — `type_name` falls to
  `repository_id().unwrap_or("<anonymous>")`, and the three kinds with no id
  refuse as `<anonymous>`. The four wire families got names on 2026-08-21;
  these three are the neighbours that arm did not reach.
- **The rule:** a refusal names the construct. `void` where a value belongs,
  `null`, and withdrawn `Principal` each have a spelling; `<anonymous>` is
  reserved for a type that truly has no name, if any remains.
- **Oracle:** unit tests over the three kinds, both AnyJSON directions;
  `one_home_for_a_wire_refusal.rs` must stay green untouched (these are not
  the four families and must not borrow their heads).
- **Footprint:** `crates/orbweaver-dynamic`.

### W1-c. Constants reach no console surface — `orbweaver-console`

- **Evidence, verified 2026-08-25:** the catalog renders interfaces only; the
  registry has held exact constant values since 2026-08-24
  (`33-const-values.idl` carries 22 of the 67 shapes that batch measured
  through `omniidl -b dump`; golden corpus-wide, 39) and no page shows one.
  *(The "67 shapes in the file" spelling this section first carried was the
  agent's first finding — the count belonged to the measurement, not the
  file.)*
- **The rule:** the console renders what the registry holds, and decides
  nothing (its charter). A constant is part of the contract a reader reviews.
- **Oracle:** catalog snapshot over `corpus/golden/33-const-values.idl` — the
  values shown are the registry's exact decimals, not a re-parse; the
  "renders, decides nothing" property tests extend to the new surface.
- **Footprint:** `crates/orbweaver-console`.

### W1-d. `repair_prompt` renders the line-0 sentinel — `orbweaver-forge`

- **Evidence, verified 2026-08-25:** `forge/src/lib.rs` (in `repair_prompt`)
  writes `line {}, column {}` unconditionally, so a finding with no position
  renders `line 0, column 0` — a position that names the wrong place, in the
  one string an agent is told to act on. The console printer already refuses
  the same sentinel.
- **The rule:** a position is rendered only when one was measured. Re-measure
  the neighbours: every renderer of `f.line`/`f.column` in the crate.
- **Oracle:** unit test — a line-0 finding's prompt names the source and no
  position; a positioned finding still names its line.
- **Footprint:** `crates/orbweaver-forge`.

### W1-e. The gap row that had already closed — documents, class D

- **Evidence, verified 2026-08-25:** `docs/COMPONENTS.md`'s `orbweaver-gen`
  row still lists "`ifr_reaches_the_agent.rs`'s witness still gives
  `Sequence => []` unconditionally, a third empty recursive witness" — the
  test's own comment says *"It used to give `Sequence => []`
  unconditionally"*. Closed, still listed; the planning cost of exactly this
  was measured 2026-08-18 (a pass spent on finished work).
- **The rule:** run `python3 spikes/gap_symbols.py`, then verify **every** gap
  row's behavioural claim against the tree — this document verified the five
  it uses; the sweep is the batch. Stale rows are repaired in COMPONENTS with
  the closure's date and commit, not deleted.
- **Oracle:** none exists for prose (D010 §6) — the deliverable is the diff
  plus, per repaired row, the command run and what it printed.
- **Footprint:** `docs/` only. No crate edits.

## 4. Wave 2 — needs what wave 1 does not / 웨이브 2 — 웨이브 1에 없는 전제

Not started until wave 1 lands; each has a precondition named here.

- **The §5.3 differ's `any` dimension** (`orbweaver-registry`):
  `TypeCode::equivalent` compares union members in order, so an `any` carrying
  a reordered union may fail extraction on a peer while the differ says "no
  change". Precondition: a landing-machine measurement against omniORB first
  (does extraction actually fail?), because the verdict table must be argued
  from a peer's behaviour, not from our reading (the union-default precedent).
- ~~**`spikes/bench/stub.rs` has no `redirect`**~~ — **void, and it was void
  when this line was written** (found 2026-08-25): the stub has carried
  `redirect` on both the servant trait and the `Dispatch` impl since `bb3f973`,
  2026-08-19. This row was copied from a `COMPONENTS.md` gap column without
  checking it, in a document whose §1 promises that a gap row is quoted **only
  together with the verification that it is still true** — the promise held for
  the five rows §3 uses and broke on the first row §4 borrowed. What remains is
  the re-blessing discipline: the stub is generated code kept by hand outside
  `tests/emitted`, so nothing regenerates it when the emitter changes.
- **D012 per-caller version cap** (`orbweaver-giop`): PROPOSED; building it
  before approval is the class-C defect. Precondition: the approval phrase.
- **The mid-reply `CloseConnection` window** (`orbweaver-giop`) — **started
  2026-08-25**, ahead of wave 1's landing, because its footprint collides with
  nothing in flight and a 2026-08-25 sweep of every deferral and class-B row
  found it to be **the only fixture in the whole inventory that can be built
  here**: B1 needs an API key, B2 an identity provider, B3 an SSL peer, B4
  docker and a second host, B6 TAO — and a scripted peer needs a socket. B5's
  second half. CI-watched for the socket-ordering races macOS does not show.

웨이브 1 착지 전에는 시작하지 않는다 — 다만 착지 중인 것과 파일이 겹치지 않고
전제가 이미 충족된 항목은 예외이며, mid-reply 항목이 그 예외다(2026-08-25 시작:
전체 유예·class B 목록에서 **여기서 지을 수 있는 유일한 픽스처**). 각 항목의
전제는 위에 명기했다.

## 5. The protocol, fixed by reference / 프로토콜은 참조로 고정

Wave mechanics are not restated here: worktree isolation, local gates
(`fmt`/clippy/`-D warnings` tests), one commit in `git log -5` style, serial
landing through one harness run per merge, fixture copy at landing, cleanup —
the coordinator holds them. Two rules ride with every agent because they were
each violated once: **no oracle peeking mid-pass** (batch first, measure
once), and **a batch scoped to a keyword will fix a keyword — scope it to the
rule and re-measure the neighbours of the shape you were given.**

웨이브 역학은 여기 다시 적지 않는다 — 조정자가 가진다. 각 한 번씩 위반된 두
규칙만 모든 에이전트에 동봉한다: **일괄 생성 중 오라클을 보지 않는다**, 그리고
**키워드에 맞춘 배치는 키워드를 고친다 — 규칙으로 범위를 잡고 받은 모양의
이웃을 재측정한다.**

## 6. After wave 1 / 웨이브 1 다음

`CHANGELOG.md`'s Unreleased section carries seven headline entries including
wire-visible Python-runtime behaviour; when wave 1 lands, cut the release
before wave 2 so the notes are written while the measurements are fresh
(the v0.5.0 lesson: twelve wire changes are a release, not a backlog).

Unreleased에 와이어 가시 변경을 포함한 표제 항목 일곱이 쌓여 있다. 웨이브 1
착지 후, 웨이브 2 전에 릴리스를 자른다 — 측정이 신선할 때 기록을 쓴다.
