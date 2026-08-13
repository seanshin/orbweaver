# D002 — TLS for SSLIOP: which implementation, if any

**STATUS: APPROVED** — proposed 2026-08-13, approved 2026-08-13 by the user
("승인, 진행"). The recommendation below is now policy: rustls under the MIT
arm of its triple licence, aws-lc-rs as provider, behind the off-by-default
`ssliop` feature, disclosed in NOTICE.
**상태: 승인됨** — 2026-08-13 제안, 같은 날 사용자가 승인("승인, 진행"). 아래 권고가
정책이 된다: rustls(3중 라이선스의 MIT 갈래), 제공자 aws-lc-rs, 기본 꺼짐 `ssliop`
피처 뒤, NOTICE에 공개.

**Verified 2026-08-13** against the licence files shipped inside the actual
crate tarballs from `static.crates.io` (rustls 0.23.43, ring 0.17.14,
aws-lc-rs 1.18.0, aws-lc-sys 0.44.0) and the crates.io registry metadata —
not against README summaries. Anything below not verified that way is marked
**unverified**.

## The question / 문제

`PLAN.md` §4.8 makes transport identity — *which process is connected?* — the
first row of the identity table, carried by SSLIOP/TLS. Stream C now parses
`TAG_SSL_SEC_TRANS`, so we can *see* TLS endpoints; dialing one needs a TLS
implementation. The workspace ships MIT and currently has exactly one external
dependency, accepted by D001. Which TLS implementation, if any, may an
MIT-only project depend on?

`PLAN.md` §4.8은 전송 신원(어느 프로세스가 연결되어 있는가)을 신원 표의 첫
행으로 두며 SSLIOP/TLS가 담당한다. 스트림 C가 `TAG_SSL_SEC_TRANS` 파싱을
구현했으므로 TLS 엔드포인트를 *볼* 수는 있다. 실제로 접속하려면 TLS 구현이
필요하다. MIT 전용 프로젝트가 어떤 TLS 구현에 의존해도 되는가?

## The distinction that matters / 핵심 구분

D001 drew the line at logic versus data: GIOP is a published specification, so
we implement it ourselves and owe nobody. TLS 1.2/1.3 are also published
specifications (RFC 5246, RFC 8446) — so why is first-party TLS not the same
call as first-party GIOP?

**Because our oracle cannot see crypto failures.** A GIOP mistake fails
loudly: omniORB rejects the message, the harness goes red, the cause gets
codified. A crypto mistake *interops perfectly*: a handshake with a timing
side channel, a non-constant-time comparison, or a broken random source
completes exactly like a correct one, against every peer we have. The entire
operating model of this project — batch, oracle, repair — rests on a
deterministic oracle, and for cryptographic correctness we do not possess one
and cannot build one. First-party TLS would be a hazard delivered with
confidence, which is the worst kind. Writing it ourselves is therefore ruled
out **not** by licensing but by honesty: we could never claim it was verified.

**우리 오라클은 암호 결함을 볼 수 없다.** GIOP 실수는 시끄럽게 실패하지만
(하네스가 빨개진다), 암호 실수는 *완벽하게 상호운용된다* — 타이밍 부채널이
있는 핸드셰이크도 올바른 것과 똑같이 완료된다. 이 프로젝트의 운영 모델 전체가
결정적 오라클 위에 서 있는데, 암호 정확성에 대한 오라클은 우리에게 없고 만들
수도 없다. 자체 TLS는 라이선스가 아니라 정직성 때문에 배제된다: 검증했다고
주장할 수 없는 것을 만들게 된다.

This does not fit D001's amended clause as written. The clause covers **data
we cannot originate**; a TLS stack is logic we *choose not to* originate,
because no oracle we can run would catch our mistakes. Adopting a TLS
dependency therefore needs its own recorded acceptance — this document — and,
if approved, a second policy amendment naming the category honestly:
*logic whose failure modes our oracles cannot detect (cryptography) is
depended on, not written, with the licence chain verified and disclosed.*

이는 D001 개정 조항(우리가 원저작할 수 없는 **데이터**)에 그대로 들어맞지
않는다. TLS 스택은 원저작하지 *않기로 선택하는* 로직이다 — 우리의 어떤
오라클도 그 실수를 잡지 못하기 때문이다. 따라서 채택하려면 이 문서와 같은
기록된 결정과, 승인 시 두 번째 방침 개정이 필요하다: *실패 양상을 오라클이
감지할 수 없는 로직(암호)은 작성하지 않고 의존하며, 라이선스 사슬을 검증하고
공개한다.*

## What the survey actually found / 조사 결과

