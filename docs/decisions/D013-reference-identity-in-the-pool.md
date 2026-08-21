# D013 — An identity map for two references to one object

**STATUS: PROPOSED** — drafted 2026-08-21 from what commit `cd9f88f` reported
and deliberately did not build. Not self-approved; §7 says what approval would
mean and §5 recommends **not building**, which is still a thing to be decided
rather than assumed. The measurement §2 rests on landed with this document
(`crates/orbweaver-giop/tests/forward_clone.rs`,
`two_references_to_one_object_each_pay_the_forward_once`), so the recommendation
can be checked rather than believed.

**상태: 제안** — 2026-08-21 작성. `cd9f88f`이 보고하고 일부러 짓지 않은 것에서
출발한다. 스스로 승인하지 않는다. §5의 권고는 **짓지 않는 것**이며, 그것도 가정이
아니라 결정되어야 할 사항이다. §5가 기대는 측정은 이 문서와 함께 착지했으므로
(`two_references_to_one_object_each_pay_the_forward_once`) 권고는 믿는 것이
아니라 확인하는 것이다.

> **Line references.** Everything cited is on `main` at `5470309`.
> *인용한 줄 번호는 `main`(`5470309`) 기준이다.*

---

## 1. What raised this / 무엇이 이 문서를 불렀나

`cd9f88f` made a permanent forward a fact about the **object** rather than
about whichever handle heard it: `Reference` gained
`moved: Arc<Guarded<Ior>>` (`crates/orbweaver-giop/src/pool.rs:857`), every
clone shares that cell, and a clone taken before the object moved is told.

It then reported, and did not build:

> **Two `Pool::reference` calls for one IOR still do not agree.** omniORB
> deduplicates by object key into a single `omniObjRef`, so a real ORB makes
> even those agree. Doing it here means an identity map in the pool keyed on
> endpoint plus object key, and a lifetime question that map does not answer by
> itself — when an entry goes, and what a re-pointed entry does to a reference
> nobody holds any more. Not attempted.

That is the question this document decides. Two things about the paragraph
above turned out to matter, and both are measured in §2: the cost is smaller
than "do not agree" suggests, and **the claim about omniORB is not what omniORB
does**. A decision written from the sentence rather than from the measurement
would have built the map on a premise that is false.

*`cd9f88f`은 영구 포워드를 "핸들의 사실"이 아니라 "객체의 사실"로 만들었지만,
`Pool::reference`를 두 번 부른 두 레퍼런스는 여전히 서로에게서 배우지 않는다고
보고하고 짓지 않았다. 그것을 여기서 결정한다. 그 보고문의 두 가지가 중요했고 둘
다 §2에서 측정됐다 — 비용은 "서로 모른다"가 시사하는 것보다 작고, **omniORB에
대한 주장은 omniORB가 하는 일이 아니다.** 측정이 아니라 문장에서 출발했다면 거짓
전제 위에 지도를 지었을 것이다.*

---

## 2. What the gap costs, measured / 격차의 비용 — 실측

### 2.1 Ours: one request per reference, once / 우리 쪽

The shape is the one a caller actually has: a servant that has moved an object
and forwards **every** request it is given — `LOCATION_FORWARD_PERM`, not just
for the first caller — and a landing that answers. Both peers count. Three
references created independently from the same IOR (`Pool::reference`, not
`Clone`), seven calls through them: the first calls once, the second five
times, and a third is created **after** the other two have been re-pointed and
calls once.

| | requests at the address the object left | requests at the object |
|---|---|---|
| reply `Big` | **3** | 7 |
| reply `Little` | **3** | 7 |

Three, not seven: a second reference pays its own forward on its **first** call
and then re-points itself, because `note` writes the permanent hop into the
cell this reference owns (`pool.rs:954`–`pool.rs:958`) exactly as it would into
a shared one. So the cost of the gap is **one request per independently created
reference, once in that reference's life** — not one per call, which is what
"do not agree" reads like and what the shape in `cd9f88f`'s own second
measurement (a template cloned per call: three calls, three forwards, never
converging) actually was before it was fixed.

