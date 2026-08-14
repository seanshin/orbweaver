# Phase 6 — deployment reality

The ORB works on the wire. Whether it works *where it will be deployed* is a
different question, and the first part of it is addressing: an IOR carries
addresses, and a server puts into it the address it believes it has. Inside a
container that belief is the container's address; behind a load balancer it is
nobody's. Every returned reference is then dead on arrival.

ORB는 자신이 가졌다고 *믿는* 주소를 광고한다. 컨테이너 안에서 그 믿음은 컨테이너의
주소이고, 로드밸런서 뒤에서는 아무의 주소도 아니다. 그러면 반환되는 모든 참조가
도착 즉시 죽는다.

`docs/PLAN.md` §9.1 **R7**.

---

# Batch 1: IOR endpoint rewriting under NAT and containers

```
R7 — IOR endpoint rewriting, measured by dialing
  ..   claimed address 1: 172.30.1.45 (a real address on this machine, nothing serving there)
  ..   claimed address 2: 10.244.3.17 (no route from here; the client hangs, as in assumption D)
  | servant bound at 127.0.0.1:51048 (object key "nat-servant")
  |   ok   control: ping() -> 42 at the address it really bound
  |
  | claimed address 172.30.1.45:51048 — an ORB that believes it is there
  |   ok   unrewritten IOR did not dial after 0.00s: all 2 endpoint(s) failed; last: io: Connection refused (os error 61)
  |   ..   map 172.30.1.45:51048=127.0.0.1:51048 → 2 profile(s): 1 IIOP, 1 preserved unread; 2 endpoint(s) rewritten, 0 unmapped, 0 alternate(s) dropped, 0 malformed alternate(s)
  |   ok   rewritten IOR completed ping() -> 42
  |   ok   untouched: object key, IIOP version, type id, 2 profiles including the one we cannot read
  |
  | claimed address 10.244.3.17:51048 — an ORB that believes it is there
  |   ok   unrewritten IOR did not dial after 6.00s: all 2 endpoint(s) failed; last: io: connection timed out
  |   ..   map 10.244.3.17:51048=127.0.0.1:51048 → 2 profile(s): 1 IIOP, 1 preserved unread; 2 endpoint(s) rewritten, 0 unmapped, 0 alternate(s) dropped, 0 malformed alternate(s)
  |   ok   rewritten IOR completed ping() -> 42
  |   ok   untouched: object key, IIOP version, type id, 2 profiles including the one we cannot read
  |
  | publish time — a servant bound wide, as a container binds
  |   ok   with no map, publishing is refused rather than wrong: bad IOR: bound to a wildcard
  |        address and no rule publishes it; an IOR must name an address a client can dial
  |   ..   map 0.0.0.0:51080=127.0.0.1:51080 → published 127.0.0.1:51080
  |   ok   the published reference completed ping() -> 42

container probe — a client in another routing domain
  skip Docker is not available here; spikes/nat/ is written and UNRUN

  failures: 0   unmeasured (skipped): 1
```

`./spikes/nat_rewrite.sh`. Implementation: `crates/orbweaver-giop/src/nat.rs`,
`Server::ior_mapped`, `spike-nat`.

## The measurement, not the assertion

Phase 0 assumption D recorded the hazard on a stock ORB and the harness still
prints it every run: *"confirmed a routable-but-local address is published, not
loopback (risk R7 is real)"*. What had never been shown is the other half — an
IOR that **fails to dial**, and the same IOR, rewritten, completing a call.

A unit test cannot supply that half. It can show a rewrite produces the fields
somebody expected, which is exactly how a plausible-but-wrong rewrite passes:
nothing in it ever opens a socket. So the check dials. The unrewritten
reference must fail, and both failure modes deployment actually produces are
exercised:

| Claimed address | What happens | Which deployment this is |
|---|---|---|
| `172.30.1.45` — a real address on this machine, nothing serving there | **Connection refused**, immediately | The load-balancer / wrong-interface case |
| `10.244.3.17` — no route from here | **Timed out** after the full dial budget | The container case; assumption D's own prefix |

The second row is why `Connection::connect` needs a timeout at all, which
assumption D also predicted. Six seconds for two endpoints at a three-second
budget: an IOR whose *alternates* are internal too costs the client the budget
twice before it gives up.

단위 시험은 올바른 재작성과 그럴듯한 재작성을 구별하지 못한다 — 소켓을 열지 않기
때문이다. 그래서 이 검사는 실제로 전화를 건다. 재작성 전 참조는 반드시 실패해야
하고, 배포에서 실제로 나타나는 두 실패 모드(즉시 거부, 타임아웃)를 모두 확인한다.

## The rules, and what is refused

