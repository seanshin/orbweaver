# Phase 1 — wire protocol core

> Batches 1–9 · 2026-08-12 · **Closed.** Reproduce with `./spikes/run_checks.sh`

An MIT ORB core written against the published OMG specification, interoperating
in both directions with two independently implemented peers.

| | |
|---|---|
| Code | ~6,800 lines across two crates, one external dependency (`encoding_rs`, [`NOTICE`](../NOTICE)) |
| Tests | 115 unit tests, 14 harness check groups |
| Corpus | 21 golden, 10 negative, 20 requirement benchmark |
| Peers | omniORB 4.3.4, JacORB 3.9 — both directions, GIOP 1.0/1.1/1.2, both byte orders |

## What is verified, and by what

The distinction matters more than the checklist. Three different strengths of
evidence are in play, and collapsing them would misrepresent the state.

검증의 강도가 셋으로 다르며, 이를 뭉뚱그리면 상태를 잘못 전달하게 된다.

| Strength | Meaning | Covers |
|---|---|---|
| **Two peers** | Two independent implementations agree | GIOP 1.0/1.1/1.2 request and reply, both directions; CDR alignment; `any`/`TypeCode`; codeset negotiation; 1.2 `wstring`; our fragment emission; `LocateRequest` handling; system exceptions |
| **One peer** | One implementation agrees | 1.1 `wstring` (JacORB only — omniORB declines wchar at 1.1 by policy); CosNaming resolution (omniNames) |
| **Self only** | Our encoder agrees with our decoder | **Fragment reception** — no available peer emits GIOP fragments |

The self-only row is the one to carry forward. `orbweaver-cdr`'s alignment
model, by contrast, is not self-only: it is exercised by every byte both peers
have accepted.

## What Phase 1 learned that the plan did not predict

Nine batches produced five findings that changed the code or the plan, none of
which came from reading the specification alone:

1. **A detached buffer aligning from zero** — the same root cause three times
   (GIOP 1.0/1.1 request bodies, `TypeCode` encapsulations, `any` values). CDR
   counts from the start of the enclosing message, and any buffer built
   separately gets it wrong in a way that only shows up at some offsets. Now
   codified in the API: `Encoder::continuing_at`, and `encode_any_with` taking
   a closure so the correct thing is the easy thing.
2. **Peers do not infer wide-character order from the message byte order.** A
   big-endian `wstring` came back byte-swapped from *both* peers. Write a BOM.
3. **`giopMaxMsgSize` is a hard cap, not a split threshold**, and JacORB 3.9 has
   no GIOP fragmentation at all — which is why fragment reception is
   unverifiable here.
4. **A minor code is a vendor id plus a value.** Printing all 32 bits turned
   "minor 23" into "1330446359" and hid which condition a peer had reported.
5. **`corbaloc:` defaults to GIOP 1.0**, so the version negotiation from Batch 1
   is load-bearing for ordinary naming lookups rather than a legacy nicety.

## Still open, carried into later phases

Not defects — scope that Phase 1 did not claim. Listed so nothing is rediscovered.

- **Fragment reception has no independent validation** (above). Revisit when a
  third peer or a fragmenting configuration is available.
- **`LocateRequest`/`CancelRequest` send**, `CloseConnection` send. All are
  served; none are sent.
- **Request multiplexing and connection pooling.** §9.5.1.2 permits multiple
  pending requests per connection; we send one at a time.
- **Multi-profile failover.** Profiles are parsed and only the first is dialled.
- **`TAG_ALTERNATE_IIOP_ADDRESS`, SSLIOP port extraction.** Components are
  preserved but not interpreted, so an SSLIOP profile still reads as port 0.
- **`valuetype`, abstract interfaces, `fixed` on the wire.** Deferred by
  `PLAN` §4.4 behind a Phase 4 decision gate.
- **Bidirectional GIOP, transports other than TCP.** Deferred by `PLAN` §1.3.
- **TLS/SSLIOP.** Phase 6.

---

# Batch 1: hardening the wire core

> 2026-08-12 · Batch 1 of Phase 1 (wire protocol core)
> Reproduce with `cargo test --workspace && ./spikes/run_checks.sh`

The work set came from an adversarial audit of the Phase 0 spike against OMG
CORBA 3.4 Part 2. Phase 0 proved the code interoperates with **one** peer,
which is the most dangerous position to be in: it feels like evidence and is
not. The audit's job was to find what breaks against the next peer.

작업 집합은 Phase 0 스파이크를 OMG CORBA 3.4 Part 2에 대조한 적대적 감사에서
나왔다. Phase 0은 **한** 피어와의 상호운용을 증명했을 뿐이며, 그것은 증거처럼
느껴지지만 증거가 아니다.

**Result: 14 confirmed defects and 7 hostile-input findings, clustered into 4
root causes. All fixed in one pass. 41 unit tests, 12/12 interop assertions
still green against omniORB.**

---

## The correction that matters most

The audit's first casualty was our own reporting. Phase 0 claimed "14/14 cases
pass". The Korean round-trip case printed a result in all three of its branches
and never incremented the failure counter, so it **could not fail**. Twelve of
those fourteen lines were assertions; two were probes.

Worse than the miscount: the probe passed because omniORB's default is
byte-transparent. With no `CodeSets` service context the specified transmission
codeset is ISO-8859-1 (§7.10.2.5) while we send UTF-8, so a peer that actually
converts — JacORB and TAO both do — would have produced mojibake. The one case
covering this project's home market was the one case that could not fail.

`docs/PHASE0.md` is corrected. The harness now counts and labels assertions and
probes separately.

감사의 첫 희생자는 우리 자신의 보고였다. 실패를 집계하지 않는 케이스는 통과가
아니다. 게다가 그 프로브가 통과한 이유는 omniORB의 바이트 투명 기본값 때문이지
우리가 옳아서가 아니었다 — 국내 시장을 다루는 유일한 케이스가 실패할 수 없는
케이스였다.

