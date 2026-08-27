# 기획서 — OMG 서비스 카탈로그에서 기록조차 없는 둘 / Two OMG specs the record does not name

**Drafted 2026-08-27.** Scope: *is there a major CORBA service this project has
neither served, excluded-with-a-reason, nor written a chapter for?*

**작성 2026-08-27.** 범위: *이 프로젝트가 서빙하지도, 이유와 함께 제외하지도,
장을 쓰지도 않은 주요 CORBA 서비스가 있는가?*

---

## 1. How this was determined / 어떻게 판정했는가

Not from recollection. The OMG catalogue was cross-checked mechanically against
`docs/` and `crates/`, one grep per specification, and the answer separated into
three classes: **served**, **excluded with a recorded reason**, and **absent with
no reason anywhere**. Only the third class is a finding.

기억이 아니라 대조로 판정했다. 명세 하나당 grep 하나로 `docs/`와 `crates/`를
훑고, 답을 셋으로 갈랐다 — **서빙 중**, **이유와 함께 제외됨**, **어디에도 이유가
없이 부재**. 발견은 세 번째 부류뿐이다.

**The record is in much better shape than the exercise assumed.** `PLAN-DEFERRED`
carries §1–§22 and covers essentially the whole CORBAservices catalogue with a
trigger per chapter; `PLAN-SERVICES` carries the five that are served. `PLAN.md`'s **Out of scope
(v1)** line excludes, by name, the CORBA Component Model, Real-Time CORBA
scheduling, GIOP over protocols other than TCP, and bidirectional GIOP. So this document is short by construction: **there is no
list of forgotten services.** There are two.

기록은 이 조사가 가정한 것보다 훨씬 낫다. **잊힌 서비스의 목록 같은 것은 없다.**
둘이 있을 뿐이다.

| Spec | grep hits in `docs/` + `crates/` |
|---|---|
| Fault Tolerant CORBA · IOGR · TAG_FT · object group | **0** |
| CORBA Messaging · AMI · TII · async invocation | **0** |
| Telecom Log Service · Management of Event Domains | 0 — see §5 |
| CCM · Real-Time CORBA | recorded out-of-scope in `PLAN.md` |

---

## 2. Gap 1 — Fault Tolerant CORBA, and it is urgent for a reason that has nothing to do with fault tolerance

**FT CORBA is the OMG's specified answer to the exact question `D029`'s
decision X is asking**, and X was drafted without it on the table.

### 2.1 What the specification says / 명세가 말하는 것

Read rather than recalled (OMG FT 1.0 §23.2.2–23.2.3):

- An **IOGR** — Interoperable Object Group Reference — *"is an IOR that contains
  multiple `TAG_INTERNET_IOP` profiles and that may contain a
  `TAG_MULTIPLE_COMPONENTS` profile."*
- **`TAG_FT_GROUP`** carries three things: the fault-tolerance domain id, the
  **object group id**, and *"the version number of the object group reference"*.
  An object group *"has an identity that persists even as the membership of the
  object group changes."*
- **`TAG_FT_PRIMARY`** appears on *"at most one"* profile and means that profile
  *"is to be used in preference to the other `TAG_INTERNET_IOP` profiles."* It
  is explicitly **not mandated** that the ORB choose it; choosing another costs
  *"one or more LOCATION_FORWARDs and thus reduced efficiency"* — not
  correctness.
- **`FT_GROUP_VERSION`** is a **service context** on the request carrying the
  client's `object_group_ref_version`. *"The ORB must … generate a
  LOCATION_FORWARD reply when the client's request contains an obsolete
  `object_group_ref_version` field."* The FT ORB performs *"most-recent IOGR
  processing"*: a client holding an old IOGR is handed a new one.
- `LOCATION_FORWARD_PERM`'s temporal scope is *"ORB lifetime or the next
  `LOCATION_FORWARD_PERM`."*
- Interfaces: `ReplicationManager`, `ObjectGroupManager`, `GenericFactory`,
  `FaultDetector`, `FaultNotifier`.

### 2.2 What this project already has / 이미 있는 것

**The substrate is built.** Not partially — the transport half is complete:

| Piece | Where |
|---|---|
| an IOR carrying **multiple profiles** | `RawIor { profiles: Vec<RawProfile> }` |
| `TAG_MULTIPLE_COMPONENTS` | known and parsed |
| `TAG_ALTERNATE_IIOP_ADDRESS`, dialled | `IiopProfile::endpoints` |
| failover across addresses **then profiles** on connect failure | the dial loop |
| `LOCATION_FORWARD` and `_PERM`, served and followed | measured, both orders |

