# Orbweaver

**AI-driven CORBA/IDL interface automation — from natural-language spec to a live ORB binding, with no hand-written stubs.**
**AI 기반 CORBA/IDL 인터페이스 자동화 — 자연어 명세에서 실제 ORB 연동까지, 손으로 쓴 스텁 없이.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Status: planning](https://img.shields.io/badge/status-planning-orange.svg)](docs/PLAN.md)
[![Spec: OMG IDL 4.2](https://img.shields.io/badge/spec-OMG%20IDL%204.2-1D4C5C.svg)](https://www.omg.org/spec/IDL/4.2/)

> **Status / 상태** — Planning stage. No code yet. The full plan lives in [`docs/PLAN.md`](docs/PLAN.md) (English) and [`docs/PLAN.ko.md`](docs/PLAN.ko.md) (한국어). Phase 0 is a three-week feasibility spike whose outcome may still reshape this architecture.
> 기획 단계입니다. 아직 코드는 없습니다. 전체 계획은 [`docs/PLAN.md`](docs/PLAN.md)(영문)과 [`docs/PLAN.ko.md`](docs/PLAN.ko.md)(국문)에 있습니다. Phase 0은 3주짜리 타당성 검증이며, 그 결과에 따라 아키텍처가 바뀔 수 있습니다.

---

## The idea / 착안점

CORBA already has what AI agents need, and nobody noticed.

CORBA는 AI 에이전트가 필요로 하는 것을 이미 갖고 있습니다. 아무도 주목하지 않았을 뿐입니다.

The Model Context Protocol standardized runtime tool discovery in 2025: a client calls `tools/list` and gets back a live catalog of callable operations with their schemas. CORBA shipped that in 1996 — it is called the **Interface Repository**, and the matching call path is the **Dynamic Invocation Interface**. Together with `TypeCode` and `DynAny`, they let a caller discover an interface it has never seen and invoke it correctly, at runtime, with zero generated code.

MCP는 2025년에 런타임 도구 발견을 표준화했습니다. 클라이언트가 `tools/list`를 호출하면 호출 가능한 연산 목록과 스키마를 받아옵니다. CORBA는 이것을 1996년에 이미 제공했습니다 — **Interface Repository**이고, 대응하는 호출 경로가 **Dynamic Invocation Interface**입니다. `TypeCode`, `DynAny`와 함께라면 한 번도 본 적 없는 인터페이스를 런타임에 발견해 정확히 호출할 수 있습니다. 생성된 코드는 한 줄도 필요 없습니다.

Three propositions drive this project:

이 프로젝트를 움직이는 세 가지 명제:

| # | Proposition | 명제 |
|---|---|---|
| 1 | CORBA is a runtime self-describing type system. IFR + TypeCode + DII/DSI + DynAny is an agent tool catalog that already exists. | CORBA는 런타임 자기기술 타입 시스템이다. IFR + TypeCode + DII/DSI + DynAny 조합이 곧 에이전트 도구 카탈로그다. |
| 2 | For an LLM, IDL's verbosity is an asset, not a cost. The strictness humans fled from is exactly what makes generated interfaces verifiable — the IDL compiler is a ground-truth oracle. | LLM에게 IDL의 장황함은 비용이 아니라 자산이다. 인간이 도망친 그 엄격함이 생성 결과를 검증 가능하게 만든다 — IDL 컴파일러가 정답 채점기다. |
| 3 | The bottleneck is specification quality, not code generation. So the first-class deliverable is a semantic annotation vocabulary, not a code generator. | 병목은 코드 생성이 아니라 명세 품질이다. 따라서 1급 산출물은 코드 생성기가 아니라 의미 어노테이션 규약이다. |

Proposition 3 is not a guess. [AutoMCP](https://arxiv.org/html/2507.16044v2) compiled 5,066 endpoints across 50 real APIs into MCP servers: 76.5% worked out of the box, rising to 99.9% after an average of 19 lines of *specification* fixes per API. The failures were spec defects — missing security schemes (62%), undocumented runtime headers (47%), malformed base URLs (41%) — not generator bugs.

명제 3은 추측이 아닙니다. [AutoMCP](https://arxiv.org/html/2507.16044v2)는 실제 API 50개·엔드포인트 5,066개를 MCP 서버로 컴파일해 즉시 동작 76.5%, API당 평균 19줄의 *명세* 수정 후 99.9%를 달성했습니다. 실패 원인은 생성기 버그가 아니라 명세 결함이었습니다 — 보안 스키마 누락 62%, 미문서화 런타임 헤더 47%, 잘못된 base URL 41%.

---

## What it does / 무엇을 하는가

```
  자연어 요구사항            IDL 4.2 + 의미 어노테이션         살아있는 연동
  natural-language     ──▶   annotated IDL contract    ──▶   live binding
  requirement                (compiler-verified)              (no stubs)
```

| Stage | 단계 | Input → Output | Target |
|---|---|---|---|
| **S1** Ingest | 흡수 | requirements / legacy source / existing IDL → IR | 95% |
| **S2** Synthesize | 합성 | IR → OMG IDL 4.2 draft | 90% |
| **S3** Annotate | 의미부착 | IDL → SIDL (`@ai_*` semantics) | 80% |
| **S4** Validate | 검증 | SIDL → compile gate + self-repair loop | 100% |
| **S5** Register | 등록 | SIDL → type registry + semantic index | 100% |
| **S6** Bind | 연동 | catalog → dynamic call, or generated stubs | 85% |
| **S7** Verify | 검증·운영 | binding → contract tests, interceptors, tracing | 90% |

**S4 is the safety belt.** An LLM writes plausible IDL that may be wrong; an IDL compiler rejects wrong IDL every single time. That asymmetry — *generative synthesis, deterministic verification* — is the trust model this whole system rests on.

**S4가 안전벨트입니다.** LLM은 그럴듯하지만 틀릴 수 있는 IDL을 씁니다. IDL 컴파일러는 틀린 IDL을 100% 거부합니다. 이 비대칭 — *생성은 확률적으로, 검증은 결정론적으로* — 이 시스템 전체의 신뢰 모델입니다.

---

## SIDL — the semantic layer / 의미 계층

IDL nails syntax and says nothing about meaning. `long transfer(in long acct, in long amt)` is perfectly typed and tells an agent nothing about whether the unit is won or cents, whether it is idempotent, whether it is destructive, or whether the argument is PII.

IDL은 구문은 완벽히 잡지만 의미는 말하지 않습니다. `long transfer(in long acct, in long amt)`는 타입은 완벽하지만, 단위가 원인지 센트인지, 멱등한지, 파괴적인지, 인자가 PII인지 — 에이전트가 알아야 할 것은 하나도 담지 못합니다.

SIDL closes that gap using OMG IDL 4.x's own `@annotation` construct, so no non-standard extension is needed.

SIDL은 OMG IDL 4.x의 표준 `@annotation` 문법으로 이 간극을 메웁니다. 비표준 확장이 필요 없습니다.

```idl
// sidl_annotations.idl
@annotation ai_desc       { string  text;  };  // intent in prose        / 자연어 의도
@annotation ai_unit       { string  unit;  };  // KRW, meter, ms         / 단위
@annotation ai_effect     { string  kind;  };  // pure|read|write|destructive
@annotation ai_idempotent { boolean value; };  // retry safety           / 재시도 안전성
@annotation ai_pii        { string  level; };  // none|low|high
@annotation ai_example    { string  json;  };  // few-shot material      / few-shot 재료
@annotation ai_precond    { string  expr;  };  // test-generation source / 테스트 생성 재료
@annotation ai_authz      { string  scope; };  // required permission    / 필요 권한

module bank {
  @ai_desc("Transfers funds between accounts. Rolls back in full on failure.")
  interface Transfer {
    @ai_effect("destructive") @ai_idempotent(FALSE)
    @ai_authz("bank.transfer.write")
    void execute(
      @ai_pii("high") in long from,
      @ai_pii("high") in long to,
      @ai_unit("KRW") in long amount
    ) raises (InsufficientFunds, AccountFrozen);
  };
};
```

One vocabulary drives both directions: at runtime it is the tool description an agent reads; at build time it is the source for generated contract tests and guardrails.

하나의 어휘가 양방향을 구동합니다. 런타임에는 에이전트가 읽는 도구 설명이고, 빌드 타임에는 계약 테스트와 가드레일을 생성하는 근거입니다.

---

## Dual-path binding / 이중 경로 연동

Pure code generation breaks automation the moment a schema changes — every change means regenerate and redeploy. Pure dynamic invocation is too slow for a hot path. So Orbweaver runs both and promotes between them.

순수 코드 생성은 스키마가 바뀌는 순간 자동화가 깨집니다 — 변경마다 재생성·재배포가 필요합니다. 순수 동적 호출은 임계 경로에 쓰기엔 느립니다. 그래서 둘 다 운영하고 사이에서 승격시킵니다.

| | **Dynamic path / 동적 경로** | **Static path / 정적 경로** |
|---|---|---|
| Mechanism | DII + DynAny | generated stubs / 생성 스텁 |
| Code generated | none / 없음 | full / 전체 |
| Schema change | adapts automatically / 자동 적응 | regenerate + redeploy / 재생성·재배포 |
| Latency | higher / 높음 | lowest / 최저 |
| Best for | discovery, experiments, low-frequency / 탐색·실험·저빈도 | hot paths, real-time / 임계 경로·실시간 |

**Promotion criteria / 승격 조건** — ≥1,000 calls/day **and** schema unchanged for 30 days **and** regression suite green. Explore dynamically, settle statically.

일 1,000회 이상 호출 **그리고** 스키마 30일 무변경 **그리고** 회귀 스위트 통과. 탐색은 동적으로, 정착은 정적으로.

---

## Licensing stance / 라이선스 방침

**This project is MIT-licensed, and every component we ship is MIT or MIT-equivalent — or we write it ourselves.** That constraint has a hard consequence worth stating plainly:

**이 프로젝트는 MIT이며, 배포하는 모든 구성요소는 MIT 또는 MIT 동등이거나 직접 구현합니다.** 이 제약에는 분명히 밝혀 둘 결과가 하나 있습니다.

> **No CORBA ORB is available under MIT.** Verified 2026-08.
> **MIT 라이선스로 제공되는 CORBA ORB는 존재하지 않습니다.** 2026-08 확인.

| Project | License (verified) | Verdict |
|---|---|---|
| omniORB / omniORBpy | LGPL (libraries) + GPL (tools) | ❌ excluded / 배제 |
| JacORB | LGPL | ❌ excluded / 배제 |
| GlassFish CORBA | EPL / GPLv2+CPE | ❌ excluded / 배제 |
| MICO | GPL / LGPL | ❌ excluded / 배제 |
| ACE / TAO | DOC License — permissive, MIT-equivalent in effect, but not an SPDX-recognized MIT | ⚠️ not literally MIT / 문자 그대로는 MIT 아님 |
| `foxglove/omgidl` | **MIT** | ✅ usable / 사용 가능 |
| `tier4/idl_parser`, `eProsima/IDL-Parser` | Apache-2.0 | ⚠️ permissive but not MIT / 관대하나 MIT 아님 |
| `sugarsweetrobotics/idl_parser`, `asenac/idl-parser` | **no license declared** | ❌ unusable / 사용 불가 |

So the ORB core is built in-house. **This is less painful than it sounds, because interoperability does not require a license.** GIOP/IIOP is a published OMG specification; implementing the wire protocol creates no obligation to TAO, omniORB, or anyone else. Existing ORBs are therefore demoted from *dependencies* to *interoperability test fixtures* — run in throwaway containers during CI, never linked, never redistributed.

따라서 ORB 코어는 직접 구현합니다. **들리는 것만큼 뼈아프지 않은 이유가 있습니다 — 상호운용에는 라이선스가 필요하지 않기 때문입니다.** GIOP/IIOP는 공개된 OMG 명세이며, 와이어 프로토콜을 구현하는 것은 TAO나 omniORB에 대해 어떤 의무도 발생시키지 않습니다. 기존 ORB는 *의존성*에서 *상호운용 테스트 픽스처*로 강등됩니다 — CI에서 일회성 컨테이너로 띄우고, 링크하지 않고, 재배포하지 않습니다.

---

## Planned components / 계획 구성요소

Everything below is MIT and written in this repository unless marked otherwise.

아래는 별도 표기가 없는 한 모두 MIT이며 본 저장소에서 직접 작성합니다.

| Component | 구성요소 | Scope |
|---|---|---|
| `orbweaver-cdr` | CDR 인코더 | OMG CDR encoding/decoding, both endiannesses |
| `orbweaver-giop` | GIOP/IIOP 전송 | GIOP 1.0–1.2 messages, IIOP over TCP, IOR parse/emit |
| `orbweaver-poa` | 객체 어댑터 | Servant lifecycle, object activation, request dispatch |
| `orbweaver-idl` | IDL 컴파일러 | OMG IDL 4.2 front end, `@annotation` support, pluggable back ends |
| `orbweaver-registry` | 타입 레지스트리 | IFR-equivalent store; also ingests remote IFRs |
| `orbweaver-dynamic` | 동적 호출 | DII/DSI/DynAny equivalents; lossless JSON ↔ CORBA `any` |
| `orbweaver-forge` | 명세 파이프라인 | S1–S5: ingest, synthesize, annotate, validate, register |
| `orbweaver-mcp` | MCP 브릿지 | Projects the registry as MCP `tools/list`; delegates calls |
| `orbweaver-guard` | 가드레일 | Interceptor chain: authz, dry-run, approval, audit log |
| `orbweaver-gen` | 정적 생성 | Static generation: stubs, skeletons, scaffolds, client SDKs |
| `orbweaver-test` | 계약 테스트 | Contract/property tests from annotations; DynAny fuzzing |
| `orbweaver-console` | 웹 콘솔 | Catalog browser, contract diff viewer, invocation traces |

---

## Roadmap / 로드맵

Roughly 45 weeks. Building the ORB core in-house adds about 15 weeks over an adopt-an-ORB plan; the licensing constraint buys full MIT freedom in exchange.

약 45주. ORB 코어 자체 구현은 기존 ORB 채택 대비 약 15주를 추가합니다. 라이선스 제약의 대가로 완전한 MIT 자유도를 얻습니다.

| Phase | Weeks | Focus | 내용 |
|---|---|---|---|
| **0** | 3 | Feasibility spike — **gates everything** | 타당성 검증 — 전체의 관문 |
| **1** | 10 | `orbweaver-cdr` + `orbweaver-giop` + IOR; interop against TAO/omniORB containers | 와이어 프로토콜 코어 및 상호운용 |
| **2** | 8 | `orbweaver-idl` (IDL 4.2 + `@annotation`) + `orbweaver-registry` + POA | IDL 컴파일러·타입 레지스트리·POA |
| **3** | 10 | `orbweaver-dynamic` + `orbweaver-forge` + `orbweaver-mcp` — **the headline demo** | 동적 호출·AI 파이프라인·MCP 브릿지 |
| **4** | 8 | Static generation, multi-target back ends, promotion engine, contract tests | 정적 생성·다중 타깃·승격 엔진 |
| **5** | 6 | TLS transport, observability, governance, web console, pilot | 보안·관측·거버넌스·파일럿 |

### Phase 0 gates the project / Phase 0가 프로젝트의 관문

Four assumptions get tested before anything else is built. Two of them can invalidate the architecture.

무엇을 만들기 전에 네 가지 가정을 먼저 검증합니다. 그중 둘은 아키텍처를 무효화할 수 있습니다.

- **A — GIOP interop is reachable.** Hand-encode a GIOP 1.2 `Request` and get a correct reply from a stock TAO and omniORB server. *If a minimal ORB cannot interoperate, the in-house path fails and the MIT-only constraint must be revisited.*
  **GIOP 상호운용이 가능한가.** GIOP 1.2 `Request`를 직접 인코딩해 순정 TAO·omniORB 서버로부터 정상 응답을 받아낸다. *최소 ORB가 상호운용되지 않으면 자체 구현 경로가 무너지고 MIT 전용 제약을 재검토해야 한다.*
- **B — LLMs write compilable IDL.** 20 requirements → IDL. Target ≥60% first-pass compile, ≥95% within three self-repair rounds.
  **LLM이 컴파일되는 IDL을 쓰는가.** 요구사항 20건 → IDL. 목표 1차 통과 ≥60%, 자가수정 3회 내 ≥95%.
- **C — `@annotation` survives real toolchains.** Most deployed ORB compilers are CORBA 2.x/3.x era and may reject IDL 4 annotations. *Fallback: structured comments plus a sidecar YAML — viable because we own the parser.*
  **`@annotation`이 실제 툴체인에서 통과하는가.** 배포된 ORB 컴파일러 대부분은 CORBA 2.x/3.x 세대라 IDL 4 어노테이션을 거부할 수 있다. *폴백: 구조화 주석 + 사이드카 YAML — 파서를 우리가 소유하므로 가능하다.*
- **D — IOR addressing works under NAT/containers.** IORs embed addresses; a container's internal IP makes them uncallable from outside. Verify endpoint rewriting under Kubernetes early.
  **NAT·컨테이너 환경에서 IOR 주소가 동작하는가.** IOR에는 주소가 박히므로 컨테이너 내부 IP가 들어가면 외부에서 호출할 수 없다. K8s 환경의 endpoint 재작성을 조기에 검증한다.

---

## Targets / 목표 지표

| Metric | 지표 | Baseline | Target |
|---|---|---|---|
| Time to define a new interface | 신규 인터페이스 정의 | 3–10 days | **< 1 hour** |
| Time to bind a new service (dynamic) | 신규 연동 (동적) | 2–4 weeks | **< 10 min** |
| IDL first-pass compile rate | IDL 1차 컴파일 통과율 | — | **≥ 85%** |
| Compile rate within 3 self-repairs | 자가수정 3회 내 통과율 | — | **≥ 98%** |
| Semantic annotation coverage | 어노테이션 커버리지 | 0% | **≥ 90%** |
| Contract tests auto-generated | 계약 테스트 자동 생성률 | 0% | **≥ 80%** |
| Breaking changes caught pre-merge | 파괴적 변경 사전 탐지율 | manual | **100%** |
| Human intervention across pipeline | 파이프라인 사람 개입 비율 | 100% | **≤ 15%** |

---

## Why CORBA in 2026 / 왜 지금 CORBA인가

Not nostalgia. Three concrete reasons.

향수가 아닙니다. 구체적인 이유가 셋 있습니다.

1. **The legacy is load-bearing and not going away.** Naval combat systems, command and control, telecom switching, air traffic control, core banking, large physics installations. These are systems where rewriting is not an option.
   **레거시가 하중을 지탱하고 있고 사라지지 않습니다.** 함정 전투체계, 지휘통제, 통신 교환기, 항공관제, 금융 코어, 대형 물리실험 설비. 재작성이 선택지가 아닌 시스템들입니다.
2. **OMG IDL 4.x is shared with DDS.** Korean defense programs are standardizing on DDS-based middleware, and DDS-XTypes uses the same IDL. One pipeline serves both — which turns the shrinking CORBA market from a risk into an expansion path.
   **OMG IDL 4.x는 DDS와 공유됩니다.** 국내 국방은 DDS 기반 미들웨어로 표준화 중이고, DDS-XTypes는 같은 IDL을 씁니다. 하나의 파이프라인이 양쪽을 커버하므로, 축소되는 CORBA 시장이 리스크가 아니라 확장 경로가 됩니다.
3. **Java severed its own connection.** [JEP 320](https://openjdk.org/jeps/320) removed `java.corba` and `javax.rmi.CORBA` in JDK 11, so Java legacy now needs a third-party ORB just to keep running. That migration is itself demand for automation.
   **Java는 스스로 연결을 끊었습니다.** [JEP 320](https://openjdk.org/jeps/320)이 JDK 11에서 `java.corba`와 `javax.rmi.CORBA`를 제거해, Java 레거시는 동작만 유지하려 해도 서드파티 ORB가 필요합니다. 그 마이그레이션 자체가 자동화 수요입니다.

And the reason that matters most: interfaces are increasingly called by agents rather than people. In that world a contract that is verbose but precise beats one that is terse but ambiguous. The complexity humans rejected in CORBA is the precision agents need.

그리고 가장 중요한 이유 — 인터페이스를 호출하는 주체가 점점 사람에서 에이전트로 옮겨가고 있습니다. 그 세계에서는 장황하지만 정밀한 계약이 간결하지만 모호한 계약을 이깁니다. 인간이 거부했던 CORBA의 복잡성이, 에이전트에게는 필요한 정밀성입니다.

---

## Documentation / 문서

| Document | 문서 | Contents |
|---|---|---|
| [`docs/PLAN.md`](docs/PLAN.md) | Development plan (English) | Full technical plan, research findings, risk register |
| [`docs/PLAN.ko.md`](docs/PLAN.ko.md) | 개발 계획서 (한국어) | 전체 기술 계획, 조사 결과, 리스크 목록 |
| [`docs/plan-page.html`](docs/plan-page.html) | Rendered plan | Standalone HTML version of the plan |

## References / 참고 자료

**Standards / 표준**
[OMG IDL 4.2](https://www.omg.org/spec/IDL/4.2/) ·
[CORBA 3.4 Interoperability (GIOP/IIOP)](https://www.omg.org/spec/CORBA/3.4/Interoperability/PDF) ·
[MCP Tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) ·
[JEP 320](https://openjdk.org/jeps/320)

**Reference implementations (interop targets, not dependencies) / 참조 구현 (상호운용 대상, 의존성 아님)**
[DOC Group ACE/TAO](https://github.com/DOCGroup/ACE_TAO) ·
[omniORB](https://omniorb.sourceforge.io/docs.html) ·
[JacORB](https://github.com/JacORB/JacORB)

**IDL tooling / IDL 툴링**
[foxglove/omgidl (MIT)](https://github.com/foxglove/omgidl) ·
[tier4/idl_parser (Apache-2.0)](https://github.com/tier4/idl_parser) ·
[Remedy IT RIDL](https://www.remedy.nl/opensource/ridl.html)

**Research / 연구**
[AutoMCP — Making REST APIs Agent-Ready](https://arxiv.org/html/2507.16044v2) ·
[OOPS — LLM-generated REST API specifications](https://www.sciencedirect.com/science/article/abs/pii/S0164121226001470) ·
[AgentModernize](https://arxiv.org/pdf/2605.17535)

---

## Contributing / 기여

The project is in its planning phase, so the most valuable contribution right now is a challenge to an assumption — particularly the four in Phase 0. Issues and discussion are welcome in either English or Korean.

기획 단계이므로 지금 가장 가치 있는 기여는 가정에 대한 반론입니다 — 특히 Phase 0의 네 가지. 이슈와 토론은 영어와 한국어 모두 환영합니다.

## License / 라이선스

MIT — see [`LICENSE`](LICENSE). Dependencies are held to the same bar: MIT or MIT-equivalent, or written here.

MIT — [`LICENSE`](LICENSE) 참조. 의존성도 동일 기준을 적용합니다: MIT 또는 MIT 동등이거나, 여기서 직접 작성합니다.
