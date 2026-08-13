# CORBA 기반 분산 MoE 아키텍처 명세서 (개정판)

> **목적**: CORBA ORB와 핵심 서비스(Naming / Trading / Interface Repository / POA / Portable Interceptors / Security / LifeCycle)를 구축하는 팀이, LLM 파운데이션 모델의 MoE(Mixture of Experts) 구조와 **기업 모델 구성(Enterprise Model Composition)** 을 "분산 객체"로 재해석하여 **expert 적재·배치·라우팅·구성** 인프라를 구현할 수 있도록 정제한 설계 기준 문서.
>
> **대상 독자**: ORB 코어 및 CORBA 핵심 서비스를 설계·구현하는 엔지니어, MoE 라우팅/서빙/적재 인프라 설계자.
>
> **핵심 관점 두 가지**
> 1. LLM의 시냅스(가중치)가 아니라 **동적 디스패치 엣지(라우팅)** 와 **expert 적재(loading)** 를 객체화한다.
> 2. CORBA는 MoE의 **컨트롤 플레인**(적재·배치·발견·생명주기)을 맡고, **데이터 플레인**(포워드 패스 matmul)은 융합 커널에 그대로 둔다.

---

## 1. 설계 원칙

### 1.1 객체화의 올바른 입도(Granularity)

시냅스(개별 가중치)는 "메시지를 받아 계산하는 개체"가 아니라 행렬 안의 계수(coefficient)다. 개별 가중치를 분산 객체로 감싸면 나노초짜리 FLOP이 네트워크 왕복으로 바뀌어 오버헤드가 10⁹~10¹²배가 된다. **따라서 시냅스는 객체화 대상이 아니다.**

객체 모델은 계산 단위가 다음 두 조건을 만족할 때만 값어치를 한다.

1. **성김(Coarse)** — 인보케이션 오버헤드를 상쇄할 만큼 계산량/비용이 크다.
2. **동적·희소(Sparse & Dynamic)** — 어떤 단위를 켜거나 적재할지가 런타임에 결정된다.

MoE의 expert는 이 두 조건을 자연히 만족한다. 그러므로 이 명세의 객체화 단위는 **expert**이며, 중개 대상은 **gate/router의 디스패치**와 **expert의 적재·배치**다.

### 1.2 컨트롤 플레인 vs 데이터 플레인 (가장 중요한 구분)

파운데이션 모델 MoE에 CORBA를 적용할 수 있는지는 이 구분 하나로 결정된다.

| 구분 | 데이터 플레인 (Data Plane) | 컨트롤 플레인 (Control Plane) |
|---|---|---|
| 하는 일 | 토큰·레이어 단위 gate 계산, expert matmul, 융합 all-to-all | 어떤 expert를 어디에 언제 적재/축출/배치할지 결정 |
| 시간 스케일 | 마이크로초 (µs), 토큰마다 | 밀리초~초, 배치·요청·통계 주기 |
| substrate | 온-가속기 융합 커널 (NVLink/InfiniBand/RDMA) | **ORB + CORBA 핵심 서비스** |
| CORBA 적용 | **금지** (핫 루프 파괴) | **적용** (오버헤드 상쇄됨) |

> **철칙 1**: CORBA/ORB를 데이터 플레인(포워드 패스 핫 루프)에 넣지 않는다.
> **철칙 2**: expert 적재/축출은 토큰 단위로 하지 않는다. 라우팅의 *시간적 국소성*(최근 라우팅 히스토리·배치 통계)에 기반한 프리페치/상주 정책으로 구동한다.

### 1.3 두 개의 적용 레짐(Regime)

| 구분 | 레짐 1 — 타이트 MoE | 레짐 2 — 루즈 MoE / 컨트롤 플레인 (본 명세의 주 대상) |
|---|---|---|
| Expert 단위 | 모델 내부 FFN 서브블록 | 전문화된 서브모델 / 에이전트, 또는 **적재 대상 expert 가중치 블록** |
| 처리 주기 | 토큰·레이어 단위 (µs) | 요청·추론단계·적재결정 단위 (ms+) |
| CORBA의 역할 | 개념적 렌즈만 | **런타임 substrate** (중개·적재·발견·생명주기) |

---

## 2. 아키텍처 개관

3계층 + 플레인 분리 구조로 나눈다.

