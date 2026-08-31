# D038 — A call travelling the other way through the seam

**STATUS: APPROVED 2026-08-31 — option A.** The owner chose **A: the nested
request**, which is §5's recommendation unchanged, together with §3's three
invariants and the rule §2.1 forces: **the nested call is made on a connection
the servant owns, never on the one the request arrived on.**

The approval is recorded as accepting what §5 says the alternatives cost: B's
price is per-language and permanent, paid by every binding that ever exists,
while the hazard it avoids has to be handled either way; and **C is not a
floor** — nothing about being written in another language makes invoking
impossible, so filing it as one would be filing missing work as a property.

It was asked for before the work started rather than after, for the reason §5
gives: the protocol has three implementations and a re-entrancy no test
currently reaches. Landing is planned as five steps whose order is the
protocol's — the message, the Rust re-entrancy with its deadlock control, the
other two runtimes in the same batch, the handle table, then the leak leg —
recorded in [`PLAN-FIRST-COMPLETION.md`](../PLAN-FIRST-COMPLETION.md) §1's L4
rather than restated here.

**상태: 승인 2026-08-31 — 선택지 A.** 소유자가 **A: 중첩 요청**을 골랐고, 이는 §5의
권고 그대로이며, §3의 불변식 셋과 §2.1이 강제하는 규칙 — **중첩 호출은 서번트가
소유한 연결로 하고, 요청이 도착한 연결로는 절대 하지 않는다** — 을 함께 승인한 것이다.

이 승인은 §5가 말하는 대안의 비용을 받아들인 것으로 기록된다: B의 값은 **언어마다,
영구적으로** 치러지는 반면 그것이 피하는 위험은 어차피 다뤄야 하고, **C는 바닥이
아니다** — 다른 언어로 쓰였다는 사실이 호출을 불가능하게 만들지 않으므로 그것을
바닥으로 분류하는 것은 빠진 작업을 성질로 기록하는 일이다.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**.

---

## 1. What is left, precisely / 정확히 무엇이 남았는가

`orbweaver_gen::seam`'s own header says it, and has since the module was
written:

> What it does **not** close is named at `ForeignServant::dispatch_body`: a
> reference *arriving* as an argument is still a handle the far side cannot
> invoke, because invoking it would need a call to travel the other way
> through `Answerer` and **this protocol has no message for that yet**.

The mechanism is `SeamReferences`: an arriving reference is issued `local-N`
and the `Ior` is kept on this side. The far side may pass the handle back and
**cannot dial it** — §4.7's bearer-address rule, deliberately. What it also
cannot do is *ask us* to dial it.

So a foreign servant can be called, can name its own objects, and can hand out
references — `ObjectIdentity` closed those three on 2026-08-26 — but it cannot
**use** one it was given. In contract terms: it can implement `Registry::lookup`
and cannot implement anything that takes a reference and calls it.

*도착한 참조는 저쪽이 호출할 수 없는 핸들이고, 저쪽은 우리에게 대신 걸어달라고
**요청할 수도** 없다. 프로토콜에 그 메시지가 없다.*

---

## 2. Why this is a protocol decision and not a patch / 왜 패치가 아니라 결정인가

`Answerer::ask` is *put this call, give me the next document*. Strict
request/response: one conversation, one direction, one outstanding message.

Every way of letting the far side invoke breaks that. While the parent waits
for a reply to call *C*, the child sends a request of its own — *invoke
`local-3`* — and the parent must answer it **before** the reply to *C* can
arrive. The seam becomes **re-entrant**: a conversation that nests.

That is a property, and three things follow from it that are not implementation
detail:

1. **A deadlock shape exists that does not exist today.** The parent is inside
   `Dispatch::dispatch_body` when it asks. If resolving `local-3` dials back
   into the same server, on a server that serves requests one at a time, the
   call cannot complete. Today that is unreachable because the child cannot
   ask; after this it is reachable and something must say what happens.
2. **Every implementation of the protocol changes**, and there are three:
   `py_bridge`'s `Parent`, `pychild::PythonChild`, and `python_rt`'s
   `Bridge`/`Host` pair. The loop that reads a line stops being *read the
   reply* and becomes *read the next document, which may be a reply or may be a
   request*.
3. **The far side gains the ability to make this process dial an arbitrary
   address**, bounded only by what is in the handle table. That table is the
   bearer-address rule's enforcement point, and it stops being read-only.

*`ask`는 엄격한 요청/응답이다. 저쪽이 호출하게 하는 모든 방법이 그것을 깨고 seam을
**재진입**으로 만든다. 오늘은 도달 불가능한 교착 모양이 그때부터 도달 가능해진다.*

---

## 3. What must remain true / 유지되어야 하는 것

Whatever lands, these do not move:

- **The far side never learns an address.** It asks by handle. A design that
  answered `local-3` with a stringified IOR would close this leak by opening
  the one §4.7 exists to prevent.
