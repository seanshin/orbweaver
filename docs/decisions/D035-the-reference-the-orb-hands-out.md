# D035 — The reference the ORB hands out, and whether it should name a place

**STATUS: PROPOSED** — drafted 2026-08-27 because D029 §6.1's lifecycle row has
waited on a decision called **X** since 2026-08-26 and nothing can move it from
this side. Not self-approvable, and not for the usual reason: §3 lists four
consequences the owner has to accept or refuse, and §4 answers the question
D029 requires of anyone proposing X — *are you claiming closure or
displacement?* — with **displacement**. A proposal that quietly claimed closure
would be the row this whole criterion exists to prevent.

**상태: 제안** — 2026-08-27 작성. D029 §6.1의 생애주기 행이 2026-08-26부터 **X**
라는 결정을 기다리고 있고, 이쪽에서는 움직일 수 없다. 스스로 승인하지 않는다.
§4는 D029가 X를 제안하는 사람에게 요구한 질문 — *폐쇄를 주장하는가, 전가를
주장하는가* — 에 **전가**라고 답한다. 조용히 폐쇄를 주장하는 제안이야말로 이
기준이 막으려고 존재하는 것이다.

> Everything cited is on `main` at `823017e`. Figures carry the date they were
> taken. *인용은 `main`(`823017e`) 기준이며, 수치에는 측정 날짜가 붙는다.*

---

## 1. What raises this / 무엇이 이 문서를 불렀나

Of D029 §6.1's five transparencies, four have a leak test that runs. The
lifecycle leg is a counted `SKIPPED` and its stated blocker is X and nothing
else. So **this is the last decision between the ledger and (A) — every row
measured** — and it is not an engineering task.

What makes it a decision rather than a task is that the mechanism is already
built. `crates/orbweaver-giop/tests/forward_for_a_name.rs`: a servant whose
object key *is* a name and which hosts no objects, `knows` answering `false`
truthfully, `redirect` a name-table lookup, `locate` giving the same answer one
message earlier. Seven tests, both byte orders, three negative controls. **And
it does not close the row.** That is the finding this document starts from: the
thing four separate records named as the blocker was built, measured, and was
not the blocker.

다섯 투명성 중 넷은 도는 누출 테스트를 갖고 있다. 생애주기 다리는 계수되는
`SKIPPED`이고, 명시된 차단 요인은 X 하나다. 즉 **원장과 (A) 사이에 남은 마지막
결정**이며, 엔지니어링 과제가 아니다. 과제가 아닌 이유는 메커니즘이 이미
만들어졌기 때문이다 — 그리고 그것이 행을 닫지 못한다. 네 개의 기록이 차단
요인으로 지목한 것이 만들어지고 측정되었는데 차단 요인이 아니었다는 것, 그것이
이 문서의 출발점이다.

## 2. What X is, precisely / X가 정확히 무엇인가

> **X.** The reference `Orb::server` hands out is **indirect**: its IIOP profile
> carries a name-resolving endpoint's address and a name, rather than the
> servant's own address and an object key.

Two things X is **not**, both stated because both were tried first:

- **X is not a successor registry.** CosNaming's `rebind` already owns the
  mapping, and a successor already calls it. Building a second registry beside
  it would be a second home for one fact.
- **X is not a new wire shape.** A forward produced by a name resolving is
  byte-for-byte the message an object moving produces —
  `the_forward_a_name_produces_is_the_same_message_an_object_move_produces`
  checks this over three GIOP versions and both byte orders rather than
  asserting it. No new peer leg is owed and none was written.

X가 **아닌** 것 둘, 둘 다 먼저 시도했기 때문에 적는다 — 후속 레지스트리가
아니다(CosNaming의 `rebind`가 이미 그 매핑의 집이다), 그리고 새로운 와이어 모양이
아니다(이름이 해석되어 나온 포워드는 객체가 이동해서 나온 메시지와 바이트 단위로
같으며, 그것은 주장이 아니라 측정되어 있다).

## 3. Why this cannot be self-approved / 스스로 승인할 수 없는 이유

Four consequences, from D029 §6.1's lifecycle subsection, each with what it
actually costs.

**3.1 It changes every IOR this project emits.** D019 step 4 made `Orb::server`
the one way a server is built, which means it is also the one place every
reference is minted. That was the point of step 4 and it is what makes X a
single edit — and it is also what makes X unopt-outable. There is no deployment
that keeps the current shape.

**3.2 It inverts a layer.** The ORB would depend on a servant built on top of
it. D019's title is *"The ORB has no object, and everything above it assembles
one by hand"*, and its whole argument is about direction. X points the arrow
back. This is not fatal — a naming service that the ORB bootstraps is ordinary
in CORBA — but it is a reversal of a decision that was approved on 2026-08-26
and should be reversed deliberately rather than by consequence.

