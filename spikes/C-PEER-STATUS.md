# The C peer: what it measures, and what a `peer c` row would need

**Measured 2026-08-26.** `docs/decisions/D033-the-programme.md` §6 item 3.4 is
this file's subject; its STATUS lives there and is **not restated here**.

This is a status record, not a decision. It states what
[`c_peer.c`](c_peer.c) does today, what it deliberately does not do, and what a
`peer c` row in [`bindings/AXES`](bindings/AXES) would have to supply — because
adding that row belongs to whoever owns the acceptance suite, and a row added
without knowing what it commits to is worse than a row that is missing.

*이 문서는 결정이 아니라 상태 기록이다. D033 §6 3.4의 STATUS는 그 문서에 있고 여기서
다시 적지 않는다. 오늘 무엇이 측정되었고, 무엇이 일부러 빠졌으며, `peer c` 행이
무엇을 공급해야 하는지를 적는다.*

---

## 1. What exists / 오늘 있는 것

| | |
|---|---|
| [`c_peer.c`](c_peer.c) | a C program that speaks GIOP over a socket. **Not a C ORB**, and not a binding to anyone's. C99 + POSIX sockets; every GIOP and IOR octet built from the published OMG specification |
| [`build_c_peer.sh`](build_c_peer.sh) | probes for a compiler, builds with `-Werror -Wconversion`, and reads the link line back off the binary |
| [`c_peer.sh`](c_peer.sh) | the gate. Starts our servers, drives the peer, judges. `--negative-control` and `--cell` |

**Run 2026-08-26 on `cc` = Apple clang 21.0.0, arm64-apple-darwin25: 36 held,
0 refuted.** The binary links exactly one library, `/usr/lib/libSystem.B.dylib`,
which is the licence evidence read off the artifact rather than asserted in
prose. `cargo tree --workspace` exited 0 and matched none of omniORB, JacORB,
ACE or TAO.

*36건 통과·0건 반증. 바이너리가 링크하는 라이브러리는 `libSystem.B.dylib` 하나이며,
이는 산문의 주장이 아니라 산출물에서 읽은 증거다.*

### 1.1 The byte order, as read off the wire / 플래그 바이트에서 읽은 순서

`bindings/AXES` counts an order only when it is read out of GIOP §15.4.1's flag
byte of what the peer actually wrote. Both directions were read, never assumed:

| direction | what wrote it | what read it | orders observed |
|---|---|---|---|
| our server → the C peer | our `Connection` | `c_peer.c`, bit 0 of octet 6 of the Reply header | **little and big** — our server mirrors the caller's order, so driving `--request-endian` both ways produced flag byte 1 and flag byte 0 |
| the C peer → our server | `c_peer.c` | our server (and, in the control, a second `c_peer` in `--role server`) | **little and big**, both `observed` |

Across GIOP **1.0, 1.1 and 1.2**, giving `--cell` six `observed` lines.

*양쪽 방향 모두 읽었다. 우리 서버는 호출자의 순서를 되비추므로 `--request-endian`을
양쪽으로 몰면 플래그 바이트 1과 0이 모두 나온다. 1.0·1.1·1.2에서 각각.*

### 1.2 The refusals measured, and what our server said / 측정한 거부

Each in **both** byte orders. A peer that only shows the happy path proves less
than one that shows our server refusing something by name.

| provoked by | our server said |
|---|---|
| an operation we do not serve | `SYSTEM_EXCEPTION` / `IDL:omg.org/CORBA/BAD_OPERATION:1.0`, `COMPLETED_NO` |
| `add` declared with no arguments sent | `SYSTEM_EXCEPTION` / `IDL:omg.org/CORBA/MARSHAL:1.0`, `COMPLETED_NO` |
| an object key nobody activated (at `spike-names`) | `SYSTEM_EXCEPTION` / `IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0`, `COMPLETED_NO` |
| a magic that is not `GIOP` | `MessageError` (message type 6), connection closed |
| GIOP 1.9 | `MessageError` (message type 6), connection closed |

### 1.3 Two things the peer found by asking / 물어서 나온 것 둘

Neither is a defect. Both are recorded because no test in this tree was written
to make either observation.

