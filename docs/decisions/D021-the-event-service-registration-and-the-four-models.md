# D021 — The event service: registration, management, and the four models

**STATUS: PROPOSED** — drafted 2026-08-25 on a request to plan the event
service's registration and management functions, and to make the push / pull /
mixed models creatable. Every count below was read from the generated block of
`SERVICES-COVERAGE.md` §8 and from `crates/orbweaver-giop/src/event_server.rs`
on that date. Not self-approvable: §3 turns on whether a deferral's trigger has
fired, and §6 changes what a running server can be told to do.

**상태: 제안** — 2026-08-25, 이벤트 서비스의 등록·관리 기능과 push/pull/혼합
모델 생성을 기획하라는 요청에서 작성.

---

## 1. The four models, measured / 네 모델, 측정

CosEvent's models are a 2×2: each side is either pushed to or pulled from, and
the channel is the other half of both. Four `obtain_*` operations select them.

| Supplier side | Consumer side | Operations | Today |
|---|---|---|---|
| push | push | `obtain_push_consumer` + `obtain_push_supplier` | **served** |
| push | pull | `obtain_push_consumer` + `obtain_pull_supplier` | **served** (the pull half landed 2026-08-18) |
| **pull** | push | `obtain_pull_consumer` + `obtain_push_supplier` | **blocked** |
| **pull** | pull | `obtain_pull_consumer` + `obtain_pull_supplier` | **blocked** |

**Two of four work. The other two are blocked by exactly one shape** — the
channel acting as a *pull consumer of a supplier*, i.e. the channel pulling
events out rather than being pushed them. Three operations carry it, all
`NO_IMPLEMENT` today:

- `SupplierAdmin::obtain_pull_consumer`
- `ProxyPullConsumer::connect_pull_supplier`
- `PullConsumer::disconnect_pull_consumer`

That is the whole gap between two models and four. It is not four separate
pieces of work; it is one.

*네 모델은 2×2다. 둘은 되고, 둘은 **정확히 한 모양**에 막혀 있다 — 채널이
공급자로부터 **끌어오는** 쪽. 연산 셋이 그것을 나른다.*

## 2. The deferral, and whether its trigger has fired / 유예와 방아쇠

`PLAN-DEFERRED` §10 defers the supplier side of pull. Its reason was split once
already — the document records that *"the original reason was two claims … and
only the second survived measurement"* — and what survived is:

> **Trigger.** A named `PullSupplier` in this workspace — something that *is*
> one.

Re-measured 2026-08-25, whole tree: **nothing in this workspace is a
`PullSupplier`.** Every hit is the channel's own outbound `ProxyPullSupplier`,
a test that mints one, or the sweep's note that the interface is
client-implemented.

**So the honest question this document must not dodge:** does a request to make
the models creatable fire that trigger?

The reading this document proposes: **it fires it, and the trigger's own words
say why.** A deferral phrased as *"until something in this workspace is one"*
is a bet that nobody will ask; the moment a consumer asks for the model, the
bet has been called. The alternative reading — that the trigger requires an
*existing* `PullSupplier` object before the channel may be able to pull from
one — makes the trigger unreachable by construction, because nobody writes a
pull supplier against a channel that cannot obtain a pull consumer.

**This is the user's to settle**, and it is the only thing in this document
that is: a trigger that fires on a request rather than on a measurement is a
different kind of trigger, and CLAUDE.md's rule is that building before a
trigger fires is the defect. §7 states what happens under each answer.

*방아쇠는 "이 워크스페이스에 `PullSupplier`인 것이 있을 때"이고, 오늘 재측정
결과 없다. **요청이 그 방아쇠를 당기는가**가 이 문서가 피하면 안 되는 질문이다.
"아무도 묻지 않을 것"이라는 내기였다면, 물은 순간 판돈이 불린 것이다. 반대로
읽으면 방아쇠는 구조적으로 도달 불가능해진다 — pull consumer를 얻을 수 없는
채널을 상대로 pull supplier를 쓰는 사람은 없기 때문이다.*

## 3. Registration — what the standard actually provides / 등록

Three routes exist and only two are ours to take.

- **`NotificationService` as an initial reference.** §8.5.2's reserved-ObjectId
  table (verified 2026-08-25) maps `NotificationService` →
  `CosNotifyChannelAdmin::EventChannelFactory`, from the Notification Service
  specification (`formal/00-06-20`). **That factory is CosNotification's, not
  CosEvent's** — `CosEventChannelAdmin` defines no factory at all — and
  CosNotification is `PLAN-DEFERRED` §1 with a trigger re-measured today as
  **not fired**. So the standard's own answer to "how is a channel registered"
  is a service we deliberately do not have.
- **CosNaming.** The available standard route, and it needs **no new IDL**: a
  channel publishes its IOR under a name, a client resolves the name. We serve
  CosNaming 14/14. This is what registration should mean here.
- **`resolve_initial_references("NameService")`.** D019 step 1 is building the
  table that makes this answer; once it does, a client bootstraps to the naming
  service and finds channels by name with nothing further invented.

**Proposal: registration is CosNaming, and the ORB's initial-references table
is how a client reaches it.** No factory, no new interface, and the reason
CosNotification's factory is absent is recorded rather than worked around.

## 4. Management — what exists and where it stops / 관리

