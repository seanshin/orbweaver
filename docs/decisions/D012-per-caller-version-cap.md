# D012 — A per-caller GIOP version cap on the pooled path

**STATUS: PROPOSED** — drafted 2026-08-20 from what commit `adf0867` (branch
`worktree-agent-a3e32d47076ae057a`, not on `main` when this was written)
reported and deliberately did not build. Not self-approved; §7 says what
approval would mean and §5 recommends **not building**, which is a thing that
still has to be decided rather than assumed.

**상태: 제안** — 2026-08-20 작성. `adf0867`이 보고하고 일부러 짓지 않은 것에서
출발한다. 스스로 승인하지 않는다. §5의 권고는 **짓지 않는 것**이며, 그것도
가정이 아니라 결정되어야 할 사항이다.

> **Line references.** Everything cited is on `main`. The citations marked
> *(branch)* were on `adf0867` when this was drafted against `main` at
> `efc728f`; **that branch has since merged** (verified 2026-08-25 — `adf0867`
> is an ancestor of `HEAD` and every cited line resolves on `main` unchanged),
> so the marker now records where a line came from rather than where a reader
> must go to find it.
> *인용한 줄 번호는 모두 `main` 기준이다. *(branch)* 표시는 기안 당시
> `adf0867`에 있었다는 뜻이며, 그 브랜치는 이미 병합되었다(2026-08-25 확인).*

---

## 1. What raised this / 무엇이 이 문서를 불렀나

`adf0867` made a caller's version cap survive the two things a caller cannot
see: a `LOCATION_FORWARD` and §9.6's restart. `Connection` gained a
`version_cap` field (`crates/orbweaver-giop/src/lib.rs:1724`, *branch*) and
`move_to` re-applies it after the redial, taking the lower of the cap and the
new profile's own §9.4.1 ceiling (`lib.rs:2344`, `lib.rs:2356`–`lib.rs:2360`, *branch*).

It then reported, and did not build:

> **`Reference` cannot declare a version cap.** […] The pooled path has no
> `cap_version`, and adding one is not free: the pool keys connections on the
> negotiated version, so a per-caller cap would have to be part of `pool::Key`
> or it would hand a capped caller a connection speaking above its cap.

That is the question this document decides. It is a decision rather than a
patch because the cheap fix — a `cap` field on `Reference` — is the one that
produces a silently wrong string rather than an error (§3).

*`adf0867`은 캡을 포워드와 §9.6 재시작 너머로 살려냈지만 **풀 경로에는 캡 자체가
없다**고 보고하고 짓지 않았다. 그것을 여기서 결정한다. 값싼 수정(`Reference`에
필드 하나)이 오류가 아니라 **조용히 틀린 문자열**을 만들기 때문에 패치가 아니라
결정이다.*

---

## 2. What a cap is, and who sets one / 캡이란 무엇이고 누가 거는가

### Two ceilings, and they cannot disagree / 두 개의 천장

A connection's version has two upper bounds. The **endpoint's** is §9.4.1's:
`Version::negotiate(advertised)` returns the profile's advertisement clamped to
`Version::max_supported()`, which is `V1_2`
(`lib.rs:144`–`lib.rs:146`, `lib.rs:120`–`lib.rs:122`). The **caller's** is
`Connection::cap_version` (`lib.rs:2006`; `lib.rs:2026` *branch*), which lowers
and never raises. The version spoken is the lower of the two, so a profile can
lower a cap further but can never contradict it — which is why `adf0867` was
able to carry the cap across a forward without a decision.

*천장은 둘이다. 엔드포인트의 천장은 §9.4.1(프로파일 광고를 `max_supported`로
자름), 호출자의 천장은 `cap_version`이며 내리기만 한다. 와이어에 나가는 것은 둘
중 낮은 쪽이라 둘이 모순될 수 없다.*

### What a cap changes on the wire / 캡이 와이어에서 바꾸는 것

Not a header field — the *body*. `WideCodec`'s own table
(`crates/orbweaver-giop/src/codeset.rs:1203`–`codeset.rs:1207`):

| | `wstring` length means | terminator | `wchar` |
|---|---|---|---|
| 1.0 | — | — | illegal (§9.3.1.6) |
| 1.1 | wide characters, **including** a terminating null | yes | fixed 2 octets |
| 1.2 | **octets**, and zero is legal | no | octet count then octets |

