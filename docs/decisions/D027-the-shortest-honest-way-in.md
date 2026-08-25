# D027 — The shortest honest way in: making the ORB easy to use without making it lie

**STATUS: APPROVED 2026-08-26** — drafted that day on a request to plan how the
ORB becomes easier to use; every figure below was measured the same day, and
the owner approved it with the phrase *승인, 진행*. It was not self-approvable:
§4 constrains what every future public API may do, and §3 refuses a shape most
ORBs ship.

**상태: 승인 2026-08-26** — ORB를 더 쉽게 쓰는 방법을 기획하라는 요청에서 작성,
같은 날 소유자가 *승인, 진행* 으로 승인.

> **What the approval covers, and the order it lands in.** §4's rule and §5's
> four batches. **E4 alone can start immediately** — it touches `spikes/` only.
> **E1, E2 and E3 all write against step 4's one-way API and therefore wait for
> it**, which is not a scheduling preference: D019 step 4 is rewriting
> `orbweaver-giop` and `orbweaver-object` as this is approved, and an example
> or an error message written against the surface it is replacing would be
> obsolete before it landed. §2 is the reason the whole document is timed this
> way and the reason waiting is the cheap option.
>
> *승인 범위는 §4의 규칙과 §5의 네 배치다. **E4만 즉시 시작**한다. E1·E2·E3은
> 전부 4단계의 일방향 API를 상대로 쓰이므로 그것을 기다린다 — 일정 선호가 아니라,
> 지금 교체되고 있는 표면에 대고 쓴 예제는 착지 전에 낡기 때문이다.*

---

## 1. What a newcomer meets today, measured / 오늘 신규 사용자가 만나는 것

| | measured 2026-08-26 |
|---|---|
| `orbweaver-giop` public surface | **87 public types, 324 public functions** |
| `orbweaver-object` | 36 types, 108 functions |
| Doctests in `orbweaver-giop` | **zero** |
| Doctests in the whole workspace | ~14 (`forge` 1, `gen` 2, `idl` 8, `object` 3) |
| `examples/` directories | **none, in any crate** |
| Smallest binary that serves an object | `spike_server.rs`, **483 lines** |
| README | 412 lines; its code blocks are `console` and `idl` |

The crate a user of the ORB touches first has **324 public functions and not one
compiled example.** The README tells a reader what the project *is* — accurately
and at length — and never shows a program.

`orbweaver-giop`'s own module doc is worth quoting because it is *good* and
still not usable: *"Implements GIOP 1.0, 1.1 and 1.2 on both sides:
request/reply and locate/locate-reply, fragmentation in both directions,
codeset negotiation, multi-profile failover at connect time and the serving
half."* Every word is true and load-bearing. None of it tells you what to type.

*ORB 사용자가 처음 만나는 크레이트에 **공개 함수 324개가 있고 컴파일되는 예제는
하나도 없다.** 모듈 문서는 훌륭하고, 무엇을 타이핑해야 하는지는 말하지 않는다.*

## 2. Why now, and not later / 왜 지금인가

**D019 step 4 is landing as this is written**, and it makes the API one-way:
`Server::bind` and `Poa::new` stop being the public way in, and `Orb` becomes
the only route to a transport and a root POA. Thirteen call sites migrate.

That is the moment. **There will be exactly one entry path, so making it a good
one costs one batch now and thirteen rewrites later.** A second chance at a
first API arrives once.

## 3. What "easier" must not mean here / 쉬워진다는 것이 뜻하면 안 되는 것

This is the section that keeps the rest honest, because "make it easier" is the
request under which every discipline this project has gets quietly traded away.

- **Not a faithful `ORB_init`.** D019 §5 refuses it by name, and D019 is now
  approved with that refusal intact. There is no OMG Rust mapping; copying C++
  spelling would import a shape nobody here argued for, and *"the standard
  tells us which facts belong to an ORB even though it cannot tell us their
  Rust names."* Ease is not familiarity to a C++ programmer.
- **Not a second path.** A "simple API" beside the real one is the
  hand-construction problem returning under a friendlier name: two ways in that
  agree until they do not. Step 4 exists to remove exactly that.
- **Not fewer refusals.** This project's refusals name a position and a fix,
  and `unwrap`-friendly convenience wrappers are how that gets lost. A `Result`
  a newcomer must handle is not friction to remove; it is the thing that told
  twelve MCP callers what would make the call succeed.
- **Not hiding the wire.** *"Where something is absent this code fails loudly
  rather than misparsing"* is the crate's own promise and the reason it is
  trusted against two peers at three GIOP versions.

**What is left after those four refusals is the actual proposal**: the same
capability, reached in fewer steps, with the first step discoverable and every
error saying what to do next.

*"쉽게"라는 요청 아래 이 프로젝트의 규율이 조용히 거래된다. 남는 것은 **같은
능력을 더 적은 걸음으로, 첫 걸음은 발견 가능하게, 모든 오류는 다음 걸음을 말하게**
하는 것뿐이다.*

## 4. The rule this proposes / 제안하는 규칙

