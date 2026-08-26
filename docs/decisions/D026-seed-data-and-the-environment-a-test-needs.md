# D026 — Seed data, and the environment a measurement needs to exist in

**STATUS: PROPOSED** — drafted 2026-08-26 from a reading of the harness's own
verdict: seven groups report `SKIPPED` and every one of them is an *absence in
the environment*, not an absence in the code. Every figure below was measured
that day. Not self-approvable: §4 proposes a rule about what a fixture may
invent, and §6 proposes spending money and CI minutes.

**상태: 제안** — 2026-08-26, 하네스 자신의 판정에서 출발: `SKIPPED` 일곱 건이
전부 코드의 부재가 아니라 **환경의 부재**다.


> **Priority zero, set 2026-08-26.** This document is subordinate to the ORB
> completion criterion, whose home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6: *no leak in the
> transparency that a caller can invoke any target holding only a reference,
> without knowing its location, backend, language or load state, and that this
> survives targets being added, removed, moved, loaded or evicted at runtime.*
> The criterion is stated there and **not restated here** — what is recorded
> below is only how this document's work bears on it.
>
> *0순위 기준의 집은 D029 §6이며 여기서 다시 적지 않는다. 아래에 적는 것은 이
> 문서의 작업이 그 기준에 **어떻게 닿는지**뿐이다.*

> **How this bears on it.** A leak is only visible against a *named* population.
> S1b's three sentences — a named population crossed intact, ranking is correct
> rather than merely running, a divergence has a subject — are the preconditions
> for testing load and backend transparency at all: you cannot assert that a
> caller could not tell an expert was evicted without stating which experts there
> were. D029 §5's O0 is the consumer of this work.

---

## 1. The measurement / 측정

`./spikes/run_checks.sh` on `b3c08ad`: **311 ok, 0 FAIL, 7 SKIPPED groups.**
The zero is real. The seven are the subject of this document, and they sort
into exactly two kinds:

| # | Group | What is absent | Kind |
|---|---|---|---|
| 1 | SSLIOP residue | omniORBpy `sslTP`, or JacORB's SSL transport configured | **environment** |
| 2 | differential, TAO column | `tao_idl` — Ubuntu ships no package (established, not assumed) | **environment** |
| 3 | search benchmark I3 | `VOYAGE_API_KEY` — the synonym class and the injection class against a real embedding model | **environment (paid)** |
| 4 | NAT container probe | docker | **environment** |
| 5 | NAT second host | a multipass VM (`ORBWEAVER_NAT_VM=1`); last measured 2026-08-14 | **environment** |
| 6 | end-to-end S1–S3 | a producer command and its key (`E2E_MODEL=1`); **replayed from a recording of 2026-08-14** | **environment (paid)** |
| 7 | CSIv2 identity | a peer advertising CSIv2 and an issuer (`ORBWEAVER_IDP_URL`) | **environment** |

**Five environment variables gate the harness** — `E2E_MODEL`,
`ORBWEAVER_IDP_URL`, `ORBWEAVER_NAT_VM`, `VOYAGE_API_KEY`,
`ORBWEAVER_CONCURRENCY_RUNS` — and four of the five gate a `SKIPPED`.

**And the seed half, measured the same day.** Five fixtures each build their
runtime population by hand, with no shared source:

```
spike_ifr      4 seeding calls / 392 lines
spike_tenants  9 / 593
spike_experts 26 / 735
spike_names   23 / 237
spike_events   9 / 474
```

There is **no seed module anywhere in `crates/`**. The corpus is rich in
*contracts* — `corpus/golden`, `negative`, `services`, `pragma`, `include`,
`requirements`, `queries`, `annotations` — and holds no *runtime state* at all.
Every offer a trader ranks, every name a naming graph binds, every tenant, every
channel is invented at the fixture that needs it, in Rust, inline.

