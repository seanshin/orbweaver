# D004 — Observability: tracing and OTLP, which dependency, if any

**STATUS: PROPOSED** — written 2026-08-13. Nothing is adopted before a human
approves. `PLAN.md` §6 names **OpenTelemetry via interceptors**, and
`docs/COMPONENTS.md` records two rows waiting on this document: *Observability
(OpenTelemetry)* — ❌, "interceptor seam unbuilt; needs D004" — and
`orbweaver-console` — ❌, "after OTel decision".
**상태: 제안** — 2026-08-13 작성. 사람이 승인하기 전에는 아무것도 채택되지 않는다.
`PLAN.md` §6이 "인터셉터 경유 OpenTelemetry"를 지명하며, `COMPONENTS.md`의 두 행
(관측, `orbweaver-console`)이 이 문서에 막혀 있다.

**Verified 2026-08-13** against shipped artifacts where feasible: crate
tarballs from `static.crates.io` (tracing 0.1.44, tracing-core 0.1.36,
tracing-subscriber 0.3.23, opentelemetry 0.32.0, opentelemetry_sdk 0.32.1,
opentelemetry-otlp 0.32.0, opentelemetry-proto 0.32.0, tonic 0.14.6,
prost 0.14.4), crates.io registry metadata, resolved `cargo tree` probes built
in a scratch directory, the OTLP specification, and this workspace's own
`cargo tree`. Anything not verified that way is marked **unverified**, in the
D001/D002/D003 tradition.

## The question / 문제

`PLAN.md` §6 promises "standard tracing without touching call sites", and
PLAN-MOE **F4** makes the mechanism concrete: the guard's checks become an
ordered chain — authn → quota → safety → **telemetry** → audit — which is the
one place in the system where every request passes exactly once. F4 is the
insertion point, so the dependency question has to be answered before F4
picks a shape, not after. What may an MIT-only project depend on to emit
traces and metrics?

`PLAN.md` §6은 "호출 지점을 건드리지 않는 표준 트레이싱"을 약속하고, PLAN-MOE
**F4**가 그 기구를 구체화한다: authn → quota → safety → **telemetry** → audit
순서의 인터셉터 체인이 모든 요청이 정확히 한 번 지나는 유일한 지점이다. F4가
형태를 고르기 *전에* 의존성 질문이 답해져야 한다.

## Where this sits in the licensing boundary / 라이선스 경계에서의 위치

The boundary now has four categories, and observability is the first question
that lands in **none of them cleanly**:

1. **Logic from a published specification** — we write it (the ORB core).
2. **Data we cannot originate** (D001) — depended on, disclosed (`encoding_rs`).
3. **Logic whose failure modes our oracles cannot detect** (D002) — depended
   on, not written (crypto).
4. **A separate process whose output we read** (D003) — omniORB, `omniidl`,
   the `claude` CLI, a collector on `localhost:4318`.

Observability is category 1 by every test that matters. OTLP is a published
schema with a published transport binding; its failures are **loud** — a
collector rejects a malformed payload with a 400 and the span never appears,
which is precisely the deterministic oracle D002 said we could not build for
crypto. So nothing here is unverifiable-by-construction, and nothing here is
data we cannot originate. **This decision is therefore not about whether we
*may*; it is about whether we *should*** — and that makes weight, not licence,
the deciding axis. Saying so up front is the honest framing, because a survey
that only counts licences would clear all three options and answer nothing.

관측은 네 범주 중 어디에도 깔끔히 들어가지 않는다. OTLP는 공개 명세이고 실패는
**시끄럽다** — 콜렉터가 400을 던지고 스팬이 나타나지 않는다. 즉 D002가 암호에
대해 만들 수 없다고 한 결정적 오라클이 여기서는 만들어진다. 따라서 이 결정은
*해도 되는가*가 아니라 *해야 하는가*의 문제이며, 판단축은 라이선스가 아니라
**무게**다. 라이선스만 세는 조사는 세 후보를 모두 통과시키고 아무것도 답하지
못한다.

## What the survey actually found / 조사 결과

Three findings reverse this survey's own brief, in the D001 tradition of the
facts correcting the question.

### 1. The OpenTelemetry crates are Apache-2.0 **only** — there is no MIT arm

The brief asked to "verify Apache-2.0/MIT". They are not dual-licensed.

