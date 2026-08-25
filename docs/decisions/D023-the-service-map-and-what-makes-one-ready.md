# D023 — The service map, and what makes a service ready to start

**STATUS: PROPOSED** — drafted 2026-08-25 on a request to plan each service in
parallel with a development schedule, extended the same day to name
transaction/concurrency, query/collections and operations/governance as later
work. Every row of §1 was measured that day by cross-checking all twenty-one
CORBAservices against `PLAN-SERVICES.md`, `PLAN-DEFERRED.md` and `PLAN.md`.
Not self-approvable: §2 proposes a rule about when a deferral may be opened.

**상태: 제안** — 2026-08-25, 서비스들을 병행 기획하고 세부 개발 계획을 잡으라는
요청에서 작성.

---

## 1. The map, measured / 지도

Twenty-one CORBAservices, cross-checked against the three plan documents:

| State | Services | Count |
|---|---|---|
| **Served** | Naming (14/14), Event (14/18), **LifeCycle** (`ModelFactory` 4/4), **Property** (folded into `Manifest`, with the reason written), Trading *engine* | 5 |
| **Deferred with a reason and a trigger** | Notification, **PSS**, Transaction (OTS), Time, Concurrency, Collections, Security-beyond-CSIv2, federated Naming/Trading, Trading *facade* | 9 |
| **Mentioned in no document at all** | **Relationship**, Containment, Reference, CompoundLifeCycle, **ObjectIdentity**, Externalization, Query, Licensing | 8 |

The third row is the finding. `PLAN-DEFERRED`'s premise is that **an exclusion
carries a reason**, and eight services have neither an implementation nor an
exclusion. That is the state D018 §3.3 named for Portable Interceptors and the
POA policies, and it is larger here.

**Two of the eight are not really absent — they are unnamed.**

