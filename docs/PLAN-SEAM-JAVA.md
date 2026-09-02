# The third implementation, and the row that would have shown it missing

**세 번째 구현, 그리고 그것이 빠졌음을 보여주었을 행**

**STANDING: a work plan, not a decision.** The decision it executes is
[`D038`](decisions/D038-a-call-travelling-the-other-way.md), **approved
2026-08-31, option A**. Nothing here proposes a choice the owner has not
already made; where this document would need one, it says so and stops.

*이것은 결정이 아니라 작업 계획서다. 실행하는 결정은 D038(2026-08-31 승인, A안)이다.
소유자가 아직 내리지 않은 선택을 제안하지 않으며, 필요해지는 자리에서는 그렇게 적고
멈춘다.*

> **Priority zero.** The criterion's home is
> [`D029`](decisions/D029-what-a-complete-orb-would-mean.md) §6 and is **not
> restated here**. How this work bears on it is in §9, and the honest answer is
> narrower than "it closes a row".

---

## 0. What this plans, and what it does not / 무엇을 계획하고 무엇을 하지 않는가

`PLAN-FIRST-COMPLETION.md` §1's **L4** is the last item in that section that is
*work* rather than a decision. This document is L4's execution plan. L4 keeps
the statement of what the leak is; this keeps the order the work lands in, the
measurement that would refuse each step, and the risks.

**It does not restate L4, D038 or D029.** Where a fact lives is where it stays.

*§1의 L4는 그 절에 남은, 결정이 아니라 **작업**인 마지막 항목이다. 이 문서는 L4의
실행 계획이다. L4·D038·D029를 다시 적지 않는다 — 사실이 사는 곳은 그대로 둔다.*

---

## 1. The state, measured before planning / 계획 전에 측정한 상태

Measured 2026-09-02, against the tree. Every row here was read, not recalled.

| | what carries the invoke direction |
|---|---|
| **Rust** | `seam.rs` — `resolve_nested` (:301), `nested_refusal` (:350), `ENVELOPE_INVOKE` (:388), `ENVELOPE_ANSWER` (:396), `PROTOCOL_VERSION = "2"` (:374) |
| **Python** | `python_rt.py` — `seam_protocol()` (:272), `ObjectRef.invoke` (:633), `_nested_invoke` (:1955), `_NESTED_CHANNEL` |
| **Java** | **none of it.** `java_rt.java` is 2179 lines; its `ObjectRef` (:617) holds `handle` and `typeId` and has no `invoke`; a grep for the envelope names returns **no line** |

So what remains of §1's largest item is **one implementation**, and the two that
exist are its specification. It is smaller than L4 has been carrying — because
the work already done was done, not because the sizing changed.

**And smaller again than that, checked rather than hoped.** A reference
*arriving* at a Java servant already becomes an `ObjectRef`:
`java_rt.java:1379` builds one from the incoming `_ref`/`_type` pair. So the
**emitter is not in scope** — `java.rs` needs no change for the arrival, and
what is missing is a method on a class that already exists and already holds the
handle. The plan below reflects that; an earlier reading of this section did
not, and would have scheduled emitter work that is already done.

*2026-09-02 측정. 세 구현 중 **둘은 이미 싣고 있고 Java에는 한 줄도 없다.** 남은
것은 구현 하나이며, 이미 있는 둘이 그것의 명세다. 항목이 작아진 이유는 사이징이
바뀌어서가 아니라 이미 한 일이 실제로 되어 있어서다.*

---

## 2. The finding that reorders this plan / 계획의 순서를 바꾼 발견

The obvious first step is `ObjectRef.invoke`. **It is the wrong one.**

`crates/orbweaver-gen/tests/the_seam_is_one_protocol.rs` exists so that *an
agreement between two implementations is a value, not a comment*. Its header
states the cost it was built against — the seam's shape once lived in three
comments and **hand-typed string literals at every site that read a key**, and
`CLAUDE.md` records what that costs — and it states its own extension rule:

> **A binding in a third language adds a function and a row here, and nothing
> else.**

**Its `BINDINGS` array has one row.** Python. The Java servant half landed
2026-09-01 and **was never enrolled**, so:

- `java_rt.java` publishes **no protocol document** — there is no
  `seamProtocol()`, where Python has one at `python_rt.py:272`;
- its serve loop reads the envelope key as a **hardcoded literal**
  (`serveOnPipes` :1985, `((Map<?, ?>) _document).get("call")`), which is
  precisely the defect the test was built to prevent, reintroduced;
