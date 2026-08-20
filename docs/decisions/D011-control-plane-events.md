# D011 — Control-plane events: what F4 may publish into F7's channel, and to whom

**STATUS: PROPOSED** — drafted 2026-08-19 against the `PLAN-SERVICES.md` §10 row
the plan review of that date re-measured, and against the code both halves
actually contain. **Not adopted, and not self-approvable**: the question is what
this project publishes to an audience it cannot identify, which is the same
class of question `PLAN-DEFERRED.md` §11 already refused to answer by writing
code. Approval commits to a field rule and to how the §10 row is closed, not to
an implementation.

**상태: 제안** — 2026-08-19 작성. 채택되지 않았고, 스스로 승인할 수 없다. 이
문서가 묻는 것은 **신원을 확인할 수 없는 청중에게 무엇을 내보내는가**이며,
`PLAN-DEFERRED.md` §11이 코드로 답하기를 거부한 것과 같은 종류의 질문이다.
승인은 필드 규칙과 §10 행을 닫는 방식에 대한 것이지 구현에 대한 것이 아니다.

---

## 1. The row that called this document / 이 문서를 부른 행

`docs/PLAN-SERVICES.md` §10, line 351, verbatim:

> | CosEvent → telemetry feedback | F4 + F7 | **both exist since 2026-08-18 and
> nothing publishes** a control-plane event into the channel — the precondition
> is met and the work is open (plan review 2026-08-19); needs a short design
> note first: what is published, what is not (the §5 trust boundary) |

This is that note. It does not edit the row; §10 of that document is where the
row lives, and a decision that rewrote its own motivation would be marking its
own homework. §11 below states, as text, what would change there under each
option, so whichever batch closes the row can apply it in one edit.

**요지.** §10의 그 행이 요구한 설계 노트가 이 문서다. 행 자체는 고치지 않는다 —
사실은 한 집에 산다. 대신 §11이 각 대안 아래에서 그 행이 어떻게 바뀌는지를
적용되지 않은 텍스트로 적어 둔다.

---

## 2. What was measured here / 여기서 실측한 것

Everything below was read in this worktree at `4917471`. No test was run and the
harness was not taken: this batch writes a document, and `spikes/run_checks.sh`
holds a machine-wide lock that a documentation batch has no business taking.
`python3 spikes/decision_status.py` was run, because this file adds a decision
and that gate reads decision files.

**F4 exists and is occupied.** `STAGE_TELEMETRY`
(`crates/orbweaver-mcp/src/interceptor.rs:279`) is filled by
`TelemetryInterceptor` (`:1203`), whose `after` (`:1239` onward) records counts
into `CallStats` and, when a `Trace` is attached, one D004 span record per
decided call.

**F7 exists and is bounded.** `crates/orbweaver-giop/src/event_server.rs` serves
the push pair both ways and the consumer half of pull; each proxy has a queue
bounded by `DEFAULT_QUEUE_LIMIT = 64` (`:229`), overflow drops the oldest into
`ChannelStats::dropped` (`:421`), and `ChannelHandle::publish` (`:773`) is the
in-process enqueue that needs no socket.

**Nothing publishes.** `event_server` is named outside its own file only by
`orbweaver-giop`'s own modules (`lib.rs`, `server.rs`, `mux.rs`, `guarded.rs`,
`typecode.rs`), by two of its own integration tests (`tests/event_pull_model.rs`,
`tests/codesets_on_the_wire.rs`) and by `src/bin/spike_events.rs`. No crate
outside `orbweaver-giop` calls `ChannelHandle::publish`. The §10 row's claim is
still true as written.

**The channel already anticipated this.** `event_server.rs:164-169`:

> `# In-process publishing` — *F3's residency transitions and F4's telemetry
> batches are produced inside this process. Making them marshal through a
> loopback socket to reach a channel in the same address space would be a cost
> paid for nothing, so `ChannelHandle::publish` enqueues directly.*

So the mechanism was built for this caller by name. That is a fact about the
mechanism and not an argument that the caller should exist — §3 to §7 are about
whether it should.

**실측 요약.** F4의 telemetry 단계는 채워져 있고(`interceptor.rs:279`, `:1203`),
F7의 채널은 존재하며 큐는 64로 유계다(`event_server.rs:229`, `:421`, `:773`).
그리고 **아무도 발행하지 않는다** — `ChannelHandle::publish`를 부르는 크레이트는
`orbweaver-giop` 밖에 없다. 채널의 모듈 문서는 F4의 텔레메트리를 이름으로 예상해
두었으나(`:164-169`), 그것은 기구에 관한 사실이지 그 호출자가 존재해야 한다는
논거가 아니다.

---

## 3. What a control-plane event would be / 컨트롤 플레인 이벤트란 무엇인가

D004 fixed one record: nine keys, in one order, settled in the decision rather
than in whichever crate landed first — `ts`, `session`, `caller`, `target`,
`operation`, `decision`, `stage`, `path`, `outcome`
(`crates/orbweaver-mcp/src/telemetry.rs:272-281`, built only in `Trace::record`
at `:531`, rendered by `SpanRecord::to_line`). There is no duration and there is
no clock: `ts` is whatever the host supplied, and every host in this workspace
today supplies none, so every line reads `"ts":"-"` (`telemetry.rs:58-70`).

