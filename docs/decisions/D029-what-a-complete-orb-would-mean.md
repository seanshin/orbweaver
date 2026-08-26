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
| **Location** | where the target runs | **measured, with a known leak**: `LOCATION_FORWARD` and `_PERM` are served and followed, and R7 rewrites an IOR for a dialable address — but `Connection::move_to` restored a hand-written field list and dropped two configured limits across every forward until today, so the *caller's* limits changed when the object moved. Fixed; the class is the leak to watch. **A second instance, found 2026-08-26 and not fixed**: `moe::Router::select` returns `ExpertSeq` — N object references, each an `Ior` stored verbatim from `register_expert` and marshalled inline with host, port and object key. A caller learns where every candidate expert runs, which is exactly what this row says it must not be able to tell. `corpus/golden/22`'s own comment beside the operation already says so — *"widening reach by N addresses at once is precisely the case §4.7's bearer-address rule exists for"* — and §4.7's rule is the authority half of the same fact. Recorded, not changed: `select` is served and has consumers. |
| **Backend** | what implements it | mostly held: a servant is behind a POA and a reference; but `spike_experts`' server root key collides with its derived registry key, which is a backend detail reaching a name. |
| **Language** | what it is written in | **leaks by construction**: Python is clients only. A Python servant cannot be dispatched into, so *the target's language is visible in whether it can be a target at all*. D030 L1. |
| **Activation / load** | whether it is loaded right now | **leaks, and now measured (2026-08-26)**: the leak is `moe::Router::select`, and it is *residency-blind by omission rather than by absence of data*. `mirror_residency` keeps `Offer::residency` live in the very store `select` reads, and `orbweaver-trading`'s query grammar has a `residency` field, but `Constraints::to_query_text` never names it — so an OFFLOADED expert comes back in the sequence and dialling it answers `OBJECT_NOT_EXIST` where a resident one answers. `expert_service.rs:882-891` records this as intended: *"the caller's cue to `prefetch`"*. That makes the leak a **design choice written down**, not an oversight, which is the strongest form for it to be in before it is decided. `Router::dispatch` is *not* the operation that would close it — it is refused (D006 option E), and its own reason is now known to be false as written (see D006's 2026-08-26 amendment). The closer is a POA-level activation path, because the criterion says *any* target, and a fix inside one application contract closes it for one contract. |
| **Lifecycle stability** | that the above survives add / remove / move / load / evict at runtime | **partly unmeasurable today**: the ORB owns the transport and **cannot stop what it handed out** (§3.1), so "remove at runtime" has no implementation to be transparent about. |

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
