# D003 — Embeddings and catalog storage: which dependencies, if any

**STATUS: PROPOSED** — drafted 2026-08-13. Nothing below is adopted until a
human approves. Two coupled questions, one document, because they gate the same
two service rows in `docs/COMPONENTS.md` and would share a store if both land.
**상태: 제안** — 2026-08-13 작성. 사람이 승인하기 전에는 아무것도 채택되지
않는다. 두 질문이 `docs/COMPONENTS.md`의 같은 두 서비스 행을 막고 있어 한
문서로 다룬다.

**Verified 2026-08-13** against shipped artifacts where feasible: crate
tarballs from `static.crates.io` (tokio-postgres 0.7.18, sqlx 0.9.0, pgvector
0.4.2, fastembed 5.17.4), crates.io registry metadata, the pgvector
extension's `LICENSE` from its repository, Anthropic's published embeddings
documentation, and the `claude` CLI installed on this machine. Anything not
verified that way is marked **unverified**, in the D001/D002 tradition.

## The questions / 문제

**A.** `docs/COMPONENTS.md` lists embeddings/semantic search as ❌ blocked on
this document. The oracle already exists and is frozen:
`corpus/queries/search-v1.tsv` measured the lexical baseline on 2026-08-13 as
**exact 18/18, synonym 0/10, negative 6/6, injection 5/5**. The synonym 0/10
is the recorded headroom an embedding index exists to close; the other three
classes are gates that must stay at 100%. What may an MIT-only project depend
on to close that headroom?

**B.** `PLAN.md` §6 names **PostgreSQL + pgvector** for catalog storage.
Today the seams are the in-process Registry and `exposure.todo.tsv` (the I2
proof runs against them). What may we depend on for durability, and — the
prior question — does anything demand durability yet?

**A.** 임베딩/시맨틱 검색은 이 문서에 막혀 있다. 오라클은 이미 동결되어 있다:
2026-08-13 측정 기준선 exact 18/18, **synonym 0/10**, negative 6/6, injection
5/5. synonym 0/10이 임베딩이 닫아야 할 여유폭이고 나머지 세 클래스는 100%를
유지해야 하는 게이트다. **B.** PLAN §6은 PostgreSQL + pgvector를 지명한다.
오늘의 이음매는 인프로세스 Registry와 `exposure.todo.tsv`다. 영속성을 위해
무엇에 의존해도 되는가 — 그보다 먼저, 지금 영속성을 요구하는 것이 있는가?

## The distinction that matters / 핵심 구분

The licensing boundary now has three categories, and this decision exercises
all three:

1. **Logic from a published specification** — we write it (the ORB core).
2. **Data we cannot originate** (D001) — depended on with a verified,
   disclosed licence chain (`encoding_rs`). **Model weights are exactly this
   category.** There is no specification to train a model from; a weights file
   is somebody's compilation, produced from training data we cannot audit.
   D001's sharpest lesson applies to weights with full force: an Apache-2.0
   declaration over the *parameters* does not account for the provenance of
   the *training data* underneath them, and that unaccounted layer is the same
   position as "a crate declaring MIT over a table it does not account for" —
   the position D001 called worse than an honestly disclosed obligation.
3. **Logic whose failure modes our oracles cannot detect** (D002) — crypto is
   depended on because a wrong TLS stack interops perfectly.

Embeddings do **not** fall under category 3, and that is worth stating
plainly: unlike crypto, embedding failures are visible to a deterministic
oracle we already own. A bad embedding model scores badly on the frozen
benchmark; a leaky one smuggles an injection match past the gate and the
harness goes red. So nothing here is unverifiable-by-construction. What rules
out first-party embeddings is category 2 — we cannot originate weights — plus
the plain fact that the oracle would measure a hand-rolled model failing.

There is also a fourth, older category this decision leans on: **a separate
process whose output we read** — omniORB as a wire peer, `omniidl` as an
oracle, the `claude` CLI as `gen_claude.sh`'s generator. A model behind an
HTTP API, and a PostgreSQL server behind a socket, are both this category.
Neither enters `cargo tree`; neither is linked, vendored, or redistributed.

