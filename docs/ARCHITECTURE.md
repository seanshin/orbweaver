# Architecture — as built / 아키텍처 — 지어진 대로

**This document describes the system that exists, not the one that is planned.**
`PLAN.md` is the plan and ages against intent; `COMPONENTS.md` is the ledger and
records what is measured; neither describes the *shape*. This file is the shape,
and it is written from the code rather than from the design that preceded it.
Where the built thing disagrees with the plan, the built thing is what is here.

**이 문서는 계획이 아니라 존재하는 시스템을 기술한다.** `PLAN.md`는 계획이고,
`COMPONENTS.md`는 측정 원장이며, 어느 쪽도 *구조*를 말하지 않는다. 여기가 구조다.
설계가 아니라 코드에서 옮겨 적었고, 지어진 것이 계획과 다르면 지어진 것을 적었다.

---

## 1. The crate graph, and the one rule that shapes it / 크레이트 그래프

Eleven first-party crates and **two external ones** — `encoding_rs` (EUC-KR,
behind the default-on `euc-kr` feature, disclosed under D001) and its `cfg-if`.
That number is a design output, not an accident: `cargo tree` is checked, and
every proposal to add a third has produced a decision document instead
(D002 rustls, D003 embeddings, D004 observability — the first is behind an
off-by-default feature, the other two adopted nothing).

```
cdr ──────► giop ──────► registry ──────► dynamic ──────► mcp ──────► forge
             ▲             │  ▲              │             │  ▲
  idl ───────┴─────────────┘  │              └─────────────┘  │
                              │                               │
                     trading ─┴──► object            gen ─────┘   test ──► (all)
```

- `cdr` — CDR encode/decode. Knows nothing above it.
- `giop` — GIOP/IIOP framing, IOR, `TypeCode`, `Server`/`Dispatch`, the
  CosNaming and CosEvent servants.
- `idl` — the IDL 4.2 front end and SIDL structured comments. Depends on
  nothing, which is why the parser can be reused without dragging the wire in.
- `registry` — types and interfaces as data, plus the read-only IFR facade and
  remote IFR ingestion.
- `dynamic` — value-driven marshalling, the DII/DSI equivalent, AnyJSON.
- `object` — POA, references, the MoE residency machine and its wire surface,
  tenancy.
- `trading` — the offer store and loading policy. Depends only on `registry`.
- `mcp` — the agent boundary: the tool triad, capability handles, the guard's
  interceptor chain, dry-run, promotion statistics.
- `forge` — the specification pipeline S1–S5.
- `gen` — static generation: client stubs and server skeletons.
- `test` — property and contract checks, and the wire fuzz.

**The rule: dependencies point away from the wire, never back.** `registry`
depends on `giop`; `giop` must never depend on `registry`. This is not
tidiness. It is what makes the ORB core usable without the AI layer, and it has
already decided a design: the read-only Interface Repository facade serves
`CORBA::Repository` over `giop`'s `Server`, and therefore lives in `registry`
rather than in `giop`, because a facade in `giop` would have inverted the edge.
The batch that built it checked the direction before writing code and said so.

**규칙: 의존은 와이어에서 멀어지는 방향으로만 간다.** 정돈의 문제가 아니라, AI
계층 없이도 ORB 코어를 쓸 수 있게 하는 조건이며, 이미 설계를 하나 결정했다 — IFR
파사드는 `giop`이 아니라 `registry`에 산다.

---

## 2. The wire path / 와이어 경로

```
TCP ─► read_message ─► RawMessage ─► decode_request ─► Dispatch ─► servant
                          │                                          │
                     reassembly                                 encode_reply
```

Three facts about this path decide most of its bugs, and all three are
load-bearing enough to have caused one:

- **Alignment origin.** A GIOP message aligns from the first byte of its
  12-byte header; an encapsulation restarts at its own first byte. A value
  built in a detached buffer that starts at offset zero is the mistake this
  project has made most often, and the fix is always `Encoder::continuing_at`.
  The server hands a skeleton an origin of 24, which is already 8-aligned —
  so the server-side oracle also dispatches at origin 20, where a zero-origin
  bug becomes visible.
- **Reassembly is concatenation, in 1.2 only.** GIOP 1.2 fragments form one
  logical stream, so reassembly appends payloads and rewrites the header. GIOP
  1.1 restarts alignment per fragment and carries no request id, so it is
  refused rather than guessed at. No available peer emits fragments, which is
  why reception is measured against hand-built streams from §9.4.9 rather than
  against a peer.