---

## Root causes

The audit clustered 14 confirmed defects into four mechanisms. Fixing C1 alone
closed four of the top five findings — which is the batch loop working exactly
as intended (`docs/PLAN.md` §5.1).

| Cause | Mechanism | Findings |
|---|---|---|
| **C1** | The GIOP minor version was parsed and then discarded everywhere it appeared | 1, 2, 4, 14 |
| **C2** | Reply status handled as a two-way branch; the other four statuses fell through a catch-all | 3, 10 |
| **C3** | `align_to(8)` applied unconditionally, and the resulting offset never bounds-checked | 6 |
| **C4** | The connection modelled as "one request, one reply, nothing else ever happens" | 7, 8, 9, 13 |

### C1 — the version was thrown away

`read_message` accepted GIOP 1.0, 1.1 and 1.2 and then dropped the version, so
`decode_reply` parsed every reply with the 1.2 field order. But the layouts are
transposed:

```
ReplyHeader_1_0 / _1_1 :  service_context , request_id , reply_status
ReplyHeader_1_2        :  request_id , reply_status , service_context
```

A GIOP 1.1 peer replying with **one** service context — which TAO, JacORB and
omniORB all do in ordinary configurations — had its context *count* read as the
request id and its request id read as the status. Count 1, id 1 gives
`request_id = 1` (matching) and `reply_status = 1` = `USER_EXCEPTION`. **A
successful call was reported as a user exception with a garbage repository ID,
and nothing in the logs mentioned a version.**

Fixed: `Version` is a type, it travels with every message, and headers are
encoded and decoded per version. The client also negotiates down to what the
IIOP profile advertises (§9.4.1 forbids exceeding it) instead of always emitting
1.2 — which previously meant every `corbaloc:` URL without an explicit version,
where §7.6.10.3 defaults to 1.0, got a `MessageError`.

버전을 파싱한 뒤 버렸기 때문에 1.0/1.1 응답을 1.2 필드 순서로 읽었다. 성공한
호출이 사용자 예외로 보고되고, 로그 어디에도 버전 문제라는 단서가 없었다.

### C2 — four reply statuses fell through a catch-all

`LOCATION_FORWARD` was returned to the caller as though it were a normal reply,
so `body().get_f64()` decoded the leading bytes of a marshalled IOR as the
return value — sometimes an error, sometimes a plausible wrong number. This
fires on any POA using a `ServantLocator`, and TAO's ImplRepository forwards
*every* first call.

Fixed: forwards are followed transparently as §9.4.3.2 requires, bounded at 8
hops. `NEEDS_ADDRESSING_MODE` fails cleanly rather than being mistaken for a
result. User exceptions now keep their body, so a caller can read the
exception's members instead of only learning that something failed.

### C3 — unconditional alignment, unchecked offset

§9.4.2.1 and §9.4.3.1 both state that there is no padding after the header when
the body is empty. We padded anyway — visible in the Phase 0 hexdump as four
trailing bytes on `ping()`.

The receive side was worse. `Decoder::align_to` had no bounds check, so a short
reply could produce `body_at` past the end of the buffer; `Reply::body()` then
did `let _ = d.get_bytes(body_at)` and **discarded the error, leaving the
decoder at offset 0** — pointing at the ASCII `GIOP` magic. Any subsequent read
returned header bytes as payload, with no error anywhere.

Fixed: alignment is conditional on a non-empty body, `align_to` returns
`Result`, seeking is explicit via `seek_to`, and `Reply::body()` is fallible.

### C4 — the connection assumed the happy path

- **`CloseConnection` was a protocol error.** §9.4.7 says the pending request
  was *not processed* and may be safely re-sent. Idle-connection reaping is
  default behaviour — omniORB closes after 180 s — so a connection used once a
  minute meets this routinely and surfaced it as a hard failure.
- **The connect timeout was silently dropped for every hostname.**
  `addr.parse::<SocketAddr>()` only succeeds for numeric literals, so the
  `unwrap_or_else(|_| TcpStream::connect(&addr))` fallback ran for all DNS
  names — with no timeout. The same expression made IPv6 literals unreachable,
  since `format!("{}:{}", "::1", 9999)` yields `::1:9999`, which parses as
  nothing. This is exactly the scenario Phase 0 assumption D flagged, so the
  mitigation we recorded was not actually in force.
- **A read timeout left the connection reusable.** The partially-read message
  stayed in the socket, so the *next* call parsed the tail of the previous reply
  as a GIOP header.

Fixed: `dial` resolves through `ToSocketAddrs` and applies the timeout to every
resolved address; `CloseConnection` is a distinct retryable error; any framing
failure poisons the connection so it cannot be silently reused.

---

## Hostile input

`orbweaver-cdr` reads bytes that arrive from the network. It now treats every
length, count and flag as attacker-controlled.

| # | Finding | Consequence before the fix |
|---|---|---|
| 1 | `message_size` drove `Vec::resize` with no ceiling | **Twelve bytes bought a 4 GiB zeroed allocation.** Allocation failure aborts the process, and `forbid(unsafe_code)` does not help. A handful of connections was a complete remote DoS — the only finding that took the process down rather than corrupting a value |
| 2 | `Ior::from_encapsulation` used `unwrap_or_default()` on the type id | A truncated IOR parsed "successfully" into a profile whose host and port came from string content. The client then dialed an endpoint nobody intended, and it looked like a working reference |
| 3 | `align_to` moved past the end unchecked | Combined with the discarded seek error, silently rewound the decoder to offset 0 |
| 4 | Embedded NULs accepted in strings | `"shutdown\0harmless"` reads as `"shutdown"` to a C peer and as the full string to us — an audit and authorization bypass once `orbweaver-guard` gates operations by name |
| 5 | Element counts drove loops before validation | Bounded only by buffer size, which finding 1 made unbounded |
| 6 | Byte-order flag masked rather than validated | §9.3.3 makes it a `boolean`; `0x37` was reinterpreted instead of rejected |
| 7 | `next_id += 1` could panic in debug | Low severity, but the wrap policy has a spec dimension |

