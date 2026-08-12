# Phase 0 — Feasibility findings / 타당성 검증 결과

> 2026-08-12 · Run on macOS 26.6 (arm64), Rust 1.95, Python 3.14, omniORB 4.3.4
> Reproduce with `./spikes/run_checks.sh`

**Verdict: GO.** All four assumptions were measured. Two were confirmed as
planned, one was confirmed *negative* exactly as risk R1 predicted (the fallback
works), and one confirmed risk R7 is real (with a working mitigation). Nothing
found here invalidates the architecture.

**판정: GO.** 네 가정을 모두 측정했다. 둘은 계획대로 확인됐고, 하나는 리스크 R1이
예측한 대로 *부정* 확인됐으며(폴백은 동작), 하나는 리스크 R7이 실재함을 확인했다
(완화책 동작 확인). 아키텍처를 무효화하는 발견은 없다.

| # | Assumption / 가정 | Result | 결과 |
|---|---|---|---|
| **A** | A from-scratch GIOP implementation can interoperate with a stock ORB | ✅ **PASS** | 자체 GIOP 구현이 순정 ORB와 상호운용됨 |
| **B** | An LLM can write IDL that compiles | ✅ **PASS** — 65% first pass, 100% after one self-repair round | 1차 65%, 자가수정 1라운드 후 100% |
| **C** | IDL 4 `@annotation` survives deployed toolchains | ❌ **REJECTED** — fallback confirmed working | 배포 컴파일러가 거부. 폴백 유효 |
| **D** | IOR addressing works under NAT/containers | ⚠️ **HAZARD CONFIRMED** — mitigation available | 기본 publish는 배포 안전하지 않음. 재작성으로 해결 |

---

## Assumption A — GIOP interoperability / GIOP 상호운용

**The gate.** If a minimal from-scratch ORB could not interoperate, the
MIT-only licensing constraint would have to be renegotiated (PLAN §7).

**관문.** 최소 자체 ORB가 상호운용되지 않으면 MIT 전용 제약을 재협상해야 했다.

**Method.** `orbweaver-cdr` and `orbweaver-giop` were written against the
published OMG specification only — no existing ORB is linked, vendored or
consulted at build time. A stock omniORB 4.3.4 Python server acts as the peer.
The client hand-encodes GIOP 1.2 `Request` messages and decodes the replies.

**방법.** `orbweaver-cdr`과 `orbweaver-giop`은 공개 OMG 명세만 보고 작성했다.
기존 ORB를 링크·벤더링하지 않는다. 순정 omniORB 4.3.4 Python 서버를 피어로 두고,
클라이언트가 GIOP 1.2 `Request`를 직접 인코딩하고 응답을 디코딩한다.

**Result: 12/12 asserted cases pass, in both byte orders, across 5 cold starts.**

> **Correction (2026-08-12, from the Phase 1 spec audit).** This section
> originally reported "14/14". That was an over-claim. The Korean round-trip
> case printed a result in all three branches and never incremented the failure
> counter, so it was **structurally incapable of failing** — 12 of the 14
> printed lines were assertions and 2 were informational probes. The harness now
> counts and labels them separately. The finding is worse than a miscount: the
> probe passed only because omniORB's default is byte-transparent, and with no
> `CodeSets` service context the specified transmission codeset is ISO-8859-1
> while we send UTF-8, so it was never evidence of correctness. See §Assumption A
> note on codesets below.
>
> **정정.** 원래 "14/14"로 보고했으나 과대 진술이었다. 한국어 케이스는 세 분기
> 모두 실패를 집계하지 않아 **구조적으로 실패가 불가능**했다. 실제 단언은 12건,
> 정보성 프로브가 2건이다. 더 나쁜 점은, 그 프로브가 통과한 이유가 omniORB의
> 바이트 투명 기본값 때문이지 우리 구현이 옳아서가 아니라는 것이다.

| Case | Exercises |
|---|---|
| `ping() -> 42` | Nullary call, scalar reply |
| `add(1000000, 337)` | Two 4-aligned integer arguments |
| `echo_string(...)` | String length prefix that counts the NUL |
| `scale(1.5, 4.0)` | 8-byte alignment of the GIOP 1.2 request body |
| `echo_ragged(...)` | Struct padding: `octet, long, short, double, octet` |
| Korean text | `함정 전투체계` round-trip |
| `no_such_operation()` | `BAD_OPERATION` system exception decoding |

Both byte orders matter: an encoder that only works native-endian passes every
local test and fails in the field.

