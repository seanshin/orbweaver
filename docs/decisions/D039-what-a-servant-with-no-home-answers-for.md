# D039 — What a servant with no home answers for

**STATUS: PROPOSED** — drafted 2026-08-31 from D029 §6.1's Backend row, which is
the criterion's one remaining `open leak`. **Not self-approvable**: every
candidate changes what a public type answers a peer for, which is the same
surface [`D036`](D036-what-a-servant-answers-for-a-key-nobody-activated.md) was
about, and D036 §6.3's reading — *the evidence is a compile error at every site,
not a test going red* — applies here too.

**상태: 제안** — 2026-08-31 작성. **스스로 승인할 수 없음**: 모든 후보가 공개 타입이
피어에게 무엇을 답하는지를 바꾸며, 이는 D036이 다룬 것과 같은 표면이다.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. This is about the one instance left under its Backend row.

---

## 1. What raises this / 무엇이 이 문서를 불렀나

D036 made `knows` required and D029's Backend cell was rewritten on 2026-08-31
to say what the leak is now: *a `knows` whose body never reads the key*. The
same rewrite recorded its own lower bound in as many words:

> the scan sees an *unconditional* `true` and cannot see a `knows` that consults
> something and still answers for a superset of the keys its servant holds

**Part of that bound was closed by looking.** `a_key_nobody_activated.rs`'s
roster gained `answers_true_on_some_path` — a `knows` with a branch whose whole
answer is `true`, rather than a body that is `true` — and measured the tree:

| | |
|---|---|
| `Dispatch`/`SharedDispatch` impls | 83 |
| answer an unconditional `true` | 21, **0 in code a build emits** |
| answer `true` on *some* path | 22, **1 in code a build emits** |

The one is `orbweaver_gen::seam::ForeignServant`:

```rust
fn knows(&self, object_key: &[u8]) -> bool {
    match &self.identity {
        Some(i) => i.oid_of(object_key).is_some(),
        None => true,
    }
}
```

A `ForeignServant` built without a home answers for **every key a caller can
fabricate**, which is D029's Backend leak with a deployable servant behind it.

**And the reason recorded beside it cites something that no longer exists.** The
rustdoc reads *"Without one, the `Dispatch` default: everything, which is right
for a single-servant process and is what every deployment of this seam had
before homes existed."* D036 **deleted that default** on 2026-08-29. The
sentence justifying the behaviour outlived the thing it appealed to, which is
this repository's *a fact restated drifts from its home* in one clause.

*D029 백엔드 셀이 스스로 적어 둔 하한의 절반을 들여다봄으로써 닫았다. 트리에
빌드가 내보내는 그런 서번트는 **정확히 하나**이고, 그 옆에 적힌 근거는 D036이
2026-08-29에 지운 기본값을 인용하고 있다.*

---

## 2. Why it is not simply required / 왜 그냥 필수로 만들 수 없는가

The obvious repair — make `ForeignServant` always carry an identity, the way
D036 made `knows` always carry an answer — runs into an ordering fact rather
than a preference. `ObjectIdentity::new` takes an `ObjectHome`, which is host,
port and root key; **a servant is usually constructed before the server it will
be mounted in has bound**. `orbweaver-py-bridge --serve` can pass one because it
binds first; the eight in-tree constructions cannot, and neither could a caller
that builds a servant and then chooses a port.

So this is not the D036 move with a different noun. It is a question about what
a servant that genuinely serves **one** object should answer.

*명백한 수리 — 항상 identity를 갖게 하기 — 는 취향이 아니라 **순서**에 부딪힌다:
서번트는 보통 자기가 올라갈 서버가 바인드하기 전에 만들어진다.*

---

## 3. What the rest of the tree does / 트리의 나머지는 무엇을 하는가

Measured 2026-08-31 over the 28 deployable `knows` bodies. Every other
single-object servant in this workspace compares against the key it serves:

    spike_server.rs    object_key == ECHO_KEY
    spike_wide.rs      object_key == OBJECT_KEY
    spike_nat.rs       object_key == NAT_KEY
    spike_orb_shutdown.rs  object_key == self.key
    trading_server.rs  object_key == self.key

**The seam is the outlier**, and it is the outlier in the direction of answering
for more. Whatever is decided, that is the comparison to answer to: a foreign
servant that answers for every key while a Rust one beside it answers for its
own is a difference a caller can measure, which is D029's Language row as well
as its Backend row.

*이 워크스페이스의 다른 모든 단일 객체 서번트는 자기가 서비스하는 키와 비교한다.
seam만 예외이고, 예외의 방향은 **더 많이 답하는** 쪽이다.*

---

## 4. The candidates / 후보

**A. The servant is told which key it serves.** `ForeignServant` gains the
object key at construction — not a home, just the key, which the caller binding
the server always knows — and `knows` compares against it when there is no
identity. Matches what every other single-object servant here does. Costs a
signature change and a compile error at the eight construction sites, which is
D036's shape and its evidence.

**B. No home means no keys.** `None => false`. Rejected here rather than left
for the reader: a servant that answers `false` for everything serves nothing,
and the leak test would go green because the endpoint stopped working. That is
the blanket-`false` experiment CLAUDE.md already records as producing a vacuous
green, one level down.

**C. Accept it as a named floor**, the way D037 accepted `select`'s addresses.
Cheapest, and the honest objection is that **nothing about a foreign servant
makes a key comparison impossible** — A is available and small. D038 §4 refused
exactly this reasoning for the Language row: *a criterion that files missing
work under "named floor" has stopped measuring.*

---

## 5. Recommendation / 권고

**A.** It is what the rest of the tree does, it closes the row's one deployable
instance rather than displacing it, and its cost is a compile error at eight
sites — all of them in this repository. B is a vacuous green with a name. C
would be the third named floor on a five-row criterion, and D037 §6.4 already
says what that measures.

What A must include, so that it is worth more than a flipped arm:

1. **The key comes from the caller, not from a default.** A construction that
   does not name a key does not compile — the gap unrepresentable rather than
   detectable, which is what D036 bought.
2. **The stale rustdoc goes with it.** The sentence appealing to `Dispatch`'s
   deleted default is removed in the same change, not left to be found again.
3. **The roster's pin moves deliberately**: `a_key_nobody_activated.rs` asserts
   exactly one such servant today and says, in its own message, that a count
   which quietly drops is indistinguishable from a scan that stopped looking.
   Closing this makes it zero, and that edit is the row moving.

*A를 권고한다. 트리의 나머지가 하는 것이고, 변위가 아니라 폐쇄이며, 비용은 여덟
곳의 컴파일 에러다. B는 이름 붙인 공허한 초록이고, C는 다섯 행 기준에서 세 번째
이름 붙인 바닥이 된다.*

---

## 6. What would refute this / 무엇이 이것을 반증하는가

- **A caller that legitimately cannot know its key at construction.** If such a
  shape exists in a real deployment, A's required parameter is a burden and C
  becomes the honest answer. None is known here; the eight sites all know it.
- **A `knows` answering from a set merely wider than its servant's.** It holds
  no `true` literal, so neither the roster nor this decision sees it, and
  closing this instance would leave that class untouched. It is named in D029's
  Backend cell as the part of the bound that stays open either way.

*반증: 생성 시점에 자기 키를 알 수 없는 정당한 호출자가 있다면 A는 부담이고 C가
정직한 답이 된다. 그리고 `true` 리터럴 없이 그냥 더 넓은 집합에서 답하는 `knows`는
어느 쪽으로도 보이지 않는다.*
