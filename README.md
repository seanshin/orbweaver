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

## Why this exists / 왜 이것을 시작했는가

#### 0. Models started calling each other, and nothing said who was calling

The premise this project began from. When one program calls another and a person
wrote both, trust is arranged out of band: someone read the docs, someone issued
a key, someone will notice when it misbehaves. **That arrangement is what stops
working when the caller is a model** — because the caller is chosen at runtime,
the callee may be a model too, and there is no human in the loop at the moment
of the call to supply the judgement the design assumed.

What is actually needed between two models is not transport. It is four things,
and each of them is an *identity* or a *proof*:

**0.1 — The callee must be nameable without being located.** An agent must be
able to hold something that says *this target*, not *this URL*. A URL is an
address: it says where to send bytes and nothing about what is there, it breaks
when the thing moves, and two of them pointing at the same object are not
comparable. CORBA's unit is the **object reference**, and it is the opposite: it
carries a repository id (*what interface this is*), an object key (*which
instance*), and an address that is allowed to change underneath it — the caller
holding the reference does not have to notice. That is why `LOCATION_FORWARD`
exists in the wire protocol rather than in a load balancer. Handing an agent a
reference hands it a **capability**: the authority to invoke a specific target,
separable from knowing where that target runs.

**0.2 — The callee must be able to describe itself, in a form a machine can
check.** An agent that has never seen an interface must be able to learn it at
the moment of the call and then invoke it *correctly*, and "correctly" has to be
decidable by something other than the model's confidence. CORBA has carried this
since 1996: the **Interface Repository** answers *what operations does this have,
with what parameter types, raising what*, and `TypeCode` makes the answer a value
rather than prose. Combined with the **Dynamic Invocation Interface**, a caller
discovers and invokes with no generated code. The 2025 rediscovery of this idea
is MCP's `tools/list`; the difference is that IDL's description is a **type**, so
a wrong call is a marshalling failure rather than a plausible-looking mistake.

**0.3 — The caller must be able to prove who it is, through intermediaries.**
Agents call through other agents. A chain of three where the middle one is
trusted to *say* who the first one was is not proof, it is a convention, and it
fails exactly when it matters. CORBA specifies this instead of leaving it to a
header: **CSIv2** carries an authenticated identity and, separately, an asserted
one for delegation, so *acting as* and *claiming to be* are different fields on
the wire rather than the same string. In this repository that is
`crates/orbweaver-giop/src/csiv2.rs`. It is honest to say what is not done: TLS
and token exchange are still open (Phase 5 is half landed), so today the
mechanism is specified and carried, not yet secured end to end.

**0.4 — The contract must say what a call *does*, before it is made.** An agent
deciding whether to call needs to know whether the operation is destructive,
whether retrying is safe, what permission it requires, and whether an argument is
personal data. IDL types the *shape* and says nothing about the *meaning*, which
is the gap **SIDL** exists to close — `//@ ai_effect`, `//@ ai_authz`,
`//@ ai_idempotent`, `//@ ai_pii` — so an authorisation decision reads the
contract rather than guessing from the operation's name. And because the
declaration is in the contract, the enforcement point can sit outside both
parties: `orbweaver-mcp`'s interceptor chain refuses by default, records
`ALLOW caller=… target=… operation=…` for what it permits, and can run a call as
a **dry run** that reports what would have happened without doing it.

**0.5 — Why an existing object model rather than a new protocol.** The four
requirements above are not exotic; every one of them can be assembled today out
of OpenAPI plus OAuth plus a gateway plus a schema registry plus a convention
about headers. The reason to start from CORBA is that there they are **one
object model, already specified, with independent implementations to check
against** — the reference, the repository, the identity token and the
interception point are defined in relation to each other rather than bolted
together per deployment. That is also what makes the work *falsifiable*: a
mistake shows up as a stock omniORB or JacORB refusing our bytes, which is a
harsher reviewer than any test written by the same author as the code.

The wager, stated plainly so it can be judged: **the properties agents need from
each other are the ones distributed-object systems spent the 1990s specifying,
and the reason CORBA lost was ergonomics for human programmers — a cost an agent
does not pay.** Verbosity, strictness, and a compiler that refuses are burdens to
a person typing and assets to a model generating.

매력적일 뿐 아니라 실제로 **도달 가능한** 이유입니다.

**0. 모델들이 서로를 호출하기 시작했는데, 누가 거는지를 말해 주는 것이 없었다.**
이 프로젝트가 출발한 전제입니다. 한 프로그램이 다른 프로그램을 호출하고 양쪽을
사람이 썼을 때, 신뢰는 대역 밖에서 마련됩니다 — 누군가 문서를 읽었고, 누군가 키를
발급했고, 오작동하면 누군가 알아챌 것입니다. **호출자가 모델이 되는 순간 그 배치가
작동을 멈춥니다.** 호출자는 런타임에 정해지고, 피호출자도 모델일 수 있으며,
그 설계가 전제한 판단을 공급해 줄 사람이 호출 시점에 없기 때문입니다.

두 모델 사이에 실제로 필요한 것은 전송이 아닙니다. 네 가지이고, 각각이 **신원**
아니면 **증명**입니다.