*계약의 코퍼스는 풍부하고 **런타임 상태의 코퍼스는 없다**. 트레이더가 순위를
매기는 오퍼, 네이밍 그래프가 묶는 이름, 테넌트, 채널 — 전부 필요한 픽스처에서
인라인으로 발명된다.*

## 2. Why this is one document and not two / 왜 한 문서인가

They are the same defect at two scales. **A measurement needs a world to happen
in**, and today that world is assembled ad hoc at the moment of measurement or
not assembled at all. The consequences already differ in kind but not in cause:

- A fixture that invents its own population **cannot be re-run against last
  month's population**, so a regression in ranking, ordering or fan-out is
  invisible unless it is also a crash.
- Two fixtures inventing overlapping populations agree by luck. `spike_experts`
  seeds 26 times and `spike_tenants` 9; nothing says whether they mean the same
  `PolicyDomain` by the same name.
- An environment nobody can reconstruct makes a `SKIPPED` **permanent by
  default**. Item 5 was last measured 2026-08-14 and item 6 is being *replayed
  from a recording of that same day*. Twelve days is not a scandal; a mechanism
  that makes twelve days become twelve months without anyone deciding is.

**The honest part of today's state must be preserved.** Every one of the seven
lands as a counted `SKIPPED` naming its fixture, per D010 §2, and that is
working exactly as designed — *an unmeasured check is a failure, never a pass*.
This document does not propose weakening that. It proposes removing the reasons.

## 3. What must not happen / 해서는 안 되는 것

- **No CI image containing omniORB, ACE/TAO or JacORB is ever published.**
  CLAUDE.md is unambiguous: they may be built or pulled *inside* CI and never
  published as project artifacts, because publishing is redistribution. A
  provisioning plan is exactly where this rule gets broken by someone being
  helpful, and `spikes/nat/Dockerfile` already exists — the temptation to push
  it to a registry "so the probe is fast" is the whole failure in one step.
- **Seed data is not a second corpus of contracts.** `corpus/` holds IDL and
  stays that way. Runtime state is a different artifact with a different job.
- **A seeded population does not become the only population.** A fixture that
  can *only* run against the blessed seed has swapped one blind spot for
  another; the property tests and `wire-fuzz` exist because a fixed population
  is a fixed set of paths.
- **No key, token or credential is committed.** Items 3 and 6 are paid
  fixtures. The plan is a documented way to supply them, never a stored value.

## 4. The rule this proposes / 제안하는 규칙

**A fixture states where its population came from, and a population that more
than one fixture uses has one home.**

This is CLAUDE.md's *"where a fact lives"* rule applied to data rather than to
sentences, and it fails the same way: two fixtures that each retype "the same"
three offers drift apart silently, because nothing compiles a population. The
gate is the same shape as `one_home_for_a_wire_refusal.rs` — a shared
population is *loaded*, not retyped, so the drift becomes impossible rather
than detectable.

The corollary, and it is the load-bearing half: **a fixture may still invent a
population, and says so.** A one-off shape that exercises one path is good
testing and should not be dragged into a shared file to satisfy a rule. What is
forbidden is the *silent* second copy.

*픽스처는 자기 모집단의 출처를 밝히고, 둘 이상이 쓰는 모집단은 집이 하나다.
픽스처가 모집단을 발명하는 것은 여전히 허용되며 — 다만 그렇다고 말한다. 금지되는
것은 **조용한 두 번째 사본**이다.*

## 5. What is proposed — four batches that do not touch each other / 제안

Ordered by what a defect would cost, and scoped so all four can run at once.

### S1 — the runtime seed corpus (`corpus/state/`, `orbweaver-test`)

A new `corpus/state/` holding runtime populations as **data, not code**: offers
with their properties for the trader, a naming graph, IFR contents, tenants and
their manifests, event channels. Loaded by a `orbweaver-test` module every
fixture can call.