The number is asserted at the peers rather than at the client because §9.6
leaves the old address valid: every one of the seven calls is **answered**. The
count is the only thing the two behaviours differ in.

*모양은 실제 호출자가 갖는 모양이다. 객체가 떠난 주소는 받는 요청마다 영구
포워드를 돌려주고, 착지점은 답한다. 같은 IOR로 **독립 생성**한 레퍼런스 셋, 호출
일곱 번 — 떠난 주소에 도달한 요청은 두 바이트 순서 모두 **3**, 객체에 도달한
요청은 7. 일곱이 아니라 셋인 이유는, 두 번째 레퍼런스가 자기 **첫** 호출에서
포워드를 한 번 물고 스스로 재지정되기 때문이다. 따라서 비용은 호출당이 아니라
**독립 생성된 레퍼런스당 한 번**이다. 일곱 번 모두 응답된다 — 차이는 오직 수뿐이다.*

### 2.2 omniORB in the same shape: also three / omniORB도 셋

`cd9f88f`'s argument for building was that a real ORB does better. Measured
2026-08-21, omniORB 4.3.4 (omniORBpy, macOS): **it does not.**

Two `string_to_object` calls on one IOR string, a third after the move, against
`crates/orbweaver-object/src/bin/spike_server.rs` forwarding with
`ORBWEAVER_FORWARD_STATUS=permanent` — the same seven calls through three
proxies as §2.1:

| | requests at the address the object left | requests at the object |
|---|---|---|
| omniORB 4.3.4, `LOCATION_FORWARD_PERM` | **3** | 7 |
| omniORB 4.3.4, `LOCATION_FORWARD` | **3** | 7 |
| ours, both reply byte orders | **3** | 7 |

Stable over three runs of each. `a._is_equivalent(b)` answers **true** for the
two proxies — they are agreed to name one object — and each still pays its own
forward exactly once. Whatever omniORB deduplicates internally, **the
observable behaviour is one forward per independently created reference**, and
that is what a client's user gets. The premise "a real ORB makes even those
agree" is refuted by the ORB it names.

The experiment is a separate process over TCP driven through `omniidl`-built
stubs, never a dependency (CLAUDE.md, licensing boundary). It is **not
committed as a gate** — §8 records that as unmeasured-here rather than implying
it will be re-run.

*`cd9f88f`의 건설 논거는 "진짜 ORB는 더 잘한다"였다. 2026-08-21 실측, omniORB
4.3.4는 **그렇게 하지 않는다.** 같은 IOR 문자열로 `string_to_object`를 두 번, 이동
후 한 번 더 — 프록시 셋, 호출 일곱, 떠난 주소에 도달한 요청 **3**. 영구·임시
상태 모두 같고, 각 3회 실행에서 안정적이다. `_is_equivalent`는 **참**을
답하면서도 프록시마다 포워드를 한 번씩 문다. 즉 **관측되는 동작은 독립 생성
레퍼런스당 포워드 한 번**이고, 그것이 사용자가 받는 것이다. 실험은 TCP 너머 별개
프로세스이며 의존성이 아니다. 게이트로 커밋되지 않았고, 그 사실은 §8에 적는다.*

### 2.3 Who creates two references today: nothing / 오늘 둘을 만드는 것

Twelve `Pool::reference` call sites, re-measured 2026-08-21, and **every one of
them is under `crates/*/tests/`**:

| Where | Count |
|---|---|
| `crates/orbweaver-giop/tests/forward_clone.rs` | 8 |
| `crates/orbweaver-giop/tests/forward_chain.rs` | 2 |
| `crates/orbweaver-giop/tests/mux_pool.rs` | 1 |
| `crates/orbweaver-gen/tests/forward_fallback.rs` | 1 |

