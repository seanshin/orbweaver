# D032 — Stopping what the ORB handed out, and why that is not `run()`

**STATUS: PROPOSED** — drafted 2026-08-26 answering the design question
[`D029`](D029-what-a-complete-orb-would-mean.md) §5 O1 asks *"first and in
writing"*: whether shutdown is graceful or immediate, and what a caller holding
a `Server` the ORB is stopping observes. Every figure below was measured that
day against the tree. Not self-approvable: §3 decides **what a peer mid-call
sees**, which is a wire-visible promise, and §4 refuses a mode rather than
deferring it.

**상태: 제안** — 2026-08-26, D029 §5 O1이 *"코드보다 먼저, 글로"* 답하라고 요구한
설계 질문에 대한 답. 종료가 우아한 것인지 즉각적인 것인지, 그리고 ORB가 멈추고
있는 `Server`를 들고 있는 호출자가 무엇을 관측하는지. 스스로 승인할 수 없다:
§3은 **통화 중인 피어가 무엇을 보는지**를 정하며 이는 와이어에 드러나는 약속이고,
§4는 한 가지 방식을 미루는 것이 아니라 거절한다.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. This document closes the fifth transparency's *implementation* gap —
> D029 §6.1's lifecycle row says *"removed at runtime has no implementation to
> be transparent about"*, and this is that implementation.
>
> **The operative bound is not restated here either.** It is the rustdoc on
> `Orb::shutdown` in `crates/orbweaver-giop/src/orb.rs`, because the bound is
> the API's contract and the person who needs it is holding the API. This
> document is the *argument*; that is the *promise*. One fact, one home.

---

## 1. The question, and why it had to be answered before code / 질문과 선행 이유

D019 step 4 made [`Orb::server`] and [`Orb::pool`] the only public way to obtain
transport. D029 §3.1: **that created this gap rather than revealing it.** Before
step 4 the caller held every `Server` it built and stopping was its own
business; after step 4 the ORB gives and cannot take back.

The mechanism to stop already existed and was never the question. Measured
2026-08-26 in `server.rs`: a stop predicate is polled by the accept loop
(`:1308`), at the top of every connection thread's loop (`:1397`) and inside
`await_message` while a thread waits for its peer (`:1607`), at `STOP_POLL`
granularity. What did not exist is **an owner for the decision**.

**Measured, and this is the number that makes it a product property rather than
a tidiness item: of 63 serve sites in this workspace, 17 pass `|| false`.**
Those seventeen processes — every `spike_*` server, `spikes/estate/servant.rs`,
`spikes/e2e/servant.rs`, the forge's inference fixtures — have no in-process
shutdown path at all. They are stopped by being killed. A harness that kills a
process group is not a transparency; it is the absence of one.

*멈추는 **기제**는 이미 있었고 그것이 질문이었던 적은 없다. 없었던 것은 **결정의
주인**이다. 63개 serve 지점 중 17개가 `|| false`를 넘긴다 — 그 열일곱은 프로세스가
죽어야 멈춘다.*

## 2. Stopping is not an event loop / 멈춤은 이벤트 루프가 아니다

D019 §5 refused `ORB::run`/`shutdown` semantics and **D019 is APPROVED with that
refusal intact.** D029 §4 says that if a design cannot separate stopping from an
event loop, *"that is a finding that stops the batch."* It separates, and here is
the separation stated so it can be checked rather than asserted:

| | `ORB::run` (refused, D019 §5) | This (proposed) |
|---|---|---|
| Who owns the serving thread | the ORB — a main thread parks in `run()` | **the caller**, exactly as today; `serve_shared` still runs on the caller's thread and still takes the caller's own stop predicate |
| Who decides when to stop | the ORB, via `shutdown` unblocking `run` | **either** — the ORB's flag and the caller's predicate are OR'd, and neither is privileged |
| What is added to the ORB | a scheduler, thread policies, a work queue | **one atomic flag per handout, and a list of them** |
| What the ORB joins | every serving thread | **nothing.** The ORB cannot join threads it did not spawn, and does not pretend to |

The last row is the one that keeps this honest. `Orb::shutdown` **raises flags
and returns**; it does not wait, because waiting would mean owning the threads,
and owning the threads is `run()`. What a caller gets instead is
`Orb::wait_until_stopped(deadline)` — a sleeping, deadline-bounded poll of
counters the servers already keep, which answers *"did they all go quiet in
time?"* with a boolean. **A bound you can state beats a guarantee you cannot**,
and a check that can come back `false` beats a promise that cannot.

*ORB는 자기가 띄우지 않은 스레드를 합류시킬 수 없고, 그런 척하지도 않는다.
`shutdown`은 깃발을 올리고 돌아온다. 기다림은 스레드를 소유한다는 뜻이고, 스레드를
소유하는 것이 곧 `run()`이다.*

## 3. Graceful, at request granularity / 우아한 종료 — 요청 단위

**The decision: graceful, and the unit of grace is one request, not one
connection.**

Three sentences, each of which the test in §5 refutes if it stops being true:

1. **A request already inside the servant runs to completion and its reply is
   written in full.** The flag is looked at *between* messages, never during
   one.
2. **No request is read from a socket after that thread has seen the flag.** A
   pipelined second request is left unread.
3. **Every live connection is ended with `CloseConnection` (§9.4.10), never with
   a bare TCP close.** §9.4.7 makes that goodbye mean *"your requests were not
   processed; re-send them elsewhere"* — which is true of exactly the requests
   this shutdown declines to read, and is the reason (2) must hold.

Sentence (3) is what makes (2) obligatory rather than merely tidy. If we read a
request after the flag and then dropped it, the `CloseConnection` that follows
would be a **lie about a request that had been processed**, and a peer that
re-sent it elsewhere would execute it twice. Grace here is not politeness; it is
what keeps a wire-level promise true.

*우아함의 단위는 연결이 아니라 **요청 하나**다. (3)이 (2)를 의무로 만든다: 깃발
이후에 읽은 요청을 버리면, 뒤따르는 `CloseConnection`은 **처리된 요청에 대한
거짓말**이 되고 그것을 다른 곳으로 재전송한 피어는 그 요청을 두 번 실행한다.*

### 3.1 The commit point / 커밋 지점

The event channel settled the same shape one layer down this week: a
`disconnect` that returned was not a `disconnect` that had stopped, and the fix
was a commit point taken under the state lock with no I/O between it and the
request going out.

Here the commit point is the atomic store itself. **There is no I/O between
raising the flag and the loops observing it** — no lock to acquire, no socket to
write, no `Guarded` section entered — which is the whole reason the bound in
`Orb::shutdown`'s rustdoc can be stated as a number of requests rather than as a
duration nobody can hold to.

## 4. Immediate is refused, not deferred / 즉시 종료는 미루는 것이 아니라 거절한다

An immediate shutdown means abandoning a thread that is inside a servant. Two
reasons, and the second is the one that matters:

- Rust has no safe thread cancellation, and `unsafe_code = "forbid"` stays.
  This is the cheap reason and it would be a bad one alone: it says what we
  cannot build, not what we should not.
- **It would put a half-written reply on the wire, and then say
  `CloseConnection` after it.** That breaks §3's sentence (3) for a request that
  was genuinely processed. There is no message in GIOP that means *"I started
  this and stopped"*, so an immediate stop has nothing honest to say, and the
  peer's only correct reading of what it received is the wrong one.

A caller that truly wants immediacy already has it and always did: drop the
`Server`. The listener closes and every peer sees a reset. That is the
process-death path, it is available without our help, and it is not something an
ORB should offer under a name that sounds orderly.

*즉시 종료가 정직하게 말할 수 있는 메시지가 GIOP에 없다. 반쯤 쓰인 응답 뒤에
`CloseConnection`을 붙이면, 실제로 처리된 요청에 대해 "처리되지 않았다"고 말하는
것이 된다. 즉시성을 원하는 호출자에게는 이미 길이 있다 — `Server`를 떨어뜨리면
된다. 그것을 질서 있게 들리는 이름으로 제공하지 않을 뿐이다.*

## 5. What a caller holding a `Server` observes / `Server`를 든 호출자가 보는 것

**`serve_shared` returns `Ok(())`, indistinguishable from its own predicate
having gone true.** That is deliberate and it is the second half of §2's "neither
is privileged": the two are the same event — *stop serving* — and inventing a
distinct return would make a caller write a branch over a difference that has no
consequence.

What the caller can ask, if it wants to know, is `Server::stop_requested()`,
which reports whether the ORB's flag is up. It is a question, not a return value,
because the answer matters to a supervisor deciding whether to rebind and to
nobody else.

### 5.1 Why the oracle had to be a peer, measured rather than argued / 왜 오라클이 피어여야 했는가

D029 §5 O1 says the measurement is *"what the client sees, not what our counters
say"*. That reads like caution. It is not: it was measured on 2026-08-26 and the
counters lied.

Running `spikes/orb_shutdown.sh` against a deliberately broken build — the
immediate shutdown §4 refuses, dropping the reply that was in flight — produced
these two lines from the same run, in both byte orders:

```text
peer     big: exit 1  {"seen": [{"kind": "reset"}], "verdict": "refuted", …}
fixture  big: exit 0  {"servers_stopped":1,"pools_closed":0,"already_gone":0,
                       "went_quiet":true,"serve_returned_ok":true}
```

**Every number this side keeps said the shutdown was clean.** One server
stopped, nothing left behind, every counter to zero, `serve` returned `Ok`. The
peer got a connection reset and not one octet of GIOP. A shutdown checked from
its own counters passes on the build this document exists to refuse — which is
the *green while measuring nothing* class with a lifecycle's clothes on, and it
is why the fixture's own exit code is reported beside the peer's and never
allowed to vouch for it.

The control also corrected the peer itself. Its first draft filed a reset under
*"could not measure"* and exited 3, so **the strongest refutation this fixture
can produce would have reported as an unmeasured check rather than a failure.**
A reset is an observation — it is precisely §3's third sentence being violated —
and it now exits 1. Found by running the control, not by reading the code.

