# TLS test fixtures / TLS 테스트 픽스처

Self-signed test certificates **we originated** with `regen.sh` (openssl CLI
only — the exact commands are in the script). Nothing here is copied from any
other project; the repository's MIT licence covers these files like any other.
They exist so the `ssliop` feature's tests (`crates/orbweaver-giop/tests/
ssliop_tls.rs`) run in-process, offline and deterministically.

`regen.sh`로 **우리가 직접 생성한** 자체 서명 테스트 인증서다(openssl CLI만
사용, 정확한 명령은 스크립트에 있다). 어떤 프로젝트에서도 복사하지 않았으므로
저장소의 MIT 라이선스가 그대로 적용된다. `ssliop` 피처의 테스트가 인프로세스로,
오프라인에서, 결정적으로 돌게 하기 위해 존재한다.

| File | What it is / 무엇인가 |
|---|---|
| `ca.pem` | Test CA certificate — the trust anchor the tests' client config uses. / 테스트 CA 인증서, 클라이언트 설정의 신뢰 앵커 |
| `server.pem` | Server certificate signed by `ca.pem`; CN `localhost`, SAN `localhost`, `127.0.0.1`, `::1`. / `ca.pem`이 서명한 서버 인증서 |
| `server.key` | The server certificate's private key (PKCS#8, ECDSA P-256). / 서버 인증서의 개인 키 |
| `wrong-ca.pem` | An unrelated CA that signed nothing — proves verification is on: a client trusting only this must refuse `server.pem`. / 아무것도 서명하지 않은 무관한 CA — 검증이 실제로 켜져 있음을 증명 |
| `regen.sh` | Regenerates the whole set from scratch. / 전체를 처음부터 재생성 |

Validity is ~25 years (to 2051), so nobody debugs a CI failure the day they
quietly expire. The CA private keys are deliberately not kept: regeneration
replaces the whole set, so nothing is ever signed twice.

유효기간은 ~25년(2051년까지)이다 — 조용히 만료된 날 CI 실패를 디버깅하는 사람이
없도록. CA 개인 키는 의도적으로 보관하지 않는다: 재생성은 전체를 교체하므로
같은 키로 두 번 서명할 일이 없다.

**These are fixtures, never deployment material.** The private key is public
in this repository by definition; anything that trusts `ca.pem` outside these
tests is misconfigured.

**픽스처일 뿐 배포용이 아니다.** 개인 키가 저장소에 공개되어 있으므로, 이 테스트
밖에서 `ca.pem`을 신뢰하는 설정은 잘못된 것이다.
