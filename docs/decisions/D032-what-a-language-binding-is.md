# D032 — What a language binding is, so that the next one is an emitter and not a project

**STATUS: PROPOSED** — drafted 2026-08-26 on a direction that the structure must
be shaped so a service can be implemented in any language. Every figure was
measured that day. Not self-approvable: §4 proposes what a language must do
before its servants are servants, which decides what every future target costs.

**상태: 제안** — 2026-08-26, 언어별 통합을 위한 서비스 구현이 가능한 형태로
구성되어야 한다는 지시에서 작성.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. Language transparency is one of its five, and it *leaks by
> construction* today: a target's language decides whether it can be a target
> at all.

---

## 1. What makes Python "a target" today, measured / 오늘 파이썬을 타깃이게 하는 것

**Not an API. A set of instruments.** That is the finding this document turns
on, because instruments are what a second language needs a share of.

| Instrument | What it holds |
|---|---|
| the differential-backed sweep | 182 values / 139 calls across golden, 70/46 across services, 0 divergences |
| `corpus/golden/28-target-keywords.idl` | every emitter's escaping rule is *executed* — it found the **Rust** list missing `yield` |
| the published refusal heads | the generated Python runtime is held to the same sentences as five Rust layers, by equality across the crate boundary |
| the harness group `Python client target` | generated Python driven against the omniORB fixture |
| repository ids, `_is_a`, exception mapping | compared against `omniidl -bpython`'s own answers |

**Three of those five are shaped around Python or around omniORB**, and that is
the cost a second language would pay again unless it is fixed first.

## 2. Three things measured today that block any second language

Each was found by a batch doing something else, which is why they are worth
stating together.

1. **`corpus/services/` is outside the differential's scope.** `differential.sh`
   globs `corpus/golden`, `corpus/requirements/generated` and `spikes` (line
   132); the corpus gate's `ENUMERATED` names the same four directories. **The
   directory holding the service contracts is not compared between front ends
   at all.** Measured by hand today: `corpus/services/ir-subset.idl` is
   **rejected by JacORB** for the already-recorded `CORBA`-scope cause — and
   **the divergence cannot be recorded**, because `divergences.tsv`'s staleness
   loop fails any row naming a file the script never checked. A real divergence
   the gate structurally forbids writing down.

   > **Corrected 2026-08-26, hours after this was drafted, by the batch that
   > took B1.** This finding is **closed and no longer describes the tree.**
   > `corpus/services/*.idl` is in `differential.sh`'s enumeration and in the
   > gate's `ENUMERATED`, and the `ir-subset.idl` divergence is recorded —
   > cause, JacORB's message and the date are in `corpus/divergences.tsv`'s
   > `ir-subset.idl` row and are **not restated here**. The staleness loop was
   > not loosened, which is what B1 said must not happen. Note what the header
   > of `corpus/divergences.tsv` now records about itself: it named the four
   > directories in prose and read as complete rather than stale, which is the
   > same defect as the omission it was describing.
   >
   > *2026-08-26 정정 — 닫혔다. `corpus/services/`는 두 목록 모두에 들어갔고,
   > `ir-subset.idl` 불일치는 `corpus/divergences.tsv`에 기록되었다(사유는 그
   > 행에 있으며 여기 다시 적지 않는다). 완화된 것은 없다.*

2. **`spikes/service_sweep.py` names its IDL inputs literally**, and for the
   standard services those inputs are **omniORB's installed `CosNaming.idl`,
   `CosEventComm.idl`, `CosEventChannelAdmin.idl` and `ir.idl`.** So the
   measurement of what our servants serve is derived from a fixture's files.
   That is why `CosTrading::Lookup` landed on the wire today and the sweep
   still cannot see it: the first-party contract now exists
   (`corpus/services/trading-lookup-subset.idl`, accepted by both front ends on
   the first pass) and **the sweep enrols nothing by directory.**

   > **Corrected 2026-08-26 — half of this closed the same day, and the half
   > that closed is not the half this finding is about.** The *payoff* landed:
   > the trader is visible to the coverage document, whose count lives in
   > `docs/SERVICES-COVERAGE.md` §8 (`### CosTrading`) and is not restated
   > here. The *honest part* landed too — `service_sweep.py` prints a
   > `#SOURCES` row per service saying which contract each operation list was
   > read from, so a service measured from a fixture's installed IDL says so
   > in the output instead of in a line number. **What did not land is the
   > finding itself:** the inputs are still four of omniORB's installed files
   > plus three first-party paths written literally, so the sweep still enrols
   > nothing by directory and the next first-party contract will be invisible
   > for the same reason the trader was. B2 below is therefore **not** closed
   > by the trader becoming visible, and that is the distinction worth keeping.
   >
   > *2026-08-26 정정 — 성과는 착지했고(트레이더가 §8에 보인다) 정직성 조항도
   > 착지했다(`#SOURCES`). **닫히지 않은 것은 이 발견 자체다** — 입력은 여전히
   > 문자열로 적힌 목록이므로 다음 1급 계약도 같은 이유로 보이지 않는다.*