- **37 protocol-key literal occurrences** sat in that file. This line first
  said **19**, which was the count for a **narrower key list I had typed** — not
  for the protocol's own keys. Doing S2 replaced the typed list with the keys
  read out of `seam::protocol()` itself, and the number moved. *A figure in
  prose carries the date it was measured*, and it also carries the question it
  answered: the first one answered a worse question;
- and nothing could go red about any of it.

**Java is 8 of 8 on the binding grid and unenrolled here, and both are correct.**
The test's own header draws the line: *`spikes/bindings/*.manifest` is where a
binding's **cells** are enrolled; this is where its **protocol** is, and the two
are different questions.* This is the same shape found elsewhere on 2026-09-02 —
**two gates green, each scoped to a place, and the rule about a claim** — and it
is the reason the first step below is a red test rather than a feature.

*뻔한 첫 단계는 `ObjectRef.invoke`다. **틀린 단계다.** 프로토콜 일치 테스트의
`BINDINGS`에는 행이 **하나**뿐이다 — Python. 2026-09-01에 착지한 Java 서번트 절반은
**등록되지 않았고**, 그래서 Java는 프로토콜 문서를 발행하지 않으며, serve 루프는
봉투 키를 **하드코딩 문자열**로 읽고(그 테스트가 막으려고 만들어진 바로 그 결함),
19개의 프로토콜 키 리터럴이 그 파일에 있으며, 그중 어느 것도 빨개질 수 없었다.
**Java가 그리드에서 8/8이면서 여기 미등록인 것은 둘 다 옳다** — 그리드는 **칸**을,
이 테스트는 **프로토콜**을 묻는 다른 질문이기 때문이다. 그래서 아래 첫 단계는 기능이
아니라 **빨간 테스트**다.*

---

## 3. The work, in order / 작업, 순서대로

Each step names the measurement that would **refuse** it. A step whose
measurement is "it compiles" is not on this list.

### S1 — Java publishes a protocol document, and the row lands red — **DONE 2026-09-02**

Java gains `seamProtocol()`, assembled **from the constants it reads with**, and
`BINDINGS` gains its row.

**S1 is larger than the test's own extension rule says, and this is the review's
sharpest finding.** That rule — *a binding in a third language adds a function
and a row here, and nothing else* — **is not implemented.** `theirs()` hardcodes
`Command::new("python3")` and writes `python::RUNTIME` to `_rt.py` in a temp
directory; the row's second field is a **Python `-c` snippet**. A Java row
cannot be added without first making the runner polymorphic: an interpreter, the
runtime file it needs written, and how it is invoked.

So the aspiration and the code disagree, and the aspiration is the one in the
header where a reader finds it. **S1 first makes the rule true**, then adds the
row — otherwise every later language pays this again and the header goes on
promising something no one has done.

- **Measurement:** the test names the difference between Java's document and
  `seam::protocol()`. **It is expected to be red**, and that is the step's
  product: the gap becomes visible before anything is built for it.
- **Control:** the test already refuses a vacuous pass — its header records that
  an emptied `BINDINGS` or a `protocol()` reduced to `{}` must fail (:148). Run
  it, do not cite it.
- **Second control, new:** after `theirs()` is polymorphic, the **Python row must
  still pass unchanged**. A refactor that broke the one working row while adding
  a second would be caught by nothing else.
- **The absent-JDK question is decided here, not discovered later** — see §5.4.

### S2 — Java's protocol keys stop being literals — **DONE 2026-09-02, with S1**

The 19 occurrences become named constants, and `seamProtocol()` is assembled
from **those**.

- **Why it is separate from S1, and load-bearing:** if the document is written
  by hand, S1's row asserts agreement between a hand-written document and Rust —
  **a second copy with better manners**, which is the defect `CLAUDE.md` names as
  a classifier retyping a sentence some other function owns. What must track is
  *what the code reads with*.
- **Measurement:** change one constant; the test goes red. Restore it; green.
  Both directions, because only the red one is evidence.

**S1 and S2 landed together, and they had to.** They were written as two
steps and they interlock: a document *assembled from the constants the runtime
reads with* cannot be assembled before those constants exist. Splitting them
would have produced a hand-written document at S1 — the very *second copy with
better manners* S2 exists to prevent. One pass: constants, document, and all 37
read sites.