양쪽 바이트오더를 모두 시험한다. 네이티브 엔디안에서만 동작하는 인코더는 로컬
테스트를 전부 통과하고 현장에서 실패하기 때문이다.

### What the wire actually looks like / 실제 와이어

Our request, and omniORB's reply, verified byte for byte:

```
REQUEST (64 bytes) — our encoder, little-endian
  0000  47 49 4f 50 01 02 01 00  34 00 00 00 01 00 00 00   GIOP....4.......
  0010  03 00 00 00 00 00 00 00  0e 00 00 00 fe 5b bf 7b   .............[.{
  0020  6a 00 00 0d c5 00 00 00  00 00 00 00 05 00 00 00   j...............
  0030  70 69 6e 67 00 00 00 00  00 00 00 00 00 00 00 00   ping............

RESPONSE (28 bytes) — stock omniORB
  0000  47 49 4f 50 01 02 01 01  10 00 00 00 01 00 00 00   GIOP............
  0010  00 00 00 00 00 00 00 00  2a 00 00 00               ........*...
                                 ^^^^^^^^^^^ 42
```

**Incidental finding.** omniORB opens with a `LocateRequest` (message type 3)
before its first `Request`. Ours goes straight to `Request` and the peer accepts
it, so `LocateRequest` is optional for a client — but Phase 1 must *serve* it,
since peers will send it to us.

**부수 발견.** omniORB는 첫 `Request` 앞에 `LocateRequest`(타입 3)를 보낸다.
우리는 바로 `Request`를 보내도 수용된다. 즉 클라이언트로서는 선택이지만,
Phase 1에서 서버로서는 반드시 응답해야 한다.

**Incidental finding.** omniORB does not zero its CDR padding bytes. Padding
content is undefined by the specification, so any test that compares whole
messages byte-for-byte against a reference ORB will produce false failures.
Compare decoded values, not raw buffers.

**부수 발견.** omniORB는 CDR 패딩을 0으로 채우지 않는다. 패딩 내용은 명세상
미정의이므로, 참조 ORB와 메시지 전체를 바이트 비교하는 테스트는 거짓 실패를
낸다. 원시 버퍼가 아니라 디코딩된 값을 비교해야 한다.

---

## Assumption B — LLM-generated IDL / LLM의 IDL 생성

**Method.** 20 requirements were written in Korean and frozen
(`corpus/requirements/README.md`) *before* any IDL was generated. Generation was
a single pass with no compiler feedback. Every file then went through the
omniidl oracle.

**방법.** 한국어 요구사항 20건을 먼저 고정한 뒤 IDL을 생성했다. 컴파일러 피드백
없이 1회 생성하고, 전체를 omniidl 오라클에 통과시켰다.

| Measurement | Result | Target |
|---|---|---|
| First-pass compile rate | **13/20 (65%)** | ≥ 60% ✅ |
| After one self-repair round | **20/20 (100%)** | ≥ 95% within three rounds ✅ |

### The finding that matters / 핵심 발견

**All seven failures had the same root cause.** IDL identifier clashes are
case-insensitive, and a member or parameter may not share a name with a type or
enclosing scope:

**실패 7건 전부가 동일한 원인이었다.** IDL 식별자 충돌은 대소문자를 구분하지
않으며, 멤버·파라미터는 타입이나 상위 스코프와 이름을 공유할 수 없다:

```idl
struct Track { Position position; };        // clash: position ~ Position
union  Value  { ... };  struct E { Value value; };  // clash: value ~ Value
module inventory { interface Inventory {...}; };    // clash with enclosing scope
struct Version { unsigned long version; };          // clash with enclosing scope
```

This is natural, idiomatic naming in every other language, which is precisely
why a model produces it. It is also mechanically detectable and mechanically
fixable.

이것은 다른 모든 언어에서 자연스러운 관용적 명명이며, 그래서 모델이 그렇게 쓴다.
동시에 기계적으로 탐지·수정 가능하다.

**Phase 1 consequences:**

1. `orbweaver-idl` ships a lint rule for case-insensitive identifier collision,
   raised *before* the oracle runs, with a rename suggestion in the diagnostic.
2. The synthesis prompt carries this constraint explicitly.
3. Together these should move the first-pass rate from 65% toward the ≥85%
   target in PLAN §11 without touching the self-repair loop.

**Methodological limitation, stated plainly.** In this run the generator and
the evaluator were the same model, so 65% is indicative, not a clean benchmark
figure. PLAN §8 requires a frozen benchmark with a hold-out subset and an
independent harness before this number gates anything.