**The format decides whether this survives.** It must be readable by the Rust
fixtures *and* by the Python peers in `spikes/`, because the peer oracles are
half the measurements this project trusts — a seed only Rust can read would
leave every cross-implementation check inventing its own again. AnyJSON already
crosses that boundary and is measured doing so, which makes it the candidate to
beat rather than an obvious answer; say why whatever is chosen wins.

**And that is not a convenience — it is the point, added 2026-08-26.** A peer
check today hands *the same literal* to both ends from inside one script. When
our servant and omniORB's client agree, part of that agreement is an artifact
of a single author having typed the value twice in one file, and the project
already knows what that costs: *"a convention both ends apply cannot be refuted
by a round trip."* Twelve wire changes in v0.5.0 were found only because a
peer's bytes were recorded with provenance rather than agreed on.

**So the seed is loaded independently at both ends, from one file neither end
authored at the moment of the test.** Our ORB is populated from it through the
Rust loader; omniORB is populated from **the same bytes** through a Python
loader that uses omniORB's own stubs and shares no code with ours. What the
comparison then measures is two implementations reading one stated population —
which is what "interoperable" was supposed to mean and is a strictly stronger
claim than today's.

*시드는 **양 끝에서 독립적으로**, 테스트 시점에 어느 쪽도 저작하지 않은 한 파일에서
적재된다. 우리 ORB는 Rust 로더로, omniORB는 **같은 바이트**를 우리와 코드를 전혀
공유하지 않는 파이썬 로더로. 그때 비교가 재는 것은 하나의 진술된 모집단을 읽는 두
구현이며, 그것이 "상호운용"이 뜻해야 했던 바다.*

**The first job of the seed is not new coverage.** It is to answer, for the
five fixtures counted in §1, whether their populations already disagree.
Migrating them is where the finding is: *"the same"* `PolicyDomain` in two
files is a claim nobody has checked.

**Oracle.** Every migrated fixture produces byte-identical output before and
after, or the difference is explained. That is the same discipline the emitted
stubs are re-blessed under.

### S1b — the same population, loaded into omniORB by omniORB (`spikes/`)

The other half of S1, and the reason S1's format constraint is not negotiable.
A Python loader that reads `corpus/state/` and populates **omniORB's** side —
binding the naming graph into omniNames, registering offers, attaching event
consumers — using omniORB's own stubs and **sharing no code with the Rust
loader**. Two readers, one file.

**Three things become measurable that are not measurable today**, and they are
the deliverable rather than the loader:

1. **A named population crossed the wire intact.** Not *"a value we sent came
   back"* but *"this stated set of offers, with these properties, is what the
   other end sees."* Today no test can say that sentence because no test has a
   population to name.
2. **Ordering, ranking and fan-out become checkable.** A trader ranking three
   offers invented inline proves the ranker runs. The same ranker over a stated
   population with a stated expected order proves it ranks *correctly*, and is
   the first thing that would catch a preference-expression regression.
3. **A divergence gets a subject.** When the two ends disagree, today the
   question is *"which of these two scripts is wrong"*. With one population the
   question is *"which implementation read it differently"*, which is the
   question `corpus/divergences.tsv` was invented to answer for the front ends
   and has no equivalent for the wire.

**The licence boundary decides the shape here and must be stated first.**
omniORB is a **fixture, never a dependency**: the loader runs it as a separate
process over TCP and reads what it prints. Nothing is linked, vendored or
copied, `cargo tree` stays clean, and the loader lives in `spikes/` where every
other peer script lives. Where a service needs IDL that only omniORB ships —
`CosTrading` is the live example, named today by the trading batch — the
prerequisite is **a first-party contract written from the OMG specification**,
not their file. That is a separate batch and it owes
`differential.sh --require omniidl,jacorb_idl --record`.

**What must not happen.** The loader must not become the *only* way omniORB is
populated: the existing peer scripts prove things about ad-hoc values and
several of them are the project's best measurements. S1b adds a population that
can be named; it does not retire the ones that cannot.

