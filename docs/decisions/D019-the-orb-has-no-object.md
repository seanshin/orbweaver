# D019 — The ORB has no object, and everything above it assembles one by hand

**STATUS: PROPOSED** — drafted 2026-08-25 from a challenge that landed: *is it
not strange that development continues without an ORB?* It is, and the
measurements below say so more sharply than the question did. This proposes the
missing thing and argues its surface **from what is scattered today**, not from
the C++ mapping. Not self-approvable: it changes the shape of the product's
own API, which is the user's to decide.

**상태: 제안** — 2026-08-25, *"ORB도 없이 후속 개발이 진행되는 것이 이상하지
않나"* 라는 지적에서 작성. 이상하고, 아래 측정이 질문보다 더 날카롭게 그것을
말한다. 제안하는 표면은 C++ 매핑이 아니라 **오늘 흩어져 있는 것**에서 논증한다.

> **Direction, 2026-08-25.** The user designated **ORB composition the primary
> goal** on reading §1–§3. That is a decision about *priority* and it is
> recorded here as one; the `STATUS` line above stays PROPOSED because what it
> gates is the **shape** in §5 — four named responsibilities and a refusal to
> copy anything else — and that is a separate question the approval phrase
> settles. Work sequenced against this direction is in §8.
>
> *사용자가 §1–§3을 읽고 **ORB 구성을 1차 목표로 지정**했다. 그것은 우선순위에
> 대한 결정이며 그렇게 기록한다. 위의 `STATUS`가 제안으로 남는 이유는 그것이
> 가로막는 것이 §5의 **모양**이고, 그것은 별개의 질문이기 때문이다.*

---

## 1. The measurement that settles it / 결론을 내는 측정

`crates/orbweaver-giop/src/naming.rs` **parses the bootstrap URL**:

```
corbaloc:rir:NameService  ->  ObjectUrl::InitialReference("NameService")   (:119)
corbaloc:rir:             ->  ObjectUrl::InitialReference("NameService")   (:761)
```

and then, thirty lines later:

```rust
ObjectUrl::InitialReference(_) => return None,                              (:152)
```

**The ORB speaks the language of `resolve_initial_references` and has nothing
to resolve against.** Not because the feature was deferred with a reason —
there is no reason anywhere, in any plan document, any deferral chapter or any
decision. It is the one state this project's rules do not allow.

*ORB가 `resolve_initial_references`의 언어를 말할 줄 알면서 그것을 대조할 표를
갖고 있지 않다. 유예된 것이 아니다 — 이유가 어디에도 없다.*

## 2. What else is already there under another name / 이미 다른 이름으로 있는 것

The capabilities exist; what is missing is the object that owns them.

| CORBA calls it | We have | Where |
|---|---|---|
| `string_to_object` | `Ior::parse` of `IOR:<hex>` | `giop/src/lib.rs:672` |
| `object_to_string` | `Ior::to_string` | `giop/src/lib.rs:731` |
| `resolve_initial_references` | the URL parses; **nothing answers** | `naming.rs:119` → `:152` |
| the ORB's transport | `Pool`, `Connection`, `Server` | `pool.rs`, `lib.rs`, `server.rs` |
| the root POA | `Poa` | `object/src/lib.rs` |
| ORB configuration | **seven compile-time constants** | see §3 |

## 3. The numbers a deployment owns, one layer below where they were just fixed

D015 §3.1 was built the same day: `orbweaver-mcp-server --config <policy.json>`
now carries handle expiry, the quota, the exposure allowlist, the audit bound,
the search cap and the dial timeout. **The ORB's own numbers did not move**, and
they are the ones a network operator changes first:

| Constant | Value | Configurable? |
|---|---|---|
| `DEFAULT_MAX_MESSAGE_SIZE` | 64 MiB | compile-time |
| `DEFAULT_FRAGMENT_THRESHOLD` | 1 MiB | compile-time |
| `MAX_FRAGMENTS` | 4096 | compile-time |
| `MAX_FORWARD_HOPS` | 8 | compile-time |
| `FOLLOW_TIMEOUT` | 10 s | compile-time |
| `DEFAULT_MAX_CONNECTIONS` | 64 | compile-time |
| `STOP_POLL` | 50 ms | compile-time |

