# D025 — The contract as a prompt, and what CORBA's model does not carry

**STATUS: PROPOSED** — drafted 2026-08-25 on the thesis that the CORBA model
does not fit the AI era and that the script specification should evolve toward
a prompt form. Everything measured below was read that day. Not
self-approvable: §5 proposes a rule about when vocabulary may be added, which
constrains every future batch.

**상태: 제안** — 2026-08-25, 기존 CORBA 모델이 AI 시대와 맞지 않으며 스크립트
규격이 프롬프트 형태로 발전해야 한다는 명제에서 작성.

---

## 1. The thesis is right, and this project is further along than it says

CORBA's contract is a **type** contract. `void retire(in ModelId m)` fixes the
bytes and says nothing an agent needs: what it is for, whether it can be
undone, what it costs, whether a human must agree, what to do when it refuses.
Every ORB assumes a **compile-time client** — you hold the IDL, you generate a
stub, the caller's shape is fixed before it runs. An agent discovers at runtime
and composes calls it was never compiled against.

**And this project already answers a large part of that.** SIDL's vocabulary,
measured 2026-08-25, is nine keys:

```
ai_desc  ai_effect  ai_authz  ai_idempotent  ai_unit
ai_pii   ai_sensitivity  ai_example  ai_precond
```

with **fourteen contract rules** enforcing them (`contract/effect-unknown`,
`gated-without-authz`, `pii-without-authz`, `read-only-mutating-name`,
`oneway-not-idempotent`, `reference-escapes-without-authz`,
`inherited-destructive-splits-the-quota`, and seven more). The interceptor
chain, the dry run, AnyJSON, capability handles with a TTL and the quota seat
are the rest of it. **The AI-era extension exists and has never been named as
one** — it is spread across SIDL, the MCP boundary and the forge, and no
document says "this is what we did about the model not fitting."

*명제는 옳고, 이 프로젝트는 스스로 말하는 것보다 앞서 있다. 다만 그것을 그렇게
이름한 문서가 없다.*

## 2. The measurement that decides where to start

**Two of the nine keys are inert.** `ai_example` and `ai_precond` are in the
known-key list — so writing one does not trip `unknown key` — and:

- **no consumer reads either.** Every occurrence in `crates/` is the vocabulary
  list itself (`annotate.rs`, `contract.rs`, `lex.rs`).
- **the corpus uses neither.** Zero files under `corpus/` carry one.

So the contract language already has a slot for a **worked example** and a slot
for a **precondition** — the two things a prompt most needs and a type contract
most lacks — and both are empty and unread. **That is where this starts**, and
it starts by making what exists reach the prompt rather than by inventing more
vocabulary.

*아홉 중 둘이 불활성이다. 프롬프트가 가장 필요로 하고 타입 계약이 가장 못 담는
것 둘 — **예시**와 **선행조건** — 이 이미 자리를 갖고 있고, 비어 있으며, 아무도
읽지 않는다.*

## 3. The renderer already exists and is where the extension lands

`orbweaver-forge`'s `Subject::to_prompt()` — *"the subject as prompt text, the
whole of what a producer is shown"* — is a first-class contract-to-prompt
renderer, and it is where a contract stops being a type and becomes a prompt.
It is also where a real defect lived until today: `render_type` called two
types the wire marshals `<unnamed type>`, and its own comment argued the right
thing for the wrong set — *"a prompt that invents a type name teaches the
producer to quote one back."*

**That comment is this document's thesis in one sentence.** A contract rendered
into a prompt is read by something that will quote it back, so every field in
it is load-bearing in a way no IDL comment has ever been.

## 4. What a prompt has that the contract still does not

Beyond `ai_example` and `ai_precond`, which exist and are inert:

- **Negative guidance.** *"Do not call this to do X; call Y."* An agent picks
  by name similarity — the failure mode `contract/read-only-mutating-name`
  already catches from the other direction. A near-miss operation is the most
  expensive mistake available and nothing steers away from one.
- **Cost.** Not modelled anywhere. `SEAT_QUOTA` bounds *how many*; nothing says
  a call is expensive, slow, or bills someone.
- **Reversibility.** `ai_effect: destructive` implies irreversibility and does
  not state it. Whether an undo exists, and what it is called, is the first
  thing a cautious caller wants.
