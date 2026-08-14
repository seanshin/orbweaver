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

> Batch 2 went back at this. The diagnosis above turned out to be true and
> shallow — the blocker was not the absent engine but the disk underneath it —
> and the routing boundary was then obtained a different way, on a real second
> host. The container probe is still unrun; **R7 across a routing domain is
> not**. See batch 2.

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

> Superseded by the snippet at the end of batch 2, which counts two skips
> rather than one. This one is left as the record of what batch 1 proposed.

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

---

# Batch 2: the probe executes, on a second host

Batch 1 left one thing unmeasured and named it plainly: **a client in another
routing domain**. This batch went to run it, and it ran — not in the container
the files were written for, but on a real second host, which is what R7 needed
all along and what a container was only ever a way of obtaining.

배치 1이 남긴 미측정 항목은 **다른 라우팅 도메인의 클라이언트** 하나였다. 이번
배치에서 그것이 **실행되었다** — 준비해 둔 컨테이너가 아니라 실제 두 번째 호스트
위에서. R7이 필요로 한 것은 애초에 컨테이너가 아니라 라우팅 도메인이 다른
클라이언트였고, 컨테이너는 그것을 얻는 한 가지 방법이었을 뿐이다.

```
R7 across a real routing boundary — a second host, not a simulation
  ..   servant runs on the host; client runs in r7client (192.168.252.4)
  ..   the host is 192.168.252.1 from inside the VM — a different routing domain
  ..   guest is aarch64; cross-compiling the client for aarch64-unknown-linux-musl

the reference an ORB publishes when it believes it is at loopback
  ..   naive: servant bound 0.0.0.0:15555, published 127.0.0.1:15555
       dialing 127.0.0.1:15555
         FAIL all 1 endpoint(s) failed; last: io: Connection refused (os error 111) after 0.00s
  ok   naive: the client could not dial it, as R7 predicts

the same servant, published through an endpoint map
  ..   published: servant bound 0.0.0.0:15555, published 192.168.252.1:15555
       dialing 192.168.252.1:15555
         ok   ping() -> 42 in 0.43s
  ok   published: the call completed from the other routing domain

identity across the two references
  ok   object key "nat-servant" and type id "IDL:spike/Echo:1.0" appear verbatim in both
  ..   profile count, IIOP version and undecodable profiles: see spike-nat prove

verdict
  failures: 0
  vm routing-domain probe: PASS
```

`./spikes/nat/vm/run.sh`. Five consecutive passing runs, one earlier failure
discussed below. macOS 26 (Darwin 25.6.0), multipass 1.16.3+mac with the qemu
driver, Ubuntu 24.04 guest.

## The experiment, and why it is shaped the way it is

The servant binds `0.0.0.0:15555` on the **host** and is therefore genuinely
reachable from the guest at the bridge address **in both cases**. The only
thing that changes between them is the address written into the reference:

| Case | Published | From the guest |
|---|---|---|
| naive | `127.0.0.1:15555` — what an ORB believes it has behind any boundary | **Connection refused** immediately: in the guest's routing domain that names the guest |
| published | `192.168.252.1:15555` via `ORBWEAVER_PUBLISH_MAP` | `ping() -> 42` |

Holding the servant reachable in both cases is the whole design. It isolates
the variable: the naive case does not fail because the server is missing or
because a network is partitioned, it fails because *the reference names an
address that means something else where the client is standing*. That is R7
with nothing else in the frame — and it is a cleaner isolation than the
container probe was ever going to give, where the servant genuinely is
unreachable and "wrong address" and "no route" are confounded.

This is also the failure mode a single host cannot fake. Batch 1's
`172.30.1.45` failed because nothing was listening there; `10.244.3.17` failed
because the packet went nowhere. Neither is *this*: here the address is real,
routable and in use on both sides of the boundary, and it resolves to a
different machine depending on who reads the reference. That is the container
case exactly, and it is the one an ORB gets wrong by default.

