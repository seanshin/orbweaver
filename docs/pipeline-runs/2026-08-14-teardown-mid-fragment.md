# A connection teardown that lands between two fragments (2026-08-14)

`crates/orbweaver-giop/tests/fragment_reception.rs` pinned what the reader does
with every shape §9.4.9 permits and no peer sends — a fragment for another
request, an interleaved message, a version change, an endless chain, a stray
leading `Fragment`, a chain cut short by a peer that vanished. It had no case
for the peer that does not vanish but **speaks**: a `CloseConnection` arriving
between fragments with the more-fragments bit still outstanding.

That case is where two rules of the specification disagree, which is why it went
unanswered rather than merely untested.

Footprint: `crates/orbweaver-giop/` (src + tests) and this file. No other crate,
no `spikes/run_checks.sh` — a suggested harness snippet is at the end for
landing from main, and it is optional.

## The collision, and why neither existing answer is true

- **§9.4.9** says nothing may interrupt a fragmented message on a connection.
  By that rule a `CloseConnection` mid-chain is an interleaved message, and the
  reader's answer was `UnexpectedMessage(CloseConnection)` — reduced by
  [`mux::Fault`] to `Desynchronized` before any caller saw it.
- **§13.5.1** makes `CloseConnection` a legitimate thing for a server to send at
  any moment: *"any outstanding messages … were not processed, and may be safely
  resent on a new connection."* `spikes/service_sweep.py` re-sends once on it,
  and `crate::pool` hides it from the caller entirely.

So the old answer told the caller *the peer is broken and the connection is
corrupt* about an orderly goodbye. A client believes that and gives up on a
service that was merely restarting; the pool refuses to retry a call §13.5.1
says nobody processed. That is an availability defect, and `mux.rs` had it
recorded as an open gap rather than fixed.

**But the obvious fix is also wrong.** Reporting `ConnectionClosed` hands the
caller §13.5.1's promise, and that promise is about outstanding messages
*without replies*. A peer that had already sent the leading piece of this
reply had, demonstrably, processed the request. Re-sending a non-idempotent
operation on that promise runs it twice — quietly, on a pooled connection the
caller never asked for. The specification does not underdetermine this; it
determines that **neither** existing variant is a true statement about a
half-received reply.

Hence a third answer, which is the honest one:

```rust
Error::InterruptedMidReassembly { control, partial, request_id, received }
```

`control` is `CloseConnection` or `MessageError`; `request_id` names the one
call §13.5.1's promise does not reach. Teardown is readable from the value —
`Error::is_orderly_close()` — so no caller has to match on the text of a
message to tell "dial again" from "stop".

## The three questions asked as one batch

| question | answer, and where it is pinned |
|---|---|
| `CloseConnection` between 1.2 fragments | `InterruptedMidReassembly { control: CloseConnection, .. }`. Teardown, not corruption; **not** re-sendable for the cut call, re-sendable for every other caller on the connection |
| The same at GIOP 1.1 | `FragmentUnsupported` wins, and the close is left **unread** in the stream. Structural, not accidental — see below |
| `MessageError` between fragments (§9.4.8) | `InterruptedMidReassembly { control: MessageError, .. }`. A report, not corruption — and not a goodbye either, so it makes *no* request re-sendable: §9.4.8 carries no body, so it names nothing |
| Does the invoker propagate it? | `Connection`, `Mux` and `Pool` all do, each with its own test. A variant nobody surfaces is not a fix |

**Why the 1.1 ordering is defensible rather than accidental.** At 1.1 the reader
refuses the moment it sees the more-fragments bit and reads nothing further, so
it never learns what came next; the close is still sitting in the stream. That
is the correct precedence and not merely the incumbent one:
`FragmentUnsupported` is *permanent* for this peer at this version — measured,
not theoretical, since omniORB 4.3.4 fragments a 1 MB reply at 1.1 — while a
close is retryable. Preferring the close would send `pool` round to a fresh
connection to be told the identical thing, which is the hot retry loop the
pool's "once, not until it works" rule exists to prevent. The test asserts the
cursor position, so a future reader that starts consuming past the refusal fails
it.