| Crate (version verified) | Declared on crates.io (verified) | What the tarball actually ships (verified) |
|---|---|---|
| `tracing` 0.1.44 | `MIT` | `LICENSE`, 25 lines, MIT text, "Copyright (c) 2019 Tokio Contributors" |
| `tracing-core` 0.1.36 | `MIT` | same MIT text |
| `tracing-subscriber` 0.3.23 | `MIT` | same MIT text |
| `opentelemetry` 0.32.0 | `Apache-2.0` | **no licence file of any kind in the tarball** |
| `opentelemetry_sdk` 0.32.1 | `Apache-2.0` | **no licence file** |
| `opentelemetry-otlp` 0.32.0 | `Apache-2.0` | **no licence file** |
| `opentelemetry-proto` 0.32.0 | `Apache-2.0` | **no licence file of its own**; the only Apache-2.0 text in the whole shipped OTel set is the vendored schema repo's, at `src/proto/opentelemetry-proto/LICENSE` (201 lines, Apache-2.0) |
| `tonic` 0.14.6 | `MIT` | `LICENSE`, 19 lines, MIT text, "Copyright (c) 2025 Lucio Franco" |
| `prost` 0.14.4 | `Apache-2.0` | `LICENSE`, 201 lines, full Apache-2.0 text |

Two things follow, and they must not be confused with each other.

**Apache-2.0 is not a bar.** It is permissive-with-attribution, one-way
compatible into our distribution, and D002 already accepted it throughout the
rustls provider chain. CLAUDE.md's non-negotiable line is drawn at LGPL/GPL/DOC
copyleft and at undisclosed provenance, and Apache-2.0 is neither.

**But the artifact hygiene is the worse position D001 warned about, in a new
shape.** D001's warning was a *declared licence that does not account for its
data*. Here the terms are honest and the provenance is plain; what is missing
is that **the shipped artifact does not carry its own licence text**. Apache-2.0
§4(a) makes giving recipients a copy of the licence the redistributor's job, so
if we ever vendored these crates the text would have to come from the
repository rather than from the crate. Verified separately: the
`opentelemetry-rust` repository does have `LICENSE` (HTTP 200) and has **no**
`NOTICE` (HTTP 404), so §4(d)'s NOTICE-propagation clause is inert — the
obligation is one attribution block in `NOTICE`, exactly the D001 shape. Same
result for `opentelemetry-proto` and for `tokio-rs/tracing` (LICENSE 200,
NOTICE 404).

**Apache-2.0는 장벽이 아니다** — 관대·귀속 조건이고 D002가 이미 rustls 체인
전체에서 수용했다. 그러나 **출하 타르볼이 자기 라이선스 텍스트를 싣지 않는다**는
점은 D001이 경고한 위치의 새로운 형태다. 저장소에는 `LICENSE`가 있고 `NOTICE`는
없음을 확인했으므로, 의무는 `NOTICE`에 귀속 한 블록 — D001과 같은 모양 — 이다.

### 2. `opentelemetry-otlp`'s default is **not** gRPC, and the weight is measured

The brief flagged "the protobuf/tonic dependency weight". The dependency is
real but the shape is not what was assumed. From the shipped
`Cargo.toml.orig`:

```
default = ["http-proto", "reqwest-blocking-client", "trace", "metrics", "logs", "internal-logs"]
grpc-tonic = ["tonic", "tonic-types", "prost", "http", "tokio", "opentelemetry-proto/gen-tonic"]
```

So the default is OTLP-over-HTTP with binary protobuf bodies sent by a
**blocking `reqwest`** client; `tonic` arrives only if `grpc-tonic` is asked
for. `prost` arrives either way. Measured with `cargo tree --edges normal` in
scratch probe crates, counting unique external crates (the probe itself
excluded):