**What S1 found, which is the mechanism working before it had even landed.**
Java's `seamProtocol()` must state what the file *reads*, and writing it that
way exposed something no one was looking for: **Java never reads the call's
`oid`.** Its `Servant` interface has no member for it and `dispatchCall` never
asks, so **a Java servant cannot tell which object of its interface it was
addressed to**, where a Python one can through `own_oid()`. Publishing
`call.object` would have made the document a *description of the protocol*
instead of a *statement of what the file reads* — the one thing it must not be.
So the key is absent from Java's document, the difference is pinned by name with
its reason, and it is recorded as **work rather than a property**. It is outside
this plan's scope; it is no longer invisible.

**And one near-miss worth keeping.** The conversion was nearly done by replacing
every `"id"` in the file. `java_rt.java:910` reads `_m.get("id")` beside
`_m.get("kind")` and `_m.get("digits")` — that is the **type-descriptor**
vocabulary, a different document entirely, and converting it would have been
*a batch scoped to a keyword rather than to the rule*. Each site was classified
before any was changed.

**The pin is exact, not a floor**, and both directions are controlled: a **new**
divergence fails (a Java constant changed → `reply.ok: "ok" vs "okay"`), and a
pinned difference that has **gone away** fails too (Java made to publish
`call.object` → the pinned line no longer matches). A pin nobody has to maintain
is a floor, and a floor here would let a difference rot while reading green.

**Landed with the harness group §5.3 requires**, because the two halves are only
honest together: the cargo test skips without a JDK, and the harness now asks
the **fixture** — not the test's output — and prints a counted `SKIPPED` naming
it. Both branches were exercised.

*S1과 S2는 함께 착지했고, 그래야 했다 — "읽는 상수로부터" 조립하는 문서는 그 상수가
생기기 전에는 조립할 수 없다. S1이 찾은 것: **Java는 호출의 `oid`를 한 번도 읽지
않는다.** Java 서번트는 자기가 어느 객체로 불렸는지 모른다. 그 키를 발행했다면 이
문서는 "파일이 읽는 것에 대한 진술"이 아니라 "프로토콜에 대한 서술"이 되었을 것이다 —
절대 되어서는 안 되는 것. 그래서 빼고, 이름을 붙여 고정하고, **성질이 아니라 작업**으로
기록했다. 그리고 아슬아슬했던 것: `"id"`를 전부 치환할 뻔했는데 910행은 **타입
서술자**의 것이었다 — 규칙이 아니라 키워드에 범위를 맞춘 배치.*

### S3 — a nested read that shares the one reader

**This step was wrong in the first draft of this plan and is corrected here.**
It said the top-level serve loop must become *reply-or-request*. Reading
`python_rt.py`'s `NestedChannel.invoke` shows it must not: the child writes its
`invoke`, then **loops on its own read** until an `answer` arrives, and a `call`
reaching that loop is refused **by name** — *the seam carries one conversation
at a time*. The top-level loop keeps reading calls and only calls.

What Java needs is therefore narrower and structural rather than conceptual:
`serveOnPipes` (:1985) builds its `BufferedReader` and `PrintStream` as
**locals**, so a nested channel has nothing to read from. They are hoisted so
one reader serves both loops.

- **The hazard is two readers over one stream, not threads.** A second
  `BufferedReader` over `System.in` would buffer ahead and consume lines the
  other loop is owed — a defect that appears as a hang or as a document arriving
  at the wrong loop, and that no single-message test would show.
- **Measurement:** a document arriving in the wrong envelope is refused **by
  name**. Java's loop today `continue`s past anything that is not a `call`,
  which would swallow an `answer` silently — the same silence Python refuses in
  words.
- **Control:** a nested call followed by a second ordinary call on the same
  child, so the reader is proven to have been left in a usable state. One
  exchange proves nothing about the stream position.

### S4 — `ObjectRef.invoke`, and the channel that exists only during a dispatch

Mirrors `_NESTED_CHANNEL`: installed for the duration of one dispatch, removed
after.

- **Measurement:** invoking **outside** a dispatch is refused — *a handle is not
  a proxy* — and the control shows the refusal, not only the success. A test that
  only proves the success passes in a world where the refusal was deleted.
- **The refusal text is not retyped.** It comes from the seam's published
  constant. Three implementations now say the same things about the same
  protocol, and `CLAUDE.md`'s rule is that a sentence many layers say is a fact
  with one home — one of those sentences has already gone false once here.