1. **`Dispatch::knows` defaults to accepting every object key** — *"right for a
   single-servant process"*, says its own documentation — and `spike-server`
   does not override it, so it serves `ping` on a key nobody ever activated.
   `OBJECT_NOT_EXIST` is therefore unreachable against that fixture, which is
   why `c_peer.sh` starts `spike-names` as well: `naming_server.rs` overrides
   `knows` with a real comparison.
2. **Our server mirrors the caller's order on a `Reply` and does not on a
   `MessageError`.** A big-endian request with a bad magic came back with flag
   bit 0 **set**. §15.4.2 requires no mirror and the caller reads the byte
   either way, so this is an asymmetry worth recording rather than a defect.

*둘 다 결함이 아니다. 다만 이 트리의 어떤 테스트도 이 관측을 하도록 쓰이지 않았기에
기록한다.*

---

## 2. What a `peer c` row would need / `peer c` 행이 필요로 하는 것

`bindings/AXES` already reserves the slot and says the peer did not exist. It
does now. **The row is still not added here**, for one reason that is a real
question rather than caution:

### 2.1 The `property` field is the question, and it is not mine to answer

The axis's third column is the property a value carries. Today:

```
peer	omniorb	foreign	…
peer	jacorb	foreign	…
peer	self	ours	… can never satisfy clause 6
```

`foreign` is the property D032 §4 clause 6 turns on: *a foreign peer — **not
us** — is one end of it*. **The C peer is first-party: we wrote it.** So it is
not `foreign` under the letter of that clause. It is equally not `self`: it
shares no line of code, no constant and no table with `crates/`, so bytes
produced by the encoder under test cannot agree with it by construction — which
is the *substance* clause 6 is reaching for.

That gap needs a decision, not an assumption. The concrete proposal:

```
peer	c	independent	a hand-written C peer speaking GIOP (spikes/c_peer.c) —
			first-party, links no ORB, shares no code with crates/;
			satisfies clause 6's SUBSTANCE and not its letter
```

and a sentence in `AXES` saying whether `independent` counts toward clause 6.
**Writing that row myself would be answering the question by declaring it**,
which is exactly the move this project's honesty rules exist to stop.

*세 번째 열이 곧 질문이다. `foreign`은 "우리가 아닌"이라는 뜻이고 C 피어는 우리가
썼으므로 문자 그대로는 아니다. 그러나 `crates/`와 코드·상수·표를 하나도 공유하지
않으므로 `self`도 아니다 — 절 6이 노리는 **실질**은 충족한다. 이 간극은 가정이 아니라
결정이 필요하며, 내가 행을 써 넣는 것은 질문을 선언으로 답하는 것이다.*

### 2.2 The manifest rows, per direction / 방향별 매니페스트 행

`direction client` is *generated code in the binding's language calls a peer*;
`direction servant` is *a peer calls generated code in the binding's language*.
So a `peer c` column means different work in each row, and the two are not
equally close:

| cell | what it needs | today |
|---|---|---|
| `servant` × `c` — the C peer CALLS a generated servant behind our ORB | the peer's **client** role, pointed at the servant's IOR with the right operation names and argument shapes | **the peer half is done.** `c_peer.c --role client` already does exactly this against `spike-server` and `spike-names`. What is missing is only an adapter that starts the binding's servant fixture and passes its IOR |
| `client` × `c` — generated code DIALS the C peer | the peer's **server** role serving the contract that binding's client was generated from | **blocked on scope.** `--role server` answers one operation, `add`. `spike-interop` — our own Rust client — calls `ping`, `add`, `echo_string`, `scale`, `echo_ragged`, `echo_any` and `echo_wstring`; the last three need a TypeCode reader, struct padding and UTF-16, which is a marshalling library and is what this peer says it is not |

A `cell` command answers with its **exit status** (0 ok, 1 red, 2 fixture
absent) and prints `observed`/`claimed`/`note` on stdout. `c_peer.sh --cell`
already does all of that, so the adapter for the first row is thin.