and the byte order of an unmarked wide value moves with the version too:
`unmarked_order` (`codeset.rs:1173`) returns big-endian for UTF-16 only at
`minor >= 2`, and the message's own order at 1.1. That rule is not inferred
from the specification — §9.3.1.6's bullets are written around the 1.2 form —
it is what JacORB 3.9 was measured to do at 1.1, 2026-08-19
(`crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs`,
`spikes/jacorb_giop11.sh`, `spikes/wide_rust.sh`, and for `wchar`
`spikes/jacorb_wchar11.sh`). omniORB 4.3.4 declines 1.1 wide text outright, so
there is one witness and not two.

The failure mode is the reason this is a decision: *"Reading a 1.2 `wstring`
with the 1.1 rule takes an octet count as a character count and then looks for
a terminator that is not there. Nothing about that fails loudly; it just
returns the wrong string."* (`codeset.rs:1209`–`codeset.rs:1211`).

**A cap below 1.2 also turns multiplexing off.** `Mux::over` splits the
transport only when `version.is_1_2_layout()` holds
(`crates/orbweaver-giop/src/mux.rs:669`, `lib.rs:125`–`lib.rs:127`), and the
pool's module docs already say why that belongs to the version rather than to
the socket: *"two versions on one connection would mean two different
concurrency rules on one socket"* (`pool.rs:26`–`pool.rs:28`). Since
`max_supported()` is 1.2, **every cap that changes anything today is 1.1 or
1.0, and therefore every capped connection is a non-multiplexing one.** §4
leans on this and §5 says why leaning on it is itself a reason to wait.

*캡이 바꾸는 것은 헤더가 아니라 본문이다. 1.1의 `wstring` 길이는 종결자를 포함한
**문자 수**, 1.2는 **옥텟 수**이고, 표시 없는 광역 값의 바이트 순서도 버전을
따른다(1.1은 메시지 순서 — 2026-08-19 JacORB 3.9 실측, 목격자는 하나뿐). 실패는
예외가 아니라 **틀린 문자열**이다. 그리고 1.2 미만 캡은 다중화를 끈다.*

### Who asks for one today: nothing outside tests / 오늘 캡을 거는 것

Eleven call sites, all of them tests or spike binaries:

| Where | Cap | Why |
|---|---|---|
| `crates/orbweaver-giop/src/bin/spike_interop.rs:356` | 1.1 | reach the 1.1 wide-text rule against a peer that would negotiate 1.2 |
| `crates/orbweaver-giop/src/bin/spike_cancel.rs:66`, `:124`; `crates/orbweaver-giop/src/bin/spike_mux.rs:253`, `:389`; `crates/orbweaver-giop/src/bin/spike_locate.rs:57` | swept | drive each version's path deterministically |
| `crates/orbweaver-giop/src/naming_server.rs:954` (a `#[test]`) | 1.0 / 1.2 | both reply header layouts |
| `crates/orbweaver-gen/tests/servant_faults.rs:171`, `crates/orbweaver-gen/tests/skeleton_wire.rs:158`, `crates/orbweaver-gen/tests/object_identity.rs:224` | swept | version matrix over generated code |
| `crates/orbweaver-gen/tests/forward_fallback.rs:281` | 1.2 | a **no-op** — `max_supported()` already guarantees it; it pins the test's intent, and changes no octet |

So a cap is, in this tree, **a measurement instrument**: the way a test reaches
a version-conditional path against a peer that would otherwise negotiate past
it. No production caller sets one, and `Pool::reference` has no production
caller at all (`crates/orbweaver-giop/tests/mux_pool.rs:746`,
`crates/orbweaver-gen/tests/forward_fallback.rs:349`,
`crates/orbweaver-giop/src/bin/spike_mux.rs` — that is the whole list). This
fact is what §5's recommendation turns on, and it is the fact most likely to
change.

*호출 지점 11곳 전부가 테스트 또는 스파이크 바이너리다. 캡은 여기서 **측정
도구**이지 배포 정책이 아니며, `Pool::reference` 자체도 운영 호출자가 없다. §5의
권고는 바로 이 사실 위에 선다.*

---

## 3. Why the pool makes it hard / 풀이 어려운 이유

### The deciding line / 결정하는 한 줄

Sharing is decided by `Key` and by nothing else:

```rust
fn pick(s: &State, key: &Key, limits: Limits) -> Option<Mux> {
    let muxes = s.live.get(key)?;          // pool.rs:691 — the deciding line
```