- **The handle table stays the boundary.** A handle nobody issued is refused,
  and refused as a *seam* failure rather than as a servant's — the far side
  guessing `local-99` is a wiring mistake in one process, not a wire event.
- **`ask`'s error contract is unchanged.** `Err` is the seam breaking; a
  refusal is a well-formed answer. A nested call that raises must come back as
  an answer the servant can catch, or a foreign servant cannot implement
  `raises` clauses on the operations it calls.

---

## 4. The candidates / 후보

**A — a nested request kind on the same pipes.** The child may write
`{"invoke": {"handle": "local-3", "op": "...", "args": {...}}}` where a reply
was expected; the parent answers it and goes back to waiting.

- *For.* One transport, one framing, no new channel. `python_rt`'s loop already
  reads documents in a loop and dispatches on a key.
- *Against.* The re-entrancy of §2 in full. The parent must invoke **while
  inside** its own `dispatch_body`, so the deadlock in §2.1 is real and needs a
  stated answer — most likely *the nested call is made on a connection this
  servant owns, never on the one the request arrived on*, which is a rule
  somebody has to hold.

**B — a second channel.** The child gets a separate pair of pipes for outbound
calls, served by a thread on this side.

- *For.* No re-entrancy in the reading loop: the reply channel stays strict
  request/response, and the deadlock in §2.1 becomes a lock-ordering question
  instead of a protocol one.
- *Against.* Two channels is two things to start, two to reap, and two ways for
  a child to be half-gone — and this repository has already paid for one
  half-gone child at twelve leaked processes. It also doubles what a new
  language must implement, which is the cost D032 §3 measures a binding by.

**C — refuse, and record the floor.** A foreign servant cannot invoke an
arriving reference, and that is written where the contract meets the seam.

- *For.* It is what is true today, and D029's Language row is otherwise closed.
- *Against, and it is the strongest objection to it.* This is not a floor like
  the bootstrap address — nothing about *being written in another language*
  makes invoking impossible. It is missing work, and calling missing work a
  floor is the move this project refuses everywhere else. **A row that accepts
  it stops being able to tell the two apart.**

---

## 5. Recommendation / 권고

**A, with §3's three invariants and one rule §2.1 forces: the nested call is
made on a connection the servant owns, never on the one the request arrived
on.**

A is recommended over B because the cost B avoids is a hazard that has to be
handled either way — a nested call *can* reach back into this process however
the bytes travel — and the cost B adds is per-language, permanent, and paid by
every binding that ever exists. D032 §3 measures a language binding by what it
must supply; B makes that two channels and a thread, where A leaves it one
function.

A is recommended over C because C is not a floor. §4's C says why: nothing about
another language makes this impossible, and a criterion that files missing work
under *named floor* has stopped measuring.

**This is the largest item in `PLAN-FIRST-COMPLETION` §1 and it should be
approved before it is started, not after** — the protocol has three
implementations and a re-entrancy no test currently reaches.

*A를 권고한다. B가 피하는 비용은 어차피 다뤄야 하는 위험이고, B가 더하는 비용은
**언어마다 영구적**이다. C는 바닥이 아니다 — 다른 언어로 쓰였다는 사실이 호출을
불가능하게 만들지 않으며, 빠진 작업을 "이름 붙인 바닥"으로 분류하는 것은 이
프로젝트가 다른 모든 곳에서 거절하는 수다.*

---

## 6. What would refute this / 무엇이 이것을 반증하는가

1. **A deadlock with no stated answer.** If the rule in §5 turns out not to
   cover a case — a servant whose nested call must reach the same object it is
   serving — then A needs either a bound (a nesting depth, a deadline) or B's
   separation, and the choice was made too early.
2. **A second language arriving before this lands.** D032 §3's measure is what
   a binding must supply. If Java or C arrives first, B's *per-language* cost
   becomes concrete rather than projected and the comparison in §5 is being
   made on an estimate.
3. **Nothing needing it.** No contract in this repository requires a foreign
   servant to invoke an arriving reference — `corpus/golden/16`'s `Registry`
   needed *handing one out*, which is closed. If nothing needs it after this is
   built, A bought a protocol change for a row's completeness, and §4's C looks
   better in hindsight than it does here.
4. **The handle table growing.** §3 says the table is the boundary. If invoking
   requires keeping more than the `Ior` — a live connection per handle, say —
   then the table stops being the pure thing `SeamReferences` documents and
   that is a cost this document has not counted.

---

## 7. What this document does not claim / 주장하지 않는 것

- It does not claim the leak is reachable by any caller today. It is a **gap in
  what a foreign servant can implement**, not something a peer can observe on
  the wire — which is why the Language row calls it the last item rather than
  an open refutation.
- It does not size the work beyond §2.2's *three implementations change*.
- It does not decide what a nested call's deadline is. §6.1 says that is where
  A is most likely to be found wanting.