All fixed, each with a regression test that reproduces the original trigger.

---

## Verification

| Check | Result |
|---|---|
| `orbweaver-cdr` unit tests | 18/18 (was 10) |
| `orbweaver-giop` unit tests | 23/23 (was 7) |
| Interop against omniORB 4.3.4 | **12/12 asserted, both byte orders** |
| Golden corpus | 21/21 compile |
| Negative corpus | 9/9 rejected |
| IDL lint | 0 false positives, 3/3 pinned cases caught |

The interop result matters more than the count: the empty-body padding fix
**changed the bytes on the wire**, and the peer still accepts them. Removing
non-conformant padding did not break compatibility, which is what conformance
being the right target looks like.

빈 바디 패딩 수정으로 **와이어 바이트가 바뀌었는데도** 피어가 계속 수용한다.
비준수 패딩을 제거해도 호환성이 깨지지 않는다는 것이, 적합성이 옳은 목표라는
증거다.

### Two test-expectation bugs, not code bugs

Worth recording because both looked like defects:

1. **The 1.1 `reserved[3]` field is invisible on the wire.** `response_expected`
   always ends at offset 21 and the `object_key` sequence aligns to 4 regardless,
   so 1.0 pads 21→24 with zeros and 1.1 writes three explicit zeros into the same
   span. The encodings are byte-identical apart from the version octet. The
   obvious expectation — that 1.1 is three bytes longer — is wrong, and acting on
   it would mean "fixing" a correct encoder. Pinned as a test.
2. An arithmetic slip in an alignment assertion. Corrected.

---

---

# Batch 2: codeset negotiation

The highest-value gap from Batch 1, and the one the Phase 0 correction exposed.
Scoped after resolving `D001`: **only the EUC-KR conversion table is blocked**,
and the negotiation machinery around it is protocol work with no data
dependency.

## The decision that reversed itself

`D001` originally recommended writing our own EUC-KR table, "consistent with the
decision already taken for the ORB". Checking the actual licence files killed
that reasoning — using a rule the document itself had stated and then failed to
apply to itself: *a table derived from an incompatibly-licensed source is not
laundered by being retyped.*

| Upstream | Licence (read from the file, not a summary) |
|---|---|
| WHATWG `index-euc-kr.txt` — 17,048 entries, the normative euc-kr definition | CC BY 4.0, and *"to the extent portions of it are incorporated into source code, such portions… are licensed under the **BSD 3-Clause License** instead"* |
| Unicode data files | `Unicode-3.0` — OSI-approved, MIT-based, expressly covers data |

The ORB and the table are not the same kind of problem. **The ORB is logic**: GIOP
is a published specification, so implementing it owes nobody anything. **The
table is data we do not own**: there is no specification to implement it from,
only somebody's compilation of 17,048 mappings, and typing them in by hand
produces the same derived work more slowly and with transcription errors.

The sharpest consequence: the pure-MIT `encoding` crate is the *least*
trustworthy option, not the safest. A declared MIT licence that does not account
for its data's provenance does not remove the upstream obligation, it hides it.
An honestly-disclosed BSD-3-Clause is a better legal position than an
unexplained MIT.

ORB는 로직이고 테이블은 우리 것이 아닌 데이터다. 손으로 옮겨 적어도 같은
파생물이 된다. 출처를 설명하지 않는 MIT 선언은 상류 의무를 없애지 않고 가릴 뿐이다.

**Awaiting owner sign-off**, because it amends the stated policy. Recommended
wording: *MIT for everything we write; where a component is data we cannot
originate, permissive-with-attribution is accepted, disclosed in `NOTICE`, and
recorded as a decision.*

## Codeset IDs, captured rather than recalled

The registry values are magic numbers, so they were taken off the wire instead
of from memory. A capture socket answered omniORB's `LocateRequest` with
`OBJECT_HERE`, which made it send the real `Request`:

```
ServiceId=1  010000000100010009010100
             ^^        ^^^^^^^^ ^^^^^^^^
             flag      char TCS  wchar TCS
  char  TCS = 0x00010001  ISO-8859-1
  wchar TCS = 0x00010109  UTF-16
```

**This is the Phase 0 correction proven on the wire.** omniORB declares
ISO-8859-1 for `char` while we send UTF-8 bytes with no context at all. §7.10.2.5
says that absent a context the transmission codeset *is* ISO-8859-1, so our
Korean text round-tripped only because omniORB passes bytes through when no
conversion is called for. Nothing agreed; nothing converted.

**Phase 0 정정이 와이어에서 증명됐다.** omniORB는 char에 ISO-8859-1을 선언하고,
우리는 컨텍스트 없이 UTF-8 바이트를 보낸다. 한국어가 왕복된 것은 변환이 일어나지
않았기 때문이지 합의가 있어서가 아니다.

Incidental: omniORB sends a `LocateRequest` and **waits for the `LocateReply`**
before sending its first `Request`. Serving one is not optional for the serving
half.

## Delivered

| Piece | Spec |
|---|---|
| `TAG_CODE_SETS` component parsing | §7.6.6.5 |
| `CodeSets` service context encode/decode | §7.10.2.5 |
| Negotiation algorithm with its five ordered cases | §7.10.2.6 |
| UTF-8, ISO-8859-1, US-ASCII, UTF-16 conversion | §9.3.2.7 |

