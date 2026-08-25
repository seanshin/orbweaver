# D018 — The surface an ORB is expected to have, and which absence is met first

**STATUS: PROPOSED** — drafted 2026-08-25 after a fair challenge: four plan
documents were written that day (D014 waves, D015 service completion, D016
measured defects, D017 practices) and **none of them plans against the
specification's own surface.** They plan the quality of what exists and the
readiness of what is deployed. This one asks the question they skip: *of what
CORBA defines, what do we serve, what do we refuse with a reason, and which
absence does a consumer meet first?*

**상태: 제안** — 2026-08-25, 정당한 지적을 받고 작성. 그날 쓴 기획서 넷 중
**명세 표면 자체를 계획한 것은 없다.** 있는 것의 품질과 배포 준비를 다뤘을 뿐이다.
이 문서가 그들이 건너뛴 질문을 한다.

---

## 1. What the other four cover, and what they do not / 나머지 넷이 덮는 것

| Document | Axis | Reaches the ORB/services? |
|---|---|---|
| D014 | wave sequencing | no — process |
| D015 | can a person who did not build this use it | partly — the *operator* surface, durability, identity, deployment |
| D016 | defects measured 2026-08-25 | **2 of 14** touch the wire (`Principal` as a fifth refusal family; the union discriminator's three homes). The rest are naming, tooling and documents |
| D017 | practices that find those without a human | no — process |

That is a real omission and it has a cause worth naming: every one of those
documents was written **from gap columns and from sweeps**, and both of those
instruments measure *the correctness of what exists*. Neither can see a thing
that was never built, because nothing declares it missing. The specification
is the only instrument that can, and no document here reads from it.

*그 넷은 전부 갭 열과 스윕에서 쓰였고, 두 도구 모두 **있는 것의 정확성**을 잰다.
지어진 적 없는 것은 볼 수 없다 — 그것이 없다고 선언하는 것이 아무것도 없기
때문이다. 그것을 볼 수 있는 도구는 명세뿐이고, 여기 어느 문서도 명세를 읽지
않았다.*

## 2. The measured surface / 측정된 표면

From `SERVICES-COVERAGE.md` §8, generated from the wire sweep (2026-08-25):

| Service | Declared | Served | Deferred `NO_IMPLEMENT` | Refused `NO_PERMISSION` | Not dispatched |
|---|---|---|---|---|---|
| CosNaming | 14 | **14** | 0 | 0 | 0 |
| MoE enterprise | 16 | **16** | 0 | 0 | 0 |
| CosEvent | 18 | 14 | 4 | 0 | 0 |
| MoE control plane | 14 | 10 | 1 | 0 | 3 |
| **IFR** | **44** | **9** | **10** | **25** | 0 |
| **total** | **106** | **63** | 15 | 25 | 3 |

**59% served, and every one of the other 43 has an answer with a reason** —
that is the project's real strength and it should not be mistaken for
completeness. The three `BAD_OPERATION`s are `moe::Expert`, explained in
`PLAN-SERVICES` §8.1.1.

And what is absent from the ORB itself, measured by grep rather than claimed:

- **Portable Interceptors — 0 files.** The standard's own mechanism for what
  `orbweaver-mcp`'s interceptor chain does locally, and the one a foreign
  client would expect to plug into.
- **BiDirectional GIOP — 0 files.** The specification's answer to a callback
  through a firewall; R7 already measures endpoint rewriting for the shapes
  one machine can show.
- **POA policies — one** (`UnknownIdPolicy`, plus a residency `MissPolicy`
  that is ours and not the standard's). The specification defines seven.
- **§4.4's four constructs** — `valuetype`, abstract interface, `fixed`,
  `native` — parsed, described, refused at the wire, decision gate open.

## 3. The question this document actually asks / 진짜 질문

Not *"what fraction of CORBA is implemented"* — that number is a vanity metric
for a project whose posture is measurement with reasons, and chasing it would
build things no consumer has asked for, which is the class-C defect D010 §9.3
withholds authorisation for.

The question is: **which of these absences does a consumer meet first, and is
its reason still true?** Three groups, and only the first is work.

### 3.1 Reasons that may have expired — check before building anything

Each of these was refused for a reason written at a date. A reason is a claim
about the world and can go stale exactly like a gap column.

- **The IFR's ten `NO_IMPLEMENT`s are one shape**: `Container::contents`,
  `lookup`, `lookup_name`, `describe_contents`, `Contained::describe`,
  `_get_defined_in`, `_get_containing_repository`, `Repository::
  get_canonical_typecode`, `get_primitive`, `IDLType::_get_type`. That is
  **the containment walk** — the half of the IFR that lets a client *browse*
  rather than look up by id. The reason recorded on 2026-08-14 was that the
  registry is a facade over our own contracts and a browse has no consumer.
  **`orbweaver-mcp` now browses** — `search_interfaces` and the console
  catalogue walk the same registry through Rust. The question is whether a
  *foreign* IFR client browsing over the wire is a shape anyone wants; if the
  answer is still no, the reason should be re-dated rather than left at
  2026-08-14.
- **`def_kind` answers `dk_none` for definitions that exist** (D016 §5 B1).
  This one is not a deferral; it is wrong, and it is the browse half's
  foundation — a client that walks a container reads `def_kind` on everything
  it finds.
- **CosEvent's four**: supplier-side pull and `destroy`, both class-C with
  triggers re-measured 2026-08-25 and **not fired**. Correctly deferred.
- **MoE control plane's `dispatch`**: excluded by D006 (APPROVED). Correct.

### 3.2 Absences with a live decision — do not build

`CosTrading::Lookup` (PLAN-SERVICES §3), OTS (PLAN-DEFERRED §2), Notification
(§1), Time (§3), PSS (§4), Concurrency (§5), Collections (§6), federated
naming (§7), Security beyond CSIv2 (§8). All nine re-measured 2026-08-25:
**no trigger fired.** Building any of them early is the defect, not the work.

### 3.3 Absences with no decision at all — this is the gap in the planning

Portable Interceptors, BiDirectional GIOP, and the six unimplemented POA
policies are in **no** plan document, no deferral chapter and no decision.
They are not deferred; they are simply unmentioned, which is the one state
this project's own rules do not allow — `PLAN-DEFERRED`'s whole premise is
that an exclusion carries a reason.

**That, and not the served fraction, is what this document is for.** The
deliverable is not an implementation; it is a reason per item, in
`PLAN-DEFERRED`'s shape, with a trigger:

- **Portable Interceptors.** Ours are local and in-process by design (D004's
  record shape rests on there being no clock and no remote hop in the chain).
  The standard's are per-ORB and see every request. A reason should say
  whether the two are the same idea at different scopes, and the trigger is
  presumably *a foreign client that expects to register one*.
- **BiDirectional GIOP.** R7 measures NAT endpoint rewriting; bidirectional
  GIOP is the other answer to the same problem and the one a firewalled
  callback needs. The trigger is presumably *a consumer whose callbacks cannot
  be dialled*, which is the shape D015 §3.4 says cannot be measured here.
- **The six POA policies.** `UnknownIdPolicy` exists because a servant
  locator needed it. The others (`ThreadPolicy`, `LifespanPolicy`,
  `IdUniquenessPolicy`, `IdAssignmentPolicy`, `ImplicitActivationPolicy`,
  `ServantRetentionPolicy`, `RequestProcessingPolicy`) each encode a choice
  this ORB has made implicitly. **Writing down which value we behave as, for
  each, is the batch** — not implementing the alternatives. An implicit choice
  is a fact with no home, which is the class D016 §2 names eleven times.

## 4. Order / 순서

1. **`def_kind`** (D016 §5 B1) — it is wrong rather than absent, and it is the
   foundation of anything IFR-browse-shaped. Needs the peer measurement.
2. **The seven POA policies written down** — a document batch, no code, and it
   converts seven implicit choices into stated ones. Cheapest item here and
   the one most likely to find a surprise.
3. **A reason and a trigger for Portable Interceptors and BiDirectional GIOP**,
   in `PLAN-DEFERRED`'s shape. Also a document batch.
4. **Re-date the IFR containment walk's reason**, or fire it — but only after
   §4.1 below.
5. Everything else stays deferred, and 3.2's nine stay untouched.

### 4.1 What must not happen / 해서는 안 되는 것

No item in §3.3 becomes an implementation because it appeared in a plan. Each
gets a **reason and a trigger first**, and building begins when a trigger
fires — the discipline that has held for twelve deferrals and, re-measured on
2026-08-25, was correct in all twelve. A plan document is exactly how that
discipline breaks: writing a thing down makes it feel owed.

*계획서에 등장했다는 이유로 구현이 시작되지 않는다. 각 항목은 **이유와 방아쇠**를
먼저 얻고, 방아쇠가 당겨질 때 짓기 시작한다. 무언가를 적어두는 것이 그것을 빚처럼
느끼게 만드는데, 계획서는 정확히 그 방식으로 규율을 깬다.*

## 5. What this document does not claim / 주장하지 않는 것

It does not claim the served fraction should rise. It does not rank CORBA's
services by importance — `PLAN-SERVICES` §1 already fixes the rule that decides
what gets served, and this document does not restate it. And it does not
propose a specification-coverage gate: a number that says "59% of CORBA" would
be a figure with no measurement behind it, since *declared* here means declared
by the five contracts we chose to serve, not by the specification.