**방법론적 한계.** 이번 실행에서는 생성자와 평가자가 동일한 모델이므로 65%는
참고치이지 정식 벤치마크 수치가 아니다. PLAN §8이 요구하는 동결 벤치마크와
홀드아웃, 독립 하네스가 갖춰지기 전에는 이 수치로 무엇도 게이팅하지 않는다.

---

## Assumption C — `@annotation` acceptance / 어노테이션 수용성

**Rejected, exactly as risk R1 predicted.** omniidl refuses IDL 4 annotations
in both forms:

**리스크 R1이 예측한 대로 부정 확인.** omniidl은 두 형태 모두 거부한다:

| Form | Result |
|---|---|
| `@annotation ai_desc { string text; };` declaration + application | ❌ `Syntax error in definition` |
| Application only, no declaration | ❌ `Syntax error in definition` |
| **Structured comments** (`//@ ai_desc: ...`) | ✅ compiles cleanly |

**Decision: SIDL v1 uses structured comments, not IDL 4 `@annotation`.**

The fallback is viable precisely because Orbweaver owns its parser — the plan's
stated reason for the in-house IDL front end (PLAN §3.1). One nuance: omniidl
*discards* the comments, so annotations survive only in `orbweaver-idl`. That is
correct and intended — omniidl is a conformance oracle for base IDL, nothing
more.

폴백이 유효한 이유는 정확히 Orbweaver가 파서를 소유하기 때문이다. 한 가지 유의점:
omniidl은 주석을 버리므로 어노테이션은 `orbweaver-idl`에만 남는다. 이는 의도된
설계다 — omniidl은 기본 IDL의 적합성 채점기일 뿐이다.

The IDL 4 syntax is not abandoned. `orbweaver-idl` will accept both spellings
and emit whichever the target toolchain tolerates, so the standard form becomes
available the moment a deployment's compiler supports it.

IDL 4 문법을 버리는 것은 아니다. `orbweaver-idl`은 두 표기를 모두 수용하고 대상
툴체인이 견디는 쪽으로 내보낸다.

---

## Assumption D — IOR addressing / IOR 주소

**The hazard is real, and the mitigation works.**

| Case | Published address | Outcome |
|---|---|---|
| Default | `172.30.1.45:54xxx` — the host's LAN address | Reachable here; **would be a pod IP in Kubernetes** |
| Simulated container | `10.244.3.17:31000` | Client hangs dialing an address that does not exist on this network |
| `-ORBendPoint` + `-ORBendPointPublish` | `127.0.0.1:40404` | ✅ Reachable — bind wide, publish correctly |

An ORB publishes the address it *believes* it has. Behind NAT, in a container,
or behind a load balancer, that belief is wrong and every returned reference is
dead on arrival. Endpoint rewriting must be part of the standard deployment
template, not a troubleshooting step (PLAN R7).

ORB는 자신이 가졌다고 *믿는* 주소를 광고한다. NAT 뒤나 컨테이너 안, 로드밸런서
뒤에서는 그 믿음이 틀리고, 반환되는 모든 참조가 도착 즉시 죽는다. endpoint
재작성은 문제 해결 단계가 아니라 표준 배포 템플릿의 일부여야 한다.

**Client-side consequence.** `Connection::connect` needs a connect timeout —
without one, an unroutable published address hangs until the OS TCP timeout
(often 75s). The spike's diagnostic tool reproduced exactly this.

**클라이언트 측 결과.** `Connection::connect`에는 연결 타임아웃이 필요하다.
없으면 라우팅 불가 주소에서 OS TCP 타임아웃(흔히 75초)까지 멈춘다.

---

## Corpus / 코퍼스

| Set | Count | Purpose |
|---|---|---|
| `corpus/golden/` | 21 | Type-system and CDR coverage. **21/21 compile.** |
| `corpus/negative/` | 8 | Diagnostic quality material. **8/8 correctly rejected.** |
| `corpus/requirements/` | 20 | Assumption B benchmark, frozen before generation |
| `corpus/annotations/` | 3 | Assumption C probes |

Two golden cases were themselves wrong on first write, and the oracle caught
both — `nested`/`Nested` (the same case-insensitivity rule as assumption B) and
an unqualified `TypeCode`. The oracle earns its place.

골든 케이스 2건은 처음 작성이 틀렸고 오라클이 둘 다 잡았다 — `nested`/`Nested`
(가정 B와 동일한 규칙)와 한정되지 않은 `TypeCode`. 오라클은 값을 한다.