- **Concurrency stops at dispatch.** Connections are served concurrently, one
  thread each, capped at 64 with the refusal spoken as §9.4.7's
  `CloseConnection`. Dispatch itself is serialized behind one servant, so a
  slow operation delays every client though it no longer excludes them.

**정렬 원점**, **1.2 한정 재조립**, **디스패치에서 멈추는 동시성** — 이 세 사실이
이 경로의 버그 대부분을 결정한다.

---

## 3. The type path / 타입 경로

IDL text → `idl::parse` → `Registry` → `TypeCode` → `dynamic::{encode,decode}`.
Nothing downstream ever parses IDL again; the registry is the single
representation, which is why the same `TypeCode` serves generated stubs, the
dynamic invoker, the IFR facade and the property tests.

Two representational choices matter more than they look:

- **A cycle is a repository id, not a pointer.** Rust cannot hold the cycle, so
  `TypeCode::Recursive(id)` names the type it points back at. The marshaller
  resolves it against the enclosing types the *error path* is already standing
  on — a marker naming a `typedef` resolves to the alias, which is the spelling
  the front end actually produces. Nesting is bounded at 64 in both directions,
  because on decode the depth is the sender's choice.
- **A `valuetype` is registered as an object reference and is not one.** The
  registry represents it that way so `_is_a` and catalogue lookups work without
  implying a wire form v1 does not have. Anything asking "is this a live
  reference?" must ask the registry what the id *names*, not what the
  `TypeCode` looks like. Two false positives in the contract checker came from
  trusting the representation.

**사이클은 포인터가 아니라 repository id다.** 그리고 **valuetype은 객체 참조로
등록되지만 객체 참조가 아니다** — 표현을 믿지 말고 레지스트리에 그 id가 무엇을
가리키는지 물어야 한다.

---

## 4. The agent path / 에이전트 경로

```
agent ─► MCP triad ─► Chain ─► Invoker ─► giop ─► target
          │            │
     capability     audit + telemetry
      handles
```

`search_interfaces` → `describe_interface` → `invoke_operation` is the whole
surface an agent sees. Under it:

- **An agent never holds a dialable address.** Capability handles are
  session-scoped, expiring, 128-bit, and typed; an IOR, host, port or object
  key reaching a transcript is a tested failure, not a review comment.
- **The chain decides, once.** Every policy question is answered by one ordered
  interceptor chain — exposure, scopes, approval, telemetry, audit — and
  `Chain::run` and `Chain::dry_run` are two wrappers around one walk, so a
  preview cannot drift from the gate it previews.
- **Registration order is not acting order.** Observers register outermost so
  their `after` still runs when a gate ahead of them refuses; the acting order
  comes back out as the standard stack. An audit stage registered last would
  never see a refusal.
- **A refusal never reaches the wire.** The guard answers `NO_PERMISSION`
  before anything is sent, and the audit line it writes holds nothing dialable.

**에이전트는 다이얼 가능한 주소를 쥐지 않는다.** 그리고 **체인은 한 번만
결정한다** — 미리보기와 실제 게이트가 같은 walk를 감싸므로 어긋날 수 없다.

---

## 5. Trust boundaries / 신뢰 경계

Four, and naming them is most of the security design. Each has a different
answer to "what could this input do if it were hostile?"

| Boundary | The input | What it could do | What holds it |
|---|---|---|---|
| **The wire** | bytes from any peer | crash the process, or decode into the wrong value | `unsafe_code = "forbid"` covers memory; panics are separately measured by `wire-fuzz` over the decoders a peer reaches before any policy runs; sizes and nesting are bounded before allocation |
| **The model** | generated IDL and annotations | put a contract we did not write in front of an agent | S4 gates syntax and semantics and refuses; the contract checker gates *meaning* and only advises, because no checker can prove prose true; S5 registers with exposure **off** |
| **The remote IR** | a peer describing types | overwrite a locally-defined contract, or hand us a name that is really markup | ingestion accepts the repository id as the sole identity and derives the rest; overwriting a local entry is refused at the registry boundary; provenance is marked and **contagious upwards** |
| **The agent** | tool calls | reach what it was not granted; escalate through a reference it was handed | default-deny exposure, per-operation `ai_authz`, approval for destructive effects, capability handles instead of IORs, and the rule that handing out an object reference is itself an authorization question |