Two design choices worth stating:

- **A common conversion set is resolved by our preference order, not by registry
  number.** Both are deterministic, but list order carries intent — ISO-8859-1
  is listed before ASCII because it is the superset — while the lowest OSF
  number is an accident of assignment. (The first implementation used numeric
  minimum; the test that caught it now pins the better rule.)
- **`Unsupported` is a distinct error from `Incompatible`.** When a peer asks
  for EUC-KR the two sides *agree* and the gap is ours. Collapsing them into one
  error would send someone hunting for a peer misconfiguration that does not
  exist.

Latin-1 conversion **refuses** Korean rather than substituting. A silent
substitution is how mojibake reaches a database and stays there.

## EUC-KR — approved and landed

`D001` was approved, the policy amended, and `encoding_rs` adopted behind the
default-on `euc-kr` feature with attribution in [`NOTICE`](../NOTICE).

Verified against an **independent** implementation rather than a self-round-trip:
`"함정 전투체계"` must encode to `c7d4 c1a4 20 c0fc c5f5 c3bc b0e8`, which is
what Python's EUC-KR codec produces. A self-round-trip would pass even if the
table were wrong in a self-consistent way. 13 bytes against 19 in UTF-8 — the
whole reason peers use it.

자기 왕복이 아니라 **독립 구현**과 대조해 검증했다. 자기 왕복은 테이블이 자기모순
없이 틀려도 통과한다.

Two behaviours worth pinning, both of which the library gets wrong for our
purposes by default:

- **`encoding_rs` substitutes HTML numeric character references** for
  characters it cannot map. Correct for a browser; catastrophic here, because
  the peer would receive the literal text `&#26085;`. We check the flag and
  raise `Untranslatable`, naming the offending text in the message.
- **Malformed input decodes to U+FFFD.** Accepting that hands the caller a
  string that looks fine and is not what the peer sent. We raise `Malformed`.

A third case emerged while testing the attribution-free build. With `euc-kr`
off, a Korean-only peer produced `Incompatible` — technically true, and
misleading: it sends an operator hunting a peer misconfiguration that does not
exist. Negotiation now checks whether the peer asked for something this crate
*implements but this build excluded*, and reports `Unsupported` so the
diagnostic points at the build flags.

`--no-default-features` drops the dependency and the obligation, and the
harness tests that promise rather than repeating it:

```
licence boundary
  ok   no ORB fixture appears in cargo tree
  ok   --no-default-features drops encoding_rs, as NOTICE states
  ok   the attribution-free build still passes its tests
```

---

# Batch 3: the serving half

Until now the asymmetry was that we could call existing CORBA systems and they
could not call us. `docs/PLAN.md` §7 commits to GIOP 1.0/1.1 compatibility **in
both directions**, so half the commitment was outstanding.

**Result: a stock omniORB client invokes our Rust server successfully at GIOP
1.0, 1.1 and 1.2 — 5/5 assertions each, with the server confirming it really
received three distinct versions.**

지금까지의 비대칭은, 우리가 기존 CORBA 시스템을 호출할 수는 있어도 그쪽이 우리를
호출할 수는 없다는 것이었다. **순정 omniORB 클라이언트가 GIOP 1.0·1.1·1.2로 우리
Rust 서버를 호출한다.**

## What had to exist first

- `decode_request` for all three versions, `encode_reply`, and system-exception
  replies.
- **`LocateRequest` / `LocateReply`.** Captured in Batch 2: omniORB sends a
  `LocateRequest` and *waits for the reply* before its first `Request`. A server
  that treats it as unexpected never receives an invocation at all — it hangs at
  the handshake rather than failing somewhere informative.
- **IOR emission.** A peer cannot call us until we can publish a reference.
- `_is_a` and `_non_existent`, which every ORB probes with.
- `MessageError`, so a malformed message gets an answer instead of silence
  (§9.4.8).

`LocateReply` carries its own trap: §9.4.6 marshals the body **immediately**
after the header with no alignment — the opposite of `Reply` in 1.2. Applying
the `Reply` rule shifts every byte of an `OBJECT_FORWARD` body.

## The bug the round-trip test caught

Writing the server surfaced a defect **I had introduced in Batch 1** while
making body padding conditional. The body was built in its own `Encoder`, whose
alignment origin was the start of *that buffer* rather than the start of the
message. CDR counts from the message, so:

- Under GIOP **1.2** the body starts 8-aligned, buffer-relative and
  message-relative alignment coincide, and everything passed.
- Under **1.0/1.1** the body starts wherever the header ended. Every `double`
  in the body landed on the wrong boundary.

It was invisible to every test written so far because they all used 1.2. The
fix is `Encoder::continuing_at`, which lets a detached buffer align as though
the bytes preceding it were already written.

Batch 1에서 **내가 넣은** 결함이다. 바디를 별도 버퍼에 만들면서 정렬 원점이 메시지
시작이 아니라 버퍼 시작이 됐다. 1.2에서는 두 원점이 우연히 일치해 보이지 않았고,
1.0/1.1에서만 드러난다.

## Guarding against a false pass

Three version runs passing means nothing if the peer used one version three
times. omniORB's `-ORBmaxGIOPVersion` could have been ignored, and this project
has already produced two passes that could not fail. The server therefore logs
each distinct version it receives, and the harness fails unless it sees exactly
three:

```
reverse interop — omniORB client against our server
  ok   omniORB client at GIOP 1.0 -> our server, 5/5
  ok   omniORB client at GIOP 1.1 -> our server, 5/5
  ok   omniORB client at GIOP 1.2 -> our server, 5/5
  ok   server confirms three distinct GIOP versions were received
```

