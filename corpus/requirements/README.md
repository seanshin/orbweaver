# Requirement benchmark v0 — Phase 0 assumption B

20 natural-language requirements used to measure whether a model can produce
IDL that compiles. Written in Korean because that is the language the target
users write requirements in, and translation is not a step we want to hide.

**Method.** Requirements are fixed *before* any IDL is generated. Generation is
one pass with no compiler feedback, then every file goes through the omniidl
oracle. First-pass rate is the headline number; the self-repair loop is then
allowed up to three rounds.

**Method, amended 2026-08-13 — there is no longer one first-pass rate.** The
pipeline now runs the stages PLAN §5 names, each with its own producer and its
own gate, so this benchmark produces **one first-pass rate and one round count
per stage**: S1 ingest (requirement → brief), S2 synthesize (brief → IDL), S3
annotate (IDL → SIDL), S4 the gate. Quoting a single number for the run hides
which stage was wrong, which is the whole reason the stages are separate — the
first split run measured 90% / 95% / 100% / 100%, and the same underlying rule
(case-insensitive identifier clashes) fired at *two different stages* with two
different fixes. Requirements are still fixed before anything is generated and
the oracle is still not consulted mid-pass. Runs are recorded under
`docs/pipeline-runs/`.

**Known limitation.** In this Phase 0 run the generator and the evaluator are
the same model, so the number is indicative, not a clean benchmark. PLAN §8
requires a frozen benchmark with a hold-out subset and an independent harness
before this figure is used to gate anything. That limitation is unchanged by
the stage split; what changed is that `omniidl` and `contract-check` (a peer
crate's rules, not the pipeline's) now cross-check every batch.

| # | 요구사항 |
|---|---|
| R01 | 사용자 계정을 생성·조회·삭제하고, 이메일로도 조회할 수 있어야 한다. 계정이 없으면 예외를 던진다. |
| R02 | 은행 계좌 간 이체. 잔액 부족과 동결 계좌를 각각 구분해 예외로 알린다. 금액 단위는 원. |
| R03 | 함정 전투체계의 표적 관리. 표적은 식별번호·분류·위경도·침로·속력을 가지며 목록 조회와 개별 조회를 지원한다. |
| R04 | 센서 원격 측정. 온도·습도·기압을 주기적으로 읽고, 임계값 초과 시 경보 등급(정보/경고/위험)을 반환한다. |
| R05 | 파일 저장소. 바이트 배열을 업로드·다운로드하고 크기와 체크섬을 조회한다. 대용량이므로 청크 단위로 나눈다. |
| R06 | 인쇄 작업 큐. 작업을 등록하면 작업 ID를 돌려주고, 상태(대기/진행/완료/실패)를 조회할 수 있다. 취소도 가능하다. |
| R07 | 항공 관제 비행계획. 편명·출발지·목적지·출발시각·경유지 목록을 등록하고 수정한다. |
| R08 | 재고 관리. 품목별 수량을 증감하고, 여러 품목을 한 번에 조회한다. 수량이 음수가 되면 거부한다. |
| R09 | 채팅방. 방을 만들고 참가자를 추가·제거하며, 메시지를 보내고 최근 N개를 조회한다. |
| R10 | 설정 저장소. 키-값 쌍을 문자열로 저장하되 값의 타입이 정수·실수·불리언·문자열 중 하나임을 표현해야 한다. |
| R11 | 지도 경로 탐색. 출발·도착 좌표를 받아 경유 좌표 목록과 총 거리(미터), 예상 소요시간(초)을 반환한다. |
| R12 | 사용자 인증. 아이디와 비밀번호로 토큰을 발급하고, 토큰 검증과 폐기를 지원한다. 실패 사유를 구분한다. |
| R13 | 로그 수집. 발생시각·심각도·출처·메시지를 가진 로그를 다건 전송한다. 응답이 필요 없는 단방향 호출로 한다. |
| R14 | 주문 처리. 주문은 여러 주문항목을 가지며 항목마다 상품코드·수량·단가가 있다. 총액을 계산해 반환한다. |
| R15 | 장비 상태 모니터링. 장비 목록과 각 장비의 가동/정지/점검 상태, 마지막 갱신 시각을 조회한다. |
| R16 | 이벤트 구독. 클라이언트가 콜백 객체를 등록하면 서버가 이벤트 발생 시 그 객체를 호출한다. 구독 해지도 지원한다. |
| R17 | 통계 집계. 숫자 배열을 받아 최소·최대·평균·표준편차를 한 번에 반환한다. |
| R18 | 다국어 메시지. 메시지 키와 로케일을 받아 번역문을 반환한다. 한국어 텍스트를 다뤄야 한다. |
| R19 | 배치 작업 스케줄. 크론 표현식으로 작업을 등록하고, 다음 실행 예정 시각 목록을 조회한다. |
| R20 | 문서 버전 관리. 문서를 저장할 때마다 버전이 올라가고, 특정 버전을 조회하거나 두 버전의 차이를 요청할 수 있다. |

---

## v2 — `inputs-v2/`, added 2026-08-14