| Field | Disposition | Why |
|---|---|---|
| Profile `host`/`port` | Rewritten | It is the address, and the address is what is wrong |
| **Every** profile, not the first | All rewritten, order and count preserved | Failover dials them in order; one unrewritten profile costs a full connect timeout before the good one is reached |
| `TAG_ALTERNATE_IIOP_ADDRESS` | Rewritten by the same map | `IiopProfile::endpoints()` dials alternates too. Rewriting only the profile address leaves a client hanging on an internal one — the same mistake `spike-failover` made twice in CI, from the other direction |
| A malformed alternate | Kept verbatim, counted | A bad hint is skipped, never fatal and never silently replaced by something well-formed nobody wrote |
| An alternate no rule matched | Kept, unless `drop_unmapped_alternates` | A route is a thing to lose carefully; the option exists because a stale internal alternate costs every client a timeout |
| `object_key` | **Never touched** | It is the servant's identity, not a route. Altering it turns "the wrong address" into "the wrong object", which fails later and further away |
| IIOP `version` | **Never touched** | The version is the peer's capability statement (§9.4.1). Rewriting it makes the client speak a protocol the server never claimed |
| `type_id` | **Never touched** | Interface identity |
| A profile tag we do not decode | **Preserved byte-for-byte** | §9.7.2. Dropping a profile a client could have used is worse than not rewriting at all |
| A profile's own address, unmapped | Kept | Dropping it would delete the profile. A rewriter that can delete profiles is the failure this module refuses |

Two properties hold the rest together, and both are tested rather than
asserted: **an empty map is byte-identical on the wire** (a profile that did
not move is re-emitted as its original bytes, so "the map matched nothing" is
distinguishable from "the map did something"), and **byte order is preserved**
on the outer encapsulation, each profile and each rewritten component.

빈 맵은 와이어에서 바이트 동일해야 하고, 바이트 순서는 바깥 캡슐화·각 프로파일·
재작성된 컴포넌트 모두에서 보존된다. 둘 다 주장하지 않고 시험한다.

## Rewriting does not go through `Ior`, and here is the measurement

`Ior` is the **dialing** view: `Ior::read_from` keeps `TAG_INTERNET_IOP`
profiles and discards every other tag, because it answers "where do I
connect". That makes it lossy, and a rewriter built on it would delete a
profile from every reference that carried one — silently, and exactly the
failure mode that is worse than not rewriting.

This is measured, not assumed: `ior_drops_a_profile_it_does_not_speak` builds a
two-profile reference, parses it both ways, and shows `Ior` returning one
profile while `nat::RawIor` returns two. Rewriting therefore runs on `RawIor`,
which keeps every profile as `(tag, body)` and decodes only the IIOP ones.

`Ior`는 다이얼용 뷰라 우리가 말하지 못하는 프로파일을 버린다. 이것을 시험으로
측정했고, 그래서 재작성은 무손실 표현인 `RawIor` 위에서 동작한다.

## Publish time or read time

Both are implemented, because both exist in the field:

- **Publish time** — `Server::ior_mapped(type_id, &map)` runs the *bound*
  address through the map. `spike-nat serve` reads the map from
  `ORBWEAVER_PUBLISH_MAP`, so a container carries the rewrite in its manifest
  rather than in an operator's head.
- **Read time** — `nat::rewrite_stringified(ior, &map)` repairs a reference a
  client received.

**This project should prefer publish time.** The argument is about who reads
the result:

1. **A foreign client cannot be patched.** A reference we publish is read by
   omniORB, JacORB and TAO clients that will never run our code. Read-time
   rewriting fixes the clients we control, which in a bridge deployment is the
   minority of them.
2. **One place, not one per client.** Publish-time configuration is O(servers);
   read-time is O(clients), and the one client that missed the memo fails in a
   way that looks like the server being down.
3. **References escape the reference.** An object reference arrives in a reply
   body, a `LOCATION_FORWARD`, a naming-service `resolve`, an `Object`-typed
   `out` parameter. To be complete, read-time rewriting has to intercept every
   unmarshalled reference — a hook inside `Ior::read_from` and everything that
   calls it — which is a much larger surface than one call at publish.
4. **Publish time cannot damage identity by construction.** It builds a
   reference from the servant's own key and version; it never re-parses
   somebody else's IOR, so there is nothing to get wrong except the address.

What read time is genuinely for, and why it stays: **brownfield**. A legacy
server that cannot be reconfigured publishes what it publishes, and the only
place left to fix it is the client — this is precisely the bridge's situation
in front of a target it does not own. Read time is also the only side that can
express **split horizon**, where two clients on different networks need
different answers for the same server; a publish-time rewrite has to cover that
case with several profiles or alternates instead.

**What full read-time support would require**, beyond the function that exists:
a rewrite policy carried on the ORB or the `Connection`, applied at every point
a reference is unmarshalled (reply bodies, forwards, naming results), plus a
trust decision — rewriting an address a peer gave you is a redirection
primitive, so a broad map (`*`) applied to a foreign reference points the caller
wherever the map's author chose. That is why the read-time entry point is an
explicit call on a reference the caller has already decided to trust, and not a
hook inside the decoder.