**Why the server stopped answering.** The same collision on the serving side:
before this change, a client that said goodbye between the fragments of its own
request got a §9.4.8 `MessageError` aimed at a peer that had stopped listening,
and left a protocol error in our own record. Where the message lands in the
stream cannot be what decides whether it is a fault — the loop already ended
quietly on a `CloseConnection` between whole messages. Fixed there, and the
same reasoning removed a second hazard found next to it: a top-level
`MessageError` was also answered with a `MessageError`, which between two ORBs
that both do it does not stop.

## Batch, oracle, repair, codify

**Batch: 12 cases, written in one pass with the source changes, no oracle
consulted mid-pass.** 5 in `fragment_reception.rs` (reader), 4 in
`mux_pool.rs` (Connection, Mux ×2, Pool), 1 unit test in `mux.rs` (the
per-caller fault), 2 unit tests in `server.rs` (serving side).

**Oracle, round 1** — `cargo fmt --check`, `cargo test -p orbweaver-giop`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets`,
`RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop`, run over the whole batch
at once. The batch did not compile, so **no case was measured in round 1**.

**Root causes, clustered by cause and not by case — 2:**

1. **A shared reduction made per-caller leaves a consumer that has no caller
   yet.** (1 site; blocked all 12 cases.) `Fault::to_error`/`unsent` grew a
   `waiter: u32` parameter because an interruption is the one fault that does
   not mean the same thing to everybody. Every consumer must then say *which*
   caller — and `Inner::send`'s early refusal is for a request that has not been
   allocated an id yet. Repaired at the cause: it passes `0`, which is never
   allocated as a request id, so an interruption describes itself to a brand-new
   caller as the close or the report it was rather than as somebody else's
   half-received reply. The reason is in the code, not only here.
2. **Style gates cannot be run in one's head, and must be re-run after every
   repair.** (5 sites across two rounds: 3 `rustfmt`, 1
   `clippy::redundant_guards` — a match guard comparing a field to a constant
   where a pattern says it better — and then 1 more `rustfmt`, because rewriting
   that guard into a pattern made the arm too long to fit on one line.) The
   recurrence is the useful half: a mechanical fix is an edit like any other and
   re-opens the gate that formats it.

**Round 2:** all 12 cases pass; clippy still had cause 2's single warning.
**Round 3:** clippy clean, and cause 2's own repair produced one new formatting
diff. **Round 4:** every gate green. **Round count: 4. First-pass rate on the
cases: 0/12 measured in round 1 (compile failure), 12/12 in round 2** — reported
this way on purpose, because a compile failure that blocks a batch is a signal
about the change and not about the cases.

**The number needs its caveat, in the same breath.** The implementation and its
tests were written by the same pass, so 12/12 measures internal consistency, not
independent correctness. The independent evidence available here is thin and
named as such below.

**Codified** — the point of the round:

- 12 tests, each stating the consequence of the opposite answer rather than the
  rule.
- `Error::InterruptedMidReassembly`, whose doc comment carries the whole
  argument above — including why `ConnectionClosed` would be a false promise.
- `Error::is_orderly_close()`, so the teardown/corruption decision is taken on a
  value. Documented as *not* implying re-send safety, which is the mistake the
  predicate invites.
- `Fault::to_error(waiter)` / `Fault::unsent(waiter)` — per caller, with the
  §13.5.1 reasoning in the code and a unit test that pins both answers for one
  fault.
- `mux.rs`'s open-gap paragraph replaced by a dated **closed** paragraph that
  says what replaced it (a gap silently deleted is a gap that comes back).
- `pool.rs` documents the one close it does not retry, and why the exception is
  not a special case but the reason the flag is per caller.

## What a real peer could and could not show

- **Measured, real peer.** omniORB 4.3.4 (`spikes/echo_server.py`), `spike-mux`
  at GIOP 1.2, 12 calls on one connection: `max_reply_fragments=2`,
  `sent=12 answered=12 out_of_order=6 orphaned=0`, `mux: PASS`. The peer really
  fragments and the reassembler — the function this change edits — still
  reassembles a real peer's fragments. That is a **regression** measurement of
  the touched path, not evidence about the new case.
- **UNMEASURED, and it will stay so.** No fixture produces a `CloseConnection`
  or a `MessageError` *between* two fragments. It needs the peer to decide to
  shut down inside the window between two writes of one reply, and neither
  omniORB nor JacORB exposes a control for that window; a kill produces EOF,
  which is the case `a_chain_cut_short_is_an_error_not_a_short_message` already
  covered. The oracle is therefore the specification plus a scripted TCP peer
  built from this crate's own encoders, exactly as `mux_pool.rs` already says of
  itself. Recorded as unmeasured in `mux.rs` too, so the file does not read as
  interop evidence.

## Found and deliberately not fixed

- **`docs/COMPONENTS.md` still advertises the gap.** Its `orbweaver-giop` row
  ends *"what remains here is a `CloseConnection` arriving between fragments,
  which surfaces as `UnexpectedMessage` rather than a retryable close"*. That
  sentence is now false. It is outside this footprint; the replacement is in the
  report to be applied from main.
- **The mux still reduces every other reader error to `Desynchronized`.** A
  `FragmentUnsupported` from a 1.1 peer reaches callers as "desynchronized" with
  the reason only on stderr — `mux.rs` says so itself. Same shape of loss as the
  one repaired here, but a different question, and widening the fault enum
  further was out of scope for this batch.
- **A top-level `MessageError` at `Connection::invoke_once` stays
  `UnexpectedMessage(MessageError)`.** The message type is already on the value,
  so the distinction is matchable, and no retry decision hangs on it (`unsent`
  is false either way). Adding a variant would have been churn.
- **Whether a *client* may legally send `CloseConnection` at 1.2 was not
  verified against the specification text.** Nothing here depends on it: the
  serving loop already treated a client's top-level `CloseConnection` as the end
  of the conversation before this change, and this only makes the mid-fragment
  case agree with it. Flagged rather than asserted.
- **Section numbering.** This record uses the repository's existing numbering —
  §9.4.7 `CloseConnection`, §9.4.8 `MessageError`, §9.4.9 fragmentation, §13.5.1
  connection management. The brief cited §15.4.3 for `MessageError`; §15.4.x is
  the CORBA 2.x numbering of the same chapter, where it is §15.4.8. Noted rather
  than silently followed.

## Gates, as run

| gate | result |
|---|---|
| `cargo test -p orbweaver-giop` | 219 unit + 16 `fragment_reception` + 18 `mux_pool` + 0 `ssliop_tls`, all pass |
| `cargo test --workspace` | **1151 pass**, 0 fail (1139 before; +12 is this batch) |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop` | pass |
| `cargo clippy -p orbweaver-giop --all-targets --features ssliop` | 0 warnings |
| `cargo doc -p orbweaver-giop --no-deps` | 5 warnings, **all pre-existing** (`event_server`, `ServerStats::peak_dispatching`); none from the links added here |
| `unsafe_code = "forbid"`, `#![deny(missing_docs)]` | unchanged |
| `./spikes/run_checks.sh` | **NOT RUN — unmeasured.** It takes the machine-wide lock and this worktree is one of many running concurrently; taking the lock would have blocked or corrupted somebody else's run. Not reported as a pass |