The question the row asks is whether a channel event is that record, a
projection of it, or a different vocabulary. The answer comes from the two
audiences, and they want almost disjoint field sets.

**The operator reading a trace page.** `crates/orbweaver-console/src/traces.rs`
reads exactly those nine keys (`KEYS`, `:53-54`) and groups them into sessions.
What makes that page useful is **correlation**: `session` is the join to the
audit ledger (`telemetry.rs:29`, `:48-54`), `caller` attributes, `stage`
diagnoses which gate refused, `ts` orders when there is a clock. This reader is
already inside the trust boundary — they hold the ledger, the catalogue and the
process. The record was designed for them and it shows.

**The servant or agent that subscribes.** A subscriber does not read; it
**reacts**. What it needs is a trigger and a subject: *something was refused on
this target*, *this residency changed*, *this path is now dynamic*. It has no
ledger to join `session` to, no reason to know which principal was involved, and
no use for `stage`, which names an internal gate of a process it is not part of.
It cannot even order events by `ts`, because `ts` is `-`. Every field that makes
the record valuable to the first audience is either useless or hazardous to the
second, and every field the second one needs — `decision`, `target`, `operation`
— is already the smallest part of the record.

**So the honest answer is: a different vocabulary, derived from the same
decision, with the mapping written down.** Not the record, because the record is
built for a reader who holds the correlating context. Not a "projection" in the
sense people reach for, either — a projection is a subset expression evaluated
over a shape that will keep changing, and it inherits every future key for free.
The tenth key that lands for the console's benefit would appear on the wire
without anybody deciding it should. A separate, closed vocabulary makes that an
explicit act.

The cost of that answer, stated because it is real: D004 fixed one shape
precisely so that two batches could agree on it, and a second shape is a second
thing to keep in step. The mitigation is not synchronisation but **direction** —
the event vocabulary is defined as a total function *from* the record, in one
place, so a new key is inert until somebody adds an arm. That is the same shape
`Decision::audit_token` already uses to keep the trace and the ledger from
disagreeing (`telemetry.rs:51-56`).

**요지.** 청중이 둘이고 원하는 필드 집합이 거의 겹치지 않는다. 콘솔을 읽는
운영자는 **상관(correlation)**을 원한다 — `session`은 감사 원장과의 조인 키이고,
`caller`는 귀속이며, `stage`는 진단이다. 그 독자는 이미 신뢰 경계 **안쪽**에
있다. 반면 구독하는 서번트/에이전트는 읽지 않고 **반응**한다 — 필요한 것은
`decision`·`target`·`operation`뿐이고, `session`·`caller`·`stage`는 쓸모없거나
위험하다. `ts`는 이 워크스페이스의 모든 호스트에서 `-`이므로 정렬에도 못 쓴다.
따라서 답은 **같은 결정에서 파생된 다른 어휘**이지 레코드도, 부분집합 투영도
아니다. 투영은 앞으로 늘어날 키를 공짜로 상속한다.

---

## 4. The trust boundary, and which fields could not cross / 신뢰 경계

`docs/ARCHITECTURE.md` §5 draws four boundaries and the fourth is the agent:
default-deny exposure, per-operation `ai_authz`, approval for destructive
effects, capability handles instead of IORs. Its closing clause is the one that
decides this document:

> **an object reference is a bearer address**. An operation that returns one
> widens what its caller can reach even if it changes nothing, and a reference
> inside a `sequence` is as dialable as a reference returned directly.

An event channel is not a boundary at all. It is a **fan-out to whoever
connected**: a consumer calls `obtain_push_supplier` and `connect_push_consumer`
and then receives everything the channel accepts, forever, with no field the
servant reads about who it is. So a channel event is not "a record with an
audience"; it is a record with *no* audience predicate, and the only control
available is what is in the event.

The audit ledger's rule is the precedent, and it applies here with more force.
`spikes/run_checks.sh:1836` names the group — *"audit ledger — a gate that sees a
secret must not publish one"*. The mechanism is a type, not a grep: `audit_entry`
takes an `AuditReason` (`crates/orbweaver-mcp/src/guard.rs:201`), which has
exactly two constructors (`already_rendered` at `:206`, `ledger_reason` at
`:252`) and the harness counts them (`run_checks.sh:1871-1881`) because a
compiler cannot see a *third* one appear. `audit_reason` (`guard.rs:242-246`)
takes `Denied::Intercepted`'s **stage name** and drops its prose, measured
against a content stage that put a PIN in an argument. That rule protects a file
on an operator's disk. A channel protects nothing, so the rule has to be
stronger, not weaker.

### 4.1 What could not cross, and why / 넘을 수 없는 것

