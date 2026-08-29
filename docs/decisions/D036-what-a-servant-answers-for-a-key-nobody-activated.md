# D036 — What a servant answers for a key nobody activated

**STATUS: APPROVED 2026-08-29 — option A.** The owner chose **A: make `knows`
required**, which is §5's recommendation unchanged. What was approved is
therefore A with §4's D — L2's two spikes — in the same batch, and §4's C gate
kept.

The approval is recorded as accepting §6's third entry: **A's evidence is a
compile error at 22 sites, not a test going red.** `a_key_nobody_activated.rs`
stays green across this change by construction, because its fixture now states
`true` explicitly where it used to inherit it, and that is the fixture's
documented choice rather than a weakening. A reader who takes that still-green
test as evidence that A did something is reading it wrong; §6.3 exists so the
reading is written down before anyone makes it.

Drafted the same day as the first step of
[`PLAN-FIRST-COMPLETION.md`](../PLAN-FIRST-COMPLETION.md) §1's L1. It was not
self-approvable: §4's candidates change a public trait and what every servant in
the workspace answers to a peer, which is the surface D029 §6.1's Backend row is
about. §3 says why the obvious change is not available.

**상태: 승인 2026-08-29 — 선택지 A.** 소유자가 **A: `knows`를 필수로**를 골랐고,
이는 §5의 권고 그대로다. 따라서 승인된 것은 A와 §4의 D(L2의 스파이크 둘)를 한
배치로, §4의 C 게이트는 유지하는 것이다. 이 승인은 §6의 셋째 항목을 받아들인
것으로 기록된다: **A의 증거는 22곳의 컴파일 에러이지 테스트가 붉어지는 것이
아니다.** `a_key_nobody_activated.rs`는 이 변경을 가로질러 구조적으로 초록으로
남는다 — 픽스처가 상속하던 것을 이제 명시하기 때문이며, 그것은 약화가 아니라 그
파일이 문서화해 둔 선택이다.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. This document is about one open leak under its Backend row.

---

## 1. What raises this / 무엇이 이 문서를 불렀나

A hand-written C peer — a program that links nothing of ours — dialled us on
2026-08-26 and established, in one call, a fact about our backend: **fabricate
any object key at an endpoint and be answered.**

`orbweaver_giop::server::Dispatch::knows` has a default:

```rust
fn knows(&self, _object_key: &[u8]) -> bool {
    true
}
```

A servant that inherits it accepts every key at its endpoint, so the object key
selects nothing and what is behind the address is not a reference — the address
is the only thing naming a target. CORBA 3.4 §15.3.8.6's own default,
`USE_ACTIVE_OBJECT_MAP_ONLY`, would have answered `OBJECT_NOT_EXIST` and told
the caller nothing. What the caller learns instead is a backend fact: *this
endpoint holds one undifferentiated servant rather than a POA with an active
object map* — which D029's Backend row says a caller must not be able to tell.

`crates/orbweaver-giop/tests/a_key_nobody_activated.rs` measures what a caller
sees today: 10 tests across 3 GIOP versions × 2 byte orders (run 2026-08-29).
The leak is gated. It is not closed.

*손으로 쓴 C 피어가 한 번의 호출로 확립했다: 아무 객체 키나 지어내면 답을 받는다.
객체 키가 아무것도 선택하지 않으므로, 주소 뒤에 있는 것은 참조가 아니다.*

---

## 2. Why the recorded reason for not changing it has weakened / 기록된 이유가 약해진 경위

D029's Backend cell gives the reason: *the repair lands in crates the GIOP crate
does not own*, with a figure — *"26 of the workspace's 72 implementations
inherit that default"* — that carries **no date beside it**. Recomputed from the
tree 2026-08-29: **81 impls, 59 override `knows`, 22 inherit it.** The
production inheritors went to zero on 2026-08-28. The 22 are:

| | |
|---|---|
| 20 | test fixtures — 11 below `src/server.rs`'s line-2200 `#[cfg(test)]`, 9 under `tests/` |
| 2 | `spikes/estate/servant.rs` and `spikes/e2e/servant.rs`, which check the key in `dispatch_body` rather than in `knows` |

Nine files, all inside this workspace, no external implementor — the trait is
`pub` in an MIT repository whose only consumers are its own crates. The
objection has not vanished; it has shrunk to two files that already check the
key in the wrong hook.

*반대 이유는 사라진 것이 아니라, 이미 잘못된 훅에서 키를 검사하고 있는 두 파일로
줄었다.*

---

## 3. The obvious change is not available, and that is measured / 명백한 변경은 불가능하다

**`knows` cannot be "changed to the specification default".** It is a trait
method with no POA and no active object map in scope — there is nothing for
`USE_ACTIVE_OBJECT_MAP_ONLY` to be expressed *against*. The only value the
default can be changed *to* is `false`.

**And that has been run.** CLAUDE.md records it: the leak test *stayed green*
under a blanket `false`, because a server that serves nothing answers both keys
identically too. The obvious repair produces a vacuous green — the failure mode
this project names most often, arriving in the repair for a leak rather than in
the leak.

A second objection to `false`, separate and sufficient: it breaks every
inheritor **at run time**, discovered by a peer, where the alternative in §4
breaks them **at compile time**, discovered by the author.

