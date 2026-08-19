# Changelog / 변경 이력

Measurements live in [`docs/COMPONENTS.md`](docs/COMPONENTS.md); this file
records what changed and, where it matters, what it changes on the wire.

측정은 `COMPONENTS.md`에, 여기에는 무엇이 바뀌었는지와 — 중요한 경우 — 그것이
와이어에서 무엇을 바꾸는지를 적는다.

---

## Unreleased

### ⚠ Mapping changed / 매핑 변경

- **AnyJSON v1.1: a type describes itself structurally** (D008, approved
  2026-08-18). `_t` keeps its v1 name for a type whose identity fits in one —
  `"double"` is still `"double"` and every v1 document still reads and still
  reproduces the same CDR — and becomes an object where v1 could say nothing,
  or where the name lost something the wire keeps: `string<5>` and `string`
  were one word to v1 and are two TypeCodes to a peer. The same representation
  is now also a **value**, so `::CORBA::TypeCode` crosses.

  Measured before the change: `to_json` wrote `{"_t":"IDL:gc12/Tagged:1.0",…}`
  and `from_json` **refused that same document**. Not a limitation — an
  asymmetry, which puts the failure on the return leg in the caller rather than
  at the boundary that produced it. And `tk_TypeCode` had no `Value` variant at
  all, so §8's *static equals dynamic* oracle was not weaker for the operations
  only the static path handled; it was **inapplicable**.

  **`ir-subset` went from 18 generated + 10 skipped to 28 + 0.** The ten
  included `InterfaceDef` itself — the skip propagated up through every
  container until `describe_interface`, the operation the IFR facade exists
  for, could not be generated. The MCP bridge speaks the same mapping, so an
  **Interface Repository is now readable through the agent path**, asserted by
  repository id over the real contract in both byte orders.

  **Upgrading:** nothing to do for a v1 producer or consumer; the change is
  additive and the compatibility claim is tested, not asserted. A Python client
  gains `_rt.TypeCode` — the document, unread: enough to receive one, hand it
  back and inspect its `kind`, not enough to marshal a value *described* by
  one, which would mean Python deciding CDR questions in a package that
  deliberately contains no wire.

  AnyJSON v1.1: `_t`가 이름 하나에 담기는 타입은 v1 그대로, 그 밖에는 구조가
  된다. **추가적**이므로 v1 문서는 전부 그대로 읽히며 그 주장은 시험된다.
  변경 전 실측: 매핑이 자기가 쓴 문서를 자기가 거부했고, `tk_TypeCode`에는
  `Value`가 없었다. **ir-subset 18+10 → 28+0**, 그 열에는 `InterfaceDef` 자신이
  들어 있었다.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **1.1 `wchar` measured against JacORB 3.9, both directions and both byte
  orders** (D010 B5, second half; `spikes/wide.idl`, `spikes/jacorb_wchar11.sh`,
  `tests/wide_1_1_from_a_peer.rs` appended). A 1.1 `wchar` is its two octets
  in the **message's** order and nothing else — JacORB writes `d5 5c` for
  U+D55C in its big-endian messages, reads our `5c d5` in a little-endian
  reply as U+D55C and the control `d5 5c` in the same frame as U+5CD5 (4/4
  units, both directions); U+FEFF is data at 1.1 on both sides. That is what
  `put_wchar`/`get_wchar` always did — **no code change**; the "unmeasured"
  doc paragraph is replaced by the measurement. Recorded behaviour: a lone
  surrogate crosses as two octets (we refuse it as not a character, and refuse
  U+1F600 as two units); four octets offered as one wchar: JacORB takes the
  first two. JacORB's whole request and reply are recorded and decode through
  `decode_request`/`decode_reply`; the live octets are re-checked against the
  recording on every run. Not measured: our live Rust stack on this contract
  (the hand-built peer stands in; the codec is held to the octets by tests);
  JacORB's writer in a little-endian 1.1 message.

  **1.1 `wchar`를 JacORB 3.9에 대해 양방향·양 바이트 순서로 측정**(D010 B5
  후반). 1.1 `wchar`는 **메시지의** 순서로 놓인 두 옥텟이고 그 이상이 아니다 —
  JacORB는 U+D55C를 빅엔디언 메시지에 `d5 5c`로 쓰고, 우리가 리틀엔디언 응답에
  넣은 `5c d5`를 U+D55C로, 대조군 `d5 5c`를 U+5CD5로 읽었다(양방향 4/4).
  U+FEFF는 양쪽 모두 데이터. 코덱이 원래 하던 그대로 — **코드 변경 없음**;
  "미측정" 문단을 측정으로 교체. JacORB의 요청·응답 전체를 기록하고 실행마다
  라이브 옥텟과 대조한다. 미측정: 이 계약에서 우리 실제 Rust 스택(손으로 만든
  피어가 대신 섬); 리틀엔디언 1.1 메시지의 JacORB 라이터.
- **`wstring` at GIOP 1.0/1.1 is written without a byte-order mark and with
  its units in the message's byte order** (was: BOM + stream order, as at
  1.2). D010 B5's first step was a JacORB-at-1.1 fixture, and it found this the
  same hour: measured against JacORB 3.9 (`spikes/jacorb_giop11.sh`, both
  directions, both byte orders), a 1.1 peer read our mark as U+FEFF text, and
  its echo of it was stripped by our reader, so the round trip was green while
  the peer's user saw the wrong value; an unmarked value is read by that peer
  in the message's order — the literal §9.3.1.6 "neither → big-endian" bullet
  is contradicted by the only 1.1 wide-text peer here, and the code says so.
  GIOP 1.2 unchanged (mark, big-endian default). The reader still removes a
  leading mark at 1.1 ("shall remove", unscoped). Recorded with provenance in
  `crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs`; the fixture asserts
  the version from bytes (JacORB's `giop_minor_version` sets the IORs it
  creates; its client follows the profile it dials) and counts wire units so
  the masking cannot recur. Harness group added; a class-B row that turned out
  measurable, and measured red first.

  **GIOP 1.0/1.1의 `wstring`은 바이트순서표식(BOM) 없이, 유닛을 메시지의
  바이트순서로 기록한다**(이전: 1.2와 같이 BOM + 스트림 순서). D010 B5의 첫
  단계인 JacORB-1.1 픽스처가 같은 시간에 이것을 찾았다: JacORB 3.9로
  측정(`spikes/jacorb_giop11.sh`, 양방향·양 바이트순서), 1.1 피어는 우리
  표식을 U+FEFF 문자로 읽었고 그 에코를 우리 리더가 표식으로 벗겨내어 왕복은
  녹색이면서 피어 사용자에게는 잘못된 값이 갔다; 표식 없는 값은 그 피어가
  메시지 순서로 읽는다. GIOP 1.2는 변경 없음. 리더는 1.1에서도 선행 표식을
  제거한다. `tests/wide_1_1_from_a_peer.rs`에 출처와 함께 기록; 하네스 그룹
  추가 — 잴 수 없다던 B류 행이 재어졌고, 먼저 빨갛게 재어졌다.


- **A union `TypeCode`'s case labels are aligned and byte-order-normalised.**
  A label is the discriminator marshalled in its own type. It was read with
  `get_bytes` and written with `put_bytes` — neither of which knows the
  endianness, and neither of which aligns — so it carried the byte order of
  whatever stream produced it. Our encode and decode agreed with each other in
  any order, which is why 1200 tests were green while, against the fixture peer
  on this host:

  - a `long long` discriminated union **could not be decoded at all**, and said
    so as `"string length must include the NUL"` — pointing four fields past
    the fault, at a string, because an unaligned 8-byte read shifted everything
    after it;
  - a `long` discriminated union decoded and then missed **every** branch, in
    both stream orders, with a refusal that blamed the caller's discriminator.

  `UnionCase::label` is now always big-endian and conversion happens at the
  wire. Both cases are pinned as the bytes omniORB actually wrote, and the
  regression test also **re-encodes them back to the peer's bytes** — the check
  our own round trip could not perform, because it agreed with itself.

  **Upgrading:** anything holding a `UnionCase::label` built by hand for a
  little-endian stream was already wrong on the wire and is now wrong in the
  type; build labels big-endian. Nothing else changes.

  union 레이블은 판별자를 그 타입으로 마샬링한 것인데, 정렬 없이 원바이트로
  읽고 썼기에 **온 스트림의 바이트 순서를 그대로** 지녔다. 우리끼리는 일치했으므로
  1200건이 초록이었다: `long long` union은 **디코드조차 되지 않았고**(진단은 네
  필드 뒤의 문자열을 가리켰다), `long` union은 **모든 분기를 빗나가며** 호출자를
  탓했다. 이제 레이블은 항상 빅엔디언이고 변환은 와이어에서 일어난다. 피어가
  실제로 쓴 바이트를 고정했고, **그 바이트로 되돌려 인코딩되는지**까지 검사한다.

- **AnyJSON writes a union's case labels as values, not base64.** They were
  base64 for exactly as long as their byte order was unknowable — one commit.
  An enum discriminator's labels now read `"label":"GREEN"` rather than four
  bytes that say nothing. A label that will not decode falls back to
  `{"_raw": <base64>}`, tagged, because a malformed TypeCode is its producer's
  problem and a renderer that refuses to render it hides the evidence.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **Six wire defects our own round trip could not see.** A sweep of
  `orbweaver-cdr` and `orbweaver-giop` for the class the union-label batch
  named — a field both our ends agree about and a peer does not — examined 24
  candidates and found 6 defects in 3 causes, with 17 correct and their reasons
  written down.

  `long double` moved its 16 octets with no byte-order reversal, where Figure
  9.2 draws the same reversal `float` and `double` get. A UTF-16 `wchar` or
  `wstring` took its byte order from the **message** when the value states its
  own — three sites, and the read half is one our round trip could never reach,
  because our writer always emits a BOM and so does omniORB's, so only the
  peer's *reader* could settle it: twelve bodies read back, six answers, none
  depending on the stream's flag. We returned U+7700 for an unmarked `00 77` in
  a little-endian stream where the peer returns U+0077. And an alignment origin
  leaked the enclosing buffer's offset, so a `TypeCode` encapsulation inside a
  GIOP 1.0/1.1 body aligned from the message's offset rather than its own flag
  — unreachable at 1.2, which rounds the body start to a multiple of 8, and
  unconditional below it.

  Two preconditions became checks rather than prose: relaying an `any` into a
  different byte order is refused, and a `Fragment` that flips the byte-order
  flag is refused per §9.4.9.

  Where no peer could answer, the specification is the oracle and the code says
  so. GIOP 1.1 `wchar` unit order is **left unchanged and recorded as
  unmeasured**: omniORBpy marshals one and then fails to unmarshal its own
  output on this host, so nothing here can settle it.

  우리 왕복이 볼 수 없던 결함 6건 — 양쪽이 서로만 합의하는 관례가 숨긴 것들.
  피어가 오라클일 수 없는 자리는 **미측정으로 남겼다.**

- **A reference that declares no codeset refuses wide text, and ours declared
  none.** §7.10.2.4: a profile with no `TAG_CODE_SETS` declares no wchar
  support. Measured: omniORB's client raised `INV_OBJREF` minor `0x4F4D0001`
  **inside itself** and sent nothing, while our server logged one earlier
  request and no error. **Every `wstring` operation we serve was unreachable by
  a conformant peer, and the refusal happening in the caller is why nothing
  here could see it.** Eleven publish sites now carry the component; `한글`
  round-trips at GIOP 1.2.

  A third defect fell out of the counting: `negotiated_char_converter`
  swallowed every negotiation failure into `None`, so "we cannot agree with
  this peer" was indistinguishable from "this peer said nothing" — the one peer
  that warned us was the one we told nothing.

  `spikes/reverse_client.py` had exercised every other operation on the echo
  contract and never `echo_wstring`. It does now, and found on its first run
  that GIOP 1.1 raises `MARSHAL`; that is **stated as unmeasured**, because the
  peer cannot unmarshal its own 1.1 wide output and so is not an oracle for it.

