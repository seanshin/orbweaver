# D029 — What a complete ORB would mean, and the three gaps that are not operation counts

**STATUS: PROPOSED** — drafted 2026-08-26 on a direction that the ORB should
reach a complete form, immediately after D019 step 4 landed. Every figure was
measured that day against the tree. Not self-approvable: §6 proposes a
definition of *done* for the ORB, which decides what later work is owed.

**상태: 제안** — 2026-08-26, ORB가 완성 형태로 가야 한다는 지시에서, D019 4단계가
착지한 직후 작성.

---

## 1. Where this starts / 출발점

**D019's four responsibilities are all landed.** The initial-references table,
the two conversions, the configuration, and — as of today — the transport and
the root POA. `Server::bind`, `Pool::new`, `Pool::with_limits`, `Pool`'s
derived `Default` and `Poa::new` are `pub(crate)`; `Orb::server` and
`Orb::pool` are the way in; thirteen hand-construction sites migrated across
thirty-one files.

**So the question changes.** D019 §5 was deliberately *minimal* — *"this
document proposes the object and its four named responsibilities, and nothing
beyond them"* — and it refused a list by name. A direction to reach a
**complete** form is a different question, and it cannot be answered by
resuming the refused list. It has to be argued.

## 2. The measured gap / 측정된 간극

CORBA 3.4 §8.3 names sixteen operations on `CORBA::ORB` that a Rust ORB could
plausibly carry. We name **six**:

| | |
|---|---|
| **named today** | `string_to_object`, `object_to_string`, `resolve_initial_reference`, `list_initial_services`, `register_initial_reference`, plus `resolve_url` which is ours and not the standard's |
| **absent** | `create_policy`, `run`, `shutdown`, `destroy`, `work_pending`, `perform_work`, `get_service_information`, `create_list`, `get_default_context`, `register_value_factory` |

**Ten absences is not ten pieces of work**, and counting them that way is the
trap this document exists to avoid. Classified by *why* each is absent:

- **Refused with a reason** (D019 §5): `run`, `shutdown`, `work_pending`,
  `perform_work` — refused as *a C++ event-loop shape*, which is a refusal
  about spelling. See §3.1, where that refusal turns out not to cover the
  thing underneath it.
- **Consistent with a wire exclusion**: `register_value_factory` serves
  valuetypes, which `PLAN.md` §4.4 excludes from the v1 wire. Adding the
  factory would be surface for a type we refuse — it is absent *correctly*.
- **No consumer has ever asked**: `get_service_information`, `create_list`,
  `get_default_context`. The last two serve DII plus `Context`, and `Context`
  is a CORBA feature this project has never had a caller for.
- **Absent with the machinery already built**: `create_policy`. See §3.2.

## 3. The three gaps that are real / 진짜인 세 간극

### 3.1 The ORB owns the transport and has no lifecycle

Measured: `orb.rs` mentions shutdown or destroy **once**, in prose. `Server`
has a stop flag polled by the accept loop and every connection thread. So as of
today **an ORB can hand out N servers and cannot stop one of them**, and that
became true this morning — before step 4 the caller held every `Server` it
built, and stopping was its own business.

**D019 §5 refused `run`/`shutdown`, and the refusal was narrower than it
reads.** Its subject was *"a faithful `ORB_init` signature, `ORB::run`/
`shutdown` semantics, thread policies … copied because the C++ mapping has
it."* That is a refusal to import an **event-loop model** — a main thread
parked in `run()` — which this ORB genuinely does not have and should not
grow. It is not a finding that stopping what you handed out belongs to
somebody else.

**This is the one gap step 4 created rather than revealed**, and that is
exactly the kind D019 §6 says belongs in a decision: the API became one-way,
so the asymmetry — the ORB gives and cannot take back — is now a property of
the product rather than of a spike.

*4단계가 **드러낸** 것이 아니라 **만든** 유일한 간극이다. API가 일방향이 되었으므로,
ORB가 주기만 하고 거두지 못한다는 비대칭이 이제 스파이크의 성질이 아니라 제품의
성질이다.*

**Closed 2026-08-26 by O1**, whose design answer and refusal are
[`D034`](D034-stopping-what-the-orb-handed-out.md) and whose bound is the
rustdoc on `Orb::shutdown`. Neither is restated here; what §6.1's lifecycle row
records is **how far the row moved**, which is less far than *closed*.

*2026-08-26 O1이 닫았다. 설계의 답과 거절은 D034, 한계는 `Orb::shutdown`의
러스트독에 있다 — 여기서 다시 적지 않는다.*

### 3.2 Seven policies exist as types, and nothing lets a caller choose one

D020 Stage A landed `ThreadPolicy`, `LifespanPolicy`, `IdUniquenessPolicy`,
`IdAssignmentPolicy`, `ServantRetentionPolicy`, `RequestProcessingPolicy` and
`ImplicitActivationPolicy` as enums carrying `NAME`/`SECTION`/`STANCE`/
`SPEC_DEFAULT`, with `Policies::spec_violations()`.

**They are a description of what this ORB does, not a choice anybody makes.**
There is no `create_policy` and no policy argument on `create_poa`. Stage A was
explicit that writing down the implicit choice was the batch and implementing
alternatives was not — that was right then. What has changed is that the ORB
now owns POA creation, so the door the standard puts the policies through
(`ORB::create_policy` → `POA::create_POA`) is a door we now have both sides of.

**The valuable half is not the alternatives.** It is that a policy a caller
*states* can be checked against a policy the code *implements*, and
`spec_violations()` already computes exactly that comparison against nothing.

### 3.3 Two ORB features still have no chapter, and D018 said so first

Measured today: **`PLAN-DEFERRED` contains zero mentions of Portable
Interceptors or BiDirectional GIOP.**

D018 §3.3 named this as the gap in the planning — *"they are not deferred; they
are simply unmentioned, which is the one state this project's own rules do not
allow"* — and put it third in its own order. Items 1 (`def_kind`) and 2 (the
seven POA policies) landed. **Item 3 did not**, and today's batch that gave
eight CORBAservices a reason and a trigger did not cover these two, because
they are ORB features rather than services.