So a reference in this project **already names more than one place and already
fails over between them.** What is missing is not plumbing. It is the
specification's *identity* for that plumbing: which group these addresses are,
and which version of it the client holds.

**이 프로젝트의 참조는 이미 여러 장소를 지칭하고 이미 그 사이에서 페일오버한다.**
없는 것은 배관이 아니라 그 배관에 대한 명세의 **정체성** — 이 주소들이 어느
그룹인지, 그리고 클라이언트가 그 그룹의 어느 버전을 들고 있는지.

### 2.3 Why it changes decision X / 왜 결정 X를 바꾸는가

`D029` §6.1 frames X as: *make the reference indirect — its profile carries a
name-resolving endpoint's address and a name.* `D035` answers D029's required
question with **displacement**: the leak moves from N server addresses to 1
forwarding address, and cannot reach 0. Its status and the order it was
approved in live in that document and are not restated here.

**FT's answer has a different shape, and D029 never considered it:**

| | X (name-forwarding) | FT / IOGR |
|---|---|---|
| what the reference names | one forwarding endpoint + a name | **the group, with N profiles inline** |
| addresses a caller learns | 1 (the forwarder's) | N (the members') |
| who detects staleness | nobody — the forwarder redirects to a dead IOR | **the server**, from `FT_GROUP_VERSION` |
| layer inversion | **yes** — ORB depends on a naming servant | **no** — it is IOR content |
| every IOR changes | yes | yes |
| new wire shape | no | **no** — components + one service context |
| stale binding repaired | **no** (D035 §3.4) | **yes** — most-recent IOGR processing |
| independent implementation to be refuted by | none | TAO ships FT CORBA — **and is not installed here**, see below |

Two of those rows are decisive. FT **does not invert a layer** — a group id and
a version are bytes in a component, not a dependency on a servant built on the
ORB, so D035 §3.2's objection does not arise. And FT **repairs the stale
binding**, which D035 §3.4 lists as something X explicitly does not do and
which it defers to *"liveness detection, a fifth and much larger decision."*

FT is **not free of the displacement problem** — a caller still learns N member
addresses, which is *more* than X's one, and under D029's Location row that is
a leak X would have been better at. That is the trade, and it is a real one: X
minimises what a caller can see; FT maximises what a caller can survive. **They
are not competing implementations of one idea; they answer different rows of
§6.1** — X the Location row, FT the Lifecycle row.

X는 위치 행을, FT는 생애주기 행을 답한다. 둘은 한 아이디어의 경쟁 구현이 아니다.

### 2.4 What is proposed / 제안

**Not** *"implement Fault Tolerant CORBA."* The ReplicationManager,
GenericFactory, FaultDetector and FaultNotifier are an infrastructure this
project has no consumer for, and building them would be the capability-over-leak
inversion priority zero forbids. What is proposed is the **reference half only**:

1. `TAG_FT_GROUP` written and read — domain id, group id, **version**.
2. `FT_GROUP_VERSION` service context on the request.
3. A server that answers `LOCATION_FORWARD` when the version it is given is
   older than the one it holds.
4. `TAG_FT_PRIMARY` honoured as a *preference*, since the spec says correctness
   does not depend on it — which makes it the cheapest half to get right.

That is a wire-shape change with a peer to check it against, which is the kind
of work this project is set up to do. It is also the smallest thing that lets
the Lifecycle leak test stop being a `SKIPPED` **without** X's layer inversion.

**The peer is the part that was assumed rather than checked, and it decided the
order this landed in.** *"A peer to check it against"* was written from the fact
that TAO implements FT, not from the fact that TAO is reachable from here.
Measured 2026-08-27: omniORB 4.3.4's headers carry no `TAG_FT_GROUP`,
`FT_GROUP_VERSION` or `IOGR`; JacORB 3.9's jar has no FT entries; and `tao_idl`
is absent — `spikes/differential.sh` has been reporting `SKIPPED tao_idl absent
— its column is unmeasured, not passing` all along. So the four items above are
landable, but nothing here can refute them, and a convention both ends apply
cannot be refuted by a round trip. **The fixture comes before the feature**: a
TAO peer, on the same terms as omniORB and JacORB — a separate-process wire peer
and an external program whose text output is read, never a dependency — and it
retires the differential's standing skip at the same time, so two rows move for
one fixture. If it will not stand up, that is a result: it makes this proposal
*unrefutable here*, which is what should be recorded rather than the proposal
being landed anyway.