| Configuration | External crates | Δ vs today |
|---|---|---|
| **this workspace today, default features** | **2** (`cfg-if`, `encoding_rs`) | — |
| this workspace, `--all-features` (i.e. `ssliop` on, D002's stack) | 12 | +10 |
| `tracing` alone (`default-features = false`, `std` + `attributes`) | 9 | +7 |
| `tracing` + `tracing-subscriber` (default features) | 18 | +16 |
| `tracing` + `tracing-subscriber` (`env-filter`, `json`) | 28 | +26 |
| `opentelemetry` + `_sdk` + `-otlp` (**default features**) | **92** | +90 |
| … `-otlp` with `grpc-tonic` only | 73 | +71 |
| … `-otlp` with `http-proto` + `trace` only, no default | 41 | +39 |

A licence sweep over all four probe trees (`cargo tree -f '{p} {l}'`) found
**no GPL, LGPL, MPL, AGPL, CDDL or EPL anywhere** — every node is MIT,
Apache-2.0, ISC, BSD, Unicode-3.0, Zlib or Unlicense. That is an honest
positive finding and it is worth stating as clearly as the negative ones. The
`Unicode-3.0` nodes (17–19 of them in the OTLP-default tree, ICU machinery
reached through `reqwest` → `url` → `idna`) are D001's accepted category
arriving as transitive freight rather than as a choice.

The comparison that decides nothing by itself but frames everything: **the
default OTLP stack adds 90 external crates to a workspace that currently has
two.** A project whose stated reason for existing is that it owns its wire
would be importing, in order to talk *about* itself, forty-six times more
third-party code than it uses to talk at all.

기본값은 gRPC가 **아니라** blocking `reqwest` + HTTP/protobuf이고, `tonic`은
`grpc-tonic`을 요청할 때만 온다. 실측: 현재 워크스페이스 외부 크레이트 **2개**
(전 피처 12개), `tracing` 단독 9, `tracing`+subscriber 18, OTLP 기본 92.
네 트리 전수 라이선스 조사에서 **copyleft는 하나도 없었다** — 정직한 긍정
결과다. 요지: 와이어 코어 전체가 2개인 시스템을 설명하기 위해 91개를 들여온다.

### 3. The "emit it ourselves" option is narrower and cheaper than expected

Two measurements change the shape of option C.

**The protobuf codegen toolchain is not, in fact, a build-time dependency of
the crate path.** `opentelemetry-proto` 0.32.0 ships **pre-generated** Rust —
`src/proto/tonic/*.rs`, 4,129 lines across eleven files — and has **no
`build.rs`**. Nothing runs `protoc` at build time. This defuses the toolchain
objection *against option B*, and it sharpens the objection *against option C*:
the crate has already paid the codegen cost, and a first-party binary-protobuf
encoder would be re-paying it by hand.

**But OTLP does not require protobuf at all.** Verified against the OTLP
specification (`opentelemetry.io/docs/specs/otlp/`, fetched 2026-08-13): OTLP
defines OTLP/gRPC and OTLP/HTTP, and OTLP/HTTP accepts **JSON-encoded Protobuf
payloads** as a normative, non-experimental encoding — "JSON Protobuf encoded
payloads use proto3 standard defined JSON Mapping", `Content-Type:
application/json`, `POST` to `/v1/traces`, `/v1/metrics`, `/v1/logs`; retryable
status codes 429/502/503/504 with exponential backoff. The spec's wording for
receivers is `SHOULD` accept, not `MUST` — but the reference receiver does:
the OpenTelemetry Collector's `otlpreceiver` README states it "can receive
trace export calls via HTTP/JSON in addition to gRPC", default HTTP port 4318.

And the encoding half is **already in this repository**.
`orbweaver_dynamic::json::Json` is a first-party JSON model with a `Display`
implementation and its own string escaper (`write_string`), written for
AnyJSON and exercised by the byte-identical round-trip tests. Emitting
OTLP/HTTP+JSON therefore needs: a struct-to-`Json` mapping against a published
schema, an HTTP/1.1 `POST`, and a backoff. It needs no protobuf, no codegen,
no tonic, no reqwest.

`opentelemetry-proto`는 생성된 Rust를 그대로 싣고 `build.rs`가 없다 — protoc
빌드 의존성은 실재하지 않는다. 그리고 **OTLP는 protobuf를 요구하지 않는다**:
OTLP/HTTP의 JSON 인코딩은 규범적이며, 콜렉터가 4318에서 받는다. 인코딩 절반은
이미 저장소에 있다(`orbweaver_dynamic::json`, `Display` + 이스케이퍼).

## Does the GIOP reasoning transfer? / GIOP 논리는 이전되는가

This is the question the project's own history forces, and the honest answer
is **partly, and the part that fails is the decisive one.**

**Where it transfers.** GIOP was written first-party because it is logic
defined by a published specification, and because our oracle sees the
mistakes. Both hold for OTLP. The schema is published under Apache-2.0
(verified: the vendored `opentelemetry-proto/LICENSE`), the JSON mapping is
proto3-standard, the transport is HTTP, and a real collector is a fixture in
exactly D003's fourth category — a separate process whose output we read.
A first-party emitter could be batch-oracle'd the same way every other wire
claim in this project is: emit N spans, read them back out of a collector's
file exporter, compare **decoded values**, both directions. That is a real
oracle, not a hoped-for one.

**Where it does not.** Three ways, in ascending order of importance.

1. *Protobuf binary would be a toolchain we would own.* Avoidable — take the
   JSON encoding — but only by accepting a `SHOULD`-level guarantee from
   receivers instead of the `MUST`-level one that binary protobuf enjoys. Any
   backend that only speaks OTLP/gRPC would be out of reach without the
   toolchain. This is a real narrowing, and it is the reason option C is a
   *default sink*, not a *complete exporter*.
2. *The spec is alive.* OMG GIOP is frozen; CORBA 3.4 dates from 2012 and the
   wire has not moved under us once. OTLP and its semantic conventions are a
   living CNCF specification that revises on its own cadence. First-party GIOP
   is a fifteen-week cost that then stops; a first-party OTLP exporter is a
   subscription to somebody else's release schedule, paid in batches that
   produce no first-pass rate anybody cares about.
3. **Nobody is forcing us to own it, and that is the whole difference.** The
   ORB is first-party because *no MIT ORB exists* — the absence is the
   project's reason to be. An MIT-licensed OTLP exporter is not a gap in the
   world; `opentelemetry-otlp` is Apache-2.0, maintained, and permissive.
   Writing GIOP created the thing that did not exist. Writing an OTLP exporter
   would recreate a thing that does, using time that only this project can
   spend on the thing only this project is doing.

And one note that is either amusing or the correct place to stop: adopting
`grpc-tonic` means a project whose thesis is that it owns its RPC stack
importing a **second, larger RPC stack** in order to describe the first one.
That is not an argument against OTLP; it is an argument about which OTLP.

**부분적으로만 이전된다.** 이전되는 쪽: OTLP는 공개 명세이고, 콜렉터를 픽스처로
둔 결정적 오라클이 실제로 만들어진다(스팬 N개를 내보내고 콜렉터 파일 익스포터에서
읽어 **디코드된 값**을 비교). 이전되지 않는 쪽 셋: (1) 이진 protobuf는 우리가
소유할 툴체인 — JSON으로 피할 수 있으나 수신자 보장이 `MUST`에서 `SHOULD`로
내려간다, (2) GIOP 명세는 2012년에 멈췄지만 OTLP는 살아 있는 명세다 — 남의 릴리스
일정 구독, (3) **아무도 우리에게 이것을 소유하라고 강요하지 않는다.** ORB가
1st-party인 이유는 MIT ORB가 *없기* 때문이다. MIT OTLP 익스포터의 부재는 세상의
공백이 아니다.

## Options considered / 검토한 대안

| Option | Verdict |
|---|---|
| **A. First-party span/event record on F4's chain + JSON-lines sink** — zero crates; reuses `orbweaver_dynamic::json` and the `identity::audit_line` precedent | **Recommended now.** Satisfies every consumer that exists today (the harness, `run_checks.sh`, the audit ledger). `cargo tree` unchanged. The one thing it does not give is a UI, and no UI exists to want it |
| **B. `tracing` + `tracing-subscriber` as facade, no exporter** | **Pre-cleared, not adopted.** MIT verified from shipped tarballs; +7 crates for the facade alone, +16 with a default subscriber. The catch is stated below: the usual reason to adopt `tracing` does not apply here |
| **C. `opentelemetry` + `opentelemetry-otlp`** | **Not cleared for adoption; recorded with its cost.** Licence-acceptable (Apache-2.0, permissive, no copyleft anywhere in the tree — verified). +90 crates at default, +39 at the narrowest useful configuration. No named consumer: the console that would read the traces is ❌ with nothing built |
| **D. First-party OTLP/HTTP+JSON emitter** | **The pre-cleared upgrade path from A**, if a pilot names a collector. Encoding is specified and half-built; ~1 batch of work; oracle is a real collector. Explicitly *not* a full exporter — no OTLP/gRPC, no binary protobuf |
| **E. `tracing-opentelemetry` bridge** | **Unverified** — not fetched, not examined. It only becomes relevant if both B and C are adopted, which nothing here recommends |
| **F. Anything statsd/Prometheus-shaped** | **Unverified** — out of scope. PLAN §6 names OpenTelemetry; substituting a different standard is a plan change, not a dependency decision |

### The sharp point about `tracing` / `tracing`에 관한 날카로운 지점

`tracing`'s real value is that it is a **facade**: the ecosystem already emits
into it, so adopting it makes other people's instrumentation visible for free.
**That argument is close to void in this workspace.** We have two external
crates. `encoding_rs` does not emit tracing events; `cfg-if` is a macro.
With `ssliop` on we would gain `rustls`, which emits through `log` — reachable,
but through `tracing-log`, which is one more crate for one peer's handshake
diagnostics. Adopting a facade whose network effect we are not connected to
buys the API, not the ecosystem — and the API we would be buying is one we can
write in an afternoon against F4's chain, which has to exist either way.

This is not an argument that `tracing` is bad. It is an argument that its
benefit **arrives later than its cost**, and the project's own sequencing rule
— a dependency may not precede the oracle that measures it — says to wait for
the arrival.

`tracing`의 진짜 가치는 **파사드**라는 점 — 생태계가 이미 그것으로 내보낸다 —
인데, 외부 크레이트가 2개인 이 워크스페이스에서 그 논거는 거의 무효다. 이익이
비용보다 **늦게 도착**하며, "의존성은 그것을 측정할 오라클을 앞서지 않는다"는
이 프로젝트의 순서 규칙이 기다리라고 말한다.

## The insertion point / 삽입 지점

Whatever is adopted, it goes in one place: **PLAN-MOE F4's interceptor chain**
(authn → quota → safety → **telemetry** → audit). At the time of writing,
`crates/orbweaver-mcp/src/interceptor.rs` **does not exist in this worktree**
(measured: the crate's `src/` holds `guard.rs`, `handles.rs`, `identity.rs`,
`lib.rs`, `policy.rs`, `promote.rs`, `rpc.rs`, `session.rs`). It may be landing
in a parallel batch; either way it is the seam this decision targets, and the
requirement on it is the same in both worlds:

- **The telemetry stage takes a sink trait, not a crate type.** If the stage's
  signature names `tracing::Span` or an OTel type, the dependency stops being
  reversible and D004 stops being a decision. A first-party
  `trait TelemetrySink` with a no-op default keeps every option in this
  document open for the cost of one indirection.
- **Absence is reported, never greened.** With no sink configured the harness
  reports the telemetry group *unmeasured* — counted, named — per the harness
  rule that an unmeasured check is a failure.
- **Granularity is the control plane's, inherited from PLAN-SERVICES §4:**
  control-plane events, never per token. The residency machine's discipline
  (`apply()` takes a slice per window; there is nowhere to hang a callback)
  is the shape to copy, not to weaken.

무엇을 채택하든 자리는 하나 — F4 인터셉터 체인의 telemetry 단계다. 집필 시점
`crates/orbweaver-mcp/src/interceptor.rs`는 **이 워크트리에 없다**(실측). 요구는
동일하다: 단계의 시그니처는 크레이트 타입이 아니라 **1st-party 싱크 트레이트**를
받는다(그렇지 않으면 의존성이 되돌릴 수 없게 되고 D004는 결정이기를 그친다),
부재는 *미측정*으로 보고하며, 입도는 컨트롤 플레인 — 토큰당 절대 금지 — 이다.

## Recommendation / 권고

**Adopt nothing today. Build option A with F4, behind a first-party sink
trait, and record B, C and D as pre-cleared paths with their triggers.**

Concretely, three tiers:

1. **Now, zero dependencies.** F4's telemetry stage emits a first-party span
   record through `trait TelemetrySink`; the only implementation is
   JSON-lines to a file or stderr, built on `orbweaver_dynamic::json` and
   shaped like `identity::audit_line` (which already proves the discipline:
   the record names the principal and structurally *cannot* carry the
   credential). `cargo tree` stays at 2. This satisfies the harness, the audit
   ledger and any `jq`-shaped question anybody has today.
2. **`tracing` + `tracing-subscriber`, pre-cleared, MIT verified from the
   shipped tarballs, behind an off-by-default `tracing` feature.** Trigger,
   named precisely so it is a decision and not a drift: *the first dependency
   we adopt that emits into `tracing` and whose events we need* (rustls under
   `ssliop` is the likely first), **or** `orbweaver-console` reaching the batch
   where it renders span trees rather than lines. Neither is true today.
3. **OTLP, pre-cleared but not endorsed.** When a pilot names a collector, the
   first reach is **option D — a first-party OTLP/HTTP+JSON emitter**, ~1
   batch, oracle = a real collector as a D003-category fixture, spans compared
   as decoded values. Reach for `opentelemetry-otlp` only if a deployment
   demands binary protobuf or OTLP/gRPC, and then with
   `default-features = false, features = ["http-proto", "trace"]` (41 external
   crates measured) rather than the 92-crate default. **`grpc-tonic` is the option of
   last resort**: it is a second RPC stack, and this project should need a
   written reason to carry one.

**오늘은 아무것도 채택하지 않는다. F4와 함께 후보 A를 1st-party 싱크 트레이트
뒤에 짓고, B·C·D는 방아쇠와 함께 사전 정리 경로로 기록한다.** (1) 지금, 의존성 0:
F4의 telemetry 단계가 `trait TelemetrySink`로 1st-party 스팬 레코드를 내보내고
유일한 구현은 `orbweaver_dynamic::json` 위의 JSON-lines — `cargo tree`는 2개
그대로. (2) `tracing`+`tracing-subscriber`는 출하 타르볼에서 MIT 확인 완료,
기본-꺼짐 `tracing` 피처 뒤 사전 정리. 방아쇠: *`tracing`으로 이벤트를 내보내는
첫 의존성을 채택할 때*(`ssliop`의 rustls가 유력) 또는 콘솔이 줄이 아니라 스팬
트리를 그리는 배치에 도달할 때 — 오늘은 둘 다 아니다. (3) OTLP는 사전 정리하되
지지하지 않는다: 파일럿이 콜렉터를 지명하면 먼저 **후보 D**(1st-party
OTLP/HTTP+JSON, 약 1배치, 오라클은 실제 콜렉터), 이진 protobuf가 필요할 때만
`opentelemetry-otlp --no-default-features --features http-proto,trace`(실측 41개),
`grpc-tonic`은 최후 수단 — 두 번째 RPC 스택을 지려면 글로 된 이유가 필요하다.

If this recommendation is approved, no policy amendment is required. D001's
data clause and D002's oracle-blind-logic clause are both untouched:
observability is category-1 logic we are choosing to *scope*, not a new
category. That is a smaller decision than D001, D002 or D003, and saying so is
part of reporting it honestly.

승인되어도 **방침 개정은 필요 없다.** D001의 데이터 조항도 D002의 오라클
사각 조항도 건드리지 않는다 — 관측은 범주 1의 로직이고 우리는 그 *범위*를 고를
뿐이다. 이 결정은 앞선 셋보다 작으며, 그렇다고 적는 것이 정직한 보고의 일부다.

## What was verified, and what was not / 검증된 것과 아닌 것

Verified directly (2026-08-13): crates.io declared licences and the licence
files (or their absence) inside the shipped tarballs of tracing 0.1.44,
tracing-core 0.1.36, tracing-subscriber 0.3.23, opentelemetry 0.32.0,
opentelemetry_sdk 0.32.1, opentelemetry-otlp 0.32.0, opentelemetry-proto
0.32.0, tonic 0.14.6 and prost 0.14.4 — the four OpenTelemetry tarballs carry
no licence file, checked by listing the extracted archives; the vendored
`opentelemetry-proto/LICENSE` (201-line Apache-2.0); `opentelemetry-otlp`'s
`[features]` and `[dependencies]` from its shipped `Cargo.toml.orig` and the
resolved `Cargo.toml`; `opentelemetry-proto`'s pre-generated `src/proto/tonic`
Rust (4,129 lines, no `build.rs`); HTTP status of `LICENSE` (200) and `NOTICE`
(404) in the `opentelemetry-rust`, `opentelemetry-proto` and `tokio-rs/tracing`
repositories; every crate count in the weight table, from `cargo tree
--edges normal --prefix none` in throwaway probe crates and from this
workspace's own tree at default and `--all-features`; the licence sweep over
all four probe trees with `cargo tree -f '{p} {l}'`; the OTLP specification's
transports, encodings, content types, paths and retry codes; the OTel
Collector `otlpreceiver` README's HTTP/JSON support; the absence of
`interceptor.rs` in `crates/orbweaver-mcp/src/`; `orbweaver_dynamic::json`'s
`Display` implementation and `write_string` escaper; and the absence of any
clock read in `orbweaver-object`'s `residency.rs` and in `orbweaver-trading`.

**Unverified, stated plainly:** the **per-file headers** of the OpenTelemetry
Rust sources (we rely on the repository `LICENSE` and the crates.io
declaration, since the artifact carries neither); the **transitive trees'
individual licence files** — the sweep read `cargo tree`'s `{l}` metadata
field for ~90 crates, not ninety tarballs, which is registry metadata and
therefore exactly the layer D001 says is not the last word; `tracing-opentelemetry`
(option E, never fetched); any **statsd/Prometheus** crate (option F, out of
scope); whether a **real OTel Collector runs on this machine** — deliberately
not probed, because measuring the fixture belongs to the batch that needs it,
the same line D003 drew for `CREATE EXTENSION vector`; the **runtime cost** of
any option (no benchmark was run; every number here is a crate count, not a
microsecond); whether `interceptor.rs` **exists in a parallel batch** (this
worktree only, this commit, today); and the **semantic-convention** attribute
names a first-party emitter would have to use — a real design cost for option
D that this survey did not price.

**검증 안 된 것도 그대로 적는다:** OpenTelemetry Rust 소스의 **개별 파일 헤더**
(아티팩트가 라이선스를 싣지 않으므로 저장소 `LICENSE`와 crates.io 선언에 의존),
추이 트리 ~90개 크레이트의 **개별 라이선스 파일**(레지스트리 메타데이터만 읽었다 —
D001이 "마지막 말이 아니다"라고 한 바로 그 계층), `tracing-opentelemetry`(미조사),
statsd/Prometheus 계열(범위 밖), 이 머신의 **콜렉터 실제 기동 여부**(일부러
측정하지 않았다 — 픽스처 측정은 그것을 필요로 하는 배치의 몫, D003이 `CREATE
EXTENSION vector`에 그은 것과 같은 선), 각 후보의 **런타임 비용**(벤치마크 없음 —
모든 숫자는 크레이트 개수이지 마이크로초가 아니다), `interceptor.rs`의 **병렬
배치 존재 여부**(이 워크트리·이 커밋·오늘), 그리고 후보 D가 따라야 할 **시맨틱
컨벤션** 속성 이름들(실재하는 설계 비용인데 이 조사는 값을 매기지 않았다).

## What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is adopted today; `cargo tree` is unchanged by this document and stays
at two external crates even if the recommendation is approved — that is the
point of tier 1. The span record's field set, the sink's configuration
surface, the log/metric split, sampling, the console's rendering, and F4's
chain ordering tests are all later batches with their own oracles. F4 itself
is PLAN-MOE's batch, not this document's: D004 constrains one signature in it
and nothing else. Whether `PLAN.md` §6's "OpenTelemetry via interceptors" line
should be reworded is a question for the batch that closes the row, not for
the survey that unblocks it.

오늘 채택되는 것은 없다. 이 문서로 `cargo tree`는 변하지 않으며, 권고가
승인돼도 외부 크레이트 2개 그대로다 — 그것이 1단계의 요점이다. 스팬 레코드의
필드 집합, 싱크 설정 표면, 로그/메트릭 분리, 샘플링, 콘솔 렌더링, F4 체인 순서
테스트는 각자의 오라클을 가진 이후 배치들의 몫이다. F4 자체는 PLAN-MOE의
배치이며 이 문서는 그 안의 시그니처 하나만 제약한다. `PLAN.md` §6 문구를 고칠지는
그 행을 닫는 배치의 질문이지, 그것을 푸는 조사의 질문이 아니다.