This is the cheapest item in this document and the only one whose deliverable
is a decision rather than code.

## 4. What complete must not mean / 완성이 뜻하면 안 되는 것

- **Not §8.3's operation list.** Six of sixteen is not a score to raise.
  `register_value_factory` is absent *because* valuetypes do not cross this
  wire, and adding it would be a surface for a refused type — a worse state
  than the gap.
- **Not an event loop.** §3.1 asks for stopping, not for `run()`. If a design
  cannot separate the two, that is a finding that stops the batch.
- **Not `ORB_init`.** D019 §5's refusal is unchanged and approved with the
  refusal intact.
- **Not "complete" as an unmeasurable word.** Which is §6.

## 5. What is proposed / 제안

**Re-ordered by §6's criterion, which is priority zero.** O1 and D030's L1 are
not peers of the rest: each closes an entire transparency, and O2/O3/O4 close
none — they are hygiene, correctness and record-keeping, all worth doing and
none of them completion. Where the two orderings disagree, §6 wins.

1. **O1 — lifecycle.** Without it "removed at runtime" has no implementation,
   so the fifth transparency cannot even be tested.
2. **D030 L1 — the servant seam.** Language transparency leaks by construction
   until a non-Rust servant can be dispatched into.
3. **A leak test per transparency** (new, see below) — because §6 says
   transparency is hunted, not confirmed, and today there is no instrument.
4. O2, O3, O4 in their original order.

### O0 — a leak test per transparency (`spikes/`, `crates/orbweaver-test`)

Five properties, each expressed as *a caller holding only a reference cannot
tell X*, each with a fixture that changes X underneath a live caller and
asserts the caller's observations are unchanged. Move the object; evict and
reload it; answer from a different servant; answer from a different language
once L1 lands.

**The instrument comes before most of the fixes**, because without it every
claim in §6.1's table is a reading rather than a measurement — and this project
has spent the day learning what a reading is worth. Where a transparency cannot
yet be tested (lifecycle, until O1), the test exists and is a **counted
`SKIPPED` naming what it waits on**, never absent.

Ordered by what a defect would cost.

### O1 — the ORB can stop what it handed out (`orbweaver-giop`, `orbweaver-object`)

**Landed 2026-08-26.** The design question below was answered in writing first,
in [`D034`](D034-stopping-what-the-orb-handed-out.md); the bound is the rustdoc
on `Orb::shutdown`; the oracle asked for below was built and is
`crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`. The paragraph
that follows is left as it was written, because it is what was proposed and
editing it to match the answer would falsify the proposal rather than record it.

An ORB-level shutdown that stops the servers and pools it created, and says
what it does to work in flight. **Not `run`.** The design question to answer
first and in writing: whether shutdown is *graceful* (stop accepting, finish
in-flight, then close) or *immediate*, and what a caller who holds a `Server`
the ORB is stopping observes. `Server`'s stop flag and `STOP_POLL` are the
mechanism that exists; the question is who owns the decision.

**Oracle.** A peer mid-call when shutdown lands. `spikes/half_reply_peer.py` is
the shape — a peer that can be held at a chosen point — and the measurement is
what the client *sees*, not what our counters say.

### O2 — a policy becomes a choice (`orbweaver-object`, `orbweaver-giop`)

`create_policy` in the standard's spelling, and `create_poa` taking policies.
The payoff is `spec_violations()` acquiring an argument: a stated policy set
compared against what the code implements, refused **by name** where they
differ. Where a policy value is not implemented, the refusal says so rather
than the POA silently behaving as the value it always had.

**Precondition.** None. This is `orbweaver-object` plus one ORB method.

### O3 — Portable Interceptors and BiDirectional GIOP get a chapter (documents)

`PLAN-DEFERRED`'s shape: what it is, why deferred, an observable trigger.
D018 §3.3 sketches both and the sketches are the starting point, not the
answer — in particular whether our in-process interceptor chain and the
standard's per-ORB one *are the same idea at different scopes* is a real
question and the chapter should answer it rather than assume it.

### O4 — an operator's flag reaches a peer, not a test (`crates/*/src/bin`, `spikes/`)

ORB step 4 named this itself as the highest-value next step: **no spike binary
accepts `-ORB…` arguments**, so `OrbConfig::from_orb_args` — §8.5.1's own flag
parsing, refusing zeros, applied whole or not at all — is measured by unit test
and never by a deployment. Give one spike the flags and let the harness set a
limit from the command line and watch a peer hit it.

That is also what makes D015's acceptance sentence — *"without editing Rust,
without a rebuild"* — true at the ORB layer rather than one layer above it.

## 6. What "complete" means — the priority-zero criterion / "완성"의 정의 — 0순위 기준

**Set by the project owner, 2026-08-26. This is the definition; the rest of
this document is subordinate to it, and so is every other plan document.**

> **The ORB is complete when there is no leak in this transparency: a caller
> can invoke any target holding only a reference, without knowing its
> location, its backend, its language, or whether it is currently loaded — and
> that property does not break when targets are added, removed, moved, loaded
> or evicted at runtime.**
>
> *ORB의 완성은 **호출자가 참조만으로 임의의 대상을 그 위치·백엔드·언어·적재
> 상태를 모른 채 호출할 수 있고, 대상이 런타임에 추가·제거·이동·적재·축출돼도 그
> 성질이 깨지지 않는다**는 투명성에 **구멍이 없을 때** 완성된 것이다.*

**Why this replaces the definition drafted earlier in this document.** The
draft asked whether a foreign client could bootstrap, whether an operator could
change numbers, whether absences had reasons. Every clause was true and every
clause was about *us* — what we had built, documented and exposed. This one is
about **what the caller cannot tell**, which is the only thing an ORB is for,
and it is falsifiable in a way a feature list is not: **you do not confirm
transparency, you hunt leaks in it.**

It also reframes the entire document. §2 counted six of sixteen operations and
§3 named three gaps; under this criterion an absent operation matters exactly
as much as the leak it causes and not at all otherwise, which is why
`register_value_factory` being absent is not a gap while §3.1's missing
lifecycle is a large one.

### 6.1 The five transparencies, and where each leaks today