서번트는 두 경우 모두 게스트에서 **실제로 도달 가능한** 상태로 유지된다. 달라지는
것은 참조에 적힌 주소뿐이다. 그래서 naive 실패는 서버가 없어서도, 네트워크가
끊겨서도 아니고, **참조가 가리키는 주소가 클라이언트가 서 있는 곳에서는 다른 것을
뜻하기 때문**이다. 배치 1의 두 실패 모드(거부·무경로)와 달리 여기서 그 주소는
양쪽 모두에서 실재하고 라우팅되며, 읽는 쪽에 따라 다른 기계를 가리킨다 — 컨테이너
사례 그 자체다.

## What the environment probe found on the way, which is worth keeping

The first attempt did not get a VM at all, and diagnosing that produced a
finding that outlives it. Batch 1 recorded `docker: command not found`. That is
true and shallow — it cannot distinguish an engine that is absent from one that
is installed and dead, nor from a machine where no engine could be installed.

| Probed | Result at first attempt |
|---|---|
| `docker`, `podman`, `nerdctl`, `finch`, Apple `container` | none installed |
| `kubectl`, `kind`, `minikube`, `k3d`; `~/.kube` | none installed; no kubeconfig at all |
| `colima`, `lima`, `krunvm`, `tart` | none installed |
| `multipass` | **installed** (1.16.3+mac, qemu driver), no instances |
| `multipass launch` | downloaded the image, then **refused**: `launch failed: Available disk (974639104 bytes) below minimum for this image (3758096384 bytes)` |
| `df -k /` | 460 GiB volume at **100 % capacity**, 526–940 MiB free |
| `sudo -n true` | `sudo: a password is required` |
| network namespaces | macOS has none |

So the blocker was one layer below where batch 1 put it: a hypervisor **was**
installed, and what was missing was about 3.5 GiB of disk. Installing Docker or
Colima would not have helped — both are a Linux VM in a trench coat and want a
multi-gigabyte disk image of their own, the same wall reached by a longer route.

Disk freed up later in the session (to ~15.6 GiB) and the launch then
succeeded. **That does not make the finding transient**, it makes it
conditional, which is exactly what a preflight is for:
`spikes/nat/preflight.sh` measures all of the above on whatever machine it is
run on and exits non-zero when no probe can run. That is the codified form —
the next person runs one script instead of repeating the investigation.

The privileged escapes were closed and would not have been enough anyway:
`sudo` wants a password, so `ifconfig lo0 alias`, a `pf` rule and a fresh
`utun` are all unavailable — and none of the three is a routing *domain*. An
extra address on `lo0` is another address in the one namespace the host has,
which is what batch 1 already measured with `172.30.1.45`.

배치 1의 진단은 사실이지만 얕았다. 하이퍼바이저는 **설치돼 있었고**, 없던 것은
디스크였다(루트 볼륨 100 % 사용, 여유 526–940 MiB, 최소 이미지 요구 3.5 GiB).
세션 도중 디스크가 확보되어 실행에 성공했다. 이는 발견을 무효화하지 않고
**조건부**로 만들 뿐이며, 그래서 `spikes/nat/preflight.sh`로 고정했다.

## The guest has no network, and that made the probe better

The first provisioning attempt built `spike-nat` inside the VM and hung: this
host's VPN eats the guest's NAT, so the guest reaches the host at the bridge
address (0 % packet loss) and reaches nothing beyond it. `apt` and `rustup`
both time out.

The fix was to stop needing a guest network: the client is **cross-compiled on
the host** for `aarch64-unknown-linux-musl` and copied in. musl and `rust-lld`
between them mean no C toolchain on either side — a statically linked guest
binary out of macOS with nothing installed but a rustup target.

That is a better probe than the one originally written, for a reason worth
stating: **the only traffic left in the VM is the traffic being measured.** A
probe whose setup needs the network it is testing has a failure mode where
setup trouble looks like the measurement failing.

게스트에는 외부 네트워크가 없다(호스트 VPN이 NAT를 먹는다). 그래서 클라이언트를
호스트에서 **교차 컴파일**해 넣는다. 결과적으로 더 나은 프로브다 — VM 안에 남은
트래픽이 측정 대상 트래픽뿐이기 때문이다.