**피어가 검사되지 않고 가정된 부분이었고, 그것이 착지 순서를 정했다.** *"대고
검사할 피어"*는 TAO가 FT를 구현한다는 사실에서 나왔지 TAO에 여기서 닿을 수 있다는
사실에서 나오지 않았다. 2026-08-27 측정: omniORB 4.3.4 헤더 0건, JacORB 3.9 jar
0건, `tao_idl` 부재 — `differential.sh`는 줄곧 `SKIPPED tao_idl absent`를
보고하고 있었다. 즉 위 네 항목은 착지시킬 수 있지만 여기서는 무엇도 그것을 반증하지
못하고, **양쪽이 적용하는 관례는 왕복으로 반박되지 않는다. 기능보다 픽스처가
먼저다** — omniORB·JacORB와 같은 조건의 TAO 피어이며, 동시에 differential의 오래된
스킵도 걷히므로 픽스처 하나로 행 둘이 움직인다. 세우지 못하면 그것이 결과이고,
이 제안이 *여기서 반증 불가*임을 기록한다.

---

## 3. Gap 2 — CORBA Messaging (AMI) / 비동기 호출

Zero hits, and no chapter. The specification covers asynchronous method
invocation (callback and polling), the Time-Independent Invocation (TII) with
routing, and a family of QoS policies (`RebindPolicy`, `SyncScopePolicy`,
`RequestPriorityPolicy`, `RoutingPolicy`, `RequestStartTimePolicy` and the
timeout policies).

**Why it deserves a chapter rather than a build.** This project's agent boundary
is `orbweaver-mcp`, and an agent making a call it does not block on is a real
shape. But two facts push it toward a recorded exclusion:

- **`oneway` already exists and is served**, so *"call and do not wait"* is
  available; what AMI adds is *"call, do not wait, and still get the answer."*
- The wire cost is not the invocation, it is `ReplyHandler` — a **callback**,
  which needs the caller to be a target, which is D029's Language row's item 4
  (*a reference arriving is a handle the far side cannot invoke*) and §22
  BiDirectional GIOP. **AMI is blocked behind work already recorded elsewhere.**

So the proposal is a `PLAN-DEFERRED` chapter whose trigger is precisely that:
*the first consumer that needs a reply it did not block for, once a caller can
be a target.* Writing it down stops the next reader re-deriving it, which is the
only thing an unwritten exclusion reliably costs.

`oneway`는 이미 서빙되므로 *"걸고 기다리지 않기"*는 있다. AMI가 더하는 것은
*"걸고 기다리지 않으면서 답도 받기"*이고, 그 비용은 호출이 아니라 `ReplyHandler`
— 즉 **콜백**이다. 콜백은 호출자가 대상이 될 것을 요구하고, 그것은 D029 언어 행의
항목 4이자 §22 양방향 GIOP다. **AMI는 이미 다른 곳에 기록된 작업 뒤에 막혀 있다.**

---

## 4. What is correctly absent / 올바르게 부재한 것

- **CCM**, **Real-Time CORBA scheduling**, **GIOP over non-TCP** — excluded by
  name in `PLAN.md`'s **Out of scope (v1)** line. Recorded; nothing owed. Note
  that bidirectional GIOP is on that line *and* has `PLAN-DEFERRED` §22, which
  is the right arrangement: the line says it is out of v1, the chapter says what
  would bring it back.
- **Telecom Log Service**, **Management of Event Domains** — zero hits and no
  chapter, but both are federations of the Notification Service, which is
  `PLAN-DEFERRED` §1 and itself deferred. A chapter for a service that
  presupposes a deferred service is a chapter about a trigger that cannot fire.
  **Recommendation: one line in §1 naming them as downstream of it**, not two
  chapters.
- **Firewall / GIOP Proxy** — the only hits are macOS's application firewall in
  `PHASE6.md`, so the traversal specification is unrecorded. It is, however, the
  same consumer as §22 BiDirectional GIOP (*"an endpoint that cannot listen at
  all"*). **Recommendation: name it inside §22**, not beside it.

---

## 5. What this document does not claim / 주장하지 않는 것

It does not claim FT CORBA should be built — §2.4 proposes four wire items and
explicitly refuses the infrastructure. It does not claim X is wrong; §2.3 argues
X and FT answer **different rows**, which is a reason to stop treating X as *the*
lifecycle decision, not a reason to refuse it. It does not claim the two gaps are
equally urgent: FT is urgent because a decision is pending that would be made
without it, and AMI is not urgent at all — it is blocked behind recorded work and
needs a chapter, not a batch. And it does not claim this sweep was exhaustive
over every OMG document ever published; it was exhaustive over the CORBAservices
catalogue and the core specifications, which is a smaller claim and the one the
greps support.
