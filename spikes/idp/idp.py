#!/usr/bin/env python3
"""orbweaver-idp — a minimal OIDC/JWT issuer. TEST FIXTURE, first-party, MIT.

Written to the published specifications and to nothing else:

  RFC 6749 §4.4   the client_credentials grant (the only grant it serves)
  RFC 7515        JWS compact serialization
  RFC 7517        JWK / JWK Set (a symmetric key is `kty: oct`, §6.4)
  RFC 7518 §3.2   HS256 (HMAC-SHA256)
  RFC 7519        JWT claims (iss, sub, aud, exp, iat, jti)
  RFC 8414        /.well-known/openid-configuration (OIDC Discovery's shape)

Python stdlib only — http.server, hmac, hashlib, secrets, json, base64 — so
there is no dependency to weigh against CLAUDE.md's licensing rule.

THE BOUND, stated here because this file is where someone would overclaim:
an issuer we wrote cannot refute a verifier's ACCEPTING direction. A verifier
wrong in the accepting direction interoperates perfectly with every honest
token this fixture mints — and would also accept a forged one, which no
fixture of ours can make visible. That is stream C's recorded reason
(crates/orbweaver-mcp/src/token.rs, D002's rule) for leaving `Verifier` a
trait this project does not implement. What this fixture buys is the OTHER
side of D010 B2: a real issuer for the exchange path to be measured against,
beside an independent peer's CSIv2 advertisement (spikes/jacorb/CsiServer).

Two deliberate fixture-isms, both visible rather than hidden:

  * The JWKS publishes the HS256 key (`kty: oct`, RFC 7517 §6.4). A symmetric
    key in a public JWKS means anyone who can read the document can also FORGE
    tokens — acceptable in a fixture whose key is random per run and dies with
    the process, and exactly the accepting-direction bound above, worn openly.
    Doing RS256 honestly would need an RSA signer, which is the dependency
    decision D002/token.rs refuses to make in a spike.
  * `--misissue` signs with a key the JWKS does NOT publish. That is the
    negative control for any consumer that claims to verify: a verifier that
    still accepts the token under --misissue is not checking the signature.

Endpoints (loopback only; port 0 by default — never a well-known port):
  GET  /.well-known/openid-configuration
  GET  /jwks.json
  POST /token   application/x-www-form-urlencoded,
                grant_type=client_credentials, client_id=... (form or HTTP
                Basic), optional scope, optional audience
Startup prints ISSUER/PORT/READY lines to stdout and flushes; run.sh waits on
READY. SIGTERM exits cleanly.

*이 픽스처가 사는 것은 교환 경로를 실측할 실제 발급자이지, 검증기의 수용 방향에
대한 증거가 아니다. 수용 방향으로 틀린 검증기는 이 발급자의 모든 정직한 토큰과
완벽히 상호운용된다.*
"""

import argparse
import base64
import hashlib
import hmac
import json
import secrets
import signal
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


def b64url(data: bytes) -> str:
    """RFC 7515 §2 base64url: no padding."""
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