## The one failure, reported rather than explained away

The very first run failed its published case:

```
  ..   published: servant bound 0.0.0.0:15555, published 192.168.252.1:15555
       dialing 192.168.252.1:15555
         FAIL io: Resource temporarily unavailable (os error 11) after 3.10s
```

`EAGAIN` at 3.10 s is the dial budget expiring on a non-blocking connect. It
has **not reproduced in five subsequent runs**, three of them back-to-back for
that purpose. The macOS application firewall is enabled on this machine
(`socketfilterfw --getglobalstate` → `State = 1`) and a freshly built unsigned
binary accepting its first inbound connection is the obvious suspect, since the
prompt it raises has nobody to answer it.

**That is a suspicion, not a diagnosis.** This project's rule is that a
transient is not diagnosed until it reproduces and a fix makes it stop, and
neither happened here. "Did not reproduce in five runs, leading suspect
recorded" is the honest state, and anyone running this on a machine with the
firewall enabled should expect it and should not read a first-run failure as a
refutation of R7.

첫 실행에서 published 경우가 한 번 실패했고(3.10 s 후 `EAGAIN`), 이후 다섯 번의
실행에서 재현되지 않았다. macOS 응용프로그램 방화벽이 켜져 있고 서명되지 않은
새 바이너리의 첫 인바운드 연결이 유력한 용의자지만, **진단이 아니라 의심**이다.

## What this run measured, and what it did not

**Measured, across a genuine routing boundary.** A reference naming an address
that is real on both sides and means a different machine on each: the dial is
refused. The same servant, same bind, same object key, republished through an
`EndpointMap`: `ping() -> 42`. The object key `nat-servant` and the type id
`IDL:spike/Echo:1.0` appear verbatim in both references.

**Not measured: full field preservation across the boundary.** The identity
check here compares two references for two strings. Profile count, IIOP
version, alternate addresses and an undecodable profile's bytes are covered by
`spike-nat prove` on the host and by the unit tests in `nat.rs` — not by this
run. `spike-nat`'s servant also does not override `Dispatch::knows`, so a
completed `ping()` is not by itself evidence that the object key was honoured.

**Not measured: port translation across the boundary.** Both cases keep port
15555. The `to_port` half of a rule is exercised by unit tests and by the
manifest check below, and has never been dialed. The Kubernetes probe is the
one that would, because a NodePort is not the port the servant bound.

**Not measured: a foreign ORB.** Both ends were ours. omniORB reading a
reference we rewrote remains the natural next check and remains undone.

**Not measured: a NAT.** A bridged VM is a routing boundary, not address
translation. Nothing here rewrote a packet header; the demonstration is that a
*reference* naming the wrong address fails and that repairing the reference
fixes it, which is R7's claim, but "NAT" in the risk's name is still doing
work no measurement has covered.

**측정한 것**: 양쪽에서 실재하지만 서로 다른 기계를 뜻하는 주소를 담은 참조는
다이얼되지 않고, 같은 서번트를 `EndpointMap`으로 다시 공표하면 `ping() -> 42`가
된다. **측정하지 않은 것**: 경계를 넘는 전체 필드 보존, 포트 변환, 외부 ORB,
그리고 NAT 자체.

## The Kubernetes half, written and unrun

`spikes/nat/k8s/` is new: a ConfigMap, a Deployment and a NodePort Service in
`manifests.yaml`, driven by `run.sh`. It reuses `spikes/nat/Dockerfile`
unchanged, because the entrypoint's `naive` and `published` modes are exactly
the two cases there as well. **It has never executed**, and every file says so
in its own header.

It is a **harder** case than either probe above, which is why it was worth
writing rather than transcribing:

1. **The published port is not the bound port.** A NodePort comes from
   30000–32767 while the servant binds 5555, so the reference must carry a
   translated port as well as a translated host — the `to_port` half of a rule,
   which neither the compose probe nor the VM probe reaches.