임베딩은 D002의 범주(오라클이 실패를 못 보는 로직)에 **속하지 않는다** — 암호와
달리 임베딩 실패는 우리가 이미 소유한 결정적 오라클(동결 벤치마크)에 보인다.
자체 제작을 배제하는 것은 D001의 범주다: **가중치는 우리가 원저작할 수 없는
데이터**이며, 파라미터에 대한 Apache-2.0 선언은 그 밑의 학습 데이터 출처를
설명하지 않는다. 그리고 네 번째 범주 — 출력만 읽는 별도 프로세스(omniORB,
omniidl, `claude` CLI) — 가 있다. HTTP API 뒤의 모델과 소켓 뒤의 PostgreSQL
서버는 둘 다 이 범주로, `cargo tree`에 들어오지 않는다.

---

## Part A — Embeddings for semantic search / 임베딩

### What the survey actually found / 조사 결과

**A model-API endpoint from Anthropic does not exist.** Verified against the
published documentation (`platform.claude.com/docs/en/build-with-claude/
embeddings`, fetched 2026-08-13): *"Anthropic does not offer its own embedding
model"* — the page recommends **Voyage AI** (plain HTTPS endpoint,
`api.voyageai.com/v1/embeddings`, `voyage-4` family). Notably, Voyage's
`voyage-4-nano` is published as **open-weight, Apache-2.0, on Hugging Face** —
the one weights file in this survey whose licence the vendor states outright.

**The `claude` CLI has no `embed` command.** Verified on this machine
(2026-08-13): the subcommand list is `agents, auth, auto-mode, doctor,
gateway, import, install, mcp, plugin, project, setup-token, ultrareview,
update` — no `embed`. The task framing assumed one might exist; it does not,
and this document will not pretend otherwise. What survives is the
**precedent, not the binary**: `spikes/gen_claude.sh` already treats a model
as an external command whose stdout we read. An `embed` wrapper in the same
mold — a POSIX-sh script that POSTs to an embeddings HTTP API with `curl` and
prints vectors — keeps the entire model dependency out of Cargo, exactly as
`gen_claude.sh` keeps the generator out.

| Option | Licence / provenance findings | Verdict |
|---|---|---|
| **1. API embeddings via external command** (`spikes/embed_voyage.sh` or equivalent; the gen_claude.sh pattern) | **Zero crates.** `cargo tree` unchanged. Vectors returned over HTTPS are outputs, not incorporated code or data tables; no licence text enters the repository. The vendor relationship is contractual (API terms), not copyright — **terms of service unverified**, noted below | **Recommended** — argued below |
| **2. Local ONNX inference** — `fastembed` 5.17.4 (declared **Apache-2.0**; shipped tarball carries the Apache-2.0 `LICENSE`, verified) pulling `ort` (declared `MIT OR Apache-2.0`; **no stable release** — latest is 2.0.0-rc.13) and `hf-hub` for runtime weight download | Crate licences are acceptable on their face. Three provenance layers below them are not equally accounted for: (a) `ort`'s default feature `ort-download-binaries` fetches **prebuilt ONNX Runtime binaries at build time** — a supply-chain artifact we neither build nor audit; (b) `hf-hub` downloads **model weights at run time**, so the actual data dependency never appears in any manifest; (c) the weights' training-data provenance is unauditable in principle. D001's clause could accept an Apache-2.0 weights file (voyage-4-nano, or the commonly used all-MiniLM family — **weights licences unverified**, we did not fetch the model cards) with disclosure in `NOTICE` — but the honest disclosure would have to say "licence declared by the publisher over data nobody can account for" | Acceptable under D001 *with eyes open*; not the recommendation. Adds a C/C++ runtime, an rc-version crate, and a build-time binary download for a corpus that fits in one API call |
| **3. No embeddings — keep lexical** | Nothing to verify | The 0/10 is real headroom, and it is the class agents actually produce (a caller asks "money transfer" and the catalog says `Wallet`). But the deciding fact is that option 1 costs **no new dependency class at all** — the usual argument for deferral ("does the headroom justify a dependency class?") has nothing to push against. Deferral here would be declining a measured improvement that costs one fixture script |

