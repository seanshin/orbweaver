# D020 — The POA against CORBA 3.4, and the policies we have been choosing silently

**STATUS: PROPOSED** — drafted 2026-08-25 under D019's direction (ORB
composition is the primary goal). Every "today" below was read from
`crates/orbweaver-object/src/lib.rs` on that date. Not self-approvable: §6
proposes a public API that existing callers become users of.

**상태: 제안** — 2026-08-25, D019의 지시(ORB 구성이 1차 목표) 아래 작성. 아래의
"오늘"은 전부 그날 코드에서 읽었다.

---

## 1. Two measurements that frame everything / 모든 것을 규정하는 두 측정

**The project cites CORBA 3.4 fifty-six times and `orbweaver-object` cites it
zero times.** Every other wire-level decision in this workspace carries its
section — `§7.11.3` in the lexer, `§9.4.9` in the fragment code, `§7.6.6.5` in
the codeset table. The POA carries none. That is not a small thing: a POA is
the half of CORBA a *server author* meets, and it was built without the chapter
open.

**The POA holds ids, not servants.** `active: HashMap<ObjectId, ()>` — the
value is unit. The servant lives in the `Dispatch` implementation the `Server`
calls, and a multi-object skeleton takes its identity as an explicit `Target`
argument, because (`COMPONENTS.md`, gen row) *none of the five hand-written
servants keeps a value per object*. **The specification's Active Object Map
maps `ObjectId → Servant`. Ours does not, and that difference is deliberate.**

Both facts point the same way: what is owed here is **not a faithful
transcription of Chapter 15**. It is to say, for each thing the chapter names,
which value we behave as and whether we chose it. (Chapter 15, verified — the
first draft of this document said 11, which is where the POA sat in CORBA 2.x.
The document that says "cite the section" got its own section wrong, which is
the argument for §8 rather than against it.)

*프로젝트는 CORBA 3.4를 쉰여섯 번 인용하고 `orbweaver-object`는 영 번 인용한다.
그리고 우리 POA는 서번트가 아니라 id를 들고 있다 — 명세의 Active Object Map과
다르고, 그 차이는 의도적이다. 둘 다 같은 곳을 가리킨다: 여기서 갚아야 할 것은
15장의 충실한 옮겨적기가 아니라, 장이 이름한 것마다 **우리가 어느 값으로
행동하는지, 그리고 그것을 고른 적이 있는지**를 말하는 것이다. — 15장은 검증한
것이고, 이 문서의 초안은 11장이라고 적었다. CORBA 2.x의 자리다.*

## 2. What exists today, measured / 오늘 있는 것

```
pub struct Poa {
    name, active: HashMap<ObjectId, ()>, lifespan, unknown_id,
    incarnation: u64, type_id: String, published: Option<(String,u16)>,
    next_transient: AtomicU64,
}
```

Thirteen public methods: `new` · `with_lifespan` · `with_unknown_id` ·
`publish_at` · `name` · `activate` · `activate_new` · `deactivate` ·
`is_active` · `object_key` · `parse_key` · `reference` · `dispatch_target`.

Two policy types: `Lifespan::{Transient, Persistent}` and
`UnknownIdPolicy::{Reject, AskLocator}`. One trait: `ServantLocator` with
`Located::{Here, Forward(Ior), Unknown}`.

## 3. The seven policies, and the value we behave as / 일곱 정책과 우리의 값

**Verified 2026-08-25 against the published document** — *CORBA — Part 1:
Interfaces, v3.4* (OMG, `omg.org/spec/CORBA/3.4/Interfaces`), text extracted
from the PDF and read. The first draft of this table cited **§11.3.7 from
working knowledge and was wrong**: the POA is **Chapter 15**, and the policies
are **§15.3.8**. Chapter 11 is where the POA sat in CORBA 2.x. That is the
error §8 said to expect, caught by doing what §8 required.

