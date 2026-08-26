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

## 6. What "complete" means, proposed / "완성"의 정의

An operation count cannot say it. This can:

> **The ORB is complete when a foreign client can bootstrap through it, call
> through it, and be told no by it; when an operator can change every number it
> owns from the command line without a rebuild; and when everything it does not
> do has a reason and a trigger.** Each clause is measurable against a peer or
> against a document, and none of them is a count.

Against that definition, today: clause one is **measured** (omniORB resolves
`rir` out of our table and calls through to a real value). Clause two is
**half** — the numbers have a home and five of eight change the wire, but no
binary takes the flags, so no deployment can. Clause three is **short by two**,
which is O3.

*연산 개수로는 말할 수 없다. 외부 클라이언트가 **부트스트랩하고, 호출하고, 거부당할
수 있을 때**; 운영자가 ORB가 소유한 모든 수치를 **재빌드 없이 명령줄에서** 바꿀 수
있을 때; 그리고 **하지 않는 모든 것에 사유와 방아쇠가 있을 때** 완성이다. 오늘
첫째는 측정됐고, 둘째는 절반이며, 셋째는 두 개 모자란다.*

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
