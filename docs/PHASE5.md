# Phase 5 — identity and credential propagation

A bridge authenticates to a legacy target with **its own** credentials. The
target therefore sees `orbweaver` on every call, whoever asked. Every audit
entry names the same principal, and every authorization decision the target
makes is about the wrong subject.

That is the confused deputy, and an AI bridge is an unusually attractive one:
trusted, long-lived, reachable by many callers. `docs/PLAN.md` §4.8.

브릿지가 **자기** 자격증명으로 인증하면 대상은 누가 요청했든 `orbweaver`만 본다.
감사 기록은 전부 같은 주체를 가리키고, 인가 판단은 잘못된 주체에 대한 판단이 된다.

---

# Batch 1: the wire, the trust boundary, and an honest measurement

```
identity propagation — what a real target says about security
  ok   omniORB 4.3.4 advertises no CSIv2: the bridge is the only enforcement point
  ok   JacORB 3.9 advertises none either — two peers, same answer
  note CSIv2 encoding is unit-tested in both byte orders; no peer here enforces it,
       so interop remains a per-peer claim and is unmeasured
```

## The measurement is the point of this batch

§4.8 predicted that many legacy targets have no authentication at all. Both
project fixtures were asked, on real IORs: **neither advertises a
`TAG_CSI_SEC_MECH_LIST` at all.**

That is not a gap to work around. It is the deployment reality the design has to
be honest about, and it decides what the bridge may claim:

> Where the target cannot enforce, the bridge is the only enforcement point and
> must say so in the catalogue. Asserting an identity the target ignores is
> theatre, and documenting it as a security control would be worse than leaving
> it out.

So `Assertion::RecordedOnly` is a named outcome rather than a degraded
`Assert`, it carries the reason, and `service_context` returns **no context at
all** in that case. Sending `ITTAnonymous` instead would claim a caller who
declined to be named — a different statement, and an untrue one.

두 픽스처 모두 CSIv2를 **전혀 광고하지 않는다.** 이것은 우회할 공백이 아니라 설계가
정직해야 할 배포 현실이다. 대상이 강제할 수 없는 곳에서 브릿지가 유일한 강제 지점이며,
대상이 무시하는 신원을 주장하는 것은 연극이다.

## Three things travel and they are not the same thing

| Layer | Question | Where it lives |
| --- | --- | --- |
| Transport identity | which process is connected? | mTLS / SSLIOP — not yet |
| Caller identity | on whose behalf? | `CSI::IdentityToken` in the SAS context |
| The bridge's own credential | who is this ORB? | GSSUP in the same message |

The last two travel in **one** `EstablishContext` and mean different things. A
test asserts both survive independently, because collapsing them is how a bridge
ends up asserting its own name as the caller.

## Credential hygiene is structural, not remembered

§4.8 requires credentials be excluded from diagnostics *by construction*.
`GssUpToken` implements `Debug` by hand and prints `<redacted>`; there is no way
to obtain a password through a formatter, so no future `{:?}` in a log line can
leak one. A test formats it with `{:?}` and `{:#?}` and asserts the password is
absent and the username is present — the username is what an audit entry needs.

The audit line takes a `&Caller` and an `&Assertion` and nothing else. There is
no argument it could be handed that carries credential material. A certificate
chain is described (`<x509 chain, 900 bytes>`), never reproduced.

자격증명 위생을 **기억이 아니라 구조**로 만들었다. `{:?}`를 쓰지 않기로 하는 약속은
통제가 아니다.

## Delegation is default-deny, per interface, with a recorded reason

Getting exposure wrong shows an agent something it should not see. Getting this
wrong makes a **target act on an identity nobody authorised**, so the reason is
required rather than optional and an empty one is refused: a decision with no
recorded reason is indistinguishable from an accident six months later.

Permission for one interface does not spread to its neighbour, and is never
inherited from the caller having been trusted enough to connect.

## Expiry is checked first, and unconditionally

§4.8's fourth discomfort: CORBA connections are long-lived by design and tokens
expire by design. A lapsed credential is refused **even against a target that
could not have checked it** — the check is ours to make, and a call must not
proceed quietly on an expired context. The test asserts this in both the
advertised and the un-advertised case, because the tempting shortcut is to skip
it where nobody would notice.

## Two decoding refusals worth naming

- **An unknown identity token type is refused, not skipped.** The union arm's
  payload shape depends on the discriminator, so reading on would produce a
  principal name out of whatever bytes followed.
- **A GSS token for another mechanism is refused, not reinterpreted.** Same
  reason: the body has a different shape, and a username parsed out of a
  Kerberos token is a username the bridge would then assert.

Declared counts are validated against the bytes present, and every truncation of
an advertisement is fed to the parser — the four-byte-field-buys-a-gigabyte
shape from the Phase 0 audit, arriving through a new door.

## What is not verified

**No peer here enforces CSIv2**, so nothing in this batch has been through a
target that checks it. The encoding is unit-tested against the specification in
both byte orders and that is all it is. §4.8 says to treat CSIv2 support as a
per-peer claim rather than a feature, and this batch does not make the feature
claim.

**여기서 CSIv2를 강제하는 피어는 없다.** 인코딩은 규격에 대해 양쪽 바이트 순서로
단위 시험했고 그게 전부다. §4.8이 말한 대로 CSIv2 지원은 기능이 아니라 피어별 주장으로
남는다.

Also absent: SSLIOP and mTLS (transport identity), token exchange from OAuth2 or
JWT into a principal — the `Caller` type is the seam it will attach to — and
matching scopes against `@ai_authz`. The `Approval` the MCP bridge still hardcodes
to "not approved" is the same seam: a host that authenticates its caller can now
produce a `Caller`, and that is what the approval channel will carry.

---

# Batch 2: SSLIOP groundwork, and D002 decided