**3.3 It displaces the leak rather than closing it.** §4.

**3.4 It does not repair a stale binding.** D029 §6.1's event-channel item 4:
unbinding is deliberately separate from the channel going away, so a name can
resolve to a target that is gone. Under X the forwarder faithfully redirects to
an IOR that is also dead and the caller fails **one hop later** than it does
today. Repairing that is liveness detection — a fifth decision, much larger,
and explicitly not this one.

**3.5 It re-opens D013.** D013 decided reference identity while assuming an IOR
names an object. Under X an IOR names a *name*, so two references that are
`==` today may be the same name resolving to different objects at different
times, and two references that differ may be one object. D013 is still
**PROPOSED** with a recommendation of *do not build*, so nothing has to be
un-built — but its recommendation was reasoned on the old assumption and would
need re-reading, not merely re-approving.

## 4. The question D029 requires an answer to / D029가 답을 요구한 질문

D029 §6.1: *"whoever proposes X must say which of displacement and closure is
being claimed."*

**This proposal claims displacement.** Stated plainly so that approving it
cannot later be read as closure:

Today every server's own address is in every reference it hands out — N
addresses reachable by a caller. Under X, the *forwarding endpoint's* address is
in every reference instead — **one address, in all of them.** The leak goes from
N to 1. It does not go to 0, and nothing in X can take it to 0, because a
caller has to be able to send a first packet somewhere and that somewhere is an
address it was given.

This is exactly the shape D029 already records for the bootstrap: event-channel
item 1 says the naming service's address is still handed over, that the leak is
*"displaced, not closed — from N channels to one bootstrap"*, and that **calling
it closed would be the row that subsection exists to avoid.** X is the same
trade one level up. So the honest form of the question for the owner is not
*should we build X* but:

> **Is a leak displaced from N to 1 what this criterion means by a row that no
> longer leaks — or is it a row that leaks once instead of N times?**

D029 §6 does not answer that, and it is not this document's to answer.

D029는 X를 제안하는 사람에게 *전가와 폐쇄 중 무엇을 주장하는지* 밝히라고 요구한다.
**이 제안은 전가를 주장한다.** 오늘은 서버마다 자기 주소가 자기가 내주는 모든
참조 안에 있다 — 호출자가 닿을 수 있는 주소 N개. X 아래에서는 **포워딩 종단점의
주소 하나가 모든 참조 안에** 들어간다. 구멍은 N에서 1로 간다. 0으로는 가지 않고,
X의 어떤 것도 0으로 보낼 수 없다 — 호출자는 첫 패킷을 어딘가로 보내야 하고 그
어딘가는 받은 주소이기 때문이다. 이는 D029가 부트스트랩에 대해 이미 기록한 바로
그 거래이며, 소유자에게 물어야 할 정직한 형태의 질문은 *X를 지을 것인가*가 아니라
**N에서 1로 옮겨진 구멍이 이 기준이 말하는 "더 이상 새지 않는 행"인가, 아니면
N번 대신 한 번 새는 행인가**이다.

## 5. An option D029 does not name / D029가 이름 붙이지 않은 선택지

D029 frames this as approve-X or keep-waiting. There is another, and it should
be on the table because it is the only one that can move the row **without**
3.1–3.5:

> **Refuse X, and record that lifecycle transparency has an irreducible
> bootstrap leak in a single-node deployment** — the same leak item 1 already
> accepts by name — moving the row from *unmeasured, waits on X* to *measured,
> leaks at the bootstrap, recorded*.

This is not a way of declaring victory. It is the observation that **a caller
must be given one address to reach anything at all**, so a deployment with one
node cannot have a lifecycle row with zero leaks, and a row that waits forever
on a decision that cannot reach zero is worse than a row that names the floor
it is standing on. Under this option the leak test stops being a `SKIPPED` and
becomes a test that measures what a caller can tell across a removal, with the
bootstrap leak as its stated, controlled limit.

The cost is real and should be weighed: this option **does not** give a caller
of a removed server anywhere to go, so *"removed at runtime"* stays observable
by that caller. X would make it observable one hop later. Neither reaches zero.

D029는 이것을 *X 승인* 아니면 *계속 대기*로 놓는다. 세 번째가 있고, 3.1–3.5 없이
행을 움직일 수 있는 유일한 선택지이므로 탁자에 올려야 한다 — **X를 거절하고,
단일 노드 배포에서 생애주기 투명성에는 환원 불가능한 부트스트랩 구멍이 있음을
기록하는 것**. 승리 선언이 아니다. 호출자는 무엇에든 닿으려면 주소 하나를 받아야
하므로 노드가 하나인 배포는 구멍 0인 생애주기 행을 가질 수 없고, 0에 닿을 수 없는
결정을 영원히 기다리는 행은 자기가 딛고 선 바닥을 이름 붙인 행보다 나쁘다.