*같은 파일을 omniORB 쪽에 적재하는 파이썬 로더 — omniORB 자신의 스텁으로, 우리
로더와 **코드를 전혀 공유하지 않고**. 산출물은 로더가 아니라 오늘 잴 수 없는 세
가지다: **이름 붙은 모집단이 온전히 건너갔다**는 문장, 순서·순위·팬아웃의 정합성,
그리고 **불일치에 주체가 생기는 것** — "두 스크립트 중 어느 쪽이 틀렸나"가 아니라
"어느 구현이 다르게 읽었나". omniORB는 픽스처이지 의존성이 아니다: 별도 프로세스로
돌리고 출력을 읽을 뿐이며, omniORB만 싣고 있는 IDL이 필요한 곳에서는 그들의 파일이
아니라 **OMG 명세에서 쓴 1차 저작 계약**이 선행 조건이다.*

### S2 — the environment is reconstructable (`spikes/`, `.github/`)

One script per absent fixture that provisions it, and a document that says what
each costs. Concretely: JacORB with its SSL transport configured (item 1);
`tao_idl` from source, or a measured statement that it cannot be had on this
platform (item 2 — the current CI comment already establishes Ubuntu has no
package, which is a real measurement and should be *quoted*, not repeated);
docker for the NAT probe (item 4); the multipass VM (item 5); a local IdP
container for CSIv2 (item 7).

**Each script's success criterion is that the corresponding `SKIPPED` becomes
an `ok` or a `FAIL`** — never that the script exits 0. A provisioner that
installs something the gate still cannot see has not provisioned anything, and
that is a measurable claim.

**The licence boundary is the first constraint, not a footnote.** Every script
builds or pulls locally. Nothing is published. `cargo tree` stays clean and the
existing CI gate — repaired 2026-08-26 after it was found able to report the
boundary clean without measuring it — is what proves it.

### S3 — the two paid fixtures get a decision, not a default (documents + `spikes/`)

Items 3 and 6 need money and a network: an embedding model for the search
benchmark's synonym and injection classes, and a producer for S1–S3.

They are already handled honestly — item 6 replays a dated recording and says
so — and the question is not technical. **What is proposed is a cadence and a
named owner**, per `PLAN` §8's *per release*: who runs them, how often, and what
the recording's expiry is. A replay with no stated expiry becomes a permanent
`SKIPPED` wearing an `ok`'s clothes, and the twelve days since 2026-08-14 are
the measurement that this has already begun.

Nothing here is built. The deliverable is a paragraph that makes the next
omission visible.

### S4 — the harness says how old each skip is (`spikes/`)

Every `SKIPPED` names its fixture. **None names the date it was last
measured**, and two of the seven are replays of a specific day. A skip that
carries *"last measured 2026-08-14, 12 days ago"* is a different artifact from
one that carries nothing: the first decays visibly.

Small, mechanical, and it is what makes S2 and S3 checkable rather than
aspirational. **It is also the one item that pays off even if the other three
are declined.**

## 6. Cost, stated rather than implied / 비용

S1 and S4 cost engineering time only. S2 costs CI minutes — a JacORB SSL setup
and a docker probe are not free on every push, and the plan should say which
run on push and which on a schedule. S3 costs money per run and is the only
item that does; it is proposed as a cadence precisely so the amount is a
decision rather than a surprise.

*S3만이 회당 비용이 든다. 그래서 주기로 제안한다 — 액수가 놀라움이 아니라 결정이
되도록.*

## 7. What this document does not claim / 주장하지 않는 것

It does not claim the seven `SKIPPED` groups are a failure of the harness: they
are the harness working, and D010 §2's rule is why they are visible at all. It
does not claim a seed corpus improves any pass rate — the first honest outcome
is that the five fixtures' populations turn out to disagree somewhere, and the
second is that they do not, which is worth knowing and is not an improvement.
And it does not claim these four batches are the complete set; they are the
four that follow from what was measured on 2026-08-26, and §1's table is the
thing to re-measure before adding a fifth.