and `Key` has four fields — host, port, version, codeset (`pool.rs:237`–
`pool.rs:250`) — whose version comes from the profile alone:
`version: Version::negotiate(profile.version)` (`pool.rs:258`). `Key::of` takes
`(&IiopProfile, &str, u16)` (`pool.rs:254`) and has no way to hear a caller.

Downstream, the version is frozen: `Mux::over` moves the connection's `version`
into the mux (`mux.rs:649`–`mux.rs:687`), and every request encodes its body
with the codec that version implies — `stream_codec(&self.char_codeset,
self.version)` at `mux.rs:1061`, which builds
`WideCodec::new(version, UTF_16)` at `mux.rs:634`. One mux, one wstring rule,
for everybody on it.

*공유는 `Key`만으로 결정된다(`pool.rs:691`). `Key`의 버전은 프로파일에서만 오고
(`pool.rs:258`), `Mux::over` 이후에는 고정된다. 하나의 mux, 하나의 `wstring`
규칙, 그 위의 모든 호출자에게 동일.*

### Two callers, one endpoint / 호출자 둘, 엔드포인트 하나

**Today, nothing breaks — because nothing can be said.** `Reference`
(`pool.rs:739`–`pool.rs:749`) carries `pool`, `ior`, `endian`, `via`,
`forwarded`, and no cap; there is no API by which caller A could declare one.
That is not a safety property, it is the absence of the feature, and it is
worth stating precisely because "what breaks today" is the question and the
honest answer is *nothing yet*.

**What would break** is the naive addition. Give `Reference` a `cap: 1.1`, leave
`Key` alone, and:

1. caller B (uncapped) calls first; the pool dials and files the mux under
   `Key { version: 1.2, … }` (`pool.rs:443`, `pool.rs:447`);
2. caller A (capped to 1.1) calls; `Key::of` ignores the cap, so A computes the
   *same* key, and `pick` at `pool.rs:691` hands A B's 1.2 mux;
3. A's `wstring` goes out under `mux.rs:634`'s 1.2 codec — an octet count, no
   terminator — where A's contract with the peer was a character count and a
   terminator. Per `codeset.rs:1209`–`codeset.rs:1211` the peer does not fault;
   it reads the wrong string.

A second, quieter hazard sits in the same path: the cap must be applied
**between** `Connection::connect` (`pool.rs:420`) and `Mux::over`
(`pool.rs:443`). Capping after `Mux::over` leaves a mux that already split its
transport for 1.2 (`mux.rs:669`) while claiming 1.1 — two concurrency rules on
one socket, which is exactly what `pool.rs:26`–`pool.rs:28` says the key exists
to prevent.

*오늘은 아무것도 깨지지 않는다 — 캡을 **말할 방법이 없기 때문**이다.
`Reference`에 필드만 더하고 `Key`를 그대로 두면: 무캡 호출자가 먼저 1.2로 걸어
등록하고, 캡 호출자가 같은 키를 계산해 그 mux를 받고, 1.2 규칙으로 `wstring`을
쓴다. 피어는 오류를 내지 않고 **틀린 문자열**을 읽는다. 덧붙여 캡은 반드시
`connect`와 `Mux::over` **사이**에 적용해야 한다.*

### The precedent that does not apply / 적용되지 않는 선례

`Reference::set_endian` (`pool.rs:758`–`pool.rs:763`) *is* a per-caller property
that is not in the key, documented "advisory". It is safe only because
`Invoker::endian` (`lib.rs:1518`) reaches nothing on the wire: generated stubs
use it to build a validation probe that is discarded
(`crates/orbweaver-gen/src/lib.rs:1151`), while the message's own order is the
connection's. A version cap that reached nothing on the wire would not be a
cap. **The endian precedent is therefore an argument against reusing its shape,
not for it** — and worth writing down, because it is the shape a future batch
will reach for first.

*`set_endian`은 키에 없는 호출자별 속성이지만, 와이어에 닿지 않기 때문에
안전하다(스텁의 검증용 인코더는 버려진다). 와이어에 닿지 않는 캡은 캡이 아니다.
따라서 이 선례는 같은 모양을 쓰라는 근거가 아니라 쓰지 말라는 근거다.*

---

## 4. The options / 선택지