Zero under `crates/*/src/`, zero under `crates/*/src/bin/`, zero under
`spikes/`. Nothing in this tree creates **one** `Reference` in product code, so
nothing creates two for one object.

And the path where an object reference would first meet a caller who did not
choose it — the agent boundary — does not produce a `Reference` at all. A
capability handle resolves to an `Ior`
(`crates/orbweaver-mcp/src/handles.rs:248`), which is used for authorisation
and for reference-valued arguments; the call itself goes out over a
`Connection` the caller supplied (`crates/orbweaver-mcp/src/lib.rs:565`,
`lib.rs:1259`, `crates/orbweaver-dynamic/src/invoke.rs:105`). A handle does not
resolve to a fresh `Reference` per call — it never becomes one. So the gap is
latent twice over: no product caller of `Pool::reference`, and the one
subsystem that hands references around bypasses the pool entirely.

*`Pool::reference` 호출 지점 12곳, 2026-08-21 재측정, **전부** `crates/*/tests/`
아래다. `src/`·`src/bin/`·`spikes/`에는 0. 즉 운영 코드는 레퍼런스를 **하나도**
만들지 않으므로 한 객체에 둘을 만들 리도 없다. 그리고 남이 고르지 않은 레퍼런스를
처음 만나게 될 경로 — 에이전트 경계 — 는 `Reference`를 아예 만들지 않는다. 능력
핸들은 `Ior`로 풀리고, 호출은 호출자가 준 `Connection` 위로 나간다. 핸들은 호출마다
새 `Reference`로 풀리는 것이 아니라, 애초에 `Reference`가 되지 않는다. 격차는 두
겹으로 잠재적이다.*

---

## 3. The lifetime question, and the trap under it / 수명 문제와 그 아래의 함정

An identity map keyed on `(endpoint, object key)` answers "which references
name one object". It does not answer three things, and the third is the one
that would have been discovered by a test rather than by design:

1. **When an entry goes.** A map that keeps every object ever referenced is
   unbounded, in a module whose own docs say *"An unbounded pool is a
   file-descriptor leak with a nicer name"* (`pool.rs:101`) and which bounds
   connections twice over for that reason. A leak measured in IORs is still a
   leak.
2. **What a re-pointed entry means for a reference nobody holds.** §9.6 leaves
   the old address valid, so a surviving entry can only ever save a round trip;
   it can never be needed for correctness. It is a cache with no invalidation
   signal — nothing tells a client that a servant has stopped forwarding.
3. **Which IOR the shared cell holds.** This is the trap. The cell is an
   `Ior`, and `pool::Key` includes the profile's **version and published
   codeset** as well as the endpoint (`pool.rs:15`–`pool.rs:35`). Two IORs can
   name the same `(endpoint, object key)` and carry different `TAG_CODE_SETS`
   components — that is exactly the case the key exists for. Under a naive map
   the second reference would silently adopt the first's profile, and go out on
   a connection whose codeset agreement it never published. That is D012 §3's
   failure class arriving through a different door: **not an error, a wrong
   string.** A sound map therefore shares `Option<Ior>` — `None` until a
   permanent hop, so an un-moved reference keeps the IOR it was handed — and
   only ever publishes what a servant actually said.

*`(엔드포인트, 객체 키)` 지도는 "어떤 레퍼런스들이 한 객체를 가리키는가"에
답하지만 세 가지에 답하지 않는다. (1) **언제 항목이 사라지는가** — 무한 지도는
연결 대신 IOR로 재는 누수이고, 이 모듈은 바로 그 이유로 연결을 두 번 묶는다.
(2) **아무도 안 쥔 항목의 재지정은 무슨 뜻인가** — §9.6이 옛 주소를 유효하게
두므로 왕복 한 번을 아낄 뿐 정확성에 필요한 적이 없다. 무효화 신호 없는 캐시다.
(3) **공유 칸은 어느 IOR을 담는가** — 여기가 함정이다. `pool::Key`는 엔드포인트
외에 **버전과 공표된 코드셋**을 포함하는데, 같은 `(엔드포인트, 키)`를 가리키면서
`TAG_CODE_SETS`가 다른 IOR이 존재할 수 있다. 순진한 지도는 두 번째 레퍼런스에게
첫 번째의 프로파일을 조용히 입히고, 공표한 적 없는 코드셋 합의 위로 문자열을
내보낸다 — 오류가 아니라 **틀린 문자열**, D012 §3의 실패 부류가 다른 문으로
들어온 것이다. 건전한 지도는 `Option<Ior>`를 공유해야 한다.*

