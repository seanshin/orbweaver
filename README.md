# Orbweaver

**A CORBA ORB written from scratch under MIT, and an AI pipeline that turns a
written requirement into a contract it can actually serve.**

**밑바닥부터 MIT로 작성한 CORBA ORB, 그리고 문장으로 쓴 요구사항을 실제로 서빙
가능한 계약으로 바꾸는 AI 파이프라인.**

[![CI](https://github.com/seanshin/orbweaver/actions/workflows/ci.yml/badge.svg)](https://github.com/seanshin/orbweaver/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Peers: omniORB 4.3.4 · JacORB 3.9](https://img.shields.io/badge/peers-omniORB%204.3.4%20%C2%B7%20JacORB%203.9-1D4C5C.svg)](docs/COMPONENTS.md)
[![External crates: 2](https://img.shields.io/badge/external%20crates-2-2F6B4F.svg)](docs/decisions/D001-codeset-crate.md)
[![Spec: OMG IDL 4.2](https://img.shields.io/badge/spec-OMG%20IDL%204.2-1D4C5C.svg)](https://www.omg.org/spec/IDL/4.2/)

Current release **v0.7.0** (2026-08-26). The ORB interoperates with omniORB
4.3.4 and JacORB 3.9 in both directions at GIOP 1.0, 1.1 and 1.2. What changed
in each release is in [`CHANGELOG.md`](CHANGELOG.md); what is measured today is
in [`docs/COMPONENTS.md`](docs/COMPONENTS.md). Neither is restated here.

현재 릴리스는 **v0.7.0**(2026-08-26)입니다. omniORB 4.3.4·JacORB 3.9와 GIOP
1.0·1.1·1.2에서 양방향으로 상호운용됩니다. 릴리스별 변경은
[`CHANGELOG.md`](CHANGELOG.md)에, 오늘 측정된 값은
[`docs/COMPONENTS.md`](docs/COMPONENTS.md)에 있습니다. 이 파일은 둘 중 어느 것도
다시 적지 않습니다.

---

## How to read this / 이 문서 읽는 법

The README answers six questions and hands off to a document for anything that
needs a number. Numbers do not live here, because a number copied into a second
file drifts from the first one silently.

이 README는 여섯 가지 질문에 답하고, 수치가 필요한 것은 해당 문서로 넘깁니다.
수치는 여기 살지 않습니다 — 두 번째 파일로 복사된 수치는 조용히 어긋나기
때문입니다.

| | Question | 질문 | Section |
|---|---|---|---|
| **What** | What is this, concretely? | 이게 정확히 뭔가 | [What it is](#what-it-is--무엇인가) |
| **Why** | Why CORBA, in 2026, under MIT? | 왜 2026년에 CORBA를, 왜 MIT로 | [Why CORBA](#why-corba-and-why-now--왜-corba인가-왜-지금인가) |
| **How** | How does a sentence become a served object? | 문장이 어떻게 서빙되는 객체가 되나 | [How it works](#how-it-works--어떻게-동작하는가) · [How the work is run](#how-the-work-is-run--작업은-어떻게-도는가) |
| **When** | How far along is it, and what is open? | 어디까지 왔고 무엇이 열려 있나 | [Where it stands](#where-it-stands--어디까지-왔는가) |
| **Where** | Where does each piece live? | 무엇이 어디에 있나 | [Where things live](#where-things-live--무엇이-어디에-있는가) |
| **Who** | Who is it for, and who wrote what? | 누구를 위한 것이고 누가 무엇을 썼나 | [Who it is for](#who-it-is-for--누구를-위한-것인가) |

To just run it, skip to [Running it](#running-it--직접-돌리기).

바로 돌려보려면 [직접 돌리기](#running-it--직접-돌리기)로 가십시오.

---

## What it is / 무엇인가

Two things that meet in the middle.

**An ORB.** `crates/orbweaver-*` is a CORBA implementation in Rust: CDR
marshalling, GIOP/IIOP over TCP, IORs, TypeCodes, a POA, an IDL 4.2 front end,
an Interface Repository, and the CosNaming / CosEvent / Trading services. It is
written against the published OMG specification and links no existing ORB —
see [Why MIT forces an in-house core](#why-mit-forces-an-in-house-core--왜-mit가-자체-구현을-강제하는가).

**A specification pipeline.** `orbweaver-forge` takes a requirement written in
prose and produces an annotated IDL contract, verified by a compiler rather than
by a reviewer. `orbweaver-mcp` then exposes the served objects to an AI agent as
callable tools, behind a default-deny policy and an interceptor chain.

The thing that makes them one project rather than two: **IDL is a contract
language strict enough that a machine can check a machine's output.** A model
writes plausible IDL that may be wrong; an IDL compiler rejects wrong IDL every
single time. Generative synthesis, deterministic verification. Everything else
here is downstream of that asymmetry.

두 가지가 가운데서 만납니다.

**ORB.** `crates/orbweaver-*`는 Rust로 쓴 CORBA 구현입니다 — CDR 마샬링,
TCP 위의 GIOP/IIOP, IOR, TypeCode, POA, IDL 4.2 프런트엔드, 인터페이스
리포지터리, 그리고 CosNaming·CosEvent·Trading 서비스. 공개된 OMG 명세만 보고
작성했고 기존 ORB를 링크하지 않습니다 —
[왜 MIT가 자체 구현을 강제하는가](#why-mit-forces-an-in-house-core--왜-mit가-자체-구현을-강제하는가) 참조.

**명세 파이프라인.** `orbweaver-forge`는 산문으로 쓴 요구사항을 받아 어노테이션이
붙은 IDL 계약을 만듭니다. 검토자가 아니라 컴파일러가 검증합니다. 이어서
`orbweaver-mcp`가 서빙 중인 객체를 AI 에이전트에게 호출 가능한 도구로 노출하되,
기본 거부 정책과 인터셉터 체인 뒤에 둡니다.

둘이 별개의 프로젝트가 아니라 하나인 이유는 이것입니다 — **IDL은 기계가 기계의
출력을 검사할 수 있을 만큼 엄격한 계약 언어입니다.** 모델은 그럴듯하지만 틀릴 수
있는 IDL을 쓰고, IDL 컴파일러는 틀린 IDL을 매번 거부합니다. 생성은 확률적으로,
검증은 결정론적으로. 나머지는 전부 이 비대칭에서 따라 나옵니다.

### What "finished" means here / 여기서 "완성"의 뜻

An ORB is never finished by counting operations, because a list of capabilities
can only grow. Since 2026-08-26 this project has a single completion criterion,
set by the owner, and it is a **property** rather than a list:

> A caller can invoke any target holding only a reference — knowing nothing of
> its location, backend, language or load state — and this survives targets
> being added, removed, moved, loaded or evicted at runtime.

Its home is [`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6
and it is deliberately not restated anywhere else. The practical consequence:
**transparency is not confirmed, it is hunted.** A change that closes a leak
outranks one that adds a capability, and the five transparency rows in D029 §6.1
each name the leaks found in them so far, including the ones still open.

연산 개수를 세는 방식으로는 ORB가 완성되지 않습니다. 능력의 목록은 늘어나기만
하기 때문입니다. 2026-08-26부터 이 프로젝트에는 소유자가 정한 완성 기준이 하나
있고, 그것은 목록이 아니라 **성질**입니다 — *호출자는 참조 하나만 들고 어떤
대상이든 호출할 수 있으며, 그 대상의 위치·백엔드·언어·적재 상태를 전혀 모르고,
대상이 런타임에 추가·제거·이동·적재·축출되어도 이 성질이 유지된다.*

기준의 집은 [`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md)
§6이며 다른 어디에도 다시 적지 않습니다. 실무적 귀결은 이것입니다 — **투명성은
확인하는 것이 아니라 구멍을 사냥하는 것입니다.** 구멍을 막는 변경이 기능을 더하는
변경보다 앞서고, D029 §6.1의 다섯 행은 각자 지금까지 발견된 구멍을 — 아직 열려
있는 것까지 — 이름으로 적고 있습니다.

---

## Why CORBA, and why now / 왜 CORBA인가, 왜 지금인가

Not nostalgia. Four reasons, in the order they actually mattered.

**1. CORBA already shipped what agent tooling is reinventing.** The Model
Context Protocol standardised runtime tool discovery in 2025: a client calls
`tools/list` and gets a live catalog of callable operations with their schemas.
CORBA shipped that in 1996 as the **Interface Repository**, with the **Dynamic
Invocation Interface** as the matching call path. Together with `TypeCode` and
`DynAny`, a caller can discover an interface it has never seen and invoke it
correctly at runtime, with no generated code.

**2. The legacy is load-bearing and is not going away.** Naval combat systems,
command and control, telecom switching, air traffic control, core banking, large
physics installations. Rewriting is not on the table for any of them.

**3. OMG IDL 4.x is shared with DDS.** Korean defence programmes are
standardising on DDS-based middleware, and DDS-XTypes uses the same IDL. One
pipeline serves both, which turns a shrinking CORBA market from a risk into an
expansion path.

**4. Java severed its own connection.** [JEP 320](https://openjdk.org/jeps/320)
removed `java.corba` and `javax.rmi.CORBA` in JDK 11, so Java legacy needs a
third-party ORB just to keep running. That migration is itself demand for
automation.

And the one underneath all four: interfaces are increasingly called by agents
rather than by people. In that world a contract that is verbose but precise
beats one that is terse but ambiguous. The strictness humans fled from is the
precision agents need.

향수가 아닙니다. 실제로 중요했던 순서대로 네 가지입니다.

**1. 에이전트 도구 생태계가 다시 만들고 있는 것을 CORBA는 이미 출하했습니다.**
MCP는 2025년에 런타임 도구 발견을 표준화했습니다 — 클라이언트가 `tools/list`를
호출하면 스키마와 함께 호출 가능한 연산 목록이 돌아옵니다. CORBA는 이것을 1996년에
**인터페이스 리포지터리**로 출하했고, 대응하는 호출 경로가 **동적 호출
인터페이스(DII)**입니다. `TypeCode`·`DynAny`와 함께라면 한 번도 본 적 없는
인터페이스를 런타임에 발견해 정확히 호출할 수 있습니다. 생성된 코드는 한 줄도
필요 없습니다.

**2. 레거시가 하중을 지탱하고 있고 사라지지 않습니다.** 함정 전투체계, 지휘통제,
통신 교환기, 항공관제, 금융 코어, 대형 물리실험 설비. 어느 쪽도 재작성이
선택지가 아닙니다.

**3. OMG IDL 4.x는 DDS와 공유됩니다.** 국내 국방은 DDS 기반 미들웨어로 표준화
중이고 DDS-XTypes는 같은 IDL을 씁니다. 하나의 파이프라인이 양쪽을 커버하므로,
축소되는 CORBA 시장이 리스크가 아니라 확장 경로가 됩니다.

**4. Java는 스스로 연결을 끊었습니다.** [JEP 320](https://openjdk.org/jeps/320)이
JDK 11에서 `java.corba`와 `javax.rmi.CORBA`를 제거해, Java 레거시는 동작만
유지하려 해도 서드파티 ORB가 필요합니다. 그 마이그레이션 자체가 자동화
수요입니다.

그리고 네 가지 아래에 깔린 이유 하나 — 인터페이스를 호출하는 주체가 사람에서
에이전트로 옮겨가고 있습니다. 그 세계에서는 장황하지만 정밀한 계약이 간결하지만
모호한 계약을 이깁니다. 인간이 도망친 그 엄격함이 에이전트에게는 필요한
정밀성입니다.

### The bet, and the evidence for it / 내기, 그리고 그 근거

Three propositions drive the project. The third is the one that decides what
gets built first.

| # | Proposition | 명제 |
|---|---|---|
| 1 | CORBA is a runtime self-describing type system. IFR + TypeCode + DII/DSI + DynAny is an agent tool catalog that already exists. | CORBA는 런타임 자기기술 타입 시스템이다. IFR + TypeCode + DII/DSI + DynAny 조합이 곧 에이전트 도구 카탈로그다. |
| 2 | For an LLM, IDL's verbosity is an asset. The strictness humans fled from is what makes generated interfaces verifiable — the IDL compiler is a ground-truth oracle. | LLM에게 IDL의 장황함은 자산이다. 인간이 도망친 엄격함이 생성 결과를 검증 가능하게 만든다 — IDL 컴파일러가 정답 채점기다. |
| 3 | The bottleneck is specification quality, not code generation. So the first-class deliverable is a semantic annotation vocabulary, not a code generator. | 병목은 코드 생성이 아니라 명세 품질이다. 따라서 1급 산출물은 코드 생성기가 아니라 의미 어노테이션 규약이다. |

Proposition 3 is not a guess.
[AutoMCP](https://arxiv.org/html/2507.16044v2) compiled 5,066 endpoints across
50 real APIs into MCP servers: 76.5% worked out of the box, rising to 99.9%
after an average of 19 lines of *specification* fixes per API. The failures were
spec defects — missing security schemes (62%), undocumented runtime headers
(47%), malformed base URLs (41%) — not generator bugs. That is why the semantic
layer below is a deliverable and the code generator is a consequence.

명제 3은 추측이 아닙니다. [AutoMCP](https://arxiv.org/html/2507.16044v2)는 실제
API 50개·엔드포인트 5,066개를 MCP 서버로 컴파일해 즉시 동작 76.5%, API당 평균
19줄의 *명세* 수정 후 99.9%를 달성했습니다. 실패 원인은 생성기 버그가 아니라
명세 결함이었습니다 — 보안 스키마 누락 62%, 미문서화 런타임 헤더 47%, 잘못된
base URL 41%. 아래의 의미 계층이 산출물이고 코드 생성기가 그 귀결인 이유입니다.

### Why MIT forces an in-house core / 왜 MIT가 자체 구현을 강제하는가

There is no CORBA ORB available under MIT. Verified 2026-08:

| Project | License | Verdict |
|---|---|---|
| omniORB / omniORBpy | LGPL (libraries) + GPL (tools) | excluded / 배제 |
| JacORB | LGPL | excluded / 배제 |
| GlassFish CORBA | EPL / GPLv2+CPE | excluded / 배제 |
| MICO | GPL / LGPL | excluded / 배제 |
| ACE / TAO | DOC License — permissive, MIT-equivalent in effect, not SPDX-recognised MIT | not literally MIT / 문자 그대로는 MIT 아님 |
| `foxglove/omgidl` | **MIT** | usable / 사용 가능 |
| `tier4/idl_parser`, `eProsima/IDL-Parser` | Apache-2.0 | permissive, not MIT / 관대하나 MIT 아님 |
| `sugarsweetrobotics/idl_parser`, `asenac/idl-parser` | no license declared | unusable / 사용 불가 |

So the core is written here. **That is less painful than it sounds, because
interoperability needs no license.** GIOP/IIOP is a published OMG specification;
implementing the wire protocol creates no obligation to anyone. Existing ORBs
are therefore demoted from dependencies to **fixtures** — run as separate
processes over TCP, or invoked as external programs whose text output we read.
Never imported, never linked, never vendored, never redistributed. `cargo tree`
is checked for this in CI.

Two external crates exist: `encoding_rs` for EUC-KR and its `cfg-if`. That is
the one carve-out in the policy, and it is deliberate. Logic defined by a
published specification we implement ourselves and owe nobody for. A character
mapping table is somebody's compilation of facts with no specification to
implement from, so retyping it produces the same derived work rather than an
original one — **a table derived from an incompatibly-licensed source is not
laundered by being retyped.** So permissive-with-attribution is accepted for
data we cannot originate, disclosed in [`NOTICE`](NOTICE), and recorded as
[`D001`](docs/decisions/D001-codeset-crate.md). It sits behind the default-on
`euc-kr` feature; `--no-default-features` removes the crate and the obligation
together, and both configurations are tested.

MIT로 제공되는 CORBA ORB는 존재하지 않습니다(2026-08 확인). 그래서 코어를 여기서
직접 씁니다. **들리는 것만큼 뼈아프지 않은 이유는 상호운용에 라이선스가 필요하지
않기 때문입니다.** GIOP/IIOP는 공개된 OMG 명세이며, 와이어 프로토콜을 구현하는
것은 누구에게도 의무를 발생시키지 않습니다. 따라서 기존 ORB는 의존성에서
**픽스처**로 강등됩니다 — TCP 위의 별도 프로세스로 띄우거나, 텍스트 출력을 읽는
외부 프로그램으로만 호출합니다. import·링크·벤더링·재배포 모두 하지 않으며,
`cargo tree`를 CI가 검사합니다.

외부 크레이트는 둘입니다 — EUC-KR용 `encoding_rs`와 그 `cfg-if`. 정책의 유일한
예외이며 의도된 것입니다. 공개 명세로 정의된 로직은 직접 구현하면 되고 누구에게도
빚지지 않습니다. 그러나 문자 매핑 테이블은 구현할 명세가 없는, 누군가의 사실
편찬물입니다. 옮겨 적어도 원저작물이 아니라 같은 2차적 저작물이 될 뿐입니다 —
**양립 불가한 라이선스에서 온 표는 다시 타이핑한다고 세탁되지 않습니다.** 그래서
우리가 원저작할 수 없는 데이터에 한해 귀속 표시 조건의 관대 라이선스를 허용하고,
[`NOTICE`](NOTICE)에 공개하며, [`D001`](docs/decisions/D001-codeset-crate.md)로
기록합니다. 기본 켜짐인 `euc-kr` 기능 뒤에 있고 `--no-default-features`가
크레이트와 의무를 함께 제거하며, 두 구성 모두 테스트합니다.

---

## How it works / 어떻게 동작하는가

```
  자연어 요구사항            IDL 4.2 + 의미 어노테이션         살아있는 연동
  natural-language     ──▶   annotated IDL contract    ──▶   live binding
  requirement                (compiler-verified)              (no stubs)
```

### The pipeline / 파이프라인

| Stage | 단계 | Input → Output |
|---|---|---|
| **S1** Ingest | 흡수 | requirements / legacy source / existing IDL → IR |
| **S2** Synthesize | 합성 | IR → OMG IDL 4.2 draft |
| **S3** Annotate | 의미부착 | IDL → SIDL (`//@ ai_*` semantics) |
| **S4** Validate | 검증 | SIDL → compile gate + self-repair loop |
| **S5** Register | 등록 | SIDL → type registry + semantic index |
| **S6** Bind | 연동 | catalog → dynamic call, or generated stubs |
| **S7** Verify | 검증·운영 | binding → contract tests, interceptors, tracing |

Each stage is a producer plus its own gate; what a given stage reports is
documented in [`PLAN.md`](docs/PLAN.md) §5. There is no "automation percentage"
per stage — no run in the tree computes one, so this file does not print one.

**S4 is the safety belt.** It is where the compiler gets to overrule the model,
and it is the reason the rest of the pipeline is allowed to be probabilistic.

각 단계는 생산자와 자기 게이트의 쌍입니다. 각 단계가 실제로 보고하는 것은
[`PLAN.ko.md`](docs/PLAN.ko.md) §5에 있습니다. 단계별 "자동화 백분율"은 없습니다 —
트리의 어떤 실행도 그것을 계산하지 않으므로 이 파일도 적지 않습니다.

**S4가 안전벨트입니다.** 컴파일러가 모델을 뒤집을 수 있는 지점이고, 그래서
파이프라인의 나머지가 확률적이어도 되는 것입니다.

### SIDL, the semantic layer / SIDL, 의미 계층

IDL nails syntax and says nothing about meaning.
`long transfer(in long acct, in long amt)` is perfectly typed and tells an agent
nothing about whether the unit is won or cents, whether it is idempotent,
whether it is destructive, or whether the argument is PII.

SIDL closes that gap with **structured comments**:

```idl
module bank {
  //@ ai_desc: Transfers funds between accounts. Rolls back in full on failure.
  interface Transfer {
    //@ ai_effect: destructive
    //@ ai_idempotent: false
    //@ ai_authz: bank.transfer.write
    void execute(
      //@ ai_pii: high
      in long from_account,
      //@ ai_pii: high
      in long to_account,
      //@ ai_unit: KRW
      in long amount
    ) raises (InsufficientFunds, AccountFrozen);
  };
};
```

**Why comments and not IDL 4's own `@annotation`.** Because we measured it and
it does not work. Phase 0 assumption C put both spellings through `omniidl`:
the declaration-plus-application form and the application-only form were each
refused with `Syntax error in definition`; structured comments compiled cleanly.
Most deployed ORB compilers are CORBA 2.x/3.x era and predate IDL 4 annotations
entirely. The fallback is viable precisely because we own the parser — which was
the stated reason for an in-house IDL front end in the first place. The full
result is in [`PHASE0.md`](docs/PHASE0.md) *Assumption C*.

The IDL 4 syntax is not abandoned: `orbweaver-idl` accepts both spellings and
emits whichever the target toolchain tolerates, so the standard form becomes
available the moment a deployment's compiler supports it. One nuance worth
knowing: `omniidl` *discards* comments, so annotations survive only inside
`orbweaver-idl`. That is intended — `omniidl` is a conformance oracle for base
IDL, nothing more.

One vocabulary drives both directions: at runtime it is the tool description an
agent reads; at build time it is the source for generated contract tests and
guardrails.

IDL은 구문은 완벽히 잡지만 의미는 말하지 않습니다.
`long transfer(in long acct, in long amt)`는 타입은 완벽하지만, 단위가 원인지
센트인지, 멱등한지, 파괴적인지, 인자가 PII인지 — 에이전트가 알아야 할 것은 하나도
담지 못합니다. SIDL은 그 간극을 **구조화 주석**으로 메웁니다(위 예시).

**왜 IDL 4의 `@annotation`이 아니라 주석인가.** 재봤고, 동작하지 않기
때문입니다. Phase 0 가정 C가 두 표기를 `omniidl`에 통과시켰습니다 — 선언+적용
형태와 적용만 하는 형태 모두 `Syntax error in definition`으로 거부되었고,
구조화 주석은 깨끗이 컴파일되었습니다. 배포된 ORB 컴파일러 대부분은 CORBA
2.x/3.x 세대라 IDL 4 어노테이션 자체가 없습니다. 폴백이 유효한 이유는 정확히
우리가 파서를 소유하기 때문이며, 애초에 IDL 프런트엔드를 직접 만든 이유가
그것이었습니다. 전체 결과는 [`PHASE0.md`](docs/PHASE0.md) *가정 C*에 있습니다.

IDL 4 문법을 버린 것은 아닙니다. `orbweaver-idl`은 두 표기를 모두 수용하고 대상
툴체인이 견디는 쪽으로 내보내므로, 배포처의 컴파일러가 지원하는 순간 표준 표기를
쓸 수 있습니다. 알아 둘 점 하나 — `omniidl`은 주석을 버리므로 어노테이션은
`orbweaver-idl` 안에만 남습니다. 의도된 설계입니다. `omniidl`은 기본 IDL의 적합성
채점기일 뿐입니다.

하나의 어휘가 양방향을 구동합니다. 런타임에는 에이전트가 읽는 도구 설명이고,
빌드 타임에는 계약 테스트와 가드레일을 생성하는 근거입니다.

### Dual-path binding / 이중 경로 연동

Pure code generation breaks automation the moment a schema changes — every
change means regenerate and redeploy. Pure dynamic invocation is too slow for a
hot path. So both run, and calls are promoted between them.

| | **Dynamic path / 동적 경로** | **Static path / 정적 경로** |
|---|---|---|
| Mechanism | DII + DynAny | generated stubs / 생성 스텁 |
| Code generated | none / 없음 | full / 전체 |
| Schema change | adapts automatically / 자동 적응 | regenerate + redeploy / 재생성·재배포 |
| Latency | higher / 높음 | lowest / 최저 |
| Best for | discovery, experiments, low-frequency / 탐색·실험·저빈도 | hot paths, real-time / 임계 경로·실시간 |

**Promotion criteria** — ≥1,000 calls/day **and** schema unchanged for 30 days
**and** regression suite green. Explore dynamically, settle statically.

순수 코드 생성은 스키마가 바뀌는 순간 자동화가 깨집니다. 변경마다
재생성·재배포가 필요하기 때문입니다. 순수 동적 호출은 임계 경로에 쓰기엔
느립니다. 그래서 둘 다 운영하고 사이에서 승격시킵니다. **승격 조건**은 일
1,000회 이상 호출 **그리고** 스키마 30일 무변경 **그리고** 회귀 스위트 통과입니다.
탐색은 동적으로, 정착은 정적으로.

### Contract evolution is proved, not asserted / 계약 진화는 주장이 아니라 실측

[`PLAN.md`](docs/PLAN.md) §5.3 lists which contract edits deployed peers
survive. That list was not reasoned out — it was measured against a real peer.
Against an omniORB servant built from the previous contract, a client encoding a
struct whose two `long` members had been swapped called `first({px:11, py:22})`
and received **22 — the other member's value, with no exception raised**. CDR
marshals by position and carries no tags, so nothing on either side can notice.

```console
$ idl-diff released.idl proposed.idl
[BREAKING] IDL:evo/Point:1.0: members reordered: ["px", "py"] became ["py", "px"] — CDR
  marshals members by position and carries no tags, so a reordered struct is read
  field-for-field into the wrong members and nothing detects it
[server-first] IDL:evo/Reader:1.0: operation "total" added — servers must be updated
  before clients, or a new client calling an old server receives BAD_OPERATION

refused: 1 change(s) break deployed peers
```

`idl-diff` refuses that revision before it ships, and accepts the additive-only
one.

이전 계약으로 빌드된 omniORB 서번트에 대해, 구조체 멤버 두 개를 맞바꾼
클라이언트가 호출하자 **예외 없이 다른 멤버의 값**이 돌아왔습니다. CDR은 위치로
마샬링하며 태그가 없어 양쪽 모두 알아챌 수 없습니다. `idl-diff`가 릴리스 전에
이 리비전을 거부하고, 추가만 있는 리비전은 통과시킵니다.

---

## How the work is run / 작업은 어떻게 도는가

Everything runs as a **batch loop**, never item by item.

```
  1. Batch     produce the whole set in one pass, oracle not consulted
  2. Oracle    verify the whole set, cluster diagnostics BY ROOT CAUSE
  3. Repair    one fix per cause across every affected item, re-verify all
  4. Codify    make the cause impossible — lint rule, prompt constraint, corpus case
     ↺         repeat until a round finds no new causes
```

This is not a style preference; Phase 0 measured it. Twenty IDL files generated
in one pass produced seven failures, and **all seven had a single root cause**:
IDL identifier clashes are case-insensitive, so `Position position` and
`module inventory { interface Inventory }` are both illegal — natural naming in
every other language, illegal here. Item-by-item work would have produced seven
patches and never surfaced the rule. One fix took the batch from 65% to 100%,
and the rule became the first lint the project shipped.

Each step is an agent role in [`.claude/agents/`](.claude/agents/). The
load-bearing constraint: **`batch-synth` has no Bash tool**, so it cannot peek at
the oracle. That is what keeps the first-pass rate honest and forces shared
causes into the open.

| Role | Step | Constraint |
|---|---|---|
| [`batch-synth`](.claude/agents/batch-synth.md) | produce | no Bash — cannot consult the oracle |
| [`oracle-sweep`](.claude/agents/oracle-sweep.md) | verify | returns causes with affected items, never a bare failure list |
| [`batch-repair`](.claude/agents/batch-repair.md) | fix | one fix per cause; challenges the clustering first |
| [`codifier`](.claude/agents/codifier.md) | persist | must prove each rule fires on the original failure |
| [`spec-auditor`](.claude/agents/spec-auditor.md) | review | audits against the OMG spec, not against the tests |

모든 작업은 건건이가 아니라 **일괄 루프**로 돕니다. 취향이 아니라 Phase 0이
측정한 결과입니다 — 20건을 한 번에 생성해 실패 7건이 나왔고, **7건 전부 동일한
근본원인**이었습니다. IDL 식별자 충돌이 대소문자를 구분하지 않는다는 것으로,
`Position position`도 `module inventory { interface Inventory }`도 불법입니다.
다른 언어에서는 자연스러운 명명이 여기서는 불법입니다. 건건이 고쳤다면 패치
7개가 나왔을 뿐 규칙은 드러나지 않았을 것입니다. 수정 하나가 65%를 100%로
끌어올렸고, 그 규칙이 프로젝트의 첫 린트가 되었습니다.

각 단계는 [`.claude/agents/`](.claude/agents/)의 에이전트 역할입니다. 하중을
지탱하는 제약은 **`batch-synth`에 Bash 도구가 없다**는 것입니다. 오라클을 미리
볼 수 없어야 1차 통과율이 정직해지고 공통 원인이 드러납니다.

### What counts as measured / 무엇을 측정으로 치는가

The harness is the single merge gate, and a few rules about it are worth stating
because each one was learned by being burned:

- **An unmeasured check is a failure, never a pass.** If a fixture will not
  start, the failure counter goes up. A harness that reports green on an
  unmeasured assumption is worse than no harness.
- **A green that means *nothing happened* looks exactly like a green that means
  *the property held*.** So a group that asserts a caller *cannot tell* two
  things apart is only worth something beside a counted companion showing the
  two answers *can* differ.
- **A new group lands with its negative control in the commit message** — the
  command that was run to make it red, and what it printed.
- **Compare decoded values, never raw buffers.** CDR padding content is
  undefined by the specification and omniORB does not zero it.
- **A peer's bytes are recorded with provenance and re-taken live.** A
  convention both ends apply cannot be refuted by a round trip.

The full set, with the incident behind each, is in [`CLAUDE.md`](CLAUDE.md).

하네스가 유일한 병합 게이트이며, 아래 규칙들은 전부 데어 보고 배운 것이라 적어
둘 값이 있습니다 — **측정하지 못한 검사는 실패이지 통과가 아니다.** 픽스처가
뜨지 않으면 실패 카운터를 올립니다. **아무 일도 일어나지 않았다는 초록과 성질이
지켜졌다는 초록은 똑같이 보인다.** 그래서 "호출자가 둘을 구별할 수 없다"는
그룹은, 두 답이 실제로 달라질 수 있음을 보이는 동반 그룹 옆에서만 값이 나갑니다.
**새 그룹은 부정 대조군을 커밋 메시지에 달고 착지합니다** — 빨갛게 만든 명령과 그
출력. **원시 버퍼가 아니라 디코딩된 값을 비교합니다.** CDR 패딩 내용은 명세가
정의하지 않으며 omniORB는 0으로 채우지 않습니다. **피어의 바이트는 출처와 함께
기록하고 라이브에서 다시 받습니다.** 양쪽이 함께 적용하는 관례는 왕복으로
반증되지 않기 때문입니다. 전체 목록은 각각의 사건과 함께
[`CLAUDE.md`](CLAUDE.md)에 있습니다.

---

## Where it stands / 어디까지 왔는가

Phases 0 through 3.5 are complete, Phase 4 is substantially landed, and Phase 5
is about half landed. The remainder is organised as five parallel streams
([`PLAN.md`](docs/PLAN.md) §7.3), each with its own batch unit and oracle,
meeting only at four named integration points.

| Phase | State | Focus | 내용 |
|---|---|---|---|
| **0** | done | Feasibility spike — verdict **GO** ([PHASE0](docs/PHASE0.md)) | 타당성 검증 — 판정 GO |
| **1** | done | Wire core: bidirectional interop, omniORB + JacORB, GIOP 1.0/1.1/1.2 ([PHASE1](docs/PHASE1.md)) | 와이어 코어: 두 피어 양방향 상호운용 |
| **2** | done | IDL front end, registry, POA, object model, §5.3 differ proved on the wire ([PHASE2](docs/PHASE2.md)) | IDL·레지스트리·객체 모델·계약 진화 |
| **3** | done¹ | Dynamic invocation, AnyJSON, MCP triad over stdio, S4 gate ([PHASE3](docs/PHASE3.md)) | 동적 호출·AnyJSON·MCP·S4 게이트 |
| **3.5** | done | Capability handles, landed *with* the bridge | 능력 핸들 — 브릿지와 동시 착지 |
| **4** | mostly | Static generation: client stubs **and** server skeletons, promotion gate, static-equals-dynamic in both directions ([PHASE4](docs/PHASE4.md)) | 정적 생성 — 스텁·스켈레톤 양방향 |
| **5** | half | CSIv2 wire, delegation policy, `//@ ai_authz` scopes; TLS and token exchange remain → stream C ([PHASE5](docs/PHASE5.md)) | 신원 전파 — 절반 착지, 나머지는 스트림 C |
| **6** | streams C·D | TLS, observability, governance, console, pilot ([PHASE6](docs/PHASE6.md)) | 운영화 → 스트림 C·D |

¹ minus S1–S3, the model-in-the-loop stages → stream A / 모델이 개입하는 S1–S3 제외 → 스트림 A

**Per-component status is in [`docs/COMPONENTS.md`](docs/COMPONENTS.md)**, which
states what is landed *and measured*, with the missing half of every partial row
written out. It is the only place that carries those verdicts.

**What is open is written down as a row or a skip, never as an omission.**
Anything still leaking is a row in [`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md)
§6.1; anything unmeasurable today is a counted `SKIPPED` group in the harness
naming its blocking fixture. The harness prints a per-transparency ledger on
every run — measured, red, or unmeasured with the reason in the group's own
words. No score is derived from that ledger and none should be: a shrinking
unmeasured list is progress only when a run closed a leak, and looks identical
to nobody looking.

Phase 0–3.5 완료, Phase 4 대부분 착지, Phase 5 절반. 남은 작업은 병행 스트림
다섯 개([`PLAN.ko.md`](docs/PLAN.ko.md) §7.3)로 조직되며, 각 스트림은 자체 일괄
단위와 오라클을 갖고 네 개의 명명된 통합 지점에서만 만납니다.

**구성요소별 상태는 [`docs/COMPONENTS.md`](docs/COMPONENTS.md)에 있습니다.**
착지했고 *측정된* 것을 적으며, 부분 착지 행은 빠진 절반까지 문장으로 씁니다.
그 판정을 담는 유일한 장소입니다.

**열려 있는 것은 행이나 건너뜀으로 적히지, 누락으로 남지 않습니다.** 아직 새고
있는 것은 [`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6.1의
행이고, 오늘 측정 불가한 것은 막고 있는 픽스처를 이름으로 적은 `SKIPPED`
그룹입니다. 하네스는 실행마다 투명성별 원장을 출력합니다 — 측정됨, 빨강, 또는
그룹 자신의 말로 적은 미측정 사유. 점수는 내지 않으며 내서도 안 됩니다. 미측정
목록이 줄어드는 것은 어떤 실행이 구멍을 막았을 때만 진전이며, 아무도 보지 않을
때와 똑같아 보이기 때문입니다.

---

## Running it / 직접 돌리기

```bash
git clone https://github.com/seanshin/orbweaver && cd orbweaver
cargo test --workspace        # the unit and integration suite; no fixtures needed
```

That is the whole story if you only want the ORB to build and its own tests to
pass. Interop needs a peer:

```bash
brew install omniorb          # fixture only — never linked, never shipped
./spikes/jacorb/setup.sh      # the second peer and its Interface Repository
./spikes/run_checks.sh        # the full harness; its exit code IS the verdict
```

The harness takes a machine-wide lock at `/tmp/orbweaver-harness.lock`. Two runs
at once destroy each other's fixtures and report failures that are about the
scheduling rather than the code — that cost two diagnoses before the lock
existed. Wait for the lock rather than removing it.

Useful single gates, without the full run:

```bash
cargo run -q --bin sidl-validate -- <files>.idl        # syntax, semantics, fix hints
cargo run -q --bin idl-diff -- <released>.idl <proposed>.idl   # refuses breaking edits
./spikes/differential.sh                               # two front ends over the corpus
cargo run -q --bin gen-python -- --out <dir> <files>.idl       # the second target
```

`cargo tree` shows exactly two external crates. The wire implementation itself
is written against the published OMG specification alone; omniORB and JacORB
appear only as separate-process peers and as conformance oracles whose text
output we read.

ORB를 빌드하고 자체 테스트만 통과시키려면 위의 두 줄이 전부입니다. 상호운용에는
피어가 필요합니다. 하네스는 `/tmp/orbweaver-harness.lock`에 머신 전역 락을
잡습니다. 두 개를 동시에 돌리면 서로의 픽스처를 죽이고 코드가 아니라 스케줄링에
대한 실패를 보고합니다 — 락이 생기기 전에 진단 두 번을 이것으로 썼습니다. 락을
지우지 말고 기다리십시오.

`cargo tree`에 외부 크레이트는 정확히 둘입니다. 와이어 구현 자체는 공개 OMG
명세만 보고 작성했으며, omniORB와 JacORB는 별도 프로세스 피어와 텍스트 출력을
읽는 적합성 채점기로만 등장합니다.

---

## Where things live / 무엇이 어디에 있는가

**One fact, one home.** A document that restates another document's fact drifts
from it on the next change, silently, because nothing compiles a sentence. This
was measured on 2026-08-18: ten stale decision-status claims and four stale
remaining-work lists across five documents, produced by nothing worse than
decisions being approved and work landing. So each kind of fact has exactly one
place, and the harness checks the restatements it can.

**사실 하나에 집 하나.** 다른 문서의 사실을 다시 적은 문장은 다음 변경에서 조용히
어긋납니다. 문장을 컴파일하는 것은 없기 때문입니다. 2026-08-18 측정 — 다섯 문서에
걸쳐 낡은 결정 상태 10건과 낡은 잔여 작업 목록 4건이 있었고, 원인은 결정이
승인되고 작업이 착지한 것 이상도 이하도 아니었습니다.

### The crates / 크레이트

| Crate | 역할 |
|---|---|
| `orbweaver-cdr` | CDR encode/decode, and the workspace's JSON reader/writer |
| `orbweaver-giop` | GIOP, IOR, TypeCode, `Server`/`Dispatch`, naming and event servants |
| `orbweaver-idl` | IDL 4.2 front end, SIDL structured comments |
| `orbweaver-registry` | types as data, the IFR facade, remote IFR ingestion, the §5.3 differ |
| `orbweaver-object` | POA, references, MoE residency, tenancy |
| `orbweaver-dynamic` | value marshalling, DII/DSI shape, AnyJSON |
| `orbweaver-trading` | offer store, constraint queries, loading policy |
| `orbweaver-forge` | the S1–S5 pipeline, each stage a producer plus its own gate |
| `orbweaver-mcp` | the agent boundary: triad, handles, interceptor chain, dry-run |
| `orbweaver-gen` | client stubs and server skeletons |
| `orbweaver-test` | property, contract advice, wire fuzz |
| `orbweaver-console` | catalog, contract diff and trace pages — renders, decides nothing |

Two crates in the original roster never came into existence, and that is
recorded rather than quietly dropped: `orbweaver-poa` is part of
`orbweaver-object`, and `orbweaver-guard` / `orbweaver-capability` /
`orbweaver-identity` are modules of `orbweaver-mcp`. Both are location choices,
not gaps, and [`COMPONENTS.md`](docs/COMPONENTS.md) says so per row.

원래 명단의 두 크레이트는 끝내 생기지 않았고, 조용히 빠지는 대신 그 사실을
기록합니다 — 위치 선택이지 공백이 아닙니다.

### The corpus and the fixtures / 코퍼스와 픽스처

| Path | Contents |
|---|---|
| `corpus/golden/` | must all compile — type-system and CDR coverage |
| `corpus/negative/` | must all be rejected — diagnostic-quality material |
| `corpus/services/` | contracts that exist to be served |
| `corpus/pragma/` | repository-id cases, diffed against `omniidl` |
| `corpus/requirements/` | the assumption-B benchmark, frozen before generation |
| `corpus/queries/` | the frozen search benchmark |
| `corpus/annotations/` | the assumption-C probes |
| `corpus/include/` | multi-file cases — resolution, prefix scope, guards, cycles |
| `spikes/estate/` | thirteen legacy contracts that include each other, nothing annotated |
| `spikes/` | fixtures, servers, the harness, and the measurement scripts |

### The documents / 문서

| Document | Contents |
|---|---|
| [`CLAUDE.md`](CLAUDE.md) | working rules — the licensing boundary, the IDL rules, and every harness defect that produced a phantom result |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | the system **as built**: crate graph and its dependency rule, wire/type/agent paths, four trust boundaries, deliberate absences |
| [`docs/COMPONENTS.md`](docs/COMPONENTS.md) | what is landed **and measured**, with the missing half of every partial row |
| [`docs/PLAN.md`](docs/PLAN.md) · [`docs/PLAN.ko.md`](docs/PLAN.ko.md) | the full technical plan, research findings, risk register, success metrics |
| [`docs/PLAN-MOE.md`](docs/PLAN-MOE.md) | CORBA as a control plane, forbidden in the data plane |
| [`docs/PLAN-SERVICES.md`](docs/PLAN-SERVICES.md) | the CosNaming / CosEvent / Trading / IFR / LifeCycle suite |
| [`docs/PLAN-DEFERRED.md`](docs/PLAN-DEFERRED.md) | eight excluded services, each with the trigger that would un-defer it |
| [`docs/SERVICES-COVERAGE.md`](docs/SERVICES-COVERAGE.md) | per-operation service coverage, generated from what the wire reports |
| [`docs/PHASE0.md`](docs/PHASE0.md) … [`PHASE6.md`](docs/PHASE6.md) | dated records: what was measured in each phase, and what the next inherits |
| [`docs/decisions/`](docs/decisions/) | 34 decision records. A decision's status lives here and nowhere else — every other mention is checked against it by the harness |
| [`docs/pipeline-runs/`](docs/pipeline-runs/) | dated records of individual batches: what was measured, what the brief got wrong |

Two decisions are load-bearing enough to name here:
[`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6 holds the
completion criterion, and [`D034`](docs/decisions/D034-stopping-what-the-orb-handed-out.md)
holds what `Orb::shutdown` promises and refuses.

결정 두 건은 여기 이름을 적어 둘 만큼 하중을 지탱합니다 —
[`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6이 완성
기준을, [`D034`](docs/decisions/D034-stopping-what-the-orb-handed-out.md)가
`Orb::shutdown`이 약속하는 것과 거절하는 것을 담고 있습니다.

---

## Who it is for / 누구를 위한 것인가

**If you maintain a CORBA estate**, the parts that concern you are the ORB core
and `idl-diff`: an MIT implementation that talks to your existing peers, and a
gate that refuses a contract change your deployed peers would not survive.

**If you are wiring an AI agent to legacy interfaces**, the parts that concern
you are `orbweaver-forge` and `orbweaver-mcp`: a contract with machine-readable
semantics, and an agent boundary that is default-deny with an audit trail.

**If you work on DDS**, the IDL front end is shared ground — OMG IDL 4.x is the
same language DDS-XTypes uses.

**If you are here for the engineering**, the interesting document is
[`CLAUDE.md`](CLAUDE.md). Most of it is a list of ways a test can be green while
measuring nothing, each one found the hard way and written down so it cannot
recur quietly.

**CORBA 자산을 운영한다면** 관계있는 부분은 ORB 코어와 `idl-diff`입니다 — 기존
피어와 대화하는 MIT 구현, 그리고 배포된 피어가 견디지 못할 계약 변경을 거부하는
게이트. **AI 에이전트를 레거시 인터페이스에 붙이고 있다면**
`orbweaver-forge`와 `orbweaver-mcp`입니다 — 기계가 읽는 의미가 붙은 계약과,
기본 거부에 감사 로그를 남기는 에이전트 경계. **DDS 쪽이라면** IDL 프런트엔드가
공통 지반입니다. OMG IDL 4.x는 DDS-XTypes가 쓰는 바로 그 언어입니다.
**엔지니어링 자체가 목적이라면** 볼 문서는 [`CLAUDE.md`](CLAUDE.md)입니다. 대부분이
"테스트가 아무것도 재지 않으면서 초록일 수 있는 방법" 목록이며, 하나하나 어렵게
발견해서 조용히 재발하지 못하도록 적어 둔 것입니다.

---

## Contributing / 기여

The most useful contribution is a **refutation**. Every claim in
[`COMPONENTS.md`](docs/COMPONENTS.md) and every transparency row in
[`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6.1 is meant to
be refutable by a test — if you can write one that goes red, that is worth more
than a feature.

Concretely, in rough order of value:

1. **A leak in one of the five transparencies** — a way for a caller to tell
   where a target runs, what implements it, what it is written in, or whether it
   is loaded.
2. **A gate that is green while measuring nothing.** Run the negative control
   before trusting any group in the harness, including the ones already there.
3. **A wire divergence against a real peer**, recorded with provenance.
4. **A challenge to one of the four Phase 0 assumptions.**

Issues and discussion are welcome in English or Korean. If you add a corpus
file, it goes in with the change that motivated it and with
`./spikes/differential.sh --require omniidl,jacorb_idl --record` — the workspace
test suite checks that verdict, so a file added without it fails for everybody
rather than only for whoever runs the harness.

가장 값나가는 기여는 **반증**입니다.
[`COMPONENTS.md`](docs/COMPONENTS.md)의 모든 주장과
[`D029`](docs/decisions/D029-what-a-complete-orb-would-mean.md) §6.1의 모든
투명성 행은 테스트로 반증 가능하도록 쓰였습니다. 빨갛게 만드는 테스트를 쓸 수
있다면 기능 하나보다 값이 나갑니다. 가치 순으로 — (1) 다섯 투명성 중 하나의
구멍: 호출자가 대상의 위치·구현·언어·적재 상태를 알아낼 수 있는 경로. (2)
아무것도 재지 않으면서 초록인 게이트. 이미 있는 그룹을 포함해, 신뢰하기 전에 부정
대조군을 돌려 보십시오. (3) 실제 피어와의 와이어 불일치를 출처와 함께 기록.
(4) Phase 0의 네 가정 중 하나에 대한 반론. 이슈와 토론은 영어·한국어 모두
환영합니다. 코퍼스 파일을 추가한다면 그것을 유발한 변경과 함께,
`./spikes/differential.sh --require omniidl,jacorb_idl --record`와 함께
들어갑니다 — 워크스페이스 테스트가 그 판정을 검사하므로, 없이 추가된 파일은
하네스를 돌리는 사람에게만이 아니라 모두에게 실패합니다.

---

## References / 참고 자료

**Standards / 표준**
[OMG IDL 4.2](https://www.omg.org/spec/IDL/4.2/) ·
[CORBA 3.4 Interoperability (GIOP/IIOP)](https://www.omg.org/spec/CORBA/3.4/Interoperability/PDF) ·
[MCP Tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) ·
[JEP 320](https://openjdk.org/jeps/320)

**Reference implementations — interop targets, not dependencies / 상호운용 대상, 의존성 아님**
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

## License / 라이선스

MIT — see [`LICENSE`](LICENSE). Dependencies are held to the same bar: MIT or
MIT-equivalent, or written here. The one exception is data we cannot originate,
which is disclosed in [`NOTICE`](NOTICE) and recorded under
[`docs/decisions/`](docs/decisions/).

MIT — [`LICENSE`](LICENSE) 참조. 의존성도 같은 기준입니다: MIT 또는 MIT 동등이거나,
여기서 직접 작성합니다. 유일한 예외는 우리가 원저작할 수 없는 데이터이며,
[`NOTICE`](NOTICE)에 공개하고 [`docs/decisions/`](docs/decisions/)에 기록합니다.