## Reproducing

```sh
cargo test -p orbweaver-giop --test fragment_reception -- --nocapture
cargo test -p orbweaver-giop --test mux_pool -- --nocapture
cargo test -p orbweaver-giop --lib -- server::tests::a_goodbye_between --nocapture
cargo test -p orbweaver-giop --lib -- mux::tests::an_interruption --nocapture

# The real-peer regression measurement, needs omniORB:
python3 spikes/echo_server.py &          # publishes spikes/echo.ior
cargo run -q --bin spike-mux -- spikes/echo.ior 12 1.2   # expect max_reply_fragments=2
```

## Optional harness snippet

Not required: every case here runs under `cargo test --workspace`, which the
harness's first group already gates. It is written out only for the case where
the fragment/teardown behaviour is wanted as a group with its own name.

```sh
# A teardown that lands between two fragments. §9.4.9 forbids the interruption
# and §13.5.1 permits the message, so the reader must report the collision
# precisely enough that a caller can tell "dial again" from "give up". Hand-built
# streams plus a scripted peer: no fixture can produce this shape (see
# docs/pipeline-runs/2026-08-14-teardown-mid-fragment.md). Skipping is a FAILURE.
hr "giop — fragment reception, and a teardown between fragments"
# Capture, then match: never pipe a producer into `grep -q` (CLAUDE.md).
frag=$(cd "$ROOT" && cargo test -q -p orbweaver-giop \
       --test fragment_reception --test mux_pool 2>&1)
case "$frag" in
    *"test result: FAILED"*|*"error["*)
        fail "fragment reception / teardown between fragments"
        printf '%s\n' "$frag" | tail -20 ;;
    *"test result: ok."*)
        pass "fragment reception + teardown: $(printf '%s' "$frag" \
            | grep -oE '[0-9]+ passed' | tr '\n' ' ')" ;;
    *)  fail "fragment reception did not report a result at all"
        printf '%s\n' "$frag" | tail -20 ;;
esac
```