Connection costs below are **counted from the code and the defaults**
(`max_per_endpoint` 4, `pool.rs:181`), not measured under load — and
`DEFAULT_SOFT_IN_FLIGHT`'s own doc already says its number is a guess
(`pool.rs:186`–`pool.rs:191`). §8 records that as unmeasured.

### A. The cap enters `pool::Key` / 캡을 키에 넣는다

**What would have to be true.** `Key` gains a fifth field (a public struct with
public fields, `pool.rs:237` — this is a breaking API change); `Key::of` gains a
parameter (`pool.rs:254`); `acquire` / `acquire_connection` (`pool.rs:369`,
`pool.rs:374`) and `attempt` (`pool.rs:547`) thread the caller's cap down from
`Reference`; the cap is applied to the dialled `Connection` between
`pool.rs:420` and `pool.rs:443`.

**Connection cost.** One partition per distinct `(endpoint, cap)`, each holding
up to `max_per_endpoint` connections. The bound is smaller than it looks
*today*: every meaningful cap is ≤ 1.1 (§2), so a capped connection cannot
multiplex, and `pick`'s `filter(|m| m.multiplexes() || m.in_flight() == 0)`
(`pool.rs:697`) already refuses to pile a second caller onto a busy
non-multiplexing socket. Capped callers were going to need their own sockets
regardless; A gives them a pool to reuse between calls instead of none.

**Oracle.** Extend `the_key_separates_versions_and_codesets`
(`pool.rs:869`–`pool.rs:884`) with a cap arm — two keys equal in every profile
respect and different in cap must be unequal. Then a wire test in
`tests/mux_pool.rs`: two references to one endpoint, one capped to 1.1, both
calling; assert the capped request's GIOP header minor is 1, the uncapped one's
is 2, `stats().dialed == 2` and `stats().reused == 0` across the pair. Negative
control: remove the field from `Key` and the same test must go red with
`reused == 1` and both requests at 1.2.

### B. The pool refuses to share with a capped caller / 캡 호출자에게는 전용 연결

**What would have to be true.** `Key` is untouched. `acquire` learns the cap and,
when it is `Some`, skips `pick` entirely, dials, caps, and returns an
**unpooled** mux. That shape already exists: `pool.rs:438`–`pool.rs:441` returns
`Ok(Mux::over(conn))` without filing it whenever the connection cannot honestly
be keyed, and the module docs already establish that some connections are not
pooled for a correctness reason rather than a performance one — TLS,
`pool.rs:42`–`pool.rs:48`.

**Connection cost.** One connection per capped caller — strictly more than A
whenever more than one capped caller shares an endpoint, identical when exactly
one does. Worse, an unpooled connection is outside the idle sweep and the
`max_total` accounting that `sweep`/`evict_idle` do for pooled ones, so a
long-lived capped caller holds a socket nothing reaps.

**Oracle.** `stats().dialed` increments on every capped call and `stats().reused`
never does; the capped mux never appears in `live`; uncapped callers to the same
endpoint still reuse. Negative control: file the capped mux under the ordinary
key and the reuse assertion goes red.

### C. `Reference` carries no cap; the limit is documented / 한계를 짓지 말고 적는다

**What would have to be true.** No code changes. A doc line on `Reference` and
`Pool::reference` stating that the pooled path speaks whatever §9.4.1 negotiates
and that a caller needing less uses `Connection::cap_version` directly, giving up
pooling — plus this decision, and the trigger in §6.

**Connection cost.** Zero. The *capability* cost is that the pooled path cannot
serve a caller who must speak below the profile's advertisement. Measured
against the tree, that cost is **zero callers today** (§2), and the concurrency
half of it is nil in any case: a capped connection could not have multiplexed,
so what a capped caller gives up by leaving the pool is socket reuse between
calls, not parallelism.

**Oracle.** None for behaviour — there is no behaviour. The only assertion C can
carry is a guard against being half-built later, and §7.2 is honest about how
weak the available guard is.

### D. The cap is a property of the endpoint / 캡을 엔드포인트 속성으로

**Measured, and the answer is no.** The endpoint's contribution to the version
is `Version::negotiate(profile.version)`, and it is *already* in the key
(`pool.rs:258`). §9.4.1 gives a peer a ceiling and gives it neither a floor nor
a preference below that ceiling, so there is nothing further about the peer to
read. Subtract the profile's contribution and what remains is by construction
the caller's own limit — and every one of §2's eleven sites confirms it by
setting a constant the caller chose, none of them reading anything from the
peer. D is not a fourth option; it is a description of what `Key::of` does. It
is recorded so that nobody re-asks.

