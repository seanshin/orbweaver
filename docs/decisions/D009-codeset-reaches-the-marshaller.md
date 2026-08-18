# D009 — How a negotiated codeset reaches the marshaller

**STATUS: APPROVED** — drafted 2026-08-18 from the measurements in commit
`cffd748`, **revised the same day after review found the first draft's central
claim was false**, and approved by the user that day with the recommendation
adopted as written: **option A**, the owned `Arc<dyn TextCodec>` slot of §5.A,
in the four batches of §8, with §10's four obligations — including the
benchmark one, which means batch 1 either takes §9's number or records in
writing that it did not.

**상태: 승인됨** — 2026-08-18 작성, 같은 날 검토로 초안의 핵심 논거가 거짓임이
드러나 개정, 같은 날 승인. **A안**(§5.A의 소유 슬롯)을 §8의 네 배치로, §10의 네
의무와 함께 채택 — 벤치마크 의무 포함, 즉 1번 배치는 §9의 숫자를 재거나 재지
못했음을 문서로 남긴다.

> **What review changed.** The first draft recommended a `&dyn TextCodec` slot
> on `Encoder`/`Decoder` and claimed "145 construction sites unchanged,
> generated code unchanged". `Encoder` has **no lifetime parameter**
> (`cdr/src/lib.rs:159`). A borrowed slot forces `Encoder<'a>`, which changes
> every one of those sites *and* `Cdr::put`'s signature — the exact churn the
> draft used to reject option B. The recommendation survives; its mechanism
> does not. §5.A below is the corrected form, and §7 lists three questions the
> draft did not ask at all.

---

## 1. What is already known, measured / 이미 측정된 것

All executed against real peers, none inferred:

1. **A reference publishing no `TAG_CODE_SETS` refuses wide text.** omniORB
   4.3.4's client raised `INV_OBJREF` minor `0x4F4D0001` *inside itself* and
   sent nothing; our server logged one earlier request and no error. Fixed for
   `orbweaver-giop`'s four publish sites; `한글` round-trips at GIOP 1.2.
2. **The `char` codeset is negotiated and honoured by exactly one caller.**
   `Connection::char_converter()` has one call site, `spike_interop.rs:226`,
   and it uses it correctly — which is why this path always measured green:
   *the one binary exercising it was the one binary honouring it.*
3. **The static marshaller cannot honour it at all.** `Cdr for String` is
   `put_str`/`get_string`, UTF-8 unconditional; `Cdr for WString` calls a
   `wide()` helper hardcoding GIOP 1.2 + UTF-16. The trait is
   `fn put(&self, e: &mut Encoder)`; a connection is nowhere in it.

Because (3) holds, (2) must: the `char` conversion lists we publish are
**empty**, so we advertise UTF-8 and nothing else. Every peer here reaches
UTF-8, which is why nothing is red. §7.10.2.5 makes a peer that publishes
nothing a declaration of ISO-8859-1, so that peer is unreachable for us.

(3) 때문에 (2)를 유지해야 하고, 그래서 우리가 공표하는 char 변환 목록은 비어 있다.

---

## 2. The question / 문제

**Where does a CDR stream learn which encoding its text is in?**

Everything follows: whether we may advertise a conversion, whether a servant
behind a `Poa::reference` is callable, and whether §8's *static equals dynamic*
oracle keeps meaning something — the two paths agree today only because both
are hardwired to the same answer, which is agreement by coincidence.

---

## 3. What is broken, in three layers / 세 층위

| Layer | State | Where |
|---|---|---|
| **L1 — the stream** | text encoding is a property of the *call site*, not of the stream | `Cdr::put`/`get` take only `&mut Encoder`/`Decoder` |
| **L2 — the servants** | 7 non-test publish sites carry `components: Vec::new()`; their servants are silently uncallable for `wstring` | `orbweaver-object` ×4 (`lib.rs:243`, `:420`, `expert_service.rs:659`, `tenant_service.rs:728`), `orbweaver-registry` ×2 (`ifr.rs:732`, `spike_ingest.rs:730`), `orbweaver-gen` ×1 (`rt.rs:346`) |
| **L3 — GIOP 1.1** | our 1.1 `wstring` form and omniORB's disagree (`MARSHAL`), and **the peer is not an oracle** — it cannot unmarshal its own 1.1 `wchar` output | UNMEASURED in `codeset.rs`; the reverse client states the skip |

L2 is nine lines and is **not** what this document is for. It is listed because
landing it without L1 advertises a capability we do not have.

---

## 4. The constraint that decides this / 결정을 좌우하는 제약

`orbweaver-cdr` has **zero dependencies**, deliberately. `codeset.rs` is 1343
lines, needs `crate::Version` (a GIOP concept) and optionally `encoding_rs`
(BSD-3-Clause, default-on `euc-kr`, disclosed in `NOTICE` under D001).