| Field | Why it cannot cross |
|---|---|
| `session` | It is the **join key**. `telemetry.rs:29` and `:48-54` say so: the trace line joins to the audit ledger by `session`. Handing it to a fan-out gives a subscriber a stable correlation handle for one caller's whole activity, without ever learning the principal — group by `session` and you have somebody's session profile from a stream you did not have to be authorized for |
| `caller` | The principal. The record structurally cannot hold a *credential* (`Caller` has no such field; `Trace::record` at `telemetry.rs:531` has no argument that could carry one) and that property is not in question. What is in question is **attribution to strangers**: publishing which principal called what, to whoever dialled the port, is the leak `PLAN-DEFERRED.md` §1 defines — *if tenant A's transitions travel to tenant B's consumer process and are discarded there, we have shipped tenant A's data to tenant B and called it filtering* |
| `target` **when it is a handle** | On the unresolved arm, `crates/orbweaver-mcp/src/lib.rs:584` sets `target: handle` — the string the caller passed, never resolved. Two problems and they are different sizes. The small one: a capability handle is session-scoped, unguessable, and PLAN §4.7 makes the unguessability the control. The large one: it is **agent-supplied free text**, and a reader cannot tell it from a repository id without knowing this rule. Stated honestly, the handle in this position is by construction one that did **not** resolve, so republishing it grants nothing — the objection is the free text and the ambiguity, not an escalation |
| `operation` **on the same arm** | Same call site, same problem, and less noticed: `lib.rs:585` passes the caller's `operation` through unvalidated, because nothing resolved. `telemetry.rs:42-43` already names `target` and `operation` "agent-influenced strings" — for *escaping*. Nothing names them agent-influenced for *publication*, which is a stronger requirement |
| argument values | Refused already, structurally: `SpanRecord` has no field for a value and `Trace::record` has no parameter that could carry one (`telemetry.rs:78-96`). A publisher inherits that only if it is built the same way — no argument that could hold one — and not by remembering |
| a content stage's `why` prose | `Denied::Intercepted`'s reason is free prose a deployment wrote, and *"`cents` looked like a credential: `pin-…`"* is the most natural sentence a content filter has (`guard.rs:224-240`). It reaches the caller, the dry-run report and observer stages — readers who already hold the arguments — and reaches the ledger only as a stage name. A subscriber holds nothing, so it gets neither the prose nor the stage name |
| IORs and object references | ARCHITECTURE §5's fourth row. An event is an `any` (PLAN-SERVICES §4), and an `any` can carry an `Object`-typed member as easily as a `long`. A published reference is a dialable address handed to whoever connected — the exact thing capability handles exist to prevent, arriving by a path the handle table never sees |
| dry-run records | `Decision::DryRunAllow` / `DryRunRefuse` describe calls that did not happen. `promote.rs` keeps them out of the counters so a hypothetical cannot recommend freezing a path nobody invoked; a channel must keep them out for a different reason — an operator's pre-deployment survey of a thousand operations, fanned out, is a **map of the policy surface** delivered to a subscriber authorized for none of it |

### 4.2 What could cross, said as plainly / 넘을 수 있는 것

Saying only what is forbidden produces a rule nobody can apply. These are
identifiers in the currency the ledger already speaks, and each is defensible to
an anonymous reader:

- `decision`, as one of the four audit tokens — and only the two
  non-hypothetical ones (`DECISION_ALLOW`, `DECISION_REFUSE`).
- `target` **when the capability table resolved it**, i.e. a repository id. A
  repository id is a type name; it is already published in every IOR the channel
  itself hands out.
- `operation`, **when it was resolved against that target** and is therefore a
  name the registry contains, not a name the caller typed.
- `outcome` when it is `ok` or a system-exception repository id
  (`telemetry.rs:122`, `:136`). `IDL:omg.org/CORBA/BAD_OPERATION:1.0` is an
  identifier, not a payload.

The seam between the two lists is one predicate: **did the capability table
resolve this call?** Everything on the unresolved arm is caller-supplied text and
nothing on it may be published. That is one condition, checkable, and it lands in
the same place `Chain::unresolved` already distinguishes
(`interceptor.rs:916-921`).

**요지.** 이벤트 채널은 경계가 아니라 **접속한 누구에게나의 팬아웃**이다. 감사
원장의 규칙 — *비밀을 보는 게이트가 비밀을 발행해서는 안 된다*
(`run_checks.sh:1836`, 타입으로 강제: `AuditReason` 생성자 정확히 2개,
`guard.rs:201`·`:206`·`:252`) — 은 여기서 더 강하게 적용된다. 원장은 운영자
디스크의 파일을 지키지만 채널은 아무것도 지키지 않기 때문이다. 넘을 수 없는 것:
`session`(조인 키), `caller`(귀속), 미해결 경로의 `target`·`operation`(에이전트가
넘긴 자유 텍스트, `lib.rs:584-585`), 인자 값, 내용 스테이지의 산문, IOR/객체 참조,
드라이런 레코드. 넘을 수 있는 것: 해결된 저장소 id, 해결된 연산명, 두 개의
비가정 `decision` 토큰, `outcome`. 두 목록을 가르는 술어는 하나 — **능력 테이블이
이 호출을 해결했는가.**

---

## 5. Who may subscribe / 누가 구독해도 되는가

Today: **whoever can dial the port.** The channel servant has no notion of who is
calling, and that is not an omission — it is written down as the reason
`EventChannel::destroy` is refused (`event_server.rs:52-63`):

> `destroy` is an **unauthenticated remote operation that ends the channel for
> every other client**, and this servant has no notion of who is calling

`PLAN-DEFERRED.md` §11 records the same, re-measured 2026-08-18, and its
un-defer trigger (§11, and the table at `PLAN-DEFERRED.md:54`) is *a caller model
in the event servant — the moment CSIv2 identity or the bridge's `Caller` reaches
a servant (stream C, D010 B2), this becomes an authorization decision like any
other.*