Stream C's second batch (parallel wave 2), plus the decision that unblocks the
third.

## See the endpoint before deciding how to dial it

`orbweaver-giop/src/ssliop.rs` parses `TAG_SSL_SEC_TRANS` (ComponentId 20):
`target_supports`/`target_requires` (the same `CSI::AssociationOptions` bits as
csiv2 — reused, not redefined) and the SSL port, which replaces the cleartext
port at the same host. Absence is `None` and not an error — the measured
common case — but an *unreadable* component is `Some(Err)`, because silently
ignoring it would downgrade to cleartext. Port 0 is handled as the deployed
convention ("same port") and labeled convention, not spec. `spike-dump` prints
the advertisement per IOR, and the harness records the baseline: **neither
fixture advertises TAG_SSL_SEC_TRANS**, so TLS work starts from a measured
fact.

읽을 수 없는 컴포넌트는 `Some(Err)`다 — 조용히 무시하면 평문으로 강등되기 때문이다.
포트 0은 규격이 아니라 배포 관례로 처리하고 그렇게 표기했다.

## D002: crypto is depended on for honesty, not licensing

Approved 2026-08-13 ("승인, 진행"): rustls under the **MIT arm** of its
`Apache-2.0 OR ISC OR MIT` triple licence, default provider aws-lc-rs, behind
an off-by-default `ssliop` feature, disclosed in NOTICE with the same testable
promise as encoding_rs.

The argument that matters: first-party TLS is ruled out **by honesty, not by
licensing**. GIOP we implement ourselves because our oracles catch a wrong
implementation — a broken handshake, by contrast, interops perfectly and our
oracle would never see it. Crypto whose failure modes our oracles cannot
detect is depended on, not written. Verified from shipped crate tarballs: the
advertising-clause OpenSSL/SSLeay text is gone from both candidate providers
as shipped; the residual provenance risk (upstream's Apache-2.0 assertion over
1995–1998 SSLeay-era files) is named in D002 as relied-upon, not verified.

**직접 만들지 않는 이유는 라이선스가 아니라 정직성이다.** 깨진 핸드셰이크는 완벽히
상호운용되므로 우리 오라클이 결코 보지 못한다 — 오라클이 실패를 감지할 수 없는
로직은 의존하지, 작성하지 않는다.

---

# Batch 3: the token exchange as a seam, and the scope gap made loud

Stream C's remaining half. Two halves landed and one is a refusal to build
something.

## The verifier is a trait with no implementation, and that is the design

`Verifier` takes a `&Secret` and returns `VerifiedClaims`. **This crate ships
no implementation.** The shipped path is `Exchange::caller_for(&VerifiedClaims,
now)`, which takes claims a host verified in *its own process* — so on that
path the crate never holds the token at all, which is the strongest form of the
credential-hygiene rule above.

A first-party verifier was costed and refused: RSA-PKCS#1-v1.5 / ECDSA-P256,
SHA-256, ASN.1 DER, base64url, JWKS fetching and rotation, `iss`/`aud`/`nbf`,
`alg`-confusion and the `none` algorithm, clock skew — which would also put the
first clock in this crate, inside a gate.

The decisive argument is batch 2's, running the other way. TLS was depended on
rather than written because a broken handshake interoperates perfectly and our
oracles never see it. A **verifier** fails in the same invisible direction and
worse: one that is wrong in the *accepting* direction interoperates perfectly
with every honest token **and also accepts a forged one**, and no oracle this
project owns can tell those apart. So the seam stays, and adopting a JWS crate
would be a decision document rather than a batch.

**검증기를 직접 쓰지 않는 이유도 정직성이다.** 받아들이는 방향으로 틀린 검증기는
정직한 토큰과 완벽히 상호운용하면서 위조 토큰도 받아들이며, 우리가 가진 어떤
오라클도 그 차이를 보지 못한다.

## The scope vocabulary gap, reported before a call

A token's scopes are the identity provider's vocabulary; `ai_authz` scopes are
the contract's. Nothing mapped one to the other, and D005 measured what that
costs: a contract asking for `parkinglot.barrier.open` while the requirement —
and therefore the IdP — says `gate:operate` refuses **every legitimate caller**,
and reads as a permissions misconfiguration rather than a generation defect.

`ScopeMap` is default-deny (passing unknown scopes through would hand the
identity provider authority over the contract) and `ScopeMap::audit` ranks three
findings. The one that matters is **unsatisfiable**: a contract scope that is
not in the map's image at all, so no token this deployment issues can ever
satisfy it. Each names the `(target, operation)` pairs that go dark, so it reads
as an outage rather than a warning, and the process **exits 3**.

An operator meets it where they already look — the dry-run survey grows a
`scope_map` section — and with no mapping configured the document is
byte-for-byte what it always was, which is asserted rather than assumed.

## Expiry, with no clock added

`Exchange::caller_for` takes `now`; the mid-session gate sits at `SEAT_EXPIRY`,
ahead of every other gate, and is stamped by the host per request — the same
discipline as D004's `ts` and the quota window. Neither of the two "the host
said nothing" cases is a default: claims with no `exp` are refused unless the
deployment declared an unbounded lifetime **with a reason**, and an unstamped
gate refuses every caller that carries an expiry, because *cannot tell* must
never render as *still valid*.

## What is not verified

**Nothing here has been through a real identity provider.** The exchange is
unit-tested against hand-built claims. That is the same shape as batch 1's
honest limit — CSIv2 remains a per-peer claim rather than a feature — and it is
the gap a pilot closes, not a batch.

**실제 IdP를 거친 적이 없다.** 손으로 만든 클레임에 대한 단위 시험뿐이며, 이는
파일럿이 닫을 공백이지 배치가 닫을 공백이 아니다.
