# 2026-08-20 — the wave the plan review produced / 계획 검토가 만든 웨이브

> v0.5.0 → v0.6.0: 52 commits, 77 harness groups, 33 golden and 19 negative
> corpus files, nine `*_from_a_peer.rs` recordings. Twelve agent batches in
> three waves, each landed serially through the harness. Kept because the
> commits say *what* and this says what the day kept finding — and because two
> of the findings are about how this project works, not about CORBA.
>
> 열두 배치, 세 웨이브, 전부 하네스를 통해 직렬 착지. 커밋은 *무엇*을 말하고 이
> 기록은 그날 무엇이 반복해서 나왔는지를 말한다.

## 1. Where the work came from / 일은 어디서 나왔나

Not from a plan. From **reading the plan against the code**
(`2026-08-19-plan-review.md`): three reviewers over `PLAN.md` §7–§12,
`PLAN-SERVICES`, `PLAN-MOE`, `PLAN-DEFERRED` and D010 §5. They found progress
wrong in both directions — nineteen rows understated, six overstated, four
naming an instrument that did not exist — and the *restatement* of those rows
was one commit. Everything else in this release came from batches sent after
what the reading turned up, and five of those batches closed a defect nobody
had asked about.

That is the shape worth keeping: **the review was cheap and the work it
exposed was not optional.**

## 2. Five defects nobody had asked about / 아무도 요청하지 않은 결함 다섯

| Defect | Why it was invisible |
|---|---|
| `sidl-validate --against` **never ran the §5.3 comparison** over a guarded multi-file contract | It re-preprocessed each side's splice; a spliced header's `#ifndef` is conditional compilation, which we refuse on purpose. It exited 1 either way — an unmeasured check reported as a refusal |
| A `valuetype` and an abstract interface went on the wire as **object references** | `tk_abstract_interface`'s parameter list is byte-for-byte `tk_objref`'s, and nothing here had ever asked omniORB what it writes. Six phases |
| `--repair-prompt` gave a **model** the wrong file's line | The library mapped positions; the binary's JSON and prompt paths did not. Nothing in the tree read the field, so nothing could go red |
| A permanent forward moved **one handle, not its clones** | §9.6 keeps the old address valid, so a stale clone is *served* — one forward per call, forever, silently. Measured: 3 requests at the address the object left, now 1 |
| The front end diverged from the oracle in **both directions** on constants | `const fixed` legal and rejected; `const fixed<3,1>` illegal and accepted. The corpus had no case either way |

## 3. Twice: a keyword was handed over, a production was found / 두 번, 키워드를 주었더니 프로덕션이 나왔다

- Told "we accept a bare `fixed` in a signature where omniidl refuses it", the
  batch measured the neighbours and found the parser called `type_spec` where
  the grammar says `param_type_spec`: **ten divergences, eight closed by one
  function.** A `fixed`-only fix would have closed three.
- Told the same about constants, the next batch found `const_def` calling
  `type_spec` again: **seven shapes, one cause.** Keyword-only: two of seven.

Codified in `CLAUDE.md`: *a batch scoped to a keyword will fix a keyword;
scope it to the rule* — and the reason the count is known at all is that both
agents re-measured the neighbours of the shape they were given.

## 4. Two triggers with no instrument, one of them circular / 계측기 없는 방아쇠 둘

`ChannelStats::dropped` summed **five** different events (the design note that
found it said three; reading the code for the split found two more), so a clean
`stop()` moved the same counter as an overloaded consumer, and
`PLAN-DEFERRED` §1's un-defer trigger — *"a measured drop rate caused by
unwanted fan-out"* — could not be answered in either direction. Splitting it
also showed the trigger was **circular**: CosEvent has no subscription
predicate, so nothing in the servant knows what a consumer *wanted* — that
knowledge is what the deferred chapter's filters would add. Restated as two
observations, one from each side.

The same batch found a push-side `relay_check` refusal that was discarded
**without being counted at all**, while the pull path counted it twice.

## 5. Decisions written, none adopted / 쓰였고 채택되지 않은 결정

- **D011** — a control-plane event is not the D004 record (`session` is the
  join key to the audit ledger; `caller` attributes a principal to whoever
  dialled the port; the unresolved arm's `target`/`operation` come from the
  caller unvalidated), **and the channel has nobody to redact for**.
  Recommendation: publish nothing plus an in-process sink seam, with
  PLAN-DEFERRED §11's trigger.
- **D012** — the pool cannot hear a caller (`pool::Key` has no cap), so a
  capped caller would be handed an uncapped connection and its `wstring` would
  go out under the 1.2 codec, which the peer reads as the wrong string rather
  than faulting. Recommendation: build nothing, record the limit and the
  trigger; **option D was measured and rejected** — what remains after
  subtracting the profile is by construction the caller's own limit.

## 6. What this cost the coordinator / 코디네이터가 치른 비용

Two records commits carried **only part of what their message claimed**: a
`python3 - <<EOF` whose anchor had drifted exited non-zero, and the `git commit`
on the next line ran anyway. `records_keep_up.py` said CHANGELOG was eighteen
commits behind and I read it as a false alarm twice before checking
`git show --stat`. The gate was right both times.

Codified in `CLAUDE.md`: *a record written by a script that fails is not a
record* — check the writer's exit status before staging, and read the gate's
complaint as true until measured otherwise.

Also this wave: a batch whose worktree was created one merge behind `main`
worked against a base that lacked the batch it was told to build on; it
noticed and fast-forwarded. Worth checking at wave setup, because the failure
mode is a conflict at landing rather than an error in the batch.

## 7. Numbers / 숫자

|  | v0.5.0 | v0.6.0 |
|---|---:|---:|
| commits | — | 52 |
| tests | 1441 | ~1515 |
| harness groups | 74 | 77 |
| golden / negative corpus files | 31 / 13 | 33 / 19 |
| peer-recorded `*_from_a_peer.rs` | 7 | 9 |
| recorded oracle divergences | 4 | 5 |
| decisions | 10 | 12 (two proposed, none adopted) |

What the numbers do not say: five of the six D010 class-B rows are still
unmeasurable here (no identity provider, no SSL peer, no docker, no TAO, no
model key), A6 waits on a consumer, and every `PLAN-DEFERRED` chapter's trigger
was re-verified as **not fired**. The work that remains is mostly not ours to
start.

숫자가 말하지 않는 것: B류 여섯 중 다섯은 여기서 잴 수 없고(픽스처 부재), A6는
소비자를 기다리며, PLAN-DEFERRED의 모든 방아쇠는 미발화로 재확인되었다.