Three have a setter somewhere; none has a home a deployment can reach. So
**D015's acceptance sentence — *"without editing Rust, without a rebuild"* — is
still false one layer below where that batch made it true.** That is the
strongest practical argument here, and it is the reason this comes before the
service work in D018: the operator surface is not finished, it is half-built,
and the half that is missing is the ORB's.

*D015의 합격 문장이 그 배치가 참으로 만든 층 **바로 아래에서** 여전히 거짓이다.
운영자 표면은 완성된 것이 아니라 절반만 지어졌고, 빠진 절반이 ORB의 것이다.*

## 4. The cost that compounds / 누적되는 비용

Every consumer assembles the ORB by hand today: a `Pool`, a `Server`, a `Poa`,
an `Exposure`, and the constants above accepted as given. Five spike binaries
do it, the MCP server does it, and each new servant adds another site. D018
proposes reasons and triggers for the services CORBA defines and we do not
serve; **every one of those, if a trigger ever fires, adds one more hand
assembly.** The scattering is not a static debt — it grows with the thing the
project is for.

## 5. What is proposed, and what is explicitly not / 제안과 비제안

**Proposed: one object that owns what an ORB owns.** Argued from §2 and §3, not
from a language mapping — there is no OMG Rust mapping, so `ORB_init`'s
spelling is not normative for us. What *is* normative is the **set**: the
standard tells us which facts belong to an ORB even though it cannot tell us
their Rust names.

Its surface, minimally, is exactly what is scattered:

- **The initial references table** — `register_initial_reference(name, ior)` /
  `resolve_initial_reference(name)`, which makes `naming.rs:152` answer instead
  of returning `None`, and makes `corbaloc:rir:NameService` work from a foreign
  client. The URL parser is already written.
- **`string_to_object` / `object_to_string`** — named, as the two operations
  every CORBA programmer looks for, delegating to the `Ior` code that exists.
- **The configuration in §3**, read once at construction, defaults exactly
  today's constants so nothing changes for an existing caller — the shape the
  MCP `--config` batch already proved and whose tests can be copied.
- **The transport and the root POA it hands out**, so a consumer asks the ORB
  for them rather than constructing both and hoping they match.

**Not proposed**: a faithful `ORB_init` signature, `ORB::run`/`shutdown`
semantics, thread policies, or anything else copied because the C++ mapping has
it. Each of those earns its place from a scattered fact or from a trigger, the
same rule as everything else here. This document proposes the object and its
**four** named responsibilities, and nothing beyond them.

## 6. Why this is a decision and not a batch / 왜 배치가 아니라 결정인가

It changes the product's own API shape: after it, the honest way to use this
ORB is to ask the ORB, and every existing assembly site becomes a caller. That
is a one-way door of the kind this project puts in `docs/decisions/` — and the
four documents written earlier today (D014, D015, D016, D017) all avoided
noticing this gap precisely because each was written from an instrument that
can only see what exists. A missing object is invisible to a gap column.

**The order this implies**, if approved: D019 before D018's service work and
before D016 §4's cross-crate batches, because both add call sites to the thing
that should be doing the assembling.

*이것은 제품 API의 모양을 바꾼다 — 이후로 이 ORB를 쓰는 정직한 방법은 ORB에게
묻는 것이고, 기존 조립 지점은 전부 호출자가 된다. 오늘 먼저 쓰인 문서 넷이 이
공백을 알아채지 못한 이유는 정확히, 각각이 **있는 것만 볼 수 있는 도구**에서
쓰였기 때문이다. 없는 객체는 갭 열에 보이지 않는다.*

## 7. What this document does not claim / 주장하지 않는 것

It does not claim the wire is wrong — it is measured against two peers at three
GIOP versions and that work stands. It does not claim the Rust API should look
like C++'s. And it does not claim the ORB object would have been found by any
existing gate: **nothing in this workspace can go red for a missing object**,
which is the honest reason it went unnoticed for the project's whole life and
was found by a person asking a question.

## 8. Sequenced against the direction / 지시에 따른 순서

Re-measured 2026-08-25 before writing this section; every file:line below was
read rather than remembered.