| # | Policy · spec section | Values (**default**) | What we do today | Chosen? |
|---|---|---|---|---|
| 1 | **Thread** §15.3.8.1 | **`ORB_CTRL_MODEL`** · `SINGLE_THREAD_MODEL` · `MAIN_THREAD_MODEL` | `Server::serve` is sequential per connection; `serve_shared` dispatches concurrently behind `&D`. Neither is named a policy | **no** |
| 2 | **Lifespan** §15.3.8.2 | **`TRANSIENT`** · `PERSISTENT` | `Lifespan::{Transient,Persistent}`, and `incarnation` makes a transient key from an earlier run refuse | **yes** — the one policy we named, **and we default to the same value** |
| 3 | **Object Id Uniqueness** §15.3.8.3 | **`UNIQUE_ID`** · `MULTIPLE_ID` | not modelled: the map has no servant, so "one servant, many ids" has no shape to be true or false in | **n/a by design** — say so |
| 4 | **Id Assignment** §15.3.8.4 | **`SYSTEM_ID`** · `USER_ID` | **both, on the same POA**: `activate(id)` is USER_ID and `activate_new()` is SYSTEM_ID | **no — and it is a divergence** |
| 5 | **Servant Retention** §15.3.8.5 | **`RETAIN`** · `NON_RETAIN` | **`RETAIN`** — corrected by Stage A, see below | **no** — behaves as the default |
| 6 | **Request Processing** §15.3.8.6 | **`USE_ACTIVE_OBJECT_MAP_ONLY`** · `USE_DEFAULT_SERVANT` · `USE_SERVANT_MANAGER` | `UnknownIdPolicy::Reject` ≈ the first, `AskLocator` ≈ the third with a `ServantLocator`. `USE_DEFAULT_SERVANT` has no analogue | **partly** |
| 7 | **Implicit Activation** §15.3.8.7 | `IMPLICIT_ACTIVATION` · **`NO_IMPLICIT_ACTIVATION`** | nothing activates implicitly; a servant is reached only through an id already in the map or through the locator | **no** — behaves as the default |

Two things the verification changed beyond the numbers:

- **The spec's own ordering is not the one recalled.** §15.3.8.5 is Servant
  Retention and §15.3.8.6 is Request Processing; Implicit Activation is last.
  The draft had Implicit Activation fifth.
- **`IMPLICIT_ACTIVATION` requires `SYSTEM_ID` *and* `RETAIN`** (§15.3.8.7,
  verbatim). That is a policy interaction, and it is the kind of constraint a
  `Policies` type can carry and a hand-written adapter cannot.

> **Corrected 2026-08-25 by Stage A, which measured what this table read off a
> name.** Row 5 above first said we were `NON_RETAIN`-shaped, and the reason
> given was that `AskLocator` uses a `ServantLocator` — which is the
> specification's `NON_RETAIN` half. That is reading a policy off an
> identifier. Measured instead: `dispatch_target` **inserts the located id into
> `active`**, and the *next* request for that key is served with **no locator
> passed at all**. That is `RETAIN` with a `ServantActivator` (§15.3.8.6's
> "RETAIN and USE_SERVANT_MANAGER" combination), wearing a name borrowed from
> the other half.
>
> Stage A found a second thing the table could not: **`USE_SERVANT_MANAGER`
> with no manager registered diverges.** §15.3.8.6 says `OBJ_ADAPTER` with
> minor code 4; `AskLocator` with no locator answers `Target::Unknown` and so
> `OBJECT_NOT_EXIST`. Recorded at the site, not fixed — it is a wire-visible
> answer and belongs to a batch that can measure it against a peer.
>
> Both corrections come from the same place: **the table was written from the
> code's vocabulary and the code's vocabulary had drifted from the
> specification's.** That is the argument for §6 Stage A being first — writing
> the seven down is what forced the reading.
>
> *두 정정 모두 같은 데서 나왔다 — 이 표는 코드의 어휘로 쓰였고, 코드의 어휘가
> 명세의 어휘에서 이미 어긋나 있었다. 일곱을 적어보는 일이 그 독해를 강제했다.*

**Item 4 is the finding.** A POA that answers to both id-assignment models is
not a POA the specification describes, and nothing today prevents a caller from
mixing them on one adapter — which is exactly how two references can collide in
a way no test would catch. It is small to fix and it is a real divergence, not
a naming gap.

## 4. What the chapter has that we do not / 장에 있고 우리에게 없는 것

- **POAManager** (§15.3.2) — `enum State {HOLDING, ACTIVE, DISCARDING,
  INACTIVE}`, verbatim from the IDL at §15.3.2.6, with `activate`,
  `hold_requests`, `discard_requests`, `deactivate` and `get_state` as
  §15.3.2.2–.7. The thing a server uses to stop taking work without tearing the
  endpoint down. We have a stop flag in `Server::serve` and nothing
  addressable.
- **POAManagerFactory** (§15.3.3) — a 3.x addition; a named manager can be
  found rather than only created with its POA.