3. **A servant emitter exists for exactly one language.** `skeleton.rs` is 1300
   lines of Rust servant; `python.rs` is 1059 lines of Python *client*. The
   asymmetry is D030 §2's measured fact and it is the whole of language
   transparency's leak.

   > **Corrected 2026-08-26 by the batch that took D030 §5 L1 — this finding is
   > refuted.** A servant emitter now exists for two languages:
   > `crates/orbweaver-gen/src/pyservant.rs` carries a request our ORB decoded
   > into a Python servant and back, and `crates/orbweaver-gen/tests/`
   > `python_servant.rs` is the gate. **What that closed and what it did not is
   > D029 §6.1's Language row and §6.1.1, and is not restated here** — in one
   > phrase, the construction leak is closed and three narrower ones remain.
   > The line counts above are stale by construction and are left as the dated
   > measurement they were; the sentence they supported is the part that is
   > wrong. What survives of this finding is its *last* clause read narrowly:
   > the seam carries values, and an object reference is the one value it does
   > not yet carry as a capability (§6.1.1 rows 4 and 5).
   >
   > *2026-08-26 정정 — 반증되었다. 서번트 이미터는 이제 두 언어에 존재한다.
   > 무엇이 닫혔고 무엇이 남았는지는 D029 §6.1과 §6.1.1에 있으며 여기 다시 적지
   > 않는다. 위 줄 수는 그날의 측정으로 남기고, 그것이 뒷받침하던 문장이 틀렸다.*

*셋 다 다른 일을 하던 배치가 찾았고, 그래서 함께 적을 값이 있다.*

## 3. The three layers, and which of them may ever be per-language / 세 층

A servant in language L needs exactly three things, and **only one of them may
be written per language.**

| Layer | Owner | May differ per language? |
|---|---|---|
| **The contract** — first-party IDL, repository ids, exception shapes | the corpus | **No.** One contract, all targets. |
| **The value representation** — how a value crosses the seam | AnyJSON v1 | **No.** It already crosses, measured, and a second encoding is a second thing to keep in agreement. |
| **The dispatch binding** — receiving a call in L and returning a reply | the emitter + a runtime in L | **Yes. This is the only per-language part.** |

> **Confirmed 2026-08-26, not corrected.** The table's third row stopped being
> a proposal the same day: `pyservant.rs` is the dispatch binding for one
> language, and it carries a dispatch and not a wire. The first two rows are
> unchanged and untested by that landing — one contract, one value
> representation, both still shared. Recorded because a row that became true
> is as worth dating as one that became false.

**The wire is never per-language.** D030 §4's first refusal stands: no second
ORB core until a consumer names one. The seam carries a *dispatch*, not a wire
— a binding that speaks GIOP is a second ORB wearing a binding's name.

## 4. The rule this proposes / 제안하는 규칙

**A language binding is accepted by passing a suite, not by being written. The
suite is one suite, parameterised by language — never a copy.**

D030 §3 already states the standard (*measured against a peer that is not us,
both byte orders, the same refusal sentences*). What this adds is the shape:
**the acceptance must be a parameterised instrument**, because a per-language
copy of an instrument drifts exactly the way a per-language copy of a sentence
does, and this project has measured that four times in four shapes.

Concretely, a binding is a target when, **driven by the same suite as every
other target**:

1. every operation of a stated contract set is called and answered;
2. both byte orders;
3. its refusals are **equal** to the published heads, not merely similar;
4. its exception repository ids match the contract's;
5. its keyword escaping is exercised by `28-target-keywords.idl`;
6. a **foreign peer** — not us — is one end of it.

*언어 바인딩은 작성됨으로써가 아니라 **스위트를 통과함으로써** 인정된다. 그리고
그 스위트는 언어마다 복사한 것이 아니라 **언어로 매개변수화된 하나**다.*

## 5. What is proposed / 제안

Ordered so each unblocks the next. **B1 and B2 are prerequisites for any second
language and are worth doing even if no second language is ever added.**

> **Status corrected 2026-08-26, the day this was drafted.** B1 is **done**,
> B2 is **half done**, B4 is **landed**. Each is marked at its own heading
> below; the ordering argument is unchanged and the remaining work is B2's
> enrolment and B3.
>
> *2026-08-26 상태 정정 — B1 완료, B2 절반, B4 착지. 순서 논지는 그대로이고
> 남은 것은 B2의 등록 방식과 B3다.*