**0.1 — 피호출자는 위치를 몰라도 지칭될 수 있어야 한다.** 에이전트는 *이 URL*이
아니라 *이 대상*이라고 말하는 무언가를 들 수 있어야 합니다. URL은 주소입니다 —
바이트를 어디로 보낼지만 말하고 거기 무엇이 있는지는 말하지 않으며, 대상이
옮겨가면 깨지고, 같은 객체를 가리키는 두 URL은 비교되지 않습니다. CORBA의 단위인
**객체 참조**는 정반대입니다. 리포지터리 ID(*어떤 인터페이스인가*), 객체
키(*어느 인스턴스인가*), 그리고 **밑에서 바뀌어도 되는** 주소를 함께 싣습니다.
참조를 든 호출자는 그 변화를 알아챌 필요가 없습니다. `LOCATION_FORWARD`가 로드
밸런서가 아니라 **와이어 프로토콜 안에** 있는 이유가 이것입니다. 에이전트에게
참조를 건네는 것은 **능력(capability)**을 건네는 것입니다 — 특정 대상을 호출할
권한을, 그 대상이 어디서 도는지 아는 것과 분리해서.

**0.2 — 피호출자는 자기를 기술할 수 있어야 하고, 그 기술은 기계가 검사할 수 있는
형태여야 한다.** 한 번도 본 적 없는 인터페이스를 호출 시점에 배우고 **정확히**
호출할 수 있어야 하며, 그 "정확히"는 모델의 확신이 아닌 다른 것이 판정해야 합니다.
CORBA는 1996년부터 이것을 갖고 있습니다 — **인터페이스 리포지터리**가 *이것에는
어떤 연산이 있고, 파라미터 타입은 무엇이며, 무엇을 raise 하는가*에 답하고,
`TypeCode`가 그 답을 산문이 아니라 **값**으로 만듭니다. **동적 호출
인터페이스(DII)**와 합치면 생성된 코드 없이 발견하고 호출합니다. 2025년에 이
발상을 다시 찾은 것이 MCP의 `tools/list`이며, 차이는 IDL의 기술이 **타입**이라는
점입니다. 잘못된 호출이 그럴듯해 보이는 실수가 아니라 마샬링 실패가 됩니다.

**0.3 — 호출자는 중간 경유자를 지나서도 자기가 누구인지 증명할 수 있어야 한다.**
에이전트는 다른 에이전트를 거쳐 호출합니다. 셋으로 이어진 사슬에서 가운데가 첫
번째가 누구였는지 **말해 주는 것을 믿는** 구조는 증명이 아니라 관례이고, 하필
중요한 순간에 무너집니다. CORBA는 이것을 헤더 관례에 맡기지 않고 명세합니다 —
**CSIv2**가 인증된 신원과, 위임을 위한 주장된 신원을 **따로** 싣습니다. *~로서
행위한다*와 *~라고 주장한다*가 같은 문자열이 아니라 와이어 위의 다른 필드입니다.
이 저장소에서는 `crates/orbweaver-giop/src/csiv2.rs`입니다. 안 된 것을 밝혀
두는 편이 정직합니다 — TLS와 토큰 교환은 아직 열려 있어(Phase 5 절반 착지), 오늘
이 메커니즘은 명세되고 실려 다니지만 종단 간으로 보호되지는 않습니다.

**0.4 — 계약은 호출이 *무엇을 하는지*를 호출 전에 말해야 한다.** 호출할지 말지
정하는 에이전트는 그 연산이 파괴적인지, 재시도가 안전한지, 어떤 권한이 필요한지,
인자가 개인정보인지를 알아야 합니다. IDL은 *형태*에 타입을 붙일 뿐 *의미*는 말하지
않으며, 그 간극을 메우려고 **SIDL**이 있습니다 — `//@ ai_effect`, `//@ ai_authz`,
`//@ ai_idempotent`, `//@ ai_pii`. 그래서 인가 판단이 연산 이름에서 추측하는 대신
계약을 읽습니다. 그리고 선언이 계약 안에 있으므로 집행 지점을 양쪽 바깥에 둘 수
있습니다 — `orbweaver-mcp`의 인터셉터 체인은 기본적으로 거부하고, 허용한 것은
`ALLOW caller=… target=… operation=…`으로 기록하며, 호출을 **드라이런**으로 돌려
실제로 하지 않은 채 무슨 일이 일어났을지 보고할 수 있습니다.

**0.5 — 왜 새 프로토콜이 아니라 기존 객체 모델인가.** 위 네 가지는 특별하지
않습니다. 오늘도 OpenAPI + OAuth + 게이트웨이 + 스키마 레지스트리 + 헤더에 대한
관례를 조립하면 전부 만들 수 있습니다. CORBA에서 출발할 이유는, 거기서는 그것들이
**하나의 객체 모델로 이미 명세되어 있고, 대조할 독립 구현이 존재한다**는
점입니다 — 참조, 리포지터리, 신원 토큰, 인터셉션 지점이 배포마다 붙여 맞추는 것이
아니라 서로에 대한 관계로 정의되어 있습니다. 그리고 그것이 이 작업을 **반증
가능하게** 만듭니다. 실수는 순정 omniORB나 JacORB가 우리 바이트를 거부하는 것으로
드러나며, 그쪽이 코드와 같은 사람이 쓴 어떤 테스트보다 가혹한 검토자입니다.