*이것은 신중함이 아니다 — 재어 보니 카운터가 거짓말을 했다. 이쪽이 세는 모든 수가
"깨끗하게 멈췄다"고 말하는 동안 피어는 GIOP 한 옥텟도 받지 못하고 리셋을 받았다.
자기 카운터로 검사하는 종료는 이 문서가 거절하는 바로 그 빌드에서 초록이 된다.*

## 6. Pools, where "stopping" means something different / 풀에서의 의미

A pool has no accept loop and no threads. Stopping one means exactly two things:

- **No further connection is dialled.** After close, `acquire` and every
  `invoke` refuse rather than dial.
- **Every pooled connection is dropped**, outside the state lock, the way
  `Pool::clear` already drops evictions — because `guarded::assert_nothing_held`
  is called by `Connection::connect` and by every `invoke`, and a close that ran
  inside a `Guarded` section would be the exact violation that module exists to
  catch.

**What it does not mean: a call already in flight on a `Mux` the caller holds is
not aborted.** The caller owns that call; aborting it is §4's immediate mode one
layer down, and it has the same nothing-honest-to-say problem. It completes, or
it fails on the timeout it already had.

*이미 호출자가 들고 있는 `Mux` 위에서 진행 중인 호출은 중단하지 않는다 — 그것은
§4의 즉시 종료를 한 계층 아래에서 하는 것이고, 할 말이 없다는 같은 문제를 갖는다.*

## 7. Two refusals that keep `shutdown` from being advisory / 권고로 전락하지 않게

A stopped ORB **refuses to hand out new transport**: `Orb::server` and
`Orb::pool` answer an error naming the ORB as stopped. Without this, `shutdown`
would mean *"stop the ones I have already given"* and the next line of the
caller's program could undo it — which is not a lifecycle, it is a suggestion.

The flag is **raised once and never lowered.** A lowerable flag creates a
connection thread that has already written its `CloseConnection` and finds the
service running again, with no way to take the goodbye back. Restarting means a
new `Server`, because it means a new listener, which is what it already meant.

## 8. What this does to D029 §6.1's lifecycle row / 다섯째 투명성에 미치는 영향

D029 §6.1's home for that row is D029, and the row's new text belongs there, not
here. What this document owes it is the honest statement of **how far the row
moves**, which is less far than "removed at runtime is now testable":

- **What becomes testable:** *the removal itself.* A server can be removed at
  runtime by something other than killing its process, and what a peer observes
  across that removal is now a measurement (§5's test), not a reading.
- **What does not:** *the transparency of the removal.* The criterion says a
  caller must not be able to tell — and a caller of a removed server can tell
  immediately, because there is nowhere else for its request to go. Closing that
  needs a **second** endpoint and a redirect, which is `LOCATION_FORWARD` served
  for a *name* rather than for an object — the gap D029 §6.1's event-channel
  subsection already names as item 3 and which nothing here touches.

So the row moves from **"partly unmeasurable"** to **"measurable, and leaking
for a reason that is now named"**, and it does not move to *held*. Saying
otherwise would be the row this project keeps a subsection open to avoid.

*행은 "부분적으로 측정 불가"에서 "측정 가능하며, 이제 이름이 붙은 이유로 새고
있음"으로 옮겨간다. **"유지됨"으로는 옮겨가지 않는다.** 제거는 측정 가능해지지만
제거의 **투명성**은 아니다 — 그것은 두 번째 엔드포인트와 리디렉션을 필요로 하고,
그것은 객체가 아니라 **이름**에 대한 `LOCATION_FORWARD`이며 D029 §6.1이 이미
3번 항목으로 이름 붙여 둔 간극이다.*

## 9. What this document does not claim / 주장하지 않는 것

- **Not that the seventeen `|| false` sites are fixed.** They are now *fixable*
  — an `Orb` is in scope at every one of them — and this batch changes none of
  them, because changing a fixture's shutdown path and measuring the shutdown
  path in the same commit would leave neither measured. Named here so the next
  reader finds a count rather than rediscovering it.
- **Not that a pool's in-flight calls are bounded by this.** §6 says they are
  not; their bound is the timeout they already had.
- **Not that `Orb::wait_until_stopped` proves the serving thread returned.** It
  proves every counter went to zero — and §5.1 is the measurement of what those
  counters are worth on their own, which is the reason the method's own rustdoc
  says what it does not prove.
- **Not that `spikes/orb_shutdown.sh` is a gate.** The gate for this claim is
  `crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`, which runs in
  `cargo test --workspace`. The spike adds provenance — a peer applying none of
  our conventions — and **wiring it into `spikes/run_checks.sh` is left
  undone**, because that file was held by another batch on the day this landed.
  Named rather than silent: the next reader adds a group, and the negative
  control it lands with is already recorded in §5.1.
- **Not that the harness declares this transparency.** `spikes/transparency.py`
  reads per-transparency tags out of `run_checks.sh`, and the lifecycle row
  therefore still has no declaring group — the same held file, the same one-line
  fix.
