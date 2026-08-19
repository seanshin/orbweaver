# 2026-08-19 — the plan reviewed against the code, row by row / 계획서를 코드에 행 단위로 대조

> At v0.5.0 (`a36e41f`), three read-only reviewers checked every remaining-work,
> status and risk row of `PLAN.md` §7–§12, `PLAN-SERVICES.md`, `PLAN-MOE.md`,
> `PLAN-DEFERRED.md` and D010 §5 against the tree, the harness and the
> generated `SERVICES-COVERAGE.md` §8. This is the synthesis: what is stale,
> what is open, what needs a fixture, and the order to act. It is a dated
> record; the plan documents are the homes of the facts and are corrected by
> the batches §5 names, not by this file.
>
> v0.5.0 시점에 계획 문서 다섯의 모든 잔여·상태·리스크 행을 코드·하네스·생성된
> 커버리지 블록에 대조했다. 이 파일은 종합이며 날짜 붙은 기록이다; 사실의 집은
> 계획 문서이고 §5의 배치가 그것을 고친다.

## 1. The shape of what was found / 발견의 모양

| Class | Count | Examples |
|---|---:|---|
| **Stale — the doc says missing, the tree has it** | 19 rows | PLAN §7.2 "S1–S3 not started" vs `COMPONENTS.md` measured rates; §7.4 I4 "audit line reconstructed" vs `Bridge::audit()` and a capturing oracle; PLAN-SERVICES §2/§8.1.1 CosNaming `bind_context`/`destroy` "refused" vs served; PLAN-MOE §2/§3 F4/F5 ◐ vs landed; D006/PLAN-MOE "`gen` drops the bound" vs `bounds_oracle.rs`; R7 "a real routing domain remains unmeasured" vs PHASE6's second-host run |
| **Overstated — the doc says done, the tree does half** | 6 rows | R13 "identity propagated via CSIv2 SAS" (unit-tested, no peer, verifier empty); R17 "mid-connection re-establishment" (expiry half only); R11 "quarantined until approved" (nothing quarantines, by design); §8 Codesets "EUC-KR against every fixture ORB" (no peer negotiates it); §8 Generated code "contract tests" (nothing generates a test); §8 AI quality "both sets run every release" (no v0.5.0 run) |
| **Instrument named wrong** | 4 rows | §8 "one `run_checks.sh` group per cell" (3 groups, 4 cells — JacORB's two directions share one counter); §8 DII "over the golden corpus" (runs over `echo.idl`; the corpus goes through DynAny/AnyJSON); §8 IDL semantics names `sidl-validate` (harness runs `idl-check`); §11 same per-cell claim |
| **Hand-typed numbers a script could compute** | 6 | "37 tests" (40), "20 tests (34 in the crate)" (21 / 84), "17 tests" (30), "12 of 107" (0 absent of 106), "fifty served" (63), golden "20–30" (31) |
| **One fact, several homes, several answers** | 3 | CosNaming's three operations: §2 "refused loudly" / §8.1.1 "moved to `NO_IMPLEMENT`" / wire: served; `Router::dispatch`: servant header says `BAD_OPERATION`, code and wire say `NO_IMPLEMENT`; the three 2026-08-18 deferrals live in Rust doc comments and D010 §5, not in `PLAN-DEFERRED` |
| **Genuinely open and measurable here** | 9 | per-peer CSIv2 catalogue record; SIDL version marker; `valuetype`/`fixed` decision gate; `idl-diff --approve` store; event channel ← telemetry (F4+F7 both exist, nothing publishes); `Bridge::stats` through `connect_static`; the JacORB harness group split; a reader test for a conformant non-zero default label (R18); a v0.5.0 pipeline run over both requirement sets (needs a model key at run time — class B until then) |
| **Deferred with a trigger, none fired** | 8 + 3 | every `PLAN-DEFERRED` chapter re-verified today (§3 Time: still no `SystemTime` in residency/trading; §7 federation: F5 evaluated the trigger *in code* and found the other shape); the three 2026-08-18 deferrals' rewritten reasons still true |
| **Needs a party we do not have** | A1–A9, B1–B4, B6 | pilot owner, independent evaluator, TAO, IdP, SSL peer, docker, model key |

**Progress was wrong in both directions again**, as it was on 2026-08-18: nineteen rows understate and six overstate. None of the six overstatements is in a place a test could reach — they are risk-register mitigations and verification-strategy rows written in the indicative.

**진행률은 다시 양방향으로 틀려 있었다** — 열아홉 행이 과소, 여섯 행이 과대. 과대
여섯은 모두 테스트가 닿지 않는 자리(리스크 완화 열, 검증 전략 행)에 있다.

## 2. PLAN.md, row by row / 계획서 본문

### §7.2 / §7.3 streams
- **Stream A** — S1/S2/S3 exist with their gates and were measured against a real model (`COMPONENTS.md:19`, `docs/pipeline-runs/2026-08-13-split-pipeline.md`); the repair loop is in-process (`pipeline.rs:407`), driven by `sidl-validate --repair-prompt`; the SIDL vocabulary is one constant mirrored in two crates with a pinning test — but **"v1" is a doc comment**: no `SIDL_VERSION`, nothing a consumer can check. §7.2's "not started … S1–S3" contradicts `COMPONENTS.md` in the same tree. And the harness's replay of S1–S3 prints `ok`, while the embeddings arm with the same shape (a model absent) prints `SKIPPED` — the "unmeasured is not passing" rule applied to one and not the other.
- **Stream B** — Python is clients only (A6, by recorded decision). I4's gate is landed and live-verified. "Contract tests generated from annotations": nothing generates a test; `ai_precond` is in the vocabulary and read by nothing. "`valuetype`/`fixed` wire decision gate": no gate — both parse, `contract-check` emits two *Advice* findings, `gen` returns `Err(… deferred at wire level (§4.4))`; consistent, undocumented as a gate, no decision record.
- **Stream C** — SSLIOP and the token seam (`orbweaver_mcp::token::Verifier`) are correctly struck. R17: the *safe-failure* half is landed and tested (`Expiry` interceptor, `Denied::CredentialExpired`); *re-establishment* does not exist. "Catalogue marking for targets that cannot enforce": the decision string exists (`Assertion::RecordedOnly`), **no per-peer record in any catalog output**, zero `csiv2` hits in the console. Measurable here today with the two peers.
- **Stream D** — index/cache/union landed (D003-A), the model absent by design; the D003-precleared local model never built. D004 tier 1 landed; tiers 2/3 pre-cleared with triggers, none fired. Console: three pages. `idl-diff --approve` is **a flag and a printed line** — no store, no identity; `forge-pipeline --supersede` already writes `superseded.tsv` and is the shape to copy.
- **Stream E** — every struck item verifies. "Nothing outstanding" is true of the list; the giop remaining column now names eight items, all unmeasurable here (a peer that closes between two writes; JacORB's LE writer; JacORB's own 1.2 U+FEFF writer/reader disagreement; …).
- **§7.4 I3** — the SKIPPED line names the *synonym* class; "injection against a real model" is unmeasured only transitively. **I4** — both halves of the ● sentence are false at HEAD: `Bridge::audit()` exists (`lib.rs:632`), and `gen_corpus.rs:516` says "CAPTURED, not reconstructed"; `promote.rs:39-45` carries the same stale sentence. Residue: `Bridge::stats()` fills only through `Bridge::invoke`, so I4's calls record into a local `CallStats`.

### §8 verification strategy
| Row | Verdict |
|---|---|
| Wire protocol / §11 interop matrix | "one group per cell" false: 3 groups, 4 cells; JacORB's two directions share `jfail` — a green harness cannot tell "both passed" from "one never reached" |
| CDR encoding | accurate, **understated**: names one recording; v0.5.0 has seven `*_from_a_peer.rs` files, three capture scripts, one group driving four |
| IDL syntax | TAO column unmeasured everywhere; `jacorb_idl` required in CI and not named |
| IDL semantics | corpus half gated by `idl-check` (row says `sidl-validate`); the S3-on-pipeline-output clause is unit-tested, no gate runs `forge-pipeline` |
| Dynamic invocation | "over the golden corpus": DII runs over `echo.idl`; the corpus goes through DynAny + AnyJSON (5248 crossings pinned) — both measured, neither named |
| Generated code | "contract tests" contradicts §11's row and A8 |
| End to end | accurate |
| AI quality | no v0.5.0 run; `inputs-v2` never run as a set; the hold-out subset (`:769`, §12 item 6) does not exist |
| Codesets | "EUC-KR against every fixture ORB" false — no peer negotiates it; EUC-KR's oracle is Python's codec |
| AnyJSON, Performance | accurate |

### §9.1 risks
Landed: R0, R1, R2, R4, R5, R10, R14, R16. Partly: R3 (pen-test clause unowned), R12 (verifier empty), R15. **Stale/overstated:** R7 (second-host run measured, PHASE6; container probe still unrun; no foreign ORB has read a rewritten IOR), R8 (v0.1 mitigation the plan overruled), R11 ("quarantined" describes nothing and contradicts PHASE3's design), R13 (first clause indicative for something SKIPPED with an empty verifier — the most misleading cell in the register), R17 (one half landed, one absent, stated as one). Open, non-code: R6, R9. **Missing row — R18:** our union `default:` label bytes are the intersection of two peers' defects (JacORB reads one octet that must be 0; omniORB ignores the value), not of the standard; the recording harness re-checks against those two only. Cheapest oracle: a pinned test that our *reader* accepts a conformant non-zero label.

### §11 / §12
Every metric row's Instrument column is accurate except the per-cell claim; the S2 first-pass number is six days older than the release. A1–A9: no trigger fired. §12: 1–2 done, 3 correctly historical, 4–5 open (non-code), 6 half (frozen yes; hold-out no). PLAN.ko.md parity holds (50/50 headings, every table row count equal).

## 3. PLAN-SERVICES, PLAN-MOE, PLAN-DEFERRED / 서비스·MoE·유예 계획

**PLAN-SERVICES:** §1 "12 of 107" (present tense, 0 absent of 106 today); §2 and §8.1.1 disagree with each other and with the wire on CosNaming's `bind_context`/`rebind_context`/`destroy` (served); §3 still states the `Capability` gap as open (closed by MeasuredCapability, PLAN-MOE §4.5.1 has it); §4 CosEvent "❌ → F7" and "pull is refused loudly" (consumer half served, F7 landed); §10 F7/F3 rows lack the landed marker; §10's "CosEvent → telemetry feedback when F4 and F7 both exist" — **both exist and nothing publishes** control-plane events into the channel; §7 IFR agrees with the wire operation for operation; §5/§6/§9 still true. **`expert_service.rs:28`** says `dispatch` answers `BAD_OPERATION` while the code and wire say `NO_IMPLEMENT` — the §8.1.1 failure with the polarity reversed.

**PLAN-MOE:** §4.5.1 current and correct. §2 rows F4/F5 ◐ are landed (chain, tenancy); IF2 reuse landed (`Bridge::invoke` → `CallStats`) but MoE `route_freq` is the offer store's own counter, unfed — the row conflates the two; §3 F4/F5 have no ✅; three test counts stale (37→40; 20/34→21/84); the D006 blockquote "`gen/src/lib.rs:164` drops the bound" is false since 526b355 and lives in D006 twice more.

**PLAN-DEFERRED:** all eight triggers re-verified **not fired**, and §3's architectural argument (no clock in residency/trading) re-verifies today. §7's trigger was evaluated *in code* by F5 (`tenant_service.rs:135-142`: "this is the other shape") — the one place the tree answered a PLAN-DEFERRED question, cited by neither PLAN-DEFERRED nor D010 §5. The three 2026-08-18 deferrals have no chapter; their reasons are still true (grep: nothing in the tree is a `PullSupplier`; the event servant has no notion of caller; naming still dials nothing, and that is now a test).

**Also stale, same day:** `SERVICES-COVERAGE.md` §9 still says `spike-experts`/`spike-tenants` have no `--hold` (both do; `spikes/svc-hold/` is orphaned) and "twelve of the fifty served" (63); `COMPONENTS.md:40` says CosEvent's pull model is refused `BAD_OPERATION`.

## 4. What this says about how documents drift / 문서가 어긋나는 방식

Every stale row above is a **restated fact** whose home moved on — the class CLAUDE.md's "where a fact lives" names, measured again with a fresh sample. Two structural causes account for almost all of it:

1. **Status written in the indicative inside scope or risk prose.** §7.3 streams are supposed to be scope, but "S1–S3 not started" and R13's mitigation are status. Status has one home (`COMPONENTS.md`, D010 §8's Landed column). Fix: strike/annotate, and stop writing status there.
2. **Numbers typed by hand.** Test counts, "N of M declared", "fifty served". Every one that had a script (`coverage_tables.py`) stayed current; every one that did not went stale within five days. Fix: the same move — a script writes it, or the number is deleted.

The reviewers also found the honest converse: PLAN §11's Instrument column, PLAN-MOE §4.5.1, PLAN-DEFERRED's chapters, PLAN-SERVICES §7 (IFR) and §9 are **all accurate** — the sections that were rewritten against the code this week stayed right; the sections nobody re-read since 2026-08-14 did not.

## 5. Recommended order / 권고 순서

Ordered by what a wrong row costs a reader, then by what a fixture allows.

| # | Batch | Class | Footprint | Oracle |
|---|---|---|---|---|
| 1 | **Restatement sweep** — every stale/overstated row in §1–§3 above (PLAN §7.2/§7.3/§7.4/§8/§9.1 incl. R18, PLAN-SERVICES, PLAN-MOE, D006's blockquote ×3, `SERVICES-COVERAGE` §9, `COMPONENTS.md:40`, `expert_service.rs:28`, `promote.rs:39-45`); three `PLAN-DEFERRED` chapters for the 2026-08-18 deferrals with their triggers; PLAN-DEFERRED §7 cites F5's in-code trigger analysis | D | docs + two comments | `decision_status.py`; heading counts EN/KO; and a new **report** `spikes/plan_numbers.py` (gap_symbols' shape): every hand-typed "N tests"/"N of M declared"/"N served" in `docs/PLAN*.md` vs `cargo test --list` and the sweep totals — printed, not gated, false-positive rate measured first |
| 2 | **Harness honesty** — split the JacORB group into two cells with two counters; the S1–S3 replay line becomes `SKIPPED` naming the model arm (`E2E_MODEL`), like the embeddings arm; `spikes/nat/vm/run.sh` as a `SKIPPED` group naming the VM fixture; the I3 skip line names both classes | A | `spikes/run_checks.sh`, `spikes/end_to_end.sh` | negative controls: stop the JacORB server before the second leg → exactly one group red; unset `E2E_MODEL` → SKIPPED counted |
| 3 | **Per-peer CSIv2 record in the catalogue** — `Assertion::RecordedOnly` writes a durable per-peer capability record the console `catalog` page renders; harness asserts both fixture IORs produce "cannot enforce — the bridge is the only enforcement point" | A | `orbweaver-mcp` identity + `orbweaver-console` | the two existing peers; negative control: a fabricated IOR with a mechanism list produces the other record |
| 4 | **Small measurable closures** — `SIDL_VERSION` beside both `VOCABULARY` copies + a `contract-check` finding for an unknown version; R18's reader test (a conformant non-zero default label is accepted and ignored); `Bridge::stats` filled through `connect_static` so I4's local `CallStats` goes | A | `orbweaver-test`, `orbweaver-forge`, `orbweaver-giop` tests, `orbweaver-mcp` | each red-then-green; the existing mirror test `annotate.rs:1601`; `gen_corpus.rs:492-495` deleted |
| 5 | **`idl-diff --approve` store** — copy `record_supersede`'s shape: append `(released, proposed, findings, reason, approver)` to a file the console `diff` page renders; a re-run with the same approval is byte-identical | A | `orbweaver-registry` bin, `orbweaver-console` | harness replays one approval; needs one decision: who the approver is (no identity in `idl_diff.rs` today) |
| 6 | **`valuetype`/`fixed` decision gate** — a `sidl/deferred-wire-type` rule at S4 agreeing with `gen`'s existing `Err(… deferred at wire level (§4.4))`; a corpus file each | A | `orbweaver-idl` sema, corpus | negative control: the corpus files go red at S4; `contract-check` promotes the two Advice findings to a counted class |
| 7 | **Event channel ← telemetry** (PLAN-SERVICES §10: F4 + F7 both exist, nothing publishes) | A, feature | `orbweaver-mcp` telemetry → `orbweaver-giop` event_server | omniORB's `PushConsumer` (already driven by `spikes/event_consumer.py`) receives a real span record; needs a small design note first (what is published, what is not — the trust boundary §5 already draws) |
| — | **A v0.5.0 pipeline run over `inputs` and `inputs-v2`** (PLAN §8 AI quality "every release") and a hold-out subset (§12 item 6, A7) | B until a key | `docs/pipeline-runs/` | `forge-pipeline`'s per-stage lines vs 2026-08-13's; SKIPPED until `E2E_MODEL`/a key is present — the honest state batch 2 makes explicit |
| — | A1–A9, B1–B4, B6, PLAN-DEFERRED §1–§8 | needs a party / a trigger | — | unchanged from D010 §4–§5 |

Batches 1–2 are the coordinator's (docs and `run_checks.sh`). Batches 3–6 have disjoint footprints and can run as one wave; 7 needs its design note before its batch.

배치 1–2는 코디네이터 몫(문서와 `run_checks.sh`), 3–6은 풋프린트가 갈리므로 한
웨이브로 병행, 7은 설계 노트가 먼저다. 나머지는 D010 §4–§5 그대로 — 픽스처나
외부 당사자를 기다린다.
