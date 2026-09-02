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
> restated here**. How this work bears on it is in §7, and the honest answer is
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
- **19 protocol-key literal occurrences** sit in that file, counted 2026-09-02
  and broken out rather than asserted as a total: `id` ×7, `returns` ×3, `op` ×2,
  `minor` ×2, `completed` ×2, `args` ×2, `call` ×1. Recompute it rather than
  quote it — this is a figure, and a figure in prose carries the date it was
  measured;
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

### S1 — Java publishes a protocol document, and the row lands red

Java gains `seamProtocol()`, assembled **from the constants it reads with**, and
`BINDINGS` gains its row.

- **Measurement:** the test names the difference between Java's document and
  `seam::protocol()`. **It is expected to be red**, and that is the step's
  product: the gap becomes visible before anything is built for it.
- **Control:** the test already refuses a vacuous pass — its header records that
  an emptied `BINDINGS` or a `protocol()` reduced to `{}` must fail (:148). Run
  it, do not cite it.

### S2 — Java's protocol keys stop being literals

The 19 occurrences become named constants, and `seamProtocol()` is assembled
from **those**.

- **Why it is separate from S1, and load-bearing:** if the document is written
  by hand, S1's row asserts agreement between a hand-written document and Rust —
  **a second copy with better manners**, which is the defect `CLAUDE.md` names as
  a classifier retyping a sentence some other function owns. What must track is
  *what the code reads with*.
- **Measurement:** change one constant; the test goes red. Restore it; green.
  Both directions, because only the red one is evidence.

### S3 — the serve loop becomes *reply-or-request*

`serveOnPipes` reads a document and expects a call. Under D038 the next document
may be the **answer** to a nested request this servant itself sent. The loop's
question changes from *read the reply* to *read the next document, which may be
a reply or may be a request* — the sentence `seam.rs`'s `ENVELOPE_INVOKE` doc
already uses.

- **Measurement:** a document arriving in the wrong envelope is refused **by
  name**, not ignored. Java's loop today `continue`s past anything that is not a
  `call`, which would swallow an `answer` silently.
- **Risk gate:** see §5.1. This is the step where the plan can be wrong.

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

### S5 — the measurement that makes it a Language claim

A Java mirror of `crates/orbweaver-gen/tests/a_call_travelling_the_other_way.rs`:
a Java servant is handed a reference to a **Rust** `Target` on a **real socket**
and invokes it.

- **Two assertions, neither sufficient alone** — the shape that file already
  argues for: the Rust target records the invocation **exactly once** (without
  it, a servant that returned successfully having done nothing passes), and the
  Java servant refuses unless the value it read back is the expected one (so the
  answer travelled back **intact**, not merely that a connection happened).

### S6 — the grid gains the cell

So this is measured by the suite **every run** rather than by a test written
once.

*각 단계는 그것을 **거절할** 측정을 함께 적는다. 측정이 "컴파일된다"인 단계는 이
목록에 없다. S1은 **빨간 테스트가 산출물**이다 — 짓기 전에 구멍을 보이게 만든다.
S2가 없으면 S1의 행은 손으로 쓴 문서와 Rust의 일치를 주장하는 **매너 좋은 두 번째
사본**이 된다. S4의 거절 문장은 다시 적지 않는다.*

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

### 5.1 Re-entrancy may be a redesign rather than a rule

D038 §3 fixes the rule the deadlock forces — *the nested call is made on a
connection the servant owns, never the one the request arrived on*. **If Java's
serving half cannot be made re-entrant without a second thread**, S3 stops being
a rule and becomes a redesign, and D038's three invariants must be re-read before
any code is written under S4.

This is checkable **at S3**, which is why S3 is not merged into S4. If it fires,
the plan stops and the question goes back to the decision — it is not something
to solve inside a batch.

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

### 5.3 `protocol()` may contain something Java cannot compute

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

## 6. What this deliberately excludes / 의도적으로 제외한 것

- **A fourth language.** D030 L3's developer tools and any C binding of the
  servant seam are out of scope.
- **The handle table's shape** beyond what D038 already approved.
- **L3's step 3** — the deployment shape — which was downgraded on measurement
  because priority zero ranks a leak above a capability, and remains so.
- **The binding grid's unread GIOP versions** (client 1.0 in both languages,
  python client 1.1, python servant 1.0). Coverage, not transparency; it is
  ranked under this work in `PLAN-FIRST-COMPLETION` §1.9's order and stays there.

---

## 7. What this bears on, said narrowly / 이 작업이 걸리는 곳, 좁게

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

## 8. How it lands / 착지 방식

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