Each is a claim that can be **refuted by a test**, which is how this gets
worked on. Measured 2026-08-26; every "leaks" below is a defect to close, not a
feature to add.

| Transparency | The caller must not be able to tell | Status today |
|---|---|---|
| **Location** | where the target runs | **measured, with a known leak**: `LOCATION_FORWARD` and `_PERM` are served and followed, and R7 rewrites an IOR for a dialable address — but `Connection::move_to` restored a hand-written field list and dropped two configured limits across every forward until today, so the *caller's* limits changed when the object moved. Fixed; the class is the leak to watch. **A second instance, found 2026-08-26 and not fixed**: `moe::Router::select` returns `ExpertSeq` — N object references, each an `Ior` stored verbatim from `register_expert` and marshalled inline with host, port and object key. A caller learns where every candidate expert runs, which is exactly what this row says it must not be able to tell. `corpus/golden/22`'s own comment beside the operation already says so — *"widening reach by N addresses at once is precisely the case §4.7's bearer-address rule exists for"* — and §4.7's rule is the authority half of the same fact. Recorded, not changed: `select` is served and has consumers. **A third thing under this row, closed 2026-08-26 and not one of the two leaks above**: the probe path *lied*. See the `LocateRequest` subsection below for what moved and why the row's status sentence is unchanged. |
| **Backend** | what implements it | mostly held: a servant is behind a POA and a reference; but `spike_experts`' server root key collides with its derived registry key, which is a backend detail reaching a name. |
| **Language** | what it is written in | **the construction leak is closed; three narrower ones remain** (2026-08-26). A Python servant is dispatched into by `orbweaver_gen::pyservant`, and `tests/python_servant.rs` compares one against the generated Rust servant for the same contract — 19 calls × 3 GIOP versions × 2 byte orders, **byte-identical replies**, with a negative control that perturbs five answers and asserts each is seen. **The peer half is now measured in both byte orders (2026-08-26):** omniORB's client little-endian, and JacORB's big-endian — the order taken from §15.4.1's flag byte on every request rather than from the peer's language — with the Python servant's 11 replies byte-identical to a Rust servant's for the same driver run, at IIOP 1.2 and 1.1. What remains is listed in §6.1.1 and none of it is the old *"cannot be a target at all"*. |
| **Activation / load** | whether it is loaded right now | **leaks, and now measured (2026-08-26)**: the leak is `moe::Router::select`, and it is *residency-blind by omission rather than by absence of data*. `mirror_residency` keeps `Offer::residency` live in the very store `select` reads, and `orbweaver-trading`'s query grammar has a `residency` field, but `Constraints::to_query_text` never names it — so an OFFLOADED expert comes back in the sequence and dialling it answers `OBJECT_NOT_EXIST` where a resident one answers. `expert_service.rs:882-891` records this as intended: *"the caller's cue to `prefetch`"*. That makes the leak a **design choice written down**, not an oversight, which is the strongest form for it to be in before it is decided. `Router::dispatch` is *not* the operation that would close it — it is refused (D006 option E), and its own reason is now known to be false as written (see D006's 2026-08-26 amendment). The closer is a POA-level activation path, because the criterion says *any* target, and a fix inside one application contract closes it for one contract. |
| **Lifecycle stability** | that the above survives add / remove / move / load / evict at runtime | **was "partly unmeasurable"; as of 2026-08-26 it is measurable and leaking for a reason that is now named.** O1 landed: `Orb::shutdown` stops the servers and pools the ORB created, and `crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs` measures what a peer mid-call observes across it — from the peer's own socket, three GIOP versions × two byte orders, with four negative controls that were each run red. So *"removed at runtime"* now has an implementation and a test. **What did not move is the transparency of the removal**: a caller of a removed server can tell immediately, because there is nowhere else for its request to go, and closing that needs a second endpoint and a redirect — `LOCATION_FORWARD` served for a *name* rather than for an object, which is item 3 of the event-channel subsection below and which O1 does not touch. The argument and the refusal (graceful, at request granularity; immediate refused; *not* `run()`) are `docs/decisions/D034-stopping-what-the-orb-handed-out.md`; the bound is the rustdoc on `Orb::shutdown` and is not restated in either. Also measured that day and not changed: **17 of this workspace's 63 serve sites pass `|| false`** — seventeen processes that are still stopped only by being killed. They are now *fixable* rather than fixed. |

*다섯 가지 각각은 **테스트로 반증 가능한 주장**이며, 그것이 이 작업의 방식이다.
투명성은 확인하는 것이 아니라 **구멍을 사냥하는 것**이다.*

**2026-08-26 측정 — 위치 행과 적재 행 두 곳이 갱신되었다.** 두 구멍 모두
`moe::Router::select` 하나에 있다. (1) `select`는 `ExpertSeq`를 돌려주는데 그
원소는 `register_expert`가 준 `Ior`를 그대로 담아 호스트·포트·객체 키를 인라인으로
실어 보낸다 — 호출자가 후보 전문가 각각이 **어디서 도는지** 알게 되며, 이는 위치
행이 알 수 없어야 한다고 적은 바로 그것이다. (2) `select`는 **데이터가 없어서가
아니라 묻지 않아서** 적재 상태에 눈이 멀어 있다: `mirror_residency`가 `select`가
읽는 바로 그 저장소에 `Offer::residency`를 최신으로 유지하고 질의 문법에는
`residency` 필드가 있는데, `to_query_text`가 그 이름을 한 번도 쓰지 않는다. 그래서
축출된 전문가가 목록에 돌아오고, 그것을 걸면 `OBJECT_NOT_EXIST`가 온다 —
`expert_service.rs:882-891`은 이것을 *"호출자가 `prefetch`하라는 신호"*로 **의도된
설계라고 적어 두었다.** `Router::dispatch`는 이 구멍을 막는 연산이 **아니다**:
거절되어 있고(D006 E안), 그 거절 사유 자체가 오늘 거짓임이 밝혀졌다(D006
2026-08-26 개정). 기준이 말하는 것은 *임의의* 대상이므로, 막는 자리는 응용 계약
하나가 아니라 POA 수준의 활성화 경로다. 둘 다 **기록만 하고 바꾸지 않았다** —
`select`는 서빙 중이고 소비자가 있다.