**A public entry point ships with a compiled example, or it is not an entry
point.**

Not prose — a **doctest**, because a doctest is the only documentation this
project's own "where a fact lives" rule cannot object to: it does not *restate*
the API, it *is* a caller of it, and it fails to compile the day the API moves.
Every other form of usage documentation is a restated fact and drifts silently,
which is the defect this file has measured four times in four different shapes.

The corollary, and it is the limit: **not every public function is an entry
point.** 324 doctests would be a worse document than none. An entry point is
where a task *starts* — construct an ORB, serve an object, make a call, resolve
a name, read a refusal — and the proposal is to name that set explicitly rather
than let it be "whatever is `pub`".

*공개 진입점은 **컴파일되는 예제**와 함께 배포된다. 산문이 아니라 doctest인
이유는, 그것이 API를 **다시 적지 않고 그것의 호출자이기 때문**이다 — API가
움직인 날 컴파일에 실패한다. 다만 모든 `pub`이 진입점은 아니다.*

## 5. What is proposed / 제안

Four batches. E1 waits on step 4; the others do not.

### E1 — the shortest program, and it compiles (`orbweaver-giop`, `orbweaver-object`)

Two doctests at the top of the crates a user meets first: **the smallest program
that serves an object**, and **the smallest that calls one**. Written against
step 4's one-way API, so they cannot be written before it lands and cannot rot
after it.

**The deliverable is a number as much as a text.** Today the smallest serving
binary in the tree is 483 lines. Whatever the doctest turns out to be — twelve
lines, thirty — that number is the measurement of how easy the ORB is, and it
is the first time the project would have one. Report it, and report what the
`use` list has to contain, because an example needing eleven imports has told
you something the line count hides.

**Oracle.** `cargo test --doc` runs them, so they are gates and not decoration.
The negative control is the one this class needs: change the API and watch the
example fail to compile.

### E2 — the ORB's own refusals teach (`orbweaver-giop`, `orbweaver-object`)

`orbweaver-mcp` landed *the refusal that teaches* on 2026-08-25: twelve guard
refusals each gained **what would make the call succeed**, from what the chain
already knew, held by `REMEDY_ACTORS`/`REMEDY_FORBIDDEN` and an exhaustive match
with no `_ =>` arm — *"the compiler asks the next variant's author for a next
step."*

**That discipline has never been applied to the ORB's own Rust API.** The
newcomer's first experience is one of these errors, and the agent boundary — a
machine — is better served today than the human is. Sweep the error types a
first-time caller can actually reach (`InvalidName`, `StringToObjectError`,
`ConfigError`, the connect and dispatch failures), and for each ask the P2
question: *what did this already know that would tell the caller what to do?*

`InvalidName` is the worked example already in the tree: three variants each
carrying the ObjectId, and §8.5.2's three states kept apart because *"a missing
registration and a typo are the same problem"* is exactly the sentence a
collapsed error would tell somebody.

### E3 — one runnable example per served service (`examples/`)

There is no `examples/` directory anywhere. `cargo run --example serve-naming`
should exist for each thing this project actually serves — naming, event,
lifecycle, the trader when T4 lands, the IFR facade.

**These are not spikes renamed.** A spike measures a claim and prints evidence;
an example is read by a person deciding whether to use this. Different jobs,
and the thirteen 200-to-700-line spikes are the measurement that the second job
has no artifact.

### E4 — the newcomer's path is measured, not asserted (`spikes/`)

A script that answers, from the tree: how many public items a caller must name
to serve an object and to make a call, and how many lines the shortest path
takes. Not a gate — a report, in the shape of `gap_symbols.py` and
`plan_numbers.py`, which this project already trusts more than prose.

**Because "easier" without a number is the one claim this project would let
through unmeasured**, and §1's table is its own argument for why: 324 public
functions and zero examples was not a decision anybody made. It accumulated,
and nothing counted it until today.

## 6. What is deliberately not here / 일부러 넣지 않은 것

- **`orbctl`** is D024 §4 and it is the *operator's* ease, not the
  programmer's. Different surface, different document, already planned.
- **The four IDL tools** are D024 §5 and they are the *agent's* ease. In
  flight as this is written.
- **A tutorial or a guide.** Prose that walks a reader through is exactly the
  restated-fact artifact §4 rejects, and it should follow the doctests rather
  than substitute for them: once E1 exists, a guide can quote code that
  compiles instead of code somebody typed into a document.

## 7. What this document does not claim / 주장하지 않는 것

It does not claim the public surface is too large — 87 types across GIOP, IIOP,
IOR, TypeCode, POA, naming and events may be the right number, and this
document counts it rather than judging it. It does not claim doctests would
have prevented any defect this project has actually had; they are proposed as
*documentation that cannot drift*, which is a different and smaller claim than
*documentation that finds bugs*. And it does not claim E1's line count will be
small: the honest outcome may be that serving an object over IIOP takes thirty
lines and always will, in which case the deliverable is thirty lines that
compile and a number nobody has to guess at.