---

## Harness bugs worth recording / 기록할 만한 하네스 버그

Both produced *phantom* failures that looked like GIOP problems and were not.
Recording them because a Phase 1 CI harness will meet them again.

둘 다 GIOP 문제처럼 보였지만 아니었던 *유령* 실패다. Phase 1 CI 하네스가 다시
만날 것이므로 기록한다.

1. **A wait loop that does not wait.** `for i in $(seq 1 500); do [ -f f ] && break; done`
   completes in microseconds. It only appeared to work because `cargo run` had
   to compile first and accidentally covered the race; once the build was warm,
   the client dialed a server that was not yet accepting and reported read
   timeouts. **This was the cause of the initial assumption A failure** — the
   protocol was correct the whole time. Wait loops must sleep.

   **기다리지 않는 대기 루프.** 위 루프는 마이크로초 만에 끝난다. `cargo` 컴파일
   시간이 우연히 경쟁을 가려주다가, 빌드가 캐시되자 드러났다. **초기 가정 A 실패의
   원인이 이것이었다** — 프로토콜은 처음부터 정상이었다.

2. **`| grep -q` truncates its producer.** `grep -q` exits on first match and
   closes the pipe, delivering SIGPIPE to the upstream process. This produced a
   reproducible false failure in the assumption D check. Capture output to a
   variable, then match.

   **`| grep -q`는 생산자를 잘라낸다.** 첫 매치에서 종료하며 파이프를 닫아
   상류 프로세스에 SIGPIPE를 보낸다. 출력을 변수에 담은 뒤 매칭해야 한다.

3. **A harness that reports green when a fixture fails to start.** The first
   version incremented no failure counter when the server did not come up, so a
   completely unmeasured assumption rendered as a pass. An unmeasured assumption
   is a failure, never a pass.

   **픽스처 기동 실패를 green으로 보고하는 하네스.** 측정되지 않은 가정은 통과가
   아니라 실패다.

---

## Licensing boundary as exercised / 실제로 지켜진 라이선스 경계

omniORB 4.3.4 (LGPL libraries, GPL tools) was used two ways, both outside the
scope of any copyleft obligation on Orbweaver:

omniORB 4.3.4(LGPL 라이브러리, GPL 툴)는 두 가지로만 사용했고, 둘 다 Orbweaver에
카피레프트 의무를 발생시키지 않는다:

- **Wire peer.** A separate process, reached over TCP using the published GIOP
  specification. No linking, no code reuse.
- **Conformance oracle.** `omniidl` invoked as an external program to check IDL,
  its output read as text.

No omniORB code is imported, linked, vendored or redistributed. Everything under
`crates/` is original work written against the OMG specification and is MIT.

omniORB 코드는 import·링크·벤더링·재배포되지 않는다. `crates/` 아래는 전부 OMG
명세를 보고 작성한 원본이며 MIT다.

---

## What Phase 1 inherits / Phase 1이 물려받는 것

**Working code**

- `orbweaver-cdr` — CDR encode/decode, both endiannesses, alignment, encapsulations. 10 unit tests.
- `orbweaver-giop` — GIOP 1.2 `Request`/`Reply`, IOR parsing, synchronous invoker. 7 unit tests.
- `spikes/run_checks.sh` — the harness, green and reproducible across consecutive runs.

**Decisions settled**

1. SIDL v1 uses structured comments; IDL 4 `@annotation` is accepted by our
   parser but not emitted to legacy toolchains.
2. Case-insensitive identifier collision is the first lint rule and a synthesis
   prompt constraint.
3. `LocateRequest` must be served in Phase 1, though not sent.
4. Interop tests compare decoded values, never raw buffers.
5. `Connection` needs a connect timeout and a retry policy.
6. Endpoint rewriting belongs in the deployment template.

**Still unmeasured**

- Codeset negotiation. The Korean round-trip passed against omniORB's
  UTF-8-capable default, which is *not* evidence that EUC-KR peers work. PLAN
  §4.4 keeps this as Phase 1 scope and it remains the riskiest open item for the
  domestic market.
- GIOP 1.0/1.1 compatibility, fragmentation, TAO and JacORB in the matrix.

**아직 측정되지 않음** — 코드셋 협상이 가장 큰 미해결 항목이다. 한국어 왕복이
통과한 것은 omniORB의 UTF-8 기본값 덕분이며, EUC-KR 피어가 동작한다는 증거가
아니다.