**v1 (`inputs/`) stays frozen. It is not edited here or anywhere else.** The
twenty above are the denominator of every assumption-B number this project has
reported, and adding to them would change a figure earlier records quote while
leaving those records unchanged.

**Why a second version at all.** The 2026-08-14 stated-scope run
(`docs/pipeline-runs/2026-08-14-stated-scope-binding.md`) measured D005 option
C's false-positive rate at **0/20 over `inputs/`** and said plainly what that
number is not: *none of the twenty states a permission in any form the rule can
recognise, so the rule cannot fire on the benchmark at all.* A zero over a set
that structurally cannot produce a one measures absence, not precision. v2 is
the repair.

**Shape, following `corpus/queries/`.** `search-v1.tsv` stayed frozen and
`search-v2.tsv` was added beside it holding every v1 line verbatim plus the new
cases, so a single v2 run reproduces the v1 numbers and extends them.
`inputs-v2/` is the same: `R01`–`R20` are byte-for-byte copies (a test asserts
it — `crates/orbweaver-forge/tests/stated_scopes.rs`), plus six new
requirements. Both sets are run and both are reported; quoting v2's rate without
v1's, or the reverse, hides which half moved.

**How the six were written.** In the same register as the twenty — one domain,
its operations, its exceptional cases — each naming the permission the way a
requirement in that domain would name it, in one pass before the scanner was run
over any of them. They are not phrased to match a regular expression, which is
the point: **three of the six state a permission the rule cannot see**, and that
is the finding, not a defect in the set.

| # | 요구사항 | 권한 표기 | 규칙 발화 |
|---|---|---|---|
| R21 | 병원 처방 관리. 환자별 처방 목록을 조회하고 새 처방을 등록하며, 등록된 처방을 취소한다. 처방 등록과 취소는 pharmacy.prescription.write 권한을 가진 의사만 수행할 수 있다. 존재하지 않는 환자와 이미 조제가 끝난 처방은 각각 구분해 예외로 알린다. | `pharmacy.prescription.write` | O |
| R22 | 배전 계통 원격 조작. 변전소별 차단기 상태와 마지막 조작 시각을 조회하고, 차단기 번호를 지정해 개방하거나 투입한다. 개방과 투입에는 grid:breaker_control 권한이 필요하며, 점검 중인 차단기는 조작을 거부하고 예외로 알린다. | `grid:breaker_control` | O |
| R23 | 결제 환불. 주문번호로 결제 내역을 조회하고 전액 또는 부분 환불을 요청한다. 환불 승인은 billing.refund.approve 권한을 가진 정산 담당자만 할 수 있고 금액 단위는 원이다. 이미 환불된 결제와 환불 가능 기간이 지난 결제는 각각 구분해 예외로 알린다. | `billing.refund.approve` | O |
| R24 | 영상 반출. 카메라 번호와 시간 구간으로 녹화 영상을 조회하고 반출 파일을 만든다. 반출은 보안 책임자 권한이 있는 사용자만 요청할 수 있으며 반출 이력은 모두 남긴다. 보존 기간이 지난 영상은 예외로 알린다. | 산문 — *보안 책임자 권한* | X |
| R25 | 학사 성적 정정. 학번과 과목코드로 성적을 조회하고 정정 사유를 붙여 성적을 수정한다. 수정은 ROLE_REGISTRAR 권한을 가진 학사 담당자만 가능하다. 정정 기간이 아닌 경우와 존재하지 않는 수강 내역은 각각 구분해 예외로 알린다. | `ROLE_REGISTRAR` | X |
| R26 | 창고 로봇 관제. 로봇별 위치와 배터리 잔량을 조회하고, 로봇 번호를 지정해 비상정지를 건다. 비상정지 호출에는 warehouse/robot/estop 권한이 필요하고, 이미 정지한 로봇은 예외로 알린다. | `warehouse/robot/estop` | X |

**The finding.** `ingest::scope_shaped` recognises one lexical convention —
two or more lower-case ASCII segments joined by `:` or `.`. Korean prose
(`R24`), the upper-case `ROLE_` convention every Spring codebase uses (`R25`)
and slash-separated ACL paths (`R26`) are all ordinary ways to state a
permission and all invisible to it. A project whose house style is any of those
three gets **no binding at all** from D005 option C, and gets it silently. That
is recorded rather than patched: widening the predicate to accept
`ROLE_REGISTRAR` would accept every upper-case constant in every requirement,
and the resulting false demands would be overridden by hand until the rule was
routed around.

**v2 — 요약.** v1은 얼어 있고 v2가 그 옆에 선다. v2는 v1 스무 건을 **바이트 단위로
그대로** 포함하고(테스트가 강제한다) 권한을 명시하는 여섯 건을 더한다. 여섯 건 중
셋만 규칙이 인식하며, 인식하지 못하는 셋(산문·대문자 `ROLE_`·슬래시 표기)이 더
중요한 결과다 — 규칙이 덮는 것은 **표기 관습 하나**뿐이고, 다른 관습을 쓰는
프로젝트는 아무 보호도 받지 못한 채 침묵한다. 예측자를 넓히는 것은 오탐을 부르므로
고치지 않고 측정으로 기록한다.