class Issuer:
    def __init__(self, url: str, ttl: int, misissue: bool):
        self.url = url
        self.ttl = ttl
        self.kid = secrets.token_hex(8)
        self.key = secrets.token_bytes(32)
        # --misissue: the JWKS still publishes self.key, but tokens are signed
        # with this one. A verifying consumer must refuse them.
        self.signing_key = secrets.token_bytes(32) if misissue else self.key

    def discovery(self) -> dict:
        return {
            "issuer": self.url,
            "token_endpoint": self.url + "/token",
            "jwks_uri": self.url + "/jwks.json",
            "grant_types_supported": ["client_credentials"],
            "token_endpoint_auth_methods_supported": [
                "client_secret_basic", "client_secret_post", "none",
            ],
            "response_types_supported": ["token"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["HS256"],
        }

    def jwks(self) -> dict:
        return {
            "keys": [{
                "kty": "oct",
                "use": "sig",
                "alg": "HS256",
                "kid": self.kid,
                "k": b64url(self.key),
            }]
        }

    def mint(self, sub: str, scope: str, aud: str) -> tuple[str, int]:
        now = int(time.time())
        header = {"alg": "HS256", "typ": "JWT", "kid": self.kid}
        claims = {
            "iss": self.url,
            "sub": sub,
            "aud": aud,
            "iat": now,
            "exp": now + self.ttl,
            "jti": secrets.token_hex(8),
        }
        if scope:
            claims["scope"] = scope
        signing_input = (
            b64url(json.dumps(header, separators=(",", ":")).encode())
            + "."
            + b64url(json.dumps(claims, separators=(",", ":")).encode())
        )
        sig = hmac.new(self.signing_key, signing_input.encode("ascii"),
                       hashlib.sha256).digest()
        return signing_input + "." + b64url(sig), self.ttl


class Handler(BaseHTTPRequestHandler):
    issuer: Issuer  # set by serve()

    # A fixture's request log is noise on the port the driver reads.
    def log_message(self, fmt, *args):  # noqa: A002 (stdlib signature)
        pass

    def _json(self, status: int, body: dict, extra: dict | None = None):
        data = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")  # RFC 6749 §5.1
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/.well-known/openid-configuration":
            self._json(200, self.issuer.discovery())
        elif path == "/jwks.json":
            self._json(200, self.issuer.jwks())
        else:
            self._json(404, {"error": "not_found"})

    def do_POST(self):
        path = urlsplit(self.path).path
        if path != "/token":
            self._json(404, {"error": "not_found"})
            return
        length = int(self.headers.get("Content-Length") or 0)
        form = parse_qs(self.rfile.read(length).decode("utf-8", "replace"))

        grant = (form.get("grant_type") or [""])[0]
        if grant != "client_credentials":
            # RFC 6749 §5.2: unsupported_grant_type is a 400.
            self._json(400, {"error": "unsupported_grant_type"})
            return

        client = (form.get("client_id") or [""])[0]
        auth = self.headers.get("Authorization", "")
        if not client and auth.startswith("Basic "):
            try:
                raw = base64.b64decode(auth[6:], validate=True)
                client = raw.split(b":", 1)[0].decode("utf-8", "replace")
            except (ValueError, IndexError):
                client = ""
        if not client:
            # RFC 6749 §5.2: invalid_client is a 401.
            self._json(401, {"error": "invalid_client"},
                       {"WWW-Authenticate": 'Basic realm="orbweaver-idp"'})
            return

        scope = (form.get("scope") or [""])[0]
        aud = (form.get("audience") or ["orbweaver-bridge"])[0]
        token, ttl = self.issuer.mint(client, scope, aud)
        body = {"access_token": token, "token_type": "Bearer", "expires_in": ttl}
        if scope:
            body["scope"] = scope
        self._json(200, body)


def serve(bind: str, port: int, ttl: int, misissue: bool) -> int:
    srv = ThreadingHTTPServer((bind, port), Handler)
    srv.daemon_threads = True
    url = f"http://{bind}:{srv.server_address[1]}"
    Handler.issuer = Issuer(url, ttl, misissue)

    def term(_signum, _frame):
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, term)

    # The driver waits on READY, printed strictly after the socket is bound
    # and listening (ThreadingHTTPServer's constructor binds and listens).
    print(f"ISSUER {url}")
    print(f"PORT {srv.server_address[1]}")
    if misissue:
        print("MISISSUE tokens are signed with a key the JWKS does not publish")
    print("READY")
    sys.stdout.flush()
    try:
        srv.serve_forever(poll_interval=0.2)
    except (KeyboardInterrupt, SystemExit):
        pass
    finally:
        srv.server_close()
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bind", default="127.0.0.1",
                    help="loopback only by default; a fixture is not a service")
    ap.add_argument("--port", type=int, default=0,
                    help="0 = ephemeral, printed as PORT (never squat on a well-known port)")
    ap.add_argument("--ttl", type=int, default=300, help="token lifetime, seconds")
    ap.add_argument("--misissue", action="store_true",
                    help="sign with a key the JWKS does not publish (negative control)")
    a = ap.parse_args()
    return serve(a.bind, a.port, a.ttl, a.misissue)


if __name__ == "__main__":
    sys.exit(main())