- **The POA hierarchy** — `create_POA`, `find_POA`, `the_parent`,
  `the_children`, and `RootPOA` as the tree's root. We have flat, independent
  `Poa` values.
- **`AdapterActivator`** — creates a child POA on demand when a request names
  one that does not exist.
- **`ServantActivator`** — the `RETAIN` half of servant management. We have
  only `ServantLocator`, the `NON_RETAIN` half.
- **The standard operations** — `activate_object_with_id`, `servant_to_id`,
  `id_to_servant`, `servant_to_reference`, `reference_to_servant`,
  `reference_to_id`, `id_to_reference`, `create_reference`,
  `create_reference_with_id`. Ours are `activate`, `activate_new`,
  `reference`, `parse_key` — the same jobs under names a CORBA server author
  would not search for.
- **The exceptions** — `AdapterAlreadyExists`, `AdapterInactive`,
  `AdapterNonExistent`, `InvalidPolicy`, `NoServant`, `ObjectAlreadyActive`,
  `ObjectNotActive`, `ServantAlreadyActive`, `ServantNotActive`,
  `WrongAdapter`, `WrongPolicy`. Ours return `bool` and `Option`.

## 5. What must not be transcribed / 옮겨 적지 않을 것

The chapter is large and most of it exists to serve a servant model we do not
have. Transcribing it would fight a design that was measured into place:

- **The servant-holding Active Object Map.** Ours maps ids because the servant
  is the `Dispatch` impl. `USE_DEFAULT_SERVANT` and `IdUniqueness` are
  meaningful only inside a map that holds servants; both should be recorded as
  **deliberately not applicable**, with this reason, rather than implemented.
- **`ThreadPolicy`'s three models as a runtime choice.** Rust's ownership makes
  `serve` and `serve_shared` two typed shapes rather than one adapter with a
  mode. What is owed is a sentence saying which spec model each corresponds to.
- **Anything for which no consumer exists.** `AdapterActivator` creates POAs on
  demand for a naming hierarchy we do not have; it earns its place when the
  hierarchy does.

*장의 대부분은 우리에게 없는 서번트 모델을 위해 존재한다. 옮겨 적으면 측정으로
자리 잡은 설계와 싸우게 된다.*

## 6. What is proposed / 제안

Four stages. Each keeps every existing call site compiling — the compatibility
requirement is not a nicety here: `naming_server`, `event_server`,
`expert_service` and `tenant_service` are all built on today's surface, and
`spikes/` has twelve assembly sites.

### Stage A — write down the seven, change nothing

A `policy` module naming all seven policies and their spec values, with **our
value for each** and the reason, cited to the verified section. `Poa` gains
`policies() -> Policies` reporting what it behaves as. No behaviour changes; no
existing signature changes.

**Why first.** It converts seven implicit choices into stated ones, which is
D018 §3.3's whole argument, and it is the step most likely to find a surprise —
item 4 was found by *writing the table*, not by reading the code.

**Oracle.** A test asserting the reported policies match the behaviour: for
each policy, an assertion that exercises the value we claim. The negative
control is changing a claimed value and watching the behavioural assertion
fail.

### Stage B — `IdAssignment` becomes a real policy

The one divergence. `Poa::new` takes (or defaults) an `IdAssignment`, and
`activate(id)` refuses under `SYSTEM_ID` while `activate_new()` refuses under
`USER_ID` — the spec's `WrongPolicy`.

**Compatibility.** Today's default must accept both, or four servants and
twelve spikes stop compiling. So: default `IdAssignment::Either`, a **fourth
value that is ours and not the spec's**, documented as the backward-compatible
mode and as the one a new POA should not choose. The two spec values are
available and refuse correctly; `Either` is what existing code keeps getting.
*A divergence that is named and defaulted-to is a different thing from one
nobody noticed.*

**Oracle.** `WrongPolicy` on each crossing, and every existing caller still
compiling and still passing.

### Stage C — POAManager

`HOLDING` · `ACTIVE` · `DISCARDING` · `INACTIVE` as a state a `Server` reads
before dispatching. This is the stage with a **consumer already waiting**: the
operator surface (D015 §3.1, landed) can configure a deployment but cannot tell
a running server to stop taking work, and `Server::serve`'s stop flag tears the
endpoint down rather than holding requests.

**Compatibility.** A `Poa` with no manager behaves exactly as today (always
`ACTIVE`).