---

## 4. The options / 선택지

### A. A weak identity map / 약한 참조 지도

`Pool` holds `(endpoint, object key) -> Weak<Guarded<Option<Ior>>>`.
`Pool::reference` upgrades the entry if a live reference still holds it and
shares the cell; otherwise it files a fresh one. An entry whose last holder
dropped fails to upgrade and is swept, the way `sweep` already drops dead
connections (`pool.rs:650`).

- **The lifetime rule it states**, which is the point of choosing it: *an
  entry lives exactly as long as some `Reference` holds it, and a re-pointing
  is forgotten when the last one goes.* Nothing is cached on behalf of a caller
  who does not exist.
- **What it buys, exactly**: one request, once, for each reference that is
  alive at the same time as one that has already moved. In §2.1's shape,
  `at_old` goes 3 → 1: the second reference stops paying, and the third —
  created while the others are still alive — stops paying too.
- **What it costs**: a map sized by live references, a lookup and an `Arc`
  upgrade per `Pool::reference` call (not per wire call), the sweep, and §3's
  `Option<Ior>` discipline, which is not optional.
- **Acceptance criteria**, written now so a future batch does not invent them:
  `two_references_to_one_object_each_pay_the_forward_once` asserts `at_old` = 1
  and `at_new` = 7 under both reply byte orders; **plus** a new test that drops
  every reference, creates one more, and measures that it pays a forward again —
  which is the only observable that distinguishes A from B and C, and therefore
  the only thing that pins the lifetime rule rather than describing it.

*`Pool`이 `(엔드포인트, 객체 키) -> Weak<Guarded<Option<Ior>>>`를 쥔다. 수명
규칙: **항목은 어떤 `Reference`가 쥐고 있는 동안만 살고, 마지막 홀더가 사라지면
재지정도 잊힌다.** 얻는 것은 정확히 "이미 이동을 아는 레퍼런스와 동시에 살아 있는
레퍼런스마다 요청 한 번" — §2.1에서 3 → 1. 비용은 살아 있는 레퍼런스 수만큼의
지도, `Pool::reference`마다의 조회와 업그레이드, 스윕, 그리고 §3의 `Option<Ior>`
규율(선택이 아니다). 수용 기준은 지금 적어 둔다 — `at_old` = 1, `at_new` = 7, 그리고
**전부 떨어뜨린 뒤 새로 만든 레퍼런스가 다시 포워드를 문다**는 테스트. 그것만이 A를
B·C와 구분하는 관측이다.*

### B. A map that never evicts / 회수하지 않는 지도

Every object ever referenced keeps an entry, so a reference created after the
last one died still learns the move.

- **What it buys over A**: one request, once, per reference created after every
  earlier one is gone — the third reference in §2.1 if it had been created
  later.
- **What it costs**: an unbounded map, contradicting `pool.rs:101` in the same
  file. A leak that is small per entry is still a leak with no bound, and this
  pool refuses to dial rather than exceed a bound (`Error::PoolExhausted`)
  precisely so that "small per entry" never becomes the argument.

*모든 객체가 항목을 영구 보존한다. A보다 얻는 것은 "앞선 레퍼런스가 모두 죽은 뒤
생성된 레퍼런스마다 요청 한 번". 비용은 무한 지도이며, 같은 파일 `pool.rs:101`과
정면으로 모순된다. 이 풀은 한도를 넘느니 다이얼을 거절하는 쪽을 택한 물건이다.*

