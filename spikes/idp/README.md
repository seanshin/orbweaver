# orbweaver-idp — a minimal OIDC/JWT issuer fixture

First-party, MIT, Python stdlib only, written to the published specifications
(RFC 6749 §4.4, RFC 7515, RFC 7517, RFC 7518 §3.2, RFC 7519, RFC 8414). It is
one of the two halves D010 B2's skip names: *"no peer advertises CSIv2 and no
issuer is configured (`ORBWEAVER_IDP_URL`)"*. The other half is
`spikes/jacorb/CsiServer.java` + `spikes/jacorb/csiv2_server.sh`.

## The bound — read this before reading any green

**An issuer of ours cannot refute a verifier's accepting direction.** A
verifier wrong in the accepting direction interoperates perfectly with every
honest token this fixture mints — and would also accept a forged one, which no
fixture we write can make visible. That is stream C's recorded reason
(`crates/orbweaver-mcp/src/token.rs` module docs, D002's rule) for leaving
`Verifier` a trait this project does not implement. What this fixture buys is
narrower and real: **the token-exchange path measured against a live issuer,
beside an independent peer's CSIv2 advertisement (JacORB's)** — not evidence
about verification.

`--misissue` is the closest an issuer can come to probing a consumer: it signs
with a key the JWKS does not publish, so any consumer that still accepts the
token is not checking signatures. `./run.sh selftest --misissue` must go red
at the signature step; that is this fixture's negative control.

*우리가 쓴 발급자는 검증기의 수용 방향을 반박할 수 없다. 수용 방향으로 틀린
검증기는 이 픽스처의 모든 정직한 토큰과 완벽히 상호운용되며, 위조 토큰도
받아들인다 — 우리 픽스처는 그것을 보이게 만들 수 없다. 이 픽스처가 사는 것은 더
좁고 실제적인 것이다: 독립 피어(JacORB)의 CSIv2 광고 옆에서, 실제 발급자를 상대로
한 토큰 교환 경로의 측정이다.*

## Endpoints

| method | path | what |
|---|---|---|
| GET | `/.well-known/openid-configuration` | discovery: issuer, `token_endpoint`, `jwks_uri` |
| GET | `/jwks.json` | one HS256 key, `kty: oct` (RFC 7517 §6.4) — published on purpose, see below |
| POST | `/token` | `grant_type=client_credentials`, `client_id` (form or HTTP Basic), optional `scope`, `audience`; returns an HS256 JWT |

Two deliberate fixture-isms, visible rather than hidden:

- **The JWKS publishes the symmetric key.** Anyone who can read it can forge
  tokens. Acceptable here because the key is random per run and dies with the
  process — and it is the accepting-direction bound above, worn openly. RS256
  would need an RSA signer, which is the dependency decision D002/token.rs
  refuses to make in a spike.
- **Loopback and an ephemeral port only.** `idp.py` binds `127.0.0.1:0` and
  prints `ISSUER`/`PORT`/`READY`; nothing squats on a well-known port.

## Running it

```bash
./spikes/idp/run.sh selftest              # start, verify end-to-end, stop; exit code is the verdict
./spikes/idp/run.sh selftest --misissue   # negative control: MUST go red at the signature check
./spikes/idp/run.sh start                 # prints the issuer URL; the starter owes the stop
./spikes/idp/run.sh stop
export ORBWEAVER_IDP_URL="$(./spikes/idp/run.sh start)"   # what D010 B2's skip asks for
```

Runtime state lives in `spikes/idp/.run/` (gitignored), never under
`/tmp/orbweaver*` — that prefix belongs to the harness's fixtures and its
cleanup sweeps. `run.sh` is a fixture driver, not a gate.