Two consequences, and the second is the one that decides between the options.

**There is no second mechanism.** Per-consumer filtering is CosNotification's
centre of gravity and CosNotification is deferred (`PLAN-DEFERRED.md` §1), for a
reason that reads on this question directly: *today every consumer of
control-plane events sits behind the MCP boundary, where the guard chain already
filters by authorization; adding a second filtering point that does not share the
first one's policy is how two filters disagree.* So the field list of §4 is not
one control among several. It is the whole of the control.

**Is publishing blocked on the caller model? For the fields worth publishing,
yes.** This has to be said precisely rather than as a slogan:

- An event set narrow enough to be **public** — resolved repository id, resolved
  operation, one of two decision tokens, an outcome id — is *not* blocked. There
  is nothing in it that authorization would protect, so there is nothing for a
  caller model to decide.
- Any event set that carries `session`, `caller`, or the unresolved arm **is**
  blocked, and blocked in the strong sense: "redacted for the audience" is a
  judgement about an audience, and there is no audience to judge. A
  `--publish-events` flag does not fix this. A flag is one operator's
  deployment-wide consent; subscription is per-connection, and a flag cannot
  distinguish the monitoring sidecar the operator was thinking of from the
  process that dialled the same port ten minutes later.
- And a set that is safe today becomes unsafe on the day there are two tenants,
  with no code change in between. F5 tenancy is exactly `PLAN-DEFERRED.md` §1's
  isolation trigger, and the channel cannot tell a single-tenant deployment from
  a multi-tenant one.

The asymmetry this leaves is worth naming, because it is an argument on its own:
this servant refuses **one** unauthenticated remote operation (`destroy`) on the
ground that it has no caller model. A stream of control-plane decisions delivered
to the same anonymous audience is a larger claim than that operation, not a
smaller one. Refusing the small thing and shipping the large one, in the same
file, would be a position nobody could defend when asked.

**요지.** 오늘 구독 자격은 **포트에 다이얼할 수 있는 누구나**다. 서번트에는
호출자 개념이 없고, 그것이 바로 `destroy`가 거부된 이유이며
(`event_server.rs:52-63`, `PLAN-DEFERRED` §11) 그 방아쇠는 스트림 C의 호출자
모델이다. 소비자별 필터링은 CosNotification의 중심이고 그것은 유예 상태이므로,
§4의 필드 목록은 여러 통제 중 하나가 아니라 **통제의 전부**다. 결론: **공개해도
되는 좁은 집합은 호출자 모델에 막혀 있지 않지만, `session`·`caller`·미해결 경로를
싣는 어떤 집합도 막혀 있다** — "청중에 맞춘 편집"은 청중이 있어야 성립하고,
플래그는 배포 전체의 동의일 뿐 접속별 인가가 아니다. 그리고 같은 서번트가 연산
하나(`destroy`)는 호출자 모델이 없어 거부하면서 결정의 스트림 전체를 같은 익명
청중에게 흘린다면, 그것은 방어할 수 없는 위치다.

---

## 6. Back-pressure, and what publishing would do to §1's trigger / 배압과 드롭

The channel's rule: each proxy has its own bounded queue, `DEFAULT_QUEUE_LIMIT`
= 64 (`event_server.rs:229`); on overflow the **oldest** event is dropped,
counted in `ChannelStats::dropped` (`:421`) and logged per event — never
silently, because *the harness rule about unmeasured checks applies to discarded
data too* (`:136-141`).

The bound's own docstring states the assumption it was chosen under
(`event_server.rs:225-228`): *Control-plane granularity (PLAN-SERVICES §4: never
per token) means a healthy consumer is never near this.* That assumption is about
producers whose rate **this project sets** — F3's residency transitions happen
when the loading policy decides they do. F4's telemetry is the first candidate
producer whose rate is set by **somebody else**: one record per decided call
(`TelemetrySink::emit`, `telemetry.rs:379-383`, *"called once per decided call"*),
and the call rate is the agent's. What rate a bridge sustains is **unmeasured
here** — no benchmark was run — and that unknown is the finding, not a number:
64 was sized against a producer whose rate we own, and telemetry is not one.

### 6.1 What it would do to `PLAN-DEFERRED.md` §1's second trigger / §1 두 번째 방아쇠

§1's un-defer triggers are: filtering becomes an **isolation** requirement;
**or** *F7 reports a measured drop rate caused by unwanted fan-out*
(`PLAN-DEFERRED.md:45`).

Publishing telemetry would very likely make F7 report a measured drop rate — and
it would fire that trigger on a **reading it was not written for**. The trigger
is about *unwanted* fan-out: events reaching consumers that did not want them,
which server-side filtering fixes. Drops caused by a fast producer and a slow
consumer are **back-pressure**, and CosNotification does not fix them: its
`DiscardPolicy` and `MaxEventsPerConsumer` are names for the same discard F7
already performs. So the trigger would fire, a chapter would un-defer, and the
work it un-defers would not address the drops that fired it.