**후보 요약:** (1) 외부 명령으로 임베딩 API 호출 — 크레이트 0개, `cargo tree`
불변, gen_claude.sh 선례 그대로 (권고); (2) 로컬 ONNX 추론 — 크레이트
라이선스는 문제없으나 빌드 시 바이너리 다운로드, 런타임 가중치 다운로드,
학습 데이터 출처라는 세 겹의 미설명 층이 있다; (3) 임베딩 없음 — 여유폭
0/10은 실재하지만, 후보 1이 의존성 부류를 전혀 추가하지 않으므로 유예론이
설 자리가 없다.

### How success is measured / 성공의 측정

The oracle predates the feature, as it should. One batch, then:

- **synonym**: strictly above the recorded 0/10 baseline — this is the number
  the embedding index exists to move, reported as first-pass rate.
- **exact 18/18, negative 6/6, injection 5/5 — gates, 100% or the batch
  fails.** The injection gate is the load-bearing one (§7.4 I3, R11):
  annotation text must cross into the index as data, instruction text
  embedding a real catalog word must not smuggle a match, and the result
  document must re-parse as valid JSON.
- Frozen `search-v1.tsv` is never edited to pass; new and corrected cases go
  to `search-v2.tsv`, and cases rotate so search is never tuned to its own
  exam (§8 discipline, already written into the file's header).
- **Absence handling per the harness rules:** when the embed command or its
  key is missing, the harness reports the embedding group as *unmeasured* —
  counted, named, never green. Lexical results must be identical with the
  feature absent (the seam: embeddings re-rank or extend lexical, never
  replace the exact-match path, so the exact gate cannot regress by
  construction).
- Vectors are cached in a versioned file next to the catalog (the
  `exposure.todo.tsv` precedent), keyed by content hash, so CI replays do not
  re-bill the API and the first-pass run is reproducible.

성공 기준: synonym이 0/10을 상회(1차 통과율로 보고), 나머지 세 클래스는 100%
게이트. 동결된 v1은 절대 수정하지 않고 신규·수정 케이스는 v2로. 명령이나 키가
없으면 해당 그룹은 *미측정*으로 보고하며(초록으로 칠하지 않는다), 임베딩은
정확 일치 경로를 대체하지 않고 얹기만 하므로 exact 게이트는 구조적으로
후퇴할 수 없다. 벡터는 내용 해시로 키한 버전 관리 파일에 캐시한다.

---

## Part B — Durable catalog storage / 영속 카탈로그 저장소

### What the survey actually found / 조사 결과

The licence layer is unusually clean — verified from the licence files shipped
inside the actual crate tarballs (`static.crates.io`, 2026-08-13):

| Component | Declared (crates.io, verified) | What ships (verified) |
|---|---|---|
| **tokio-postgres 0.7.18** (rust-postgres) | `MIT OR Apache-2.0` | Tarball ships `LICENSE-MIT` (Steven Fackler) and `LICENSE-APACHE`; genuine dual licence, we may take MIT |
| **pgvector 0.4.2** (Rust client crate) | `MIT OR Apache-2.0` | Tarball ships both files (Andrew Kane); genuine dual licence |
| **sqlx 0.9.0** | `MIT OR Apache-2.0` | Tarball ships both files; genuine dual licence. Heavier (macros, compile-time checking, its own pool); more surface than the task needs |
| **PostgreSQL server + pgvector extension** | — | Both under the **PostgreSQL License** (permissive, BSD-like; the extension's `LICENSE` verified from its repository). Moot for `cargo tree` regardless: the server is a **separate-process fixture**, the same category as omniORB — spoken to over a socket, never linked, never redistributed |

Unlike D001 (a data table hiding under an MIT label) and D002 (an OpenSSL
lineage under the provider), this survey found no buried layer at the top:
the wire protocol client is first-order permissive code. What it did **not**
verify is the transitive tree — noted below.

라이선스 층은 이례적으로 깨끗하다. tokio-postgres·pgvector·sqlx 모두 실제
타르볼에 MIT/Apache 이중 라이선스 파일이 들어 있음을 확인했고, PostgreSQL
서버와 pgvector 확장은 PostgreSQL License(관대)이며 어차피 omniORB와 같은
**별도 프로세스 픽스처** 범주라 `cargo tree`와 무관하다. D001·D002와 달리
최상층에 묻힌 층은 없었다 — 단, 추이적 트리는 검증하지 않았다(아래).

### The operational half — what the fixture batch must probe first

The licence question was never the hard half. This repo's harness philosophy
says a dependency may not precede the oracle that measures it, and the sslTP
precedent (`spikes/tls/PEER-STATUS.md`) says exactly how packaged fixtures
fail: brew's omniORB shipped the C++ half of SSL and silently omitted the
python half. **Assume the same class of gap here until measured.** Before any
storage code lands, a fixture batch must probe, in this order, with every
result recorded pass/fail/unmeasured:

1. **Does the local fixture exist at all?** Homebrew packages `postgresql@N`
   and `pgvector` as *separate formulae*, and the extension is compiled
   against one specific server version — a mismatch is precisely an
   sslTP-shaped failure (server present, extension half missing or built for
   the wrong keg). Probe: `CREATE EXTENSION vector;` succeeds and
   `SELECT '[1,2,3]'::vector;` round-trips. **Unmeasured today** — this
   document deliberately did not run it, because measuring it belongs to the
   fixture batch, not the decision document.
2. **CI fixture**: a dockerized postgres with pgvector baked in
   (`pgvector/pgvector` image or built in CI), pulled or built inside CI and
   **never published** — the existing CI-image rule covers it verbatim.
3. **Readiness is waited for, not assumed**: a sleeping, deadline-bounded
   loop on actual connectability (the "wait loops must sleep" rule; a
   completed TCP connect does not mean the server can accept — measured on
   this platform in stream E).
4. **If the fixture will not start, the failure counter increments.** A
   storage harness that greens an unstarted database is worse than none.
5. **Both paths tested**: with the store present, and with it absent — the
   Registry + files seam must remain the working default, same shape as
   `--no-default-features` proving the EUC-KR promise.

운영의 절반이 어려운 절반이다. sslTP 선례가 보여준 실패 방식 — 패키지가 반쪽만
싣고 온다 — 을 그대로 가정하고 측정한다: brew의 postgresql과 pgvector는 별개
formula이고 확장은 특정 서버 버전에 대해 컴파일되므로, `CREATE EXTENSION
vector`가 성공하는지부터 조사한다(**오늘은 미측정** — 측정은 픽스처 배치의
몫이다). CI는 pgvector 포함 postgres 컨테이너를 CI 안에서만 빌드/풀하고 절대
발행하지 않는다. 준비 대기는 잠자는 기한부 루프로, 픽스처가 안 뜨면 실패
카운터를 올리고, 저장소가 있는 구성과 없는 구성을 둘 다 시험한다.

### The alternative, weighed honestly / 대안의 정직한 검토

**Keep the in-process Registry + files seam until a pilot demands
durability.** The case for it is strong: the corpus is ~30 interfaces; the I2
proof already runs against `exposure.todo.tsv` with every row `exposed=no`;
`pipeline::register` was built as the durable store's seam on purpose; and
Part A above needs only a versioned vector cache file, which the files seam
provides. Nothing measured today is blocked by the absence of a database.
Adopting one now would violate the project's own sequencing rule — the
dependency would precede any oracle that needs it.

The case against deferral is thinner: PLAN §6 names the technology, and
pre-clearing the licence costs one survey (this one). Both are satisfied by
deferring the *adoption* while recording the *clearance*.

**파일럿이 영속성을 요구할 때까지 인프로세스 Registry + 파일 이음매를
유지한다.** 코퍼스는 인터페이스 ~30개 규모이고, I2 증명은 이미
`exposure.todo.tsv` 위에서 돌며, `pipeline::register`는 애초에 영속 저장소의
이음매로 설계됐다. 오늘 측정되는 어떤 것도 데이터베이스 부재에 막혀 있지
않다. 지금 채택하면 "의존성이 오라클을 앞서지 않는다"는 이 프로젝트의 순서
규칙을 스스로 어기게 된다. 반대 논거는 얇다: PLAN §6의 지명과 라이선스
사전 정리 — 둘 다 채택은 미루고 정리만 기록하는 것으로 충족된다.

---

## Recommendation / 권고

**A — Adopt API embeddings behind an external command, zero new crates.**
An `embed` wrapper script in `spikes/` (the `gen_claude.sh` pattern: POSIX sh,
capture-then-process, non-zero exit means the call itself failed) POSTs to an
embeddings HTTP API — Voyage AI is the documented default given Anthropic
ships no endpoint — and the Rust side reads vectors from the versioned cache
file. `cargo tree` is unchanged; the model is a separate process/service whose
output we read, the same boundary category as `omniidl`. Success is the frozen
benchmark: synonym above 0/10, the three gates at 100%, absence reported as
unmeasured. Local ONNX inference (option 2) is licence-acceptable under D001
but is not needed to close a 10-query headroom, and its three unaccounted
provenance layers (build-time binaries, runtime weight downloads, training
data) make it the *worse-disclosed* choice today. Revisit it only if an
offline/air-gapped requirement appears — and if that day comes, prefer an
explicitly-licensed weights file (e.g. the Apache-2.0 voyage-4-nano), vendored
by hash, disclosed in `NOTICE`, with weights treated as D001 data.

**B — Defer the durable store; record the cleared path.** Keep Registry +
files as the working store. When a pilot demands durability, the pre-cleared
stack is **PostgreSQL + pgvector as a separate-process fixture** with
**`tokio-postgres` + the `pgvector` crate** (both verified `MIT OR
Apache-2.0` from shipped files, take MIT) as the only Cargo additions —
`sqlx` is licence-equivalent but heavier than the need. Adoption then requires
only the fixture batch in Part B (probe extension presence first — the sslTP
lesson), not a new licence survey.

**A — 외부 명령 뒤의 API 임베딩을 채택한다. 새 크레이트 0개.** `spikes/`의
embed 래퍼 스크립트(gen_claude.sh 패턴)가 임베딩 HTTP API에 POST하고 — 
Anthropic은 임베딩 엔드포인트가 없음을 확인했으므로 문서화된 기본값은 Voyage
AI — Rust 쪽은 버전 관리된 캐시 파일에서 벡터를 읽는다. `cargo tree` 불변.
성공은 동결 벤치마크로 측정한다: synonym > 0/10, 세 게이트 100%, 부재는
미측정으로 보고. 로컬 ONNX(후보 2)는 D001상 수용 가능하지만 10개 쿼리
여유폭을 닫는 데 필요 없고, 설명되지 않은 세 겹의 출처 층 때문에 오늘은 더
나쁜 공개 위치다. **B — 영속 저장소는 유예하고, 정리된 경로만 기록한다.**
파일럿이 영속성을 요구하면 사전 정리된 스택은 별도 프로세스 픽스처로서의
PostgreSQL + pgvector, Cargo 추가는 `tokio-postgres` + `pgvector` 크레이트
(둘 다 출하 파일에서 MIT OR Apache-2.0 확인, MIT 선택)뿐이다. 그때 필요한
것은 새 라이선스 조사가 아니라 Part B의 픽스처 배치다.

## What was verified, and what was not / 검증된 것과 아닌 것

Verified directly (2026-08-13): Anthropic's embeddings documentation stating
no first-party embedding model exists and recommending Voyage AI; the
installed `claude` CLI's full subcommand list (no `embed`); crates.io
declared licences and the licence files inside the shipped tarballs of
tokio-postgres 0.7.18, pgvector 0.4.2, sqlx 0.9.0, and fastembed 5.17.4;
`ort`'s declared licence and its lack of a stable release (registry metadata);
fastembed's `ort`/`hf-hub`/binary-download feature wiring (its shipped
`Cargo.toml`); the pgvector extension's PostgreSQL License (repository
`LICENSE`); the frozen baseline numbers (read from `search-v1.tsv`'s header,
not re-run).

