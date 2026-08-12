# D001 — EUC-KR conversion: which dependency, if any

**Status:** APPROVED and implemented 2026-08-12. Policy amended; `encoding_rs` adopted behind the default-on `euc-kr` feature with attribution in `NOTICE`.
**상태:** 2026-08-12 승인·구현 완료. 방침 개정, `encoding_rs` 채택, `NOTICE`에 귀속 표시.

**Verified 2026-08-12** against the actual licence files, not summaries.

## The question / 문제

`PLAN.md` §4.4 makes codeset negotiation a first-class Phase 1 requirement with
EUC-KR support, because domestic legacy peers commonly run EUC-KR-family native
codesets. Rust has no EUC-KR conversion in its standard library, and the
workspace has zero external dependencies.

## What the survey actually found / 조사 결과

The first pass looked at crate licences. That was the wrong layer. **Every
EUC-KR table in existence derives from one of two upstream data sets, and the
crate licence sits on top of whatever that upstream requires.**

1차 조사는 크레이트 라이선스를 봤다. 잘못된 계층이었다. **현존하는 모든 EUC-KR
테이블은 두 상류 데이터셋 중 하나에서 파생되며, 크레이트 라이선스는 그 상류의
요구 위에 얹혀 있을 뿐이다.**

| Upstream | Licence (verified from the file) | Effect on us |
|---|---|---|
| **WHATWG `index-euc-kr.txt`** (17,048 entries, the normative euc-kr definition) | Spec is CC BY 4.0; *"To the extent portions of it are incorporated into source code, such portions in the source code are licensed under the **BSD 3-Clause License** instead."* | BSD-3-Clause attribution, unavoidable |
| **Unicode data files** | `Unicode-3.0` — OSI-approved, MIT-based, expressly covers data files | Permissive with attribution. The old `MAPPINGS/EASTASIA/KSC/KSX1001.TXT` path now 404s |

This explains the crate licences rather than contradicting them:

| Crate | Licence | Why |
|---|---|---|
| `encoding_rs` | `(Apache-2.0 OR MIT) AND BSD-3-Clause` | The BSD term **is** the WHATWG data. Honest and traceable |
| `encoding` | `MIT` | Declares MIT — but does **not** state where its table came from |

## The conclusion that reverses the first recommendation

The original recommendation was "write our own table, consistent with the
decision already taken for the ORB". **That reasoning does not survive contact
with the facts, for a reason the first draft of this document stated and then
failed to apply to itself:**

> a table derived from an incompatibly-licensed source is not laundered by
> being retyped.

The ORB and the table are not the same kind of problem:

- **The ORB is logic.** GIOP is a published specification, so we can implement
  it from the spec and owe nobody anything. That is why writing one was the
  right call even at fifteen weeks.
- **The table is data we do not own.** There is no specification to implement it
  from; there is only somebody's compilation of 17,048 mappings. Typing them in
  by hand produces the same derived work, more slowly and with transcription
  errors.

**ORB는 로직이고, 테이블은 우리 것이 아닌 데이터다.** GIOP는 공개 명세라 명세를
보고 구현하면 누구에게도 빚지지 않는다. 테이블에는 구현할 명세가 없고 누군가의
17,048개 매핑 편찬물만 있다. 손으로 옮겨 적어도 같은 파생물이 되며, 더 느리고
전사 오류가 생긴다.

Note the sharpest point: **the pure-MIT `encoding` crate is the least
trustworthy option, not the safest one.** A declared MIT licence that does not
account for the provenance of its data does not remove the upstream obligation;
it only hides it. An honestly-disclosed BSD-3-Clause is a better legal position
than an unexplained MIT.

**가장 날카로운 지점:** 순수 MIT인 `encoding` 크레이트가 가장 안전한 선택이
아니라 **가장 신뢰하기 어려운 선택**이다. 데이터 출처를 설명하지 않는 MIT 선언은
상류 의무를 없애지 않고 가릴 뿐이다.

## Outcome / 결과 — approved and implemented

**`encoding_rs` adopted for EUC-KR, behind the default-on `euc-kr` feature.**
Attribution is in `NOTICE`; the policy amendment is in `CLAUDE.md`, `README.md`
and `PLAN.md` §10. `--no-default-features` removes both the dependency and the
obligation, and `run_phase0.sh` tests that promise rather than merely repeating
it.

Verification of the table itself was done against an **independent**
implementation rather than a self-round-trip: `"함정 전투체계"` must encode to
`c7d4 c1a4 20 c0fc c5f5 c3bc b0e8`, which is what Python's EUC-KR codec
produces. A self-round-trip would pass even if the table were wrong in a
self-consistent way.

테이블 검증은 자기 왕복이 아니라 **독립 구현**과 대조했다. 자기 왕복은 테이블이
자기모순 없이 틀려도 통과하기 때문이다.

## Recommendation as filed / 당시 권고

**Use `encoding_rs` for EUC-KR, and amend the policy to say so explicitly.**

Proposed policy wording: *MIT for everything we write. Where a component is
data we cannot originate — a character mapping table, a timezone database —
permissive-with-attribution is accepted, disclosed in `NOTICE`, and recorded as
a decision.*

`encoding_rs` is the reference implementation of the very standard that defines
euc-kr, it is maintained, and its licence chain is fully traceable. The cost is
one attribution line.

**대안이 필요하다면:** Unicode 데이터에서 `Unicode-3.0`으로 파생하는 경로도
동등하게 정당하며 의무도 비슷하다. 어느 쪽이든 귀속 표시는 발생한다.

## What is NOT blocked by this / 이 결정이 막지 않는 것

Scoped down after the survey: **only the EUC-KR table is affected.** The rest of
codeset negotiation is protocol work with no data dependency, and is proceeding:

- `TAG_CODE_SETS` component parsing (§7.6.6.5)
- `CodeSets` service context emission (§7.10.2.5)
- The negotiation algorithm and its error cases (§7.10.2.6)
- UTF-8, UTF-16 and ISO-8859-1 conversion — all algorithmic, no table needed

EUC-KR sits behind a seam so that adding it is a table drop-in, not a redesign.

**축소된 범위: EUC-KR 테이블만 영향받는다.** 나머지 코드셋 협상은 데이터 의존성이
없는 프로토콜 작업이며 진행 중이다. EUC-KR은 이음매 뒤에 두어, 나중에 테이블만
끼워 넣으면 되도록 한다.