## 6. Alternatives — one pair refused, one that answers a different row / 대안들

### 6.1 Refused / 거절

- **A tombstone** — leaving the removed server's ORB listening at the same
  address to answer forwards. Refused in D029 and refused here: it contradicts
  D034 (a shutdown that keeps a listener never returns its port), it does not
  survive the cases *removed* usually means (crash, eviction, machine loss), and
  it is unbounded, since nothing can know when the last client holding a
  reference has gone.
- **`corbaname:`** — refused because it resolves **on the client, once, at bind
  time**, and what is kept afterwards is exactly as dead as an IOR. This is not
  rhetoric: `forward_for_a_name.rs`'s third negative control hands the forwarder
  a *snapshot* of the name table, which is what resolving once amounts to, and
  exactly the two late-resolution tests go red.

### 6.2 Not refused — the one D029 never named: Fault Tolerant CORBA / 거절이 아니라, D029가 이름 붙이지 않은 것

Found 2026-08-27 by sweeping the OMG catalogue against this repository: **`Fault
Tolerant`, `IOGR`, `TAG_FT` and `object group` return zero hits in `docs/` and
`crates/` alike.** X was drafted without the specification OMG wrote for this
problem on the table, and that is a defect in this document's own preparation
rather than in X.

Read out of the specification rather than recalled — `TAG_FT_GROUP = 27`,
`TAG_FT_PRIMARY = 28`, `FT_GROUP_VERSION = 12` (a service context) — an **IOGR**
is *"an IOR that contains multiple `TAG_INTERNET_IOP` profiles"*, each carrying
a group id **and the version of the reference**; and the rule, verbatim: *"If
the server determines that the client is using an obsolete object group
reference, the server returns a `LOCATION_FORWARD_PERM` response that contains
the most recent object group reference."*

**And the transport half is already built here**, verified in the code:
`RawIor` holds `Vec<RawProfile>`, `Connection::connect` dials each profile's
address then its `TAG_ALTERNATE_IIOP_ADDRESS` alternates then the next profile,
and a successful connection keeps **the whole IOR** so a §9.6 restart gets the
same failover. `ServiceContext` read and write already exist.

Two rows decide how this bears on X:

| | X (name-forwarding) | FT / IOGR |
|---|---|---|
| inverts a layer (§3.2) | **yes** | **no** — it is IOR content, not a servant the ORB depends on |
| repairs a stale binding (§3.4) | **no** | **yes** — most-recent IOGR processing |
| addresses a caller learns | 1 | N |

So FT escapes two of the four objections in §3 — and it is **worse** on the
Location row, because a caller learns every member's address where X would have
shown it one. **They answer different rows of D029 §6.1**: X minimises what a
caller can see, FT maximises what a caller can survive.

**This does not make X unnecessary and it does not make FT the answer.** What it
changes is the framing: X should stop being treated as *the* lifecycle decision,
and §5's third option now has a fourth beside it. It also does not reach zero
either — failing over needs a second member, which is a property of a deployment
and not of this repository, so in one process what FT makes measurable is the
smaller claim in §4's terms: a caller holding a replaced reference is told so.

**2026-08-27에 발견.** OMG 카탈로그를 이 저장소에 대조한 결과 `Fault Tolerant`,
`IOGR`, `TAG_FT`, `object group`이 **전부 0건**이었다. X는 OMG가 이 문제에 대해
쓴 명세를 탁자에 올리지 않은 채 기안되었고, 그것은 X의 결함이 아니라 이 문서의
준비의 결함이다. FT는 §3의 네 반대 중 둘을 피한다 — **계층을 뒤집지 않고**(IOR
내용일 뿐이다), **낡은 바인딩을 복구한다**. 대신 위치 행에서는 **더 나쁘다**:
호출자가 멤버 전부의 주소를 알게 된다. **둘은 §6.1의 서로 다른 행을 답한다.**
그러므로 X가 불필요해지는 것도, FT가 답이 되는 것도 아니다. 바뀌는 것은 틀이다 —
X를 *유일한* 생애주기 결정으로 다루기를 그만두어야 한다.

## 7. The four paths, and what each costs / 네 가지 경로와 비용

§6.2 added a path this document did not have when §3 was written, so the choice
is four-way and not three. Cost is **rough and labelled as such** — it is an
estimate from the shape of the work, not a measurement, and this project's own
rule about numbers applies to it.