### C. Bounded survivors — an LRU of re-pointings / 한도 있는 생존자(LRU)

B with a cap and an eviction order.

- **What it buys**: B's benefit until the cap is reached.
- **What it costs**: a second bound to choose with no measurement to choose it
  from — `pool.rs:186`–`pool.rs:191` already says the number governing spreading
  is a guess — and a cache holding a servant's statement with no way to learn
  it has been withdrawn (§3.2). The failure it can produce is a reference that
  starts at an address the servant abandoned *twice*: forwarded once by a
  stale cache entry, then forwarded again. Nothing goes red; the count goes up.

*B에 한도와 회수 순서를 붙인 것. 얻는 것은 한도까지의 B. 비용은 근거 없는 두 번째
숫자와(같은 파일이 이미 "이 숫자는 추측"이라고 적고 있다), 철회를 알 길 없는 캐시다.
만들 수 있는 실패는 폐기된 주소에서 **두 번** 출발하는 레퍼런스이며, 붉어지는 것은
없고 숫자만 올라간다.*

### D. Build nothing; the boundary stays documented / 짓지 않고 경계를 적어 둔다

What the tree does today. `Pool::reference`'s own doc already says it
(`pool.rs:624`–`pool.rs:629`): *"two of them for the same IOR are two
references that happen to name one object, and a permanent forward taken by one
says nothing about the other."* §2.1's test is now the measured half of that
sentence.

- **What it buys**: nothing built before its trigger, and no identity structure
  added to a module whose docs are mostly about what must **not** be shared.
- **What it costs**: one request per independently created reference, once —
  measured, and equal to what omniORB charges (§2.2).

*오늘의 트리. `Pool::reference`의 문서가 이미 그렇게 말하고 있고, §2.1의 테스트가
그 문장의 측정된 반쪽이 되었다. 얻는 것은 방아쇠 전에 아무것도 짓지 않는 것.
비용은 독립 생성 레퍼런스당 요청 한 번이며, 그것은 omniORB가 물리는 것과 같다.*

### Summary / 요약

| | Lifetime rule | Extra requests saved | What it costs |
|---|---|---|---|
| **A** | entry lives while a `Reference` holds it | one per co-living reference | map + upgrade per `reference()`; §3's `Option<Ior>` is mandatory |
| **B** | never | A's, plus one per later-created reference | unbounded map, against `pool.rs:101` |
| **C** | LRU | B's, up to a cap | a bound with no measurement; a cache with no invalidation |
| **D** | none to state | none | one request per reference, once — omniORB's own number |

*A는 홀더가 있는 동안, B는 영원히, C는 한도까지, D는 지도가 없다. 아끼는 요청은
차례로 "동시에 사는 레퍼런스마다 한 번", "그 위에 나중 생성마다 한 번", "한도까지",
"없음"이고, 비용은 지도·무한·근거 없는 숫자·요청 한 번이다.*

---

## 5. Recommendation / 권고

**Adopt D — build nothing — with the trigger in §6, and record now that A is
the shape if the trigger fires.**

Three arguments, in the order of their weight:

1. **The measured cost is one request per reference, once, and the reference
   ORB charges the same** (§2.1, §2.2). Building A would not close a gap
   against omniORB; it would open one in the other direction, and the thing
   `cd9f88f` proposed to copy is not what omniORB was measured to do. A design
   whose stated motivation is refuted by measurement should not be built on the
   motivation anyway.