The fourth row's last clause is the one people miss: **an object reference is a
bearer address**. An operation that returns one widens what its caller can
reach even if it changes nothing, and a reference inside a `sequence` is as
dialable as a reference returned directly — which is why the contract checker
descends into constructed types.

**객체 참조는 bearer 주소다.** 아무것도 바꾸지 않는 오퍼레이션이라도 참조를
건네면 호출자의 도달 범위를 넓히며, 시퀀스 안의 참조도 똑같이 다이얼된다.

---

## 6. Two planes / 두 평면

The MoE control plane (`PLAN-MOE.md`) draws the line this architecture already
implied: **CORBA is the control plane and is forbidden in the data plane.**
Expert registration, residency transitions, policy decisions and routing
metadata travel over GIOP; activations do not. The residency state machine is
built so that no per-token hook can exist — a `compile_fail` doctest proves it,
because the plane boundary is the kind of thing that erodes by convenience one
call site at a time.

**CORBA는 컨트롤 플레인이고 데이터 플레인에서는 금지된다.** 상주 상태 기계에는
토큰 단위 훅이 존재할 수 없고, 그 사실을 `compile_fail` doctest가 증명한다 —
평면 경계는 편의를 이유로 한 호출 지점씩 무너지는 종류의 것이기 때문이다.

---

## 7. Deliberate absences / 의도적 부재

Things that are missing on purpose. Each is a decision, and each would be an
improvement to somebody who did not know why:

- **No clock in the interceptor chain or the residency machine.** Trace replay
  is deterministic because of it; a duration nobody can reproduce is worse than
  an absent one. A batch that needs time takes it as an argument.
- **No writable Interface Repository.** The registry is populated from IDL
  through S4. A writable IFR would be a second, ungated ingestion path.
- **No pull model on the event channel.** It inverts flow control into the
  unbounded buffer the bounded queue exists to avoid.
- **No second policy in the console.** It renders what the registry and the
  audit say and decides nothing.
- **No SIDL on ingested contracts.** An annotation inferred from a foreign
  service is a claim, not a fact, so an ingested interface cannot satisfy the
  guard's gates by accident.

**의도적으로 없는 것들.** 각각은 결정이며, 이유를 모르는 사람에게는 전부 개선처럼
보인다.

---

## 8. How it is verified / 검증 구조

Five layers, weakest claim first. The point of the layering is that each one
catches a class the one below cannot:

1. **Unit tests** — a function does what its author meant.
2. **Property and fuzz** — seeded round-trips over every golden type at every
   alignment phase in both byte orders, and panic-freedom over the decoders a
   peer reaches first. Catches what nobody thought to write a case for.
3. **Self-consistency spikes** — our client against our server. Catches
   integration mistakes, and *cannot* catch a shared misreading of the
   specification: both ends are ours.
4. **Foreign peers** — omniORB 4.3.4 and JacORB 3.9, in both directions. This
   is the layer that finds specification misreadings, and it has: a transposed
   completion status survived every local test because both sides compared
   against the same enum, and only an ORB we did not write could disagree.
5. **Differential compilers** — omniidl and JacORB's IDL compiler over the
   whole corpus, with divergences recorded rather than reconciled.

The harness (`spikes/run_checks.sh`) is the merge gate, and its exit code is
the verdict. Two rules give the number meaning: **an unmeasured check is a
failure, never a pass** — absent fixtures are counted as skips and named — and
**a batch reports its first-pass rate and its round count separately**, because
they measure different things.

**약한 주장부터 다섯 계층.** 각 계층은 아래 계층이 잡을 수 없는 부류를 잡는다.
특히 4계층(외부 피어)만이 규격 오독을 잡는다 — 우리 양쪽 끝이 같은 오독을
공유하면 3계층까지는 전부 통과한다.

---

## 9. Where to look next / 다음에 볼 곳

| Question | File |
|---|---|
| What is measured, and what is missing | [`COMPONENTS.md`](COMPONENTS.md) |
| The plan and its parallel streams | [`PLAN.md`](PLAN.md) · [`PLAN.ko.md`](PLAN.ko.md) |
| The MoE control plane | [`PLAN-MOE.md`](PLAN-MOE.md) |
| The core CORBA services suite | [`PLAN-SERVICES.md`](PLAN-SERVICES.md) |
| What is excluded, and what would un-defer it | [`PLAN-DEFERRED.md`](PLAN-DEFERRED.md) |
| Why each dependency question was answered as it was | [`decisions/`](decisions/) |
| What each phase measured | `PHASE0.md` … `PHASE5.md` |
