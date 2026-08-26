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