**2026-08-26 측정 — 생애주기 행도 같은 날 옮겨졌다.** O1이 착지했다:
`Orb::shutdown`이 ORB가 내어준 서버와 풀을 멈추고,
`crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`가 **통화 중인 피어가
자기 소켓에서 무엇을 보는지**를 잰다 — GIOP 3개 버전 × 바이트 순서 2가지, 그리고
각각 붉게 만들어 본 부정 대조군 4개. 그래서 *"런타임에 제거됨"*은 이제 구현과
테스트를 갖는다. **옮겨가지 않은 것은 제거의 투명성이다**: 제거된 서버의 호출자는
즉시 알아차린다 — 요청이 갈 다른 곳이 없기 때문이다. 그것을 막으려면 두 번째
엔드포인트와 리디렉션이 필요하고, 그것은 객체가 아니라 **이름**에 대한
`LOCATION_FORWARD`이며 아래 이벤트 채널 절의 3번 항목이다. 논증과 거절(요청 단위의
우아한 종료, 즉시 종료 거절, `run()` 아님)은 D034에 있고, 한계는 `Orb::shutdown`의
러스트독에 있으며 어느 쪽도 다시 적지 않는다. 같은 날 측정하고 **바꾸지 않은 것**:
이 워크스페이스의 serve 지점 63개 중 **17개가 `|| false`를 넘긴다** — 여전히 죽여야만
멈추는 프로세스 열일곱이다. 이제 *고칠 수 있게* 되었을 뿐 고쳐진 것은 아니다.

#### Every row above now has a test, or a counted skip that says why not (2026-08-26)

**No row's status changes here. This subsection cites instruments; it does not
move a verdict.** §5 O0 landed and, on the same day, reached the harness:

| Row | The instrument that could refute it | What it says today |
|---|---|---|
| Location | `what_a_caller_can_tell.rs` — a move under a live caller, and the caller's limits across it | measures |
| Backend | the same file — the servant behind one reference replaced mid-session | measures |
| Language | `spikes/leak_tests.sh`'s language leg | counted `SKIPPED`: waits on a Python servant mountable as a `Dispatch` in a server the test owns |
| Activation / load | its activation leg | counted `SKIPPED`: waits on a POA-level activation path that reloads an evicted target |
| Lifecycle stability | `spikes/orb_shutdown.sh` (D034) measures the removal; the leak leg is a counted `SKIPPED` | **the blocker changed on 2026-08-26 and the row did not.** It no longer waits on *a redirect emitted for a name* — that is built and measured (`crates/orbweaver-giop/tests/forward_for_a_name.rs`). It waits on **X**: a decision that the reference `Orb::server` hands out is *indirect*. See §6.1's lifecycle subsection, which is also why a forward can never be emitted by the party that went away |

Two things are worth stating rather than inferring. **A test existing does not
move a row** — the two rows with a measuring leg are the two that already read
*measured, with a known leak* and *mostly held*, and the leg refutes neither
leak. And **the three skips are the valuable half**: each is a counted `SKIPPED`
naming one blocker, so D031's ledger prints them under this row on every run and
the next batch is scoped from a sentence rather than from a reading. A leg that
did not exist and a leg that cannot run yet used to print identically — as
nothing.

The controls are in the tree rather than in a commit message:
`spikes/leak_controls.sh` puts each leak back and requires the test to see it,
by exit code and in the test file's own sentence, and it runs in the harness
ahead of the legs so that a green leg is evidence about a leak rather than about
a switch that has stopped working.

*여기서 어떤 행의 상태도 바뀌지 않는다. 이 절은 **계기를 인용**할 뿐 판정을 옮기지
않는다.* §5 O0*이 착지했고 같은 날 하네스에 닿았다. 두 가지는 추론이 아니라 명시해
둘 값어치가 있다. **테스트가 생겼다는 것이 행을 옮기지는 않는다** — 재는 다리를 가진
두 행은 이미 "알려진 구멍과 함께 측정됨", "대체로 유지됨"이라 적혀 있던 두 행이고,
그 다리는 어느 구멍도 반증하지 않는다. 그리고 **스킵 셋이 값어치 있는 절반이다**:
각각이 장애물 하나를 이름 붙인 계수되는 `SKIPPED`이므로 D031의 원장이 매 실행마다 이
행 아래에 그것을 찍고, 다음 배치는 읽기가 아니라 **문장**에서 범위를 잡는다. 존재하지
않는 다리와 아직 돌 수 없는 다리는 예전에는 똑같이 — 아무것도 아닌 것으로 — 찍혔다.
대조군은 커밋 메시지가 아니라 트리에 있다: `leak_controls.sh`가 각 구멍을 되돌려 넣고
테스트가 그것을 보는지를 요구하며, 하네스에서 다리들보다 **먼저** 돈다 — 그래야 초록인
다리가 구멍에 대한 증거이지 고장 난 스위치에 대한 증거가 아니다.*

#### Location, for a `LocateRequest` — the probe answered "nowhere" (2026-08-26)

**The row's status sentence does not move, and saying so is the point.** Both
leaks it names are untouched: `Connection::move_to`'s field list is the class to
watch, and `moe::Router::select` still hands a caller N addresses. What closed is
a third defect that sits under this row and was not on it, because nobody had
looked at the *earlier* of the two moments a forward can be given.

`LOCATION_FORWARD` on a `Request` is served and followed — that is the row's
"measured" half. `OBJECT_FORWARD` on a `LocateRequest` is the same answer given
one message earlier, before the caller has spent an invocation, and this ORB
could **name that status and not serve it**: `encode_locate_reply` wrote a
request id and a status word and stopped, while the serve loop decided the
answer by asking `Dispatch::knows`, a boolean. Measured on this workspace before
the change, with a servant whose object had moved: `Connection::locate()`
answered `Ok(Unknown)`. Not "elsewhere" — **"nowhere"**. A caller that used the
side-effect-free probe §9.4.5 exists to provide was told its reference named
nothing, and the only way to learn otherwise was to send the request anyway.

