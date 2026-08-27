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

### 2.5 B — the lifecycle leg, and what makes its green mean something / 생애주기 다리

D035 was approved 2026-08-27 with **B first**. Its claim: lifecycle transparency
has an **irreducible bootstrap leak in a single-node deployment**, because a
caller must be given one address to send a first packet to. The row moves from
*unmeasured, waits on X* to *measured, leaks at the bootstrap, recorded*.

**What B is not, and this is its failure condition.** Not a declaration of
victory. `D029` §6.1 already records the same trade for the event channel —
*"displaced, not closed — from N channels to one bootstrap"* — and B is that
sentence one level up. **If the row reads as closed, B has failed even if the
test is green.**

**Where the edit lands, which is narrower than it looks.**
`spikes/transparency.py` reads exactly one row — §6.1's five-transparency table
— and takes its cells as (slug + title, tell, status). So:

- **cell 0 must not change.** The slug is its first word, lowercased; changing
  it breaks `bears_on lifecycle`, `leak_tests.sh`'s `lifecycle)` arm, and every
  `--check`.
- **the table must stay five rows.** Four or six makes every mode exit 2, which
  the harness counts as a failure.
- **cell 2 is what B rewrites.** It is what `--cite` returns and what the ledger
  prints verbatim, wrapped at 62 columns, under *"unmeasured, per D029 §6.1 —
  where it leaks today"*.
- the Korean half is **not** in the cell; it is the prose paragraph after the
  table and moves with it, one fact in two languages.
- the leg itself is `spikes/leak_tests.sh`'s `lifecycle` arm, whose `--raw`
  detail is today `decision X: the reference Orb::server hands out is not
  indirect` — the blocker that no longer exists.

**The control is the load-bearing half, and this project has measured why.** An
*indistinguishability* assertion passes in every world where nothing happens:
the backend leg stayed green when `Dispatch::knows` was made a blanket `false`,
because a server that serves nothing answers both keys identically too. So B's
leg needs a counted companion showing the two answers **can** differ — a caller
whose target was *not* removed still gets through — or its green means only that
nothing occurred. `spikes/leak_controls.sh` is where that goes; it proves three
of its four controls today and states so in the file.

D035가 2026-08-27에 **B를 먼저**로 승인했다. 주장은 단일 노드 배포에서 생애주기
투명성에 **환원 불가능한 부트스트랩 구멍**이 있다는 것이다 — 호출자는 첫 패킷을
보낼 주소 하나를 받아야 하기 때문이다. 행은 *미측정, X 대기*에서 *측정됨,
부트스트랩에서 샘, 기록됨*으로 옮겨 간다. **B가 아닌 것이 곧 B의 실패 조건이다**:
승리 선언이 아니다. D029 §6.1이 이벤트 채널에 대해 이미 *"닫힌 것이 아니라
옮겨졌다"*고 적었고 B는 그 문장을 한 단계 위에 적용하는 것이므로, **행이 닫힌
것으로 읽히면 테스트가 초록이어도 B는 실패다.** 편집 지점은 보기보다 좁다:
`transparency.py`가 읽는 것은 §6.1 다섯 투명성 표의 그 한 행뿐이고, **첫 칸은
슬러그의 출처라 건드릴 수 없으며 행 수는 다섯을 유지해야 한다**(아니면 모든 모드가
exit 2). **셋째 칸**이 다시 쓸 곳이고, 한국어 절반은 표 안이 아니라 표 뒤 산문이며
함께 움직인다. **통제군이 짐을 진다**: 구별불가능성 주장은 아무 일도 일어나지 않는
세계에서도 통과하므로, 제거되지 *않은* 대상을 든 호출자는 여전히 통과한다는 계수된
동반자가 없으면 초록은 아무것도 뜻하지 않는다.

### 2.6 P — a TAO peer, and what it costs / TAO 피어

**P is not a new item.** It is `docs/PLAN.md`'s standing aspiration **A6** —
*TAO as a wire round-trip peer* — and **D026 item 2**, promoted by D035's
approval from "waits on somebody else" to the step before R1. D026 already wrote
its success criterion and P should quote rather than reinvent it: *each script's
success criterion is that the corresponding `SKIPPED` becomes an `ok` or a
`FAIL` — never that the script exits 0.*