- **What it returns is a boundary, and Java makes it louder than Python did.**
  `python_rt.py`'s `_reply_or_raise` says it out loud: the nested result is
  **AnyJSON and not a mapped value**, because a client knows its callee's
  contract through generated descriptors and a servant invoking a handle it was
  *handed* does not. In Java that is an `Object`/`Map` and **cannot be narrowed
  by a generated stub** — so the plan states it rather than letting a reader
  expect a typed result and find one that is not. This is a property of the
  boundary, not a Java shortfall.
- **A nested call that raises comes back as an answer the servant can catch**,
  which is D038 §3's third invariant and the reason a foreign servant can
  implement a `raises` clause on an operation it calls. Java's `invoke` maps the
  same four branches Python's does — `error`, `system_exception`,
  `user_exception`, `ok` — or the invariant is only half held.

### S5 — the measurement that makes it a Language claim

A Java mirror of `crates/orbweaver-gen/tests/a_call_travelling_the_other_way.rs`:
a Java servant is handed a reference to a **Rust** `Target` on a **real socket**
and invokes it.

- **Two assertions, neither sufficient alone** — the shape that file already
  argues for: the Rust target records the invocation **exactly once** (without
  it, a servant that returned successfully having done nothing passes), and the
  Java servant refuses unless the value it read back is the expected one (so the
  answer travelled back **intact**, not merely that a connection happened).

### S6 — the suite gains a `clause` row, and the acceptance question is flagged

`spikes/bindings/java.manifest` already has the row type this needs:
`clause <name> <command>` — *clauses 3/4/5, no peer and no wire* — which is
where a property of the **emitter and its runtime** lives. A nested invoke is
that shape, not a `(direction × peer)` cell, and the first draft of this plan
said "the grid gains the cell" without saying which kind.

**But whether the acceptance definition gains a clause is not this plan's to
decide.** `spikes/bindings/AXES` is *the ONE home for these names* and it is
built from **D032 §4's six clauses**. A nested invoke is not among them. So:

- this plan lands the row as a named `clause` measured every run, and
- **flags** that calling it part of *what a language binding must do to be one*
  would extend D032 §4 — which D032 owns, and which this document does not
  assert by adding a row.

The distinction matters because the alternative is quiet: a seventh clause that
arrived because somebody added a line is an acceptance standard nobody agreed
to, and D032's whole argument is that a binding is accepted by passing a suite.

*각 단계는 그것을 **거절할** 측정을 함께 적는다. 측정이 "컴파일된다"인 단계는 이
목록에 없다. S1은 **빨간 테스트가 산출물**이며, 검토가 밝힌 대로 그 테스트의 확장
규칙("함수 하나와 행 하나")은 **구현되어 있지 않다** — `theirs()`가 `python3`을
하드코딩하므로 S1은 먼저 그 규칙을 참으로 만든다. S2가 없으면 S1의 행은 손으로 쓴
문서와 Rust의 일치를 주장하는 **매너 좋은 두 번째 사본**이 된다. S3는 초안이
틀렸다: 최상위 루프는 그대로이고, 필요한 것은 **하나의 리더를 공유하는 중첩 읽기**이며
위험은 스레드가 아니라 **한 스트림 위의 두 리더**다. S4의 거절 문장은 다시 적지
않으며, 돌려주는 값이 **AnyJSON이지 매핑된 값이 아니라는 것**은 경계이지 Java의
부족이 아니다. S6은 `(방향 × 피어)` 칸이 아니라 `clause` 행이고, 그것을 합격 정의에
넣을지는 **D032의 몫이지 이 문서의 몫이 아니다**.*

---

## 4. What must not happen / 해서는 안 되는 것

- **No direct dial from the Java side.** The far side never learns an address —
  §4.7, and the reason `invoke` exists rather than a `connect`. A shortcut that
  dialled would **pass every test in S5** while deleting the property the row is
  about.
- **Java must not announce protocol 2 before it serves 2.** A runtime claiming a
  version it does not implement is the *claimed versus observed* distinction the
  binding grid exists to refuse, one layer down. See §5.2 for how the interim is
  held honestly.
- **The grid's 8 of 8 must not be offered as evidence about the protocol.**
  Different questions, and §2 is what happens when they are conflated.
- **No fourth language enrolled to make a count look better.** The row is worth
  something because it is red first.

*저쪽은 주소를 배우지 않는다 — 다이얼하는 지름길은 S5의 모든 테스트를 통과하면서
이 행이 지키려는 성질을 정확히 지운다. 구현하지 않은 버전을 알리지 않는다. 그리드의
8/8을 프로토콜에 대한 증거로 내놓지 않는다.*

---

## 5. The risks, stated as what would make this plan wrong / 이 계획이 틀리는 경우