That is a leak in this row's own terms. The criterion says a caller invokes
holding only a reference *without knowing the target's location*; a caller told
its reference is dead has learned something about location, and learned a
falsehood. `crates/orbweaver-giop/tests/locate_forward_and_reply_contexts.rs`
refutes it: 12 tests, three GIOP versions × two byte orders, seven negative
controls each run red — including the serve loop reverted to `knows()`, which
reproduces the `Ok(Unknown)` above.

**What is still open, named rather than left looking closed.**

1. ~~**`knows` gates the forward on the request path too.**~~ **Closed later the
   same day, by a later batch, and the characterisation test did its job.**
   `serve_one` asked `knows` before `redirect`, so the servant that answered
   `OBJECT_FORWARD` to a probe answered `OBJECT_NOT_EXIST` to an ordinary
   request; one root cause, two messages. The order is now `redirect`, `knows`,
   `dispatch`, and the argument for it has one home rather than two copies —
   `orbweaver_giop::server::serve_one_ordering()`, which returns the order as
   data so both `serve_one` implementations are asserted against it rather than
   against a comment. `a_moved_object_is_still_refused_on_the_request_path` went
   red on the reorder exactly as it was built to and is now
   `a_moved_object_is_forwarded_on_the_request_path_too`, asserting the
   opposite against a **live** second server, so what it measures is that the
   caller is served rather than that a message came back. The reorder is a no-op
   for every servant here: none of the five overrides `redirect`, and every
   skeleton `orbweaver-gen` emits opens its `redirect` with
   `self.refs.oid_of(&req.object_key)?`. Measured: in
   `locate_forward_and_reply_contexts.rs` exactly that one test changed answer
   and the other eleven were identical, under the reverted order.
2. **The multiplexer answers for its members.** `orbweaver_gen::rt::Servants`
   uses the default `locate`, which consults its own `knows` — "any member
   knows" — so a member that has moved an object would be overruled into
   `ObjectHere` and the caller would spend the request the probe existed to
   save. `orbweaver-gen` was another batch's footprint on 2026-08-26 and was
   left alone; the delegation is three lines and belongs with whoever owns it.
3. **No peer has been asked.** Every measurement here is this ORB's encoder
   against this ORB's decoder and a socket between them. omniORB and JacORB
   have not been made to emit an `OBJECT_FORWARD` at us, so the recorded-bytes
   discipline this project applies to wire changes has not been applied to this
   one. That is the honest limit of the measurement.

**Not a row-mover, landed the same day and recorded here so it is not looked
for under this row.** A `Reply` could not carry an `IOP::ServiceContextList`
(§9.4.3.1 requires one in every GIOP version; the encoder wrote a hard zero) and
an inbound one was discarded by `decode_reply` while `decode_request` kept the
identical list. §9.7.2's *"ignored, but preserved"*, the rule this codebase
already applies to a `TaggedComponent`. That is conformance and interoperability,
not transparency: no caller learns anything about a target's location from it.
Nothing attaches a context to an outgoing reply and there is no hook for doing
so — *who may* is `docs/PLAN-DEFERRED.md` §21.

*이 행의 상태 문장은 **옮겨지지 않는다**. 행이 이름 붙인 두 구멍은 그대로다.
닫힌 것은 이 행 아래 있었지만 행에 적히지 않았던 세 번째 결함이다 — 포워드를
줄 수 있는 두 순간 중 **이른 쪽**을 아무도 보지 않았다. `Request`의
`LOCATION_FORWARD`는 서빙되고 따라간다. `LocateRequest`의 `OBJECT_FORWARD`는
호출자가 요청을 쓰기 **한 메시지 전에** 주는 같은 대답인데, 이 ORB는 그 상태를
**이름 부를 수는 있고 서빙할 수는 없었다**: `encode_locate_reply`는 요청 id와
상태 워드를 쓰고 멈췄고, 서브 루프는 불리언인 `Dispatch::knows`로 답을 정했다.
변경 전 이 워크스페이스에서 측정: 객체가 이동한 서번트에 대해
`Connection::locate()`가 `Ok(Unknown)`을 답했다 — "다른 곳"이 아니라
**"아무 데도 없음"**. 부작용 없는 조사를 쓴 호출자가 자기 참조가 아무것도 가리키지
않는다고 들었고, 그것은 위치에 대해 **거짓을 배운 것**이므로 이 행의 용어로
구멍이다. `locate_forward_and_reply_contexts.rs`가 반증한다 — 12개 테스트,
GIOP 3개 버전 × 바이트 순서 2가지, 각각 붉게 만들어 본 부정 대조군 7개(서브
루프를 `knows()`로 되돌린 것 포함, 위의 `Ok(Unknown)`이 재현된다).*

***열려 있는 것, 닫힌 것처럼 보이지 않게 이름 붙여 둔다.*** *(1) ~~`knows`가
요청 경로에서도 포워드를 막는다~~ — **같은 날 뒤이은 배치가 닫았고, 특성화
테스트가 제 역할을 했다.** `serve_one`이 `redirect`보다 `knows`를 먼저 물었기에
조사에 `OBJECT_FORWARD`를 답하는 바로 그 서번트가 일반 요청에는
`OBJECT_NOT_EXIST`를 답했다 — 원인 하나, 메시지 둘. 이제 순서는 `redirect`,
`knows`, `dispatch`이며 그 근거는 복사본 둘이 아니라 집 하나를 갖는다:
`orbweaver_giop::server::serve_one_ordering()`이 순서를 **데이터로** 돌려주므로
두 `serve_one` 구현이 주석이 아니라 그 함수에 대해 검증된다.
`a_moved_object_is_still_refused_on_the_request_path`는 설계대로 재정렬에서
붉어졌고 지금은 `a_moved_object_is_forwarded_on_the_request_path_too`로서 **살아
있는** 두 번째 서버를 목적지로 두고 반대를 주장한다 — 메시지가 돌아왔다가 아니라
호출자가 응답받았다를 측정하기 위해서다. 재정렬은 여기 모든 서번트에 무영향이다:
다섯 중 `redirect`를 재정의한 것이 없고, `orbweaver-gen`이 내는 모든 스켈레톤은
`redirect`를 `self.refs.oid_of(&req.object_key)?`로 시작한다. 측정: 순서를
되돌린 상태에서 `locate_forward_and_reply_contexts.rs`의 정확히 그 한 테스트만
답이 바뀌었고 나머지 열한 개는 동일했다. (2) 다중화기가 멤버 대신 답한다:
`orbweaver_gen::rt::Servants`는 기본 `locate`를 쓰므로 이동한 멤버가
`ObjectHere`로 덮어써진다 — `orbweaver-gen`은 다른 배치의 footprint여서 손대지
않았다. (3) **피어에게 물어보지 않았다**: 모든 측정이 우리 인코더 대 우리 디코더다.
omniORB·JacORB가 우리에게 `OBJECT_FORWARD`를 보내게 한 적이 없으므로, 이
프로젝트가 와이어 변경에 적용하는 기록-바이트 규율이 이 변경에는 적용되지 않았다.
이것이 측정의 정직한 한계다.*