**And today the counter cannot tell the two apart.** `ChannelStats::dropped`
(`event_server.rs:418-421`) counts, by its own doc, *"queued events discarded: by
overflow (drop-oldest) or by a disconnect abandoning a backlog"* — and
`ChannelHandle::stop` adds a third contributor, counting every abandoned queued
event at shutdown (`:834-845`), so a clean stop increments the same number as an
overloaded consumer. One counter, three causes, and §1's trigger asks a question
about exactly one of them. This is a defect in the trigger's instrument that
exists whether or not anything ever publishes; it is recorded here and **not
fixed here**, because this batch's footprint is one document.

**요지.** 큐 상한 64는 *우리가 속도를 정하는* 생산자(F3의 상주 전이)를 기준으로
골랐고, F4의 텔레메트리는 **호출자가 속도를 정하는** 첫 생산자다 — 결정된 호출당
레코드 하나. 실제 호출률은 **여기서 측정하지 않았다.** 발행은
`PLAN-DEFERRED` §1의 두 번째 방아쇠(*원치 않는 팬아웃으로 인한 실측 드롭률*)를
당기겠지만, **의도와 다른 독법으로** 당긴다: 느린 소비자 때문의 드롭은 배압
문제이고 Notification의 필터가 고치는 문제가 아니다. 게다가 오늘
`ChannelStats::dropped`는 오버플로 드롭·연결 해제 시 폐기·`stop()` 시 폐기를 한
숫자로 합산하므로(`:418-421`, `:834-845`) 그 방아쇠의 계측기 노릇을 아예 할 수
없다. 발견했고, 이 배치에서는 **고치지 않는다** — 이 배치의 산출물은 문서 하나다.

### 6.2 A constraint on whatever publishes / 발행자에 걸리는 제약

`TelemetrySink`'s contract (`telemetry.rs:373-378`): an implementation *"may
write, count, forward or drop; it may not fail loudly, because a trace that can
break a call path is a worse instrument than no trace."* A publishing sink sits
on the MCP call path and `ChannelHandle::publish` (`event_server.rs:773`) takes
the channel's state lock to fan out. Two rules follow and they are not optional:
the sink enqueues and returns, never blocks; and its failures are counted, the
way `JsonLines` counts its write failures (`telemetry.rs:406`), never raised.
`orbweaver-giop`'s `guarded` registry, which refuses an outbound call while a
state guard is open (`event_server.rs:128-135`), is the machinery that would
catch the worst version of getting this wrong — but only in a debug build, and
only if a test does it.

**요지.** 싱크는 **시끄럽게 실패해서는 안 된다**(`telemetry.rs:373-378`).
발행하는 싱크는 MCP 호출 경로 위에 있고 `publish`는 채널 상태 락을 잡으므로,
규칙은 둘 — 넣고 즉시 반환하며 절대 블록하지 않을 것, 실패는 세되 올리지 말 것.

---

## 7. Options / 대안

Five, each with what would have to be true for it to be right, its cost, and the
oracle that would hold it.

### A — publish nothing; close the row as deliberately not built, with a trigger

**What would have to be true.** No named subscriber exists. PLAN-SERVICES §1
rule 2 — *only the subset a named consumer needs; every operation implemented
must name who calls it* — is the project's own standard, and §2 measured the
answer: nothing outside `orbweaver-giop` calls `ChannelHandle::publish`, and
nothing in the workspace is a `PushConsumer` except `PushConsumerServant` and the
spike that drives it.

**Cost.** The §6 feedback loop stays open, and it stays open visibly rather than
as a row that describes work. Somebody who wanted control-plane events gets a
document explaining why not, which is a worse outcome than events if a consumer
actually exists and a better one if none does.

**Oracle.** That the row's claim stays true: `ChannelHandle::publish` has no
caller outside `orbweaver-giop`, counted rather than grepped for a match, with a
negative control in the commit that adds it. CLAUDE.md's own lesson applies —
the grep this project trusted once *caught its own explanatory comment and missed
a real violation* (`guard.rs:188-192`), which is why `AuditReason`'s rule is a
type and the harness only counts constructors.

### B — publish a redacted projection of the D004 record, behind `--publish-events`, default off

**What would have to be true.** Two things, and the second is not checkable by
the flag. First, a subscriber exists whose need is the *operator's* need — it
wants the correlating fields, because a projection that drops them is not a
projection of this record, it is option A's event set with extra machinery.
Second, every subscriber is inside the trust boundary. The flag asserts the
second; the channel cannot check it, and §5 is why.

**Cost.** A second shape to keep in step with D004's nine keys and with the
console's `KEYS` array (`traces.rs:53-54`), drifting the first time a tenth key
lands. A deployment-wide switch standing in for a per-connection decision. And
the position §5 names: a servant that refuses one unauthenticated operation while
streaming decisions to the same audience.

**Oracle.** The `a_secret_in_a_session_reaches_no_trace_line` shape, one layer
out: a real session with secrets in the caller's scopes, an object key and the
arguments, driven through a real channel to a real consumer, asserting none of
them arrive — with something asserted *present* in the same test, so a publisher
that emits nothing cannot pass. Plus `service_sweep.sh` coverage of any new
operation.

**Verdict.** Blocked on the caller model for exactly the fields that motivate it.
Recommended against.

### C — publish only after a caller model exists (blocked on stream C)