D001's lesson was to check the layer under the declared licence. Applied here:
a TLS crate's declared licence sits on top of its *crypto provider's* licence,
and the provider is where the OpenSSL heritage lives. The pure-Rust protocol
layer is licence-trivial; the provider is the whole question.

D001의 교훈은 선언된 라이선스 아래 계층을 보라는 것이었다. 여기 적용하면: TLS
크레이트의 라이선스는 *암호 프로바이더*의 라이선스 위에 얹혀 있고, OpenSSL
혈통은 프로바이더 쪽에 있다. 순수 Rust 프로토콜 계층은 라이선스상 사소하며,
프로바이더가 문제의 전부다.

| Layer | Declared (crates.io, verified) | What the shipped licence files actually say (verified) |
|---|---|---|
| **rustls 0.23.43** | `Apache-2.0 OR ISC OR MIT` | Tarball ships `LICENSE-APACHE`, `LICENSE-ISC`, `LICENSE-MIT`; a genuine triple licence, we may take it as MIT. Default feature is `aws_lc_rs` (its `Cargo.toml` line `default = ["aws_lc_rs", ...]`); `ring` is an optional alternative |
| **aws-lc-rs 1.18.0** (default provider, Rust bindings) | `ISC AND (Apache-2.0 OR ISC)` | Tarball `LICENSE` reproduces exactly those texts. Effectively ISC |
| **aws-lc-sys 0.44.0** (the C library underneath) | `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)` | The vendored `aws-lc/LICENSE` (316 lines) is an honest provenance map: AWS-LC is a fork of BoringSSL, itself a fork of OpenSSL. OpenSSL-derived code is stated as **Apache-2.0** (post-relicensing); some files retain the 1995–1998 Eric Young (SSLeay) copyright notice with `SPDX-License-Identifier: Apache-2.0`; Jitter Entropy is BSD-3-Clause; mlkem-native is Apache-2.0/MIT/ISC. **No SSLeay or old OpenSSL advertising-clause licence text appears anywhere in the file** — `grep -i 'advertis'` finds nothing |
| **ring 0.17.14** (alternative provider) | `Apache-2.0 AND ISC` | Tarball `LICENSE` names: ISC for new code, Apache-2.0/ISC for BoringSSL-derived code, Apache/MIT for a once_cell polyfill, Apache-2.0 for fiat-crypto. A recursive grep of the whole shipped tree finds **zero** occurrences of "SSLeay" or "OpenSSL license" — the old OpenSSL-heritage licence text this survey went in expecting is gone as of this version |

Two findings that reverse this survey's own starting assumptions, in the D001
tradition of the facts correcting the brief:

1. **The feared OpenSSL/SSLeay dual licence — the one with the advertising
   clauses — is not present in either provider as currently shipped.** OpenSSL
   relicensed to Apache-2.0 (completed with OpenSSL 3.0), BoringSSL relicensed
   its upstream to Apache-2.0 (stated in AWS-LC's own LICENSE), and both
   providers inherit that. What remains of the heritage is copyright *notices*
   (Eric Young's name on some files) under Apache-2.0 terms — an attribution
   obligation, not an advertising clause.
2. **The provenance posture differs, and that is the real differentiator.**
   AWS-LC's LICENSE is a per-origin map that accounts for every lineage —
   exactly the "honestly disclosed" position D001 says to prefer. ring's is
   also honest but its README describes the project, in its own words, as
   "an experiment"; RUSTSEC-2025-0007 declared it unmaintained in February
   2025 and was withdrawn only because the rustls team took over security
   maintenance.

**두 가지가 이 조사의 출발 가정을 뒤집었다.** (1) 광고 조항이 있는 구
OpenSSL/SSLeay 이중 라이선스는 현재 출하되는 두 프로바이더 어디에도 없다 —
OpenSSL과 BoringSSL의 Apache-2.0 재라이선스를 물려받았고, 남은 것은 광고
조항이 아니라 귀속 표시 의무뿐이다. (2) 진짜 차이는 출처 관리 태도다:
AWS-LC의 LICENSE는 모든 혈통을 계보별로 설명하는 지도이고, ring은 자기
README가 스스로를 "실험"이라 부르며 2025년 2월 비유지보수 권고(철회됨)를
겪었다.

## Options considered / 검토한 대안

| Option | Verdict |
|---|---|
| **Write TLS ourselves** | Ruled out above. A published spec we *could* implement, but no oracle we possess detects crypto mistakes; the result would be unverifiable by our own honesty rules. The one option that is wrong for non-licensing reasons |
| **rustls + aws-lc-rs (its default)** | **Recommended.** Every licence in the chain is permissive-with-attribution (MIT-choice / ISC / Apache-2.0 / BSD-3-Clause), verified from shipped files. Actively maintained. Cost: a C/assembly build dependency (`aws-lc-sys` builds with cmake/cc) — a build-weight concern, not a licensing one, stated so nobody discovers it at integration |
| **rustls + ring** | Licence-acceptable on the same evidence (Apache-2.0 AND ISC, verified). Pure-ish build (cc only). But: self-described experiment, maintenance transferred to the rustls team after a withdrawn unmaintained advisory, and rustls itself moved its default away from it. Acceptable fallback, not the recommendation |
| **native-tls / platform TLS** | **Unverified** in this survey. Rejected on grounds that need no licence check: behavior varies per platform, which breaks "test both byte orders"-style determinism — the harness could not make one claim across CI and a developer laptop |
| **openssl crate (link OpenSSL 3)** | **Unverified** in this survey. OpenSSL 3 is Apache-2.0 so it is likely licence-fine, but it drags a system C library ABI into every build for no advantage over the option above |

## Recommendation / 권고

**Adopt `rustls` (taking the MIT arm of its triple licence) with its default
`aws-lc-rs` provider, behind an off-by-default `ssliop` feature, when stream C
reaches the dial-TLS batch.** Disclose the Apache-2.0, ISC and BSD-3-Clause
attributions in `NOTICE` exactly as D001 did for BSD-3-Clause. Amend the
policy with the category this actually is: logic whose failure modes our
oracles cannot detect is depended on, not written. Keep the feature
off-by-default until the certificate fixture and the per-peer harness group
exist, so the dependency never precedes the oracle that measures it.

**`rustls`(삼중 라이선스 중 MIT 선택) + 기본 `aws-lc-rs` 프로바이더를,
기본-꺼짐 `ssliop` 피처 뒤에서 채택할 것을 권고한다.** 귀속 표시는 D001과
동일하게 `NOTICE`에 공개하고, 방침에는 이것이 실제로 속하는 범주 — 오라클이
실패를 감지할 수 없는 로직은 작성하지 않고 의존한다 — 를 명시해 개정한다.
인증서 픽스처와 피어별 하네스 그룹이 생기기 전에는 피처를 켜지 않는다.

## What was verified, and what was not / 검증된 것과 아닌 것

Verified directly (files read from the named crate tarballs, 2026-08-13):
rustls 0.23.43's three licence files and its `default = ["aws_lc_rs", ...]`;
ring 0.17.14's LICENSE set and the absence of SSLeay/OpenSSL-licence text in
its shipped tree; aws-lc-rs 1.18.0's LICENSE; aws-lc-sys 0.44.0's vendored
`aws-lc/LICENSE` including its OpenSSL/SSLeay provenance statements; all four
crates.io licence declarations. RUSTSEC-2025-0007 (issued, then withdrawn)
from rustsec.org search results.

**Unverified, stated plainly:** the per-file headers of AWS-LC's ~thousands of
sources (we rely on its LICENSE file's summary being accurate); the legal
validity of upstream's Apache-2.0 assertion over the SSLeay-copyright files
(we rely on the OpenSSL relicensing effort and AWS's statement — this is the
one residual provenance risk, and it is the industry-wide position, not one
unique to us); native-tls and openssl crate licence chains (rejected on other
grounds before a licence check was warranted); the FIPS variants of aws-lc
(not proposed, not examined).

**검증 안 된 것도 그대로 적는다:** AWS-LC 수천 소스 파일의 개별 헤더(LICENSE
파일의 요약이 정확하다고 신뢰), SSLeay 저작권 파일에 대한 상류의 Apache-2.0
주장(OpenSSL 재라이선스 작업과 AWS의 진술에 의존 — 유일하게 남는 출처 위험이며
업계 전체가 같은 위치에 있다), native-tls·openssl 크레이트(라이선스 검토 전에
다른 근거로 배제), aws-lc의 FIPS 변형(제안하지 않았고 조사하지 않았다).

## What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is adopted today. This batch lands `TAG_SSL_SEC_TRANS` parsing and the
`spike-dump` visibility line with **zero** new dependencies — `cargo tree` is
unchanged. Certificate fixture choice, mTLS client-auth policy, and the
per-peer harness groups are later stream-C batches with their own oracles.
This document only answers the question that had to be answered before any of
them: *which implementation may we even consider.*

오늘 채택되는 것은 없다. 이 배치는 새 의존성 **0개**로 `TAG_SSL_SEC_TRANS`
파싱과 `spike-dump` 가시성 라인만 싣는다 — `cargo tree`는 변하지 않았다.
인증서 픽스처, mTLS 클라이언트 인증 정책, 피어별 하네스 그룹은 각자의 오라클을
가진 이후 배치들의 몫이다. 이 문서는 그 전에 답해야 했던 한 가지 — *어떤
구현을 고려해도 되는가* — 에만 답한다.