- **A stub's `wstring` takes its form from the connection.** `Cdr::put(&self,
  e: &mut Encoder)` has no connection to ask, so `WString` answered with GIOP
  1.2's form always — 1.2 counts octets and 1.1 counts characters, so on a 1.1
  connection that is a different field. Nothing here could see it: **our own
  round trip used the same constant at both ends**, which is the shape the
  union-label batch named — a convention both ends apply cannot be refuted by a
  round trip.

  A stream with nothing attached still writes the 1.2 form, and that is not a
  leftover: §9.3.1.6 fixes it for an encapsulation whatever the message says.
  Both halves are asserted, and reverting the fix fails the first.

  D009 §7.3 said to retire `wide()` and `default_codec()` together, and
  **counting the call sites showed it had them backwards**: every wire use of
  `default_codec()` is inside an `any`'s encapsulation, where the fixed answer
  is the specification. Its doc gave two justifications and only one was a
  reason — the other was a guess sitting next to a rule, borrowing its
  authority. The decision is corrected in place.

- **The transmission codeset reaches the marshaller** (D009, approved). A CDR
  stream carries an optional `TextCodec`; `None` is UTF-8 and is byte-for-byte
  what shipped before. A connection puts its negotiated `char` agreement on the
  encoder that carries a call's arguments, and a servant reads and answers in
  the codeset the **client declared** — derived from the request, not the
  connection, because two clients on one multiplexed connection can differ.

  The codec goes on the body and never the header: `operation` and the object
  key are identifiers the contract chose, and re-encoding them would change the
  name a servant dispatches on. An encapsulation does not inherit it either —
  that would silently re-encode the repository ids and member names inside
  every `TypeCode`.

### Decided / 결정

- **D010 — what remains, and which of it cannot be measured here** (PROPOSED).
  Written from the current gap columns rather than from memory, because the
  session that produced it found four "gaps" already closed and four gates
  green while measuring nothing — progress was wrong in both directions. It
  splits the remainder into four classes: **A** buildable and measurable here,
  **B** buildable but the oracle is absent (lands only as a SKIPPED harness
  group naming its fixture, never as `ok`), **C** deferred with a trigger that
  has not fired (building early is the defect), **D** a claim in a document
  that cannot be tested. Six A items ordered by cost of defect, six B items
  each naming what is missing, eleven C items each naming where its reason
  lives, and the five D rows yesterday's plan batch left. Two process
  proposals: a gap-column symbol check, and a rule that a new harness group
  lands with its negative control in the commit message.

  기억이 아니라 현재의 공백 열에서 썼다. 남은 것을 네 부류로 가른다 — 여기서
  짓고 잴 수 있는 것, 지을 수 있으나 오라클이 없는 것(`ok`가 아니라 픽스처를
  이름 붙인 SKIP으로만 착지), 방아쇠 달린 유예(일찍 짓는 것이 결함), 시험 불가한
  문서 주장.

### Added / 추가

- **CosNaming serves `bind_context`, `rebind_context` and `destroy`.** Two of
  the three deferral reasons were **descriptions of the servant rather than
  obstacles**: contexts lived as long as the process *because* nothing removed
  a key, and binding a context this dispatch already serves is a map insert,
  not a call over the wire — it was also the only way the already-served
  `new_context` produced anything reachable. Binding a **foreign** context stays
  deferred with a rewritten reason: it is implementable now, and that is a
  reason it is possible, not a reason to do it.

  A peer drove it — 20 labelled rows, every expected value measured against
  omniNames 4.3.4 first, and two deliberate divergences from omniNames recorded
  (it type-checks neither rebind, and accepts any reference for
  `bind_context`). The property the module rests on is now **checked**: the
  servant names no `Connection`, `Pool`, `Mux`, `invoke`, `TcpStream` or
  `connect(`, and all 16 operations run with nothing held.

  Its negative control changed a test: the lock sweep *passed* with a violation
  planted in `destroy`, because `destroy` at a populated root stops at
  `NotEmpty` and never reaches the removal.

  Landing it meant the generated-skeleton comparison had to follow, and that
  comparison caught the interesting part. `destroy` had sat near the top of the
  script as a deferral both halves refused; once served, it **destroyed the
  root before every other step ran** — both servants identically, so the byte
  comparison stayed green while value-carrying replies fell from 25 to 5.
  **Agreement by mutual destruction**, which is exactly what
  `the_comparison_is_not_vacuous` exists to catch. The lifetime steps moved to
  the end, and the `NOT_COMPARED` entry that recorded an ordering difference
  was **retired rather than deleted**: the difference existed *because*
  `bind_context` was deferred, and it left with the deferral.

### Added / 추가

- **CosEvent serves the pull model's consumer side** — `obtain_pull_supplier`,
  `connect_pull_consumer`, `pull`, `try_pull`, `disconnect_pull_supplier`. The
  deferral's reason was *"the same unbounded buffer this module spends its
  bounded queue avoiding, for no named consumer"*, and **only the second clause
  survived measurement**: a pull proxy holds events in the same bounded deque,
  moved by the same knob, dropped oldest-first into the same counter. Nine
  pushes into a limit of three give `queued=3, dropped=6` on both sides.

  It **drops at the bound and blocks at the empty end**, deliberately and at
  different ends. Blocking a supplier — CORBA's own answer to a full channel —
  would let one slow puller wedge the channel for every other consumer.
  Blocking a caller that asked to wait is what `pull` means; it is bounded,
  woken by an arriving event, and expires as `TIMEOUT` with **`COMPLETED_NO`**,
  the load-bearing half: nothing was consumed, so calling again is safe.

  The **supplier** side stays deferred with a rewritten reason — there the
  channel is the puller, `PullSupplier::pull` is specified to block, and the
  channel would hold a thread per supplier on somebody else's clock, for no
  named supplier. `destroy` stays too, its reason moved from outbound calls
  (which `guarded` now answers) to authorization: it is an **unauthenticated
  remote operation that ends the channel for every other client**.

  CosEvent 19 → **24 served**, `NO_IMPLEMENT` 9 → 4. **No peer verified any of
  it** — omniEvents is absent and omniORBpy ships no `ProxyPullSupplier` stubs.

- **The sweep was measuring the wrong object.** It probed the pull operations
  against a *push* proxy, because no pull proxy could be obtained when that
  code was written — so the moment they were served it reported the whole
  `ProxyPullSupplier` interface as **unserved**. A false absence produced by
  asking the wrong reference, and one that appears exactly when the underlying
  thing gets better.

- **F5 was already served, so what was missing was the second direction.**
  `PLAN-SERVICES` §10 listed LifeCycle/Property as never started;
  `COMPONENTS.md` had it ✅ since 2026-08-14 and coverage measured 16/16 — the
  fourth "gap" this session that turned out to be closed. What was genuinely
  missing is one section down in that same document: *no cross-ORB direction*.
  An omniORB client now calls **all sixteen** through its own stubs. A hole it
  found: `bind_expert`/`set_policy` take references **no operation returns**.

- **S4 reads an `#include` from the item's own directory.** The thirteen-file
  estate scored **1/13 (8%)** first-pass, each contract refused for a file
  sitting beside it, and nothing was red because `estate/run.sh` amalgamates
  first. **13/13** now, with the exposure worksheet byte-identical to the
  amalgamated one — the equivalence that script had been assuming.

  The shape is a `Source`, not a path: a model writes IDL that was never a
  file. Text with no origin still says so in the same words, pinned by a test.
  Two sites shared the cause, including `DiffOutcome::compared` recording
  **`true` when the baseline failed to parse** — an unmeasured check reported
  as a pass, the third found this session.

### Known limits / 알려진 한계

- **The `char` conversion list stays empty, and that is now measured rather
  than cautious** (D009 §8 row 4, **BLOCKED**). The row conditioned a non-empty
  list on a peer that cannot reach UTF-8. Eleven configurations of the two
  installed ORBs were probed and ten measured: **every one reaches UTF-8**.
  Neither ORB exposes an option that *names* its conversion list — omniORB has
  `nativeCharCodeSet`/`defaultCharCodeSet` and accepts only ISO-8859-1 and
  UTF-8 as a native set; JacORB's list follows its build. The one setting that
  removes UTF-8, `jacorb.codeset=off`, removes the component entirely, and an
  absence is not an advertisement.

  What growing the list *would* cost was measured too, and it is not nothing.
  Offered ISO-8859-1 from a listener we wrote, against clients we did not:
  omniORB keeps sending UTF-8, and **JacORB configured native ISO-8859-1 moves
  down to it** — `café` as `63 61 66 e9` instead of `63 61 66 c3 a9`, and
  `함정 전투체계` as **each character truncated to its low octet, raising
  nothing**. §7.10.2.6 leaves that case open and the two ORBs resolve it in
  opposite directions; the empty list is what keeps the ambiguity unreachable.
  A guard test fails if anyone grows the list, and names the probe to run
  first.

  광고하지 않는 편이 옳다는 것이 **조심이 아니라 측정**이 되었다. 설치된 두 ORB의
  열한 구성 중 열을 측정했고 전부 UTF-8에 닿는다. 그리고 목록을 키웠을 때의 비용도
  쟀다: JacORB는 제안하는 즉시 **내려가고**, 한글을 **각 문자의 하위 옥텟으로 잘라
  보내면서 아무 예외도 올리지 않는다.**

### 🔒 Security / 보안

- **Twelve bytes bought sixty-four megabytes, and an overflow the release
  fuzzer could not see.** Two hazards reachable from peer bytes, both
  reproduced before being fixed.

  `csiv2` added a peer-supplied DER length to a cursor unchecked. Fed
  `60 88 FF FF FF FF FF FF FF FF` it panicked in debug and, in release,
  **wrapped and returned "GSS token is truncated"** — an error message that was
  a lie about what happened. `wire-fuzz` runs `--release`, where overflow
  checks are off, so it was **structurally blind to the class**: its "0 panics"
  was silent about it, not clearing it. The harness now carries the one run in
  this tree that can see an arithmetic overflow at all.

  And a GIOP 1.2 header declaring `message_size = 67,108,863` followed by
  silence committed and zeroed 67,108,875 bytes before a body byte arrived. The
  body is now read in 64 KiB chunks: the same header peaks at 65,548.

- **An array length from an agent's document bought 206 GB.** A 198-byte
  document declaring `array<octet, 4294967295>` as a union discriminator made
  `decode_at` reserve before reading — then refuse the stream as truncated a
  moment later, which is why nothing looked wrong. The `Sequence` arm fourteen
  lines above has carried the guard since Phase 0 with a comment naming the
  rule; the `Array` arm did not need it while every TypeCode it decoded against
  had been compiled here. AnyJSON v1.1 made that length a field in a document
  an agent sends.



- **An argument value a content stage saw could reach the audit ledger.** The
  `SEAT_SAFETY_CONTENT` interceptor seat reads argument *values* — that is what
  it is for — and it can also refuse. Its refusal is `Denied::Intercepted`,
  whose `reason` is free prose written by whatever stage a deployment
  installed, and `AuditInterceptor` rendered that prose verbatim. A content
  filter's most natural sentence names the thing it objected to, so the payload
  landed in the one artifact this crate writes to disk, retains, and greps.

  Measured, not reasoned: a real session with a marker in an argument produced

  ```
  REFUSE caller=alice … why=the safety.content stage refused this call:
         this looked like a credential: {"cents":"pin-s3cret-4242"}
  ```

  The channel was opened on 2026-08-14 by the batch that filled the seat, which
  checked that a stage *could* see the arguments and did not check its own
  second condition — that the audit must not thereby gain a way to log one.
  `guard.rs`'s claim that an audit line "can carry no credential material:
  nothing here holds one" was false from that day and left standing.

  **The ledger now takes the stage's name and drops its prose.** Typed refusals
  are unchanged — their fields are repository ids, operation and scope names,
  quota arithmetic, none of which can hold a byte the agent sent. The full
  sentence still reaches the caller, the dry-run report and every observer
  stage: readers who already hold the arguments. The gate did not move; a
  refusal still precedes anything being sent.

  **A gate that has to see a secret must not thereby be a gate that publishes
  one.** Two harness checks: the property, and the shape — an `audit_entry`
  call site taking a `Denied`'s `Display` — that would reintroduce it.

  내용 좌석은 인자 **값**을 읽는다(그러라고 있는 자리다). 그 좌석의 거부 사유는
  배포자가 설치한 스테이지가 쓴 자유 산문이고, 원장은 그것을 그대로 실었다 —
  디스크에 남고 사람이 grep 하는 유일한 산출물에. 실측: 인자에 넣은 PIN이 `why=`에
  그대로 찍혔다. 좌석을 채운 2026-08-14 배치가 **자기 두 번째 조건을 검사하지
  않았고**, `guard.rs`의 "자격증명은 담기지 않는다"는 주장은 그날부터 거짓이었다.
  이제 원장은 스테이지 **이름**만 싣는다. **비밀을 봐야 하는 게이트가 비밀을
  퍼뜨리는 게이트가 되어서는 안 된다.**

### ⚠ Behaviour changed / 동작 변경

- **A servant that deliberately does not implement a declared operation now
  answers `NO_IMPLEMENT`, not `BAD_OPERATION`.** The three answers are three
  different facts on the wire, with no document needed to tell them apart:
  `NO_PERMISSION` — the operation exists and the answer is no, as policy;
  `NO_IMPLEMENT` — declared, not implemented, on purpose; `BAD_OPERATION` —
  this interface does not declare that name at all.

  The IFR facade worked this out first and it stayed in one servant. Everywhere
  else a decision and an oversight gave the same answer, so the difference
  lived only in a document the client cannot read. Moved: CosNaming's
  `bind_context`, `rebind_context` and `destroy`; the event channel's whole
  pull model and its `destroy`; and `moe::Router::dispatch`.

  **`moe::Router::dispatch` is the one that mattered.** D006 approved excluding
  it on 2026-08-14, and the servant went on saying "no such operation" — a
  decision recorded in prose and contradicted on the wire, in exactly the class
  `PLAN-SERVICES.md` §8.1 exists to name. The new gate found it on its first
  green run.

  **Upgrading:** a client that treats `BAD_OPERATION` as "not implemented"
  should read `NO_IMPLEMENT` too; a client that treats it as "wrong object"
  now gets a more accurate answer.

  의도적 미구현은 이제 `NO_IMPLEMENT`로 답한다. 세 답이 와이어에서 세 사실이 되며,
  구분에 문서가 필요하지 않다. IFR만 갖고 있던 규칙을 다섯 서비스 전부에 적용했다.
  **`moe::Router::dispatch`가 핵심이다**: D006이 제외를 승인한 뒤에도 서번트는
  "그런 연산 없음"이라 답하고 있었다 — 산문의 결정을 와이어가 부정한 것.

- **The service sweep decides instead of counting, and fails on an absence.**
  A `BAD_OPERATION` from an object that *claims* the interface — measured by
  whether that object answers any other operation of it — is a servant
  half-serving something it says it is, and there is no longer a document to
  look it up in. An interface no object claims is reported as its own fact.
  `NO_IMPLEMENT` is also no longer counted as *dispatched*, which had
  overstated the IFR facade's served count by **14 operations**, in the
  direction that flatters.

  스윕이 세는 대신 판정하고, 부재에서 실패한다. `NO_IMPLEMENT`를 서빙으로 계수하던
  탓에 IFR의 서빙 수치가 **14만큼** 부풀려져 있었다 — 유리한 쪽으로.

### Known limits / 알려진 한계

- **`_rt.py` reads only a named type in an `any`'s `_t`.** The Rust half reads
  and writes the structural form; the Python half refuses it by name, with the
  decision cited, rather than accepting the document and marshalling `_v` as
  something else — the same rule the Rust side follows, applied to whichever
  implementation is behind.

### Added / 추가

- **The property sweep takes every value across AnyJSON and back.**
  `orbweaver-test`'s `one_case` now does `to_json` → text → `from_json` and
  re-encodes in the same byte order; the bytes must equal the CDR-only leg's
  (both encoders and the mapping are ours, so byte equality is legitimate here
  — and not redundant with value equality: `-0.0 == 0.0` yet encodes
  differently). Six `json/*` error classes fail `contract-check`;
  `json/unmapped` (advice) names the types the mapping documents as not
  crossing (`fixed`, `Principal`, `void`), pinned over golden to gc21's
  `Amount`/`Invoice`. The summary line and `--json` print how many CDR round
  trips also crossed (golden 5248 of 5248; 12,928 over four corpora), and the
  harness pins the floor. First pass: **0 findings**; two negative controls
  red (2712 and 70). Closes 1b6b4c8's report that the sweep never called
  `anyjson`.

  **속성 스윕이 모든 값을 AnyJSON으로 건넜다 돌아온다.** `to_json` → 텍스트 →
  `from_json` → 같은 바이트 순서로 재인코딩, 바이트가 CDR 전용 왕복과 같아야
  한다(두 인코더와 매핑 모두 우리 것이므로 바이트 동일이 정당하고, `-0.0`은
  값은 같아도 바이트가 다르므로 값 비교와 중복이 아니다). `json/*` 오류 6종은
  실패, `json/unmapped`는 매핑이 건너지 못한다고 문서화한 타입(gc21의 `fixed`)만.
  요약 줄이 건넌 수를 찍고 하네스가 바닥을 고정(golden 5248/5248, 네 코퍼스
  12,928). 1차 통과 0건; 음성 대조 2건 적색(2712, 70).

- **The CLI's dry run takes values, and the static guard's dry run resolves
  the session's handles.** `orbweaver-mcp-server --dry-run=<id>.<operation>
  [--dry-run-args <json>]` asks about one operation with values: the document
  carries `payload`/`payload_why`/`raises` and `would: marshal` past the gate;
  surveys unchanged (`allow=10 need_scope=1 stray=0`). `Guarded::dry_run_with`
  resolves declared handles against the session's table, shared with the
  bridge (`Bridge::handles()` returns `RefMut`; `Bridge` is `!Send`), a forged
  handle predicts `would_not_marshal` naming it, nothing dials. Mapping errors
  inside an argument now name the parameter. `tests/ifr_reaches_the_agent.rs`
  witnesses non-empty sequences and asserts it — the third empty recursive
  witness, closed; nothing went red under it.

  **CLI 드라이런이 값을 받고, 정적 가드의 드라이런이 세션 핸들을 해석한다.**
  `--dry-run=<id>.<operation> [--dry-run-args <json>]`은 연산 하나를 값과
  함께 묻는다 — `payload`/`payload_why`/`raises`가 실리고 게이트 통과 후 `would:
  marshal`; 서베이는 그대로. `Guarded::dry_run_with`는 선언된 핸들을 브리지와
  공유하는 세션 테이블에 대해 해석하고(`Bridge`는 `!Send`), 위조 핸들은 이름을
  붙여 `would_not_marshal`, 다이얼 없음. IFR 증인이 비어 있지 않은 시퀀스를
  만들고 단언한다 — 세 번째 빈 재귀 증인 닫힘.
- **The content seat sees a static call's payload** (D010 A3, 2026-08-19).
  `Guarded` reads a stub's own bytes back through the contract and hands the
  chain the same AnyJSON document the dynamic path hands it — no stub, trait
  or emitted file changed, so stubs already compiled are covered; the
  three-crate `Invoker::invoke` change D010 named was not needed. A payload the
  guard cannot read is refused `MARSHAL`/`BAD_OPERATION` after the gate and
  before the wire, never forwarded. The leak test now has a static arm,
  red-then-green in the same commit. **A dry run can take values**:
  `Bridge::dry_run_with` / `Guarded::dry_run_with` hand them to the content
  seat and predict marshalling from the contract's `TypeCode`s, both byte
  orders, into a dropped buffer (`Would::Marshal`, `payload`, `raises`); a
  `string<8>` given nine characters predicts `marshal` where it predicted
  `allow`. Nothing dials. Pinned rather than fixed: a stub's over-bound
  argument is refused by the stub's probe before the guard hears of it
  (nothing sent, nothing audited).

  **정적 호출의 페이로드가 내용 좌석에 도달한다** (D010 A3). 가드가 스텁이 쓴
  바이트를 계약에 따라 되읽어 동적 경로와 같은 AnyJSON 문서를 체인에 건넨다 —
  스텁·트레이트·생성 파일은 바뀌지 않았으므로 이미 컴파일된 스텁도 적용되며,
  D010이 이름 붙인 세 크레이트 변경은 필요 없었다. 읽을 수 없는 페이로드는
  게이트 뒤·와이어 앞에서 `MARSHAL`/`BAD_OPERATION`으로 거절되고 전달되지
  않는다. 누출 시험에 정적 경로 팔이 같은 커밋에서 red→green으로 추가되었다.
  **드라이런이 값을 받는다**: 내용 좌석에 값을 건네고 같은 `TypeCode`로 양쪽
  바이트 순서 마샬링을 예측한다(`Would::Marshal`); `string<8>`에 아홉 글자는
  `allow`였던 자리에서 `marshal`을 예측한다. 아무것도 다이얼하지 않는다.
  고치지 않고 못 박은 것: 스텁의 상한 초과 인자는 가드가 듣기 전에 스텁의
  프로브가 거절한다.
- **Fallback-on-failure oracle for `LOCATION_FORWARD` vs
  `LOCATION_FORWARD_PERM`** (`spikes/perm_fallback.sh`,
  `crates/orbweaver-gen/tests/forward_fallback.rs`): two servers, the
  forwarded-to one killed by PID. omniORB 4.3.4 restarts at the original after
  a temporary forward (§9.6 "shall") and stays on the dead address after a
  permanent one (§9.6 "may replace") — the first peer measurement that tells
  the two statuses apart, and through which a server that downgrades status 4
  to 3 goes red. Ours measured and pinned: `Connection` returns Err under both
  (holds no original address); `Reference` re-asks the original on every call
  under both (never caches a forward, never replaces on permanent).
  `spike-server` gains `ORBWEAVER_FORWARD_TO/_STATUS/_PING_ANSWER`.

  **`LOCATION_FORWARD`와 `LOCATION_FORWARD_PERM`을 구별하는 장애-시-복귀
  오라클**(`spikes/perm_fallback.sh`, `tests/forward_fallback.rs`): 서버 두 대,
  전달받은 쪽을 PID로 종료. omniORB 4.3.4는 임시 전달 뒤에는 원래 주소로
  되돌아가고(§9.6 "shall") 영구 전달 뒤에는 죽은 주소에 머문다(§9.6 "may
  replace") — 두 상태를 구별하는 최초의 피어 측정이며, 상태 4를 3으로 낮추는
  서버가 이것으로 빨갛게 된다. 우리 쪽: `Connection`은 두 상태 모두 Err(원래
  주소를 갖고 있지 않음); `Reference`는 두 상태 모두 매 호출마다 원래 주소에
  다시 묻는다.
- **The pool says which forward it followed.** `mux::Sent::Forward` carries
  `Forward`, not a bare `Ior`; `Pool::invoke_tracking` returns the reply and
  the last hop followed; `Reference::forwarded()` is the pooled
  `Connection::forwarded()`. Following is unchanged. Measured: 2 kinds × 3
  versions × 2 reply byte orders against a scripted peer, plus the same matrix
  through our own `Server` (native order) — permanent reported only for 1.2 ×
  permanent. Negative controls: interpret forced all-temporary and
  all-permanent, `Reference` recording nothing — each red once.

  **풀이 어떤 포워드를 따랐는지 말한다.** `mux::Sent::Forward`가 `Ior` 대신
  `Forward`를 나른다. `Pool::invoke_tracking`은 응답과 마지막으로 따라간 홉을
  돌려주고, `Reference::forwarded()`는 `Connection::forwarded()`의 풀 판이다.
  따라가는 동작은 그대로다. 측정: 스크립트 피어로 2종 × 3버전 × 2응답 바이트
  순서, 같은 행렬을 우리 `Server`로(네이티브 순서) — 1.2 × permanent에서만
  permanent로 보고. 음성 대조: 전부 temporary/전부 permanent 강제, `Reference`
  미기록 — 각각 한 번 빨강.
- **`LOCATION_FORWARD_PERM` is reachable from a servant** (D010 A1).
  `Forward { Temporary, Permanent }` and a defaulted `redirect` on
  `Dispatch`/`SharedDispatch` (giop) and on every generated servant trait
  (gen); `Served::Forward` carries it and the server puts status 4 on the wire
  to a 1.2 peer, 3 to a 1.0/1.1 peer whose status enumeration has no 4.
  `Connection::forwarded()` reports which a client followed. Measured: raw
  status off the wire from a generated skeleton, both byte orders, all three
  versions; omniORB 4.3.4 following our status 4. Request count at the old
  reference is **1 under both statuses** for our client and for omniORB — the
  count is not an oracle, the status byte is. `Servants` delegates `redirect`
  explicitly; the trait default was silent through it. D010 A1's review
  correction was itself wrong (`rt::Dispatch` is giop's trait); the batch was
  giop + gen, and D010 §3 now says so.

  **`LOCATION_FORWARD_PERM`을 서번트가 말할 수 있다** (D010 A1). `Forward {
  Temporary, Permanent }`와 기본 구현이 있는 `redirect`가 giop의
  `Dispatch`/`SharedDispatch`와 gen의 모든 생성 서번트 트레이트에 추가됐다.
  서버는 1.2 피어에게 상태 4를, 상태 열거형에 4가 없는 1.0/1.1 피어에게는 3을
  보낸다. `Connection::forwarded()`로 클라이언트가 무엇을 따랐는지 알 수 있다.
  측정: 생성 스켈레톤에서 와이어로 읽은 원시 상태값(양쪽 바이트 순서, 세 버전),
  omniORB 4.3.4가 우리의 상태 4를 따름. 옛 참조에 도달한 요청 수는 우리
  클라이언트와 omniORB 모두 **두 상태에서 1로 같다** — 횟수는 오라클이 아니고
  상태값이 오라클이다. D010 A1의 검토 정정 자체가 틀렸고(`rt::Dispatch`는
  giop의 트레이트), 배치는 giop + gen이었다.

- **moe v1.1 — the Capability gap closed additively (D010 A2).**
  `corpus/golden/22` gains `MeasuredCapability` (composes the released
  `Capability` + `specialization` + `latency_p50_ms`) and
  `ExpertRegistry::register_measured` / `heartbeat_measured`; `idl-diff` exit 0
  against the frozen release `corpus/evolution/moe/v1.0`, exit 1 for the
  in-place edit kept at `corpus/evolution/moe/v1.1-in-place` — both in the
  harness. `orbweaver-trading`: `Selection::unranked` + `is_complete()` — an
  offer with no value for the `ORDER BY` field is set aside and named, no
  longer sorted last (which still picked an unmeasured expert when nothing was
  measured). `orbweaver-object`: both operations served, both byte orders; a
  v1.0 `heartbeat` keeps the two members it cannot mention (it used to erase
  them, and an out-of-band `declare_specialization` with them — same root
  cause: a message with no room for a fact cannot withdraw it). `spike-experts`
  windows 4–5: refused → refused with the unmeasured one named → complete
  answer by measurement. The generated coverage block moved 12→14 declared on
  the control plane by itself, which is what it is for.

  **moe v1.1 — Capability 간극을 추가만으로 닫음 (D010 A2).** golden 22에
  `MeasuredCapability`와 `register_measured`/`heartbeat_measured` 추가; 동결
  릴리스 `corpus/evolution/moe/v1.0` 대비 `idl-diff` exit 0, 제자리 수정
  대조군 exit 1 — 둘 다 하네스에. trading: `Selection::unranked`·
  `is_complete()` — 순위를 매길 수 없는 오퍼는 마지막이 아니라 따로 명명.
  object: 두 오퍼레이션 양쪽 바이트 순서로 서비스, v1.0 heartbeat는 언급할 수
  없는 두 멤버를 지우지 않음. spike-experts 4·5번 창: 거부 → 부분 측정 거부 →
  측정에 의한 완전한 답.

- **`_rt.py` reads and writes AnyJSON v1.1's structural `_t`** (D010 A4). An
  `any` carrying a struct, enum, union, exception or typedef crosses to Python
  and back; a type the package never declared is synthesised from the document
  and registered, so no prior copy is needed at the reader — the property D008
  chose the structural form for, now true of both implementations. Generated
  modules carry each type's IDL name (`_idl_name`, `register_alias(id, desc,
  name)`, `register_name`) because a rebuilt TypeCode names the type beside
  its id. Round-trip sweep: golden 78/132 → **158/132**, services 35/46 →
  **70/46**, 0 divergences; the new cases are red against the old runtime with
  the D008 refusal (85 + 35 divergences), and the harness now pins the counts.
  Found on the way: `anyjson` (Rust) does not resolve `TypeCode::Recursive`,
  so a non-empty value under a recursion marker cannot cross the reference
  mapping — reported, not fixed here.

  **`_rt.py`가 AnyJSON v1.1의 구조적 `_t`를 읽고 쓴다** (D010 A4).
  struct·enum·union·exception·typedef를 실은 `any`가 Python으로 건너갔다
  돌아온다. 패키지가 선언한 적 없는 타입은 문서에서 합성해 등록하므로 읽는
  쪽에 사본이 미리 있을 필요가 없다 — D008이 구조적 형식을 택한 이유가 이제
  두 구현 모두에서 참이다. 왕복 스윕: golden 78/132 → **158/132**, services
  35/46 → **70/46**, 불일치 0; 새 케이스는 이전 런타임에 대해 D008 거부로
  빨간불(85 + 35). 도중 발견: Rust `anyjson`이 `TypeCode::Recursive`를 풀지
  않아 재귀 마커 아래의 비어 있지 않은 값은 기준 매핑을 건너지 못한다 — 보고만
  하고 여기서 고치지 않음.
- **`docs/SERVICES-COVERAGE.md` §8 is generated from the sweep and diffed by
  the harness** (D010 A5, batch 1). `spikes/coverage_tables.py` renders
  `service_sweep.sh --raw` into per-service tables, totals, the interfaces no
  object claimed, and — new — the interfaces the IDL declares that the sweep
  **probed against no object** (`BindingIterator`, 21 of `ir.idl`'s), which
  used to be silent and read as coverage. `--check` fails the harness with the
  diff when the wire and the file disagree; the fix is to regenerate, never to
  edit the block. §3–§7 are re-framed as the dated first reading kept for the
  reasons quoted there, and their headings lose the counts they carried by
  hand: every one was stale, and D010 A5's own sentence about "§2's naming row
  says 13 of 16" named a number that occurs in no document — the class it was
  describing, in the description. Negative control: one served count edited in
  the block → `FAIL … no longer says what the wire says` with the one-line
  diff; regenerated → ok.
- **`spikes/gap_symbols.py`** — D010 §7.1's report, deliberately not a gate:
  per `COMPONENTS.md` gap row, the symbols it names and whether each exists in
  its crate. Measured today: 12 of 12 exist, all legitimately — which is the
  number that demoted it from a gate, printed at the bottom of every run.

  **`SERVICES-COVERAGE.md` §8을 스윕이 생성하고 하네스가 diff한다.** 선언되었으나
  어떤 객체에도 프로브하지 않은 인터페이스가 처음으로 드러난다(전에는 침묵이
  커버리지로 읽혔다). §3–§7의 손으로 적은 수치는 모두 낡아 있었고 제목에서
  뺐다. `gap_symbols.py`는 게이트가 아닌 리포트 — 오늘 12/12가 존재하며 그
  숫자가 게이트에서 강등한 이유다.

### Fixed / 수정

- **The live dynamic call's marshalling error names the argument and the path
  inside it** (`at key.tag[2]: string is bounded at 8 but 9 were given`), on
  the marshaller's own `Path`; `encode_named`/`decode_named` (+`_with`) added
  to `orbweaver-dynamic`, and the mcp dry run uses the same call so a
  prediction and a refusal read alike (it used to prepend the name itself,
  joined with a dot the live path never wrote). Reported by f47ddcd.
- **A union branch that is both labelled and `default:` keeps its labels** —
  in the Python emitter, the Python runtime (form reader and writer) **and the
  Rust emitter, which had emitted the variant twice and did not compile**;
  `corpus/golden/29-labelled-default.idl`; Python sweep 158/132 → **170/137**,
  0 divergences, pins moved. Reported by 50a4d12. **Found on the way, not
  fixed here (a fix batch is in flight):** a union TypeCode with a bare
  `default:` encodes zero label bytes while the decoder reads discriminator
  width — our own encode→decode of golden 06's `WithDefault` TypeCode fails,
  never red because every comparison ran both sides through the same encoder;
  and the registry collapses `case 1: default:` to one member where omniidl
  produces two.

  **동적 호출의 마샬링 오류가 인자 이름과 내부 경로를 명시한다**(`at
  key.tag[2]: …`), 마샬러의 `Path` 그대로; `encode_named`/`decode_named` 추가,
  mcp 드라이런도 같은 호출을 써서 예측과 거절이 같은 문장을 쓴다. **라벨과
  `default:`를 동시에 가진 유니온 분기가 라벨을 유지한다** — Python 이미터·
  런타임·**Rust 이미터**(변형을 두 번 내보내 컴파일되지 않았다) 모두;
  `corpus/golden/29`; Python 스윕 158/132 → **170/137**. 도중 발견(수정 배치
  진행 중): 맨 `default:` 케이스의 유니온 TypeCode가 라벨 바이트를 0개
  쓴다 — 우리 자신의 encode→decode가 실패하는데 양쪽이 같은 인코더를 지나므로
  한 번도 빨갛지 않았다.

- **Two class-B rows were `note` lines the verdict did not count.** D010 §2
  says every B row lands as a `SKIPPED — unmeasured, not passing` group; B2
  (identity through a real provider) and B3 (SSLIOP against a peer) were
  prose after an `ok`. Both are counted SKIPPED groups now, each naming its
  fixture: a peer advertising a CSIv2 mechanism list plus an issuer
  (`ORBWEAVER_IDP_URL`) — and FAIL, not ok, on the day both exist and nothing
  measures them; `from omniORB import sslTP` as the probe, with a distinct
  line if the module is present somewhere and the peer proof still is not
  built. Harness SKIPPED count 3 → 5. **The first version of that probe was
  itself the gate-green-measuring-nothing class:** it grepped its own marker
  out of an `ImportError` traceback that echoes the source line, and reported
  `sslTP` present where it is not — caught on the first run by reading the
  harness's line against the shell's; the probe is the interpreter's exit code
  now.

  **B류 두 행이 판정이 세지 않는 `note`였다.** B2(실제 제공자 통한 신원)와
  B3(피어 대상 SSLIOP)는 `ok` 뒤의 문장이었다. 이제 둘 다 픽스처를 이름 붙인
  SKIPPED 그룹이며, 픽스처가 있는데 재지 않는 날에는 ok가 아니라 FAIL이다.
  하네스 SKIPPED 3 → 5.

- **AnyJSON: a value beneath a `TypeCode::Recursive` marker now crosses in
  both directions** (found by D010 A4; `to_json`/`from_json` resolved aliases
  only, so `Reading { kids: sequence<Reading> }` with a child failed "is not a
  value of"). The mapping walks on the marshaller's own `Path`, so recursion
  and the 64-level nesting bound are one mechanism for CDR and JSON; a
  document nesting past the bound is refused, not followed. **Why nothing was
  red:** every recursive witness was the empty list — the property sweep's
  `TreeSeq` was `[]` on every valued case and `None` on 22 of 32 (skipped
  through a bare `continue` while the summary line said 32), and the Python
  sweep terminated at the first re-entry. The sampler's depth predicate now
  mirrors the sampler, a valueless case is a `prop/unmeasured` finding the
  harness fails on, and the Python witness follows a marker one level (all
  five Python tests pass; `_rt.py` needed nothing). Found and not fixed: the
  property sweep never calls `anyjson`, so it could not have caught this
  regardless of witness; a third empty recursive witness lives in
  `orbweaver-mcp/tests/ifr_reaches_the_agent.rs`.

  **AnyJSON: `Recursive` 마커 아래의 값이 양방향으로 건너간다**(D010 A4 발견;
  `to_json`/`from_json`은 별칭만 풀었다). 매핑이 마샬러의 `Path` 위를 걷게 되어
  재귀와 64단계 중첩 한도가 CDR·JSON 하나의 메커니즘이 되었다. **왜 아무것도
  빨갛지 않았나:** 모든 재귀 증인이 빈 목록이었다 — 속성 스윕의 `TreeSeq`는
  값 있는 모든 케이스에서 `[]`, 32건 중 22건은 `None`(요약 줄은 32라 말하며
  조용히 건너뜀). 샘플러의 깊이 술어를 샘플러와 일치시키고, 값 없는 케이스는
  하네스가 실패시키는 `prop/unmeasured` 소견으로 남기며, Python 스윕 증인은
  마커를 한 단계 따라간다. 보고만: 속성 스윕은 `anyjson`을 호출하지 않는다;
  세 번째 빈 재귀 증인이 `ifr_reaches_the_agent.rs`에 있다.

- **README — the sixth class-D row the PLAN batch named.** The S1–S7 stage
  table lost its *Target* column (95/90/80/100/100/85/90 — now only PLAN §11
  A9); *Targets / 목표 지표* no longer copies PLAN §11 without its instrument
  column and points there instead; the Phase 0 bullet B stops carrying the
  spike threshold (≥ 60 / ≥ 95 %, home `PHASE0.md`) beside a plan that carries
  the standing target (≥ 85 / ≥ 98 %, home PLAN §11) — two facts, each now
  with one home. `docs/plan-page.html` (hand-maintained, dated v0.3) gets a
  kept-as-written card and its TAO/omniORB/JacORB line annotated historical,
  mirroring PLAN §12. Headings 18 → 18.

  **README — 계획서 배치가 지목한 클래스 D 여섯째 행.** S1–S7 단계 표의 *목표*
  열 삭제(계획서 §11 A9만 보유); *목표 지표* 절은 계측기 열 없이 §11을 베끼던
  표를 지우고 그곳을 가리킴; Phase 0 가정 B 줄은 스파이크 기준값(≥ 60 /
  ≥ 95 %, 집은 `PHASE0.md`)과 상시 목표(≥ 85 / ≥ 98 %, 집은 계획서 §11)를
  각각의 집으로 돌려보내고 어느 수치도 담지 않음. `docs/plan-page.html`
  (수작업, v0.3 날짜)에는 기록 카드와 TAO 줄 역사 주석. 헤딩 18 → 18.

- **PLAN §5, §8, §11, §12 — the five class-D rows D010 §6 named.** The §5
  *Automation target* column (seven percentages, no instrument) moved to §11
  as aspiration A9; *contract tests auto-generated* audited (nothing generates
  a test) and kept at **none** with A8; the first-pass / three-rounds rows
  restated against `forge-pipeline`'s actual per-stage `first-pass:` /
  `rounds:` / `result:` lines; the §8 CDR row no longer asks for byte-identity
  against a reference ORB — it names the decoded-value, re-encoded-outside-
  padding and derived-TypeCode comparisons that exist; §12 action 3 annotated
  as historical with A6 / D010 B6 as the trigger's home. Found and not fixed:
  `README.md` restates both target tables without instruments, and its
  assumption-B targets (≥ 60 % / ≥ 95 %) differ from PLAN's — a sixth row of
  the class, outside this batch.

  **PLAN §5·§8·§11·§12 — D010 §6이 지목한 클래스 D 다섯 행.** §5의 *자동화
  목표* 열은 §11의 지향 A9로 이동; *계약 테스트 자동 생성률*은 감사 후(테스트를
  생성하는 것이 없음) **없음**으로 유지하고 A8; 1차 통과율·3회 내 통과율 행은
  `forge-pipeline`이 실제로 찍는 단계별 줄에 맞춰 재기술; §8 CDR 행은 더 이상
  참조 ORB 대상 바이트 동일을 요구하지 않고 실재하는 비교 셋을 명시; §12
  액션 3은 역사적 기록으로 주석. `README.md`가 같은 표를 계측기 없이 다시
  적고 있음 — 같은 클래스의 여섯째 행, 이 배치 밖.

### Fixed / 수정

- **CI had been red for ten consecutive runs while the local harness was
  green, on three causes, none of them in the code.** Found by reading the
  runs before planning the next batch rather than after landing it.

  1. `spikes/union_label_capture.py` compared the recorded omniORB union
     TypeCodes to the live peer **byte for byte, exempting only offsets 9..12**
     — the padding the local peer happened to leave non-zero. CI's omniORB,
     built from source on Linux, leaves different garbage after the
     repository-id string and before every 8-aligned label (`U` at 29; `W`
     at 52, 76, 118, 119), and the script called that a change in the peer.
     Every one of those offsets is padding the specification says nothing
     about — CLAUDE.md's *compare decoded values, never raw buffers* rule,
     broken by a script written the same week the rule was cited. The mask is
     now **derived by walking the encapsulation** (§9.4.2 layout, alignment
     restarting at the encapsulation's first byte); a byte the walker does not
     understand raises rather than being treated as padding. Negative control:
     flipping a repository-id byte → `FAIL … differ at [16]`; flipping byte 29
     → `ok`.
  2. The R7 endpoint-rewriting fixture bound a **fixed port inside Linux's
     ephemeral range** (40404 ∈ 32768–60999). The harness makes a few thousand
     outbound connections first, so in two of ten runs the kernel had already
     lent that port to one of them: *"Failed to bind to address :: port
     40404. Address in use?"* Now 24404, below both kernels' ranges. Negative
     control: holding 24404 reproduces the exact CI text; releasing it, the
     fixture serves.
  3. `spikes/jacorb/setup.sh` fetched five jars with a single `curl -f`; one
     transient mirror error (exit 22) failed the whole interop job. `--retry 3`.

  **CI가 열 런 연속 빨강이었고 로컬 하네스는 초록이었다 — 원인 셋, 코드에는
  없음.** 유니온 레이블 캡처는 패딩을 오프셋 목록(9..12)으로 예외 처리했고
  CI의 omniORB는 다른 자리에 쓰레기를 남겼다 — 이제 인캡슐레이션을 걸어서
  마스크를 만든다. R7 픽스처의 고정 포트가 리눅스 임시 포트 범위 안에 있었고
  (40404), 이제 두 커널 범위 아래(24404). JacORB jar 다운로드에 재시도.

- **The §5.3 release gate accepted a struct member with no type, and refused a
  contract both oracles accept.** Two faces of one omission, found by asking
  what a marker means rather than by fixing what was reported.

  `Registry`'s resolver joined an unqualified name onto *lexical* prefixes
  only, so a `raises` declared in a base interface did not resolve from a
  derived one. `corpus/services/gen-naming-subset.idl` — `NamingContextExt :
  NamingContext`, exactly as the OMG writes it — produced five unresolved
  markers and **exit 2**, while `omniidl` and JacORB both accept the file. A
  gate that cries wolf gets bypassed, which makes that worse than the defect
  the include work had just fixed.

  CORBA 3.4 §7.19.2 fixes the order and the order *is* the rule: an
  unqualified name is searched in the current scope, then in **inherited**
  scopes, then outward through enclosing ones — so a base's declaration beats
  an enclosing module's declaration of the same name. A base's base counts, a
  diamond contributes one name, and a cycle terminates.

  Asking what `Unresolved` meant then found the other face. It recorded bases
  and `raises` and **not types**, so an unresolved type name silently became
  `TypeCode::Void`: `module n04 { struct S { Widget w; }; };` diffed as *"no
  change … nothing here breaks a deployed peer"*, **exit 0**. `void` marshals
  nothing where a peer expects a value. The marker now means one thing — *this
  translation unit declares no such name* — and both cases exit 2.

  The negative control is the point of the design: a **sibling** interface
  using another's `typedef` must still fail, because a resolver that "fixed"
  inheritance by searching every interface in the unit passes every positive
  case and only fails that one. Both oracles reject it too.

  Nothing had ever run `idl-diff` over the corpus, which is why a gate could
  refuse a valid contract unnoticed. The harness does now, with both controls.

  §5.3 게이트가 **타입 없는 구조체 멤버를 통과시키고**, 두 오라클이 받아들이는
  계약을 거부하고 있었다. 하나의 누락이 가진 두 얼굴이다 — 보고된 것을 고치는
  대신 *마커가 무엇을 뜻하는가*를 물어서 나왔다. 음성 대조군이 설계의 핵심이다:
  **형제** 인터페이스의 스코프는 여전히 새면 안 된다.

### Added / 추가

- **`idl-diff` resolves what it is asked to diff.** The §5.3 release gate was
  given two revisions whose root file is byte-identical and whose two breaking
  changes both live in the header they share, and it printed *"no change"* and
  exited **0**. One call — `orbweaver_idl::parse`, the string entry point —
  across **19 sites**, of which 12 used `parse` (silent) and 6 used `check`
  (loud about the include and equally unable to resolve it). All are now
  decided per site, with the ones that correctly stay on a raw parse carrying
  their reason. An unresolvable include is **exit 2**, never a verdict: a diff
  of two partial graphs says nothing about the contracts.

  `orbweaver-mcp-server` turns out not to have served an estate at all — it
  refused to start, and `spikes/estate/run.sh`'s amalgamation step was a
  load-bearing workaround that read as a convenience.

  Measured cost of the old silence: stripping `#include` from the thirteen-file
  estate drops **27 references without a word** — 8 base interfaces and 19
  raised exceptions.

- **The estate goes in as the thirteen files it is stored as.** `run.sh`
  amalgamated them first, and that step turned out to be **load-bearing rather
  than a convenience**: without it `orbweaver-mcp-server` did not serve less, it
  refused to start. A direct stage now runs beside the amalgamated one and the
  two are measured to agree — 12 interfaces, 76 operations, identical interface
  by interface (and, as it happens, identical as whole documents, recorded but
  not asserted, because the two inputs genuinely differ).

  `amalgamate.py`'s docstring had rotted in place: it said the front end
  *skips* `#include` and cited a line of `lex.rs` that today reads *"it used to
  be in that list, and being in it was a defect… It is resolved before the
  lexer runs now."* The script stays, for a reason that is still true — some
  consumers take a translation unit rather than a path — and stage 7b measures
  the equivalence instead of assuming it.

  Two more things were found to rest on the amalgamation without saying so:
  `forge-pipeline`'s S4 supplies its item as **text**, which has no directory
  for a quoted `#include` to resolve against, so pointed at the thirteen files
  it exits 1 — the same class as the nineteen call sites, reported and not
  fixed here; and `gen-corpus`'s output compiles against a hand-written servant
  written for the single amalgamated module, which is a genuine dependency and
  now stated.

- **DynAny**, over `Value`/`TypeCode`: navigation whose cursor is a path
  re-resolved at every operation, so nothing exists below the focus and
  past-the-end is representable but never readable. 76 of 78 golden types are
  taken apart and reassembled into identical CDR, both byte orders, all eight
  alignment phases; the two it cannot are `fixed`, deferred by §4.4, and the
  test asserts that list is exactly those two.

  Its first oracle was worthless and only mutation showed it: the source value
  was generated with DynAny too, so breaking `next()` to skip every second
  component **still passed**. A producer and a consumer sharing a defect agree
  about the result.

- **`agent-fuzz`**, for the parsers a `tools/call` reaches. AnyJSON v1.1 put a
  recursive parser on the agent boundary and nobody had fuzzed it, including
  whoever wrote it. Seven targets, zero panics over 50k/50k/200k at three
  seeds; the one finding was the array reservation above.

- **CSIv2 fuzz targets, and the reach that made them worth having.** Two of the
  three were reached **zero times in 50,000 cases** before seeding — the
  green-and-worthless case, stated. Twenty seeds took them to 659 / 1930 /
  2811, and the dilution that cost the GIOP half is reported rather than
  hidden. Twenty-five hostile literals run on every invocation whatever
  `--cases` says, because a class the shipping build cannot see should not also
  depend on a random draw.

- **`call-bench`**, the LAN echo benchmark §8 has cited since v0.2 and did not
  have. One loopback connection shared by both clients, calls interleaved with
  the order swapped, every sample checked against the expected answer. The
  dynamic path costs **+2.0 µs p50 on a 64-string payload (1.06×)** — about
  31 ns per string, and per *string* rather than per byte.

- **A generated skeleton is compared to the hand-written servant it must
  match**, byte for byte: 59 scripted steps × 2 byte orders over CosNaming,
  every structured reply decoded back by `orbweaver-giop`'s own readers,
  because two servants can agree on wrong bytes. It is what forced D009's L2
  early — the naming server began publishing `TAG_CODE_SETS` and the generated
  reference did not.

- **`#include` inside a module**, which no corpus file had. Eight new roots
  produced 32 repository ids, **7 of which diverged from omniidl — and JacORB
  agreed with omniidl against us on all seven.** The resolver had implemented a
  file boundary as an injected `#pragma prefix`, and prefix *replaces* the id
  path, so it could express neither half of a save/restore once the path held a
  module.

- **A decision's status is checked, not just written.**
  `spikes/decision_status.py` reads the authoritative status out of each
  `docs/decisions/D00N-*.md` and holds every other mention to it, in the
  harness. Dated records — `pipeline-runs/`, `PHASE*.md`, released sections of
  this file — are out of scope by construction: they state what was true on a
  date, and editing them to match today would falsify them rather than repair
  them. The gate also refuses a citation to a decision that does not exist.

  결정의 상태는 `docs/decisions/`에 한 번만 산다. 나머지 언급은 하네스가 그것과
  대조한다. 날짜가 붙은 기록(실행 기록·PHASE·릴리즈된 절)은 그 시점의 사실이므로
  범위 밖이다 — 오늘 기준으로 고치는 것은 수리가 아니라 위조다.

### Fixed / 수정

- **Ten stale status claims, and the four remaining-work lists no gate can
  see.** Four decisions the user approved on 2026-08-14 — D003, D004, D005,
  D006 — were still being called open in five documents. `PLAN.md` §7.2 still
  listed three Phase 4 items as remaining — a servant that cannot raise a system exception,
  a missing server-side static-equals-dynamic oracle, and no Python target —
  when all three had landed, and stream E carried an un-struck second copy of
  its own last three items through three passes. Nothing here was a code
  defect and nothing could go red: the measured cost was **a planning pass
  spent proposing work that was already finished.** §7.2 no longer restates a
  remaining-work list at all; it links the `COMPONENTS.md` row that owns one.

  다섯 문서가 이미 승인된 결정 넷을 PROPOSED로 불렀고, §7.2는 이미 착지한 세
  항목을 남은 일로 열거하고 있었다. 코드 결함은 하나도 없고 빨개질 수 있는
  것도 없었다 — 실측된 비용은 **이미 끝난 일을 다음 작업으로 제안한 계획 한
  번**이다.

- **`D003` said APPROVED in English and 제안 in Korean, four lines apart.** The
  approval edit overwrote the head of its own PROPOSED block and left the tail,
  in the file that is the source of truth — and every document that copied it
  copied the English half. The gate now requires a decision's status markers to
  agree with each other before it checks anyone against them.

- **The gate's own two blind spots, found by running it.** It read documents
  line by line, so a claim whose reference and status word fell either side of
  a markdown wrap was invisible — `PLAN-MOE.md` passed while saying the wrong
  thing, and only its Korean twin was caught, because that one happened not to
  wrap. It also attached every status word to every decision named in the same
  sentence, which reported two correct sentences as drift. Passages are now
  built per paragraph, and a status word binds to the decision most recently
  named before it. First run 6 findings, after both repairs 10, false positives
  0. It then caught the first draft of this very entry, which quoted the old
  status without naming the new one — the entry was reworded rather than the
  gate loosened.

---

## v0.4.0 — 2026-08-14

The release a **consumer-shaped input** produced. Thirteen legacy contracts
that include each other, four `#pragma prefix` styles, an acquired company's
prefix inheriting our base, one file with no prefix at all, and nothing
annotated anywhere — the shape an estate actually has, rather than the shape
`corpus/` has. It found eight root causes in one pass, **six of which no test
in this repository could ever have gone red on**, because every corpus file is
self-contained and every corpus file is annotated.

Two of them change what a deployment does. They lead.

이번 릴리즈는 **소비자 모양의 입력**이 만들었다. 서로 include 하는 13개의 레거시
계약, prefix 스타일 넷, 주석은 어디에도 없음. 근본원인 8건이 한 번에 나왔고 그중
**6건은 이 저장소의 어떤 테스트도 빨갛게 될 수 없던 것**이다. 코퍼스 파일은 전부
self-contained이고 전부 주석이 달려 있기 때문이다.

### ⚠ Behaviour changed / 동작 변경

- **An unannotated operation is now refused, where it used to be allowed.**
  The bridge asked `annotations.get("ai_effect")?`, so a *misspelled* effect
  required approval and a *missing* one did not. Expose the estate's twelve
  interfaces with no scopes and the answer was **`allow=76, refuse=0`** —
  `SHUTDOWN`, `purge`, `void_invoice` and a writable attribute's setter among
  them. `COMPONENTS.md` recorded this hole for *ingested* contracts; it was a
  property of **any** unannotated one, which is what every legacy estate is.
  And S4 advised fifty-two times over that estate that the bridge *"must assume
  it needs approval"* — measured false. One part of the system stated the safe
  rule while the mechanism did the opposite.

  Closed means **refused**, with its own `Denied::EffectUnstated`. Not
  approval-required: an approval is a human saying yes to a *described* call,
  nobody can say yes to a call whose effect nobody stated, and one `--approve`
  would otherwise unlock a whole estate at once. Not not-exposed: the operator
  did expose it, and saying otherwise sends them into the allowlist to debug a
  contract problem.

  **Upgrading:** a contract that annotates its operations is unaffected
  (`end_to_end.sh` still reads `allow=2 need_scope=1`, unmoved). A contract
  that does not needs either `//@ ai_effect:` on its operations or the
  exposure's new **`--assume-effect <value>`** — the operator's single
  declaration of what that estate's silence means, run through the same
  recognition a contract's own value gets. Not seventy-six approvals. Every
  allow resting on it says so: `assumed: true`, `effect_stated_by: "exposure"`,
  and a startup line counts the silence.

  Absent and unrecognised are now deliberately different, in the direction that
  costs something: a typo reaches a human, because somebody was annotating and
  got one wrong; a silence goes back to the contract, because there is nothing
  there to say yes to.
  *주석 없는 오퍼레이션은 이제 거부된다. 침묵은 허가가 아니다.*

- **A `CloseConnection` between fragments is neither corruption nor a clean
  retry.** Both existing answers were false statements about the stream.
  `Desynchronized` says the peer is broken and the connection is corrupt, about
  a well-framed orderly goodbye — the client gives up on a service that was
  merely restarting. But `ConnectionClosed` hands the caller §13.5.1's promise
  that outstanding messages *were not processed and may be safely resent*, and
  that promise is about messages **without replies**. A peer that had already
  sent the leading piece of a reply demonstrably processed that request;
  re-sending a non-idempotent operation on that promise runs it twice, silently,
  on a pooled connection.

  So the retry fact is **per caller, not per connection**.
  `Error::InterruptedMidReassembly` carries what arrived and what was
  outstanding, `Fault::unsent` is false for the cut call and true for every
  other waiter, and `Error::is_orderly_close()` decides teardown against
  corruption on a value rather than on a message string. At GIOP 1.1
  `FragmentUnsupported` wins and the close is left *unread* — 1.1's refusal is
  permanent for that peer and a close is retryable, so preferring the close
  sends the pool round to hear the same thing again. A `MessageError` mid-chain
  is a report rather than corruption, but §9.4.8 gives it no body, so it names
  nothing and makes **no** request re-sendable. The serving loop stopped
  answering a goodbye with a `MessageError`, and stopped answering a
  `MessageError` with a `MessageError`, which was a two-ORB loop.

  **Unmeasured, and named as such:** the shape needs a peer to shut down
  between two writes of one reply, and neither fixture exposes that window.
  What *is* measured is the regression — omniORB 4.3.4's real 1.2 fragments
  still reassemble.

- **`describe_interface` reports the resolved surface.** Ten of twelve estate
  interfaces inherit; the agent was shown 11 operations, the guard judged 13,
  the servant served 13. Nothing was allowed that policy forbade — but the
  description an agent plans against was not the surface it could reach, and a
  reviewer reading it did not know the rest was there. Three walks became one,
  public, so nothing has an excuse to write a fifth.

### Added / 추가

- **`#include` resolves.** Eleven of thirteen estate files failed our own S4
  while `omniidl` accepted all thirteen, and the ~90 diagnostics were one
  thing: a name declared in an included file reported as undeclared. Quoted
  includes resolve against the including file's own directory, angled against
  `-I`, and the working directory is **never** searched — a validator run from
  a build directory would otherwise resolve differently from the same validator
  run from the source tree, and the difference would surface as a repository id
  rather than as an error. Once-only by canonical path, so **guards are not
  required** (the estate measured that real IDL in the wild has none), with
  non-blocking advice when an unguarded file repeats, because a deployed
  compiler rejects what we accept and being quietly laxer is worse than saying
  so. Cycles name the loop; a missing file lists every path searched; a
  diagnostic reports the included file's own line with the chain.

  The prefix rule across a file boundary was **measured, not reasoned** — two
  probes through `omniidl -Wbinline` say the includer's prefix does not enter
  and the included file's does not escape. Thirteen roots, 49 ids, all agreeing
  with `omniidl`. Full C preprocessing stays out and is **refused rather than
  skipped**: skipping `#if` compiles every arm at once, which is a silent
  misparse.

  `sidl-validate` takes `-I` and resolves before validating, which moves the
  estate's per-file acceptance from **2/13 to 13/13**. One of the original two
  passed *because* its include was silently skipped — the defect wearing a pass.
  *`#include`가 해석된다. 코퍼스 파일이 전부 self-contained이라 여섯 페이즈 동안
  아무 테스트도 빨갛게 되지 않았다.*

- **A Python client target.** The point of a second target language is not the
  language — it is that a second target is the only thing separating the IDL
  mapping from what happened to be convenient in Rust. It paid immediately: the
  **Rust** emitter's keyword list was missing Rust's own reserved words, so a
  contract naming `yield` emitted `fn yield()` and did not compile. No
  emitter's escaping had ever been *executed*, because no corpus file named a
  target language's reserved word until `28-target-keywords.idl` did.

  The seam is a **local process, not FFI**: generated Python renders arguments
  as AnyJSON v1 and hands them to `orbweaver-py-bridge`. No new dependency, no
  second wire implementation, `cargo tree` unchanged. **D007 is PROPOSED**, not
  adopted, and compares it against a PyO3 extension module (a new dependency
  class *and* an `unsafe_code` question at the boundary) and pure-Python
  CDR/GIOP. Mapping implemented against the OMG specification and *compared*
  against `omniidl -bpython` — 129 scope names and 100 operation names agree,
  and no name exists on our side that `omniidl` does not also produce.
  Comparing is not deriving.

  Measured: 28 golden contracts and 12 services generate, import and execute —
  73 values and 100 calls round-tripped, both byte orders, 0 divergences; a
  generated Python client completed 12 real calls against the omniORB fixture.

- **D005 option B — a regeneration is diffed against what is registered.**
  "Registered" is a directory the run is pointed at, which is what a previous
  run's output already is. The S5 catalog was rejected because `register()`
  builds it *after* S4 out of the artifacts S4 just gated, so an item would be
  diffed against itself. An absent contract is **counted and printed**, never
  silence; a registered-but-unreadable one is an error.

  What B cannot see is measured rather than asserted: a regeneration keeping
  every identifier and changing only `//@ ai_authz` is compatible by §5.3 and
  passes, with option C refusing it in the same test. That is why C landed
  first, and why the harness runs both.

- **Declared bounds are enforced by generated code**, at the same refusal point
  the dynamic path uses, and the §8 oracle gained the reading that would have
  caught the gap: for a value that violates its contract, the two paths must
  write the same bytes and reach the same verdict. Byte equality over valid
  values never could — which is how the divergence survived until D006 measured
  it while arguing about something else.
- **D005 option C**: a scope-shaped token the requirement states verbatim must
  survive to the `//@ ai_authz` S3 emits, checked by string equality with no
  model. Finding recorded rather than smoothed: **none of the twenty frozen
  requirements states a scope literally**, so the benchmark cannot exercise the
  rule at all — and of six naturally-phrased new ones, **three do not trip it
  either**. Korean prose, `ROLE_REGISTRAR` and `warehouse/robot/estop` are all
  ordinary ways to state a permission and all invisible to the rule, so a
  project with any of those house styles gets no binding and gets it silently.
  Not patched: accepting `ROLE_REGISTRAR` accepts every upper-case constant.
- **D005 and D006 approved.** `Expert::process` and `Router::dispatch` are
  excluded; the bound is their return path via a new versioned interface.
- **The content interceptor seat can read the arguments.** The chain was *not*
  moved after mapping, which is the fix people reach for: a chain there answers
  a mapping error before a policy refusal, turning failures into an oracle for
  the shape of operations the caller may not call. `CallContext` carries the
  agent's arguments unmapped instead. The static path supplies `None`, which is
  §4.7's bypass wearing a safety label and is now in the module docs rather
  than left to be discovered.
- **IF2's two stores are joinable** (`CallStats::merge`) and deliberately not
  joined: a static call is evidence a path *already has* a stub, so folding it
  in would have the promotion policy recommend promoting it again.

### Fixed / 수정

- **A gate found only what it could not load.** `Workspace::load` resolves
  `<id>.sidl.idl` **or** `<id>.idl` — a documented fallback for a contract no
  annotation stage ever saw — but discovery globbed only the annotated suffix,
  so the fallback was unreachable and `--only s4` over a directory of legacy
  `.idl` found nothing. The estate's workaround was to rename, which then made
  the run's attribution line report a contract with **no annotations in it** as
  annotated, because that line asked the filename; and because `gen-corpus`
  derives its module name from the file stem, the rename put a pipeline-stage
  suffix into a generated module name (`f_ESTATE_sidl`). Three symptoms, one
  cause — a name treated as evidence about content. The third needed no fix of
  its own.
- **The fix hint mangled qualified names**, printing `qualify it with
  ``Module::::```. The cause was in the *parser*, not the printer: `scoped_name`
  gave a scoped name the span of its **first token**, so
  `::MFS::Common::StringList` sliced to `"::"`. Three sites had it; the estate
  saw one. Splitting `unknown-scoped-name` off `unknown-name` makes the advice
  right as well as the text — "qualify it with its module" is meaningless for a
  name that is already qualified.
- **`validate(&str)` was the corpus's self-contained shape written into an
  API.** Handing it a resolved unit's *text* refuses it: thirteen files' guard
  directives are still in there, and four `#ifndef` blocks in one string is
  conditional compilation rather than an include guard. `validate_unit` takes
  the unit.
- **A file-scope `#pragma prefix` leaks across a splice.** A prefix runs to the
  end of its *file*; after concatenation there is one file, so a deliberately
  prefix-less contract inherited the previous one's prefix and five repository
  ids came out **well-formed and wrong**. Nothing errors on our side — only a
  peer disagrees, and then `_is_a` fails and an `--expose` allowlist names an
  interface no object has, which reads exactly like a permissions mistake. The
  rule is one line, *each file begins with the empty prefix*, and it was
  written nowhere because until a set existed there was nowhere to write it.
- **`spikes/echo.idl` states an effect on every operation.** They are all reads
  — the interface has no state — but "obviously a read" is not something a
  bridge can see. The harness caught it; the fixture was written when silence
  was an allow.

### Known limits / 알려진 한계

- **An agent cannot read an Interface Repository through AnyJSON.** §4.5 has no
  form for `::CORBA::TypeCode`, so `corpus/services/ir_subset` loses ten items
  including `describe_interface` — and the MCP agent path speaks the same
  mapping. Recorded, not fixed: §4.5 is a specification decision, not a Python
  problem.
- **The dry-run survey is correct and hard to read.** Closing the effect gate
  took it from 7,253 bytes of uniform `allow` to 31,957 bytes with real signal,
  and the growth is *entirely* 64 verbatim copies of one `why` sentence. One
  reason per **class** rather than per row would put it near 8 KB with strictly
  more signal. A report shape, not a gate.
- **`--assume-effect` is a real widening and is meant to be.** It is one
  operator declaration covering every silence in an exposure. That is the point
  — the alternative is seventy-six approvals and a human who learns to click
  through them — but it is written into every allow that rests on it so an
  audit can find them.
- The Python target is **clients only**. A Python servant needs the bridge to
  call *back* into Python, which is a second protocol direction.
- Cross-front-end portability of the include semantics is **unmeasured** — TAO
  and JacORB were absent. An `#include` inside a module whose file sets a
  prefix is exercised by no file we have. Hard links to the same content are
  two files to the resolver.

---

## v0.3.0 — 2026-08-14

The wire-behaviour section leads again, for the same reason. Everything in it
was found by a reader we did not write, or by composing two parts that had only
ever been measured apart.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **`::CORBA::TypeCode` loaded as `void`.** The front end predeclares the name
  for checking, the registry resolves against the spec's own definitions where
  it is absent, and it fell through to `void` — silently, for months. An
  operation returning a TypeCode generated `-> ()` and **marshalled nothing at
  a peer expecting one**. `CLAUDE.md` requires that spelling, which is what
  made the gap read as support. Found by the generated-servant batch, which
  could not express `describe_interface` and said so rather than emitting an
  empty reply.
- **A refusal claimed the call had completed.** `Guarded` raised system
  exceptions with a literal `0` — `COMPLETED_YES` — so every refused caller was
  told its call had run. A separate path from the transposed enum fixed in
  v0.2.0, and now that quota refusals are `TRANSIENT`, it was an invitation to
  retry attached to "it already happened".
- **A deferred IFR operation answered "no such operation".** `ifr.rs` refused
  the ten IR operations it defers with `BAD_OPERATION`, which is byte-for-byte
  what an operation nobody thought about answers — so a client could not tell a
  decision from a gap, and `SERVICES-COVERAGE.md` could only tell them apart by
  searching this repository for a written reason. They answer **`NO_IMPLEMENT`**
  now: `NO_PERMISSION` is policy, `NO_IMPLEMENT` is deferred, `BAD_OPERATION` is
  "not an operation of the object you addressed". Affects `contents`, `lookup`,
  `lookup_name`, `describe_contents`, `describe`, `_get_defined_in`,
  `_get_containing_repository`, `get_canonical_typecode`, `get_primitive`,
  `_get_type`.
  *유예 연산은 이제 `NO_IMPLEMENT`로 답한다 — 와이어가 유예와 누락을 구분한다.*

### Added / 추가

- **`Contained::_get_version` is served.** It answered `BAD_OPERATION` while its
  write half `_set_version` answered `NO_PERMISSION` — "the operation exists and
  the answer is no" — on a version the facade parses out of every repository id
  it handles. The two were the wrong way round by `ifr.rs`'s own argument. The
  read answers now; the write is still refused. `corpus/services/ir-subset.idl`
  declares the attribute, so the generated skeleton serves it too and the
  byte-for-byte comparison in `ifr_shape.rs` covers it.
- **Constants carry their value, and are generated.** `Entry::Const` recorded
  the type and not the value, so `orbweaver-gen` skipped every constant — which
  the end-to-end run met concretely when a contract declared its authorization
  scope as a `const string` so a servant could name it, and the servant could
  not. The registry now folds the expression **once, where the names are**
  (`const long OFFSET = MAX_RETRIES * 2;` needs both arithmetic and IDL's
  outward scope resolution), coerces it to the declared type, and stores an
  evaluated `ConstValue`. An expression it cannot fold stores **no value** and
  the generator skips that item with a reason — never a guessed zero. A
  `string` constant is emitted as `&str`, since Rust has no `const String`.
  *상수는 값을 갖는다 — 레지스트리가 한 번 접고, 접을 수 없으면 값을 만들지 않는다.*

- **`CosNaming::NamingContextExt::to_url`**, measured against **two** producers:
  omniORB's own client resolved a URL ours built, and omniNames was run as a
  second producer over 14 argument pairs (11 identical, 3 differing only in
  hex-digit case, each parser reading the other's). One behaviour changed
  *because* of that comparison: an empty name returns the bare `corbaname:`
  form, as omniNames does.
- **`moe::Router::select`**, delegating to the trading engine. When a
  constraint names a field a wire-registered offer cannot answer it refuses the
  **whole call** with `NO_IMPLEMENT`: a shorter list would say *these are all
  the experts that qualify*, which is the sentence three-valued matching exists
  to prevent.
- **Constants are generated**, with the value folded once in the registry
  rather than an expression every consumer would have to fold — three folders
  that will disagree, and the one that disagrees silently ships. An expression
  that cannot be folded stores nothing rather than a guessed zero.
- **A deferral answers `NO_IMPLEMENT`, an oversight still answers
  `BAD_OPERATION`.** Ten Interface Repository operations moved, so the
  distinction `SERVICES-COVERAGE.md` opens by saying the wire cannot make is
  one the wire now makes for that service — and `BAD_OPERATION` there is zero.
- **The audit ledger is bounded** and spends one slot on an in-band `ELIDED`
  marker naming what it dropped, because a dropped hour and a quiet hour read
  identically exactly when somebody is reading the log to tell them apart.
  `verify_promotion` refuses a truncated history rather than concluding from a
  gap, and checks before parsing so the marker is not reported as a formatting
  bug.
- **A trace can name what failed**: `CallResult` carries the exception's
  repository id, so D004's `outcome` column stops saying `-` for a failure it
  simply had not been told about.
- **D006** (PROPOSED): the data-plane rule, and the finding that no option can
  enforce it — a bound constrains size, not frequency, and "never per token" is
  the operative phrase.
- **Attribute accessors are gated and visible.** `allow_interface` made
  `_get_balance` callable while the policy resolved scopes only through
  declared operations, so an `ai_authz` written on an attribute bought nothing
  and a `_set_` was never approval-gated — invisible *and* ungated, in the
  instrument built so an operator can see what an agent may reach. An
  `ai_effect` gates the setter only; a scope guards the value and so guards
  both accessors.
- **Token → `Caller` exchange** with the verifier as a trait this crate does
  not implement, and a **scope audit** that reports a contract scope no issued
  token can satisfy as an outage, naming the operations that go dark, before
  any call is made (D005's class).
- **`describe_interface` generated byte-identically** to the hand-written
  Interface Repository facade — 77 cases × 2 byte orders, with omniidl
  accepting the contract and the oracle's own decoder reading the reply back.
- **`docs/SERVICES-COVERAGE.md`** — all 107 declared operations probed over the
  wire, classified served / refused-with-a-reason / absent. **12 answered
  `BAD_OPERATION` with no reason written anywhere**; `PLAN-SERVICES` §8.1 now
  writes the sentence each was missing, including two that turned out to be
  defects and one that is recorded as undecided.
- The fuzz reaches the **text** parsers too (stringified IORs, D004 trace
  lines, the ingestion validators): 450,000 cases × 17 targets across five
  seeds, zero panics, with every target's reach asserted.
- **Dispatch runs concurrently** (`SharedDispatch`). One lock per servant, at
  most one held per thread, nothing blocking inside it — so lock *ordering* is
  vacuous rather than documented, since a deadlock cycle needs a thread holding
  two locks. Enforced structurally: a guard cannot escape its closure, a
  per-thread marker refuses a second section, and dialling asserts nothing is
  held (panicking in debug, so a violation fails the test that wrote it). The
  existing `Dispatch` trait and every generated skeleton are unchanged and
  still serialized. **The finding**: `ServerStats::peak_dispatching` was written
  to witness overlap and could not — it sits outside the servant's own lock and
  counted callers queued for it, reaching N on a *serialized* server. The
  negative control caught it; review had not.
- **Multi-object generated skeletons**: `knows()` required with no default,
  identity as an explicit `Target` argument, and **71 pinned cases answering
  byte-identically to the hand-written Interface Repository facade**, down to a
  minted reference's object key and port — with `ifr.rs` unmodified as the
  oracle.
- **IOR rewriting for NAT and containers** (R7): profiles and
  `TAG_ALTERNATE_IIOP_ADDRESS` rewritten, the object key refused as a target
  because it is identity rather than a route, an undecodable profile preserved
  byte for byte. Both real socket failures constructed. The container probe is
  written and **has never executed** — counted as a skip.
- **The quota interceptor seat**, with the window taken as a host-supplied
  label rather than from a clock, and refusals typed `TRANSIENT` when they
  renew and `NO_PERMISSION` when they do not.
- **The audit ledger leaves the process** in serving mode; it used to be
  drained only under `--dry-run`.
- **`#pragma prefix`/`version`/`ID`** and an S4 report for an explicit ID that
  is not a repository id.
- **D005** (PROPOSED): contract stability, and the argument that semantic drift
  is worse than identifier drift.

### Fixed / 수정

- **An unknown latency is not a fast one.** An offer registered over the wire
  carried `latency_p50: 0.0`, which did not merely fail to match
  `latency_p50 < 20` — it **matched**, so a latency-ordered router preferred
  exactly the experts nobody had measured. Matching is now three-valued and
  unknown sorts after every known value.
- The generator inverted a repository id into a Rust module path, emitting
  `pub mod acme.com` for prefixed IDL.
- **`_get_version` answered "no such operation" while `_set_version` answered
  "you may not"** — backwards by the servant's own argument, on data the
  registry parses out of every repository id it holds.
- **Annotations on types were never checked.** The contract checker walked
  interfaces only, so a typo on a `typedef` was silent — contradicting its own
  premise that an `ai_*` key nobody reads is what it reports.
- **A bound change is no longer reported as the silent class.** `idl-diff`
  described `sequence<octet>` becoming `sequence<octet, 64>` as "the encoded
  form differs, and CDR gives a receiver no way to notice". Both halves are
  false: the bytes are identical, and a peer that exceeds the bound is refused
  loudly. The verdict stays Breaking; the reason now says which kind of
  breaking, because the sentence being borrowed describes §5.3's measured case
  of a peer returning the wrong member and raising nothing.
- The `*.log` ignore swallowed the end-to-end provenance record, which only a
  fresh clone could reveal — the third time a blanket ignore has taken a
  committed artefact.

---

## v0.2.0 — 2026-08-14

Phase 4 substantially landed; five CORBA services served on our own POA; the
specification pipeline split into stages that can be measured one at a time.

### ⚠ Wire behaviour changed / 와이어 동작 변경

Four defects, in three groups, that change the bytes we put on the wire or
what we accept from a peer. Two of the three groups were found by a reader we
did not write — a foreign ORB, and the specification itself where no peer
could serve as one.

- **`completion_status` was transposed.** `COMPLETED_YES` is ordinal 0 per
  §4.11.4 (confirmed against omniORB before changing anything); our
  `Completion` had `No = 0, Yes = 1`. **A servant reporting "the operation did
  not run" reached every foreign ORB as "it ran"** — so a call refused before
  it started looked like a mutation that had happened, and a client that could
  have safely re-sent concluded it must not. Every servant uses the symbolic
  names, so the fix corrects the naming, event, IFR, expert and tenant services
  at once. `MAYBE` was 2 either way, which is why only two of the three were
  wrong and why nothing local caught it: our own client compared against the
  same enum and agreed with itself, including the test that asserted the
  encoded byte equalled `Completion::No as u32` and therefore moved with the
  bug. It now asserts the literal ordinal.
  **재시도 안전성을 결정하는 두 값이 뒤바뀌어 있었다.** 우리 클라이언트는 같은
  enum으로 비교하므로 스스로와는 늘 일치했고, 외부 ORB만이 이견을 낼 수 있었다.

- **Recursive types could not be marshalled at all.** Every non-empty recursive
  value was refused with "expected a value of type an indirection", and nothing
  noticed because the only generator that could have produced such a value was
  the one reporting the arm as unmeasured. Markers now resolve against the
  enclosing type the error path is already standing on; nesting is bounded at
  64 in both directions, because on decode the depth is the sender's choice.

- **Two fragment-reception defects**, found against hand-built §9.4.9 streams
  since no available peer emits fragments: a stray leading `Fragment` was
  returned as a message, and a fragment at a different GIOP version was
  accepted as a continuation (in 1.1 the bytes read as a request id are body,
  so a match would have been coincidence).

### Added / 추가

- **The end-to-end path, measured as one path** (`spikes/end_to_end.sh`, in the
  harness): a fresh requirement → S1–S5 → both generated halves → a servant on
  our POA → an agent-shaped caller through the guard, with a scope refusal
  visible in the transcript. **185 hand-written product lines against 778
  generated.** Composing it produced the release's most useful finding — see
  *Known limits*.
- **`#pragma prefix` / `version` / `ID`** — repository ids now match omniidl on
  a 25-id corpus, prefixes and all. Before this, every legacy IDL file (the OMG
  recommends a reverse-DNS prefix) would have given us a different identity for
  every type than the peer had, while looking correct locally.
- **`orbweaver-console`** — catalog, contract diff and D004 traces as
  self-contained HTML, no web framework and no template engine.
- **D004 tier 1 telemetry** — one span record per decision, no clock, and a
  credential structurally unable to reach a line.
- **S3i** — annotations inferred for ingested contracts, which never occupy a
  key a gate reads until a human approves them.
- **CORBA services on our POA**: CosNaming server, CosEvent push channel
  (bounded queue, dead consumers disconnected with **drops counted**),
  Trading wire surface for the MoE control plane, a read-only Interface
  Repository facade, and LifeCycle/tenancy with the tenant in every object key.
- **Remote IFR ingestion** — JacORB 3.9's Interface Repository served us. A
  contract can now be taken off the wire with no IDL file, with provenance
  marked and **contagious upwards**.
- **Server skeletons** (`orbweaver-gen`), driven by omniORB's own python
  client, with a servant fault surface whose `#[must_use] Raising` cannot
  become a `SystemException` without naming the completion status.
- **Server-side static-equals-dynamic oracle** — 204 reply-byte comparisons,
  three GIOP versions × two byte orders × two reply origins.
- **Concurrent connections** — cap 64, refusal spoken as §9.4.7's
  `CloseConnection`. Dispatch remains serialized and the documentation says so.
- **The guard's interceptor chain** (F4) and **dry-run**: an exposure can be
  read before it is deployed, audited under its own `DRYRUN-` token, and unable
  to diverge from the live gate by construction.
- **S1–S3 as distinct pipeline stages**, each a producer plus the gate that
  judges it, each runnable alone.
- **Property and contract testing** (`orbweaver-test`), including a wire fuzz
  measuring panic-freedom over the decoders a peer reaches before any policy
  runs: 0 panics in 50,000 cases × 10 targets.
- **Vector search** behind `search_interfaces` via an external command (D003-A)
  — no new dependency. The synonym class remains **UNMEASURED** without a key.
- Decisions **D003** (approved) and **D004** (approved): both adopted zero
  Cargo dependencies.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
  [`docs/PLAN-SERVICES.md`](docs/PLAN-SERVICES.md),
  [`docs/PLAN-DEFERRED.md`](docs/PLAN-DEFERRED.md),
  [`docs/PLAN-MOE.md`](docs/PLAN-MOE.md).

### Fixed / 수정

- The generator bound its encoder closure as `e`, so an IDL parameter named `e`
  shadowed it into `e.put(e)`. Generated locals are now `__`-prefixed and the
  corpus keeps a parameter named `e` on purpose.
- `--expose IDL:spike/Echo:1.0` split at the version's dot, so the
  bare-interface form allowlisted an interface nobody had. Found by dry-run on
  its first run.
- The registry's union `default_index` was computed against the unexpanded case
  list, and **the existing test asserted the buggy semantics**.
- `Poa::new` took an incarnation from a freed `Box` address, which the
  allocator then reused, so two POAs could share one.
- The offer store could lag the residency machine, so under memory pressure the
  loading policy returned an empty decision list — silently.

### Changed / 변경

- The harness takes a **machine-wide lock** and kills fixtures by process
  group. Two concurrent runs used to destroy each other's peers and report
  failures that were about the scheduling; that cost two diagnoses.
- `PLAN.md` / `PLAN.ko.md` at **v0.7**; the streams are scope and the status
  lives in `COMPONENTS.md`, which is refreshed after every wave.

### Known limits / 알려진 한계

Stated because an absence that is not written down reads as a feature:

- Nothing in the token exchange has been through a **real identity provider**;
  it is unit-tested against hand-built claims — the same shape as CSIv2 being a
  per-peer claim rather than a feature.
- **No rewritten IOR has been put in front of a foreign ORB**, and port
  translation has never been dialled. R7 is now measured across a real routing
  boundary (a second host), but both ends of that measurement were ours.
- **Multiplexing is refused below GIOP 1.2**, deliberately: a 1.1 `Fragment`
  carries no request id, so a fragmented 1.1 reply is attributable only by
  position. Against a stock omniORB that is a live limit, not a theoretical
  one — it fragments a 1 MB reply at 1.1, and we refuse it.
- A `CloseConnection` arriving **between fragments** surfaces as
  `UnexpectedMessage` rather than as a retryable close.
- Ingested contracts carry no SIDL, so the guard's gates have nothing to key
  on — a second, independent reason exposure stays off.
- The embedding synonym class, the TAO oracle column and the SSLIOP peer proof
  are **unmeasured**, each for a stated reason, and the harness counts them as
  skips rather than passes.
- **The pipeline is not reproducible across runs, and nothing catches it.**
  Re-running S1–S3 on the same requirement with the same prompts passed every
  gate 1/1 again and produced a different contract: different module and
  operation names, a different parameter type, and an authorization scope that
  drifted from the one the requirement literally states. An identity provider
  issuing the stated scope against such a contract refuses every legitimate
  caller. Recorded in `docs/pipeline-runs/2026-08-14-end-to-end.md`; the fix
  needs a decision about what S2 may choose, so it is named rather than
  patched.

---

## v0.1.0 — 2026-08-13

Phases 0–3.5. A from-scratch MIT ORB interoperating with omniORB 4.3.4 and
JacORB 3.9 in both directions at GIOP 1.0/1.1/1.2; IDL 4.2 front end and type
registry in full oracle agreement; POA and object model; dynamic invocation and
AnyJSON; the MCP triad over stdio with default-deny exposure and capability
handles; the S4 validation gate; CSIv2 wire and delegation policy.