## 요약 (Korean summary)

이 기록은 영어가 정본이다. 선례대로 **결론만** 옮긴다.

- **조각 사이에 도착한 `CloseConnection`은 새 변형으로 보고한다.**
  `Error::InterruptedMidReassembly { control, partial, request_id, received }`.
  §9.4.9는 "끼어들기 금지"라 하고 §13.5.1은 "서버는 언제든 보낼 수 있다"고 한다.
  기존 두 답 **모두 거짓**이라는 것이 이번 결론이다. `Desynchronized`/
  `UnexpectedMessage`는 정상 종료를 "연결 손상"이라 말하고, `ConnectionClosed`는
  §13.5.1의 "처리되지 않았다"는 약속을 주는데 **이미 응답이 시작된 요청에는
  그 약속이 성립하지 않는다** — 비멱등 연산이 두 번 실행된다.
- **호출자는 문자열이 아니라 값으로 구분한다.** `Error::is_orderly_close()`.
  단, 이것이 재전송 안전을 뜻하지는 않는다는 점을 문서에 명시했다.
- **재전송 가능 여부는 이제 호출자별이다.** `Fault::unsent(waiter)` — 응답이
  잘린 그 호출만 재전송 불가, 같은 연결의 나머지 호출은 §13.5.1대로 재전송 가능.
  `pool`은 이 플래그만 보므로 자동으로 옳게 동작한다.
- **GIOP 1.1에서는 `FragmentUnsupported`가 이긴다.** 우연이 아니라 구조다:
  리더가 그 지점에서 멈추고 뒤를 **읽지 않는다**(커서 위치로 검증). 1.1의
  거부는 영구적이고 close는 재시도 대상이므로, close를 택하면 풀이 새 연결에서
  같은 답을 다시 듣는 무의미한 재시도가 된다.
- **`MessageError`는 손상이 아니라 보고다.** 다만 §9.4.8은 본문이 없어 **어떤
  요청도 지목하지 않으므로** 아무 호출도 재전송 안전으로 만들지 않는다. 서버는
  `MessageError`에 `MessageError`로 답하지 않는다(두 ORB가 서로 그러면 멈추지
  않는다) — 조각 중간이든 메시지 사이든 동일하게 고쳤다.
- **서버 측도 조용히 끝낸다.** 클라이언트가 자기 요청 조각 사이에서 작별하면
  이제 protocol error가 아니라 정상 종료다.
- **배치 12건, 라운드 4회, 근본원인 2개.** 1라운드는 컴파일 실패로 **한 건도
  측정되지 않았다**(공유 축약을 호출자별로 바꾸자 아직 id가 없는 소비자가 남았다,
  1곳). 2라운드에서 12/12 통과, clippy 경고 1건. 3라운드에서 그 clippy 수정이
  다시 포맷 차이를 만들었다 — 기계적 수정도 편집이므로 게이트를 다시 열게 된다는
  것이 두 번째 근본원인의 요점이다. 4라운드 전부 통과. 구현과 테스트를 같은
  패스에서 썼으므로 12/12는 **내적 일관성**의 수치임을 밝힌다.
- **실제 피어로 새 형태는 만들 수 없다(UNMEASURED).** 응답 한 건을 쓰는 도중에
  피어가 종료를 결정해야 하는데, omniORB도 JacORB도 그 창을 제어할 수단을 주지
  않는다. 대신 omniORB 4.3.4가 실제로 조각낸 응답(`max_reply_fragments=2`)을
  변경 후에도 재조립함을 측정했다 — 이는 회귀 측정이지 새 사례의 증거가 아니다.
- **워크스페이스 1151건 통과**(이전 1139 + 이번 12), fmt·clippy 무경고.
  `run_checks.sh`는 **실행하지 않았다(UNMEASURED)** — 머신 전역 락을 다른 워크트리
  실행과 공유하기 때문이며, 통과로 보고하지 않는다.