| | Path | Commits to | Moves the Lifecycle row to | Rough cost |
|---|---|---|---|---|
| **A** | Approve **X** | 3.1 (every IOR changes), 3.2 (D019's direction reversed), 3.4 (a stale binding still fails, one hop later), 3.5 (D013 re-read) | *measured, leak displaced to the forwarding endpoint* — never *closed* | large: one IOR-minting path, a name-resolving servant on the serving side, and a re-reading of D013 |
| **B** | §5's third option — refuse X, record the bootstrap leak as irreducible in one process | nothing in 3.1–3.5 | *measured, leaks at the bootstrap* | small: a leak-test leg and a row edit |
| **C** | The **FT reference half** (§6.2): `TAG_FT_GROUP` (27), `FT_GROUP_VERSION` (12), a server that forwards on a stale version, `TAG_FT_PRIMARY` (28) as a dial preference | a wire-shape addition with a published specification and an independent implementation (TAO) to be refuted against; **no layer inversion**, and it repairs the stale binding X cannot | *measured*, on the smaller claim: a caller holding a replaced reference is told so. Failover itself needs a second member, which is a deployment property | medium: four items, each independently landable, on a transport half that already exists |
| **D** | Refuse all three | nothing | stays a counted `SKIPPED` | none |

A and C are **not exclusive**. §6.2's table is the reason: X minimises what a
caller can see and FT maximises what a caller can survive, and they answer
different rows of D029 §6.1 — X the Location row, FT the Lifecycle row.

## 8. Recommendation / 권고

**C, then B; A deferred rather than refused.** Stated so it can be rejected
rather than left for the reader to assemble:

- **C first**, because it is the only path that repairs the stale binding, needs
  no layer inversion, and is checkable against a peer that implements the same
  specification. Its four items are independently landable, and R1 and R2 change
  no wire behaviour at all — a component nobody acts on and a service context an
  unknowing peer ignores — so the first half is close to free to try.
- **B alongside it**, because the Lifecycle row should stop being unmeasured
  regardless of which mechanism lands, and B is honest about a floor that
  neither A nor C can reach: **a caller must be given one address to send a
  first packet to.**
- **A deferred, not refused.** X remains the better answer for the *Location*
  row, and §4's displacement argument is a reason to be precise about what it
  buys, not a reason to discard it. What it should stop being is *the* lifecycle
  decision, which is how D029 framed it and how this document was commissioned.

The recommendation this document does **not** make: building Fault Tolerant
CORBA. §6.2 proposes the reference half only and refuses the infrastructure —
`ReplicationManager`, `ObjectGroupManager`, `GenericFactory`, `FaultDetector`,
`FaultNotifier`, heartbeating and transparent reinvocation — because it has no
consumer here and would put a capability ahead of a leak, which priority zero
forbids.

## 9. What would refute the recommendation / 무엇이 이 권고를 반증하는가

Written down so approving it is not the end of the argument:

- **C is refuted** if a peer that implements FT rejects our `TAG_FT_GROUP` or
  ignores `FT_GROUP_VERSION` where the specification says it must forward — the
  claim is interoperability, and an independent implementation is what can
  falsify it. It is also refuted if R3 cannot be measured in one process, since
  the whole argument for C over A is that its useful half needs no second node.
- **B is refuted** if a single-node deployment turns out to have a way to give a
  caller its first address without that address being a leak. §4 argues there is
  none; a counter-example ends B.
- **The framing is refuted** if X and FT turn out to answer the *same* row after
  all — if, say, a name-resolving endpoint can carry the group version and make
  the stale binding repairable. Then A and C stop being complementary and the
  choice really is exclusive.

## 10. What this document does not claim / 주장하지 않는 것

It **does** now recommend — §8 — and that is a change from this document's first
draft, which listed options and left the synthesis to the reader. A decision
record that will not say what it thinks is asking the owner to do the work it
was written to do. The recommendation is stated so it can be **rejected**, and
§9 says what would refute it.

What it does not claim: that the forwarding mechanism is unproven — it is built
and measured, which is precisely why what is left is a decision and not work.
That either option reaches zero leaks; §4 says why nothing in one process can.
That the cost column in §7 is a measurement — it is an estimate from the shape
of the work, labelled as such, and this project's rules about numbers apply to
it. That FT should be built — §6.2 proposes the reference half and refuses the
infrastructure. And that the Lifecycle row is the last thing between here and a
complete ORB: it is the last thing between here and every row being
**measured**, which is a smaller claim and the one D029 §6 can check.

이 문서는 이제 **권고합니다**(§8). 첫 초안은 선택지만 늘어놓고 종합을 읽는 사람에게
넘겼는데, 자기 생각을 말하지 않는 결정 기록은 스스로 하려던 일을 소유자에게
떠넘기는 것입니다. 권고는 **거절당할 수 있도록** 적었고, §9가 무엇이 그것을
반증하는지 말합니다. §7의 비용 열은 측정이 아니라 작업의 모양에서 나온 추정이며,
그렇게 표시했습니다.
