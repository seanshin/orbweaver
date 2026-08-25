# D026 — Seed data, and the environment a measurement needs to exist in

**STATUS: PROPOSED** — drafted 2026-08-26 from a reading of the harness's own
verdict: seven groups report `SKIPPED` and every one of them is an *absence in
the environment*, not an absence in the code. Every figure below was measured
that day. Not self-approvable: §4 proposes a rule about what a fixture may
invent, and §6 proposes spending money and CI minutes.

**상태: 제안** — 2026-08-26, 하네스 자신의 판정에서 출발: `SKIPPED` 일곱 건이
전부 코드의 부재가 아니라 **환경의 부재**다.

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

**The first job of the seed is not new coverage.** It is to answer, for the
five fixtures counted in §1, whether their populations already disagree.
Migrating them is where the finding is: *"the same"* `PolicyDomain` in two
files is a claim nobody has checked.

**Oracle.** Every migrated fixture produces byte-identical output before and
after, or the difference is explained. That is the same discipline the emitted
stubs are re-blessed under.

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