`EventChannelServer` holds **one channel**: `host`, `port`, `base`,
`consumer_admin`, `supplier_admin`, and one `Arc<Shared>`. There is no map, so
a process is a channel. Runtime management exists but only in Rust:

- `ChannelHandle::stats()` → `ChannelStats`, which is genuinely rich —
  `accepted`, `fanned_out`, and drops **split by cause** (`dropped_overflow`,
  `unrelayable`, `dropped_on_disconnect`, `dropped_on_failure_disconnect`,
  `dropped_at_stop`), with `by_cause()` and `split_adds_up()`.
- `set_queue_limit`, `set_pull_block` — tunable at runtime, **through the Rust
  handle only**, so an operator cannot reach them.
- `stop()`.
- `destroy` over the wire answers `NO_IMPLEMENT` (`PLAN-DEFERRED` §11).

**And §11's own v1 sketch presupposes what this document proposes.** It reads:
*"`destroy` allowed to the principal that created the channel, `NO_PERMISSION`
to others."* **"The principal that created the channel"** requires a creation
operation and a caller model. Today there is neither, which is why §11's
trigger is *a caller model reaching the servant*. Multi-channel creation does
not fire that trigger — it makes the sketch's first half meaningful and leaves
the second half (who is calling) exactly as absent as before.

## 5. What is proposed / 제안

Four batches, ordered so each is measurable on its own.

### E1 — the fourth side of the 2×2 (gated on §2)

`SupplierAdmin::obtain_pull_consumer`, `ProxyPullConsumer::connect_pull_supplier`
and `PullConsumer::disconnect_pull_consumer`, which makes pull/push and
pull/pull work and takes CosEvent from **14 of 18 served to 17 of 18** — the
remaining one being `destroy`, correctly deferred.

The shape: the channel becomes a *client* of a supplier's `PullSupplier`,
calling `pull`/`try_pull` on a schedule. That is a new outbound direction for
this servant and it is where the design work is — the bounded queue, the
drop-by-cause accounting and `MAX_CONSECUTIVE_FAILURES` all already exist and
should carry it rather than being duplicated.

**Oracle.** omniORB's Python client can be a pull supplier: the fixture drives
our channel to pull from it, both byte orders. That is a peer measurement, not
a self-test, which is the standard everything else in this service was held to.

### E2 — many channels in one server

`EventChannelServer` gains a map from a channel name to its `Shared`, and each
channel keeps its own admin keys derived from its name. `channel_ior()` becomes
per-channel. **No new wire interface**: creation is a Rust API and a
configuration entry, exactly as `Poa` creation is today.

**Compatibility.** A server constructed the current way is a server with one
channel named by its existing `base` key; every existing spike and test keeps
working. That is the rule the MCP `--config` batch proved and D020 Stage A
applies: *absent is not zero.*

### E3 — registration through CosNaming

Each channel publishes its IOR under a name in the naming context we already
serve, and the harness measures a client that resolves the name and connects —
ideally reaching it through `resolve_initial_references("NameService")` once
D019 step 1 lands, so the whole bootstrap is standard from the client's side.

### E4 — management reaches an operator

`ChannelStats` and the two tunables exist and stop at the Rust boundary. Two
honest routes, and this document does **not** choose between them:

- **Configuration** (D019 step 3's shape): queue limit, pull block and push
  timeout become deployment settings rather than constants. Cheap, no new wire
  surface, and consistent with where the other knobs are going.
- **A contract** (D004's shape): stats become an operation an operator's tool
  can call. That is a new interface, and by this project's rules it needs a
  consumer that names it — the console renders what it is given and has not
  asked.

**Recommendation: E4 as configuration now, a contract only if something asks.**

## 6. What must not happen / 해서는 안 되는 것

- **Do not invent a channel factory.** `CosEventChannelAdmin` has none;
  `CosNotifyChannelAdmin::EventChannelFactory` is CosNotification's and is
  deferred. An Orbweaver-specific factory interface would be a fifth wire
  surface nobody asked for, and E2 achieves creation without one.
- **Do not let E2 change `destroy`'s answer.** Its deferral turns on a caller
  model, not on channel count. Creation makes its sketch meaningful and does
  not fire its trigger; conflating the two would build an unauthenticated
  remote operation that ends a channel for every other client, which is the
  exact sentence §11 refuses.
- **Do not build E1 before §2 is answered.** If the reading in §2 is wrong,
  E1 is a class-C defect — built before its trigger — and this document would
  be the thing that caused it.

## 7. The two answers, and what each means / 두 답과 각각의 의미

- **If the request fires §10's trigger**: E1 proceeds, `PLAN-DEFERRED` §10 is
  closed with the date and the reason it fired recorded there, and E2–E4 follow
  in order. CosEvent goes to 17 of 18.
- **If it does not**: E2, E3 and E4 proceed anyway — none of them touches the
  pull direction — and §10's trigger is **re-dated with the new reading
  written down**, so the next person to ask meets an argument rather than a
  sentence from 2026-08-19. That is a smaller result and still a real one.

*두 답 중 어느 쪽이든 E2–E4는 진행된다. 차이는 E1과, §10의 방아쇠 문장이
그대로 남는지 아니면 오늘의 읽기를 담아 다시 쓰이는지다.*