- **Relationship.** `tenant_service::Manifest` holds three of them as strings:
  `base_model` ("names the shared base, which is *not* owned by this tenant"),
  `experts: Vec<String>` (by capability id), and `policy_domain` ("names the
  `PolicyDomain` governing this model"). Each is a reference to another object
  with an integrity rule, and **nowhere does any document say these are
  relationships or what their rules are.** `COMPONENTS.md`'s gap row —
  `bind_expert`/`set_policy` take references *no operation of the contract
  returns* — is one end of exactly this.
- **ObjectIdentity.** `orbweaver-object` already has `is_equivalent` and
  `reference_hash`, and D013 measured `_is_equivalent` against omniORB. The
  capability exists and the standard's name for it does not appear — the same
  shape D019 found for `string_to_object`.

*세 번째 줄이 발견이다. 그리고 그중 둘은 없는 게 아니라 **이름이 없다**.*

## 2. What makes a service ready / 무엇이 서비스를 준비시키는가

Today's diagnosis, recorded because it explains the whole map: **every
deferral's trigger has a subject outside this project** — *"a peer requires
it"*, *"a pilot peer"*, *"a foreign client"*, *"two ORB processes"*, *"a named
`PullSupplier` in this workspace"*. There is no pilot, no foreign client, no
second host. Each deferral is individually well argued; **their conjunction is
a service programme that cannot start.**

Twelve deferrals were re-measured on 2026-08-25 and **zero fired**. Read one
way that is a clean result. Read the other way it is the measurement of a
stopped process, and nobody was reading it the second way.

**Proposed rule — the missing door.** *The project owner naming a consumer
fires a trigger, recorded with the date and the naming.* This does not weaken
the discipline; it names **who** may open it, which D010 already does for
approval. Without it, a trigger phrased *"until something asks"* is unreachable
by construction, because the only thing that could ask is the owner.

Fired under this rule on 2026-08-25, both recorded in their batches:
- **CosEvent's supplier-side pull** (`PLAN-DEFERRED` §10) — the owner asked for
  the four models to be creatable.
- **The `CosTrading::Lookup` facade** (`PLAN-SERVICES` §3) — the owner asked
  for the trading service to be opened.

*빠진 문: **소유자가 소비자를 지명하면 방아쇠가 당겨진다.** 규율을 약화시키지
않고 **누가 문을 열 수 있는지**를 이름한다.*

## 3. Phase 1 — now, and it is one batch / 1단계

Only one of the four services named in the request is actual work today.

### R1 — the relationships get their names (`orbweaver-object`)

**Not a deferral document — a naming batch**, because the relationships are
already in the tree carrying integrity rules nobody wrote down. What the batch
owes, in order:

1. **Name the three.** `Manifest::base_model`, `::experts`, `::policy_domain`
   become documented relationships with their **role, cardinality and integrity
   rule** each: who may point at what, what happens when the target does not
   exist, and who may change it. `bind_expert` and `set_policy` are the
   mutators and their scopes already differ — that difference is a relationship
   rule already being enforced with no name.
2. **State what is not checked.** COMPONENTS records that `bind_expert` /
   `set_policy` take references no operation returns. Whether a dangling
   `expert` or a `policy_domain` naming nothing is refused, tolerated or
   unmeasured is a fact, and today it is in nobody's document.
3. **Say what `clone_model` does with them.** This is CosLifeCycle's own
   question: the standard defines `copy`/`move` over relationship graphs
   (`CosCompoundLifeCycle`), and our `clone_model` is exactly that operation.
   **Whether it follows `base_model`, copies `experts`, or shares
   `policy_domain` is behaviour that exists and is undocumented.** Measure it
   and write it down; do not change it.

**Oracle.** The behaviour is already reachable through `spike-tenants` and
`tenant_service`'s tests. Every rule written down gets a test that would fail
if the rule were different — the standard Stage-A discipline: a document that
reports rules nobody checks against behaviour is the green-while-measuring-
nothing class.

### R2 — LifeCycle's missing sentence (documents)

`PLAN-SERVICES` §5 records why `ModelFactory` drops GenericFactory's
genericity. It does not record that the standard's `copy`/`move` are defined
over relationships and that ours are not. One paragraph, and it depends on R1's
measurement.

### R3 — `ObjectIdentity`, named not built (documents + one crate)

`is_equivalent` / `reference_hash` gain the standard's vocabulary in their doc
comments and a note that `CosObjectIdentity` is the interface this would be, if
a consumer named one. Follows D019 step 2's shape exactly.

**PSS and Property need nothing.** PSS is `PLAN-DEFERRED` §4 with four reasons
and an honest trigger (*"whether that has ever happened to anyone is
unverified; no survey was done"*). Property is the best-handled service in the
map: folded into `Manifest`, with the location and the reason both written.

## 4. Phase 2 — already running / 2단계

Fired by §2's rule and in flight as this is written: **Event E1** (the fourth
side of the 2×2, taking CosEvent 14/18 → 17/18) and **Trading T1** (the
constraint language toward TCL, the prerequisite for `Lookup::query`). D021 and
D022 hold their stages.

## 5. Phase 3 — named as later / 3단계

The owner named transaction/concurrency, query/collections and
operations/governance as later work. What "later" must mean for each, so the
scheduling is a decision and not a queue:

- **Transaction (OTS) + Concurrency.** `PLAN-DEFERRED` §2 and §5. **These two
  are one decision, not two**: §5's own reason says the interesting half is
  `TransactionalLockSet`, which needs the coordinator §2 declines. §2's first
  reason — *"we deliberately have no durable store"* — is D003-B's deferral,
  and D015 §3.2 makes the same store the pilot's trigger. **So OTS, Concurrency
  and durability are one gate with three doors**, and opening any of them opens
  the others. That is the thing to decide, and it is bigger than a service.
- **Query + Collections.** `PLAN-DEFERRED` §6 covers Collections (trigger: a
  foreign client expecting the interfaces by name, or an unboundable result
  set). **Query is in the third row of §1 — no document at all** — and it is
  the closest of the eight to something we have: `orbweaver-trading::Query` is
  a constraint query language, and Trading T1 is widening it now. A reason and
  a trigger for `CosQuery` should say plainly why the trader's query is not it.
- **Operations / governance.** This is the one where **we are ahead of the
  standard rather than behind it.** CORBA has no governance service; this
  project has the MCP boundary's interceptor chain (exposure → scopes → quota
  seat → approval, with audit and telemetry), D004's record shape, and the
  deployment configuration landed 2026-08-25. What remains is named already:
  **D019 step 3** (the ORB's own seven numbers, still compile-time constants,
  so D015's *"without editing Rust"* is false one layer below where the
  operator-surface batch made it true) and **D015 §3.3** (identity, which needs
  a provider). Nothing new to plan; two existing items to finish.

## 6. What must not happen / 해서는 안 되는 것

- **No service in §1's third row becomes an implementation because it appeared
  here.** Each gets a reason and a trigger first, in `PLAN-DEFERRED`'s shape.
  D018 §4.1's sentence stands: *writing a thing down makes it feel owed*, and a
  plan document is exactly how that discipline breaks.
- **R1 changes no behaviour.** It writes down rules that already run. If a rule
  turns out to be wrong, that is a finding and its own batch — not a quiet fix
  inside a documentation change.
- **Phase 3's transaction gate is not opened service by service.** Opening OTS
  because "transactions were named" while the store stays deferred would build
  a coordinator with nothing to coordinate over.
- **§2's rule does not apply retroactively.** It fires triggers the owner names
  from now on; it does not reopen the nine deferrals nobody has asked about.

## 7. What this document does not claim / 주장하지 않는 것

It does not claim the served fraction should rise, and it does not rank the
twenty-one by importance — `PLAN-SERVICES` §1 already fixes the rule that
decides what gets served. It does not claim the eight unmentioned services
should be built; it claims they should be **decided about**, which is a
different and much cheaper thing. And it does not claim §2's rule is obviously
right: a trigger that fires on a request rather than on a measurement is a
different kind of trigger, and that is precisely why it is proposed here rather
than applied silently.
