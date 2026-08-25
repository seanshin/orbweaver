# Flows — the paths, drawn / 흐름 — 경로를 그린 것

Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md), which describes the system
in prose. **This file draws paths and states no facts of its own.** Where a
claim belongs to another document it is linked, not repeated — a diagram that
restates a fact drifts from it exactly like a sentence does, and nothing
compiles a picture either.

Every symbol below was read from the tree on 2026-08-25, and the one gap marked
in red is the subject of [`decisions/D019`](decisions/D019-the-orb-has-no-object.md).

[`ARCHITECTURE.md`](ARCHITECTURE.md)의 동반 문서. **이 파일은 경로를 그리고
자기 사실은 말하지 않는다.** 다른 문서에 속한 주장은 링크하지 다시 적지 않는다 —
사실을 다시 적은 그림은 문장과 똑같이 어긋나고, 그림을 컴파일하는 것도 없다.

---

## 1. Crate ownership / 크레이트 소유

Who owns what, and which way the dependencies point. The rule that shapes this
is in [`ARCHITECTURE.md` §1](ARCHITECTURE.md); the sizes are `wc -l` over
`src/`, 2026-08-25.

```mermaid
flowchart TB
  subgraph wire["wire — 와이어"]
    cdr["orbweaver-cdr<br/>1.2k · CDR encode/decode"]
    giop["orbweaver-giop<br/>23.5k · GIOP · IOR · TypeCode<br/>Server · Dispatch · Pool · mux<br/>naming + event servants"]
  end
  subgraph types["types — 타입"]
    idl["orbweaver-idl<br/>7.4k · lex · parse · sema · include"]
    reg["orbweaver-registry<br/>10.0k · types as data · IFR facade<br/>§5.3 differ"]
  end
  subgraph objects["objects — 객체"]
    obj["orbweaver-object<br/>9.7k · POA · references<br/>residency · tenancy"]
  end
  subgraph above["above the wire — 와이어 위"]
    dyn["orbweaver-dynamic<br/>5.7k · values · DII/DSI · AnyJSON"]
    gen["orbweaver-gen<br/>6.0k · stubs · skeletons · Python"]
    forge["orbweaver-forge<br/>9.5k · S1–S5 pipeline"]
    mcp["orbweaver-mcp<br/>17.9k · agent boundary<br/>triad · handles · interceptors"]
    trade["orbweaver-trading<br/>2.1k · offers · constraints"]
    console["orbweaver-console<br/>3.9k · renders, decides nothing"]
  end
  cdr --> giop
  giop --> obj
  giop --> dyn
  idl --> reg
  reg --> gen
  reg --> forge
  reg --> mcp
  dyn --> mcp
  obj --> mcp
  trade --> mcp
  mcp --> console
```

## 2. The client call path / 클라이언트 호출 경로

From a string a person holds to bytes on a socket. **The red node is the one
break**: the URL parser understands `corbaloc:rir:` and nothing answers it.

```mermaid
flowchart LR
  s1["a string<br/>IOR:&lt;hex&gt; · corbaloc: · corbaname:"] --> p{"which form?"}
  p -->|"IOR:&lt;hex&gt;"| ior["Ior::parse<br/>giop/lib.rs:676"]
  p -->|"corbaloc: / corbaname:"| url["ObjectUrl::parse<br/>giop/naming.rs"]
  url --> tio["ObjectUrl::to_ior<br/>naming.rs:148"]
  tio -->|"Corbaloc · Corbaname<br/>address given"| ior
  tio -->|"InitialReference<br/>name only"| gap["returns None<br/>naming.rs:152"]:::gap
  ior --> ref["Reference<br/>giop/pool.rs"]
  ref --> pool["Pool::invoke<br/>pool.rs:488"]
  pool --> conn["Connection<br/>· codeset · version cap"]
  conn --> mux["mux — GIOP 1.2 only<br/>giop/mux.rs"]
  mux --> enc["encode_request<br/>giop/lib.rs"]
  enc --> tcp(["TCP"])
  classDef gap fill:#7f1d1d,stroke:#ef4444,color:#fff
```

- `Corbaname` resolves *a name inside a naming service whose address you were
  given*; `InitialReference` is the case where **no address is given** and the
  ORB is supposed to know. That is the whole difference, and the whole gap.
- What a reply does on the way back — `LOCATION_FORWARD`, fragment
  reassembly, `CloseConnection` mid-reply — is
  [`ARCHITECTURE.md` §2](ARCHITECTURE.md).

*`Corbaname`은 **주소를 받은** 네이밍 서비스 안에서 이름을 푼다.
`InitialReference`는 **주소가 없는** 경우이고, ORB가 알고 있어야 하는 경우다.
그 차이가 곧 공백이다.*

## 3. The server dispatch path / 서버 디스패치 경로