*명백한 수정은 이미 돌았고 **공허한 초록**을 만들었다. 그리고 `false`는 모든
상속자를 **실행 시점에** 깨뜨린다 — 피어가 발견한다. 대안은 **컴파일 시점에**
깨뜨린다 — 저자가 발견한다.*

---

## 4. The candidates / 후보

**A — make `knows` required.** Delete the default. Every implementation must
state its answer; the 22 become 22 explicit statements, most of them one line.

- *For.* It makes the gap **unrepresentable rather than detectable**, which is
  the rule this repository already codified for a different cascade: *"the
  walker must ask the mapper at every node rather than keep its own list."* A
  servant cannot inherit permissiveness by accident, because there is nothing to
  inherit. Breakage is a compile error at every site, in one commit, by the
  person making the change.
- *Against.* It changes a public trait. Every future servant pays one line even
  when `true` is right for it.
- *What it does not do.* It closes nothing by itself: a fixture that explicitly
  answers `true` leaks exactly as before. **What it buys is that the next
  production servant cannot leak by omission**, and production inheritors are
  already zero — so this is a change about the future, stated plainly rather
  than sold as a closure.

**B — a `Server`-side key set.** Servants register the keys they serve; the
default consults the server's set.

- *Against, and it is decisive.* `Server` already holds one `object_key`, and
  servants serve **derived** keys — `MOE_BASE_KEY` plus an expert id, a tenant,
  a name. A set cannot be enumerated ahead of a servant that mints keys, so the
  server would need to ask the servant, which is `knows` again with more
  machinery in front of it.

**C — leave the default and keep the gate.** Status quo:
`a_key_nobody_activated.rs` measures what a caller sees and goes red if the
answer changes.

- *For.* It is honest today: the leak is measured, named, and its landing site
  is empty of production code.
- *Against.* A gate that measures a leak is not a gate that prevents the next
  one. The gate names the inheritors; it cannot fail a servant that has not been
  written yet.

**D — fix the two spikes only.** Move `spikes/estate/servant.rs` and
`spikes/e2e/servant.rs`'s key check from `dispatch_body` into `knows`.

- This is real and small and is **PLAN-FIRST-COMPLETION's L2**. It is listed
  here because it is the part of the work that needs no decision, and because a
  batch that did only this would be *a batch scoped to a keyword rather than to
  the rule* — the two servants are instances of the rule, not the rule.

---

## 5. Recommendation / 권고

**A, with D in the same batch, and C's gate kept.**

A is recommended for one reason and it is not that it closes the leak — it does
not. It is that every other candidate leaves *the omission* possible, and the
omission is how this leak arrived: nobody chose to accept any key at any
endpoint; twenty-six servants inherited a default and one C peer noticed.

D lands with it because the two spikes are the same fact one layer down — they
answer the request path correctly and the **probe** path permissively, which is
the request/probe disagreement the `serve_one` reorder closed for a *moved* key
and left open for an *unknown* one.

C's gate is kept, and this is not a formality: after A, the 22 explicit answers
need something that reads them, and `a_key_nobody_activated.rs`'s roster is
computed from the tree rather than typed — *the guard beside the old list only
asserted the list was non-empty*, which is the mistake it was rebuilt to avoid.

*A를 권고하는 이유는 구멍을 닫아서가 아니다 — 닫지 않는다. 다른 모든 후보가
**누락**을 가능한 채로 남기고, 이 구멍은 누락으로 도착했기 때문이다: 아무도 모든
키를 받겠다고 **선택**하지 않았다. 스물여섯이 기본값을 상속했고 C 피어 하나가
알아챘다.*

---

## 6. What would refute this / 무엇이 이것을 반증하는가

1. **An external implementor.** A is a breaking change to a `pub` trait. The
   claim that this costs nothing outside the workspace rests on there being no
   outside — true today, in one MIT repository, and false the day something
   depends on `orbweaver-giop`.
2. **A servant that genuinely cannot answer.** If a real servant exists whose
   key set is unknowable at `knows` time — decided only by the body of the
   request — then A forces it to write `true` and A has bought nothing there.
   None is known; one would be an argument for B's shape rather than A's.
3. **A measuring the wrong thing.** After A, `a_key_nobody_activated.rs`'s
   fixture will state `true` explicitly, and the leak it measures will be
   unchanged. That is correct and is also the trap: a reader could take the
   still-green leak test as evidence that A did something. **A's evidence is a
   compile error at 22 sites, not a test going red** — and if that is not
   acceptable evidence, A should not be approved.
4. **The figure moving again.** §2's roster is a reading of 2026-08-29. It has
   moved twice in four days. Re-read it before acting, do not quote it.

---

## 7. What this document does not claim / 주장하지 않는 것

- It does not claim the leak is closed by any candidate. **None of them closes
  it.** The permissive answer is still available to any servant that states it,
  which is the point of stating it.
- It does not claim `USE_ACTIVE_OBJECT_MAP_ONLY` is implemented here. §3 says
  the opposite: there is no active object map at the trait, and a POA-level
  answer is a different piece of work under a different row.
- It does not decide L2's two spikes on its own authority — §4's D is named so
  the batch is one batch, and it is small enough to land either way.

*어느 후보도 구멍을 닫지 않는다. 허용하는 답은 그것을 **명시하는** 서번트에게 여전히
열려 있으며, 명시하게 하는 것이 요점이다.*