세 버전 실행이 통과해도 피어가 한 버전을 세 번 썼다면 아무 의미가 없다. 서버가
수신 버전을 기록하고, 하네스는 정확히 세 개를 보지 못하면 실패한다.

`echo_string` on the server side passes bytes through rather than decoding to a
Rust string: without a negotiated codeset there is no basis for claiming what
they mean, and echoing verbatim is the one answer correct under any codeset.

The harness is now `spikes/run_checks.sh`; it outgrew the name `run_phase0.sh`.

---

# Batch 4: negotiation wired to the call path

The machinery from Batch 2 existed but nothing used it. `Connection` now parses
`TAG_CODE_SETS` from the peer's profile, negotiates, sends the `CodeSets`
context on the first request of a connection, and exposes the agreed converter.

## The spec left a gap, and the gap was not neutral

§7.10.2.6 accepts a match between one side's native codeset and the other's
conversion list, but does **not** say which direction wins when both hold.
Against omniORB — native ISO-8859-1, conversion list including UTF-8 — both
readings are legal and they disagree about whether Korean survives:

| Choice | Legal? | Korean |
|---|---|---|
| Peer's native, ISO-8859-1 | yes | **destroyed** |
| Ours, UTF-8 | yes | carried |

The first implementation took the peer's native, on the reasoning that it keeps
conversion cost on our side. That reasoning optimised the wrong thing. Among
mutually acceptable candidates we now choose the **widest repertoire**, because
picking a narrower codeset is a data-loss decision taken at connection setup,
before anyone knows what text will actually flow.

명세는 두 방향을 모두 허용하면서 우선순위를 정하지 않는다. 그 재량은 중립적이지
않다 — omniORB 상대로 한쪽은 한국어를 살리고 다른 쪽은 파괴한다. 상호 수용
가능한 후보 중 **가장 넓은 레퍼토리**를 고른다. 더 좁은 코드셋을 고르는 것은,
어떤 텍스트가 흐를지 아무도 모르는 연결 설정 시점에 내리는 데이터 손실 결정이다.

## Proving it is negotiation and not byte-transparency again

Korean round-tripping through omniORB proves nothing on its own: if the peer
passes bytes through, the test passes whether or not either side agreed
anything. That is exactly how the Phase 0 result fooled us.

The evidence is peer-side. With `-ORBtraceLevel 40` the server logs:

```
Receive codeset service context and set TCS to (UTF-8,UTF-16)
```

and the received bytes carry our context — `0501 0001` (UTF-8) and
`0001 0109` (UTF-16). **The peer changed its behaviour because of what we
declared.** That is negotiation; the Phase 0 pass was not.

한국어 왕복만으로는 아무것도 증명하지 못한다 — 피어가 바이트를 통과시키면 아무도
합의하지 않아도 통과한다. Phase 0가 우리를 속인 방식이 정확히 그것이다. 증거는
피어 쪽 로그다: **피어가 우리 선언 때문에 동작을 바꿨다.**

The interop probe is therefore now a real assertion, and the run reports
**14/14 asserted** rather than 12/12 plus two probes that could not fail. If
negotiation picked a codeset that cannot carry the text, `encode` fails and the
case fails with it.

Where the peer publishes no `TAG_CODE_SETS`, §7.10.2.5 specifies ISO-8859-1 and
we send no context — claiming otherwise would be asserting an agreement that
never happened.

---

# Batch 5: a second peer

Every interop result so far carried the same caveat: *it proves compatibility
with omniORB*. One peer is the most dangerous kind of evidence, because it feels
like proof. JacORB 3.9 is now a second, independently implemented peer, in both
directions.

지금까지의 모든 상호운용 결과에 같은 단서가 붙어 있었다 — *omniORB와의 호환을
증명할 뿐*. 피어 하나는 가장 위험한 종류의 증거다. 증명처럼 느껴지기 때문이다.

| Direction | Result |
|---|---|
| JacORB client → our Rust server | **5/5** |
| Our Rust client → JacORB server | **14/14**, both byte orders |
| Codeset negotiated with the second peer | **UTF-8**, Korean round-trips |

## What the second peer actually exercised

**A big-endian request path.** JacORB is big-endian, being Java; omniORB was
little-endian. Our server had decoded a great many requests and never one from a
big-endian peer. The harness now asserts this rather than hoping for it — it
greps the server log for a GIOP 1.2 **(Big)** request and fails if the second
peer stopped providing one.

**Negotiation against a different `TAG_CODE_SETS`.** Both peers publish
different codeset components, and negotiation reached UTF-8 with each. Agreeing
with two independently written implementations is much better evidence that the
logic reads real data than agreeing with one twice.

**JacORB는 자바라서 big-endian이다.** 우리 서버는 수많은 요청을 디코딩했지만
big-endian 피어의 요청은 한 번도 받은 적이 없었다. 하네스가 이제 이를 단언한다.

## The fixture met JEP 320 in person

JacORB 3.9 will not run on a modern JDK without help, for two reasons that are
the project's own subject matter looking back at it:

1. **`java.applet.Applet`** is referenced by JacORB's `ORB.init` signature and
   was removed in JDK 24, so the fixture will not even *compile* there. It needs
   JDK 21.