2. **Nothing in the tree creates a `Reference` in product code** (§2.3), and
   the subsystem most likely to hold many references to one object — the agent
   boundary — does not use the pool at all. This project's own rule is that
   building before the trigger is the defect, not the omission (`PLAN-DEFERRED`
   §0's trigger table; D010 §5).
3. **A's whole benefit is bounded by the number it saves.** One request, once,
   per co-living reference. That is a round trip on an established pooled
   connection, against an identity map, a weak-reference sweep and a mandatory
   `Option<Ior>` discipline (§3.3) whose absence produces a wrong string rather
   than an error. The ratio is not close.

**Prefer A over B and C if the trigger fires**, and this document says so now so
the future batch does not re-derive it: A is the only option that can *state*
its lifetime rule in one sentence and *test* it (§4.A's second acceptance
criterion), and B and C both keep a servant's statement alive on behalf of
nobody, which §9.6 gives them no way to revalidate.

*권고: **D** — 짓지 않는다. §6의 방아쇠와 함께. 근거는 (1) 측정된 비용이
레퍼런스당 요청 한 번이고 **참조 ORB도 같은 값을 물린다** — 베끼자던 대상이 그렇게
하지 않는다, (2) 운영 코드는 `Reference`를 하나도 만들지 않으며 레퍼런스를 많이
쥘 법한 유일한 서브시스템(에이전트 경계)은 풀을 쓰지 않는다, (3) A의 이득 전체가
"동시에 사는 레퍼런스당 왕복 한 번"인데 그 대가가 지도·스윕·필수 규율이다. 비율이
가깝지 않다. **방아쇠가 당겨지면 B·C가 아니라 A**이며, 이유는 A만이 수명 규칙을 한
문장으로 말하고 테스트로 고정할 수 있기 때문이다.*

---

## 6. The trigger / 방아쇠

Observable, in `PLAN-DEFERRED` §0's form — an event, not a feeling:

> **The first caller outside `crates/*/tests/` that creates more than one
> `Reference` for one object** — concretely, product code that calls
> `Pool::reference` twice with IORs naming the same `(endpoint, object key)`,
> such as a client that resolves a name per call rather than holding what it
> resolved. `two_references_to_one_object_each_pay_the_forward_once` is then no
> longer measuring a hypothetical, and its `at_old` is the deployment's own
> forward count.

A second trigger, which changes the **class** of the cost rather than its size:

> **A peer measured to retire the old address rather than keep forwarding.**
> §9.6 permits the old address to stay valid and every measurement here — ours
> and omniORB's — is taken against a peer that honours that. Against a servant
> that stops answering the abandoned address, a reference that has not been
> told is not one extra request, it is a failed call, and D's cost stops being
> a round trip. Re-argue before building, not after.

*방아쇠 둘: (1) 테스트 밖에서 한 객체에 대해 `Reference`를 둘 이상 만드는 첫
호출자 — 예컨대 호출마다 이름을 다시 푸는 클라이언트. 그 순간 §2.1의 테스트는
가정이 아니라 그 배포의 포워드 수를 재는 계기가 된다. (2) **옛 주소를 유지하지 않고
폐기하는 피어가 측정되는 순간** — 여기의 모든 측정은 §9.6대로 옛 주소를 살려 두는
피어를 상대로 얻은 것이다. 폐기하는 서번트 앞에서 못 들은 레퍼런스는 "요청 한 번"이
아니라 **실패한 호출**이고, D의 비용은 왕복이 아니게 된다. 짓기 전에 다시 논한다.*

---

## 7. What approval would mean / 승인의 의미

1. **It approves not building.** No identity map on `Pool`, and a future batch
   that wants one needs §6's first trigger — except under §6's second trigger,
   which explicitly reopens this.
2. **It approves A as the shape, in advance, for the day a trigger fires**,
   with §4.A's acceptance criteria — including the drop-everything test, which
   is what makes the lifetime rule a measured claim instead of a sentence.
3. **It records B and C as argued and rejected**, so the never-evict and LRU
   shapes are not re-proposed by inspection of the map A would add.
4. **It approves the correction in §2.2 as the standing fact about omniORB**:
   independently created references there pay one forward each, `_is_equivalent`
   notwithstanding. Any future argument from "omniORB deduplicates" must
   re-measure first.
5. **It changes no measured document.** `COMPONENTS.md` records what is
   measured now and `CHANGELOG.md` what changed; this file restates neither,
   per CLAUDE.md's *where a fact lives*.

*승인은 (1) **짓지 않음**을 승인하고, (2) 방아쇠가 당겨진 날의 모양으로 A를
수용 기준(특히 "전부 떨어뜨린 뒤" 테스트)과 함께 미리 승인하며, (3) B·C를 논증 후
기각으로 기록하고, (4) §2.2의 정정을 omniORB에 대한 상시 사실로 승인하며(향후
"omniORB는 중복 제거한다"는 논거는 재측정부터 해야 한다), (5) 측정 문서는 아무것도
바꾸지 않는다.*

---

## 8. What would falsify this, and what is unmeasured / 반증과 미측정

- **"Zero product callers" is a count over this tree, not over the world**
  (§2.3). It is the claim most likely to go stale, and §6's first trigger
  exists because of that. Re-measured 2026-08-21 it is twelve sites, all under
  `crates/*/tests/`; D012 §2 names `spike_mux.rs` in the same list, and that
  binary uses `Pool::invoke` directly and calls `reference` nowhere. The
  fact both statements turn on — **zero callers outside tests** — is the same
  either way.
- **The omniORB result is one ORB, one version, one platform.** omniORB 4.3.4
  through omniORBpy on macOS, three runs per status. JacORB is a fixture in
  this tree (`spikes/jacorb/`) and was **not** driven in this shape; a second
  witness would make §2.2 a property of ORBs rather than of omniORB, and its
  absence is why §5's first argument is stated as "the reference ORB" and not
  as "every ORB".
- **The omniORB experiment is not committed.** It ran as scratch — two
  `spike-server` processes and a `string_to_object`-twice client — and nothing
  in `spikes/` re-takes it. Under CLAUDE.md's rule that a peer's bytes are
  re-taken live, a number that cannot be re-taken is a number with a date on
  it, and §2.2's date is what it is worth. Committing it as a
  `spikes/perm_fallback.sh` mode is the obvious home and was outside this
  batch's footprint.
- **`spikes/run_checks.sh` does not run `forward_clone`.** The harness runs
  `forward_chain` (`run_checks.sh:1057`) and the pool group
  (`run_checks.sh:1039`) beside it; this file gates only under `cargo test`,
  so §2.1's measurement is not in the harness's verdict. Reported by `cd9f88f`
  and still true; the amendment is recommended, not applied.
- **A's cost is reasoned, not measured.** No benchmark exists for a map lookup
  and an `Arc` upgrade per `Pool::reference` call, because there is no product
  caller to benchmark. If the trigger fires, that measurement comes before A
  does.
- **The falsifier for the whole recommendation is a number.** If a deployment
  measures `at_old` growing faster than one per reference — references created
  in a loop, or a peer that retires the abandoned address — D's cost is not
  what §2 says and A is owed a re-argument, not a build.

*미측정: (1) "운영 호출자 0"은 이 트리의 계수일 뿐이다(D012 §2의 목록과 오늘의
재측정이 한 항목에서 다르지만, 두 문장이 기대는 사실 — **테스트 밖 0** — 은 같다).
(2) omniORB 결과는 ORB 하나·버전 하나·플랫폼 하나이며 JacORB로는 이 모양을 돌리지
않았다. (3) omniORB 실험은 커밋되지 않았다 — 다시 뜰 수 없는 숫자는 날짜가 붙은
숫자이고, §2.2의 값어치는 그 날짜만큼이다. (4) `run_checks.sh`는 `forward_clone`을
돌리지 않는다(보고는 하되 고치지 않음, 발자국 밖). (5) A의 비용은 계산이지 측정이
아니다 — 잴 운영 호출자가 없기 때문이며, 방아쇠가 당겨지면 그 측정이 A보다 먼저
온다. (6) 전체 권고의 반증은 숫자다 — `at_old`가 레퍼런스당 하나보다 빨리 자라는
배포가 측정되면 §2의 비용은 사실이 아니고, A는 건설이 아니라 재논증을 받는다.*