**The tables cannot move down.** Any option putting `codeset` into
`orbweaver-cdr` drags a GIOP concept and a licence obligation into the one
crate that has neither, and breaks the `--no-default-features` promise
`run_checks.sh` tests. That is the licensing boundary, not a preference.

---

## 5. The options / 선택지

### A. The stream carries a codec it does not own — *recommended*

`orbweaver-cdr` gains a **trait and an owned slot**, no tables:

```rust
pub trait TextCodec: Send + Sync {
    fn put_string(&self, e: &mut Encoder, s: &str) -> Result<()>;
    fn get_string(&self, d: &mut Decoder<'_>) -> Result<String>;
    fn put_wstring(&self, e: &mut Encoder, s: &str) -> Result<()>;
    fn get_wstring(&self, d: &mut Decoder<'_>) -> Result<String>;
}
```

`Encoder`/`Decoder` carry **`Option<Arc<dyn TextCodec>>`**, not a reference.

**Why `Arc` and not `&dyn`, which is the whole correction.** `Encoder` is
`pub struct Encoder { buf, endian, origin, virtual_offset }` — no lifetime. A
borrowed slot makes it `Encoder<'a>`, and that propagates to every signature
naming it, including `Cdr::put(&self, e: &mut Encoder)` and its 145
construction sites. `Arc` keeps the type as it is: `None` costs nothing, a
clone is a refcount, and `orbweaver-cdr` gains no dependency because `Arc` is
`std`.

| | |
|---|---|
| Generated code | **unchanged** — `Cdr::put(&self, e)` keeps its signature, so no emitted line moves and `tests/emitted/` stays valid |
| Construction sites | unchanged; the slot defaults to `None`, which is today's behaviour exactly |
| New dependency | none — a trait definition, not a table |
| Cost | a `dyn` call per string, an `Option` that can be `None` by accident, and a **second place text encoding is described** |

### B. Thread the codec through the trait

`fn put(&self, e: &mut Encoder, cs: &Codecs)`.

| | |
|---|---|
| Generated code | every emitted `put`/`get` changes; `gen/src/lib.rs` emits them in six places |
| Call sites | 11 `Cdr` impls in `rt.rs`, every generated one, every caller |
| Gain | explicit; no `Option` to forget |
| Cost | a public trait break, every fixture regenerated, and a parameter every future implementor must remember — the same "remember the rule" shape this project replaced with a type twelve hours ago (`AuditReason`) |

### C. Convert in the generated stub

**Refused on this project's own rule**: `orbweaver-gen` exists on the principle
that a generated file contains no encoding rules. Phase 3's `wstring` BOM
failure came from re-implementing wire knowledge instead of reusing it.

### D. Do nothing

The status quo, and **honest today** — an empty conversion list is a true
statement about what we can do. It is not a failure state; it is a smaller
product, permanently unreachable by a peer without UTF-8.

---

## 6. The codec's contract, stated / 코덱 계약 명시

The first draft left this to the implementor. It cannot be:

- **The codec owns the octets; the stream keeps the framing — for narrow text.**
  *Corrected 2026-08-18, during batch 1, by reading the code this paragraph was
  about.* The draft said the codec owns the whole field including the length
  and the NUL. It should not: `Encoder::put_string_bytes` and
  `Decoder::get_string_bytes` **already** separate framing from encoding, and
  the framing carries three rules — the length counts the NUL, an embedded NUL
  is `Error::EmbeddedNul`, a zero length is malformed — that exist in exactly
  one place today. Handing them to every codec is how they stop agreeing.
  `spike_interop.rs:226`, the one caller that converts correctly, already uses
  this seam. So: `fn encode_narrow(&str) -> Vec<u8>` / `fn decode_narrow(&[u8])
  -> String`, and `put_str`/`get_string` consult the slot with `None` meaning
  UTF-8.

- **Wide text is the other way round, and that asymmetry is the design.** A
  `wstring`'s framing *itself* varies — GIOP 1.1 versus 1.2, and the BOM — which
  is why `WideCodec` exists and does its own framing. `orbweaver-cdr` has no
  wide-string support at all and must not grow any: it would have to learn a
  GIOP version. Batch 1 therefore covers **narrow text only**; batch 3 unifies
  where the wide codec is *stored*, not what it is.

  *좁은 문자열: 코덱은 옥텟만, 프레이밍은 스트림이 지킨다 — 초안이 틀렸고, 그
  문단이 다루는 코드를 읽어서 고쳤다. 넓은 문자열은 프레이밍 자체가 버전에 따라
  달라 정반대다. 이 비대칭이 설계다.*
- **Alignment stays the stream's.** The codec is called with the encoder
  already positioned; it must not align, because an encapsulation's origin is
  the stream's business and §9.4's alignment origin rule is already subtle
  enough (`5c41961` fixed one leak of it).