**Unverified, stated plainly:** the **transitive dependency trees** of
tokio-postgres/pgvector/sqlx (only the top crates' shipped files were read; a
`cargo tree` + licence sweep is part of the adoption batch, not this survey);
**Voyage AI's terms of service** (the vectors question is contractual, not
copyright — someone must read the terms before the wrapper ships; priced
per-token, cost unexamined); **candle** (not fetched — ruled out with option 2
before a licence check was warranted); the **ONNX Runtime binaries** `ort`
downloads and their build provenance; **any model weights file** (no model
card was fetched; voyage-4-nano's Apache-2.0 is the vendor's statement via
Anthropic's docs, not a file we read); **training-data provenance of any
weights** (unverifiable in principle — that is an argument in the survey, not
an oversight); whether the **brew pgvector formula matches the brew
postgresql keg** on the development machine (deliberately left to the fixture
batch — measuring it here would be doing the batch's work without its
harness); and the local `claude` CLI finding is **this machine, this version,
today** — an `embed` subcommand appearing later would not change the
recommendation, only simplify the wrapper.

**검증 안 된 것도 그대로 적는다:** tokio-postgres·pgvector·sqlx의 **추이적
의존성 트리**(최상위 크레이트의 출하 파일만 읽었다 — `cargo tree` 라이선스
전수 조사는 채택 배치의 몫), **Voyage AI 약관**(벡터 문제는 저작권이 아니라
계약이다 — 래퍼 출하 전에 누군가 약관을 읽어야 하며, 토큰당 과금 비용도
미검토), **candle**(라이선스 검토 전에 후보 2와 함께 배제), `ort`가 받는
**ONNX Runtime 바이너리**의 빌드 출처, **모든 가중치 파일**(모델 카드를
가져오지 않았다 — voyage-4-nano의 Apache-2.0은 우리가 읽은 파일이 아니라
공급자의 진술), **가중치의 학습 데이터 출처**(원리적으로 검증 불가 — 누락이
아니라 조사의 논거), 개발 머신에서 **brew pgvector formula와 postgresql keg의
버전 일치**(일부러 픽스처 배치에 남겼다), 그리고 `claude` CLI 조사는 **이
머신, 이 버전, 오늘**의 결과다.

## What is NOT decided by this / 이 문서가 결정하지 않는 것

Nothing is adopted today; `cargo tree` is unchanged by this document, and it
stays unchanged even if A is approved — that is the point of the
external-command shape. The embed wrapper's exact vendor and script name, the
vector cache file format, the search-v2 case set, the storage schema, and the
CI image build for postgres are all later batches with their own oracles.
OpenTelemetry (the third blocked row) is D004, not smuggled in here. This
document answers only the two questions that had to be answered before any of
those batches could start: *which dependency shapes may we even consider, and
does anything demand the heavier one yet.*

오늘 채택되는 것은 없다. 이 문서로 `cargo tree`는 변하지 않으며, A가 승인돼도
변하지 않는다 — 그것이 외부 명령 형태의 요점이다. 래퍼의 공급자와 스크립트
이름, 벡터 캐시 파일 형식, search-v2 케이스, 저장 스키마, postgres CI 이미지
빌드는 각자의 오라클을 가진 이후 배치들의 몫이다. OpenTelemetry(세 번째 막힌
행)는 D004이며 여기 끼워 넣지 않는다. 이 문서는 그 전에 답해야 했던 두 가지
— *어떤 의존성 형태를 고려해도 되는가, 그리고 더 무거운 쪽을 지금 요구하는
것이 있는가* — 에만 답한다.