```mermaid
flowchart LR
  tcp(["TCP"]) --> bind["Server::bind<br/>giop/server.rs:1099"]
  bind --> serve["serve / serve_shared<br/>server.rs:1223 · :1253"]
  serve --> rm["read_message<br/>giop/lib.rs:1237"]
  rm --> dr["decode_request<br/>server.rs:317"]
  dr --> disp["trait Dispatch::dispatch<br/>server.rs:625"]
  disp --> poa["Poa<br/>object/lib.rs"]
  poa -->|"Located::Here"| serv["servant"]
  poa -->|"Located::Forward"| fwd["LOCATION_FORWARD<br/>temporary · permanent"]
  poa -->|"Located::Unknown"| unk["OBJECT_NOT_EXIST"]
  serv --> er["encode_reply<br/>server.rs:404"]
  fwd --> er
  unk --> er
  er --> tcp
```

The servants that ship in-tree — `naming_server`, `event_server` (in
`orbweaver-giop`), `expert_service`, `tenant_service` (in `orbweaver-object`) —
are `Dispatch` implementations. What each serves and refuses, with the wire's
own answer per operation, is the generated block of
[`SERVICES-COVERAGE.md` §8](SERVICES-COVERAGE.md).

## 4. The agent path / 에이전트 경로

A requirement becomes a contract, both halves are generated, and a call is made
under a guard. Stages and their gates are
[`ARCHITECTURE.md` §4](ARCHITECTURE.md); the pipeline's own gates are in
[`PLAN.md`](PLAN.md) §5.

```mermaid
flowchart TB
  req["a requirement<br/>corpus/requirements/"] --> s1["S1–S3 · forge<br/>infer · annotate"]
  s1 --> idl["IDL + SIDL comments<br/>//@ ai_desc · ai_effect · ai_authz"]
  idl --> s4["S4 · sidl-validate<br/>syntax · semantics · fix hints"]
  s4 --> regy["registry<br/>types as data"]
  regy --> s5["S5 · register"]
  regy --> gen["orbweaver-gen<br/>Rust stubs + skeletons<br/>Python clients"]
  s5 --> cat["catalog<br/>exposure off by default"]
  cat --> agent(["agent"])
  agent --> bridge["mcp::Bridge"]
  bridge --> chain["interceptor chain<br/>audit · telemetry · exposure<br/>scopes · (quota) · approval"]
  chain --> dyn["orbweaver-dynamic<br/>AnyJSON ↔ CDR"]
  chain -.->|"static path"| gen
  dyn --> pool["Pool::invoke"]
  gen --> pool
  pool --> wire(["the wire"])
```

`(quota)` is drawn in parentheses because the seat has an occupant that
`Chain::standard` deliberately does not install — the reason is written at the
seat, in `mcp/interceptor.rs`.

## 5. What D019 proposes, on the same paths / D019가 제안하는 것

The four responsibilities of [`decisions/D019`](decisions/D019-the-orb-has-no-object.md)
§5, drawn where they attach. Green is the object; the red node is §2's break,
closed by step 1.

```mermaid
flowchart TB
  subgraph orb["the ORB object — D019 §5"]
    tab["initial references table<br/>step 1"]:::new
    sto["string_to_object<br/>object_to_string<br/>step 2"]:::new
    cfg["the seven numbers<br/>max message · fragment threshold<br/>forward hops · follow timeout<br/>max fragments · max connections<br/>stop poll — step 3"]:::new
    hand["hands out transport + root POA<br/>step 4 · atomic, two crates"]:::new
  end
  rir["corbaloc:rir:NameService"] --> tab
  tab --> ans["an IOR, or a refusal naming the key"]
  sto --> ior["Ior"]
  cfg --> conn["Connection · Server"]
  hand --> asm["12 hand-assembly sites<br/>become callers"]
  classDef new fill:#064e3b,stroke:#34d399,color:#fff
```

Steps 1–3 are one crate each and default to today's behaviour. **Step 4 is the
one the approval phrase gates**, because it is where the API becomes one-way.
The sequencing, with each step's oracle and negative control, is D019 §8.

## 6. Where the flows are measured / 흐름이 측정되는 곳

Not a claim about coverage — a map from a path above to the thing that runs it.

| Path | Measured by |
|---|---|
| §2 client, §3 server | `spikes/service_sweep.sh` (every declared operation over the wire), `spikes/differential.sh` |
| §2 forwarding | `spikes/perm_fallback.sh`, `giop/tests/forward_*.rs` |
| §2 wide text, versions | `spikes/jacorb_giop11.sh`, `jacorb_wchar11.sh`, `wide_rust.sh` |
| §3 concurrency | the harness's five-run dispatch group |
| §4 end to end | `spikes/end_to_end.sh`, `spikes/estate/run.sh` |
| §5 step 1 | **a peer resolving `corbaloc:rir:NameService`** — the reason step 1 is first |

The harness runs all of these and its exit code is the verdict; what it cannot
measure it prints as a counted `SKIPPED` group naming the fixture.