- **A bound is in characters and must stay so.** `Boundable for String`
  already documents this: *"Characters, not bytes: `string<8>` admits eight
  Korean syllables, which are twenty-four octets on the wire."* A codec that
  made the bound count transmitted octets would silently narrow every bounded
  string under a multi-byte codeset. The bound check therefore stays **above**
  the codec, where it is now.
- **Errors cross as the stream's error.** `orbweaver-cdr::Error` does not know
  about codesets, and should not learn. An unconvertible character is a
  `Malformed` with a message the codec supplies — the same shape a truncated
  read already has.

- **코덱이 길이 접두사와 NUL까지 소유한다** — 나누면 NUL을 세는지에서 둘이 갈린다.
  **정렬은 스트림의 몫**이다. **상한은 문자 단위이며 그대로 둔다** — 전송 옥텟으로
  세는 코덱은 다바이트 코드셋에서 모든 상한을 조용히 좁힌다. **오류는 스트림의
  오류로 건넌다.**

---

## 7. Three questions the first draft did not ask / 초안이 묻지 않은 것

### 7.1 The reply side

`Request::code_sets()` now exists, so a servant can see what the client
declared. Nothing says where the codec is attached to the **reply** encoder.
It must be the same agreement as the request's, and that is a `Server`/
`Dispatch` change, not a `Cdr` one. Batch 2 owns it; a reply encoded under a
different codeset from its request is the defect this whole document is about,
pointing the other way.

### 7.2 TCS-C and TCS-W negotiate independently

They are separate fields of `CodeSetComponentInfo` and can fail independently:
a peer may agree on `char` and not on `wchar`. `CharCodeset` already has three
outcomes (`cffd748`); the trait above lumps `string` and `wstring` into one
object, so **the object must be able to hold a working `char` codec and a
failed `wchar` one**, refusing only the operations that need the failed half.
A single `Option<Arc<dyn TextCodec>>` that is `None` when *either* half fails
would take working `char` conversion away for a `wchar` disagreement.

### 7.3 The dynamic path must converge, not coexist

`orbweaver-dynamic` already threads a codec explicitly: `encode_with(…, wide:
WideCodec)`, with `default_codec()` for the convenience entry points. If the
static path reads the stream and the dynamic path keeps its parameter, there
are two mechanisms for one fact — precisely the drift D007 warns about, and
§8's oracle compares these two paths. Batch 3 must retire `default_codec()`
and `wide()` **together**, or state why one survives.

> **Corrected 2026-08-18, at the start of batch 3: one survives, and this
> paragraph had them backwards.** Counting the call sites rather than assuming
> them: every *wire* use of `default_codec()` is inside an `any`'s
> encapsulation, and §9.3.1.6 makes the 1.2 form the rule there **regardless of
> the message's version** — so it is not a hardcoding, it is the specification.
> Its doc comment gives two justifications, "what both fixtures negotiate in
> practice" and "what an encapsulated `any` uses regardless of the connection's
> version", and only the second is a reason. The first is a guess sitting next
> to a rule and borrowing its authority.
>
> `wide()` in `orbweaver-gen`'s `rt.rs` is the actual defect: it serves
> `Cdr for WString`/`WChar` on the **top-level message body**, where the form
> does depend on the connection's version, and a trait method that takes only
> `&mut Encoder` cannot ask. That is what batch 3 fixes.
>
> **하나는 살아남고, 이 문단은 둘을 거꾸로 짚고 있었다.** `default_codec()`의
> 와이어 사용처는 전부 `any`의 캡슐화 안이고 거기서 1.2는 §9.3.1.6의 **규칙**이다.
> 주석이 근거를 둘 대지만 하나만 근거이고, 나머지는 규칙 옆에 앉아 권위를 빌린
> 추측이다. 실제 결함은 `wide()` — 최상위 본문에서 쓰이며 거기서는 형식이 연결의
> 버전에 달렸는데, `&mut Encoder`만 받는 트레이트 메서드는 물어볼 수가 없다.

---

## 8. Recommendation / 권고

**Adopt A, in four batches, each with its own oracle.** The order is set by
what would otherwise be advertised before it works.

| # | Batch | Oracle |
|---|---|---|
| 1 | `TextCodec` + `Option<Arc<…>>` slot in `orbweaver-cdr`; `None` is today's behaviour | **byte identity**: the whole workspace green and every peer capture re-read unchanged. A refactor that moves one byte has failed |
| 2 | `codeset` implements it; `Connection`/`Mux` attach it; **the reply side (§7.1)**; §9.3.1.6's rule that a `wchar` inside an encapsulation is always the 1.2 form, in code rather than in a comment | the existing peer captures byte for byte; a negative control per rule; a request and its reply asserted to use one agreement |
| 3 | `Cdr for String`/`WString` ask the stream; `wide()` and `default_codec()` deleted **together** (§7.3) | §8's *static equals dynamic* over the golden corpus, both peers, both byte orders — the paths must still agree **and now agree for a reason** rather than by both being hardwired |
| 4 | L2's seven publish sites; **then** a non-empty `char` conversion list | a peer advertising ISO-8859-1 **without** UTF-8. If none can be produced here, the list stays empty and the batch reports **BLOCKED** — advertising a conversion we cannot demonstrate is the defect this document exists to avoid |