*D는 이미 구현되어 있다. 엔드포인트가 버전에 기여하는 것은
`Version::negotiate(profile.version)`이고 그것은 이미 키 안에 있다. §9.4.1은
피어에게 천장만 주고 바닥도 선호도 주지 않는다. 프로파일 몫을 빼고 남는 것은
정의상 **호출자 자신의 한계**이며, §2의 11곳 전부가 그렇게 쓴다. 선택지가 아니라
현재 동작의 서술이라, 다시 묻지 않도록 기록한다.*

### Summary / 요약

| | Key changes | Connections | What it buys | What it costs |
|---|---|---|---|---|
| **A** | fifth field, `Key::of` signature, public break | one partition per `(endpoint, cap)`, ≤ 4 each | correct sharing, capped callers still pooled | a public map key grows to serve zero callers |
| **B** | none | one per capped **caller**, unpooled | `Key` untouched, matches the TLS precedent | no idle sweep, no `max_total` accounting for those sockets |
| **C** | none | none | nothing built before its trigger | the pooled path cannot serve a below-1.2 relationship |
| **D** | — | — | — | not an option; already what `Key::of` does |

*A는 `Key`에 다섯 번째 필드를 더한다 — 공개 API 파손이고, `(엔드포인트, 캡)`마다
파티션이 하나씩 생기지만 오늘은 모든 유효 캡이 ≤1.1이라 어차피 다중화되지 않던
연결이므로 비용이 작다. B는 `Key`를 건드리지 않는 대신 캡 호출자마다 전용 연결을
열고, 그 소켓은 유휴 회수와 `max_total` 회계 **밖**에 놓인다. C는 아무것도 짓지
않고 한계를 적는다. D는 선택지가 아니다.*

---

## 5. Recommendation / 권고

**Adopt C — document the limit, build nothing — with the trigger in §6, and
record now that A is the shape if the trigger fires.**

Three arguments, in the order of their weight:

1. **Nothing in the tree needs it.** Eleven cap sites, none outside tests and
   spikes; `Pool::reference` has no production caller either (§2). This
   project's own rule is that building before the trigger is the defect, not
   the omission (`PLAN-DEFERRED` §0's trigger table; D010 §5, which states it
   as a rule and gives two measured instances of it holding).
2. **The one caller shape that would need it is deliberately not pooled.** A
   relationship that must stay below 1.2 is, in this tree, the JacORB-at-1.1
   measurement (§2), and it is driven through `Connection` **on purpose**:
   the point is to know which socket spoke 1.1, and a pool exists to make that
   unknowable.
3. **A's cost argument depends on a fact that a future version bump deletes.**
   A is cheap today only because `max_supported()` is 1.2, so every cap is
   ≤ 1.1 and every capped connection is non-multiplexing anyway (§2). The day
   `max_supported()` moves past 1.2, a 1.2 cap becomes meaningful, multiplexing
   survives it, and A's partitioning stops being free. Building on a bound that
   a one-line change silently invalidates is worse than not building.

**Prefer A over B if the trigger fires**, and this document says so now so the
future batch does not re-derive it: B's connection count grows per caller while
A's grows per distinct cap, and B's sockets fall outside the idle sweep and the
`max_total` bound — the pool's two safety properties — for exactly the
long-lived callers most likely to hold a cap.

*권고: **C** — 한계를 문서로 적고 짓지 않는다. §6의 방아쇠와 함께. 근거는 (1)
필요로 하는 호출자가 트리에 없고, (2) 필요할 법한 유일한 모양(JacORB 1.1 측정)은
일부러 풀을 쓰지 않으며, (3) A가 싸 보이는 이유가 `max_supported()`가 1.2라는
한 줄에 달려 있어 그 줄이 바뀌면 사라지기 때문이다. **방아쇠가 당겨지면 B가
아니라 A**이며, 그 판단을 지금 적어 둔다.*

---

## 6. The trigger / 방아쇠

Observable, in `PLAN-DEFERRED` §0's form — an event, not a feeling:

> **The first caller outside `crates/*/tests/` and `crates/*/src/bin/` that must
> speak below `Version::max_supported()` to a peer it reaches through `Pool`** —
> concretely, a reference resolved at run time whose profile advertises 1.2 but
> whose servant is measured to mis-read a 1.2 `wstring` or `wchar`.