*`servant × c`는 피어 쪽이 이미 되어 있고 어댑터만 남았다. `client × c`는 피어의 서버
역할이 연산 하나만 답하기 때문에 막혀 있으며, 그 셋을 더하는 것은 이 피어가 아니라고
선언한 마샬링 라이브러리를 만드는 일이다.*

---

## 3. Is a C emitter unblocked? / C 방출기는 열렸는가

**Partly, and the honest answer reframes the question.**

D033 §6 3.4's argument is that starting C's emitter first would produce a target
*measured against itself*. That argument is sound, and this peer removes the
version of it that says *there is nothing in this tree a C program can be
measured against*. But two things are now clearer than they were:

1. **For the client direction, a foreign peer already existed and it is not
   this one.** `spikes/echo_server.py` is an omniORB server, and generated
   Python dials it today as `bindings/python/client-omniorb.sh`. A generated C
   client could dial the same fixture, by the same route, and that satisfies
   clause 6 **in the letter** in a way `peer c` may not. The C emitter's client
   half is therefore not blocked on a peer at all.
2. **What this peer actually buys is the runtime, not the oracle.** A generated
   C stub needs a C runtime that does what `c_peer.c` does — build a GIOP
   header from the right alignment origin, encode CDR in a chosen order, parse
   a stringified IOR, read a reply's flag byte, decode a `SystemException`
   body. That layer had never been written in C in this tree, and it has now
   been written **and measured against our ORB before any emitter depends on
   it**. Rust's runtime is 1043 lines (D030 §1); this is the wire part of C's.

So: **the C emitter is unblocked on measurement and still blocked on a
runtime** — and this peer is the wire half of that runtime, measured. Whether
D033 3.5 should be driven by `peer c` or by `peer omniorb` is now a real choice
where before it looked like there was only one, and it belongs in D033 rather
than here.

*부분적으로 열렸고, 정직한 답은 질문을 다시 짜게 만든다. (1) 클라이언트 방향의 외부
피어는 이미 있었다 — omniORB 서버이며, 생성된 C 클라이언트도 같은 경로로 걸 수 있다.
(2) 이 피어가 실제로 사 준 것은 오라클이 아니라 **런타임**이다. 생성된 C 스텁이 필요로
하는 와이어 계층이 이 트리에 C로 쓰인 적이 없었고, 이제 방출기가 그것에 의존하기
전에 쓰이고 측정되었다. 즉 **측정에서는 열렸고 런타임에서는 아직 막혀 있다.***

---

## 4. What is left undone, named / 남은 것

Each of these is absent on purpose. None is a stub waiting to be filled.

- **No `peer c` row in `AXES` and no manifest row anywhere.** §2.1's `property`
  question first. `c_peer.sh --cell` is ready for whichever answer.
- **No harness group in `run_checks.sh`.** That file is held by another batch
  and was not edited. The recommended group: run `./spikes/c_peer.sh`, treat
  exit 2 as a counted `SKIPPED` naming `cc`, exit 1 as a counted failure — the
  same shape as `orb_shutdown.sh`, which is also not yet a group.
- **The server role serves one operation.** §2.2. The reverse leg — our Rust
  client dialing the C peer — is unreachable until either the peer grows a
  TypeCode reader (which it says it will not) or a smaller contract than
  `spike::Echo` exists for `spike-interop` to drive.
- **No fragmentation, no codeset negotiation, no wide text.** A `Fragment`
  arriving is reported, never reassembled. `jacorb_wchar11.sh` measures wide
  text against a peer that is genuinely not us.
- **No `LocateRequest`/`LocateReply`, no `CancelRequest`.** The peer names the
  message types and never sends them. `LOCATION_FORWARD` is decoded as far as
  the forwarded IOR's type id and not followed.
- **No `valuetype`, no `Any`, no object references as arguments.** v1's wire
  excludes the first; the other two need the marshalling library this is not.
- **Measured on one platform.** Apple clang 21, arm64, macOS 25.6. `-std=c99`
  and no compiler extension is used, but no other compiler or architecture has
  run it, and a big-endian *host* would be the interesting one — every order in
  §1.1 was chosen by the peer rather than forced by the machine.

*하나하나 일부러 빠졌다. 채워지기를 기다리는 미완성이 아니다.*