- **The refusal that teaches.** S4 gives IDL diagnostics a position and a fix
  hint, and this project counts diagnostics as a product. **A refused *call*
  gets a rule id and a reason, and not the one thing an agent needs: what would
  make it succeed.** The guard chain knows — it refused for a stated reason —
  and does not say.
- **Provenance of an answer.** A value that came from a degraded peer, a cache
  or a fresh call is the same value. An agent deciding whether to act on it
  cannot tell.

## 5. The rule that keeps this from being decoration / 장식이 되지 않게 하는 규칙

A prompt-shaped contract is exactly where *"it feels better"* becomes the
failure mode, and this project refuses unmeasured claims everywhere else.

**Proposed: no key joins the vocabulary until it moves a frozen benchmark.**
The instruments exist — `corpus/requirements/{inputs,inputs-v2}` (assumption
B), `corpus/queries/{search-v1,search-v2}.tsv` (the frozen search benchmark),
and the S1–S5 pass rates. The measurement is: with the key and without it, on
the frozen set, does the producer do better?

And the honesty rule that already governs these numbers stands unchanged: **one
model family generating and evaluating makes the number indicative, and it is
said in the same breath as the number.** A vocabulary addition that cannot be
measured this way is not forbidden — it is recorded as unmeasured, with what it
would take, exactly as a class-B claim is.

*어휘는 동결 벤치마크를 움직여야 합류한다. 잴 수 없는 추가가 금지되는 것은
아니고, **미측정으로 기록**된다 — class-B 주장과 같은 규율.*

## 6. What is proposed / 제안

### P1 — the two inert keys reach the prompt (`orbweaver-forge`)

`ai_example` and `ai_precond` are rendered by `Subject::to_prompt()` and
carried by the registry to the MCP boundary's `describe_interface`. Corpus
files gain them where they are true — **written by hand, not inferred**, since
an inferred example is a guess quoted back as fact.

**Measured against `corpus/requirements`'s frozen inputs**: the S1–S3 pass rate
with the two keys rendered and without. If the number does not move, **that is
the result and it is worth having** — an inert key that has been measured inert
is a different thing from one nobody looked at.

### P2 — the refusal that teaches (`orbweaver-mcp`)

Every guard-chain refusal already names its stage and its reason. Add **what
would make it succeed**, from what the chain already knows: which scope is
missing, which annotation is absent, which approval is pending. This is S4's
fix-hint discipline applied to calls, and it is the single highest-value item
here because it converts a dead end into a next step.

### P3 — negative guidance and reversibility, if they measure

Two candidate keys — `ai_not_for` and `ai_undo` — proposed **only** as
measurement subjects under §5, not as decided vocabulary. `ai_undo` has a
second life: it is checkable, since the named operation must exist in the same
contract, which makes it the rare annotation a compiler can hold to account.

### P4 — name what already exists (documents)

One section, in `ARCHITECTURE.md` or `PLAN.md`, saying plainly: *this is what
was done about the CORBA model not fitting an agent caller* — SIDL's nine keys,
the fourteen contract rules, the chain, the dry run, the handle TTL, AnyJSON.
Today a reader has to assemble it from five documents and would not know it was
one answer.

## 7. What must not happen / 해서는 안 되는 것

- **The contract does not become free text.** SIDL is structured comments
  precisely because deployed compilers reject IDL 4 `@annotation` (Phase 0
  assumption C) and because a key a rule can check is worth more than prose a
  model interprets. Fourteen rules depend on the keys being keys.
- **Nothing is inferred into an authored slot.** `contract/inference-overwrites-authored-annotation`
  already exists as a rule; an inferred `ai_example` would be a guess wearing a
  fact's clothes, and the producer quotes it back.
- **No key ships unmeasured and unmarked.** §5's rule, and the alternative is a
  vocabulary that grows by intuition — which is the failure this project's
  whole measurement discipline exists to prevent, arriving through the one door
  where it would feel most reasonable.

## 8. What this document does not claim / 주장하지 않는 것

It does not claim CORBA is obsolete: the wire is measured against two peers at
three GIOP versions and that work stands unchanged by any of this. It does not
claim the five gaps in §4 are the complete list — they are the five that were
measurable today. And it does not claim P3's two keys are good ideas; it claims
they are testable ones, which under §5 is the only claim that may be made about
a key before it is measured.