판정받을 수 있도록 내기를 분명히 적어 둡니다 — **에이전트가 서로에게 필요로 하는
성질은 분산 객체 시스템이 1990년대를 들여 명세한 바로 그것이고, CORBA가 패배한
이유는 인간 프로그래머에게의 사용성인데, 그 비용은 에이전트가 치르지 않는다.**
장황함, 엄격함, 거부하는 컴파일러는 타이핑하는 사람에게는 짐이고 생성하는
모델에게는 자산입니다.

---

## How to read this / 이 문서 읽는 법

The README answers six questions and hands off to a document for anything that
needs a number. It carries no figures of its own, because a number copied into a
second file stops tracking the first one and nothing announces that it has. What
a number in this project is allowed to claim is set out under
[What a number here means](#what-a-number-here-means--수치가-뜻하는-것).

이 README는 여섯 가지 질문에 답하고, 수치가 필요한 것은 해당 문서로 넘깁니다.
수치 자체는 여기 적지 않습니다. 한 번 옮겨 적은 수치는 원본이 바뀌어도 따라
바뀌지 않고, 어긋났다는 사실을 알려 줄 장치도 없기 때문입니다. 이 프로젝트에서
수치를 어떻게 읽고 쓰는지는
[수치가 뜻하는 것](#what-a-number-here-means--수치가-뜻하는-것)에 자세히 적었습니다.

| | Question | 질문 | Section |
|---|---|---|---|
| **What** | What is this, concretely? | 이게 정확히 뭔가 | [What it is](#what-it-is--무엇인가) |
| **Why** | Why was this started at all? | 애초에 왜 시작했나 | [Why this exists](#why-this-exists--왜-이것을-시작했는가) |
| **Why** | And why CORBA, in 2026, under MIT? | 그리고 왜 2026년에 CORBA를, 왜 MIT로 | [Why CORBA](#why-corba-and-why-now--왜-corba인가-왜-지금인가) |
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

둘을 하나로 묶는 것은 **IDL이 기계가 기계의 출력을 검사할 수 있을 만큼 엄격한
계약 언어**라는 점입니다. 모델은 그럴듯하지만 틀릴 수
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
§6이며 다른 어디에도 다시 적지 않습니다. 여기서 따라 나오는 것이 **투명성은
확인하는 것이 아니라 구멍을 사냥하는 것**이라는 원칙입니다. 구멍을 막는 변경이
기능을 더하는 변경보다 앞서고, D029 §6.1의 다섯 행은 각자 지금까지 발견된
구멍을 — 아직 열려 있는 것까지 — 이름으로 적고 있습니다.

---

## Why CORBA, and why now / 왜 CORBA인가, 왜 지금인가

The premise is above, in [Why this exists](#why-this-exists--왜-이것을-시작했는가).
These four are why the answer turned out to be **reachable** rather than merely
appealing — in the order they actually mattered.

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

전제는 위 [왜 이것을 시작했는가](#why-this-exists--왜-이것을-시작했는가)에 있습니다. 아래 넷은
그 답이 **도달 가능한** 이유이며, 실제로 중요했던 순서대로입니다.

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

### What a number here means / 수치가 뜻하는 것

#### Why a number gets set at all / 애초에 수치를 두는 이유

The completion criterion in D029 §6 is a **property**, not a list of features.
A property is only workable if claims about it can be refuted, and a number is
the shape a refutable claim takes: *this many of the corpus compiles*, *this
many operations answered over the wire*, *this many implementations inherit the
default*. Prose can be argued with. A number can be run.

That is also the trap. **A number that cannot actually be refuted is worse than
prose, because it looks like it can be.** So before a figure is allowed into
this project, four things have to exist, and every drift incident in this
repository is one of the four missing:

| | What must exist | What its absence produces |
|---|---|---|
| **1** | a **denominator** — *this many out of what?* | a percentage that cannot be wrong |
| **2** | an **instrument** — the run that takes the reading | a target nothing can fail |
| **3** | a **procedure** — how the reading is taken, repeatably | a number nobody can take twice |
| **4** | a **decision it serves** — what changes at that value | a threshold picked to look rigorous |

Each has a real case behind it. **The denominator** killed the seven
per-stage "automation targets" this file used to carry — 95 / 90 / 80 / 100 /
100 / 85 / 90 — because *automation of what* was never answered: items,
operations, or human touches? Two of the seven stages are deterministic
programs, where a percentage is a category error rather than a hard goal.
**The instrument** is the column in [`PLAN.md` §11](docs/PLAN.md#11-success-metrics)
that separates a gate from a wish; rows whose instrument reads *none* are filed
below the table as **aspirations**, each carrying the trigger that would give it
one, rather than sitting in the table looking enforceable. **The procedure**
is what "≥80% reduction versus manual" never had: no pilot, no cooperative
owner, and no logged manual baseline, so nothing could have failed it.
**The decision** is why `spikes/entry_cost.py` reports and does not gate — no
threshold for *too many things a newcomer must learn* is defensible, so the
project declines to invent one and prints the figure instead.

That last one is the case worth reading twice. **Refusing to set a number is a
result, not an omission**, and it is recorded as one.

#### The four kinds, and the reason they get confused / 네 종류, 그리고 섞이는 이유

Once a figure is legitimate it is still one of four different things, and they
do not mean the same:

- a **reading** — what was true when someone looked. It ages.
- a **floor** — a regression tripwire. Its value is *what we had when we last
  looked*, chosen so that legitimate growth does not turn it red. It is not a
  description of anything.
- a **threshold** — a point on a continuum where an action changes, chosen by a
  person who has to defend the choice.
- a **given** — a constant from outside: a specification, an OS, a peer's
  version. Not ours to pick.

**And here is the root cause of everything in the next subsection: on the page
they are the same characters.** `5248` written in a sentence does not say
whether it means *there are 5248* or *there were at least 5248 the last time
anybody checked*. Nothing in Markdown carries provenance, and nothing
recompiles a sentence, so the reader supplies the missing half — and readers
supply *reading*, because that is what a number in prose normally is. That is
how a floor of 5248 came to be quoted beside an actual 6016 without anybody
being careless, and why the rules below are about **provenance** rather than
about arithmetic. They are not hygiene for its own sake; they are the only way
a figure can carry which of the four it is.

#### The rules that follow from it / 거기서 따라 나오는 규칙

Numbers are the easiest thing in a repository to get wrong, because a wrong one
looks exactly like a right one and nothing recompiles a sentence. Every rule
below was learned by shipping the mistake first.

**A figure carries the date it was measured, or it comes from a script.** A
count in prose is true on the day it is written and drifts every day after.
Where a figure has to appear in a document, it either says when it was taken or
it is written by [`spikes/coverage_tables.py`](spikes/coverage_tables.py), which
computes it from the run. The header of this file says `v0.7.0 (2026-08-26)` for
the same reason.

**A floor is not a figure.** A gate pinned as `>= N` proves that nothing
regressed and proves *nothing at all* about the count. Quote `N` in prose as if
it were today's measurement and the sentence drifts upward in silence while the
gate stays green over it, because green is all `>= N` was ever going to say.
Measured 2026-08-25, twice in one sweep: `COMPONENTS.md` said the AnyJSON leg
crosses `5248/5248` when the floor was 5248 and the actual was **6016**, and
that the Python sweep crosses `172 values / 137 calls` when the floor was
170/137 and the actual was **182/139** — and the harness's own comment beside
that floor said `170 / 137` too, so the document and the gate agreed with each
other and both disagreed with the run. Where a document and a gate quote the
same number, the document says which one is the floor.

**A number carries its method, not only its date.** This is the newest rule
here and it was added on 2026-08-27 after being demonstrated live: a scan of
this workspace reported `63 of 79` `Dispatch` implementations overriding
`knows`, and the real answer is `53 of 80`. Nothing was mistyped. The scan
terminated each `impl` body at the *next* `impl` rather than at its matching
brace, so it read later blocks' methods as belonging to earlier ones — and it
under-reported the defect it was counting, which is the direction that gets a
number believed. A count is only as good as the parse under it, so a figure
that matters is produced by something a second implementation can disagree
with, and the disagreement is resolved rather than averaged.

**A count of items is not a count of causes**, and the second number is the one
worth having. Phase 0's batch was 20 files, 7 failures and **1** cause.
Reporting "7 defects fixed" would have been arithmetically true and would have
described the wrong thing, because the seven were one rule nobody had written
down. Every batch reports its size, its first-pass rate, the causes found with
their affected counts, and what was codified — and the first-pass rate and the
round count are reported *separately*, because they measure the generator and
the oracle respectively and averaging them measures neither.

**A number a model produced about its own work is labelled indicative**, in the
same breath as the number and not in a footnote. Where the generator and the
evaluator are the same model, the figure is evidence about consistency and not
about correctness.

**An unmeasured check is a failure, and never a zero.** If a fixture will not
start, the count it would have produced does not become 0 — the run reports that
it could not be taken. This is why the harness prints what it could not measure
in each group's own words, and why a `SKIPPED` is counted in the verdict rather
than mentioned in passing.

**Some numbers are deliberately not computed.** No run in this tree produces an
"automation percentage" per pipeline stage, and no score is derived from the
transparency ledger. The ledger's own arithmetic is the reason: a shrinking
unmeasured list is progress only when a run actually closed a leak, and it looks
identical to nobody looking. A number that cannot distinguish those two states
is worse than no number, because it will be quoted.

**애초에 수치를 두는 이유.** D029 §6의 완성 기준은 기능 목록이 아니라
**성질**입니다. 성질은 그에 대한 주장을 반증할 수 있을 때만 작업 가능해지고,
수치는 반증 가능한 주장이 취하는 형태입니다 — *코퍼스 이만큼이 컴파일된다*,
*연산 이만큼이 와이어로 답했다*, *구현 이만큼이 기본값을 상속한다*. 산문은
말싸움이 되지만 수치는 돌려 볼 수 있습니다.

그리고 바로 그것이 함정입니다. **실제로는 반증할 수 없는 수치는 산문보다
나쁩니다. 반증할 수 있어 보이기 때문입니다.** 그래서 수치가 이 프로젝트에
들어오기 전에 네 가지가 있어야 하고, 이 저장소에서 일어난 모든 어긋남은 그 넷 중
하나가 빠진 경우입니다.

| | 있어야 하는 것 | 없을 때 생기는 것 |
|---|---|---|
| **1** | **분모** — *무엇 중에 이만큼인가* | 틀릴 수가 없는 백분율 |
| **2** | **계측기** — 값을 읽는 실행 | 아무것도 실패시킬 수 없는 목표 |
| **3** | **절차** — 그 값을 반복해서 읽는 방법 | 두 번 잴 수 없는 수치 |
| **4** | **그 수치가 답하는 결정** — 그 값에서 무엇이 달라지는가 | 엄밀해 보이려고 고른 임계값 |

넷 다 실제 사례가 있습니다. **분모**는 이 파일이 한때 달고 있던 단계별 "자동화
목표" 일곱 개 — 95 / 90 / 80 / 100 / 100 / 85 / 90 — 를 없앴습니다. *무엇의*
자동화인지가 끝내 답해지지 않았기 때문입니다. 항목인지, 연산인지, 사람의 손이
닿는 횟수인지. 게다가 일곱 중 둘은 결정론적 프로그램이라 거기서의 백분율은 높은
목표가 아니라 범주 오류입니다. **계측기**는 관문과 소망을 가르는
[`PLAN.ko.md` §11](docs/PLAN.ko.md#11-성공-지표)의 열입니다. 계측기가 *없음*인
행은 표 안에서 강제력 있어 보이게 앉아 있는 대신, 표 아래 **지향**으로 내려가
계측기를 얻으려면 무엇이 관측되어야 하는지 방아쇠를 답니다. **절차**는 "수작업
대비 80% 이상 단축"이 끝내 갖지 못한 것입니다 — 파일럿도, 협조하는 소유자도,
기록된 수작업 기준선도 없었으므로 그 수치는 애초에 실패할 수가 없었습니다.
**결정**은 `spikes/entry_cost.py`가 보고만 하고 게이트가 되지 않는 이유입니다.
*새로 온 사람이 익혀야 할 것이 너무 많다*의 임계값으로 방어 가능한 수가 없으므로,
프로젝트는 하나 지어내기를 거절하고 수치만 출력합니다.

마지막 사례는 두 번 읽을 값이 있습니다. **수치를 두기를 거절한 것은 누락이 아니라
결과이며**, 결과로 기록됩니다.

**네 종류, 그리고 섞이는 이유.** 정당한 수치가 되고 나서도 그것은 여전히 네 가지
중 하나이고, 넷은 같은 뜻이 아닙니다.

- **판독값** — 누군가 봤을 때 참이었던 것. 나이를 먹습니다.
- **하한** — 퇴행 감지용 걸림선. 그 값은 *마지막으로 봤을 때 우리가 갖고 있던
  수*이며, 정당한 증가가 빨간불이 되지 않도록 그렇게 고른 것입니다. 무언가에
  대한 서술이 아닙니다.
- **임계값** — 연속량 위에서 행동이 바뀌는 지점. 고른 사람이 그 선택을 방어해야
  합니다.
- **주어진 값** — 바깥에서 온 상수. 명세, 운영체제, 피어의 버전. 우리가 고를 수
  있는 것이 아닙니다.

**그리고 다음 절 전체의 근본 원인이 여기 있습니다 — 지면에서 넷은 같은
글자입니다.** 문장 속의 `5248`은 *5248개가 있다*인지 *마지막으로 확인했을 때
최소 5248개였다*인지를 말해 주지 않습니다. 마크다운은 출처를 싣지 않고 문장을
다시 컴파일하는 것도 없으므로, 빠진 절반은 읽는 사람이 채웁니다 — 그리고
읽는 사람은 **판독값**으로 채웁니다. 산문 속의 수치는 보통 그것이기 때문입니다.
아무도 부주의하지 않았는데 하한 5248이 실제 6016 옆에 인용된 경위가 이것이고,
아래 규칙들이 산술이 아니라 **출처**에 관한 것인 이유도 이것입니다. 규칙 자체가
목적인 위생 수칙이 아니라, 수치가 자기가 넷 중 무엇인지를 지고 다닐 수 있는
유일한 방법입니다.

**거기서 따라 나오는 규칙.** 수치는 저장소에서 가장 틀리기 쉬운 것입니다. 틀린
수치는 맞은 수치와 똑같이 생겼고, 문장을 다시 컴파일하는 것은 없기 때문입니다.
아래 규칙은 전부 먼저 틀려 보고 얻은 것입니다.

**수치에는 측정한 날짜가 붙거나, 아니면 스크립트가 씁니다.** 산문에 적힌 개수는
쓴 날에만 참이고 그다음 날부터 매일 어긋납니다. 문서에 수치가 꼭 나와야 하면
측정 시점을 적거나, 실행 결과에서 계산해 주는
[`spikes/coverage_tables.py`](spikes/coverage_tables.py)가 씁니다. 이 파일 머리에
`v0.7.0 (2026-08-26)`이라 적은 것도 같은 이유입니다.

**하한은 수치가 아닙니다.** `>= N`으로 고정한 게이트는 퇴행이 없었다는 것을
증명할 뿐, 개수에 대해서는 **아무것도** 증명하지 않습니다. `N`을 오늘의
측정치처럼 인용하면 그 문장은 조용히 위로 어긋나고, 게이트는 그 위에서 초록으로
남습니다. `>= N`이 할 수 있는 말이 초록뿐이기 때문입니다. 2026-08-25 한 번의
스윕에서 두 건 나왔습니다 — `COMPONENTS.md`가 AnyJSON 다리를 `5248/5248`이라
적었는데 하한이 5248이고 실제는 **6016**이었고, Python 스윕을 `172 values / 137
calls`라 적었는데 하한이 170/137이고 실제는 **182/139**였습니다. 게다가 하네스
자신의 주석도 `170 / 137`이라 적고 있어서, 문서와 게이트가 서로 일치하면서 둘
다 실행 결과와 어긋나 있었습니다. 문서와 게이트가 같은 수를 인용하는 자리에서는
어느 쪽이 하한인지 문서가 밝힙니다.

**수치에는 날짜만이 아니라 방법도 붙습니다.** 여기서 가장 새로운 규칙이고,
2026-08-27에 실연으로 얻었습니다. 이 워크스페이스를 훑은 스캔이 `knows`를
재정의하는 `Dispatch` 구현을 `79개 중 63개`로 보고했는데, 실제 답은 `80개 중
53개`였습니다. 오타는 없었습니다. 스캔이 각 `impl` 본문의 끝을 짝이 맞는 중괄호가
아니라 **다음 `impl`**로 잡아서, 뒤 블록의 메서드를 앞 블록 것으로 읽었습니다.
그래서 자기가 세고 있던 결함을 실제보다 **적게** 보고했는데, 그쪽이 바로 수치가
믿겨지는 방향입니다. 개수는 그 아래 깔린 파싱만큼만 정확하므로, 중요한 수치는
두 번째 구현이 반박할 수 있는 방식으로 만들고, 불일치가 나오면 평균 내지 않고
해소합니다.

**항목의 개수는 원인의 개수가 아니며**, 값이 나가는 쪽은 두 번째 수치입니다.
Phase 0의 배치는 20건, 실패 7건, 원인 **1개**였습니다. "결함 7건 수정"은 산술적으로
참이지만 엉뚱한 것을 묘사합니다 — 그 일곱은 아무도 적어 두지 않은 규칙 하나였기
때문입니다. 모든 배치는 규모, 1차 통과율, 영향 개수와 함께 찾은 원인, 그리고
성문화한 것을 보고합니다. 1차 통과율과 라운드 수는 **따로** 보고합니다. 각각
생성기와 오라클을 재는 수치라서, 합치면 어느 쪽도 재지 못합니다.

**모델이 자기 작업에 대해 낸 수치는 참고치로 표시합니다.** 각주가 아니라 그
수치와 같은 호흡에서 밝힙니다. 생성기와 평가기가 같은 모델이면 그 수치는
일관성에 대한 증거이지 정확성에 대한 증거가 아닙니다.

**측정하지 못한 검사는 실패이지 0이 아닙니다.** 픽스처가 뜨지 않았다면 그것이
냈을 개수는 0이 되는 것이 아니라, 잴 수 없었다고 보고됩니다. 하네스가 재지 못한
것을 그룹 자신의 말로 출력하고, `SKIPPED`를 지나가는 말이 아니라 판정에 계수하는
이유입니다.

**일부러 계산하지 않는 수치도 있습니다.** 이 트리의 어떤 실행도 파이프라인
단계별 "자동화 백분율"을 내지 않고, 투명성 원장에서 점수를 뽑지 않습니다. 원장
자신의 산술이 그 이유입니다 — 미측정 목록이 줄어든 것은 어떤 실행이 실제로
구멍을 막았을 때만 진전인데, 아무도 들여다보지 않아 줄어든 것과 겉으로는
구별되지 않습니다. 그 두 상태를 구별하지 못하는 수치는 없는 것만 못합니다.
인용될 것이기 때문입니다.

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
목록이 줄어든 것은 어떤 실행이 실제로 구멍을 막았을 때만 진전입니다. 아무도
들여다보지 않아서 줄어든 것과 겉으로는 구별되지 않습니다.

### The roadmap / 로드맵

What stands between here and every transparency row being **measured** is not
one kind of thing. One item is a decision only the owner can make; the rest are
wire changes with a published specification behind them, and one is a record
rather than any code at all. The order below is
[`D035`](docs/decisions/D035-the-reference-the-orb-hands-out.md) §8's
recommendation, which is a recommendation and not a schedule — it is stated so
it can be rejected.

| | Stage | Closes | Isolated to |
|---|---|---|---|
| **R1** | `TAG_FT_GROUP` (27) read and written — domain id, group id, **version** | nothing yet; it is content nothing acts on | `orbweaver-giop`, IOR only |
| **R2** | `FT_GROUP_VERSION` (12) service context sent with the version dialled | nothing yet; an unknowing peer ignores it | the request path |
| **R3** | server answers `LOCATION_FORWARD_PERM` when the version it is given is stale | **a caller holding a replaced reference is told, rather than failing** | one `Dispatch` |
| **R4** | `TAG_FT_PRIMARY` (28) honoured as a dial *preference* | dial order only — the spec says correctness does not depend on it | `Connection::connect` |
| **B** | record the bootstrap leak as **irreducible in a single-node deployment** | moves the Lifecycle row from *unmeasured* to *measured, leaks at the bootstrap* | a leak-test leg and a `D029` §6.1 row; no wire change |
| **G1** | a reference *arriving* at a foreign servant is a handle it cannot invoke — `D029` §6.1.1 item 4 | the Language row's last named leak | `orbweaver-gen`'s seam |
| **L0** | **Decision X** — is the reference `Orb::server` hands out *indirect*? | the **Location** row, not this one — see below | a decision record; no code until it is answered |

**R1 and R2 change no wire behaviour at all**, which is why they are first: a
component nobody acts on and a service context an unknowing peer ignores. The
first half of the FT work is close to free to try, and R3 is where behaviour
changes.

**L0 is last here and that is a change from this file's first version**, which
listed it first because `D029` framed X as *the* lifecycle decision. D035 §4
answers the question D029 required — X claims **displacement**, moving the leak
from N server addresses to one forwarding address, and cannot reach zero — and
§6.2 shows FT and X answer **different rows**: X minimises what a caller can
see, FT maximises what a caller can survive. X is deferred, not refused; it
remains the better answer for the Location row.

여기서 모든 투명성 행이 **측정됨**이 되기까지 남은 것은 한 종류가 아닙니다.
하나는 소유자만 내릴 수 있는 결정이고, 나머지는 공개 명세가 뒤를 받치는 와이어
변경이며, 하나는 코드가 아니라 기록입니다. 위 순서는
[`D035`](docs/decisions/D035-the-reference-the-orb-hands-out.md) §8의
**권고**이지 일정이 아닙니다 — 거절당할 수 있도록 적은 것입니다.

**R1과 R2는 와이어 동작을 전혀 바꾸지 않습니다.** 아무도 반응하지 않는 컴포넌트와,
모르는 피어가 무시하는 서비스 컨텍스트입니다. 그래서 앞에 둡니다. 동작이 바뀌는
지점은 R3입니다.

**L0이 맨 뒤인 것은 이 파일의 첫 판에서 바뀐 것입니다.** 처음에는 D029가 X를
*유일한* 생애주기 결정으로 놓았기에 맨 앞이었습니다. D035 §4가 D029가 요구한
질문에 답하면서 — X는 **전가**를 주장하며 구멍을 서버 주소 N개에서 포워딩 주소
하나로 옮길 뿐 0에 닿지 못합니다 — §6.2가 FT와 X가 **서로 다른 행**을 답한다는
것을 보였습니다. X는 거절이 아니라 유예이며, 위치 행에는 여전히 더 나은 답입니다.

**Why R1–R4 exist, and what they are not.** Fault Tolerant CORBA specifies an
**Interoperable Object Group Reference** — an IOR carrying several
`TAG_INTERNET_IOP` profiles, each with a `TAG_FT_GROUP` component naming the
group and *the version of the reference*, and a `FT_GROUP_VERSION` service
context by which a server detects that a client's reference is out of date.
This project already carries the transport half of that: an IOR holds several
profiles, `Connection::connect` dials each profile's address then its
`TAG_ALTERNATE_IIOP_ADDRESS` alternates then the next profile, and a successful
connection keeps **the whole IOR** so a restart gets the same failover. What is
missing is the specification's identity for it — *which group these addresses
are, and which version of it the caller holds.*

**R1–R4 are the reference half only.** The `ReplicationManager`,
`ObjectGroupManager`, `GenericFactory`, `FaultDetector` and `FaultNotifier` are
infrastructure with no consumer here, and building them would put a capability
ahead of a leak, which the priority-zero criterion forbids. Heartbeating
(`TAG_FT_HEARTBEAT_ENABLED`, 29) and transparent reinvocation (`FT_REQUEST`, 13)
are out for the same reason.

**And the honest limit: R1–R4 do not close the Lifecycle row.** Failing over
needs a second member to fail over *to*, which is a property of a deployment and
not of this repository. What they make measurable in one process is the smaller,
real claim — **a caller holding a reference that has since been replaced is
told so, instead of dialling something that is gone.** Saying they close the row would be exactly
the move D029 §6 exists to prevent.

여기서 모든 투명성 행이 **측정됨**이 되기까지 둘이 남았고, 둘은 종류가 다릅니다.
하나는 소유자가 내려야 할 결정이고, 하나는 공개 명세가 뒤를 받치는 와이어
변경입니다.

**R1–R4가 있는 이유.** 내결함성 CORBA는 **상호운용 객체 그룹 참조(IOGR)**를
명세합니다 — 여러 `TAG_INTERNET_IOP` 프로파일을 담은 IOR이고, 각 프로파일에
그룹과 *참조의 버전*을 적은 `TAG_FT_GROUP` 컴포넌트가 있으며, 서버가 클라이언트의
참조가 낡았음을 알아내는 `FT_GROUP_VERSION` 서비스 컨텍스트가 함께 있습니다. 이
프로젝트는 그것의 **전송 절반을 이미 갖고 있습니다** — IOR이 프로파일을 여럿
담고, `Connection::connect`가 프로파일마다 자기 주소 → alternates → 다음 프로파일
순으로 다이얼하며, 연결에 성공하면 **IOR 전체**를 보관해 재시작이 같은 페일오버를
받습니다. 없는 것은 그것에 대한 명세의 정체성입니다 — *이 주소들이 어느 그룹이고,
호출자가 그 그룹의 어느 버전을 들고 있는가.*

**R1–R4는 참조 절반뿐입니다.** `ReplicationManager`·`FaultDetector` 같은 인프라는
여기 소비자가 없고, 그것을 짓는 일은 구멍보다 기능을 앞세우는 것이라 0순위 기준이
금지합니다. 하트비트(`TAG_FT_HEARTBEAT_ENABLED`, 29)와 투명 재호출
(`FT_REQUEST`, 13)도 같은 이유로 제외입니다.

**그리고 정직한 한계 — R1–R4는 생애주기 행을 닫지 않습니다.** 페일오버하려면
넘어갈 두 번째 멤버가 있어야 하고, 그것은 배포의 성질이지 이 저장소의 성질이
아닙니다. 한 프로세스에서 이들이 측정 가능하게 만드는 것은 더 작고 진짜인
주장입니다 — **낡은 참조를 든 호출자가, 사라진 것에 걸어 보는 대신 낡았다는 말을
듣는다.** 이것이 행을 닫는다고 말하는 것이야말로 D029 §6이 막으려고 존재하는
움직임입니다.

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

### What it costs on disk, and keeping it down / 디스크에 무엇을 쓰는가

Worth knowing before a long session, because both numbers below are larger than
they look and one of them corrupts a search rather than merely filling a drive.

**A full workspace build reaches about 35 G**, measured 2026-08-27, and roughly
14 G of that is `target/*/incremental` — a rebuild cache, not a build product.
It regenerates on demand and nothing reads it across a clean checkout.

**Parallel work leaves a worktree behind per batch.** Landing merges the branch
and nothing removes the checkout, so they pile up: **70 mounted, 61 of them
already merged into `main`**, the same day. The disk is the smaller half of that
problem. The larger half is that a tree-wide `grep -r` reads them, so a scan
reports *other branches'* defects as this tree's — which happened to this
project's own early-exit gate on the day it was written, and is why that gate
scans `git ls-files` rather than walking a directory.

```bash
./spikes/reclaim.sh                        # dry run: what could be reclaimed
./spikes/reclaim.sh --apply                # remove merged worktrees and branches
./spikes/reclaim.sh --apply --incremental  # and drop the rebuild cache
```

It removes a worktree only when it sits under `.claude/worktrees/` **and** its
branch is an ancestor of `main` — every commit already landed. A branch that is
not merged is left alone and printed by name. Losing an unlanded batch to a
cleanup script is the one failure this must not have, so the script never
decides that question from a commit message.

**One practical trap, measured the same day.** Running a `RUSTFLAGS`-modified
cargo command in the default target directory — `RUSTFLAGS="-D warnings" cargo
test -p …` — changes the fingerprint for *everything*, so the next plain
`cargo test --workspace` rebuilds the whole tree. That cost a harness run more
than twenty minutes of build before its first group. The harness already knows:
the gates that need those flags give them their own `CARGO_TARGET_DIR`. Do the
same, or run them after the harness rather than before it.

긴 세션 전에 알아 둘 값이 있습니다. 아래 두 수치는 보기보다 크고, 그중 하나는
디스크를 채우는 것을 넘어 **검색을 오염시킵니다.**

**워크스페이스 전체 빌드는 약 35 G에 이릅니다**(2026-08-27 측정). 그중 약 14 G가
`target/*/incremental`로, 빌드 산출물이 아니라 재빌드 캐시입니다. 필요할 때 다시
만들어지고, 새로 받은 체크아웃에서는 아무도 읽지 않습니다.

**병행 작업은 배치마다 워크트리를 하나씩 남깁니다.** 착지는 브랜치를 병합할 뿐
체크아웃을 지우지 않으므로 쌓입니다 — 같은 날 **70개가 마운트되어 있었고 그중
61개가 이미 `main`에 병합**된 상태였습니다. 디스크는 그 문제의 작은 절반입니다.
큰 절반은 트리 전체를 훑는 `grep -r`이 그것들을 읽는다는 것입니다. 스캔이 *다른
브랜치의* 결함을 이 트리의 것으로 보고하게 됩니다 — 이 프로젝트의 early-exit
게이트가 작성된 날 실제로 그랬고, 그래서 그 게이트는 디렉터리를 걷는 대신
`git ls-files`를 훑습니다.

워크트리는 `.claude/worktrees/` 아래에 있고 **그 브랜치가 `main`의 조상일 때만**
— 즉 모든 커밋이 이미 착지했을 때만 — 제거합니다. 병합되지 않은 브랜치는 손대지
않고 이름을 출력합니다. 착지하지 않은 배치를 정리 스크립트에 잃는 것은 이 도구가
절대 가져서는 안 되는 실패이므로, 그 판단을 커밋 메시지로 내리지 않습니다.

**실무적인 함정 하나, 같은 날 측정.** 기본 target 디렉터리에서 `RUSTFLAGS`를 바꾼
cargo 명령 — `RUSTFLAGS="-D warnings" cargo test -p …` — 을 돌리면 *전체*의
지문이 바뀌므로, 다음번 평범한 `cargo test --workspace`가 트리를 통째로 다시
빌드합니다. 그 때문에 어떤 하네스 실행은 첫 그룹이 나오기까지 20분 넘게 빌드만
했습니다. 하네스는 이미 알고 있습니다 — 그 플래그가 필요한 게이트들은 자기
`CARGO_TARGET_DIR`을 따로 갖습니다. 같이 하거나, 하네스 앞이 아니라 뒤에서
돌리십시오.

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

결정 두 건은 여기에 이름을 적어 둘 만큼 자주 쓰입니다 —
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