두 방식 모두 구현했다. **이 프로젝트는 publish 시점을 선호해야 한다** — 외부 ORB
클라이언트는 우리가 고칠 수 없고, 설정 지점이 서버 수만큼이지 클라이언트 수만큼이
아니며, 참조는 응답 본문·`LOCATION_FORWARD`·네이밍 결과 어디로든 새어 나오기
때문이다. read 시점은 **재설정할 수 없는 레거시**와 **split horizon**을 위해 남긴다.
읽은 참조를 재작성하는 것은 리디렉션 원시연산이므로, 디코더 안의 훅이 아니라
호출자가 신뢰를 결정한 참조에 대해 명시적으로 부르는 함수다.

## `0.0.0.0` is bindable and unpublishable

The one thing publish-time rewriting **refuses**: a wildcard bind that no rule
names. `0.0.0.0` binds happily and cannot be dialed, so an ORB that publishes
what it bound emits references that fail at every client instead of at the one
process that could still have been configured. `ior_mapped` returns an error
there rather than a default. An address that is *not* a wildcard and that no
rule names is published unchanged — a deployment with no NAT in front of it
sets no map and must still get a working reference.

## What was measured, and what was not

**Measured.** On real sockets, in one process: an unrewritten reference failing
to dial in both failure modes; the rewritten reference completing a real
`ping() -> 42`; the object key, IIOP version, type id, profile count and an
undecodable profile's bytes all surviving; the publish-time path producing a
dialable reference from a wildcard bind, and refusing to produce one without a
map. Plus 21 unit tests over the rules, the parser and the encoding fidelity —
19 in `nat`, 2 on the publish-time path — taking the crate from 167 to 188.

**Not measured: a NAT boundary.** There is no NAT on this machine. What makes
the claimed address unusable here is that the servant is not listening on it,
where in a container it would be the namespace boundary. The mechanism is the
same — an IOR naming an address the client cannot be served at — and the
demonstration is weaker than a container's. Saying otherwise would be claiming
the fix works where it has not been tried.

**Not measured: the container probe.** `spikes/nat/` puts the servant and the
client on separate Docker networks, so the servant's own address is genuinely
unreachable from the client and only the published one works. It is written,
and it has **never been run** — this machine has no Docker (`docker: command
not found`). It is a counted SKIP in `nat_rewrite.sh`, never a pass, and the
scripts say so in their own headers. The first person to run it should expect
to fix it rather than to confirm it.

**Not measured: a foreign ORB reading a rewritten reference.** Every dial here
was ours. omniORB reading an IOR we rewrote is the natural next check and it
has not been done.

**측정한 것**: 실제 소켓 위에서 재작성 전 실패(두 가지 실패 모드) → 재작성 후
실제 호출 성공, 그리고 주소 외에는 아무것도 바뀌지 않았다는 점.
**측정하지 않은 것**: NAT 경계 자체(이 기계에는 NAT가 없다), 컨테이너 프로브
(Docker 없음 — 작성했고 **실행한 적 없음**, 통과가 아니라 건너뜀으로 계수), 그리고
외부 ORB가 재작성된 참조를 읽는 경우.

## The harness group this wants, not yet applied

`spikes/run_checks.sh` is not edited by this batch. The group it should gain,
after the existing assumption D block:

```bash
# ── R7 — endpoint rewriting on our own ORB ───────────────────────────────────
hr "R7 — IOR endpoint rewriting (ours)"
# Capture then match: `grep -q` closes the pipe and SIGPIPEs the producer.
nat=$(./spikes/nat_rewrite.sh 2>&1)
printf '%s\n' "$nat" > /tmp/orbweaver-r7.log
if printf '%s' "$nat" | grep -q "nat rewriting: PASS"; then
  echo "  ok   unrewritten IORs did not dial; rewritten ones completed a call"
  # The container probe is unmeasured wherever Docker is absent, and an
  # unmeasured check is counted, never silently passed.
  if printf '%s' "$nat" | grep -q "spikes/nat/ is written and UNRUN"; then
    echo "  skip container probe (no Docker here) — R7 across a real routing domain is unmeasured"
    skipped=$((skipped+1))
  fi
else
  echo "  FAIL see /tmp/orbweaver-r7.log"
  fail_total=$((fail_total+1))
fi
```

It takes about seven seconds, most of it the deliberate connect timeout on the
unroutable address.

## Known limits

- **IPv6 literals must be bracketed** in a map specification (`[fd00::1]:5555`).
  An unbracketed one is refused with that message rather than mis-split.
- **No CIDR matching.** Rules are exact hosts or `*`. A pod range needs `*` plus
  the knowledge that every address in the reference belongs to this deployment.
- **`TAG_SSL_SEC_TRANS` carries a port** (`ssliop.rs`) and the map does not
  rewrite it. A deployment that NATs the TLS port to a different number outside
  is not covered; the cleartext port and host are.
- **`Ior` remains lossy.** Making it lossless means adding a field to a public
  struct that other crates build with literal syntax, which is outside this
  batch's footprint. `RawIor` is the lossless path; `Ior`'s documentation now
  says which view it is.