```
┌─────────────────────────────────────────────────────────────┐
│  얼굴 계층 (Face)  —  MCP / JSON-RPC                          │
│  · LLM·클라이언트를 향한 의미론적 발견·계약 (soft contract)   │
└───────────────┬─────────────────────────────────────────────┘
                │  MCP 툴 핸들 ↔ IOR 바인딩
┌───────────────┴─────────────────────────────────────────────┐
│  컨트롤 플레인 (Control Plane)  —  ORB Core + 핵심 서비스     │
│  · Trading Service : 능력·QoS·메모리·부하 기반 선택/적재 결정 │
│  · POA            : expert 적재·오프로딩 생명주기            │
│  · Interface Repo / Naming / Portable Interceptors           │
│  · IOR 관리, 위치 투명성                                      │
└──────┬──────────────────────────────────┬───────────────────┘
       │ 적재/축출/배치 명령 (ms+)         │ IOR (안정적 참조)
       ▼                                  ▼
┌──────────────────────────┐   ┌──────────────────────────────┐
│  데이터 플레인 (Data)     │   │  Expert 저장소 (Storage)      │
│  · 융합 커널 포워드 패스  │◀──│  · NVMe / 오브젝트 스토어     │
│  · 상주 expert matmul     │적재│  · cold expert 가중치         │
│  · all-to-all (RDMA)     │   │                               │
└──────────────────────────┘   └──────────────────────────────┘
       ▲
       │  데이터 플레인은 CORBA 밖. 컨트롤 플레인이 "무엇이 적재돼
       │  있는가"만 관리하고, 실제 matmul 라우팅은 커널이 수행.
```

---

## 3. MoE ↔ CORBA 컴포넌트 매핑

| MoE 개념 | CORBA 컴포넌트 | 플레인 | 역할 |
|---|---|---|---|
| Expert (가중치 블록/서브모델) | **분산 객체 + IOR** | 컨트롤 | 독립 주소지정·참조·적재 가능한 단위. |
| Expert 적재·오프로딩 | **POA (ServantActivator)** | 컨트롤 | 스토리지↔가속기 지연 로딩/언로딩, 생명주기. |
| 적재/배치 결정 | **Trading Service** | 컨트롤 | 능력·QoS·메모리·부하 기반 상주/프리페치/축출 결정. |
| Expert 로스터 | **Trading Service 등록소** | 컨트롤 | 고정 로스터 → 런타임 동적 로스터(elastic MoE). |
| Expert 능력·계약 | **Interface Repository** | 컨트롤 | 전문성·컨트랙트 버전 런타임 조회. |
| Expert 간 위임 | **IOR pass-by-reference** | 컨트롤 | 조합적 라우팅(promise pipelining). |
| 정책/안전/계측 | **Portable Interceptors** | 컨트롤 | 인증·쿼터·안전·텔레메트리 균일 삽입. |
| Named expert | **Naming Service** | 컨트롤 | 이름 기반 정적 바인딩(디버깅·핀). |
| gate matmul / all-to-all | **(CORBA 밖)** | 데이터 | 융합 커널이 수행. ORB 미개입. |

> **중요**: 게이팅 신경망의 친화도 계산과 expert matmul(데이터 플레인)은 CORBA가 대체하지 않는다. CORBA가 대체하는 것은 **적재·배치·발견·생명주기·조합**(컨트롤 플레인)이다.

---

## 4. 핵심 서비스 구현 명세

### 4.1 ORB Core

**책임**: 요청 마샬링/언마샬링, IOR 해석, 연결 관리, 컨트롤 플레인 method invocation 라우팅.