**Oracle.** A peer's request held in `HOLDING` and answered after `ACTIVE`;
`TRANSIENT` refused in `DISCARDING`; the endpoint still bound in both. This is
measurable against omniORB's client, which is the reason to prefer it over the
hierarchy.

### Stage D — the standard names, and the hierarchy if a consumer names it

`servant_to_reference`, `reference_to_id`, `id_to_reference`,
`create_reference_with_id` as the names a CORBA server author searches for,
delegating to today's methods. The hierarchy (`create_POA`, `find_POA`,
`the_parent`) **waits for a trigger** — it earns its place when something needs
more than one adapter, and `PLAN-DEFERRED`'s shape is where that reason goes.

### Stage A — landed 2026-08-25 / 착지

The seven are written down. `crates/orbweaver-object/src/policy.rs` gives each
policy a Rust enum with the specification's value names, a `Policy` trait
carrying `NAME` / `SECTION` / `STANCE` / `SPEC_DEFAULT`, and `Default` held to
the spec's default by a test — **so the table is compiled rather than prose**.
`Poa::policies()` reports; nothing is stored and nothing is configurable.
`Policies::spec_violations()` compiles the constraints §15.3.8 states *between*
policies, including §15.3.8.7's verbatim requirement that `IMPLICIT_ACTIVATION`
also requires `SYSTEM_ID` and `RETAIN`.

**Five of seven claims carry a behavioural test in the same test as the
claim.** Two do not, and say so in their own doc comments rather than being
covered by a test that would pass whatever they said: §15.3.8.1 (Thread) is not
observable from this crate — the concurrency is `orbweaver_giop::Server`'s —
and §15.3.8.3 (Object Id Uniqueness) is not observable *in principle*, being a
policy about servants in a map that holds none.

Ten negative controls, each applied alone and reverted. The one worth reading:
making `dispatch_target` consult the locator under `Reject` as well took the
new test red **while the other 98 tests in the crate passed** — that gap is
exactly what the new test closes, and it is the reason a `policies()` that
reported strings nobody checked against behaviour would have been the
green-while-measuring-nothing class in a new coat.

Workspace 1570 → 1585 passed, 0 failed. No signature changed, no behaviour
changed.

**Stage A's sweep also found what Stage B inherits.** `crates/orbweaver-object`
carries 81 implicit choices; two are id-assignment facts and belong to Stage B:
the `obj{n}` generator shares a namespace with caller-chosen ids like
`ObjectId::from_name("obj1")`, and — unstated anywhere — **a POA name must not
contain `/` and no two POA names may be prefixes of one another**, or
`Poa::new("Root")` with id `POA/x` and `Poa::new("Root/POA")` with id `x` mint
**the identical object key**. `tenant_service::is_key_safe` enforces exactly
that rule for the other key space and neither names the other.

*Stage A의 스윕이 Stage B가 물려받을 것도 찾았다: 자동 id가 호출자 id와
네임스페이스를 공유하고, **POA 이름 둘이 서로의 접두사이면 동일한 객체 키가
만들어진다** — 어디에도 적혀 있지 않다.*

## 7. Compatibility, stated as a rule / 호환성 규칙

**No stage may break a caller that compiles today.** Concretely: `Poa::new`,
`with_lifespan`, `with_unknown_id`, `publish_at`, `activate`, `activate_new`,
`deactivate`, `is_active`, `object_key`, `parse_key`, `reference` and
`dispatch_target` keep their signatures and their behaviour under the defaults.
Every new policy defaults to what the code does now — the property the MCP
`--config` batch proved as *absent is not zero*, applied to an API instead of a
file.

Where the specification and today's behaviour disagree and we keep today's
(Stage B's `Either`), the divergence is **named in the type**, recorded here,
and — following the corpus rule for the front end — gets a row in
`corpus/divergences.tsv` if it is ever observable to a peer.

## 8. What this document does not claim / 주장하지 않는 것

It no longer guesses at section numbers: §3's table was verified against
*CORBA — Part 1: Interfaces, v3.4* on 2026-08-25 and **the first draft was
wrong about the chapter** (11, which is CORBA 2.x's, rather than 15). Every
number in §3 and §4 is now read. Where a claim here is still recalled rather
than read, it says so. It does not claim
the POA is wrong — it is measured over the wire against two peers through four
servants. And it does not propose the CORBA C++ POA API in Rust: what is owed
is the chapter's **vocabulary and semantics**, so that a server author's
knowledge transfers, not its signatures.