**What would have to be true.** Identity reaches a servant: CSIv2 or the bridge's
`Caller` (D010 B2, `PLAN-DEFERRED.md` §11's trigger). Today it does not — no
`Caller`, no service context reaches `event_server`, re-measured 2026-08-19 and
recorded in §11.

**Cost.** The row stays open for an unknown time. That cost is zero code and one
accurate sentence, which is the cheapest kind.

**Oracle.** The same one §11 already names, because it is the same question:
when `destroy` becomes an authorization decision, subscription becomes one in the
same breath, decided by the same principal check.

### D — an in-process subscriber seam, no channel at all

**What would have to be true.** The reactor is in-process. F3's residency
transitions and F4's telemetry both are, and `event_server.rs:164-169` says so in
the course of explaining why `publish` skips the socket.

**Cost.** Almost none, and that is the point: `TelemetrySink`
(`telemetry.rs:379`) *is* the seam, `Trace` already holds one behind a
`Box<dyn TelemetrySink>` (`:456-462`), and a fan-out implementation holding
several is a small, testable type in `orbweaver-mcp` with no new crate edge.
What it does not give is a **remote** consumer — which is the only thing
PLAN-SERVICES §10's row is about.

**Oracle.** Unit tests, no wire, no fixture. Honest about its own limit: it
closes the §6 feedback loop and does not close the §10 row, and saying otherwise
would be reporting a measurement nobody took.

### E — publish an aggregate, not per-call events

`CallStats` is already the aggregate the promotion policy reads — *"`CallStats`
is the promotion policy's only input"*, `interceptor.rs:1284`, held by the
telemetry stage at `:1203-1207` — and a counter snapshot carries no session, no
caller and no per-call operation. **Rejected on the clock.** An aggregate is
useful because it is periodic, there is no clock in the interceptor chain and
this project refuses to add one (ARCHITECTURE §7, D004's no-duration rule), so
the snapshot would have to be emitted on some caller's tick — which puts the
event rate back in an external party's hands and reintroduces §6 while giving up
§3's trigger value. Named so that the next person does not have to rediscover it.

**대안 요약.** A: 아무것도 발행하지 않고 §10 행을 *의도적 미구축*으로 방아쇠와
함께 닫는다(오라클: `publish`의 외부 호출자 0을 세는 검사 + 음성 대조). B:
플래그 뒤에 D004 레코드의 편집본을 발행한다 — 동기가 되는 필드들에 대해 호출자
모델에 막혀 있으므로 반대. C: 호출자 모델 이후에만 발행 — 비용은 코드 0과 정확한
문장 하나. D: 채널 없이 **프로세스 내** 구독 시임 — `TelemetrySink`가 이미 그
시임이며, §6의 되먹임 고리는 닫지만 §10 행은 닫지 못한다. E: 집계 발행 — 시계가
없어 기각.

---

## 8. Recommendation / 권고

**Adopt A together with D, and take C's trigger as A's trigger. Reject B.**

Concretely: publish nothing over the wire; build the in-process fan-out seam on
`TelemetrySink` when a reactor needs it, as its own batch with its own oracle;
and close PLAN-SERVICES §10's row as *deliberately not built*, with the un-defer
trigger being the one `PLAN-DEFERRED.md` §11 already carries — a caller model in
the event servant.

Three legs, in ascending order of weight.

1. **Nothing names a subscriber, and rule 2 is the project's own standard.**
   Measured in §2: no crate outside `orbweaver-giop` publishes, and nothing in
   the workspace subscribes except a test consumer and the spike that drives it.
   PLAN-SERVICES §1 rule 2 calls an operation with no consumer *surface to get
   wrong*.

2. **Strip what cannot cross and the event is already free.** §4's two lists
   leave `{decision, target, operation, outcome}` for resolved calls. An
   in-process reactor gets exactly that, plus everything else, from the sink it
   already has — with no bytes on a wire, no queue to overflow, no counter that
   conflates three causes, and no audience to identify. The remote version buys a
   remote consumer and nothing else, and no remote consumer exists.

3. **The channel cannot tell two subscribers apart, so "redacted" has no
   referent.** This is B's defeat and it is structural, not a matter of care.
   And the asymmetry from §5 is the sentence to hold onto: the same servant
   refuses one unauthenticated operation because it does not know who is calling.
   A stream of every gate decision, to the same audience, is the larger claim.
   Shipping it while refusing `destroy` would be indefensible the first time
   somebody asked.

**Why not simply wait for C.** Because waiting is what A *is*, and A says so with
a trigger instead of leaving a row that reads like scheduled work. The difference
between "deferred" and "open" is not the code; it is whether the next planning
pass spends time on it. This project has already measured what an open row that
describes finished work costs — a planning pass, which no test can go red on.

**권고: A + D를 채택하고 C의 방아쇠를 A의 방아쇠로 삼는다. B는 기각.** 와이어로는
아무것도 발행하지 않고, 반응자가 필요해지면 `TelemetrySink` 위의 프로세스 내
팬아웃 시임을 별도 배치로 짓고, §10 행은 *의도적 미구축* + `PLAN-DEFERRED` §11의
방아쇠로 닫는다. 근거 셋: (1) **이름 붙은 소비자가 없다** — §1 규칙 2가 그런
연산을 *잘못될 표면*이라 부른다. (2) 넘을 수 없는 것을 걷어내면 남는
`{decision, target, operation, outcome}`는 프로세스 내 반응자가 이미 공짜로 받는
것이다 — 원격 버전이 사는 것은 원격 소비자 하나뿐이고, 그것이 없다. (3) 채널은
두 구독자를 구별하지 못하므로 "편집본"에는 **대상이 없다** — 그리고 같은 서번트가
`destroy` 하나는 거부하면서 모든 게이트 결정의 스트림은 흘린다는 위치는 질문받는
첫날 무너진다.

---

## 9. What approval would mean / 승인의 의미

**Approval commits to four things.**

1. **The §10 row is closed as deliberately not built**, with `PLAN-DEFERRED.md`
   §11's trigger, in the text §11 of this document supplies. A batch applies it;
   this document does not.
2. **The D004 record stays the operator's artifact and never the wire's.** No
   projection of it is published to a channel. If something is ever published,
   it is a closed vocabulary defined as a total function from the record, in one
   place.
3. **§4's two lists are the standing answer** for whatever eventually publishes —
   including a future CosNotification layer, which inherits F7's channel as its
   transport core (`PLAN-DEFERRED.md` §1) and would inherit this field rule with
   it. The seam between the lists is one predicate: did the capability table
   resolve this call.
4. **Subscription and `destroy` are one authorization question**, answered
   together when a caller model reaches the event servant, or not at all.

**Approval does not commit to** building option D — that is a batch with its own
oracle and its own first-pass rate; to any change in the D004 record or the
console's `KEYS`; to any CosNotification work; to a schedule for stream C; or to
fixing the `ChannelStats::dropped` conflation §6.1 records, which is a separate
finding with a separate owner.

**No policy amendment is required.** Nothing here touches D001's data clause,
D002's oracle-blind-logic clause or D003's separate-process clause. This is a
scoping decision about what an existing servant carries, and saying that plainly
is part of reporting it honestly — it is a smaller decision than D001 through
D004.

**승인이 약속하는 것 넷.** (1) §10 행을 §11의 방아쇠와 함께 *의도적 미구축*으로
닫는다. (2) D004 레코드는 운영자의 산출물로 남고 와이어의 것이 되지 않는다 —
발행한다면 레코드로부터의 전사(全射) 함수로 정의된 닫힌 어휘다. (3) §4의 두
목록이 앞으로 무엇이 발행하든 **표준 답**이며, F7 채널을 전송 코어로 물려받을
CosNotification도 이 필드 규칙을 함께 물려받는다. (4) 구독과 `destroy`는 **하나의
인가 질문**이다. **약속하지 않는 것:** 대안 D의 구축, D004 레코드나 콘솔 `KEYS`의
변경, CosNotification 작업, 스트림 C의 일정, §6.1이 기록한 `dropped` 합산 결함의
수정. **방침 개정은 필요 없다** — D001·D002·D003 어느 조항도 건드리지 않는다.

---

## 10. What was verified, and what was not / 검증된 것과 아닌 것

**Verified directly, 2026-08-19, by reading this worktree at `4917471`:** the
telemetry stage's constant and occupant (`interceptor.rs:279`, `:306`, `:349`,
`:1203`, `:1239`); the nine-key record, its private fields, its single
construction site and its sink trait (`telemetry.rs:122`, `:136`, `:272`,
`:379`, `:406`, `:456`, `:531`); the unresolved arm setting `target` and
`operation` from the caller (`lib.rs:580-589`, `interceptor.rs:916-921`); the
ledger rule as a type and its harness group (`guard.rs:201`, `:206`, `:242-246`,
`:252`; `run_checks.sh:1836`, `:1871`); the channel's bound, its counters, its
handle and its in-process publish (`event_server.rs:229`, `:414`, `:421`,
`:743`, `:773`, `:834`); `destroy`'s refusal and its stated reason
(`event_server.rs:52-63`); the module doc naming F4's telemetry as an expected
in-process publisher (`event_server.rs:164-169`); the console's `KEYS`
(`traces.rs:53-54`); that `event_server` is named outside its own file only by
`orbweaver-giop` modules, two of its own tests and `spike_events.rs`; and the
plan and deferral text quoted throughout (`PLAN-SERVICES.md:351` and §1, §4;
`PLAN-DEFERRED.md:45`, `:54`, §1, §11; `ARCHITECTURE.md` §5, §6, §7; `PLAN.md`
§4.7, §4.8). `python3 spikes/decision_status.py` was run against this file.

**Unverified, stated plainly.** No test was run and `spikes/run_checks.sh` was
not taken — this batch produces a document, and the harness holds a machine-wide
lock. **No event rate was measured**: §6's claim is that 64 was sized against a
producer whose rate this project sets and telemetry is not one, and it
deliberately attaches no number to a bridge's call rate, because none was taken.
**No consumer was built**, so no drop rate was measured and `PLAN-DEFERRED.md`
§1's second trigger remains unfired on evidence as well as on decision. Whether
omniORBpy ships `CosNotification` stubs is still unverified — §1 says so and this
document did not probe it. The **cost of option D** is called "almost none" from
reading the types, not from writing it; that is an estimate and a batch would
price it. And no claim is made here about what a *future* CSIv2 caller model
would allow a subscriber to see: §9's fourth commitment is that the question is
answered then, not that this document answers it.

**검증한 것:** 위 파일:행 인용 전부를 이 워크트리(`4917471`)에서 직접 읽어
확인했고, `decision_status.py`를 이 파일에 대해 실행했다. **검증하지 않은 것을
그대로 적는다:** 테스트를 돌리지 않았고 하네스를 잡지 않았다(문서 배치가 머신
전역 락을 잡을 이유가 없다). **이벤트 발생률은 측정하지 않았다** — §6은 상한 64가
*우리가 속도를 정하는* 생산자를 기준으로 골라졌다는 사실만 말하고 호출률에는 어떤
숫자도 붙이지 않는다. **소비자를 짓지 않았으므로** 드롭률도 측정되지 않았고,
`PLAN-DEFERRED` §1의 두 번째 방아쇠는 결정으로도 증거로도 발화하지 않았다.
omniORBpy의 `CosNotification` 스텁 유무는 여전히 미측정이다. 대안 D의 비용
"거의 없음"은 타입을 읽고 낸 추정이지 실측이 아니다.

---

## 11. What would change in `PLAN-SERVICES.md` §10, as text / §10에 적용될 텍스트

Unapplied. §10's row (line 351) is quoted in §1 above. Under each option it
becomes:

**Under A + D (recommended):**

```
| CosEvent → telemetry feedback | F4 + F7 | **deliberately not built** (D011) —
both halves exist and nothing publishes, because nothing subscribes: rule 2 wants
a named consumer and there is none. Un-defer trigger: a caller model in the event
servant (PLAN-DEFERRED §11's trigger, stream C) — subscription and `destroy` are
one authorization question. The in-process feedback loop is `TelemetrySink`, not
the channel |
```

**Under B:** the row becomes a batch — *publish a redacted projection to a
well-known channel behind `--publish-events`, default off; the field rule is
D011 §4; the oracle is a real consumer asserting the forbidden fields absent and
something present.* D011 recommends against it.

**Under C:** the row becomes *blocked on stream C* and moves nowhere until a
caller model lands — the same sentence as A's trigger, without A's closure, and
therefore a row that keeps costing planning passes.

**Under D alone:** the row is **not** closed, because D builds no wire path.
The honest edit would be to leave the row open and add a line to `PLAN-MOE`'s F4
about the in-process seam. That is why the recommendation is A *and* D rather
than D.

**Under E:** rejected in §7; no row text is offered for it.

Also worth stating and **not** proposed here: `PLAN-DEFERRED.md` §1's second
trigger needs the drop counter it names to distinguish overflow from
disconnect-abandon from stop-abandon (§6.1). That is a change to
`event_server.rs` and to §1's trigger text, owned by whichever batch touches
either — not by this document, whose footprint is this file.

**§10에 적용될 텍스트(미적용).** 권고안(A+D) 아래 그 행은 *의도적 미구축(D011)*이
되고, 방아쇠는 `PLAN-DEFERRED` §11의 것 — 이벤트 서번트의 호출자 모델 — 이 되며,
프로세스 내 되먹임은 채널이 아니라 `TelemetrySink`라고 적는다. B 아래에서는 그
행이 배치가 되고(권고는 반대), C 아래에서는 *스트림 C에 막힘*이 되며(닫히지 않아
계획 비용을 계속 낸다), D만으로는 **닫히지 않는다** — 와이어 경로를 짓지 않기
때문이며, 그것이 권고가 D 단독이 아니라 A+D인 이유다. 아울러 여기서 제안하지
**않는** 것: §6.1의 드롭 카운터 분리는 `event_server.rs`와 §1 방아쇠 문구를
건드리는 일이고, 이 문서의 footprint는 이 파일 하나다.

---

## 12. What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is built by this document and no file outside it changes. The event
vocabulary's exact members, if one is ever defined, are the batch's; §4 gives the
two lists and the predicate that separates them, not a struct. Whether the
in-process fan-out seam holds a `Vec<Box<dyn TelemetrySink>>` or something else
is `orbweaver-mcp`'s question. The `ChannelStats::dropped` conflation is recorded
and left. CosNotification stays deferred on §1's own terms, and this document
does not fire, weaken or strengthen either of its triggers — it observes that
publishing would fire the second one on a reading it was not written for, which
is a reason to be careful about publishing, not an amendment to §1. And nothing
here says when stream C lands; §9's fourth commitment is about the *order* of two
questions, not about a date.

이 문서는 아무것도 짓지 않으며 자기 파일 밖의 어떤 파일도 바꾸지 않는다. 이벤트
어휘의 정확한 멤버는 배치의 몫이고 §4가 주는 것은 구조체가 아니라 두 목록과 그
둘을 가르는 술어다. 프로세스 내 팬아웃 시임의 형태는 `orbweaver-mcp`의 질문이다.
`ChannelStats::dropped`의 합산 결함은 기록하고 남겨 둔다. CosNotification은 §1의
조건 그대로 유예 상태이며, 이 문서는 그 방아쇠를 당기지도 고치지도 않는다 —
발행이 두 번째 방아쇠를 *의도와 다른 독법으로* 당길 것이라는 관찰은 발행을
조심하라는 이유이지 §1의 개정이 아니다. 스트림 C의 일정에 대해서도 아무 말도 하지
않는다 — §9의 네 번째 약속은 두 질문의 **순서**에 관한 것이지 날짜에 관한 것이
아니다.
