#!/bin/sh
# Regenerates the TLS test fixtures in this directory, from scratch.
#
# These are self-signed fixtures we originated with the commands below —
# nothing is copied from any other project, so the repository's MIT licence
# covers them like any other file here. The outputs are committed so the
# `ssliop` tests are deterministic and run offline; re-run this script only
# when the set must be replaced (validity is ~25 years), then commit the new
# outputs together.
#
# 이 디렉터리의 TLS 테스트 픽스처를 처음부터 다시 생성한다. 아래 명령으로 우리가
# 직접 만든 자체 서명 픽스처이며 어디에서도 복사하지 않았으므로 저장소의 MIT
# 라이선스가 그대로 적용된다. `ssliop` 테스트가 오프라인에서 결정적으로 돌도록
# 산출물을 커밋한다. 유효기간(~25년)이 남은 동안은 재생성할 필요가 없다.
#
# Requires only the openssl CLI (generated with OpenSSL 3.6.2 on macOS).
set -eu
cd "$(dirname "$0")"

DAYS=9125 # ~25 years: a fixture that outlives the project beats a CI failure
          # that nobody can attribute the day it silently expires.

# 1. The trust anchor the tests build their rustls ClientConfig from.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -sha256 -days "$DAYS" -nodes \
  -subj "/O=Orbweaver test fixtures/CN=Orbweaver test CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign" \
  -keyout ca.key -out ca.pem

# 2. The certificate the in-process rustls test server presents. CN and the
#    SANs cover every way a test can name the loopback peer.
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -sha256 -nodes -subj "/CN=localhost" \
  -keyout server.key -out server.csr

cat > server.ext <<'EOF'
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost, IP:127.0.0.1, IP:::1
EOF
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
  -sha256 -days "$DAYS" -extfile server.ext -out server.pem

# 3. A second, unrelated trust anchor that has signed nothing. The tests use
#    it to prove certificate verification is actually on: a client trusting
#    only this CA must refuse the server above.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -sha256 -days "$DAYS" -nodes \
  -subj "/O=Orbweaver test fixtures/CN=Orbweaver wrong CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign" \
  -keyout wrong-ca.key -out wrong-ca.pem

# The CA keys are deliberately not kept: nothing will ever be signed again —
# regeneration replaces the whole set — and a committed CA key invites the
# question of what else it might have signed.
rm -f ca.key wrong-ca.key server.csr server.ext ca.srl
