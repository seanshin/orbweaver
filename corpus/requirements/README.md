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
