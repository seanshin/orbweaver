# 2026-08-18 — a union label is a value, and it has a byte order

> Batch 3. Not the batch that was agreed: it was promoted ahead of the service
> absences because the previous batch produced a measurement that changed what
> the highest-value work was. The finding came out of writing
> [`D008`](../decisions/D008-anyjson-self-description.md)'s structural TypeCode
> and was deliberately *not* fixed there — see
> [`2026-08-18-anyjson-self-description.md`](2026-08-18-anyjson-self-description.md) §2.

## 1. Two defects, one line of code apart, both against a real peer

`orbweaver-giop` read a union case label with `get_bytes` and wrote it with
`put_bytes`. Neither knows the stream's byte order and neither aligns. A label
is the **discriminator marshalled in its own type**, so it needs both.

The first measurement was hand-built and therefore only suggestive. The
decisive one asked omniORB 4.3.4 to marshal an `any` carrying a union, on this
host, which is little-endian — and so is nearly every host a peer runs on:

```
--- long discriminated
case "as_long" label bytes = [1, 0, 0, 0]        ← the wire's order, kept raw
Big:    REFUSED: no branch of U matches the discriminator …
Little: REFUSED: no branch of U matches the discriminator …

--- long long discriminated
DECODE FAILED: Cdr(Malformed("string length must include the NUL"))
```

- **A1, alignment.** An 8-byte label must be 8-aligned inside the
  encapsulation. omniORB pads to it; we did not, so the read was four bytes
  early and everything after it shifted. The diagnostic named a **string**,
  four fields past the actual fault. A `long` union survived only because the
  case count in front of it happened to leave the stream 4-aligned — which is
  why this looked like it worked.
- **A2, byte order.** A `long` union decoded and then matched **nothing**:
  `select_case` probes big-endian, the label was little-endian, and the refusal
  said *"no branch of U matches the discriminator"* — blaming the caller for
  the label's encoding.

**Why 1200 tests were green.** Our encoder wrote labels raw and our decoder
read them raw, so the two agreed with each other in *any* byte order. A
round-trip oracle cannot fail on a convention it applies to both ends. Nothing
in the repository compared our bytes to somebody else's for this field.

## 2. The fix

`UnionCase::label` is now always **big-endian** — the order
`orbweaver_dynamic::select_case` already probes in — and conversion happens
exactly at the wire, in one function that is its own inverse. Labels align to
`min(width, 8)` on both sides.

## 3. What this unblocked, immediately

AnyJSON v1.1 shipped union labels as base64 *in the same session*, for one
reason: the bytes had no knowable order, so turning them into a number would
have been a guess. That reason lasted one commit. Labels now cross as values —
`"label":1`, and for an enum discriminator `"label":"GREEN"`, which is the
entire point of a mapping whose justification is that a type describes itself.
An undecodable label falls back to `{"_raw": <base64>}`, tagged: a malformed
TypeCode is its producer's problem, and a renderer that refuses to render it
hides the evidence.

The base64 form was never released — it existed only in `Unreleased` — so this
is a correction, not a migration.

## 4. Codified

- `crates/orbweaver-giop/tests/union_labels_from_a_peer.rs`: **the bytes
  omniORB actually wrote**, both discriminator widths, recorded with their
  provenance. The tests decode them, and one **re-encodes back to the peer's
  bytes** — the direction our own round trip could never check. Padding is
  excluded from the comparison because the peer does not zero it, which is
  `CLAUDE.md`'s rule showing up in a capture.
- `spikes/union_label_capture.py` + a harness group: re-takes the capture from
  the live fixture and compares it to the recording, because a recording nobody
  re-takes is a claim about the past. Reports SKIPPED, never passing, when the
  fixture cannot marshal.
- An enum-discriminated union in the §4.5 tests, asserting the document is
  **readable** and not merely round-tripping — the assertion base64 would have
  passed.

**Negative control:** all four peer-byte tests fail against the pre-fix code
and pass after. Run before claiming the tests were worth writing.

## 5. Measurements

| | before | after |
|---|---|---|
| omniORB `long` union TypeCode | decodes, **every branch misses** | decodes, branches select |
| omniORB `long long` union TypeCode | **cannot be decoded** | decodes, re-encodes to the peer's bytes |
| `cargo test --workspace` | 1205 | **1209** |

## 6. 한국어 요약 / Korean summary

union 레이블은 판별자를 그 타입으로 마샬링한 것이므로 **정렬과 바이트 순서**가
모두 필요하다. `get_bytes`/`put_bytes`는 둘 다 모른다. 실측(omniORB 4.3.4, 이
호스트는 리틀엔디언):

- **A1 정렬.** 8바이트 레이블은 캡슐화 안에서 8정렬이어야 한다. omniORB는 패딩을
  넣고 우리는 넣지 않아 네 바이트 일찍 읽었고, 이후 전부 어긋났다. 진단은 네 필드
  뒤의 **문자열**을 가리켰다. `long` union이 멀쩡해 보인 건 앞의 케이스 개수가
  우연히 4정렬을 남겼기 때문이다.
- **A2 바이트 순서.** `long` union은 디코드된 뒤 **아무 분기와도 맞지 않았고**,
  거부 메시지는 레이블이 아니라 호출자의 판별자를 탓했다.

**왜 1200건이 초록이었나.** 인코더가 원바이트로 쓰고 디코더가 원바이트로 읽었으니
둘은 어떤 순서에서도 서로 일치했다. **왕복 오라클은 양쪽에 똑같이 적용되는 관례를
반증하지 못한다.** 이 필드에 대해 우리 바이트를 남의 바이트와 비교한 것은 저장소에
하나도 없었다.

**성문화.** 피어가 실제로 쓴 바이트를 출처와 함께 고정하고, 그중 하나는 **피어의
바이트로 되돌려 인코딩**되는지까지 본다(우리 왕복이 결코 할 수 없던 방향). 하네스는
살아 있는 픽스처에서 캡처를 다시 떠 기록과 대조한다 — 다시 뜨지 않는 녹음은 과거에
대한 주장일 뿐이다. 음성 대조군: 네 건 전부 옛 코드에서 실패, 새 코드에서 통과.

**이것이 즉시 푼 것.** v1.1이 같은 세션에 base64로 내보낸 레이블은 이제 값이다 —
enum 판별자는 `"label":"GREEN"`으로 읽힌다. base64 형식은 릴리즈된 적이 없으므로
마이그레이션이 아니라 정정이다.
