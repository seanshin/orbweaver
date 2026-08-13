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
| `orbweaver-giop` | ✅ | GIOP 1.0–1.2 both directions, codesets, fragmentation, locate, failover, SSLIOP (`ssliop` feature), **concurrent connections** (stream E): one servant behind one mutex taken per message, cap 64 with the refusal spoken as §9.4.7's `CloseConnection`, `ServerStats` counters, deadlock proven absent by test rather than by argument | **dispatch is still serialized** — a slow operation delays every client though it no longer excludes them; multiplexing and pooling |
| `orbweaver-idl` | ✅ | IDL 4.2 front end, SIDL comments, full oracle agreement | pluggable back ends beyond gen's use |
| `orbweaver-registry` | ◐ | registry from IDL, TypeCodes vs two peers, §5.3 differ | **remote IFR ingestion** (planned scope, unstarted) |
| `orbweaver-object` (poa) | ✅ | references, identity, POA, LOCATION_FORWARD emit, **expert residency state machine + ExpertLoader** (F3, 2026-08-14), **`moe::ExpertRegistry`/`ExpertLoader` served on the wire** (2026-08-15) | — |
| `orbweaver-dynamic` | ✅ | value marshalling, DII-shaped invoke, AnyJSON, first-party JSON, **recursive types** (markers resolved against the enclosing type the error path is already standing on; nesting bounded at 64 on both sides, because on decode the depth is the sender's choice) | DynAny mutation API (fuzz seed exists via Value) |
| `orbweaver-forge` | ◐ | S4 gate, §5.1 orchestrator, real-model batch (20/20), S5 exposure-off registration (I2 ✅) | **S1 ingest, S3 annotate as distinct stages** (today S2's prompt does both) |
| `orbweaver-mcp` | ✅ | triad, stdio transport, capability handles, default-deny, `ai_authz`, promotion+I4, **vector search by external command (D003-A)**: `spikes/embed.sh` process boundary, `orbweaver-vectors` cache format, lexical∪vector union tagged `via`, no-index path byte-identical to before (golden-literal assertion) | **the synonym class is UNMEASURED** — no `VOYAGE_API_KEY` here, the harness SKIPs it, and the offline stand-in's 0/10 is a plumbing number a token-overlap embedder cannot beat by construction |
| `orbweaver-guard` | ✅ | authz scopes, destructive approval, audit emitted on BOTH paths (one formatter, string-equality pinned), **F4 interceptor chain**: audit / telemetry / authz.exposure / authz.scopes / safety.approval, short-circuit on first refusal and `after` unwound in reverse. Registration order is deliberately not acting order — observers register outermost so an audit stage still sees a refusal a gate ahead of it produced | **dry-run mode**; the named-but-empty seats `SEAT_QUOTA` and `SEAT_SAFETY_CONTENT` (the latter needs decoded arguments, which the chain runs before); telemetry has counts and no latency (no clock) |
| `orbweaver-capability` | ✅ | lives inside mcp (`handles.rs`) rather than its own crate — a location choice, not a gap | expiry policy configuration surface |
| `orbweaver-identity` | ◐ | CSIv2 wire, delegation policy, hygiene, `Caller` seam | **OAuth2/JWT → Caller exchange; credential store; SSLIOP peer proof** — measured BLOCKED: brew's omniORBpy ships no sslTP binding (C++ SSL transport present in the keg, python half unbuilt); unblock path in `spikes/tls/PEER-STATUS.md` |
| `orbweaver-gen` | ◐ | Rust client stubs, oracle static=dynamic, I1/I4, **server skeletons**: servant trait + generated `Dispatch` + a user-exception enum per interface, `_is_a` from the resolved chain, `MARSHAL`/`BAD_OPERATION` answered rather than panicked. **omniORB's python client drives a generated skeleton** (harness) | **a servant cannot raise a system exception** — the trait's error type is the user-exception enum, so an interface with no `raises` has an uninhabited error and literally cannot fail; no `knows()`/object keys, so one servant per process and `naming_server`'s multi-context shape is not yet generatable; no `LOCATION_FORWARD`; no server-side static-equals-dynamic oracle; Python/other targets |
| `orbweaver-test` | ✅ | seeded round-trip property over 66 golden types × 32 cases × 2 orders × 8 alignment phases (**0 defects**, findings replay from `seed=`), annotation contract advice, `contract-check` in the harness, **`wire-fuzz` panic freedom**: 10 decoders a peer reaches before any policy runs (`read_message`, `decode_request`, `decode_reply`, TypeCode, IOR, dynamic `any`), uniform/mutated/truncated inputs, **0 panics in 50k cases × 10 targets** with the reach reported so a green run can be read | quota/safety contract rules once a policy exists to check against; `fixed` reports as `prop/unmeasured` rather than passing (§4.4); the recursive gap closed and is now asserted at zero |
| `orbweaver-console` | ❌ | nothing | catalog browser, diff viewer, traces — after OTel decision |

## Services (PLAN §6) / 서비스

| Service | Status | Note |
|---|---|---|
| Catalog storage (PostgreSQL + pgvector) | ❌ | in-process Registry + `exposure.todo.tsv` are the stated seams; **D003 drafted (PROPOSED)**: defer until a pilot demands durability, adoption path pre-cleared (tokio-postgres + pgvector, licences verified) |
| Embeddings / semantic search | ◐ | D003-A landed: wrapper, cache format and lexical∪vector union built and tested; the **synonym class is still UNMEASURED** because no key exists here. The frozen v1 set keeps the 0/10 headroom baseline beside v2's widened 28/28 |
| Observability (OpenTelemetry) | ❌ | the seam now exists — F4's `TelemetryInterceptor`, counts only — and nothing is emitted through it. **D004 drafted (PROPOSED)**: first-party JSON-lines sink behind a sink trait on F4's chain (zero crates), `tracing` (MIT, verified) and OTLP pre-cleared with triggers, awaiting approval |
| Deployment (Docker/K8s, IOR rewriting) | ❌ | R7 mitigation designed in PLAN, nothing built; CI runners are the only containers used |
| Naming (CosNaming client) | ✅ | corbaname/corbaloc + omniNames in harness |
| MCP transport | ✅ | stdio JSON-RPC; no real MCP client driven yet (stated in PHASE3) |
| CosNaming **server** | ✅ | F6 landed 2026-08-14: full context surface + NamingContextExt, both oracle directions measured — omniORB's client decoded our NotFound bytes. Suite plan: PLAN-SERVICES §2 |
| Event/Notification (CosEvent) | ✅ push model | F7 landed: `EventChannel`/both admins/both proxies served on our POA, `any` relayed verbatim, bounded queue (64, drop-oldest), dead consumers disconnected after 3 consecutive failures with **drops counted, never silent**. omniORBpy 4.3.4's `PushConsumer` attaches to our channel and decodes what we push (harness group). Pull model refused `BAD_OPERATION` with a reason — it inverts flow control into the unbounded buffer the queue exists to avoid; `destroy` refused (F6 precedent). Notification service: `docs/PLAN-DEFERRED.md` |
| Trading (decision engine) | ✅ | `orbweaver-trading` (2026-08-14, 37 tests): offers, §4.3 constraint queries, §6 loading policy over deterministic traces. Wire surface: the row below |
| Interface Repository (read-only facade) | ✅ | `orbweaver-registry::ifr`: `lookup_id`, the `Contained` getters, `describe_interface`, `is_a`, `_get_base_interfaces`, served on our POA with keys derived from the repository id (no per-reference state, references survive a restart). **omniORB's own IR client** narrows it and prints enumerators by *name* (`PARAM_IN`, `OP_ONEWAY`, `dk_Interface`), so the ordinals are right rather than self-consistent. Writes refused `NO_PERMISSION` before target resolution — the registry is populated from IDL through S4, and a writable IFR would be a second, ungated ingestion path |
| Trading **wire surface** | ✅ | `orbweaver-object::expert_service` serves `moe::ExpertRegistry`/`ExpertLoader` (corpus/golden/22) — the project contract, not the standard `CosTrading` facade (PLAN-SERVICES §3). Oracle is our own client (`spike-experts`); no foreign MoE peer exists to test the other direction, and none is claimed |
| LifeCycle / Property | ✅ | F5: `ModelFactory`/`ComposedModel`/`PolicyDomain`/`EnterpriseExpert` served from corpus/golden/23. **The object key is the tenant context** — `moe::CallContext` carries no tenant and a servant sees no service context, so there is a factory per tenant; a shared one could not have checked `retire` at all. Refusal precedes the existence check, so it is not an existence oracle. `base()` is served, counted and audited: two tenants on one base necessarily get the identical reference, which is a correlator by construction and only stops by not sharing. Scopes and caller identity are **not** enforced here and the module says which layer owns each (guard chain, MCP capability handles, CSIv2) |
| Transaction / Time / PSS / Concurrency / CosCollections / Notification / federated naming / full Security Service | — | **excluded and designed**: `docs/PLAN-DEFERRED.md` gives each a chapter with the concrete trigger that would un-defer it and a v1 sketch, so "excluded" means "designed enough to resume" rather than "forgotten". Honest absence over decorative interfaces |

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
