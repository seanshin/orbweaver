# D001 — EUC-KR conversion: which dependency, if any

**Status:** OPEN — blocks Phase 1 Batch 2 (codeset negotiation). Does not block Batch 1.
**상태:** 미결 — Phase 1 배치 2(코드셋 협상)를 막음. 배치 1은 막지 않음.

## Problem / 문제

`PLAN.md` §4.4 makes codeset negotiation a first-class Phase 1 requirement, with
EUC-KR support, because domestic legacy peers commonly run EUC-KR-family native
codesets and a wrong negotiation corrupts exactly the text this project's home
market cares about.

Rust has no EUC-KR conversion in its standard library. The workspace currently
has **zero external dependencies**, which is a property worth spending
deliberately rather than by accident.

## Licence survey / 라이선스 조사 (verified 2026-08-12, crates.io API)

| Crate | Version | Licence | Verdict against the MIT-or-build-it policy |
|---|---|---|---|
| `encoding_rs` | 0.8.35 | `(Apache-2.0 OR MIT) AND BSD-3-Clause` | ⚠️ The `AND BSD-3-Clause` is not optional. Permissive and MIT-equivalent in effect, but **not literally MIT** — the same situation as the DOC License in `PLAN.md` §3.1 |
| `codepage` | 0.1.2 | `Apache-2.0 OR MIT` | ✅ MIT available, but it only maps codepage numbers onto `encoding_rs`, so it inherits the question above |
| `encoding` | 0.2.33 | `MIT` | ✅ Pure MIT, covers EUC-KR / Windows-949 — but unmaintained since ~2017 |
| `oem_cp` | 2.1.2 | `MIT` | ❌ OEM/DOS codepages only, no EUC-KR |

## Options / 선택지

1. **`encoding_rs`** — maintained, correct, widely used. Cost: accepts a
   BSD-3-Clause obligation, which relaxes the stated policy from "MIT" to
   "MIT-equivalent". *유지보수됨. 대가: BSD-3-Clause 의무 수용.*
2. **`encoding`** — satisfies the policy literally. Cost: unmaintained for ~9
   years, which is a real risk for a defence-adjacent deployment.
   *정책을 문자 그대로 충족. 대가: 약 9년간 유지보수 없음.*
3. **Write our own** — full control, unambiguously MIT, consistent with the
   decision already taken for the ORB core. Cost: the KS X 1001 / Windows-949
   table is roughly 8,000 entries. It is data rather than logic, so the risk is
   transcription accuracy, not design — and that is testable exhaustively by
   round-tripping every code point.
   *ORB 코어와 동일한 결정. 대가: 약 8,000 항목 테이블. 로직이 아니라 데이터이므로
   위험은 설계가 아니라 전사 정확성이며, 전 코드포인트 왕복으로 전수 검증 가능.*

## Recommendation / 권고

**Option 3, with option 1 as the fallback if schedule pressure demands it.**

The policy has already been applied at much greater cost — no MIT ORB existed,
so we are writing one. Writing a conversion table is a far smaller commitment
than that, and unlike the ORB it can be verified exhaustively: every code point
round-trips or it does not. Taking a BSD-3-Clause obligation to avoid ~8,000
lines of generated data would be inconsistent with a decision that already cost
15 weeks.

이미 훨씬 큰 비용을 치르며 정책을 적용했다 — MIT ORB가 없어서 직접 만들고 있다.
변환 테이블은 그보다 훨씬 작은 약속이고, ORB와 달리 전수 검증이 가능하다.

**Open question for the owner:** the source of the mapping data itself needs
checking. The Unicode Consortium mapping files carry their own terms, and a
table derived from an incompatibly-licensed source is not laundered by being
retyped. Resolve this before Batch 2 begins.

**소유자 확인 필요:** 매핑 데이터의 출처 자체를 확인해야 한다. 호환되지 않는
라이선스의 자료에서 파생된 테이블은 옮겨 적는다고 세탁되지 않는다.