### 5.1 The risk this plan first named, downgraded on evidence

The first draft said: *if Java's serving half cannot be made re-entrant without
a second thread, S3 becomes a redesign.* **That risk is smaller than stated, and
the correction is worth more than the risk was.**

Two things were conflated. D038 §3's connection rule — *the nested call is made
on a connection the servant owns, never the one the request arrived on* — is a
rule the **parent** holds, because the parent is what dials on the child's
behalf. **The child never dials at all**, which is §4.7 and the reason `invoke`
exists. The child's side is not concurrent: `NestedChannel.invoke` writes, then
blocks on its own read until the `answer` comes, and refuses a `call` arriving
there in words. Python is the existence proof that no second thread is needed.

**What remains is the real risk and it is in S3**: two readers over one stream
(see S3). Lower, but it fails in the way stream bugs fail — as a hang, not as a
wrong answer — so S3 carries a control that runs a second ordinary call after
the nested one.

*첫 초안의 위험은 증거로 낮아졌다. 두 가지가 섞여 있었다 — D038 §3의 연결 규칙은
대신 다이얼하는 **부모**의 규칙이고, **자식은 다이얼하지 않는다.** Python이 두 번째
스레드가 필요 없다는 존재 증명이다. 남은 진짜 위험은 **한 스트림 위의 두 리더**이며,
그것은 틀린 답이 아니라 멈춤으로 실패한다.*

### 5.2 The interim, where Java is at version 1 and Rust at 2

Between S1 and S5, Java's document honestly differs from Rust's. Three ways to
hold that, and only one is honest:

| | |
|---|---|
| land the row at S1 and let the test stay red | a red harness for the length of the work — refused |
| land the row at S5 | the gap stays invisible during exactly the work that is about it — refused, it is §2's defect repeated |
| **land the row at S1 with the difference set pinned and counted** | **taken** |

The pin is **a floor, not a figure** — `CLAUDE.md`'s rule applies directly: it
proves *nothing new appeared*, and proves nothing about the size. It carries its
rationale in the comment, and it comes off at S5 when the set is empty. A pinned
difference that is still there after S5 is a failure, not a smaller number.

### 5.3 The absent JDK, where two doctrines in this repository disagree

`the_seam_is_one_protocol.rs` says an interpreter that will not run is **a
failure, never a skip disguised as a pass** — right for Python, which is always
there. `a_java_servant_this_process_owns.rs` prints `SKIPPED no JDK — set
ORBWEAVER_JAVA_HOME`, which is right for a fixture that may be absent. **S1's
row makes those two meet**, and the meeting has to be decided rather than
discovered when a machine without a JDK first runs `cargo test --workspace`.

Neither doctrine is wrong; they answer different questions. The resolution this
plan proposes, in D010 §2's terms:

- the cargo test **skips** where the JDK is absent, following the Java tests
  already in that crate — making the JDK a hard dependency of
  `cargo test --workspace` would be a larger change than this work, decided as a
  side effect;
- **and the harness carries a counted `SKIPPED` group naming the fixture**, so
  the verdict sees the unmeasured claim. A class-B claim lands as a counted
  `SKIPPED` naming its fixture, never as a `note` and never as `ok`.

Without the second half the first half is exactly the *skip disguised as a pass*
the test's header refuses. They are only compatible together.

*JDK 부재에서 이 저장소의 두 원칙이 만난다 — "돌지 않는 인터프리터는 실패지 건너뜀이
아니다"와 "부재 픽스처는 세어지는 SKIPPED"다. 둘 다 옳고 다른 질문에 답한다. 제안:
cargo 테스트는 건너뛰되 **하네스가 픽스처를 이름 붙인 세어지는 `SKIPPED` 그룹을
싣는다.** 뒤 절반이 없으면 앞 절반은 바로 그 "통과로 위장한 건너뜀"이다.*

### 5.4 `protocol()` may contain something Java cannot compute

If the document holds anything Rust-specific, equality is the wrong assertion
shape and the test would need a per-binding subset rule — which **weakens it**,
so it must be argued in writing and not adopted quietly to get to green. Checked
at S1, before S2 spends effort on constants.

*5.1 재진입이 규칙이 아니라 재설계일 수 있다 — S3에서 확인되며, 발화하면 계획을
멈추고 질문을 결정으로 되돌린다(배치 안에서 풀 일이 아니다). 5.2 중간 구간은 **차이
집합을 고정하고 세는 것**으로 잡는다 — 그 고정은 **수치가 아니라 하한**이고, S5에서
집합이 비면 떼어낸다. 5.3 문서에 Java가 계산할 수 없는 것이 있으면 동등성은 틀린
모양이며, 조용히 약화시키지 말고 글로 논증한다.*