### What the URL layer already knows / URL 계층이 이미 아는 것

`ObjectUrl` (`giop/src/naming.rs`) has **three** variants, and the difference
between them is exactly the gap:

| Variant | Carries | Resolvable today |
|---|---|---|
| `Corbaloc { addresses, object_key }` | an address | **yes** — `to_ior` builds a profile per address |
| `Corbaname { addresses, object_key, name }` | an address **and** a name to resolve inside it | **yes** — the address is explicit, the naming service does the rest |
| `InitialReference(String)` | **only a well-known name** | **no** — `to_ior` returns `None` (`:152`) |

So the missing thing is precise and small: **a table from a well-known name to
an IOR, for the one case where the caller supplies no address.** Everything
else in the URL layer works because the address was given.

`to_ior`'s callers make that `None` visible: `giop/tests/codesets_on_the_wire.rs:185`
does `.expect("it becomes an IOR")` and six sites in `nat.rs` do `.unwrap()`.
None passes an `rir` today, so none panics — but the shape of the return type
is what tells a reader the case exists and is unanswered.

### Step 1 — the initial references table

- **Where.** `giop/src/naming.rs:152` stops being `return None`. The table
  itself belongs on the ORB object, so step 1 introduces the smallest possible
  version of that object: a thing that holds `BTreeMap<String, Ior>` and
  answers `resolve_initial_reference(&str)`.
- **What the standard fixes.** The names are not ours to invent —
  `NameService`, `InterfaceRepository`, `RootPOA` and the rest are OMG-assigned,
  and `naming.rs:761-765` already tests two of them (`corbaloc:rir:` defaults
  to `NameService`; `corbaloc:rir:InterfaceRepository` parses). Register what
  we actually serve and refuse an unknown name **by name**, never silently.
- **Oracle — and this is the reason it is first.** It is the only step in this
  list whose measurement is a **peer**, not a test of our own belief:
  `spike-names` already publishes a naming context, and omniORB's client can be
  handed `corbaloc:rir:NameService` instead of a file-read IOR string. If it
  resolves and calls, the table is right. The harness group is one more line in
  the naming block that already exists.
- **Negative control.** Empty the table, keep the code: the peer must fail with
  a refusal naming `NameService`, not with a panic and not with a silent
  `None`. And the reverse: a name we do not serve must be refused by name.
- **Precondition.** The `orbweaver-giop` branch in flight (the mid-reply
  fixture, `spikes/half_reply*`) lands first — same crate, and merging around
  an unlanded branch is the conflict this session already measured.
- **Footprint.** `crates/orbweaver-giop`. One crate.

### Step 2 — the two conversions get their names

- **Where.** `Ior::parse` (`giop/src/lib.rs:676`) and `Ior::to_stringified`
  (`:736`) already do exactly `string_to_object` and `object_to_string`. The
  batch is **naming and routing**, not new behaviour: the ORB object exposes
  the two operations under the names every CORBA programmer looks for, and
  every site that spells the conversion itself calls them.
- **Why it is not cosmetic.** `to_stringified` is a *serialiser* name; the
  operation a caller wants is *"give me the object this string denotes"*, and
  the difference matters at the one place the two diverge — a string that is a
  `corbaloc:`/`corbaname:` URL rather than an `IOR:<hex>` blob. Today those are
  two different functions in two modules and the caller must know which it has.
  **`string_to_object` is the one that decides**, which is precisely why the
  standard has one operation and we have two.
- **Oracle.** Round-trip every IOR in the corpus and every URL form in
  `naming.rs`'s tests through the single entry point; the existing tests for
  both halves become its callers rather than being duplicated.
- **Precondition.** Step 1, because the `rir` case is one of the forms
  `string_to_object` must decide about.
- **Footprint.** `crates/orbweaver-giop`.

### Step 3 — the seven numbers get a home

`DEFAULT_MAX_MESSAGE_SIZE` (64 MiB, `lib.rs:75`) · `MAX_FORWARD_HOPS` (8, `:78`)
· `FOLLOW_TIMEOUT` (10 s, `:84`) · `DEFAULT_FRAGMENT_THRESHOLD` (1 MiB, `:91`)
· `MAX_FRAGMENTS` (4096, `:97`) · `DEFAULT_MAX_CONNECTIONS` (64,
`server.rs:910`) · `STOP_POLL` (50 ms, `:916`).

