# Orbweaver — working instructions

AI-driven CORBA/IDL interface automation. An MIT ORB core written against the
published OMG wire specification, plus an AI specification pipeline on top.

Plan: `docs/PLAN.md` (EN) · `docs/PLAN.ko.md` (KO) · Phase 0 results: `docs/PHASE0.md`

---

## The operating model: batch → oracle → repair → codify

**Work the whole set at once, verify the whole set at once, fix by root cause,
then make the cause impossible.** Never item-by-item.

**전체를 한 번에 작업하고, 한 번에 검증하고, 근본원인 단위로 고치고, 그 원인이
다시 발생하지 못하게 만든다.** 건건이 처리하지 않는다.

This is not a style preference. Phase 0 measured it: 20 IDL files generated in
one pass produced 7 failures, and **all 7 had a single root cause**. Item-by-item
work would have produced 7 separate patches and never surfaced the rule.
Batching made the cause visible; one fix took the batch from 65% to 100%.

이것은 취향이 아니다. Phase 0이 측정했다: 20건 일괄 생성 → 실패 7건 → **7건 전부
동일한 근본원인**. 건건이 고쳤다면 패치 7개가 나왔을 뿐 규칙은 드러나지 않았다.

### The four steps

1. **Batch** — produce every item in the set in one pass. **Do not consult the
   oracle mid-pass.** Peeking contaminates the first-pass measurement and, worse,
   lets you patch symptoms one at a time so the shared cause never appears.
   *일괄 생성 중에는 오라클을 보지 않는다. 중간에 보면 1차 통과율이 오염되고
   공통 원인이 드러나지 않는다.*

2. **Oracle** — run every deterministic check across the whole batch at once.
   Then **cluster the diagnostics by root cause, not by item.** The output of
   this step is a list of causes with their affected items, never a list of items.
   *진단을 항목이 아니라 근본원인으로 묶는다.*

3. **Repair** — one fix per root cause, applied across every affected item.
   If a "fix" only helps one item, it is not a root-cause fix; find the real
   cause or record it as a genuine one-off with a reason.
   *원인 하나당 수정 하나. 한 항목에만 듣는 수정은 근본 수정이 아니다.*

4. **Codify** — turn every confirmed cause into something permanent: a lint
   rule, a prompt constraint, a corpus case, or a rule in this file. A cause
   that is only fixed will come back; a cause that is codified cannot.
   *확인된 원인은 반드시 영구 산출물로 바꾼다. 고치기만 한 원인은 돌아온다.*

Repeat until a round yields no new root causes. Report the first-pass rate and
the round count separately — they measure different things.

### Reporting a batch

Always state: batch size, first-pass rate, root causes found (with affected
counts), what was codified. Never report only the final number — the first-pass
rate is the signal about the generator, the round count is the signal about the
oracle.

---

## Hard rules

### Licensing boundary — non-negotiable / 라이선스 경계 — 타협 불가

This project ships MIT. omniORB, ACE/TAO and JacORB are **LGPL/GPL/DOC and are
fixtures, never dependencies.**

- **Never** `import`, link, vendor, copy from, or redistribute any part of them.
- They may only be (a) run as separate-process wire peers over TCP, and
  (b) invoked as external programs whose text output we read (`omniidl` as a
  conformance oracle).
- `cargo tree` must stay free of them. Anything under `crates/` is original work
  written against the OMG specification.
- CI images containing them are built or pulled inside CI and **never published**
  as project artifacts — publishing is redistribution.

Before adding any dependency, check its license against this rule. MIT or
MIT-equivalent, otherwise we write it.

### IDL rules the compiler enforces / 컴파일러가 강제하는 IDL 규칙

- **Identifier clashes are case-insensitive.** A member, parameter or operation
  may not share a name with a type or an enclosing scope, ignoring case.
  `Position position`, `Value value`, `module inventory { interface Inventory }`
  and `struct Version { unsigned long version; }` are all illegal. This is
  natural naming in every other language, which is exactly why it is the
  dominant generation failure. *가장 흔한 실패 원인.*
  Run `python3 spikes/idl_lint.py <files>` before the oracle — it catches this
  class with an actionable message. Documenting the rule does not prevent it:
  it has since caught two corpus files and two fixtures, one written by someone
  who had just described the rule in that same file's header.
- `TypeCode` must be qualified as `::CORBA::TypeCode`.
- v1 wire support excludes `valuetype`, abstract interfaces and `fixed`. The
  parser accepts them; the wire does not. See `docs/PLAN.md` §4.4.
- SIDL v1 uses **structured comments** (`//@ ai_desc: ...`), not IDL 4
  `@annotation` — deployed compilers reject the latter (Phase 0 assumption C).

### Wire and test rules / 와이어·테스트 규칙

- **Compare decoded values, never raw buffers.** CDR padding content is
  undefined by the specification and omniORB does not zero it, so byte-for-byte
  message comparison against a reference ORB produces false failures.
- **Test both byte orders.** An encoder that only works native-endian passes
  every local test and fails in the field.
- **Alignment origin matters.** A GIOP message aligns from the first byte of its
  12-byte header; an encapsulation restarts alignment at its own first byte.

### Harness rules / 하네스 규칙

Each of these produced a phantom failure during Phase 0. They will recur.

- **Wait loops must sleep.** `for i in $(seq 1 500); do [ -f f ] && break; done`
  finishes in microseconds and does not wait at all. This caused the initial
  assumption A failure; the protocol was correct the whole time.
- **Never pipe into `grep -q`** when the producer matters. `grep -q` exits on
  first match and SIGPIPEs upstream. Capture to a variable, then match.
- **An unmeasured check is a failure, never a pass.** If a fixture will not
  start, increment the failure counter. A harness that reports green on an
  unmeasured assumption is worse than no harness.

### Honesty rules / 정직성 규칙

- Report what was measured, not what was intended. If something is unmeasured,
  say so and say why.
- When the generator and the evaluator are the same model, label the number
  indicative and say so in the same breath as the number.
- Do not claim a transient is diagnosed until it reproduces and the fix makes it
  stop. "Did not reproduce in N runs" is a valid, honest result.

---

## Commands

```bash
cargo test --workspace          # unit tests: CDR alignment, GIOP framing, IOR
./spikes/run_phase0.sh          # full assumption harness; exit code is the verdict
python3 spikes/idl_lint.py *.idl  # pre-oracle lint: case-insensitive clashes
omniidl -b dump <file>.idl      # conformance oracle for a single file
cargo run -q --bin spike-dump -- spikes/echo.ior <op> <big|little> <n>
```

Fixture setup: `brew install omniorb` (interop peer and oracle only).

## Layout

```
crates/orbweaver-cdr/     CDR encode/decode
crates/orbweaver-giop/    GIOP messages, IOR, invoker, spike binaries
corpus/golden/            must all compile — type-system and CDR coverage
corpus/negative/          must all be rejected — diagnostic quality material
corpus/requirements/      assumption B benchmark, frozen before generation
corpus/annotations/       assumption C probes
spikes/                   omniORB fixture, server, harness
docs/                     PLAN.md, PLAN.ko.md, PHASE0.md
```

## Conventions

- Documents are maintained in **English and Korean**, kept structurally
  symmetric section by section.
- Corpus additions go in with the change that motivated them, never later.
- Rust: `unsafe_code = "forbid"` at the workspace level. Keep it that way —
  wire parsing is the classic memory-safety hazard and that is why the core is
  Rust in the first place.
