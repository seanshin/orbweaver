#!/usr/bin/env python3
"""End-to-end oracle for the orbweaver-idp fixture. Stdlib only. Exit code is
the verdict: 0 every check held, 1 a check failed, 2 usage.

Run by `spikes/idp/run.sh selftest`, which owns the issuer's lifecycle; this
file only speaks HTTP to a URL it is handed.

What it checks, in the order a real consumer would meet them:
  1. discovery names this issuer and points at real endpoints;
  2. the JWKS carries an HS256 `oct` key (published on purpose — see idp.py's
     header for why that is a fixture-ism and what it deliberately gives up);
  3. /token answers a client_credentials POST with a three-segment JWT whose
     HMAC verifies against the JWKS key and whose claims are coherent
     (iss/sub/aud/scope/exp>iat);
  4. a tampered signature does NOT verify (the check can go red);
  5. an unsupported grant_type is a 400, a missing client is a 401 — refusals
     per RFC 6749 §5.2, not silence.

Against `run.sh selftest --misissue` step 3's signature check MUST fail —
that is the whole point of the flag, and the command is this fixture's
negative control: a selftest that stays green there verifies nothing.

THE BOUND (same sentence as idp.py, because a green here is where it would be
misread): these checks prove OUR issuer emits what the specifications say.
They cannot refute any verifier's accepting direction — a verifier wrong in
the accepting direction passes every one of these and a forgery besides.
"""

import base64
import hashlib
import hmac
import json
import sys
import urllib.error
import urllib.parse
import urllib.request


def b64url_decode(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def get_json(url: str):
    with urllib.request.urlopen(url, timeout=10) as r:
        return json.loads(r.read().decode())


def post_form(url: str, form: dict, headers: dict | None = None):
    data = "&".join(f"{k}={urllib.parse.quote(str(v))}" for k, v in form.items())
    req = urllib.request.Request(url, data=data.encode(), method="POST",
                                 headers=headers or {})
    req.add_header("Content-Type", "application/x-www-form-urlencoded")
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode())


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: selftest.py <issuer-url>", file=sys.stderr)
        return 2
    issuer = sys.argv[1].rstrip("/")
    failed = 0

    def check(ok: bool, what: str):
        nonlocal failed
        print(("  ok   " if ok else "  FAIL ") + what)
        if not ok:
            failed = 1

    # 1. discovery
    disc = get_json(issuer + "/.well-known/openid-configuration")
    check(disc.get("issuer") == issuer, f"discovery: issuer is {issuer}")
    token_ep = disc.get("token_endpoint", "")
    jwks_uri = disc.get("jwks_uri", "")
    check(token_ep.startswith(issuer), f"discovery: token_endpoint {token_ep}")
    check(jwks_uri.startswith(issuer), f"discovery: jwks_uri {jwks_uri}")
    check("client_credentials" in disc.get("grant_types_supported", []),
          "discovery: client_credentials supported")

    # 2. JWKS
    jwks = get_json(jwks_uri)
    keys = [k for k in jwks.get("keys", [])
            if k.get("kty") == "oct" and k.get("alg") == "HS256"]
    check(len(keys) == 1, "jwks: exactly one HS256 oct key")
    if not keys:
        return 1
    key = b64url_decode(keys[0]["k"])
    kid = keys[0].get("kid", "")

    # 3. token
    status, body = post_form(token_ep, {
        "grant_type": "client_credentials",
        "client_id": "selftest",
        "scope": "gate:operate",
    })
    check(status == 200, f"token: HTTP {status}")
    check(body.get("token_type") == "Bearer", "token: token_type Bearer")
    tok = body.get("access_token", "")
    parts = tok.split(".")
    check(len(parts) == 3, "token: three JWS segments")
    if len(parts) != 3:
        return 1
    header = json.loads(b64url_decode(parts[0]))
    claims = json.loads(b64url_decode(parts[1]))
    sig = b64url_decode(parts[2])
    check(header.get("alg") == "HS256" and header.get("kid") == kid,
          "token: header alg HS256, kid matches the JWKS")
    want = hmac.new(key, f"{parts[0]}.{parts[1]}".encode(), hashlib.sha256).digest()
    check(hmac.compare_digest(sig, want),
          "token: HMAC-SHA256 signature verifies against the JWKS key")
    check(claims.get("iss") == issuer, "claims: iss is the issuer")
    check(claims.get("sub") == "selftest", "claims: sub is the client")
    check(claims.get("scope") == "gate:operate", "claims: scope round-trips")
    check(isinstance(claims.get("exp"), int) and isinstance(claims.get("iat"), int)
          and claims["exp"] > claims["iat"], "claims: exp > iat")
    check(body.get("expires_in") == claims.get("exp", 0) - claims.get("iat", 0),
          "token: expires_in equals exp - iat")

    # 4. the signature check can go red
    bad = bytearray(sig)
    bad[0] ^= 0x01
    check(not hmac.compare_digest(bytes(bad), want),
          "control: a tampered signature does not verify")

    # 5. refusals are refusals
    status, body = post_form(token_ep, {"grant_type": "password",
                                        "client_id": "selftest"})
    check(status == 400 and body.get("error") == "unsupported_grant_type",
          "refusal: grant_type=password is 400 unsupported_grant_type")
    status, body = post_form(token_ep, {"grant_type": "client_credentials"})
    check(status == 401 and body.get("error") == "invalid_client",
          "refusal: no client is 401 invalid_client")

    return failed


if __name__ == "__main__":
    sys.exit(main())