- **Copy the shape that was just proved.** The MCP `--config` batch landed the
  same day with three properties this step should reuse verbatim: *absent is
  not zero* (every setting `Option`, so "no configuration changes nothing" is a
  property of the type rather than a claim a test chases), *default-deny cannot
  be widened by an absence*, and *refused whole or applied whole* with the
  file, the key and the expectation named. Its tests are the template and its
  five negative controls are the model.
- **Scope to the rule, not the seven.** The rule is the same one that batch
  used: *a number only a deployment can know has one home, and it is not a
  source file.* Sweep `orbweaver-giop` for the others — `csiv2.rs:44`'s 15,
  `event_server.rs`'s 64/3/two durations — and give each a verdict:
  configurable, or in the code with the reason written.
- **What must not move.** `MAGIC`, `HEADER_LEN`, the `TAG_*` constants and the
  repository ids are the **specification's**, not a deployment's. A
  configuration key for one of those would be a way to write a non-conformant
  ORB from a file.
- **Precondition.** Step 1 (the ORB object must exist to hold them);
  independent of step 2.
- **Footprint.** `crates/orbweaver-giop`.

### Step 4 — the ORB hands out the transport and the root POA

- **The measurement that sizes it.** Twelve files construct `Pool`, `Server` or
  `Poa` by hand today — `giop/src/bin/{spike_concurrent,spike_mux,spike_events,spike_names,spike_nat}.rs`,
  `object/src/bin/{spike_server,spike_wide,spike_experts,spike_tenants}.rs`,
  `registry/src/bin/{spike_ifr,spike_ingest}.rs`, `forge/src/bin/sidl_infer.rs`
  — across a workspace of 38 spike binaries. Each becomes a caller.
- **Why it is atomic and cannot be parallelised.** It is the only step touching
  both `orbweaver-giop` and `orbweaver-object`, and the root POA cannot be
  handed out by an ORB that does not own the transport it dispatches on.
  Landing half turns the other half red — D016 §4's class.
- **Precondition.** Steps 1–3, **and the §5 shape approved**: this is the step
  that makes the API one-way, and it is what the approval phrase gates.
- **Footprint.** `orbweaver-giop` + `orbweaver-object`, one commit.

### What must not happen / 해서는 안 되는 것

- **No step invents a name.** `resolve_initial_reference`'s keys are OMG's;
  the operations' names are OMG's. Where the standard names a thing, we use
  that name, so that a reader who knows CORBA finds it.
- **No step copies a C++ signature because the C++ mapping has it.** §5 already
  refuses `ORB_init`'s shape, `run`/`shutdown`, and thread policies. Each earns
  its place from a scattered fact or a fired trigger, like everything else.
- **No step changes behaviour by default.** Steps 1–3 each default to exactly
  today's values and today's answers, which is what lets them proceed under the
  direction without waiting on the shape question. If a step cannot be written
  that way, that is a finding and it stops the step.
- **Do not fold the naming *service* into the ORB.** `naming_server.rs` serves
  `CosNaming` over the wire and is a servant; the initial references table is a
  bootstrap that answers before any servant is reached. They meet at one entry
  (`NameService` resolves to the naming server's IOR) and are otherwise
  different things.

### Ordering against everything else / 다른 것들과의 순서

D018's service work and D016 §4's cross-crate batches queue **behind step 4**,
since both would otherwise add call sites to the thing that should be doing the
assembling. D016 §3's six parallel batches are unaffected — none touches the
transport — and can continue alongside steps 1–3.

*1–3은 각각 한 크레이트이고 기본값이 오늘의 동작이므로 지시만으로 진행할 수
있다. **4가 승인 문구가 가로막는 단계**다. 단계 1이 첫째인 이유는 크기가 아니라
**오라클이 피어**라는 점이다 — 우리 믿음을 단언하는 테스트가 아니라, 외부
클라이언트가 우리에게 이름을 해소해 호출까지 가는 것.*