**The licence position, before any command.** ACE/TAO is DOC-licensed and is a
**fixture, never a dependency**, exactly as omniORB and JacORB are: never
imported, linked, vendored or redistributed; only run as a separate-process wire
peer over TCP or invoked as an external program whose text output is read;
`cargo tree` free of it — which `spikes/licence_boundary.sh` now enforces from
one home for all four call sites; and **no CI image containing it is ever
published**, because publishing is redistribution.

**What one fixture buys — two rows for one build.** It is what can refute
R1–R4, which is why it precedes them; and it retires `spikes/differential.sh`'s
standing `SKIPPED tao_idl absent — its column is unmeasured, not passing`,
giving the corpus a third independent front end. `differential.sh` already has
the column wired: `tao_idl_verdict()` exists, detection is a bare
`command -v tao_idl`, and `--require omniidl,jacorb_idl,tao_idl` turns the skip
into a counted failure with no other change.

**The shape, narrowed by measurement 2026-08-27 and not yet priced.** Homebrew's
`ace` formula downloads `ACE+TAO-8.0.7.tar.bz2` — the **combined** distribution
— but its install block is `make -C ace`, so it builds the ACE library and
nothing under `TAO/`. `brew install ace` therefore leaves `command -v tao_idl`
false, and a bottle exists so ACE itself costs a download rather than a compile.
The plausible route is that bottle plus a build of **`TAO/TAO_IDL` alone** — one
compiler binary, not the ORB. **What is not measured is whether that builds
against an installed ACE at all**: TAO's makefiles expect `ACE_ROOT` to be a
source tree, and the fallback is an in-tree ACE build. So P's cost stays
*unknown until tried*; the finding narrows the shape and does not price it.

**In CI**, `.github/workflows/ci.yml` already records — as a measurement, not a
guess — that *Ubuntu has no tao-idl package, which the first run of this
workflow established rather than assumed.* The same tarball route applies there,
and the interop job already builds omniORBpy from source, so a source build in
that job has precedent and 117 G of reclaimed disk to happen in.

**If it will not stand up, that is the result.** R1–R4 become *unrefutable
here*, and that is what gets recorded — not a decision to land them anyway,
which would put a capability ahead of a leak and is what priority zero forbids.

**P는 새 항목이 아니다.** `PLAN.md`의 상시 아스피레이션 **A6**과 **D026 항목 2**이며,
D035 승인으로 *남을 기다림*에서 R1 앞 단계로 승격된 것이다. D026이 성공 기준을 이미
써 두었고 P는 그것을 인용한다 — *각 스크립트의 성공 기준은 해당 `SKIPPED`가 `ok`나
`FAIL`이 되는 것이지, 스크립트가 0으로 끝나는 것이 아니다.* **라이선스 위치는 어떤
명령보다 먼저**: ACE/TAO는 DOC 라이선스이고 omniORB·JacORB와 똑같이 **픽스처이지
의존성이 아니다** — 링크·벤더링·재배포 없음, 별도 프로세스 피어이거나 출력을 읽는
외부 프로그램일 뿐, `cargo tree`에 없음(이제 `licence_boundary.sh`가 네 호출 지점
모두에 대해 한 집에서 강제한다), 그리고 **그것이 든 CI 이미지는 발행하지 않는다.**
**픽스처 하나로 행 둘이 움직인다**: R1–R4를 반증할 수 있는 것이자, differential의
오래된 `tao_idl` 스킵을 걷어 코퍼스에 **독립적인 세 번째 프론트엔드**를 준다.
**모양은 2026-08-27 측정으로 좁혀졌으나 값은 아직 매겨지지 않았다**: Homebrew의
`ace`는 결합 tarball을 내려받지만 `make -C ace`만 하므로 `tao_idl`을 만들지 않는다.
유력한 길은 그 병 + **`TAO/TAO_IDL`만** 빌드하는 것이다 — ORB 전체가 아니라 컴파일러
하나. **재지 않은 것은 그것이 설치된 ACE에 붙는가**이며(TAO는 `ACE_ROOT`가 소스
트리이길 기대한다), 그래서 비용은 *시도 전에는 모름*으로 남는다. **세우지 못하면
그것이 결과다** — R1–R4가 *여기서 반증 불가*임을 기록할 뿐, 그냥 착지시키지 않는다.
그것은 구멍보다 기능을 앞세우는 것이고 0순위가 금지한다.

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
