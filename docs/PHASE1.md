# Phase 1 — Batch 1: hardening the wire core

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

## Still open after Batch 3
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
- **`Fragment` reassembly.** Currently detected and refused with a clear error
  rather than silently truncated, which is the correct interim behaviour but not
  the requirement.
- **`LocateRequest` send, `CancelRequest` send** — served but not sent.
- **`wchar`/`wstring`, `any`, `TypeCode`, `long double`, and inline object
  references in general** — `Ior::read_from` exists now, but the constructed-type
  surface is still only `sequence<octet>` and `string`.
- **`corbaloc:` / `corbaname:` / CosNaming** — no object-reference acquisition
  beyond a stringified IOR.
- **Multi-profile failover, `TAG_ALTERNATE_IIOP_ADDRESS`, SSLIOP port
  extraction.** Components are now preserved but not interpreted.
- **A second interop peer.** TAO is not in homebrew and needs a source build;
  JacORB needs a JDK, now installed. Until a second peer exists, every interop
  result carries the Phase 0 caveat: it proves compatibility with omniORB.
