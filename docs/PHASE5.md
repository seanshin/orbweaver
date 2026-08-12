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