2. **The published address is the Service's, not the pod's.** The pod IP is the
   address the ORB believes it has, and it is *correct* — briefly, and only
   from inside, and it changes on every restart. The Service is the stable
   identity, and the map is how a servant is told which of its several true
   addresses is the one to hand out.

**The trap it is built to avoid.** Pod-to-pod networking is flat by default, so
an in-cluster client dials a pod IP perfectly well. A client Pod or Job would
therefore make the *naive* case **succeed**, and the probe would report a pass
having demonstrated nothing — the same shape as compose.yaml's "a run where
both succeed is a broken probe", but much easier to walk into, because putting
the client in the cluster is the obvious thing to do. (A NetworkPolicy does not
rescue it unless the CNI enforces one, and kind's default kindnet does not.) So
the client runs **outside**, on the host, and reaches the servant only through
the NodePort — which is also the path a real foreign CORBA client takes.

**The trap inside the driver.** Environment injected by `configMapKeyRef` is
read once, at container start: rewriting the ConfigMap does not reconfigure a
running pod. Without the `rollout restart` between cases the probe measures the
first case twice and cannot tell. That is written down in `run.sh` next to the
line that prevents it.

**What is expected to break first** is the node address. On Linux the node's
InternalIP works as-is; on macOS, kind and minikube put the node inside a VM
whose address the host cannot reach, so `ORBWEAVER_NODE_ADDR` must be supplied
(kind `extraPortMappings` with `127.0.0.1`, or `minikube service --url`). If it
is wrong, *both* cases fail to dial, and `run.sh` reports that as a failure
rather than as a naive-case pass — a probe whose control case does not work has
measured nothing.

`spikes/nat/k8s/`는 이번에 추가한 절반이고 **실행된 적이 없다**. 위의 두 프로브보다
어려운 경우를 다룬다: (1) NodePort라서 **포트까지** 변환되고, (2) 공표 주소가
파드가 아니라 **서비스**의 것이다. 피하려고 설계한 함정은 **클러스터 안의
클라이언트**다 — 파드 간 통신은 평평하므로 in-cluster 클라이언트는 naive 경우까지
성공시켜 아무것도 증명하지 않은 채 통과를 보고한다.

## What an unrun artefact is allowed to say about itself

Every file under `spikes/nat/` that has not executed states so in its own
header, and `nat_rewrite.sh` counts each as a skip rather than a pass. That is
the floor, and this batch went one step above it: **the part of the unrun
artefact that can be checked without the missing environment, is checked.**

`nat_rewrite.sh` reads the `publish-map` string **out of `manifests.yaml`** —
not a retyped copy, which would check the copy and not the file — substitutes
only the placeholder host, and runs it through the real publish path:

```
the cluster manifest's publish map — checked here, without a cluster
  ..   manifest carries 0.0.0.0:5555=REPLACE-WITH-A-NODE-ADDRESS:30555
  ..   checking 0.0.0.0:5555=127.0.0.1:30555 → expecting a reference naming 127.0.0.1:30555
  ok   the manifest's map is accepted and publishes 127.0.0.1:30555 (host and port both translated)
  ..   unmeasured here: whether anything answers there. That needs the cluster.
```

It **deliberately does not dial**. Nothing listens on `127.0.0.1:30555`. It
shows the configuration is well-formed and applied as intended and shows
nothing about reachability — the same distinction batch 1 drew about unit
tests, applied to a check that sits one step above a unit test and still below
a probe.

An unrun manifest presented as a tested one is the failure this project has a
rule about. An unrun manifest whose configuration has been machine-checked, and
which says so and says what remains unchecked, is a strictly better thing to
hand over.

실행되지 않은 파일은 모두 헤더에 그렇게 적혀 있고 하네스는 통과가 아니라 건너뜀으로
계수한다. 그 위에 한 가지를 더 했다 — **없는 환경 없이도 검사할 수 있는 부분은
검사했다**(매니페스트가 실제로 들고 있는 맵 문자열을 파일에서 읽어 실제 publish
경로에 통과시킨다). 단, **이 검사는 다이얼하지 않는다.**

## Reproducing, and not inheriting, the VM

The probe creates its own VM and deletes it on exit. It was created with:

```bash
multipass launch --name r7client --cpus 2 --memory 2G --disk 8G 24.04
```

`spikes/nat/vm/run.sh` does this itself when no instance exists, and removes it
again unless `ORBWEAVER_KEEP=1` is set. A VM left behind is removed with:

```bash
multipass delete --purge r7client
```

**No VM was left running by this batch.** `nat_rewrite.sh` will not launch one
either: it runs the probe only when an instance is *already* running, because a
check that downloads an image and boots a VM behind its caller's back is not a
check anybody can trust the timing of.

## The harness group this wants, not yet applied

`spikes/run_checks.sh` is not edited by this batch. This replaces the snippet
proposed in batch 1; it counts the skips separately, because each is a
different unmeasured thing and an unmeasured check is counted, never passed.

```bash
# ── R7 — endpoint rewriting on our own ORB ───────────────────────────────────
hr "R7 — IOR endpoint rewriting (ours)"
# Capture then match: `grep -q` closes the pipe and SIGPIPEs the producer.
nat=$(./spikes/nat_rewrite.sh 2>&1)
printf '%s\n' "$nat" > /tmp/orbweaver-r7.log
if printf '%s' "$nat" | grep -q "nat rewriting: PASS"; then
  echo "  ok   unrewritten IORs did not dial; rewritten ones completed a call"
  if printf '%s' "$nat" | grep -q "the manifest's map is accepted"; then
    echo "  ok   the cluster manifest's publish map translates host and port"
  else
    echo "  skip the cluster manifest's map was not exercised (port 5555 busy)"
    skipped=$((skipped+1))
  fi
  # The VM probe is the one that has executed. It runs only against an
  # instance that already exists, so its absence is a skip and not a failure.
  if printf '%s' "$nat" | grep -q "vm routing-domain probe: PASS"; then
    echo "  ok   R7 measured across a real routing boundary (second host)"
  else
    echo "  skip vm probe — no multipass instance running; spikes/nat/vm/run.sh"
    skipped=$((skipped+1))
  fi
  # These two have never executed anywhere. spikes/nat/preflight.sh says which
  # prerequisite is missing, measured.
  if printf '%s' "$nat" | grep -q "spikes/nat/ is written and UNRUN"; then
    echo "  skip container probe (no engine here) — never executed anywhere"
    skipped=$((skipped+1))
  fi
  if printf '%s' "$nat" | grep -q "spikes/nat/k8s/ is written and UNRUN"; then
    echo "  skip cluster probe (no cluster here) — the port-translation case is unmeasured"
    skipped=$((skipped+1))
  fi
else
  echo "  FAIL see /tmp/orbweaver-r7.log"
  fail_total=$((fail_total+1))
fi
```

About seven seconds without the VM probe, about forty with one already running
— and several minutes the first time, when it launches and cross-compiles.

## Known limits added by this batch

- **A bridged VM is a routing boundary, not a NAT.** See "what it did not
  measure" above. The risk is named for address translation and no measurement
  has covered translation itself.
- **Port translation is still undialed.** Every case that has actually
  connected kept its port. The NodePort case is the one that would change that,
  and it is unrun.
- **The NodePort is pinned, not allocated** (`30555`). It has to be, because
  the map is written before the Service exists; a cluster with a narrowed
  `--service-node-port-range` will reject the Service and say so.
- **The k8s image must be side-loaded** (`kind load` / `minikube image load`).
  No registry is used: publishing images is a licensing question (PLAN §10)
  this probe has no reason to open, and the image contains only our code and
  the Rust toolchain's in any case.
- **The VM probe needs a rustup target**, `<arch>-unknown-linux-musl`, fetched
  on first run. On a host with no network it will fail there, and the message
  will say so.
- **`preflight.sh` reports, it does not repair.** It will not free disk, start
  a daemon or create a cluster, and it should not — a script that silently
  provisions is a script whose green run means something different every time.