A second, weaker trigger, which changes the *argument* rather than the need:

> **`Version::max_supported()` moves past 1.2** (`lib.rs:120`–`lib.rs:122`). At
> that moment a cap can be both meaningful and multiplexing, §4.A's cost
> reasoning stops holding, and this decision must be re-argued before A is
> built rather than after.

*방아쇠 둘: (1) 테스트·스파이크 밖에서 `Pool`을 통해 1.2 미만으로 말해야 하는
첫 호출자, (2) `max_supported()`가 1.2를 넘어가는 순간 — 이때는 필요가 아니라
§4.A의 비용 논거가 무너지므로 짓기 전에 다시 논해야 한다.*

---

## 7. What approval would mean / 승인의 의미

1. **It approves not building.** No cap API on `Pool` or `Reference`, and a
   future batch that wants one needs §6's trigger, not a new decision — except
   under §6's second trigger, which explicitly reopens this.
2. **It approves a documentation obligation, and an honest statement about how
   weakly it can be enforced.** The limit gets one home — `Reference`'s own doc
   comment — and no other document restates it, per CLAUDE.md's *where a fact
   lives*. A grep-shaped gate is available and cheap (`pool.rs` naming
   `cap_version` while `struct Key` does not is a failure), in the style of
   `spikes/decision_status.py`; it is **recommended, not required**, and its
   blind spot must be stated where it lives: it sees a name, not a capability,
   so a cap threaded under any other spelling passes it. No deterministic check
   in this tree can assert the real property, and saying so is better than
   installing a gate that reads as if it could.
3. **It records D as measured and rejected**, so the endpoint-property question
   is not re-opened by inspection of `Key`.
4. **It approves A as the shape, in advance, for the day the first trigger
   fires** — with the acceptance criteria already written in §4.A, including
   the negative control.
5. **It changes no measured document.** `COMPONENTS.md` records what is
   measured now and is `adf0867`'s business, not this decision's; this file
   does not restate it.

*승인은 (1) **짓지 않음**을 승인하고, (2) 문서화 의무와 그 강제가 얼마나 약한지에
대한 정직한 진술을 함께 승인하며(가능한 게이트는 이름을 볼 뿐 능력을 보지 못한다),
(3) D를 측정 후 기각으로 기록하고, (4) 첫 방아쇠가 당겨졌을 때의 모양으로 A를
미리 승인하며, (5) 측정 문서는 아무것도 바꾸지 않는다.*

---

## 8. What would falsify this, and what is unmeasured / 반증과 미측정

- **"Zero callers need it" is measured over this tree, not over the world.**
  It is a count of call sites (§2), and it is the claim most likely to go
  stale — the trigger exists because of that.
- **No peer has been probed for a version-conditional defect.** D009 §8 row 4
  probed eleven peer configurations for a codeset question and reported
  **BLOCKED** on the finding. Nothing analogous has been run for versions: no
  configuration here has been searched for a peer that advertises 1.2 and
  mishandles a 1.2 `wstring`. **Unmeasured, and it is the measurement that would
  fire §6's first trigger.** It was not run here because this batch's footprint
  is one document.
- **A's connection cost is reasoned, not measured.** §4's counts come from
  `Limits`' defaults and from `pick`'s rule, with no benchmark; `pool.rs:186`–
  `pool.rs:191` already says the number that governs spreading is a guess.
- **The falsifier for the whole recommendation is one line.** If `Reference`
  ever grows a cap while `Key` does not, §3's three-step failure is what
  appears — and it appears as a wrong string, not as an error
  (`codeset.rs:1209`–`codeset.rs:1211`), which is why C's value is not the code
  it avoids but the failure it names in advance.

*미측정: (1) "필요한 호출자 0"은 이 트리의 호출 지점 수일 뿐이고, (2) **버전
조건부 결함을 가진 피어를 찾는 탐침은 한 번도 돌지 않았다** — D009가 코드셋에
대해 한 것과 같은 작업이 버전에 대해서는 없다. 이것이 §6 첫 방아쇠를 당길
측정이다. (3) A의 연결 비용은 계산이지 측정이 아니다. (4) 전체 권고의 반증은 한
줄이다 — `Key`를 건드리지 않고 `Reference`에 캡이 생기는 순간.*
