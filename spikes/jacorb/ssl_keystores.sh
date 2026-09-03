#!/usr/bin/env bash
# Derives JSSE keystores for `SslServer.java` from the PEM fixtures in
# spikes/tls/. Idempotent; the outputs are ignored by git.
#
# ── Why derived and not committed ────────────────────────────────────────────
#
# `spikes/tls/regen.sh` owns the fixtures and deletes the CA keys so nothing can
# ever be signed again. A JSSE keystore is a PACKAGING of those PEMs, not a
# second identity — committing it would be committing the same bytes twice in a
# format `regen.sh` does not know about, which is how two copies drift.
#
# ── Why JKS and not PKCS12 ───────────────────────────────────────────────────
#
# Not by preference. JacORB 3.9's `KeyStoreUtil.getKeyStore` loads from a FILE
# only when the type is `JKS`; any other type gets `KeyStore.load(null)`, an
# empty store, and the handshake fails with `No available authentication
# scheme`. Measured 2026-09-03 by calling that loader from its own package:
# `type=PKCS12 size=0`, `type=JKS size=1`. keytool warns that JKS is legacy; the
# warning is right in general and irrelevant to a fixture that exists to be
# read by one particular loader.
#
# ── What goes where ──────────────────────────────────────────────────────────
#
#   .server.jks   the server's identity (server.pem + server.key) AND the
#                 self-signed client certificate as a trusted entry — because
#                 `jacorb.security.jsse.trustees_from_ks=on` reads trust from
#                 the KEYSTORE, and `client.pem` is self-signed on purpose (see
#                 echo_server_ssl.py).
#   .trust.jks    ca.pem + client.pem, kept for symmetry with the omniORB
#                 fixture; with trustees_from_ks on, JacORB does not read it.
#
# *JSSE 키스토어는 PEM 픽스처의 **포장**이지 두 번째 신원이 아니므로 파생하고
# 커밋하지 않는다. JKS인 이유는 취향이 아니다 — JacORB 3.9의 로더는 JKS일 때만
# 파일에서 읽고 그 외에는 빈 저장소를 만든다(2026-09-03 측정: PKCS12 size=0).*
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TLS="$HERE/../tls"
JH="${ORBWEAVER_JAVA_HOME:-${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}}"
KT="$JH/bin/keytool"
[ -x "$KT" ] || { echo "no keytool at $KT"; exit 2; }
PASS=fixture

cd "$TLS"
rm -f .server.p12 .server.jks .trust.jks
openssl pkcs12 -export -in server.pem -inkey server.key -name server \
  -passout "pass:$PASS" -out .server.p12
"$KT" -importkeystore -noprompt -srckeystore .server.p12 -srcstoretype PKCS12 \
  -srcstorepass "$PASS" -destkeystore .server.jks -deststoretype JKS \
  -deststorepass "$PASS" 2>/dev/null
"$KT" -importcert -noprompt -alias client -file client.pem \
  -keystore .server.jks -storetype JKS -storepass "$PASS" >/dev/null 2>&1
"$KT" -importcert -noprompt -alias client -file client.pem \
  -keystore .trust.jks -storetype JKS -storepass "$PASS" >/dev/null 2>&1
"$KT" -importcert -noprompt -alias ca -file ca.pem \
  -keystore .trust.jks -storetype JKS -storepass "$PASS" >/dev/null 2>&1
rm -f .server.p12

# Said rather than assumed: the server store holds one key and one trustee.
n=$("$KT" -list -keystore .server.jks -storepass "$PASS" 2>/dev/null | grep -cE "PrivateKeyEntry|trustedCertEntry")
[ "$n" -eq 2 ] || { echo "expected 2 entries in .server.jks, got $n"; exit 1; }
echo "keystores derived: $TLS/.server.jks (key + client trustee), $TLS/.trust.jks"