Do not adopt B, C or D for the reasons in §5.

**A안, 네 배치.** 4번의 비어 있지 않은 변환 목록은 그것을 요구하는 피어를 만들어
낼 수 있을 때만 착지하고, 못 만들면 **BLOCKED**로 보고한다.

---

## 9. What would falsify this, and what cannot be measured yet / 반증과 미측정

> **Discharged 2026-08-18, batch 1.** The benchmark was built (`call-bench`)
> and the number taken, twice, independently: the dynamic path costs **+2.0 µs
> p50 on a sixty-four-string payload** whose round trip is 33 µs — a ratio of
> **1.06×**, and about **31 ns per string**. It is per *string*, not per byte:
> one 4 KiB string costs the same 0.5 µs as a 16-byte one. A codeset
> indirection paid once per string is lost inside the call. **The falsifier did
> not fire; A stands on a measurement rather than on "a refcount is cheap".**
>
> A second finding came with it, and it is the same class this project keeps
> hitting: **§11's target cannot be compared to anything.** *"≤ 5 ms added and
> ≤ 3× static"* names no operation shape, no payload, no sample count and no
> machine, and at 21 µs its two clauses disagree by three orders of magnitude —
> "≤ 5 ms added" allows 240× the entire call while "≤ 3× static" allows 42 µs.
> Which clause binds is a different test. The benchmark prints that instead of
> picking one. Restating §11 as a shape-qualified ratio is its own change.
>
> §9 배치 1에서 **갚았다**: 문자열당 약 31ns, 64개 문자열 페이로드에서 1.06배.
> **반증 시험은 발화하지 않았고, A안은 "refcount는 싸다"가 아니라 측정 위에
> 선다.** 함께 나온 두 번째 발견: **§11의 목표는 무엇과도 비교할 수 없다** —
> 모양도 페이로드도 표본 수도 기계도 없고, 두 절이 세 자릿수만큼 어긋난다.

**A `dyn` call per string was claimed to be free. That claim could not be
tested when this was written.** §8's table names a *LAN echo benchmark, dynamic path vs static stub,
within §11 targets* — **it does not exist.** `COMPONENTS.md` says telemetry has
"counts and no latency (no clock)", and the only `bench` binary in the tree is
`search_bench`, which measures retrieval quality.

So batch 1 carries a second deliverable: **the echo benchmark §8 already
promises**, or an honest statement that the cost is unmeasured. Adopting A on
"a refcount is cheap" is exactly the kind of unmeasured constant this project
refuses to write into a contract.

If the number turns out to be visible, B becomes worth its churn — it has no
indirection at all.

A slower falsifier: if a pilot needs a codeset **per operation** rather than
per connection, the slot is in the wrong place and B's explicit parameter is
right. Nothing suggests it; CORBA negotiates per connection.

**`dyn` 호출 비용이 공짜라는 주장은 지금 시험할 수 없다** — §8 표가 약속한 LAN
에코 벤치마크가 **존재하지 않는다.** 1번 배치가 그 벤치마크를 함께 만들거나,
비용이 미측정임을 정직하게 적는다. "refcount는 싸다"로 채택하는 것은 이 프로젝트가
계약에 쓰기를 거부하는 바로 그 미측정 상수다.

---

## 10. What approval would mean / 승인의 의미

1. **A second place text encoding is described.** `TextCodec` joins §4.5 as a
   contract with a round-trip acceptance criterion, and must be held to one by
   its own tests, not only its callers'.
2. **A public API addition to `orbweaver-cdr`**, the crate whose dependency
   count is a stated property. A trait is not a dependency, and the
   `--no-default-features` check must still pass with the slot in place.
3. **Nothing about advertising.** Approval does not approve a non-empty
   conversion list: batch 4 is conditioned on a peer, not on a decision.
4. **A benchmark obligation.** §9's number gets taken in batch 1, or batch 1
   says in writing that it was not.

승인은 (1) 텍스트 인코딩을 기술하는 **두 번째 자리**를 인정하고 왕복 수용 기준을
지우며, (2) 의존성 수가 명시된 성질인 크레이트에 트레이트를 더하고, (3) 광고에
대해서는 **아무것도** 승인하지 않으며, (4) §9의 숫자를 1번 배치에서 재거나 재지
못했음을 문서로 남기는 의무를 진다.