***이 행을 움직이지 않지만 같은 날 착지한 것***, *이 행 아래에서 찾지 않도록
여기 적어 둔다: `Reply`가 `IOP::ServiceContextList`를 실을 수 없었고(§9.4.3.1은
모든 GIOP 버전에서 요구한다; 인코더는 하드코딩된 0을 썼다) 들어온 것은
`decode_reply`가 버렸다 — `decode_request`는 같은 목록을 보관하는데도. §9.7.2의
*"무시하되 보존한다"*, 이 코드베이스가 `TaggedComponent`에 이미 적용하는 규칙이다.
이것은 적합성과 상호운용성이지 투명성이 아니다: 이것으로부터 호출자가 대상의
위치에 대해 알게 되는 것은 없다. 나가는 응답에 컨텍스트를 붙이는 것은 아무것도
없고 그럴 훅도 없다 — **누가 붙일 수 있는가**는 `PLAN-DEFERRED` §21이다.*

#### Lifecycle — a redirect for a *name*, built, and the decision it waits on (2026-08-26)

**The lifecycle row does not move, and declining to move it is the result.**
Four records name "a redirect emitted for a **name** rather than for an object"
as this row's blocker — the row itself, the instrument table below, the leak
test's counted `SKIPPED`, and the event-channel item 3 that follows. It is now
built and measured, and it still does not close the row. Why not is the finding.

**It is built.** `crates/orbweaver-giop/tests/forward_for_a_name.rs`: a servant
whose object keys are names and which hosts no objects. `knows` is `false` —
truthfully — `redirect` is a name-table lookup, `locate` says the same thing one
message earlier. A caller knowing only a name is served by whatever currently
answers to it, and when the binding moves the caller's reference does not
change. Seven tests, both byte orders, three negative controls.

**It could not have been built the day before**, and not for want of a hook: the
`knows`-before-`redirect` order refused a truthful `knows` of `false` before
`redirect` was reached, so a forwarder could only forward *everything* or refuse
*everything*. Saying *this name I redirect, that name does not exist* is the
entire content of a name-keyed redirect. That is the same root cause as the
Location subsection's item 1 above, which is why one reorder closed both.

**Why it does not close the row.** A forward is a **reply**, and a reply needs a
listener. A server that has been removed is not listening, so a
`LOCATION_FORWARD` emitted *by the removed server* is a contradiction in terms —
a server still able to answer has not been removed. The redirect must therefore
come from a **third endpoint that outlives both**, and the client's reference
must have pointed at that endpoint **from the start**. A client holding a dead
backend's IOR cannot be redirected by anybody at any layer; that is not a gap in
this ORB, it is what an IOR is. `corbaname:` is not the answer either: it
resolves on the client, once, at bind time, and what is kept afterwards is
exactly as dead. That claim is not rhetoric — the test's third negative control
hands the forwarder a *snapshot* of the name table, which is what resolving once
amounts to, and exactly the two late-resolution tests go red.

**X, the decision this waits on.** *That the reference `Orb::server` hands out is
**indirect**: its IIOP profile carries a name-resolving endpoint's address and a
name rather than the servant's own address and an object key.* X is **not** a
successor registry, and the batch deliberately did not build one — CosNaming's
`rebind` already owns the mapping and the successor already calls it. X is a
decision because it (a) changes every IOR this project emits, which D019 step 4
made one path's promise; (b) inverts a layer, making the ORB depend on a servant
built on it, against D019's title; (c) **displaces** the leak to the forwarding
endpoint rather than closing it — the shape item 1 of the next subsection
already names for the bootstrap address, and whoever proposes X must say which
of displacement and closure is being claimed; and (d) does not repair a stale
binding, because item 4 below has unbinding deliberately separate from the
channel going away, so the forwarder faithfully redirects to an IOR that is also
dead and the caller fails one hop later. Repairing *that* is liveness detection,
a fifth and much larger decision. X also re-opens **D013**, which decided
reference identity assuming an IOR names an object.

The argument in full, beside the tests that check its claims, is that file's
module documentation. **No new wire shape was added** — a forward produced by a
name resolving is byte-for-byte the message produced by an object moving, which
`the_forward_a_name_produces_is_the_same_message_an_object_move_produces`
checks over three versions and both orders rather than asserting. That is why no
new peer leg is owed and none was written.

*이 행은 움직이지 않으며, **움직이지 않기로 한 것이 결과다.** 네 개의 기록이
"객체가 아니라 **이름**에 대해 발행되는 리다이렉트"를 이 행의 차단 요인으로
지목한다. 그것이 이제 만들어졌고 측정되었으며, 그럼에도 행을 닫지 못한다. 그
이유가 발견이다.*

***만들어졌다.*** `forward_for_a_name.rs` — 객체 키가 이름이고 객체를 하나도
호스팅하지 않는 서번트. `knows`는 (사실대로) `false`, `redirect`는 이름표 조회,
`locate`는 한 메시지 앞서 같은 답을 한다. 이름만 아는 호출자가 지금 그 이름에
답하는 쪽에게 응답받고, 바인딩이 옮겨져도 호출자의 참조는 바뀌지 않는다. 테스트
7개, 바이트 순서 양쪽, 부정 대조군 3개.