---

## 6. The acceptance sentence / 합격 문장

D015's method: one sentence, and the work is done when it is true and measured.

> **A servant written in Java, handed a reference it did not create, invokes it
> and uses the answer — and a caller cannot tell from what that servant can *do*
> with the reference whether it was written in Java, Python or Rust.**

Measured by S5, held every run by S6, and **not** claimed by S1–S4 alone: those
are the protocol being agreed and the method existing, which is not the same as
a servant having used one.

*합격 문장 하나. S5가 재고 S6가 매 실행 지킨다. S1–S4만으로는 주장하지 않는다 —
그것은 프로토콜이 합의되고 수단이 생긴 것이지, 서번트가 실제로 쓴 것이 아니다.*

## 7. The cost, stated rather than implied / 비용

- **S1 is the largest step and it is not the feature.** Making `theirs()`
  polymorphic touches the one test every binding's protocol claim rests on, so
  it is done first, alone, with the Python row proven still green.
- **S2 is 19 sites in one pass**, scoped to the rule and not the keyword.
- **S3–S4 are one file** (`java_rt.java`), and the emitter is untouched.
- **S5 needs a JDK**, and §5.3 says what happens where there is none.
- Harness time: one `clause` row, which runs `javac` — the same cost the Java
  cells already pay, not a new fixture.

---

## 8. What this deliberately excludes / 의도적으로 제외한 것

- **A fourth language.** D030 L3's developer tools and any C binding of the
  servant seam are out of scope.
- **The handle table's shape** beyond what D038 already approved.
- **L3's step 3** — the deployment shape — which was downgraded on measurement
  because priority zero ranks a leak above a capability, and remains so.
- **The binding grid's unread GIOP versions** (client 1.0 in both languages,
  python client 1.1, python servant 1.0). Coverage, not transparency; it is
  ranked under this work in `PLAN-FIRST-COMPLETION` §1.9's order and stays there.

---

## 9. What this bears on, said narrowly / 이 작업이 걸리는 곳, 좁게

D029 §6.1's **Language** row. And the honest statement is narrower than *it
closes a row*:

- the Language row already reads **`held`**, because its leak leg measures a
  language swapped under a live caller;
- what this closes is the **invoke direction**, which L4 names as the last thing
  left under that row;
- **it is not criterion-moving in the ledger's sense.** D039 remains the only
  item that would move a row's standing, and it awaits the owner.

Saying otherwise would be filing a capability as a leak closure, which is the
inverse of the error D038 refused when it declined to record this as a floor.

*D029 §6.1의 **언어** 행. 정직한 진술은 "행을 닫는다"보다 좁다 — 그 행은 이미
`held`이고, 이 작업이 닫는 것은 L4가 그 행 아래 마지막으로 남겨둔 **호출 방향**이며,
**원장 의미에서 기준을 움직이는 항목은 아니다**. 그것은 여전히 D039뿐이고 소유자를
기다린다.*

---

## 10. How it lands / 착지 방식

The operating model applies unchanged: **batch → oracle → repair → codify.**

- **Batch** — S2's 19 literals are one pass, not nineteen. Scoped to the rule
  (*a protocol key is read through a named constant*), never to the keyword.
- **Oracle** — the one-protocol test, the harness, and the grid cell from S6.
- **Repair** — one fix per root cause across every affected site.
- **Codify** — S1's row is itself the codification: after it, a fourth language
  that skips the protocol cannot be silent. What is *additionally* owed is the
  rule that put Java in this position — **a binding is enrolled in the protocol
  test in the same commit that gives it a runtime**, which belongs in the test's
  header where the extension rule already lives.

Each step lands with its negative control in the commit message (D010 §7.2), and
the harness is green before any of it is pushed.

*운영 모델 그대로: **일괄 → 오라클 → 수리 → 성문화.** S2의 19개는 한 번의 패스이며,
범위는 키워드가 아니라 규칙에 맞춘다. 성문화는 S1의 행 자체다 — 그 뒤로는 프로토콜을
건너뛴 네 번째 언어가 조용할 수 없다. 추가로 갚아야 할 것은 Java를 이 자리에 놓은
규칙이다: **바인딩은 런타임을 얻는 바로 그 커밋에서 프로토콜 테스트에 등록된다.***