### B1 — the service contracts join the gates (`spikes/`, `crates/orbweaver-test`) — **DONE 2026-08-26**

`corpus/services/` enters `differential.sh`'s scope and the corpus gate's
`ENUMERATED`. The `ir-subset.idl` divergence gets recorded with the reason it
already has elsewhere. **What must not happen:** the staleness loop being
loosened to permit unchecked rows — the fix is that the files *are* checked,
not that the check tolerates gaps.

> **Done 2026-08-26** as written, including the clause about what must not
> happen: the files are checked rather than the check tolerating gaps. The
> evidence is §2's first finding above, and the divergence's own row.

### B2 — the sweep enrols by contract, not by filename (`spikes/`) — **HALF DONE 2026-08-26**

`service_sweep.py` reads a directory of first-party contracts instead of a
literal list including a fixture's files. **The payoff is immediate and
independent of any language work:** `CosTrading::Lookup` becomes visible to
`SERVICES-COVERAGE` §8, which is the one thing today's trading batch could not
finish. **The honest part:** where a first-party contract does not exist yet for
a standard service we serve, the sweep must say **which service it is measuring
from a fixture's file**, rather than the fact living in a line number.

> **Half done 2026-08-26.** The payoff and the honest part both landed — the
> trader is in `SERVICES-COVERAGE` §8 and `#SOURCES` names each service's
> origin. **The proposal's own sentence did not:** the inputs are still a
> literal list. What remains of B2 is one change, and it is the one that makes
> the next contract free rather than the one that made this contract visible.
> §2's second finding above carries the measurement.

### B3 — the acceptance suite is parameterised (`spikes/`, `crates/orbweaver-gen`)

One suite, a language argument. Today's Python group becomes its first
instance and **must produce byte-identical results as an instance** — that is
the migration's oracle, the same discipline the emitted stubs are re-blessed
under. Until a second language exists this looks like refactoring; it is the
difference between the next target costing an emitter and costing a project.

### B4 — the servant seam, once (`orbweaver-gen`, the bridge) — **LANDED 2026-08-26**

D030 §5 L1, **already in flight as this is written.** Listed here for the order
rather than the content: whatever protocol that batch settles is the protocol
every later binding inherits, so B3's suite should be able to drive it before a
second language arrives to discover it cannot.

> **Landed 2026-08-26, the same day.** The protocol every later binding
> inherits is now settled and it is a **dispatch, not a wire** — the refusal
> §3 and D030 §4 state, honoured in the implementation rather than restated:
> `crates/orbweaver-gen/src/pyservant.rs`'s module documentation is where the
> shape lives, and the seam's own refusals are the published constructors
> rather than reproduced ids. What it closes and what it leaves is D029 §6.1's
> Language row and §6.1.1. **The order's warning still stands and is now the
> live risk:** B3 does not exist, so this seam has one instance and no
> parameterised suite driving it, which is exactly the state B3 was written to
> prevent a second language from discovering.
>
> *2026-08-26 착지. 이후 모든 바인딩이 물려받을 프로토콜은 와이어가 아니라
> 디스패치로 정해졌다. **다만 순서상의 경고는 이제 실제 위험이다** — B3가 없어
> 이 심에는 인스턴스가 하나뿐이고 그것을 구동하는 매개변수화된 스위트는 없다.*

## 6. What must not happen / 해서는 안 되는 것

- **No second ORB core.** D030 §4, unchanged.
- **No vendored IDL, and no fixture's IDL as a source of truth.** §2's second
  finding is the live instance: our own coverage is derived from omniORB's
  files today. First-party contracts are the fix and B2 is where it lands.
- **No per-language refusal wording.** The generated Python runtime once wrote
  its own fourth wording for `fixed`, *measured by nothing until it was broken
  on purpose*. A second language triples that exposure.
- **No binding accepted on a self-test.** §4's clause 6 is the one that will be
  tempting to skip when a peer is inconvenient, and it is the one that made the
  Python target mean something.

## 7. What this document does not claim / 주장하지 않는 것

It does not claim a second language is due — D030 §4 gives a second ORB core a
trigger and this document does not lower it. It does not claim B1–B3 are
language work: they are gate work that a second language would otherwise pay
for, and **B2's payoff lands today with no language attached** — which it did,
hours later; see B2's status note, and note that the payoff landing is not the
proposal landing. And it does not
claim the three layers in §3 are the complete decomposition; they are the three
that today's measurements distinguish, and §3's table is the thing to re-measure
before adding a fourth.
