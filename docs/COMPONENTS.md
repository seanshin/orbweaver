# Component coverage — measured, not aspirational / 구성요소 커버리지 — 측정 기준

Reviewed 2026-08-14 against the roster in README and PLAN §5/§6. "✅" means
landed **and** exercised by the harness or tests named in a PHASE report;
"◐" means partially landed with the missing half stated; "❌" means no code.
Anything requiring a new dependency class needs a decision document first
(D001/D002 precedent) — those rows say so.

## Components / 구성요소

| Component | Status | What exists | What is missing |
|---|---|---|---|
| `orbweaver-cdr` | ✅ | full CDR, both endians, alignment origins | — |
| `orbweaver-giop` | ✅ | GIOP 1.0–1.2 both directions, codesets, fragmentation, locate, failover, SSLIOP (`ssliop` feature) | multiplexing, pooling (stream E) |
| `orbweaver-idl` | ✅ | IDL 4.2 front end, SIDL comments, full oracle agreement | pluggable back ends beyond gen's use |
| `orbweaver-registry` | ◐ | registry from IDL, TypeCodes vs two peers, §5.3 differ | **remote IFR ingestion** (planned scope, unstarted) |
| `orbweaver-object` (poa) | ✅ | references, identity, POA, LOCATION_FORWARD emit | — |
| `orbweaver-dynamic` | ✅ | value marshalling, DII-shaped invoke, AnyJSON, first-party JSON | DynAny mutation API (fuzz seed exists via Value) |
| `orbweaver-forge` | ◐ | S4 gate, §5.1 orchestrator, real-model batch (20/20), S5 in flight (wave 3) | **S1 ingest, S3 annotate as distinct stages** (today S2's prompt does both) |
| `orbweaver-mcp` | ✅ | triad, stdio transport, capability handles, default-deny, `ai_authz`, promotion+I4 | embeddings behind search (needs D003) |
| `orbweaver-guard` | ◐ | authz scopes, destructive approval, audit (Guarded; Bridge emission in flight) | **dry-run mode**; interceptor chain as a formal seam |
| `orbweaver-capability` | ✅ | lives inside mcp (`handles.rs`) rather than its own crate — a location choice, not a gap | expiry policy configuration surface |
| `orbweaver-identity` | ◐ | CSIv2 wire, delegation policy, hygiene, `Caller` seam | **OAuth2/JWT → Caller exchange; credential store; SSLIOP peer proof (wave 3 probing)** |
| `orbweaver-gen` | ◐ | Rust client stubs, oracle static=dynamic, I1/I4 | **server skeletons, scaffolds, Python/other targets** |
| `orbweaver-test` | ❌ → wave 3.5 | nothing | **contract/property tests from annotations; corpus-wide fuzz** — agent launched with this review |
| `orbweaver-console` | ❌ | nothing | catalog browser, diff viewer, traces — after OTel decision |

## Services (PLAN §6) / 서비스

| Service | Status | Note |
|---|---|---|
| Catalog storage (PostgreSQL + pgvector) | ❌ | in-process Registry + `exposure.todo.tsv` are the stated seams; **needs D003 (storage/embedding dependencies)** before any crate lands |
| Embeddings / semantic search | ❌ | frozen benchmark holds the 0/10 headroom baseline; **same D003** |
| Observability (OpenTelemetry) | ❌ | interceptor seam unbuilt; **needs D004 (otel dependency licence review)** |
| Deployment (Docker/K8s, IOR rewriting) | ❌ | R7 mitigation designed in PLAN, nothing built; CI runners are the only containers used |
| Naming (CosNaming client) | ✅ | corbaname/corbaloc + omniNames in harness |
| MCP transport | ✅ | stdio JSON-RPC; no real MCP client driven yet (stated in PHASE3) |

## Reading this honestly / 정직한 독해

The wire→bridge spine (cdr→giop→idl→registry→dynamic→mcp→gen) is implemented
and measured. What is **not** progressing in parallel today is the operations
layer: durable catalog, telemetry, console, and the model-facing S1/S3 stages.
None of those are blocked technically — storage/otel/embeddings are blocked on
**dependency decisions** (the D001/D002 discipline), S1/S3 on nothing at all.
This file exists so that gap stays visible instead of being implied by absence.

와이어→브릿지 축은 구현·측정되었다. 오늘 병행되지 **않고** 있는 것은 운영
계층(영속 카탈로그, 관측, 콘솔)과 모델 대면 단계(S1/S3)다. 기술적으로 막힌 것은
없다 — 저장소·관측·임베딩은 **의존성 결정 문서**(D001/D002 규율)에, S1/S3는 아무
것에도 막혀 있지 않다. 이 파일은 그 공백이 부재로 암시되는 대신 눈에 보이게 한다.