2. **`javax.rmi.CORBA`** was removed by [JEP 320](https://openjdk.org/jeps/320)
   in JDK 11 and JacORB still needs it at runtime. Supplying it required a
   standalone RMI-IIOP API jar; the obvious GlassFish one drags in
   `com.sun.corba.ee` internals, so the JBoss spec jar is used instead.

The migration `docs/PLAN.md` §1.2 cites as *demand for automation* is the same
one that cost this batch three attempts to stand up a test fixture.

`docs/PLAN.md` §1.2가 *자동화 수요*라고 지목한 그 마이그레이션이, 이번 배치에서
테스트 픽스처 하나 세우는 데 세 번의 시도를 쓰게 만든 바로 그것이다.

## Two harness bugs, both self-inflicted

- **The JacORB check called `start_server`, which launches the *omniORB*
  fixture.** The JacORB client then dialled a stale IOR and failed while the
  same command passed by hand. Fixed by naming the two helpers apart:
  `start_server` for the omniORB fixture, `start_rust_server` for ours.
- **`./run_checks.sh | tail; echo $?` reports `tail`'s exit code**, not the
  harness's. It read as green while the verdict line said failed. Measuring the
  wrong thing is the recurring theme of this project's harness bugs.

## Skipping is not passing

The JacORB jars are downloaded artefacts and are not committed, so a fresh clone
cannot run this check. The harness reports it as **SKIPPED** and the verdict line
says so separately from the pass count:

```
  1 check group(s) SKIPPED — those claims are unmeasured, not passing
  all measured checks green
```

`spikes/jacorb/setup.sh` fetches and builds the fixture reproducibly.

---

# Batch 6: `any` and `TypeCode`

The piece the AI path rests on. `docs/PLAN.md` §2.1 claims CORBA is "a runtime
self-describing type system", and `TypeCode` is what makes that true rather
than aspirational — it is how a value describes itself well enough to be
decoded by a caller that has never seen its IDL. AnyJSON's `_t` field had
assumed it existed since v0.2.

Delivered: `TCKind` 0–28, all three parameter forms (empty, inline, complex
encapsulation), indirection with recursion, and `any`. **20/20 against both
peers**, including a struct `any` carrying its own `TypeCode`.

## Two alignment rules that contradict each other

Complex parameters live in an encapsulation, so **alignment restarts** at its
byte-order flag (§9.3.3). Indirection offsets are measured in the **outermost**
stream (§9.3.5.1), so an offset can point out of the encapsulation it sits in.

Satisfying both means writing everything into one buffer and moving the
alignment origin in and out of each encapsulation, rather than building
encapsulations in buffers of their own. That is why `Encoder::set_origin`
exists.

복합 파라미터는 캡슐화 안에 있어 **정렬이 재시작**하지만, 간접참조 오프셋은
**최외곽** 스트림 기준이다. 둘을 동시에 만족시키려면 모든 것을 한 버퍼에 쓰고
캡슐화마다 정렬 원점을 넣고 빼야 한다.

## Recursion

`corpus/golden/15-forward-recursive.idl` has a struct containing a sequence of
itself. Without indirection its `TypeCode` does not terminate. Rust cannot hold
the cycle and flattening would not halt, so a self-reference decodes to
`Recursive(repository_id)` — which is honest, because a recursive type has no
finite expansion and the consumer is the only thing that can decide what to do
with that.

## The bug the peer caught that our tests could not

Self-round-trips passed for every type, including nested structs. omniORB
rejected the struct `any` immediately, with *"Garbage left at end of input
message"*.

The cause was not in the `TypeCode` encoder. **An `any`'s value is marshalled
immediately after its `TypeCode`, with alignment continuing from there**, so
its internal padding depends on where the whole `any` lands in the message. The
test built the value in a buffer of its own, starting at offset zero — padding
computed for a position it would never occupy.

It survived `any/long`, which has no internal padding, and `any/string`, which
happened to land right. A struct of `octet, long, short, double, octet` did
not. The peer reported garbage at the *end* of the message rather than an error
at the offending field, which is what a padding error looks like from the
outside.

This is the third appearance of one root cause — **a detached buffer aligning
from zero** — after the GIOP 1.0/1.1 body in Batch 3. It is now codified where
it can be seen: the API is a closure that writes into the live stream
(`encode_any_with`), and the raw-bytes form is named
`encode_any_at_same_alignment` so its precondition is in the call.

같은 근본원인의 세 번째 등장이다 — **분리된 버퍼가 0에서부터 정렬하는 것**. API를
라이브 스트림에 쓰는 클로저로 바꾸고, 원시 바이트 형태는 전제조건이 호출부에
드러나도록 이름에 담았다.

A real encoder bug was also found and fixed along the way: `encapsulation_end`
restored the alignment origin to zero instead of the enclosing value, which
misaligns a struct nested inside a sequence. Our round-trips missed it because
the decoder saved and restored correctly and the offsets coincided.

---

# Batch 7: object-reference acquisition

Everything before this found a target by reading a stringified IOR out of a
file. That is fine for a spike and is not how anything is deployed: real
systems publish into a naming service and hand out a URL. The dynamic invoker
cannot look a target up in a catalogue without this.

Delivered: `corbaloc:` and `corbaname:` parsing (§7.6.10), a CosNaming client
with both `resolve` and `resolve_str`, and an end-to-end path verified against
a real `omniNames`.

```
object-reference acquisition — corbaname: through a real naming service
    ok   connected to the naming context
    ok   resolve() returned IDL:spike/Echo:1.0 (1 profile(s))
    ok   resolve_str() agreed with resolve()
    ok   ping() through the resolved reference -> 42
  ok   naming service contacted at GIOP 1.0, as corbaloc defaults require
```

## The defaults are the trap, and Batch 1 already paid for them

`corbaloc::host/Key` — empty protocol token, no port — is legal and extremely
common. §7.6.10.3 then fills in **IIOP 1.0** and **port 2809**.

The version default is the interesting one. Contacting a naming service through
a bare `corbaloc:` URL means speaking **GIOP 1.0**, and until Batch 1 this
implementation always emitted 1.2 regardless of what the peer advertised. That
defect (cause C1) would have made every `corbaname:` resolution fail with a
`MessageError` — the audit's finding predicted exactly this case, and here it
is in the ordinary path rather than a corner.

The harness asserts the negotiated version rather than only the outcome, so a
silent upgrade to 1.2 cannot hide a regression behind a passing test.

기본값이 함정이다. 버전 없는 `corbaloc:`는 **GIOP 1.0**을 뜻하고, Batch 1 이전에는
피어가 무엇을 광고하든 1.2를 보냈다. 그 결함(C1)이었다면 모든 `corbaname:` 해석이
`MessageError`로 실패했을 것이다. 감사가 예측한 그 케이스가 구석이 아니라 평범한
경로에 있었다.

## Details worth pinning

- **IPv6 must be bracketed.** `corbaloc:iiop:[::1]:88/Key` — unbracketed, the
  address's own colons are read as a port separator and the host truncates to
  nothing. The key split also has to skip over brackets before looking for `/`.
- **The object key is bytes, not text.** `%XX` escapes decode to raw octets,
  including `%00`; it is opaque server state and may hold anything.
- **A comma-separated address list becomes one profile per address**, so
  multi-profile failover covers it without a second mechanism.
- **`corbaloc:rir:` addresses nothing dialable** and deliberately produces no
  IOR — it is a request to resolve locally.
- Parse failures name the `BAD_PARAM` minor code §7.6.10.3 assigns (7–10), so a
  diagnostic and the specification say the same thing.

---

# Batch 8: GIOP fragmentation

Detected-and-refused was the correct interim behaviour and not the requirement.
`docs/PLAN.md` §4.4 makes receive mandatory, and a peer that fragments is not
exotic: any file transfer or large sequence reaches it.

Delivered: reassembly for GIOP 1.2, send-side splitting, and the hostile-input
bounds a reassembler needs.

## What could be proven, and what could not

This is the first batch where the honest answer is *half*.

| Direction | Evidence |
|---|---|
| **We fragment, they reassemble** | ✅ omniORB **and** JacORB both reconstruct 250 KB split at a 4 KB threshold — roughly 61 fragments — byte for byte |
| **They fragment, we reassemble** | ⚠️ **No independent evidence.** Covered only by round-trip against our own emitter |

Neither available peer emits GIOP fragments. Two assumptions failed on the way
to finding that out:

- **omniORB's `giopMaxMsgSize` is a hard cap, not a split threshold.** Setting
  it to 8 KiB and asking for 40 KB produced
  `MARSHAL_MessageSizeExceedLimitOnClient` and a `MessageError`, not fragments.
- **JacORB 3.9 has no GIOP fragmentation property.** `jacorb.fragment_size`
  changed nothing, and the only `Fragment` class in the jar belongs to MIOP,
  which is multicast rather than GIOP.

The emission side is still strong evidence: our fragments are only reassembled
correctly if the `divisible-by-8` rule for non-final pieces, the
`FragmentHeader_1_2` request id and the flag handling are all right. Two
independent readers agreeing on 61 pieces is not a coincidence.

**증명된 것은 절반이다.** 두 피어 모두 GIOP 분할을 내보내지 않는다. omniORB의
`giopMaxMsgSize`는 분할 임계값이 아니라 하드 상한이고, JacORB 3.9에는 GIOP 분할
속성이 없다. 송신 측은 두 독립 구현이 61개 조각을 정확히 복원하므로 강한 증거지만,
수신 측에는 독립 검증이 없다 — 그렇게 적어 둔다.

## The false pass this batch nearly shipped

The first JacORB run reported 4/4 green at every size. It proved nothing: the
peer had sent whole messages and nothing was reassembled. The check now counts
fragments per logical reply and says `note … the peer did not fragment` rather
than `ok`, which is what turned an apparent success into the finding above.

첫 JacORB 실행은 모든 크기에서 통과했고 아무것도 증명하지 못했다. 피어가 통짜로
보냈기 때문이다. 이제 논리 응답당 조각 수를 세고, 분할이 없었으면 `ok`가 아니라
`note`로 보고한다.

## Bounds a reassembler needs

- A cap on fragments per logical message, or a peer that never sets the final
  bit holds the connection open and grows the buffer without limit.
- The running total is checked against the message ceiling, not just each piece.
- A fragment whose request id does not match is a desynchronisation, not
  something to append.
- **GIOP 1.1 fragments are refused.** They restart alignment per fragment and
  carry no request id, so concatenation is not reassembly and there is no way
  to tell whose fragments they are. Refusing beats producing a plausible wrong
  value.

## The lint had a hole, and the rule found it

Adding a `Payload blob(...)` operation against a `Blob` typedef went straight
to the oracle: the lint matched `Type ident` followed by `;` `,` `)` or `[`,
and an operation name is followed by `(`. Fixed, with
`corpus/negative/n10-operation-name-clash.idl` pinning it. **A lint that
catches most forms of a rule still lets the rule cost you** — that is the
fourth distinct shape this one identifier rule has taken.

---

# Batch 9: `wchar`, `wstring` and `long double`

The last of the Phase 1 type surface, and the part where the wire form changes
between GIOP versions in ways that corrupt rather than fail.

| | `wstring` length means | terminator | `wchar` |
|---|---|---|---|
| **1.0** | — | — | **illegal** (§9.3.1.6) |
| **1.1** | wide characters, **including** a terminating null | yes | fixed 2 octets |
| **1.2** | **octets**, and zero is legal | no | octet count then octets |

Reading a 1.2 `wstring` with the 1.1 rule takes an octet count as a character
count and then hunts for a terminator that is not there. Nothing about that
fails loudly.

`long double` is carried as 16 raw octets. Rust has no stable 128-bit float, and
routing it through `f64` would silently discard precision the peer took care to
send.

## The BOM, which self-tests could never have found

Our first `wstring` implementation round-tripped perfectly against itself and
against a little-endian peer. The **big-endian** client came back with
`眀椀搀攀` — U+7700 where `w` (U+0077) belonged, every unit byte-swapped, from
**both** peers.

So neither omniORB nor JacORB infers wide-character order from the enclosing
message's byte order. omniORB in fact prepends a byte-order mark to everything
it sends. We now write an explicit BOM and act on one when reading, including
swapping every unit when the mark comes back reversed — which removes the
ambiguity rather than betting on which convention a peer picked.

우리 첫 구현은 자기 자신과도, little-endian 피어와도 완벽히 왕복했다. **big-endian**
클라이언트에서는 두 피어 모두 모든 단위가 바이트 스왑되어 돌아왔다. 어느 쪽도
메시지 바이트오더로 와이드 문자 순서를 추론하지 않는다. 이제 BOM을 명시적으로
쓰고, 뒤집힌 BOM을 만나면 전 단위를 스왑한다.

Reversed-BOM handling matters more than it looks: without it the result is
plausible CJK text rather than an error, so a corrupted field reaches a database
looking like data.

## What each peer would and would not do

| | 1.2 `wstring` | 1.1 `wstring` |
|---|---|---|
| omniORB | ✅ | ❌ **declines** — publishes `UTF-16(1.2)` only |
| JacORB | ✅ | ✅ |

omniORB answers a 1.1 `wstring` with `BAD_PARAM`, OMG minor **23** — *wchar used
against a peer that declared no wchar transmission codeset*. That is its
published position, not a fault in what we sent, and the harness now says so
instead of counting someone else's policy as our failure.

Getting that right needed one more fix: a minor code is a 20-bit vendor id plus
a 12-bit value, and printing all 32 bits turned "minor 23" into "1330446359",
hiding which condition the peer had actually reported.

**Result: 1.2 validated against both peers in both byte orders; 1.1 validated
against JacORB.** The 1.1 rule is therefore confirmed by an independent
implementation rather than by our own encoder agreeing with our own decoder.

**1.2는 두 피어 양쪽 바이트오더로, 1.1은 JacORB로 검증됐다.** 1.1 규칙이 우리
인코더와 디코더의 합의가 아니라 독립 구현으로 확인됐다는 뜻이다.

## Also refused rather than approximated

A character outside the Basic Multilingual Plane is a surrogate pair — two
UTF-16 units, and therefore not one `wchar`. Emitting half a pair would hand the
peer a lone surrogate, so it is an error.

## Still open after Batch 9
- Wiring the negotiated converter through `Connection` and the string paths,
  including the per-connection "send the context once" rule and the
  `MARSHAL` minor 9 case for conflicting contexts on one connection.
- Everything in the Batch 1 list below.

---

## What Batch 1 did not do

Stated plainly so the next batch does not have to rediscover it. All of this is
Phase 1 scope from `docs/PLAN.md` §7 and remains open:

- **Codeset negotiation** — `TAG_CODE_SETS` parsing, the `CodeSets` service
  context, and conversion for UTF-8, UTF-16, ISO-8859-1 and EUC-KR. This is
  the highest-value gap for the domestic market and is blocked on decision
  `D001` (which dependency, if any, supplies EUC-KR conversion).
- **`LocateRequest` send, `CancelRequest` send** — served but not sent.
- **`wchar`/`wstring` and `long double`** — `any` and `TypeCode` landed in
  Batch 6; these remain.
- **Multi-profile failover, `TAG_ALTERNATE_IIOP_ADDRESS`, SSLIOP port
  extraction.** Components are now preserved but not interpreted.
- **A second interop peer.** TAO is not in homebrew and needs a source build;
  JacORB needs a JDK, now installed. Until a second peer exists, every interop
  result carries the Phase 0 caveat: it proves compatibility with omniORB.

---

# Stream E batch 1: LocateRequest, sent at last

Carried forward on every phase report since Phase 2: the server side has
answered `LocateRequest` all along, but nothing here had ever **sent** one.
The batch unit is PLAN §7.3's — one capability, both peers, all three GIOP
versions — and both answers are measured, because a locate that can only
produce "here" has not been tested against anything.

```
wire hardening — LocateRequest send, both peers, all three versions
  ok   omniORB: OBJECT_HERE for the real key, UNKNOWN for a corrupted one, GIOP 1.0/1.1/1.2
  ok   JacORB agrees on all six answers — a second, independent locate responder
```

`Connection::locate()` asks about the connection's own object;
`locate_key(&[u8])` exists so the harness can prove the **negative** answer —
the bogus key is the real one with every byte inverted, same length and shape,
so the refusal cannot be an artifact of a malformed probe. An unmeasured
refusal is not a refusal.

Two wire details the unit tests pin, both version asymmetries:

- **GIOP 1.2 wraps the key in `TargetAddress`** (`KeyAddr`, discriminant 0);
  1.0/1.1 carry the bare sequence. The client encoder is checked against the
  server decoder we have had since Phase 2 — two halves of one rule, agreeing
  before any peer is asked.
- **A `LocateReply` body is never 8-aligned**, even in 1.2, unlike a `Reply`
  (§9.4.6). A decoder borrowing the `Reply` rule reads a forwarded IOR four
  bytes late. `LOC_SYSTEM_EXCEPTION` (status 4) surfaces as the system
  exception it carries.

서버 쪽은 Phase 2부터 locate에 답해 왔지만 **보내는** 쪽은 이월 목록에만 있었다.
음의 답도 측정한다 — 가짜 키는 진짜 키의 전 바이트를 뒤집은 것이라 길이와 모양이
같고, 거부가 형식 오류의 부산물일 수 없다. 측정되지 않은 거부는 거부가 아니다.

Remaining on the stream-E list: `CancelRequest`/`CloseConnection` send,
request multiplexing, connection pooling, multi-profile failover,
`TAG_ALTERNATE_IIOP_ADDRESS`, `#pragma prefix`, and independent validation of
fragment reception.