***하루 전이었다면 만들 수 없었다*** — 훅이 없어서가 아니라, `knows`가
`redirect`보다 먼저였기에 사실대로인 `false`가 `redirect`에 닿기 전에 거절되었기
때문이다. 그래서 포워더는 **전부** 넘기거나 **전부** 거절할 수만 있었다. *이
이름은 넘기고 저 이름은 없다*고 말하는 것이 이름 기반 리다이렉트의 전부다. 위
Location 절 항목 1과 같은 근본원인이며, 그래서 재정렬 하나가 둘 다 닫았다.

***왜 행을 닫지 못하는가.*** 포워드는 **응답**이고 응답에는 듣는 쪽이 필요하다.
제거된 서버는 듣고 있지 않으므로 *제거된 서버가 발행하는* `LOCATION_FORWARD`는
용어상 모순이다 — 아직 답할 수 있는 서버는 제거된 것이 아니다. 따라서
리다이렉트는 **둘보다 오래 사는 제3의 종단점**에서 와야 하고, 클라이언트의 참조는
**처음부터** 그 종단점을 가리키고 있었어야 한다. 죽은 백엔드의 IOR을 든
클라이언트는 어느 계층에서도 리다이렉트될 수 없다. 이는 이 ORB의 결함이 아니라
IOR이 무엇인지의 문제다. `corbaname:`도 답이 아니다 — 클라이언트에서 바인드
시점에 한 번 해석되고, 그 뒤 들고 있는 것은 똑같이 죽어 있다. 이는 수사가 아니다:
세 번째 부정 대조군이 포워더에게 이름표의 **스냅숏**을 넘기는데(한 번만 해석한다는
것이 바로 그것이다) 늦은 해석에 의존하는 정확히 그 두 테스트가 붉어진다.

***X — 이것이 기다리는 결정.*** *`Orb::server`가 내주는 참조가 **간접적**이라는
것 — IIOP 프로필이 서번트 자신의 주소와 객체 키가 아니라 이름 해석 종단점의
주소와 이름을 싣는다.* X는 **후계자 레지스트리가 아니며** 이 배치는 그것을 짓지
않기로 했다. 매핑의 주인은 이미 CosNaming의 `rebind`이고 후계자가 이미 그것을
부른다. X가 결정인 이유: (a) 이 프로젝트가 내는 모든 IOR을 바꾼다 — D019 4단계가
한 경로의 약속으로 만든 것이다; (b) 계층을 뒤집어 ORB가 자기 위에 세워진 서번트에
의존하게 한다 — D019의 제목에 반한다; (c) 구멍을 닫는 것이 아니라 포워딩 종단점으로
**옮긴다** — 다음 절 항목 1이 부트스트랩 주소에 대해 이미 지목한 모양이며, X를
제안하는 쪽은 이전과 폐쇄 중 무엇을 주장하는지 말해야 한다; (d) 낡은 바인딩을
고치지 못한다 — 아래 항목 4가 언바인드를 채널 소멸과 의도적으로 분리하므로,
포워더는 역시 죽은 IOR로 충실히 리다이렉트하고 호출자는 한 홉 뒤에 실패한다. 그것을
고치는 것은 생존 감지이며 다섯 번째이자 훨씬 큰 결정이다. X는 IOR이 객체를
가리킨다는 전제 위에서 참조 동일성을 정한 **D013**도 다시 연다.

*전체 논증은 그 파일의 모듈 문서에 있고, 그 주장들을 검사하는 테스트가 옆에 있다.
**새로운 와이어 모양은 추가되지 않았다** — 이름 해석이 낳은 포워드는 객체 이동이
낳은 메시지와 바이트 단위로 같으며,
`the_forward_a_name_produces_is_the_same_message_an_object_move_produces`가 버전
3개와 순서 양쪽에서 주장 대신 검사한다. 그래서 새 피어 검사는 빚지지 않았고 쓰지
않았다.*

#### Location, for event channels — what closed and what did not (2026-08-26)

D021 E3 landed: a channel is published under a name in a CosNaming context and
a client reaches it holding an `Orb`, the string `corbaloc:rir:NameService` and
the channel's name. Measured twice — `channel_found_by_name.rs` with our client
at both ends, and `spikes/event_by_name.sh` with omniORB's client, which is
what makes it a measurement rather than a self-test. The claim is refutable and
its control is the leak: the same client handed the pre-move IOR cannot survive
the move, and when that control was made to pass the whole assertion went red.

**What a client can still tell, named rather than left looking closed.** Every
one of these is a defect to close, on the same terms as the table above.

1. **The naming service's address is still handed over.** The channel's is not,
   but something had to put an address into the ORB's initial-references table
   for `corbaloc:rir:` to answer. The leak is **displaced, not closed** — from
   N channels to one bootstrap — and calling it closed would be the row this
   subsection exists to avoid.
2. **A moved channel is a redeployment, and the client has to notice.** §3.1's
   gap means "move" is really "stop one server and start another with the same
   keys", so an *already-attached* consumer is not carried across: it is
   dropped, and the client learns by failing. The test re-runs the whole
   bootstrap unconditionally, so it measures that a **new** bootstrap is
   unaffected and measures **nothing** about an existing connection surviving.
   That is the honest limit of the measurement and the next thing to close.
3. **Nothing re-publishes.** Publication is the deployer's explicit call. A
   channel that moves without one leaves its name pointing at a dead address,
   and the client gets a connect failure rather than a redirect —
   `LOCATION_FORWARD` is served for objects but nothing emits it for a name.
4. **A binding outlives its channel.** Unbinding is deliberately separate from
   the channel going away (§2.5.1, and what omniNames measurably does), so a
   name can resolve to a channel that is gone. The client tells the difference
   only by dialling and failing.
5. **The channel's *name* is still deployment knowledge**, including that the
   kind is `EventChannel`. That is a name and not a location, so it is not a
   leak in this row — recorded so the next reader does not re-derive it.