- **전송(Transport)**: 기본 IIOP(GIOP over TCP). 신규 구축 시 GIOP/CDR 직접 재구현보다 **현대 바이너리 RPC(Cap'n Proto, gRPC)** 위에 CORBA 의미론을 얹는 것을 권장(§9).
- **IOR(Interoperable Object Reference)**: 적재된 expert 인스턴스에 대한 이동 가능·안정적 핸들. 최소 필드 — 타입 ID, 엔드포인트(호스트·포트·프로토콜), 객체 키, QoS/배치 태그(전문성·비용·지연·현재 배치 노드). **오프로딩 후 재적재해도 객체 키로 참조 유효.**
- **CDR / 대용량 페이로드**: expert 가중치·Activation 텐서는 IOR로 참조만 전달하고 실체는 zero-copy(공유 메모리/RDMA/NVMe DMA) 경로로 이동. IOR에 텐서를 인라인하지 말 것.
- **호출 의미론**: 동기 외 비동기(AMI)·oneway 지원(적재 프리페치는 oneway로).

### 4.2 POA (Portable Object Adapter) — expert 적재기의 핵심

**책임**: expert 객체의 적재·오프로딩·생명주기.

- **Servant 관리 정책**:
  - `TRANSIENT` — 요청/프리페치 시 적재되는 임시 상주 expert.
  - `PERSISTENT` — 상태(적응/캐시/LoRA 델타)를 유지하는 영속 expert. 오프로딩 시 상태 보존.
- **ServantActivator = 적재기**: expert 가중치를 스토리지(NVMe/오브젝트 스토어)에서 가속기 메모리로 지연 로딩(lazy load)하고, 유휴·메모리 압력 시 언로드. 이것이 대규모 MoE(레이어당 수백 expert)를 한정된 가속기 메모리에서 돌리는 핵심 메커니즘.
- **객체 ID ↔ IOR 매핑**: POA가 expert에 안정적 객체 키를 부여 → 재적재/재배치 후에도 참조 유효성 유지.
- **적재 상태 머신**은 §5 참조.

### 4.3 Trading Service — 적재·배치 결정 엔진

**책임**: "능력 + QoS + 메모리 + 부하" 속성으로 (1) 라우팅 대상 expert 선택, (2) 적재·프리페치·축출 결정.

- **Service Offer(등록 항목)**: expert가 등록·heartbeat 시 제출하는 프로퍼티.
  - `specialization` (code, math, retrieval, vision …)
  - `cost`, `latency_p50/p99`, `load`
  - `residency` (RESIDENT / PREFETCHING / OFFLOADED)
  - `mem_footprint`, `placement_node`
  - `route_freq` (최근 라우팅 히스토리 기반 상주 우선순위)
- **질의(선택)**:
  ```
  specialization == 'math' AND latency_p99 < 200ms AND load < 0.8
  ORDER BY affinity_score DESC
  ```
- **적재 정책(§6)**: `route_freq` + `mem_footprint` + 가속기 여유 메모리 → 상주/프리페치/축출 결정.

### 4.4 Interface Repository

**책임**: expert 인터페이스·능력의 런타임 인트로스펙션. expert가 `describe()`로 계약 노출 → 사전 컴파일된 스텁 없이 능력 발견. 버전 태그로 롤링 업데이트 지원. MCP 얼굴 계층은 이 저장소를 계약의 단일 출처로 사용.

### 4.5 Portable Interceptors

**책임**: 모든 컨트롤 플레인 호출 경로에 횡단 관심사 균일 삽입.

- 삽입 지점: `send_request` / `receive_request` / `send_reply` / `send_exception`.
- 표준 스택(권장 순서): (1) 인증·인가 → (2) 쿼터·레이트 리밋 → (3) 안전 필터 → (4) 텔레메트리(지연·토큰·비용 계측 → Trading Service QoS 피드백) → (5) 감사 로그.
- 게이팅 정책을 인터셉터로 구현하면 라우팅 로직과 정책을 분리 가능.

### 4.6 Naming Service

**책임**: 이름 기반 정적 바인딩(고정 라우트, 디버깅, A/B 핀). 계층적 네임스페이스: `experts/math/theorem-prover`.

---

## 5. Expert 적재 상태 머신 (POA 구동)

```
                 프리페치 신호(route_freq↑)
   ┌──────────┐  ────────────────────────▶  ┌──────────────┐
   │ OFFLOADED │                              │ PREFETCHING  │
   │ (스토리지)│  ◀────────────────────────  │ (적재 중)    │
   └──────────┘        축출 완료              └──────┬───────┘
        ▲                                            │ 적재 완료
        │ 축출(메모리 압력 &&                         ▼
        │  route_freq↓ && !inflight)          ┌──────────────┐
        │                                     │  RESIDENT    │
        └─────────────────────────────────── │  (가속기 상주)│
                                              └──────┬───────┘
                                                     │ 활성 호출
                                                     ▼
                                              ┌──────────────┐
                                              │   ACTIVE     │◀─ 데이터 플레인
                                              │ (matmul 수행)│   (융합 커널)
                                              └──────────────┘
```

- **OFFLOADED → PREFETCHING**: Trading Service가 최근 라우팅 통계로 곧 필요할 expert를 예측, POA에 프리페치 명령.
- **PREFETCHING → RESIDENT**: ServantActivator가 가중치 적재 완료, IOR 활성화.
- **RESIDENT → ACTIVE**: 데이터 플레인 커널이 상주 expert로 matmul 수행(CORBA 미개입).
- **RESIDENT → OFFLOADED**: 메모리 압력 && `route_freq` 하락 && inflight 호출 없음일 때만 축출. PERSISTENT는 상태 보존.
- **전이 주기**: 배치·통계 단위(ms+). **절대 토큰 단위로 전이하지 않는다.**

---

## 6. 적재 정책 (Trading Service)

목표: 한정된 가속기 메모리 안에서 hit rate(상주 expert가 라우팅될 확률)를 최대화.

| 정책 요소 | 규칙 |
|---|---|
| **상주 우선순위** | `score = route_freq × affinity_weight ÷ mem_footprint`. 높을수록 상주 유지. |
| **프리페치** | 라우팅 히스토리의 마르코프/n-gram 예측 → 곧 쓸 expert를 유휴 대역폭으로 선적재. |
| **축출(Eviction)** | LFU 기반(`route_freq` 최저) + inflight 없음 조건. PERSISTENT는 상태 스왑 후 축출. |
| **핀(Pin)** | 시스템·핫 expert는 Naming Service로 상주 고정, 축출 대상 제외. |
| **메모리 워터마크** | 여유 메모리 < low-watermark → 축출 트리거, > high-watermark → 프리페치 허용. |
| **피드백 루프** | 인터셉터 텔레메트리 → `route_freq`·`load` 갱신 → 다음 적재 결정에 반영. |

---

## 7. IDL 정의

```idl
module moe {

  // ---- 데이터 타입 ----
  typedef sequence<octet> Tensor;      // 참조 전달 권장(zero-copy)
  typedef string          CapabilityId;

  enum Residency { OFFLOADED, PREFETCHING, RESIDENT, ACTIVE };

  struct Activation { Tensor data; string dtype; string shape; };
  struct Context    { string request_id; string trace_id; unsigned long step; };

  struct Capability {
    CapabilityId id;
    float        cost;
    float        latency_p99_ms;
    float        load;              // 0.0 ~ 1.0
    Residency    residency;
    unsigned long long mem_footprint;
    float        route_freq;        // 최근 라우팅 빈도
    string       placement_node;
    string       contract_version;
  };

  struct GateSignal  { Tensor affinity; unsigned short top_k; };
  struct Constraints { CapabilityId required; float max_latency_ms; float max_cost; };

  // ---- Expert 객체 ----
  interface Expert {
    Capability describe();                       // Interface Repo / Trading 등록용
    Activation process(in Activation x, in Context ctx);  // 상주 시 데이터 플레인 진입점
    Expert     delegate(in Capability need);     // pass-by-reference 조합적 라우팅
  };
  typedef sequence<Expert> ExpertSeq;

  // ---- Router (컨트롤 플레인 gate) ----
  interface Router {
    ExpertSeq  select(in GateSignal g, in Constraints qos);
    Activation dispatch(in Activation x, in Context ctx);
  };

  // ---- 적재·등록 (POA + Trading Service) ----
  interface ExpertRegistry {
    void register(in Expert e, in Capability cap);
    void deregister(in Expert e);
    void heartbeat(in Expert e, in Capability updated_cap);  // QoS/residency 갱신
  };

  interface ExpertLoader {                       // POA ServantActivator 프론트
    void  prefetch(in CapabilityId id);          // OFFLOADED → PREFETCHING (oneway 권장)
    void  evict(in CapabilityId id);             // RESIDENT → OFFLOADED
    void  pin(in CapabilityId id);               // 축출 제외
    Residency status(in CapabilityId id);
  };
};
```

---

## 8. 호출·적재 흐름

```
Client/LLM (MCP 얼굴)
   │ 1. 의미론적 요청
   ▼
Router.dispatch(x, ctx)
   │ 2. gate 신경망 → GateSignal            [데이터 플레인 / 텐서 math]
   │ 3. Router.select(g, qos)
   ▼
Trading Service (컨트롤 플레인)
   │ 4. 속성 질의 → top-k Expert 선택
   │ 4a. 선택 expert가 OFFLOADED면 ExpertLoader.prefetch → 적재 대기
   │ 4b. 라우팅 통계로 후속 expert 프리페치(유휴 대역폭)
   │ 5. 상주 Expert IOR 반환
   ▼
[Portable Interceptors: 인증 → 쿼터 → 안전 → 텔레메트리]
   │ 6. Expert.process(x, ctx) — 상주 expert에서 데이터 플레인 커널 실행
   │    (텐서는 IOR 참조 + RDMA/공유메모리 zero-copy)
   │ 7. 필요 시 Expert.delegate(need) → 다른 expert IOR (promise pipelining)
   ▼
Router
   │ 8. 결과 결합(가중 합/투표) → Activation
   ▼
Client/LLM
   │ 9. 텔레메트리 → route_freq/load 갱신 → 다음 적재 결정에 피드백
```

---

## 9. 기업 모델 구성 (Enterprise Model Composition)

기업의 "모델"은 단일 모놀리스가 아니라 **파운데이션 base + 기업 전용 expert(파인튜닝/LoRA 어댑터/사내 지식 expert) + 거버넌스 정책 + 검색(RAG) 컴포넌트**의 **구성물(composition)** 이다. 이 명세는 그 구성 자체를 1급 CORBA object로 정의하여, 기업별 조립·격리·배포·통제가 컨트롤 플레인에서 객체로 다뤄지도록 한다.

### 9.1 설계 원칙

- **구성은 객체다**: 기업이 조립한 모델 전체를 `ComposedModel` 객체로 표현한다. base 참조 + enterprise expert 오버레이 + 정책 바인딩 + 검색 소스의 결합체.
- **base는 공유, 델타는 소유**: 파운데이션 expert는 전 테넌트가 공유 상주하고, 기업은 그 위에 LoRA/어댑터 **델타만 소유**한다(pass-by-reference로 base 참조). 메모리·비용 효율의 핵심.
- **기업 경계 = CORBA 도메인**: 각 기업은 독립 Naming/Trading 스코프를 갖는 ORB 도메인. 기업 전용 expert는 해당 도메인에만 등록되어 교차 테넌트로 노출되지 않는다.
- **거버넌스는 1급 객체**: 데이터 레지던시·컴플라이언스·접근제어·감사를 `PolicyDomain` 객체로 두고 Portable Interceptor + CORBA Security Service로 강제한다.

### 9.2 컴포넌트 매핑

| 기업 구성 개념 | CORBA 컴포넌트 / 서비스 | 역할 |
|---|---|---|
| 조립된 기업 모델 | **`ComposedModel` 객체** | base + enterprise expert + 정책 + 검색의 구성물, 추론 진입점. |
| 기업 전용 expert | **`EnterpriseExpert : Expert`** | 파인튜닝/LoRA/도메인 expert. 기업 도메인 Trading에만 등록(격리). |
| base 위 어댑터 오버레이 | **AdapterOverlay (pass-by-ref)** | 공유 base expert 참조 + 기업 소유 델타. |
| 기업 경계·멀티테넌시 | **CORBA Domains + Federation** | 테넌트별 독립 Naming/Trading 스코프, 선택적 브리징. |
| 모델 인스턴스화·배포 | **`ModelFactory` (CosLifeCycle GenericFactory)** | 테넌트별 ComposedModel 생성/복제/버전/롤아웃. |
| 거버넌스·컴플라이언스 | **`PolicyDomain` + Security Service** | 인가·데이터 레지던시·감사. |
| 테넌트별 설정 | **CosProperty Service** | per-tenant 구성 프로퍼티. |
| 가드레일·안전 expert | **Interceptor 필수 경로** | 안전·컴플라이언스 expert를 호출 경로에 강제 삽입. |

### 9.3 구성 패턴

1. **Base + Private Overlay** — 공유 파운데이션 expert(상주) + 기업 LoRA 오버레이(테넌트 전용 적재). 데이터 플레인에서 base matmul + adapter delta 합성. base는 한 번만 적재해 전 테넌트가 공유.
2. **Mixture of Enterprise Experts** — 기업이 자체 expert 집합을 자사 Trading 도메인에 등록, 라우터가 base expert와 함께 혼합 선택.
3. **Guardrail-in-path** — 안전·컴플라이언스·PII 필터 expert를 Portable Interceptor 경로에 필수로 끼워 모든 추론이 통과하도록 강제.

### 9.4 멀티테넌시·격리

- **도메인 격리**: 각 기업 = 독립 Trading/Naming 도메인. IOR에 `tenant_id` 태그, 인터셉터가 교차 테넌트 접근을 차단.
- **적재 격리**: 테넌트 전용 expert는 해당 테넌트 요청에만 적재/상주. base expert만 공유 풀.
- **데이터 레지던시**: `PolicyDomain.check_residency`가 `placement_node`를 제약 → 특정 리전에 상주 가능한 expert만 적재(§6 적재 정책과 결합).
- **감사·재현성**: 인터셉터 텔레메트리가 `PolicyDomain.audit`로 흘러 테넌트별 호출 추적을 남긴다.

### 9.5 IDL 정의 (기업 구성 확장)

```idl
module moe { module enterprise {

  struct Manifest {
    string                        tenant_id;
    string                        base_model;        // 공유 파운데이션 참조
    sequence< ::moe::CapabilityId> experts;          // 기업 전용 expert 목록
    string                        policy_domain;
    string                        version;
    string                        residency_region;
  };

  // 기업 전용 expert — base는 참조, 델타만 소유
  interface EnterpriseExpert : ::moe::Expert {
    string        tenant_id();
    ::moe::Expert base();            // pass-by-reference: 공유 base expert
    ::moe::Tensor adapter_delta();   // 기업 소유 LoRA/어댑터 델타
  };

  // 거버넌스 1급 객체
  interface PolicyDomain {
    boolean authorize(in string principal, in ::moe::CapabilityId target);
    boolean check_residency(in string placement_node);
    void    audit(in ::moe::Context ctx, in string event);
  };

  // 조립된 기업 모델
  interface ComposedModel {
    Manifest         manifest();
    ::moe::Activation infer(in ::moe::Activation x, in ::moe::Context ctx);
    void             bind_expert(in EnterpriseExpert e);
    void             set_policy(in PolicyDomain p);
  };

  // 인스턴스화·배포 (CosLifeCycle GenericFactory)
  interface ModelFactory {
    ComposedModel create(in Manifest m);
    ComposedModel clone(in ComposedModel src, in string new_version);
    void          deploy(in ComposedModel m);
    void          retire(in ComposedModel m);
  };

}; };
```

### 9.6 기업 추론 흐름

```
Enterprise Client (테넌트 인증됨)
   │ 1. ComposedModel.infer(x, ctx)
   ▼
[Interceptor: PolicyDomain.authorize → residency 검사 → 가드레일 expert]
   │ 2. 인가·컴플라이언스 통과
   ▼
Router.select  (테넌트 Trading 도메인 스코프)
   │ 3. 후보 = 공유 base expert + 기업 EnterpriseExpert
   │ 4. base는 공유 상주, LoRA 오버레이는 테넌트 전용 적재(§5 상태머신)
   ▼
Data Plane
   │ 5. base matmul + adapter_delta 합성  [CORBA 밖 융합 커널]
   ▼
Router → 결합 → Activation
   │ 6. PolicyDomain.audit(ctx) — 테넌트별 감사 로그
   ▼
Enterprise Client
```

### 9.7 배포·거버넌스 매핑

| 기업 요구 | 구현 |
|---|---|
| 모델 버전·롤아웃 | `ModelFactory.clone(src, new_version)` + `deploy` / `retire`. Interface Repo의 컨트랙트 버전과 연동. |
| 테넌트 격리 | 도메인별 Trading 스코프 + IOR `tenant_id` + 인터셉터 차단. |
| 데이터 레지던시 | `PolicyDomain.check_residency` → `placement_node` 제약 → 리전 상주 expert만 적재. |
| 접근 제어 | CORBA Security Service + `PolicyDomain.authorize`(principal↔capability). |
| 감사·컴플라이언스 | 인터셉터 → `PolicyDomain.audit`, 재현 가능한 호출 추적. |
| 사내 IP 보호 | base 공유·델타 소유 구조로 기업 가중치(adapter_delta)만 테넌트 경계 내 보관. |

---

## 10. 언어별 구성 (IDL-to-Language Mapping)

CORBA Object의 규격상 핵심은 **IDL이라는 단일 언어 중립 계약**에서 각 언어별 산출물을 자동 생성하는 것이다. 이 절은 OMG IDL-to-language mapping 규격을 검토하여, 앞서 정의한 인터페이스(`Expert`, `Router`, `ComposedModel` 등)를 언어별로 구성하는 형태를 정의한다.

### 10.1 규격 검토 — CORBA Object 구성 원리

하나의 IDL 인터페이스에서 각 언어 매핑은 항상 **4대 산출물**을 만든다.

1. **클라이언트 Stub** — 원격 객체 호출을 로컬 호출처럼 보이게 하는 프록시.
2. **서버 Skeleton / Servant base** — POA에 등록되어 실제 구현이 상속하는 기반 클래스.
3. **Object Reference 타입** — IOR의 언어별 로컬 표현(핸들).
4. **Helper / Holder** — narrowing(범용 Object → 구체 인터페이스 안전 캐스트), 마샬링, `out`/`inout` 파라미터 전달 보조.

이 구조 덕분에 **언어 간 상호운용**이 보장된다 — IIOP/IOR(또는 substrate의 공유 스키마)만 지키면 Python 클라이언트가 C++ servant를, Go 라우터가 Rust expert를 호출할 수 있다. MoE 스택에서 이것이 핵심 이점이다: 컨트롤 플레인은 Python/Go로, 데이터 플레인 인접 expert는 C++/Rust로 구현해도 같은 `Expert` 계약으로 묶인다.

### 10.2 언어별 매핑 표 (IDL 구성요소 → 각 언어)

| IDL 구성요소 | C++ (OMG C++) | Java (org.omg) | Python (omniORBpy) | C (OMG C) |
|---|---|---|---|---|
| `interface Expert` | 클래스 `Expert` + `POA_moe::Expert` | `Expert` I/F + `ExpertPOA` | `moe.Expert` + `moe__POA.Expert` | 불투명 타입 `moe_Expert` |
| object reference | `Expert_ptr` / `Expert_var` | `Expert` (Object) | objref | `moe_Expert` |
| operation `process()` | 멤버 함수 | 메서드 | 메서드 | `moe_Expert_process(obj,…,&ev)` |
| `in` 파라미터 | 값/`const&`/`_ptr` | 값/객체 | 인자 | 값/포인터 |
| `out`·`inout` | `_out`/`&` | `Holder` | 반환 튜플 | 포인터 |
| servant 기반 | `POA_moe::Expert` 상속 | `ExpertPOA` 상속 | `moe__POA.Expert` 상속 | vtable + skeleton |
| narrowing | `Expert::_narrow(obj)` | `ExpertHelper.narrow(obj)` | `obj._narrow(moe.Expert)` | `moe_Expert__narrow` |
| 예외 | `throw`/`CORBA::Exception` | `throws` | `raise` | `CORBA_Environment*` |

### 10.3 언어별 `Expert` 객체 구성 예시

**C++ (OMG C++ 매핑 — 데이터 플레인 인접·고성능)**
```cpp
// 생성물: ExpertC.hh(stub), ExpertS.hh(skeleton)
// --- 클라이언트 ---
moe::Expert_var exp = moe::Expert::_narrow(obj);
moe::Activation_var out = exp->process(x, ctx);

// --- 서버 (POA servant) ---
class ExpertImpl : public POA_moe::Expert {
public:
  moe::Activation* process(const moe::Activation& x,
                           const moe::Context& ctx) override;
  moe::Capability* describe() override;
  moe::Expert_ptr  delegate(const moe::Capability& need) override;
};
```

**Java (OMG Java 매핑 — 기업 서비스·거버넌스)**
```java
// 생성물: Expert, ExpertOperations, ExpertHelper, ExpertHolder, ExpertPOA, _ExpertStub
// --- 클라이언트 ---
Expert exp = ExpertHelper.narrow(obj);
Activation out = exp.process(x, ctx);

// --- 서버 ---
public class ExpertImpl extends ExpertPOA {
  public Activation process(Activation x, Context ctx) { /* ... */ }
  public Capability describe() { /* ... */ }
  public Expert delegate(Capability need) { /* ... */ }
}
```

**Python (omniORBpy 매핑 — 컨트롤 플레인 오케스트레이션)**
```python
import moe, moe__POA
# --- 클라이언트 ---
exp = obj._narrow(moe.Expert)
out = exp.process(x, ctx)

# --- 서버 (servant) ---
class ExpertImpl(moe__POA.Expert):
    def process(self, x, ctx): ...
    def describe(self): ...
    def delegate(self, need): ...
```

**C (OMG C 매핑 — 커널·RDMA 글루)**
```c
CORBA_Environment ev;
moe_Activation* out = moe_Expert_process(exp, &x, &ctx, &ev);
/* servant: POA_moe_Expert vtable에 skeleton 함수 등록 */
```

**Rust (substrate = Cap'n Proto — 안전한 고성능, IOR/delegate 최적)**
```rust
// capnp: interface Expert -> expert::Server(트레이트) + expert::Client
#[async_trait]
impl expert::Server for ExpertImpl {
    async fn process(&mut self, p: ProcessParams, mut r: ProcessResults)
        -> Result<(), Error> { /* ... */ }
}
// 클라이언트: let out = client.process_request().send().promise.await?;
// delegate 반환은 capability(원격 객체 참조) → promise pipelining
```

**Go (substrate = gRPC — 클라우드 네이티브 컨트롤 플레인)**
```go
// proto: service Expert -> ExpertServer 인터페이스 + ExpertClient
type expertImpl struct{ pb.UnimplementedExpertServer }
func (s *expertImpl) Process(ctx context.Context, x *pb.Activation)
    (*pb.Activation, error) { /* ... */ }
// 참고: gRPC는 pass-by-reference 객체 모델이 없어 IOR/delegate는 앱 레벨 구현
```

### 10.4 플레인·역할별 언어 배치 권고

| 계층 / 역할 | 권장 언어 | 근거 |
|---|---|---|
| 데이터 플레인 인접 expert (커널 바인딩) | **C++ / C / Rust** | 저지연, zero-copy, RDMA·CUDA 글루. |
| 컨트롤 플레인 라우터·적재기 | **Python / Go** | ML 생태계(Python), 동시성·운영(Go). |
| Trading / Interface Repo / POA 서비스 | **Go / Java** | 성숙한 서비스 런타임·거버넌스. |
| 기업 구성(§9) ComposedModel·PolicyDomain | **Java / Go** | Security·LifeCycle 서비스, 엔터프라이즈 통합. |
| MCP 얼굴 계층 | **Python / TypeScript** | LLM·툴 생태계 근접. |

### 10.5 substrate별 생성 방식

| substrate | 생성 도구 | 객체 참조 | pass-by-reference(delegate) |
|---|---|---|---|
| Classical CORBA | `idl` 컴파일러(언어별) | IOR(IIOP) | 규격 내장 |
| **Cap'n Proto** (권장) | `capnp compile` | capability | **네이티브(promise pipelining)** |
| gRPC/protobuf | `protoc` | (없음) | 앱 레벨 구현 필요 |

> **원칙**: 어떤 언어·substrate를 쓰든 **IDL/스키마가 계약의 단일 출처(single source)** 다. 언어별 stub/servant는 이 계약에서 생성만 하고, 손으로 계약을 재정의하지 않는다. Interface Repository(§4.4)가 이 계약의 런타임 권위 소스가 된다.

---

## 11. 상태·성능·트레이드오프

| 항목 | 지침 |
|---|---|
| **플레인 분리 준수** | 프로파일링으로 데이터 플레인에 ORB가 침투하지 않는지 검증. Trading Service 선택 지연이 expert 계산 대비 <5%. |
| **적재 주기** | 토큰 단위 금지. 배치·통계 주기로만 전이. 프리페치로 적재 지연을 은닉. |
| **텐서 이동** | 참조 전달 + zero-copy. IOR에 데이터 인라인 금지. |
| **상태 관리** | 무상태(수평 확장)·상태 유지(세션/적응) expert를 POA 정책으로 명시 분리. |
| **장애 처리** | ORB 재시도 + heartbeat 기반 자동 격리. PERSISTENT는 객체 키로 재적재. |
| **위치 투명성 남용 주의** | 로컬 GPU와 원격 expert 지연 차이는 수 오더. `latency_p99`·`placement_node`로 반영. |
| **보안** | IOR 위조 방지(서명된 객체 키) + 인터셉터 인가 필수. |

---

## 12. 구현 로드맵 및 권고

1. **substrate 결정**
   레거시 IIOP(TAO·omniORB·JacORB)를 문자 그대로 되살리는 것은 툴체인 부담이 크다. 프로토콜 연구·레거시 상호운용 목적이 아니라면:
   - **Cap'n Proto** — capability 기반 RPC, 원격 객체 pass-by-reference, promise pipelining. IOR·delegate 의미론에 최적.
   - **gRPC/protobuf** — 생태계 성숙. 단 pass-by-reference는 애플리케이션 레벨 구현 필요.
   - **권장 조합**: "IIOP 정신 + Cap'n Proto 몸통 + MCP 얼굴".

2. **최소 코어(MVP) 구축 순서**
   1. ORB Core + IOR + `Expert.process` (단일 노드, 상주 expert만)
   2. POA ServantActivator 지연 적재 + `ExpertLoader` (프리페치/축출)
   3. Trading Service 속성 질의 + 적재 정책(§6)
   4. 적재 상태 머신(§5) + 메모리 워터마크
   5. Portable Interceptors(인증·텔레메트리) → 적재 피드백 루프
   6. `Expert.delegate` pass-by-reference + Interface Repository + MCP 얼굴 바인딩
   7. 기업 구성 계층(§9): `ComposedModel`·`EnterpriseExpert`·`PolicyDomain`·`ModelFactory` + 도메인 격리·감사

3. **검증 기준**
   - 데이터 플레인 핫 루프에 ORB 미침투(프로파일링).
   - expert hit rate(상주 상태에서 라우팅될 확률) 목표치 달성.
   - 프리페치가 적재 지연을 은닉하여 p99 지연 열화 없음.

---

## 13. 용어집

- **컨트롤 플레인 / 데이터 플레인** — 적재·배치·라우팅 결정 계층 / 실제 forward-pass matmul 계층.
- **ORB** — 객체 간 method invocation을 중개하는 런타임 커널.
- **IIOP / GIOP** — CORBA 바이너리 와이어 프로토콜.
- **IOR** — 원격/적재된 객체에 대한 이동 가능·안정적 핸들.
- **CDR** — CORBA 바이너리 마샬링 포맷.
- **POA / ServantActivator** — 객체 생명주기 관리자 / 지연 적재 트리거(본 설계의 expert 적재기).
- **Trading Service** — 능력·QoS 속성으로 서비스를 찾고 적재를 결정하는 서비스.
- **Interface Repository** — 인터페이스 계약 런타임 인트로스펙션 저장소.
- **Portable Interceptor** — 호출 경로 횡단 관심사 훅.
- **Residency** — expert의 적재 상태(OFFLOADED/PREFETCHING/RESIDENT/ACTIVE).
- **MoE** — 게이트가 N개 중 k개 expert만 희소 활성화하는 신경망 구조.
- **MCP** — LLM·툴 간 의미론적 발견·호출 프로토콜(본 설계의 얼굴 계층).
- **ComposedModel** — 기업이 조립한 모델 전체(base + enterprise expert + 정책 + 검색)를 나타내는 1급 CORBA 객체.
- **EnterpriseExpert** — Expert의 서브타입. 공유 base를 참조하고 기업 소유 델타(LoRA)만 갖는 테넌트 전용 expert.
- **PolicyDomain** — 인가·데이터 레지던시·감사를 담당하는 거버넌스 1급 객체.
- **ModelFactory** — 테넌트별 ComposedModel을 생성·복제·배포하는 CosLifeCycle GenericFactory.
- **CORBA Domain / Federation** — 기업 경계를 나타내는 독립 Naming/Trading 스코프와 그 간 브리징.

---

*이 문서는 구현 기준선이다. 데이터 플레인(융합 커널)은 CORBA 범위 밖이며, 컨트롤 플레인(적재·배치·발견·생명주기)만 본 명세로 구현한다. 상세 정책은 팀의 substrate 결정(§10)에 따라 구체화한다.*
