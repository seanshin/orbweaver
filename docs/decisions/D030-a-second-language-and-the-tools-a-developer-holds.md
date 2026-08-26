# D030 — A second language, and the tools a developer actually holds

**STATUS: PROPOSED** — drafted 2026-08-26 on a direction that per-language ORB
implementations and development-support tooling are needed. Every figure was
measured that day. Not self-approvable: §3 proposes a rule about what a
language target must do before it is called one, and §5 commits the project to
a maintenance surface it does not have today.

**상태: 제안** — 2026-08-26, 언어별 ORB 구현과 개발 지원 도구가 필요하다는 지시에서
작성.


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

> **How this bears on it.** L1 is not one proposal among four: language
> transparency **leaks by construction** while Python is clients only, because a
> target's language decides whether it can be a target at all. Under D029 §6 that
> makes the servant seam a completion item, and L2/L3/L4 hygiene.

---

## 1. What exists, measured / 오늘 있는 것

**Emitters.** `orbweaver-gen` emits three things, not two: Rust **client
stubs**, Rust **server skeletons**, and Python **clients only**.

```
lib.rs       2005 lines   the Rust client half and the type mapping
skeleton.rs  1300         the Rust server half
python.rs    1059         the Python client half
rt.rs        1043         the Rust runtime a generated stub needs
```

**The Python limit is recorded and is the whole subject of this document.**
`COMPONENTS.md` states it plainly: *"Python is clients only — a Python servant
needs the bridge to call back into Python, **a second protocol direction**."*

**Tooling.** The workspace ships **twenty-plus developer-facing binaries** —
`sidl-validate`, `idl-diff`, `gen-python`, `contract-check`, `repository-ids`,
`forge-pipeline`, `orbweaver-console`, `orbweaver-mcp-server`,
`orbweaver-py-bridge`, and the rest. Measured the same day: **zero** files in
the IDL front end or the forge mention an LSP, a language server, syntax
highlighting, or an editor integration.

*방출기는 셋이다: Rust 클라이언트, Rust 스켈레톤, **Python 클라이언트만**. 개발자용
바이너리는 스무 개가 넘고, 에디터 쪽 도구는 **영**이다.*

## 2. The two directions are not one job / 두 방향은 한 가지 일이 아니다

The single most important measured fact here: **a client and a servant are not
the same amount of work in a second language.**

A generated **client** marshals a request, writes it, reads a reply, and hands
back a value. Everything it needs is a function it calls. A generated
**servant** must be *called by* the ORB — the dispatch has to arrive in the
target language, which means either the ORB runs in that language or something
carries the call across a process boundary. That is what `COMPONENTS.md` means
by *a second protocol direction*, and it is why the Python target stopped where
it did rather than by anyone's preference.

So "a per-language ORB implementation" splits into three quite different
propositions, and conflating them is the failure mode:

| | what it is | cost |
|---|---|---|
| **A** | a generated **client** in language L, speaking to our ORB | an emitter + a runtime (Rust's is 1043 lines) |
| **B** | a generated **servant** in language L, dispatched by our Rust ORB across a seam | a seam with a protocol, plus B's half of it in L |
| **C** | an **ORB** written in language L | the whole wire again — GIOP, IIOP, CDR, IOR, POA |

**C is the one the phrase "per-language ORB" most naturally means and the one
this document argues hardest against.** Not because it is large, but because
the project's licence position makes it expensive in a specific way: an ORB
core is *logic defined by a published specification we implement ourselves*,
and a second one is a second thing to keep in agreement with the first, over a
wire where the whole discipline is that agreement is measured against a foreign
peer rather than assumed.

## 3. The rule this proposes / 제안하는 규칙

**A language is a target when its generated code is measured against a peer
that is not us, in both byte orders, and its refusals say the same sentences
ours do. Anything short of that is an emitter, and is called one.**

This is not a formality. The Python target earned the name by exactly this
route and the route found real defects: the cross-implementation sweep crosses
182 values and 139 calls with zero divergences, and it caught the **Rust**
emitter's keyword list missing `yield` — *"no emitter's escaping had ever been
executed."* And the generated Python runtime *"had written its own fourth
wording for `fixed`, measured by nothing until it was broken on purpose."*

**A second language multiplies the sentences that have to agree**, and this
project has already measured what happens when they drift: five wire-refusal
families, whose heads are `pub` in `orbweaver-dynamic` and read by five Rust
layers *and the generated Python runtime*, because twelve literals in two
crates had written them again and one had gone false for three days.

*언어가 대상이 되는 것은, 생성된 코드가 **우리가 아닌 피어**에 대해 양쪽 바이트
순서로 측정되고 그 거부가 우리와 **같은 문장**을 말할 때다. 그에 못 미치면 방출기이며
그렇게 부른다.*

### 3.1 Where Python stands against the rule, clause by clause (2026-08-26)

The rule has three clauses, and a target that meets two of them is an emitter.
Stated as clauses so that "Python is a target" is never asserted as one word.

| Clause | Servant direction | Client direction |
|---|---|---|
| measured against **a peer that is not us** | **met** — omniORB (`omniorb_calls_a_python_servant`) and JacORB (`jacorb_calls_a_python_servant`), both calling a Python servant behind our ORB | **met** — the live leg recorded in `docs/pipeline-runs/2026-08-14-python-target.md` |
| **in both byte orders** | **met, 2026-08-26** — little-endian by omniORB, big-endian by JacORB, and in each case the order is read out of §15.4.1's flag byte on the peer's own request rather than assumed from its language | **not established by that batch and not by this one.** `python_target.rs` walks both orders, but through `_rt.Loopback` with no peer in it; the live leg's peer is omniORB, which writes its native order. What is missing is the same shape as what JacORB just closed, one direction over |
| **refusals say the same sentences ours do** | **met** — the five wire-refusal heads are `pub` in `orbweaver-dynamic` and the generated Python runtime reads them, with a gate that computes the expected text by calling the same function | same |

**What the second order actually bought.** Not reassurance: the two servants'
replies are compared as **bytes**, so this is the first measurement in which a
padding byte or an alignment origin that differed between the Python seam and
the Rust skeleton could have shown up under a peer that chose big-endian. It
did not — 11 of 11 replies identical at IIOP 1.2 and 1.1 — and D029 §6.1.1
records that the list of remaining language differences **did not grow**, along
with the three things that leg still does not measure.

*규칙에는 절이 셋이고, 둘만 충족한 대상은 방출기다. 서번트 방향은 세 절 모두
충족한다 — 바이트 순서는 피어의 언어가 아니라 피어가 쓴 플래그 바이트에서 읽는다.
**클라이언트 방향의 "양쪽 순서"는 아직 성립하지 않았다**: `python_target.rs`는 양쪽을
돌지만 피어 없이 루프백이고, 살아 있는 다리의 피어는 자기 고유 순서로 쓰는
omniORB다. 두 번째 순서가 사 준 것은 안심이 아니라 **바이트 비교**다 — 심과 스켈레톤
사이의 패딩·정렬 차이가 드러날 수 있었던 첫 측정이며, 드러나지 않았다(1.2·1.1에서
11/11 동일). 남은 차이 목록이 늘지 않았다는 것과 그 다리가 여전히 재지 못하는 세
가지는 D029 §6.1.1에 있다.*

## 4. What must not happen / 해서는 안 되는 것

- **No second ORB core until a consumer names one.** Proposition C above. The
  trigger would be *a deployment that cannot run a Rust process*, and nothing
  in this project has ever been that.
- **No vendored IDL.** A Java target needs `CosNaming`, `CosTrading` and the
  rest as contracts; JacORB and omniORB ship them and **they must not be
  copied**. First-party contracts written from the OMG specification are the
  prerequisite — the trading batch hit exactly this wall today and correctly
  stopped at it.
- **No emitter without its runtime's sentences.** A target whose refusals are
  its own wording is a target that will tell a peer something false, and this
  project has measured that happening twice.
- **No tool that is a second front end.** An editor integration that
  re-implements parsing is `sidl-validate`'s facts with a second home. The
  front end already produces positioned diagnostics with fix hints; a tool
  *renders* them.

## 5. What is proposed / 제안

Ordered so that each earns the next.

### L1 — the Python servant, which is the seam question and not a language question

Finish the direction Python stopped at: a Python **servant** our Rust ORB
dispatches into. `orbweaver-py-bridge` exists and D007 settled its shape — a
local process, AnyJSON v1 over it, no new dependency, `cargo tree` unchanged,
and **the bridge is deliberately not a security boundary**.

**Why this before any new language.** The seam it requires is the same seam
every non-Rust servant needs. Build it once for the language whose client half
is already measured, and B-shaped targets become an emitter each; skip it, and
every new language pays for the seam again.

**Oracle.** omniORB's client calling a **Python servant behind our ORB**, both
byte orders — the mirror of the direction already measured, and the same peer.

### L2 — a Java client, because the oracle is already in the tree

JacORB is already a fixture: `spikes/jacorb/setup.sh`, the differential's
second front end, and the GIOP 1.1/1.2 wide-text measurements. **A Java client
target is the one whose independent peer already exists**, which is the
property that made Python the right second target and not an arbitrary one.

Scope: clients only, deliberately, until L1 proves the servant seam.

**What it will find, predicted so the prediction can be wrong:** Java's
reserved words are not Rust's or Python's, and `corpus/golden/28-target-keywords.idl`
exists precisely because no emitter's escaping had been executed until it did.
Adding a target means adding its keyword list to that file's coverage —
CLAUDE.md says so already.

**The prediction was too narrow, and the counter-example arrived before the
target did (2026-08-26).** Building the JacORB fixture for §3.1's big-endian
measurement meant running `org.jacorb.idl.parser` 3.9 over
`corpus/golden/24-skeleton-surface.idl`, and the Java it emitted **did not
compile**: `_GaugeStub.java` declares `catch (java.io.IOException e)` inside
every operation's method body, in the same scope as that operation's own
parameters, while every other local it writes is `_`-prefixed. So an IDL
parameter named `e` is fatal — and 24 has one on purpose, because *"`e` is what
a hand-written encoder would have called its encoder."* Two errors, nothing in
the package builds.

Three things follow, and the third is the one that changes L2's scope.

1. **The hazard is not "reserved words", it is *every* identifier the emitter's
   own template puts in scope** — a caught exception, a loop variable, a
   temporary. Java's keyword list would not have caught this, because `e` is
   not a Java keyword.
2. **A production ORB's emitter has the defect**, which is worth knowing before
   we claim our own does not: the fixture's IDL copy renames the parameter, and
   a parameter name is not on the wire, so the workaround costs the measurement
   nothing.
3. **`28-target-keywords.idl` should grow a section for template-locals, not
   just keywords**, when the Java emitter is written — and the honest way to
   populate it is to read what our own templates put in scope, since that is
   the list only we can know.

*예측이 너무 좁았고, 반례가 대상보다 먼저 왔다(2026-08-26). §3.1의 빅엔디언 측정을
위해 JacORB 픽스처를 세우며 `org.jacorb.idl.parser` 3.9을 24번 계약에 돌렸더니
**컴파일되지 않는 Java**가 나왔다: 생성된 스텁이 연산의 매개변수와 같은 스코프에
`catch (java.io.IOException e)`를 두는데, 다른 지역 변수는 모두 `_` 접두사를 붙인다.
그래서 `e`라는 이름의 IDL 매개변수는 치명적이며, 24번은 바로 그 이름을 일부러 갖고
있다. 따라오는 것 셋: (1) 위험은 "예약어"가 아니라 **방출기 자신의 템플릿이 스코프에
넣는 모든 식별자**다 — `e`는 Java 예약어가 아니다, (2) 실제 운영 ORB의 방출기가 이
결함을 갖고 있다(픽스처는 매개변수 이름을 바꿔 쓰며, 매개변수 이름은 와이어에 없다),
(3) `28-target-keywords.idl`은 예약어만이 아니라 **템플릿 지역 변수** 절을 가져야
하고, 그 목록은 우리 템플릿을 읽어야만 알 수 있다.*

> **L2's client half landed 2026-08-26, and the suite is what says so.** This
> document's status is unchanged — it is still PROPOSED, and a batch does not
> approve the decision it works under — but a proposal that has been executed
> should say what came back. `spikes/binding_suite.sh --language java` reports
> cells run 3, skipped 3, red 0; the **client direction meets §3's three clauses
> in full**, which Python's client direction does not: `client × little` read
> from omniORB and `client × big` read from JacORB, each read off §15.4.1's flag
> byte rather than inferred from the peer's host, and the refusal sentences
> equal to the published heads by a test that computes them from the same
> function.
>
> Two of the three consequences above are now measured rather than predicted.
> (1) The hazard is indeed every identifier the template puts in scope, and the
> emitter answers it by construction — every local it binds begins with `_`,
> which no IDL identifier can — but writing it found a **second form**: an
> escaped member name *is* `_` plus the IDL name, so one prefix is not enough
> and the constructor's parameter needed a different one. (3)
> `28-target-keywords.idl` has its template-locals section, and Java's coverage
> went from 17 of 59 executed by accident to 38 of 59 with the residue named.
>
> The scope line above — *clients only, until L1 proves the servant seam* — held
> and is why all three servant cells are a counted SKIPPED rather than a gap
> nobody wrote down. §3.1's table is **not** edited by this: it records where
> **Python** stands, and Java closing a cell for itself does not move Python's
> client column.
>
> *L2의 클라이언트 절반이 2026-08-26에 착지했다. 이 문서의 상태는 그대로다 — 배치는
> 자기가 따르는 결정을 승인하지 않는다 — 그러나 실행된 제안은 무엇이 돌아왔는지를
> 말해야 한다. Java의 **클라이언트 방향은 §3의 세 절을 모두 충족한다**: 양쪽 순서를
> 피어의 호스트가 아니라 플래그 바이트에서 읽었다. 위 세 귀결 중 둘이 이제 예측이
> 아니라 측정이며, (1)에는 **두 번째 형태**가 있었다 — 이스케이프된 멤버 이름 자체가
> `_` + IDL 이름이므로 접두사 하나로는 부족하다. §3.1의 표는 **Python**이 어디에
> 서 있는지를 적은 것이므로 이 착지로 수정하지 않는다.*

### L3 — the developer tools that are missing, and they are not an IDE

Measured: twenty-plus binaries, zero editor integration. But the gap that costs
a developer most is not syntax highlighting — it is that **every one of those
binaries is a separate invocation with its own arguments**, and a newcomer has
to know which of twenty to reach for.

Two items, in order:

1. **One entry point.** `orbweaver <verb>` over the binaries that already
   exist — `validate`, `diff`, `gen`, `catalog`, `serve`. No new capability;
   a discoverable surface over capabilities that are already measured. This
   is D027's argument (a public entry point ships with a compiled example) at
   the command line rather than in the API.
2. **A language server, and only after (1).** Its whole job is to *render*
   `Report::to_json` — positions, rules, fix hints — which the front end
   already produces. §4's constraint is the design: it must not parse. If it
   cannot be written without a second parser, that is a finding and it stops.

### L4 — first-party contracts for the standard services (`corpus/services/`)

The prerequisite L2 and every future target will hit, and it is already
blocking: today's trading batch could not add `CosTrading::Lookup` to
`SERVICES-COVERAGE` §8 because the sweep parses IDL from our own corpus, **no
first-party `CosTrading` contract exists, and omniORB's must not be vendored.**
`CosNaming` and `CosEvent` will be the same the moment a Java client wants
them.

Each contract owes `differential.sh --require omniidl,jacorb_idl --record`,
which is now enforced by `cargo test --workspace`.

## 6. What this document does not claim / 주장하지 않는 것

It does not claim a second ORB core is wrong in principle — §4 gives it a
trigger, which is what this project does with things it is not building. It
does not claim Java is the right third target for any reason other than that
its peer is already installed and measured, which is a weaker and more honest
reason than fit. It does not claim the tooling gap is an IDE-shaped hole: §5
L3 argues the expensive part is twenty entry points rather than zero editors,
and that claim is testable by asking a newcomer, which nobody has done. And it
does not claim any of this outranks the ORB's own completion (D029) or the four
TypeCode agreement failures the harness reported today — a second language that
speaks a wire we have a live regression in would multiply the regression.