*무엇이 닫혔고 무엇이 닫히지 않았는가. 채널의 주소는 더 이상 건네지지 않지만,
**네이밍 서비스의 주소는 여전히 건네진다** — 구멍은 닫힌 것이 아니라 N개에서
하나로 **옮겨졌다**. §3.1 때문에 "이동"은 사실 재배포이므로 **이미 붙어 있던
소비자는 이어지지 않는다**; 테스트는 새 부트스트랩이 영향받지 않음을 재고 기존
연결의 생존은 **재지 않는다** — 이것이 측정의 정직한 한계다. 재발행하는 것은
없고, 바인딩은 채널보다 오래 살아남으며, 채널의 **이름**은 여전히 배포 지식이다
(이름은 위치가 아니므로 이 행의 구멍은 아니다).*

### 6.1.1 What a caller can still tell about a servant's language / 남은 구멍

Measured 2026-08-26 by `crates/orbweaver-gen/tests/python_servant.rs`, which is
where each of these is named. The first three are differences in **what a
servant author can get wrong**, not in what a correct servant answers; the last
two are differences in **what a servant can do at all**, and are the ones worth
closing next.

| # | Difference | Caller sees |
|---|---|---|
| 1 | An operation the author never implemented: Rust will not compile, Python answers `NO_IMPLEMENT` | only when the author erred, and then a legal CORBA refusal |
| 2 | A raise the operation does not declare: Rust's generated fault enum has no variant for one, Python can raise anything | only when the author erred, and then `UNKNOWN` + OMG minor 1, which is §4.11's own mapping |
| 3 | A system exception with no completion status: Rust's `#[must_use] Raising` warns, Python's seam refuses at runtime | only when the author erred, and then a refusal rather than a guessed "safe to retry" |
| 4 | **An object reference argument reaches a Python servant as an opaque handle it cannot invoke** — §4.5 emits no IOR, so a reference crosses as a token into the bridge's table | on any contract that passes a reference the servant must *use*: the Python servant cannot, the Rust one can |
| 5 | **A Python servant cannot mint a new object reference**, having no POA on its side | on any operation whose contract returns a reference the servant creates |

4 and 5 are one fact from two sides: **the seam carries values, and an object
reference is the one value whose meaning is a capability rather than data.**
They are the language transparency that is left, and they are a smaller and
more specific claim than the row above used to make.

*1–3은 서번트 작성자가 **틀릴 수 있는 방식**의 차이이지 올바른 서번트가 내놓는
답의 차이가 아니다. 4와 5는 **서번트가 아예 할 수 없는 일**의 차이이며, 한 사실의
양면이다 — 심(seam)은 값을 나르는데, 객체 참조는 데이터가 아니라 능력을 뜻하는
유일한 값이다. 이것이 남은 언어 투명성이며, 위 행이 예전에 하던 주장보다 좁고
구체적인 주장이다.*

#### The list did not grow when a big-endian peer was added (2026-08-26)

The five above were found against one foreign peer in one byte order, which is
the shape of measurement that hides an order-dependent difference by
construction. A second peer in the other order was added the same day —
`spikes/jacorb_python_servant.sh` and `jacorb_calls_a_python_servant` in
`crates/orbweaver-gen/tests/python_servant_wire.rs` — and **found no sixth**.
That is a result and not a formality: the servants' replies were compared as
*bytes*, so a padding byte or an alignment origin that differed between the two
implementations would have been a difference this file had never been able to
see, and the eleven replies were identical at IIOP 1.2 and at 1.1.

**What is still not measured, named rather than left looking closed.**

1. **One peer per order, not two peers per order.** Little-endian is omniORB's
   and big-endian is JacORB's, so a difference that is really *"which ORB"*
   rather than *"which order"* would be invisible. On a big-endian host omniORB
   would swap sides and the pairing would be testable; nothing here has one.
2. **GIOP 1.0 is unmeasured against JacORB.** 1.2 and 1.1 are measured by
   republishing the IIOP profile; 1.0 is not, so the version whose reply header
   differs most is measured against our own client only.
3. **The comparison is of one contract.** `corpus/golden/24` was chosen because
   it holds every hazard a dispatcher has, but 4 and 5 in the table above are
   exactly the two things it cannot exercise — it passes and returns no object
   reference, which is why they remain named there rather than measured here.

*목록은 빅엔디언 피어를 더해도 늘지 않았다. 다섯 가지는 **한 피어, 한 바이트
순서**에서 나온 것이고, 그 형태의 측정은 순서에 의존하는 차이를 구조적으로 숨긴다.
같은 날 반대 순서의 두 번째 피어를 붙였고 **여섯 번째는 나오지 않았다.** 응답을
**바이트로** 비교했으므로 패딩 한 바이트나 정렬 기준의 차이였다면 이 파일이 여태
볼 수 없던 차이로 드러났을 것이다 — IIOP 1.2와 1.1에서 열한 개 응답이 동일했다.
**아직 재지 않은 것:** (1) 순서마다 피어가 하나뿐이라 "어느 순서"가 아니라 "어느
ORB"인 차이는 보이지 않는다, (2) JacORB에 대한 GIOP 1.0은 재지 않았다, (3) 계약이
하나이며 위 표의 4·5는 바로 그 계약이 시험할 수 없는 두 가지다.*

### 6.2 What this criterion does to the order

O1 (lifecycle) and D030 L1 (the language seam) are **no longer two items of
comparable weight**: each closes a whole transparency, and the other proposals
close none. The re-ordering is stated in §5's preamble rather than left implied.

The clauses of the earlier draft do not disappear — a foreign client
bootstrapping *is* how location transparency is measured, and an operator's
flag reaching the wire *is* how a deployment stops being a special case. They
become **instruments** for this criterion rather than the criterion itself.

## 7. What this document does not claim / 주장하지 않는 것

It does not claim the ten absent operations should shrink to zero — §4 says two
of them are absent *correctly* and three have never had a caller. It does not
claim §3.1's asymmetry is a defect in step 4: step 4 was right, and creating a
gap by closing a door is what one-way doors do. It does not claim §6's
definition is the only possible one; it claims a definition is required before
"complete" can be worked toward, and this one is measurable, which is the
property that matters. And it does not claim any of the four is urgent against
the four TypeCode agreement failures the harness reported while this was being
written — those are a regression and outrank every proposal here.
