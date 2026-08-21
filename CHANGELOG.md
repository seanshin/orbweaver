# Changelog / 변경 이력

Measurements live in [`docs/COMPONENTS.md`](docs/COMPONENTS.md); this file
records what changed and, where it matters, what it changes on the wire.

측정은 `COMPONENTS.md`에, 여기에는 무엇이 바뀌었는지와 — 중요한 경우 — 그것이
와이어에서 무엇을 바꾸는지를 적는다.

---

## Unreleased

### Fixed / 수정

- **The AnyJSON layer names the rule that refused.** §4.4 defers `valuetype`,
  abstract interfaces and `fixed`; three layers refuse an *instance* and only
  two named the section. The AnyJSON layer — **the one a peer-fed document
  actually meets** — answered `tk_value cannot cross yet` and
  `Struct([…]) is not a value of IDL:m/Money:1.0`, and for `fixed` it named the
  type `<anonymous>`. All three now say one sentence, both directions, all
  three kinds: *`fixed<9,2>` is not marshalled by the v1 wire (docs/PLAN.md
  §4.4); the TypeCode describing it reads, the value behind it does not.* The
  tail is D008's distinction said out loud — the description crosses, the
  instance does not — so a reader does not conclude the TypeCode form is
  refused too, and the test asserts the crossing *beside* the refusal so the
  two cannot drift into agreement. The Rust pair is pinned by
  `deferred_sentence_agreement`; the generated Python half is compared for
  **equality** in `python_target`, because Python cannot import a Rust
  constant. No count moved. **Found on the way, not fixed:** `ValueBase`
  marshals as an **object reference** — the valuetype-as-ObjRef defect of
  2026-08-20 surviving in one keyword, named by S4's closure and refused by
  nothing on the wire path.

  **AnyJSON 레이어가 어느 규칙이 거부했는지 말한다.** 세 계층이 인스턴스를
  거부하는데 절을 이름한 곳은 둘뿐이었고, 하필 피어 문서가 실제로 만나는 계층이
  빠져 있었다(`fixed`는 타입 이름조차 `<anonymous>`였다). 이제 세 계층이 세
  종류·양방향 모두 한 문장을 쓴다; 뒷부분은 D008의 구분을 그대로 말한 것이라
  읽는 이가 TypeCode 형태까지 거부되었다고 결론짓지 않는다. **도중 발견(미수정):**
  `ValueBase`가 객체 참조로 마샬링된다 — 어제 닫은 결함이 키워드 하나에 살아남았고,
  S4는 이름을 대는데 와이어 경로의 어떤 층도 거부하지 않는다.


---

## v0.6.0 — 2026-08-20

The release **the plan reviewed against the code** produced. Three reviewers
read every remaining-work, status and risk row of `PLAN.md` §7–§12,
`PLAN-SERVICES`, `PLAN-MOE`, `PLAN-DEFERRED` and D010 §5 against the tree and
found progress wrong in **both directions**: nineteen rows understated what had
landed, six overstated it, four named an instrument that did not exist, and six
hand-typed numbers had gone stale within five days. Every section rewritten
against the code that week was accurate; every section nobody had re-read since
was not. The restatement is one commit; what it *found* is most of this release.

Because the same reading turned up work nobody had asked for, five defects
below were found by a batch sent after something else. **`sidl-validate
--against` had never run the §5.3 comparison over a guarded multi-file
contract** — the ordinary shape of a released one — and exited 1 anyway, so
nothing looked wrong. **A `valuetype` and an abstract interface went on the
wire as object references**, invisible for six phases because
`tk_abstract_interface`'s parameter list is byte-for-byte `tk_objref`'s and
nobody had asked omniORB what it writes. **`--repair-prompt` gave a model the
wrong file's line number.** **A permanent forward moved one handle and not its
clones**, costing a forward per call forever, silently, because §9.6 keeps the
old address valid. And the front end diverged from the oracle in **both**
directions on constants — seven shapes, one production.

Twice the batch that was sent to fix a keyword found a production: a signature
takes `param_type_spec` (ten divergences, eight closed by one function), and a
constant takes `const_type` (seven shapes, one cause). Fixing the keyword each
time would have closed three of ten and two of seven.

Three decisions were written and none adopted: D011 (a control-plane event is
not the D004 record, and the channel has nobody to redact for), D012 (the pool
cannot hear a caller, and nothing outside tests needs it to yet), and D010's
class-A rows all landed. Two counters were split because a trigger had no
instrument, and one of those triggers turned out to be **circular** — CosEvent
cannot report that fan-out was unwanted, because the filters that would know
are what the deferred chapter would add.

이번 릴리즈는 **계획서를 코드에 대조한 검토**가 만들었다. 검토는 진행률이
**양방향으로** 틀렸음을 찾았다 — 19행 과소, 6행 과대, 계측기 이름 오류 4건,
닷새 만에 낡은 손수치 6건. 그리고 그 읽기가 아무도 요청하지 않은 일을 드러냈다:
**가드가 있는 다중 파일 계약에서 `--against`의 §5.3 비교가 한 번도 실행되지
않았고**(종료 코드는 1이라 아무도 몰랐다), **valuetype과 추상 인터페이스가
객체 참조로 와이어에 나갔으며**(여섯 페이즈 동안 보이지 않았다),
**`--repair-prompt`가 모델에게 엉뚱한 파일의 줄을 주었고**, **영속 포워드가 한
핸들만 옮겨** 매 호출마다 포워드 하나를 조용히 치렀다. 키워드를 고치라고 보낸
배치가 두 번 모두 **프로덕션**을 찾았다.

### Decided / 결정

- **D012 — a per-caller version cap on the pooled path** (**PROPOSED**, not
  adopted). The forward-chain batch made `Connection` carry a caller's
  `cap_version` across a forward and a §9.6 restart and reported that
  `Pool`/`Reference` have none: `pool::Key` cannot hear a caller, so an
  uncapped caller's 1.2 mux would be handed to a capped one and its `wstring`
  would go out under the 1.2 codec — an octet count where its contract was a
  character count, which the peer reads as the wrong string rather than
  faulting. Four options; **D is measured and rejected** — the endpoint's
  contribution is already `Version::negotiate(profile)` and already in the key,
  so what remains after subtracting the profile is by construction the
  caller's own limit, and all eleven in-tree cap sites set a constant the
  caller chose. Recommendation: **C — build nothing, record the limit and the
  trigger**, because nothing outside tests and spikes needs a cap on the
  pooled path, while recording now that **A** (the cap enters `pool::Key`) is
  the shape if the trigger fires. Trigger: the first caller outside
  `crates/*/tests/` and `crates/*/src/bin/` that must speak below
  `Version::max_supported()` to a peer it reaches through `Pool`.

  **D012 — 풀 경로의 호출자별 버전 상한** (**제안됨**, 채택 아님). 포워드 체인
  배치가 `Connection`에 상한을 실어 보냈고 `Pool`/`Reference`에는 없다고
  보고했다: `pool::Key`는 호출자를 듣지 못하므로 상한 없는 호출자의 1.2 mux가
  상한 있는 호출자에게 건네지고, 그 `wstring`은 1.2 코덱으로 나간다 — 피어는
  오류를 내지 않고 잘못된 문자열을 읽는다. 네 안 중 **D는 측정으로 기각**(프로파일
  몫은 이미 키에 있으므로 남는 것은 정의상 호출자 자신의 한계). 권고: **C —
  짓지 않고 한계와 방아쇠를 기록**, 다만 방아쇠가 당겨지면 **A**(상한이
  `pool::Key`에 들어감)가 형태임을 지금 적어 둔다.

- **D011 — control-plane events into the channel** (**PROPOSED**, not adopted).
  PLAN-SERVICES §10's "the loop closes when both exist" precondition has been
  met since 2026-08-18 and nothing publishes; the note asks what would be
  published and finds two answers. **A control-plane event is not the D004
  record**: `session` is the documented join key to the audit ledger (group by
  it and you have one caller's whole profile), `caller` attributes a named
  principal to whoever dialled the port, and on the *unresolved* arm `target`
  and `operation` come straight from the caller unvalidated — the code already
  calls them agent-influenced for escaping and nothing called them that for
  publication. What may cross is the resolved repository id, the resolved
  operation, the two non-hypothetical decision tokens and the outcome — and
  an in-process reactor already gets exactly that from the sink. **And the
  channel has nobody to redact for**: redaction is a judgement about an
  audience, the channel cannot tell two subscribers apart, and a flag is
  deployment-wide consent standing in for a per-connection decision.
  Recommendation: publish nothing (A) plus an in-process `TelemetrySink` seam
  (D), with PLAN-DEFERRED §11's trigger — a caller model in the event servant
  — as the un-defer trigger, because subscription and `destroy` are one
  authorization question. Found on the way: `ChannelStats::dropped` sums
  overflow, disconnect-abandon and clean-shutdown drops into one number, so
  PLAN-DEFERRED §1's "a measured drop rate caused by unwanted fan-out" trigger
  has no instrument that can answer it in either direction.

  **D011 — 제어면 이벤트를 채널에 발행할 것인가** (**제안됨**, 채택 아님).
  PLAN-SERVICES §10의 전제(F4·F7 둘 다 존재)는 2026-08-18부터 충족되었고 아무것도
  발행하지 않는다. **제어면 이벤트는 D004 레코드가 아니다**: `session`은 감사
  원장과의 조인 키이고, `caller`는 포트를 다이얼한 자에게 명명된 주체를 귀속시키며,
  *미해결* 갈래의 `target`/`operation`은 호출자가 준 값 그대로다. 건널 수 있는 것은
  해결된 저장소 id·연산명·결정 토큰·결과뿐이고, 그것은 인프로세스 반응자가 이미
  싱크에서 공짜로 받는다. **그리고 채널에는 가릴 대상이 없다** — 가림은 청중에 대한
  판단인데 채널은 두 구독자를 구별하지 못한다. 권고: 발행하지 않음(A) + 인프로세스
  `TelemetrySink` 씸(D), 방아쇠는 PLAN-DEFERRED §11의 것(이벤트 서번트의 호출자
  모델) — 구독과 `destroy`는 하나의 인가 질문이므로. 도중 발견:
  `ChannelStats::dropped`가 오버플로·연결 해제·정상 종료 폐기를 한 숫자로 합산해
  PLAN-DEFERRED §1의 방아쇠에 답할 계측기가 없다.

### Added / 추가

- **S4 names what the v1 wire cannot carry.** `wire/deferred-type` (replacing
  `wire/valuetype`, which named a declaration and stopped): every declaration
  that is or carries a `valuetype`, an abstract interface or a `fixed` —
  through members, typedefs, elements, signatures, `raises` and inheritance —
  reported at its name span with the reach as prose (`the return of operation
  "sum" is "gc21::Amount", which is fixed<9,2>`) and a fix per family. **A
  warning by default, a refusal in `forge-pipeline`'s S4 and under
  `sidl-validate --wire v1`**: golden 20/21 exist to pin that these constructs
  *parse*, so a refusing default would fail the S4 group on IDL both oracles
  accept; a contract a model just wrote for this ORB is a different caller.
  `orbweaver-gen`'s §4.4 skips are held to the same set over golden
  (`tests/deferred_wire_agreement.rs`: 11 of 11 for `fixed`, both targets).
  `contract-check` prints "N deferred-wire declaration(s) (§4.4) of which M
  unmeasured by the property" (golden 19 / 7), both pinned in the harness.
  New `corpus/golden/deferred-reach.idl`. **Found and not fixed, pinned as the
  divergence it is:** both emitters emit a `valuetype` or abstract interface
  as an **object reference** rather than skipping it — the registry records it
  as `TypeCode::ObjRef`, so a peer expecting a value gets a reference; the
  Python emitter writes an empty name into a skipped interface's `tk_objref`
  TypeCode; and the parser accepts a bare `fixed<d,s>` as a parameter or
  return type where omniidl rejects it.

  **S4가 v1 와이어가 나를 수 없는 것을 이름으로 말한다.** `wire/deferred-type`:
  `valuetype`·abstract interface·`fixed`이거나 이를 멤버·typedef·요소·시그니처·
  `raises`·상속으로 품는 모든 선언을, 도달 경로를 문장으로, 계열별 수정과 함께
  보고한다. **기본은 경고, 파이프라인의 S4와 `--wire v1`은 거부** — golden 20/21은
  이 구문이 *파싱된다*는 것을 고정하려 존재하므로 기본값이 거부면 두 오라클이
  받아들이는 IDL에서 S4 그룹이 깨진다. 생성기의 §4.4 skip 집합과 golden 전체에서
  일치함을 테스트로 고정(`fixed` 11/11, 두 타깃). `contract-check` 요약에 §4.4
  개수(19/7), 하네스에 핀. **보고만:** 두 이미터 모두 `valuetype`/abstract
  interface를 **객체 참조로** 내보낸다 — 값을 기다리는 피어에게 참조가 간다;
  Python은 건너뛴 인터페이스의 `tk_objref`에 빈 이름을 쓴다; 파서는 omniidl이
  거부하는 맨 `fixed` 파라미터를 받는다.

- **The catalogue says, per peer, who enforces a caller identity.**
  `orbweaver-console catalog … --ior <file>` carries each reference's CSIv2
  capability record beside its interface — `enforces_identity`,
  `transport_secured`, enforcement point `target` | `bridge only` — read off
  the IOR's `TAG_CSI_SEC_MECH_LIST` / `TAG_SSL_SEC_TRANS` by
  `identity::PeerCapability`, the same classification `Assertion::RecordedOnly`
  and the audit line now derive from. Measured on both fixtures: omniORB 4.3.4
  and JacORB 3.9 read `enforced-by=bridge only, cleartext`; negative control:
  a fabricated identity-asserting IOR reads `enforced-by=target` in both byte
  orders (`tests/peer_record.rs`). An interface nobody handed a reference for
  says *unmeasured here*, not "bridge only". `Bridge::peer_capability(handle)`
  gives the record without the IOR leaving the table.
- **`Bridge::stats()` fills through `connect_static`.** `CallStats` is one
  store with a `CallPath` column; a guard's calls land in the issuing session's
  counters under `static` the moment they complete, `recommend` reads the
  dynamic column only (a promoted path is never re-recommended), and the
  gen-corpus I4 oracle's local `CallStats` is gone. `promote.rs`'s stale
  "still reconstructs" sentence corrected.

  **카탈로그가 피어별로 누가 호출자 신원을 강제하는지 말한다.**
  `orbweaver-console catalog … --ior <file>`이 각 참조의 CSIv2 능력 레코드를
  인터페이스 옆에 싣는다 — `enforces_identity`, `transport_secured`, 강제 지점
  `target` | `bridge only`. IOR의 `TAG_CSI_SEC_MECH_LIST`/`TAG_SSL_SEC_TRANS`를
  `identity::PeerCapability`가 읽고, `Assertion::RecordedOnly`와 감사 줄도 같은
  분류에서 파생된다. 두 픽스처 실측: omniORB 4.3.4·JacORB 3.9 모두
  `enforced-by=bridge only, cleartext`; 음성 대조군: 신원 단언을 광고하도록 조작한
  IOR은 양 바이트 순서 모두 `enforced-by=target`. **`Bridge::stats()`가
  `connect_static`을 통해 채워진다** — `CallStats`는 `CallPath` 열을 가진 하나의
  저장소; gen-corpus I4 오라클의 로컬 `CallStats`는 사라졌다.

- **`idl-diff --approve` is now a record, not a printed line.** Each blocking
  finding accepted under `--approve <reason>` is appended to
  `<proposed>.approvals.tsv` (or `--approvals <file>`) with the two units'
  SHA-256 fingerprints, the finding key, reason, **required `--approver`** (or
  `ORBWEAVER_APPROVER`; absent → exit 2: a decision with no name on it is not
  a decision on record) and an ISO timestamp. A later run reads the store:
  covered findings report `[approved by <who>: <reason>]` and pass; an edited
  contract — including a shared header — invalidates the row and the gate
  refuses again, saying so; a store with a nameless row is refused whole.
  Replays are byte-identical apart from `approved_at` (`SOURCE_DATE_EPOCH`
  pins that too). The console `diff` page renders who/why/when per finding
  from the same store and still decides nothing. SHA-256 is first-party (FIPS
  180-4, published vectors in tests); no dependency added. Harness: the replay
  property, and no corpus contract may carry a committed store.

  **`idl-diff --approve`는 이제 출력이 아니라 기록이다.** 파괴적 판정마다
  `<proposed>.approvals.tsv`에 두 번역 단위의 SHA-256, 판정 키, 이유, **필수
  `--approver`**(없으면 exit 2), ISO 시각을 한 행으로 남긴다. 재실행은 저장소를
  읽어 `[approved by …]`로 통과시키고, 계약(공유 헤더 포함)이 한 바이트라도
  바뀌면 행이 무효가 되어 다시 거부하며, 이름 없는 행이 있으면 저장소 전체를
  거부한다. 재실행 기록은 시각 열을 빼면 바이트까지 같다. 콘솔 `diff` 페이지는
  같은 저장소에서 누가·왜·언제를 그리고 결정하지 않는다.

- **SIDL v1 has a version constant.** `SIDL_VERSION = "1"` beside both
  vocabulary copies (forge S3, test S7), pinned equal across crates for the
  first time — until now no test compared the two crates' vocabularies to each
  other; contracts may declare `//@ sidl_version: N` (read from the syntax
  tree, since a file-top comment lands on a `module` the registry keeps
  nothing for); unknown or later → `s3/unknown-sidl-version` /
  `contract/unknown-sidl-version` (Warning); none → v1. golden 19 declares
  it; the harness runs the checker over a scratch v2 copy and requires the
  finding. **R18: a conformant non-zero default label is read as nothing** —
  42 hand-built union TypeCodes (six discriminator kinds, MAX/MIN/all-ones/
  colliding/invalid, both orders) decode `==` the zero-label shape and 304
  values round-trip under them; three negative controls in the commit. Found
  on the way: our own zero label collides with a real zero-valued case (legal;
  ignored by us and omniORB, accepted by JacORB).

  **SIDL v1에 버전 상수가 생겼다.** 두 어휘 사본 옆에 `SIDL_VERSION = "1"`,
  크레이트 간 동일성을 처음으로 테스트로 고정; 계약은 `//@ sidl_version: N`을
  선언할 수 있고 모르는 버전은 S3·S7에서 Warning; 선언 없음 → v1. golden 19가
  선언. **R18: 적합한 피어가 쓴 0이 아닌 default 라벨은 없는 것으로 읽는다** —
  손으로 만든 union TypeCode 42개(판별자 6종, 양쪽 바이트 순서)가 0-라벨 형태와
  `==`; 값 왕복 304회.

### Fixed / 수정

- **`ChannelStats::dropped` split by cause — the trigger that had no
  instrument.** One counter summed **five** different events (the design note
  that found it said three; reading the code for the split found two more), so
  a clean `stop()` moved the same number as an overloaded consumer and
  `PLAN-DEFERRED` §1's un-defer trigger for CosNotification — *"a measured
  drop rate caused by unwanted fan-out"* — could not be answered in either
  direction. `dropped` stays the total; `dropped_overflow` (back-pressure),
  `unrelayable` (our own relay limitation), `dropped_on_disconnect`,
  `dropped_on_failure_disconnect` and `dropped_at_stop` say which.
  `ChannelStats::discard` is the only discard path, so the total and the
  causes cannot drift, and `split_adds_up()` is asserted by every test that
  drives a drop. New `fanned_out` counts per-proxy copies — the denominator a
  rate needs, since `accepted` is one per event whatever it fans out to. Found
  on the way and fixed with it: a push-side `relay_check` refusal was
  discarded **without being counted in `dropped` at all**, while the pull path
  counted the same refusal twice. And the trigger itself was **circular** —
  restated in PLAN-DEFERRED §1 as two observations, one from each side,
  because CosEvent has no subscription predicate and cannot know what a
  consumer wanted; that is what the deferred chapter's filters would add.

  **`ChannelStats::dropped`를 원인별로 분리 — 계측기 없던 방아쇠.** 한 숫자가
  서로 다른 **다섯** 사건을 합산했다(발견한 설계 노트는 셋이라 했고, 분리하려
  코드를 읽자 둘이 더 나왔다). 깨끗한 `stop()`이 과부하 소비자와 같은 카운터를
  올렸으므로 `PLAN-DEFERRED` §1의 방아쇠는 어느 방향으로도 답할 수 없었다. 이제
  원인별 카운터가 총합을 이루고, 폐기 경로는 `discard` 하나뿐이며, 드롭을
  유발하는 모든 테스트가 합산을 단언한다. 새 `fanned_out`이 비율의 분모다. 함께
  발견해 고친 것: 푸시 경로의 `relay_check` 거부는 `dropped`에 아예 계상되지
  않았고 풀 경로는 두 번 셌다. 그리고 방아쇠 문장 자체가 **순환**이었다 — §1에서
  양쪽 관찰 둘로 다시 썼다.

- **A permanent forward moves the object, so every clone of a `Reference` is
  told.** `LOCATION_FORWARD_PERM` re-pointed the handle that heard it and not
  its clones, and §9.6 made the disagreement silent — the old address stays
  valid, so a stale clone is *served*, one forward per call, forever. Measured
  at a template cloned per call, the shape `Invoker::invoke`'s `&mut self` asks
  for: **3 requests at the address the object left, now 1**, both reply byte
  orders, and it did not converge before. The address now lives behind
  `Arc<Guarded<Ior>>`: a shared-mode read per call, an exclusive one only on a
  permanent hop, never held across the wire. The **temporary** cache stays per
  handle by decision (§9.6 keeps the original authoritative, so it is routing
  state and self-corrects), pinned by a test that stays green under the control.
- **`sidl-validate --against` compares two resolved units, not two splices.**
  The command resolves both contracts and then handed both `Unit::text`s back
  to the string entry point, which preprocessed each splice a *second* time. A
  splice carries the `#ifndef` of every header inside it, and a guard that is
  not the first directive of its text is conditional compilation, which this
  front end refuses on purpose — so over a **guarded** multi-file contract, the
  ordinary shape of a released one, **the §5.3 comparison never ran**. It
  exited 1 either way, which is why nothing looked wrong: an unmeasured check
  reported as a refusal. `validate_unit_against{,_for}` take resolved units and
  leave positions to the caller's unit. Corpus: `corpus/include/evo-*`.
- **A peer's `valuetype` and abstract-interface *description* now reads in the
  Python target.** `_rt.py` had no `_desc_of` arm for AnyJSON v1.1's `value`
  and `abstract_interface` forms, so a peer-fed document carrying one was
  refused — and with it every type the form was nested inside. Both now read
  (a valuetype as a synthesised `_rt.ValueType` carrying modifier, concrete
  base and per-member visibility, registered before its body so recursion
  resolves); marshalling a *value* of either is still refused in both
  directions through one format string identical to
  `orbweaver_dynamic::decode`'s sentence. Sweep unmoved at 172/137 and 70/46.
- **A diagnostic's position names a file somebody can open, in all three of
  `sidl-validate`'s output forms.** `--json` and `--repair-prompt` served the
  *splice's* line under the *root* file's name — and `--repair-prompt` is read
  by a **model**, so a wrong line sent a repair to the wrong file with nothing
  going red. `line`/`column` now mean the position in the file the text was
  written in, and a finding written elsewhere names its own file in a
  per-finding `"file"` emitted only when it differs (single-file documents
  byte-identical, `cmp` over five corpus pairs). Two bugs went with it: a
  line-0 finding printed as line 1 of the root, and every finding printed its
  position twice. Four negative controls.
- **A constant takes `const_type`, not `type_spec`.** §7.4.1.4.2's `const_type`
  is the **third** production narrower than a declaration's, and we diverged
  from omniidl in **both** directions: `const fixed LIMIT = 9.9d;` is legal
  (digits and scale come from the value) and we rejected it, while
  `const fixed<3,1>`, `const any`, `const sequence<T>`, `const void`,
  `const Object` and `const ValueBase` are syntax errors and we accepted them.
  **Seven shapes, one cause** — `const_def` called `type_spec` — and fixing
  only `fixed` would have closed two of seven. A `const fixed` is deliberately
  outside `wire/deferred-type`'s closure: nothing about a constant reaches a
  peer, so refusing a whole file for one was a false refusal. New
  `corpus/golden/30-const-type.idl` and `corpus/negative/n18`; **JacORB 3.9
  refuses the bare keyword**, measured on landing and recorded with its reason
  in `corpus/divergences.tsv` (five now). Found and not fixed: a fixed
  constant has no *value* in the registry (the lexer folds `9.9d` to a float),
  `const wchar`/`const wstring` cannot be written at all, and
  `const long double` is legal by the grammar while omniidl refuses it.

  **영속 포워드는 객체의 이동이므로 모든 클론이 듣는다**(옛 주소 요청 3→1).
  **`--against`는 스플라이스가 아니라 해석된 유닛을 비교한다** — 가드가 있는
  다중 파일 계약에서 §5.3 비교가 **아예 실행되지 않았고**, 종료 코드가 1이라
  아무도 몰랐다. **피어의 valuetype·추상 인터페이스 기술을 Python이 읽는다**(값은
  여전히 양방향 거부). **진단의 위치가 열어 볼 수 있는 파일을 가리킨다** —
  `--repair-prompt`는 모델이 읽으므로 수리가 엉뚱한 파일로 갔다. **상수는
  `const_type`을 받는다** — 일곱 형태가 어긋나 있었고 원인은 하나였다; JacORB는
  맨 `fixed` 키워드를 거부하므로 그 불일치를 이유와 함께 기록했다.

- **A forward *chain* re-points the reference at the hop that asked for it.**
  `Pool::attempt` reported only the last hop, so a `permanent → temporary`
  chain cached the temporary target against the address the caller started
  from and §9.6's restart went back **through** the permanent hop — within the
  spec, one hop more than the servant asked for. Hops are now accumulated
  (`pool::Chain`) and applied per hop as `Connection::follow` already did: a
  permanent hop re-points `Reference::ior` and clears the cached forwarding
  information, a temporary hop after it is cached relative to the new
  reference, and the restart returns there. Measured both reply byte orders,
  3 shapes (`tests/forward_chain.rs`); the restart costs no dial (the pooled
  permanent connection is reused, `dialed == 3`). Negative control: the
  last-hop-only rule → the restart is answered by the original (99, not 7).
- **A caller's `cap_version` survives a forward and a restart.**
  `Connection::move_to` restored byte order, converter, TLS policy and origin
  but re-negotiated the version from the forwarded-to profile, so a caller
  capped to 1.1 spoke 1.2 at a 1.2 target — a wire-format change under a
  caller who cannot see the hop. The cap is kept and the version spoken is the
  lower of it and §9.4.1's own ceiling, so a profile can lower it further and
  never contradict it; there was nothing to decide. Measured off the wire at
  both peers, both request orders. `Pool`/`Reference` have no cap API at all —
  that is D012's question, not this batch's.

  **포워드 *체인*이 그것을 요구한 홉으로 레퍼런스를 다시 겨눈다.**
  `Pool::attempt`는 마지막 홉만 보고했다 — `영구 → 임시` 체인이 임시 대상을
  호출자가 출발한 주소에 캐시했고, §9.6의 재시작이 이미 대체된 영구 홉을 **거쳐**
  돌아갔다. 이제 홉을 누적해 홉마다 적용한다; 재시작에 다이얼 비용 없음
  (`dialed == 3`). 음성 대조: 마지막 홉만 보면 재시작을 원본이 응답(7이 아닌 99).
  **호출자의 `cap_version`이 포워드와 재시작을 넘어 살아남는다.** 상한을 유지하고
  §9.4.1의 천장과 **더 낮은 쪽**을 말한다 — 프로파일은 더 낮출 수는 있어도 상한과
  모순될 수 없으므로 결정할 것이 없었다. 양 요청 바이트 순서로 와이어에서 실측.

- **A signature takes `param_type_spec`, not `type_spec`.** A bare
  `fixed<d,s>`, an anonymous `sequence<T>` and `void` were accepted as
  attribute, parameter and return types; omniidl refuses all of them
  (`Syntax error in interface body` / `in operation parameters`) because
  `param_type_spec` is `base_type_spec | string_type | wide_string_type |
  scoped_name` — a template type reaches a signature only through a `typedef`.
  The batch was scoped to `fixed` and the root cause was one production wider:
  the parser called `type_spec` where the grammar says `param_type_spec` /
  `op_type_spec`, which differ in exactly three constructs, and **ten
  divergences were measured, eight closed by one function**
  (`Parser::signature_type_spec`) — a `fixed`-keyword fix would have closed
  three. New rules `anonymous-type-in-signature` and `void-in-signature`, each
  with a `fix_for` edit naming the typedef, and `corpus/negative/n13`–`n17`,
  rejected by omniidl and by us. Negative control: with the fix reverted,
  `differential.sh` names all five as "we say accept, oracles reject".
  Found on the way, not fixed: `const_type` is a third narrowed production and
  diverges in **both** directions — `const fixed LIMIT = 9.9d;` is legal and we
  reject it, `const fixed<3,1> LIMIT = 9.9d;` is illegal and we accept it.

  **시그니처는 `type_spec`이 아니라 `param_type_spec`을 받는다.** 맨
  `fixed<d,s>`, 익명 `sequence<T>`, `void`가 속성·파라미터·반환 타입으로
  통과했다. omniidl은 전부 거부한다 — `param_type_spec`에는 템플릿 타입이 없어
  `typedef`를 거쳐야만 시그니처에 도달한다. `fixed`로 좁혀 시작한 배치가 한
  프로덕션 더 넓은 근본원인을 찾았고, **실측 불일치 10건 중 8건을 함수 하나로**
  닫았다(키워드만 고쳤다면 3건). 규칙 `anonymous-type-in-signature`·
  `void-in-signature`에 수정 힌트, `corpus/negative/n13`–`n17`. 음성 대조군:
  수정을 되돌리면 `differential.sh`가 다섯 파일을 "우리는 수용, 오라클은 거부"로
  이름 붙인다. 도중 발견(미수정): `const_type`은 세 번째로 좁혀진 프로덕션이며
  **양방향으로** 어긋난다.

- **The plan reviewed against the code, row by row — nineteen rows
  understated, six overstated, all restated where they live**
  (`docs/pipeline-runs/2026-08-19-plan-review.md`). PLAN §7.2 no longer says
  S1–S3 are not started (measured 2026-08-13); §7.3 strikes what landed in
  streams A/C/D and names the actual open items (a SIDL version marker, R17's
  re-establishment half, the per-peer CSIv2 record, an `--approve` store);
  §7.4 I4 is ✅ (the dynamic audit line is captured, not reconstructed); §8's
  rows name the instrument that exists (`idl-check`, DII over `echo.idl` +
  the corpus through DynAny/AnyJSON, seven `*_from_a_peer.rs` files, EUC-KR
  unmeasured on the wire, no v0.5.0 model run, contract tests not generated,
  the hold-out subset as A7's procedure); §9.1 R7/R8/R11/R13/R17 restated as
  the halves they are, and **R18** added — a peer's defect becoming our
  specification (the union `default:` label). PLAN-SERVICES: CosNaming's three
  operations served (one fact had three homes and three answers), CosEvent ✅
  with the consumer half of pull, the `Capability` gap closed, F3/F7 marked
  landed, the F4+F7 telemetry-feedback row named as open with its precondition
  met. PLAN-MOE: F4/F5 ✅, IF2 landed, the D006 quote dated (the bound has been
  enforced by both paths since 526b355 — noted in D006 too). PLAN-DEFERRED
  gains §10–§12 for the three 2026-08-18 deferrals with their triggers, and §7
  cites F5's in-code trigger evaluation. `SERVICES-COVERAGE` §9's `--hold`
  paragraph dated and the orphaned `spikes/svc-hold` removed; `COMPONENTS`'
  CosEvent cell three answers behind, corrected; `expert_service.rs`'s header
  said `BAD_OPERATION` where the wire says `NO_IMPLEMENT`. **Codified:**
  `spikes/plan_numbers.py`, a report — every hand-typed count in the plan
  documents beside today's computed figure (six found, six stale within five
  days; the sentences now carry their date and point at the script). Harness:
  one group per interop cell, three claims that were green while unrun now
  counted `SKIPPED` (S1–S3 replay, the second-host NAT probe, the I3 line
  naming both classes) — SKIPPED 5 → 7.

  **계획서를 코드에 행 단위로 대조 — 19행 과소, 6행 과대, 각 사실이 사는
  자리에서 재기술.** PLAN §7.2·§7.3·§7.4·§8·§9.1(R18 신설), PLAN-SERVICES,
  PLAN-MOE, D006 인용의 날짜, PLAN-DEFERRED §10–§12 신설, SERVICES-COVERAGE
  §9, COMPONENTS의 CosEvent 칸, `expert_service.rs` 헤더. **코드화:**
  `spikes/plan_numbers.py` — 계획 문서의 손으로 적은 수치를 오늘 계산값 옆에
  찍는 리포트(6건 발견, 6건 모두 닷새 안에 낡음). 하네스: 상호운용 셀당 그룹
  하나, 돌지 않고 초록이던 주장 셋을 `SKIPPED`로 — 5 → 7.

---

## v0.5.0 — 2026-08-19

The release **a peer's bytes** produced. Twelve wire-behaviour changes lead,
and every one of them was invisible to our own round trip: union case labels
stored in the arriving order, `long double`'s octets, UTF-16 read in the
message's order rather than its own, an encapsulation's alignment origin, a
`wstring` written from a 1.2 constant on every connection, a 1.1 `wstring`
carrying a mark JacORB read as text, a union `default:` member with no label
bytes at all, a member list that was ours rather than omniidl's, a 1.2 `wchar`
that was itself a mark. Each was found by asking omniORB or JacORB for their
octets, recording them with provenance, and re-taking the capture from the
live fixture on every harness run — 30-odd captures in eight files now. *A
convention both ends apply cannot be refuted by a round trip*, and its
corollary from this release: **a convention one end applies on read can hide
the other end's defect on write.**

Two decisions closed (D008 AnyJSON v1.1: a type describes itself structurally;
D009: the negotiated codeset reaches the marshaller through an owned slot,
measured at ~31 ns a string) and one was written from the gap columns and then
corrected three times by the batches that built it (D010 — what remains, and
which of it cannot be measured here). All of D010's class-A rows landed. Its
class-B rows are counted `SKIPPED` groups that name their missing fixture, and
the one that turned out measurable (GIOP 1.1 wide text against JacORB)
measured **red before it measured green**.

Three security findings from the agent boundary (a 12-byte message that
reserved 64 MB, a document array length that reserved 206 GB, an argument a
content stage saw reaching the audit ledger), each with a negative control in
its landing message — which is now the rule for every harness group. And the
gates that were green while measuring nothing: four found by negative controls
in the first session, one more written by the coordinator and caught on its
own first run (a probe that grepped its marker out of a traceback echoing the
source line). Documents a script writes replace documents a hand transcribes:
`SERVICES-COVERAGE.md` §8 is generated from the sweep and diffed by the
harness, and went red by itself the first afternoon.

CI was red for eleven runs when this release's second day began, on three
harness-only causes nobody had read; green on every push since — including
the two things Linux caught that macOS cannot.

이번 릴리즈는 **피어의 바이트**가 만들었다. 와이어 동작 변경 열둘이 앞장서며,
하나도 우리 자신의 왕복으로는 보이지 않던 것이다 — 도착한 순서대로 저장된 union
레이블, `long double`의 옥텟, 자기 순서가 아닌 메시지 순서로 읽힌 UTF-16,
인캡슐레이션의 정렬 원점, 매 연결마다 1.2 상수로 쓰인 `wstring`, JacORB가
텍스트로 읽은 1.1 `wstring`의 표식, 레이블 바이트가 아예 없던 union `default:`
멤버, omniidl이 아닌 우리 것이던 멤버 목록, 표식 그 자체이던 1.2 `wchar`. 전부
omniORB·JacORB에 옥텟을 물어 출처와 함께 기록하고 하네스가 돌 때마다 라이브
픽스처에서 재채취해서 나왔다. **양쪽이 똑같이 적용하는 관례는 왕복으로 반증되지
않고, 한쪽이 읽을 때 적용하는 관례는 다른 쪽이 쓸 때의 결함을 가린다.**

결정 둘이 닫히고(D008, D009) 하나는 공백 열에서 쓰인 뒤 그것을 지은 배치들이 세
번 정정했다(D010). D010의 A류는 전부 착지했고, B류는 빠진 픽스처를 이름 붙인
`SKIPPED` 그룹이며, 잴 수 있게 된 하나(JacORB 상대 GIOP 1.1 와이드 텍스트)는
**초록 전에 빨강으로** 재어졌다. 에이전트 경계의 보안 발견 셋, 각각 착지 메시지에
음성 대조군 — 이제 모든 하네스 그룹의 규칙이다. 스크립트가 쓰는 문서가 손으로
옮겨 적던 문서를 대체한다. 이 릴리즈의 둘째 날은 CI가 열한 런 빨간 채로 시작했고,
그 뒤 모든 푸시에서 초록이다 — 리눅스만 잡을 수 있던 둘을 포함해서.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **A GIOP 1.2 `wchar` that is itself a mark is written behind a mark, and a
  bare mark is read as the unit** (D010 B5, fourth part; `codeset.rs`,
  `spikes/wide_rust.sh`, `tests/wide_1_2_from_a_peer.rs` new). ff2c742's open
  finding — U+FEFF at 1.2 written `02 fe ff` by both writers and stripped as a
  mark by both readers — was measured against JacORB 3.9's reader first: it
  honours `04 fe ff fe ff` and `04 ff fe ff fe` as U+FEFF (13-row matrix, both
  message orders). `put_wchar` at 1.2 now writes U+FEFF as `04 fe ff fe ff`
  and U+FFFE as `04 fe ff ff fe`, every other unit unchanged; `get_wchar` at
  1.2 reads `02 fe ff`/`02 ff fe` as U+FEFF/U+FFFE (a wchar is never empty, so
  only a bare writer — JacORB's — produces them). Measured after: JacORB's user
  gets both back from our real server, our client gets both from JacORB's bare
  echoes, both orders. Reported: JacORB's own writer still writes them bare
  and its own reader cannot read them back.

  **표식 그 자체인 GIOP 1.2 `wchar`는 표식 뒤에 쓰고, 맨 표식은 유닛으로
  읽는다** (D010 B5 네 번째). ff2c742의 미해결 발견을 JacORB 3.9 리더에 먼저
  쟀다: `04 fe ff fe ff`와 `04 ff fe ff fe`를 U+FEFF로 읽는다(13행 행렬, 두
  메시지 순서). `put_wchar` 1.2는 이제 U+FEFF를 `04 fe ff fe ff`, U+FFFE를
  `04 fe ff ff fe`로 쓰고 다른 유닛은 그대로; `get_wchar` 1.2는 `02 fe ff`/
  `02 ff fe`를 U+FEFF/U+FFFE로 읽는다. 수정 후 실측: JacORB 사용자는 우리 실제
  서버로부터 둘 다 되받고, 우리 클라이언트는 JacORB의 맨 에코에서 둘 다 받는다.
  보고: JacORB 자신의 라이터는 여전히 맨 형태로 쓰고 자신의 리더가 그것을 되읽지
  못한다.
- **A union branch that is both labelled and `default:` now produces one
  TypeCode member per label plus a labelless default member at the position
  `default:` was written, `default_index` on it** — structurally equal (`==`)
  to omniidl's and JacORB's IDL-derived TypeCode (measured against 8 omniORB
  captures, both stream orders). Before, `case 2: default: string rest;` was
  two members with `default_index` on `(2, rest)`: the same branch selected at
  both peers, but a different `member_count`/`default_index` on the wire, and
  IDL regenerated from our own decoded TypeCode lost the `case 2:`. Value
  encoding unchanged; Rust stubs unchanged (emitter output over all 31 golden
  files identical); Python classes gain `_idl_default_slot`; sweep 170/137 +
  70/46, 0 divergences. Found and not fixed: the §5.3 differ compares the
  default member by label, not by role (fixed below, a40317a).

  **라벨과 `default:`를 함께 가진 union 분기는 이제 라벨마다 한 멤버에 더해
  `default:`가 쓰인 위치에 라벨 없는 default 멤버를 하나 더 만들고
  `default_index`가 그것을 가리킨다** — omniidl·JacORB가 같은 IDL에서 만드는
  TypeCode와 구조적으로 동일(`==`, omniORB 캡처 8건, 양쪽 바이트 순서). 이전에는
  `case 2: default: string rest;`가 두 멤버였고 양쪽 피어의 분기 선택은 같았지만
  와이어의 `member_count`/`default_index`가 달랐으며 재생성한 IDL은 `case 2:`를
  잃었다. 값 인코딩 불변, Rust 스텁 불변, Python 클래스에 `_idl_default_slot`
  추가. 보고만: §5.3 differ가 default 멤버를 역할이 아닌 라벨로 비교한다(아래에서 수정, a40317a).
- **A union TypeCode's `default:` member is written with a label of the
  discriminator's width — zeros — and that slot is ignored on read** (was:
  zero bytes for a bare default, so any `any` carrying `corpus/golden/06`'s
  `WithDefault` went out malformed; our own decode failed "implausible CDR
  length prefix", omniORB `MARSHAL_PassEndOfMessage`, JacORB "buffer too
  small", never red because every gate ran both ends through one encoder —
  found by golden 29 the same day). §9.3.5.1.4: the value "has no semantic
  significance … should be ignored". Zeros because JacORB 3.9 reads the slot
  as one octet that must be 0 (omniORB's unused-value choice fails it
  big-endian; the registry's labelled default written as its own label failed
  it little-endian), and omniORB ignores the value entirely. Recorded with
  provenance in `tests/union_default_label_from_a_peer.rs` (omniORB 4.3.4,
  nine captures including a big-endian stream), retaken by
  `spikes/union_default_capture.py`. Registry unchanged: a bare default keeps
  its empty label in memory; the codec translates. Measured after: omniORB and
  JacORB decode golden 06/29's TypeCodes in both byte orders and select the
  same branch for every discriminator. Found and not fixed: the registry folds
  `case 2: default:` where omniidl expands it (same selection everywhere,
  different `member_count`); omniORBpy segfaults on `default_index == 0`
  without a stub loaded; JacORB misreads a `long long` default label from any
  conformant peer.

  **유니온 TypeCode의 `default:` 멤버는 판별자 폭만큼의 레이블(0으로 채움)로
  기록하고, 읽을 때는 그 슬롯의 값을 무시한다**(이전: 레이블 없는 default를
  0바이트로 기록 — `golden/06`의 `WithDefault`를 담은 모든 `any`가 잘못된
  형태로 나갔고, 우리 디코더·omniORB·JacORB 모두 거부; 양 끝이 같은 인코더를
  거쳐 한 번도 붉어지지 않았다). §9.3.5.1.4: 그 값은 "의미가 없으며 무시해야
  한다". 0인 이유: JacORB 3.9는 이 슬롯을 옥텟 하나로 읽고 0이 아니면
  거부(omniORB의 미사용값은 빅엔디언에서, 레이블 붙은 default를 그 레이블로
  쓰면 리틀엔디언에서 실패), omniORB는 값을 완전히 무시. omniORB 4.3.4 캡처
  9건을 출처와 함께 기록, `spikes/union_default_capture.py`로 재채취.
  레지스트리는 변경 없음. 보고만: 레지스트리는 `case 2: default:`를 한 멤버로
  접고 omniidl은 둘로 편다(선택은 같음, `member_count`는 다름).
- **`wide.idl` from our own stack.** `spike-wide` (orbweaver-object) serves
  and dials `IDL:spike/Wide:1.0` through `Server`/`Connection`;
  `spikes/wide_rust.sh` re-runs 382baa9's matrix with the Rust stack in each
  seat plus 1.0/1.1/1.2 self-consistency in both orders, and checks the live
  octets against `wide_1_1_from_a_peer.rs` — the real server writes exactly
  the recorded replies in both orders and our 1.1 request is octet-for-octet
  JacORB's. Recorded: JacORB's lone surrogate is refused by the real reader
  (MARSHAL). **Found, not fixed:** U+FEFF as a GIOP 1.2 `wchar` crosses
  neither stack — both writers write `02 fe ff`, both readers strip it as a
  mark (JacORB → U+0000, ours → MARSHAL); pinned until `put_wchar` at 1.2
  changes, and the proposed `04 fe ff fe ff` must be measured against JacORB's
  reader before it is adopted (measured and adopted the same day — see the
  Wire behaviour entry above).

  **`wide.idl`을 우리 스택으로.** `spike-wide`가 `Server`/`Connection`으로
  `IDL:spike/Wide:1.0`을 서빙·호출; `spikes/wide_rust.sh`가 382baa9의 행렬을
  Rust 스택을 양쪽 자리에 앉혀 다시 재고 1.0/1.1/1.2 자기일관성을 더하며 실측
  옥텟을 기록과 대조 — 실제 서버가 기록된 응답을 두 순서 모두 옥텟 단위로 쓰고,
  우리 1.1 요청은 JacORB의 것과 옥텟 단위로 같다. **발견, 미수정:** GIOP 1.2
  `wchar` U+FEFF는 어느 스택도 건너지 못한다(두 라이터 모두 `02 fe ff`, 두 리더
  모두 마크로 벗김); `put_wchar` 1.2가 바뀔 때까지 고정, 제안 형태는 JacORB
  리더로 먼저 재야 한다.
- **1.1 `wchar` measured against JacORB 3.9, both directions and both byte
  orders** (D010 B5, second half; `spikes/wide.idl`, `spikes/jacorb_wchar11.sh`,
  `tests/wide_1_1_from_a_peer.rs` appended). A 1.1 `wchar` is its two octets
  in the **message's** order and nothing else — JacORB writes `d5 5c` for
  U+D55C in its big-endian messages, reads our `5c d5` in a little-endian
  reply as U+D55C and the control `d5 5c` in the same frame as U+5CD5 (4/4
  units, both directions); U+FEFF is data at 1.1 on both sides. That is what
  `put_wchar`/`get_wchar` always did — **no code change**; the "unmeasured"
  doc paragraph is replaced by the measurement. Recorded behaviour: a lone
  surrogate crosses as two octets (we refuse it as not a character, and refuse
  U+1F600 as two units); four octets offered as one wchar: JacORB takes the
  first two. JacORB's whole request and reply are recorded and decode through
  `decode_request`/`decode_reply`; the live octets are re-checked against the
  recording on every run. Not measured: our live Rust stack on this contract
  (the hand-built peer stands in; the codec is held to the octets by tests);
  JacORB's writer in a little-endian 1.1 message.

  **1.1 `wchar`를 JacORB 3.9에 대해 양방향·양 바이트 순서로 측정**(D010 B5
  후반). 1.1 `wchar`는 **메시지의** 순서로 놓인 두 옥텟이고 그 이상이 아니다 —
  JacORB는 U+D55C를 빅엔디언 메시지에 `d5 5c`로 쓰고, 우리가 리틀엔디언 응답에
  넣은 `5c d5`를 U+D55C로, 대조군 `d5 5c`를 U+5CD5로 읽었다(양방향 4/4).
  U+FEFF는 양쪽 모두 데이터. 코덱이 원래 하던 그대로 — **코드 변경 없음**;
  "미측정" 문단을 측정으로 교체. JacORB의 요청·응답 전체를 기록하고 실행마다
  라이브 옥텟과 대조한다. 미측정: 이 계약에서 우리 실제 Rust 스택(손으로 만든
  피어가 대신 섬); 리틀엔디언 1.1 메시지의 JacORB 라이터.
- **`wstring` at GIOP 1.0/1.1 is written without a byte-order mark and with
  its units in the message's byte order** (was: BOM + stream order, as at
  1.2). D010 B5's first step was a JacORB-at-1.1 fixture, and it found this the
  same hour: measured against JacORB 3.9 (`spikes/jacorb_giop11.sh`, both
  directions, both byte orders), a 1.1 peer read our mark as U+FEFF text, and
  its echo of it was stripped by our reader, so the round trip was green while
  the peer's user saw the wrong value; an unmarked value is read by that peer
  in the message's order — the literal §9.3.1.6 "neither → big-endian" bullet
  is contradicted by the only 1.1 wide-text peer here, and the code says so.
  GIOP 1.2 unchanged (mark, big-endian default). The reader still removes a
  leading mark at 1.1 ("shall remove", unscoped). Recorded with provenance in
  `crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs`; the fixture asserts
  the version from bytes (JacORB's `giop_minor_version` sets the IORs it
  creates; its client follows the profile it dials) and counts wire units so
  the masking cannot recur. Harness group added; a class-B row that turned out
  measurable, and measured red first.

  **GIOP 1.0/1.1의 `wstring`은 바이트순서표식(BOM) 없이, 유닛을 메시지의
  바이트순서로 기록한다**(이전: 1.2와 같이 BOM + 스트림 순서). D010 B5의 첫
  단계인 JacORB-1.1 픽스처가 같은 시간에 이것을 찾았다: JacORB 3.9로
  측정(`spikes/jacorb_giop11.sh`, 양방향·양 바이트순서), 1.1 피어는 우리
  표식을 U+FEFF 문자로 읽었고 그 에코를 우리 리더가 표식으로 벗겨내어 왕복은
  녹색이면서 피어 사용자에게는 잘못된 값이 갔다; 표식 없는 값은 그 피어가
  메시지 순서로 읽는다. GIOP 1.2는 변경 없음. 리더는 1.1에서도 선행 표식을
  제거한다. `tests/wide_1_1_from_a_peer.rs`에 출처와 함께 기록; 하네스 그룹
  추가 — 잴 수 없다던 B류 행이 재어졌고, 먼저 빨갛게 재어졌다.


- **A union `TypeCode`'s case labels are aligned and byte-order-normalised.**
  A label is the discriminator marshalled in its own type. It was read with
  `get_bytes` and written with `put_bytes` — neither of which knows the
  endianness, and neither of which aligns — so it carried the byte order of
  whatever stream produced it. Our encode and decode agreed with each other in
  any order, which is why 1200 tests were green while, against the fixture peer
  on this host:

  - a `long long` discriminated union **could not be decoded at all**, and said
    so as `"string length must include the NUL"` — pointing four fields past
    the fault, at a string, because an unaligned 8-byte read shifted everything
    after it;
  - a `long` discriminated union decoded and then missed **every** branch, in
    both stream orders, with a refusal that blamed the caller's discriminator.

  `UnionCase::label` is now always big-endian and conversion happens at the
  wire. Both cases are pinned as the bytes omniORB actually wrote, and the
  regression test also **re-encodes them back to the peer's bytes** — the check
  our own round trip could not perform, because it agreed with itself.

  **Upgrading:** anything holding a `UnionCase::label` built by hand for a
  little-endian stream was already wrong on the wire and is now wrong in the
  type; build labels big-endian. Nothing else changes.

  union 레이블은 판별자를 그 타입으로 마샬링한 것인데, 정렬 없이 원바이트로
  읽고 썼기에 **온 스트림의 바이트 순서를 그대로** 지녔다. 우리끼리는 일치했으므로
  1200건이 초록이었다: `long long` union은 **디코드조차 되지 않았고**(진단은 네
  필드 뒤의 문자열을 가리켰다), `long` union은 **모든 분기를 빗나가며** 호출자를
  탓했다. 이제 레이블은 항상 빅엔디언이고 변환은 와이어에서 일어난다. 피어가
  실제로 쓴 바이트를 고정했고, **그 바이트로 되돌려 인코딩되는지**까지 검사한다.

- **AnyJSON writes a union's case labels as values, not base64.** They were
  base64 for exactly as long as their byte order was unknowable — one commit.
  An enum discriminator's labels now read `"label":"GREEN"` rather than four
  bytes that say nothing. A label that will not decode falls back to
  `{"_raw": <base64>}`, tagged, because a malformed TypeCode is its producer's
  problem and a renderer that refuses to render it hides the evidence.

- **Six wire defects our own round trip could not see.** A sweep of
  `orbweaver-cdr` and `orbweaver-giop` for the class the union-label batch
  named — a field both our ends agree about and a peer does not — examined 24
  candidates and found 6 defects in 3 causes, with 17 correct and their reasons
  written down.

  `long double` moved its 16 octets with no byte-order reversal, where Figure
  9.2 draws the same reversal `float` and `double` get. A UTF-16 `wchar` or
  `wstring` took its byte order from the **message** when the value states its
  own — three sites, and the read half is one our round trip could never reach,
  because our writer always emits a BOM and so does omniORB's, so only the
  peer's *reader* could settle it: twelve bodies read back, six answers, none
  depending on the stream's flag. We returned U+7700 for an unmarked `00 77` in
  a little-endian stream where the peer returns U+0077. And an alignment origin
  leaked the enclosing buffer's offset, so a `TypeCode` encapsulation inside a
  GIOP 1.0/1.1 body aligned from the message's offset rather than its own flag
  — unreachable at 1.2, which rounds the body start to a multiple of 8, and
  unconditional below it.

  Two preconditions became checks rather than prose: relaying an `any` into a
  different byte order is refused, and a `Fragment` that flips the byte-order
  flag is refused per §9.4.9.

  Where no peer could answer, the specification is the oracle and the code says
  so. GIOP 1.1 `wchar` unit order is **left unchanged and recorded as
  unmeasured**: omniORBpy marshals one and then fails to unmarshal its own
  output on this host, so nothing here can settle it.

  우리 왕복이 볼 수 없던 결함 6건 — 양쪽이 서로만 합의하는 관례가 숨긴 것들.
  피어가 오라클일 수 없는 자리는 **미측정으로 남겼다.**

- **A reference that declares no codeset refuses wide text, and ours declared
  none.** §7.10.2.4: a profile with no `TAG_CODE_SETS` declares no wchar
  support. Measured: omniORB's client raised `INV_OBJREF` minor `0x4F4D0001`
  **inside itself** and sent nothing, while our server logged one earlier
  request and no error. **Every `wstring` operation we serve was unreachable by
  a conformant peer, and the refusal happening in the caller is why nothing
  here could see it.** Eleven publish sites now carry the component; `한글`
  round-trips at GIOP 1.2.

  A third defect fell out of the counting: `negotiated_char_converter`
  swallowed every negotiation failure into `None`, so "we cannot agree with
  this peer" was indistinguishable from "this peer said nothing" — the one peer
  that warned us was the one we told nothing.

  `spikes/reverse_client.py` had exercised every other operation on the echo
  contract and never `echo_wstring`. It does now, and found on its first run
  that GIOP 1.1 raises `MARSHAL`; that is **stated as unmeasured**, because the
  peer cannot unmarshal its own 1.1 wide output and so is not an oracle for it.

- **A stub's `wstring` takes its form from the connection.** `Cdr::put(&self,
  e: &mut Encoder)` has no connection to ask, so `WString` answered with GIOP
  1.2's form always — 1.2 counts octets and 1.1 counts characters, so on a 1.1
  connection that is a different field. Nothing here could see it: **our own
  round trip used the same constant at both ends**, which is the shape the
  union-label batch named — a convention both ends apply cannot be refuted by a
  round trip.

  A stream with nothing attached still writes the 1.2 form, and that is not a
  leftover: §9.3.1.6 fixes it for an encapsulation whatever the message says.
  Both halves are asserted, and reverting the fix fails the first.

  D009 §7.3 said to retire `wide()` and `default_codec()` together, and
  **counting the call sites showed it had them backwards**: every wire use of
  `default_codec()` is inside an `any`'s encapsulation, where the fixed answer
  is the specification. Its doc gave two justifications and only one was a
  reason — the other was a guess sitting next to a rule, borrowing its
  authority. The decision is corrected in place.

- **The transmission codeset reaches the marshaller** (D009, approved). A CDR
  stream carries an optional `TextCodec`; `None` is UTF-8 and is byte-for-byte
  what shipped before. A connection puts its negotiated `char` agreement on the
  encoder that carries a call's arguments, and a servant reads and answers in
  the codeset the **client declared** — derived from the request, not the
  connection, because two clients on one multiplexed connection can differ.

  The codec goes on the body and never the header: `operation` and the object
  key are identifiers the contract chose, and re-encoding them would change the
  name a servant dispatches on. An encapsulation does not inherit it either —
  that would silently re-encode the repository ids and member names inside
  every `TypeCode`.

### ⚠ Mapping changed / 매핑 변경

- **AnyJSON v1.1: a type describes itself structurally** (D008, approved
  2026-08-18). `_t` keeps its v1 name for a type whose identity fits in one —
  `"double"` is still `"double"` and every v1 document still reads and still
  reproduces the same CDR — and becomes an object where v1 could say nothing,
  or where the name lost something the wire keeps: `string<5>` and `string`
  were one word to v1 and are two TypeCodes to a peer. The same representation
  is now also a **value**, so `::CORBA::TypeCode` crosses.

  Measured before the change: `to_json` wrote `{"_t":"IDL:gc12/Tagged:1.0",…}`
  and `from_json` **refused that same document**. Not a limitation — an
  asymmetry, which puts the failure on the return leg in the caller rather than
  at the boundary that produced it. And `tk_TypeCode` had no `Value` variant at
  all, so §8's *static equals dynamic* oracle was not weaker for the operations
  only the static path handled; it was **inapplicable**.

  **`ir-subset` went from 18 generated + 10 skipped to 28 + 0.** The ten
  included `InterfaceDef` itself — the skip propagated up through every
  container until `describe_interface`, the operation the IFR facade exists
  for, could not be generated. The MCP bridge speaks the same mapping, so an
  **Interface Repository is now readable through the agent path**, asserted by
  repository id over the real contract in both byte orders.

  **Upgrading:** nothing to do for a v1 producer or consumer; the change is
  additive and the compatibility claim is tested, not asserted. A Python client
  gains `_rt.TypeCode` — the document, unread: enough to receive one, hand it
  back and inspect its `kind`, not enough to marshal a value *described* by
  one, which would mean Python deciding CDR questions in a package that
  deliberately contains no wire.

  AnyJSON v1.1: `_t`가 이름 하나에 담기는 타입은 v1 그대로, 그 밖에는 구조가
  된다. **추가적**이므로 v1 문서는 전부 그대로 읽히며 그 주장은 시험된다.
  변경 전 실측: 매핑이 자기가 쓴 문서를 자기가 거부했고, `tk_TypeCode`에는
  `Value`가 없었다. **ir-subset 18+10 → 28+0**, 그 열에는 `InterfaceDef` 자신이
  들어 있었다.

### ⚠ Behaviour changed / 동작 변경

- **`Connection` keeps the reference it was dialled from (`origin()`) and,
  after a temporary `LOCATION_FORWARD`, restarts there when the forwarded-to
  connection fails without the request having run** (CloseConnection, write
  failure, or already poisoned) — reuse of the forwarding information first,
  then the origin, once per call (§9.6 "shall restart the location process
  using the original address"). A permanent forward replaces the origin and
  never falls back (§9.6 "may replace", taken). Failures of unknown completion
  are still errors; the next call restarts. `Reference` caches a temporary
  forward (one round trip per call while it stands, was two), restarts at its
  IOR the same way, and re-points on `LOCATION_FORWARD_PERM`;
  `Reference::forwarded()` now reports the redirect in force, as
  `Connection::forwarded()` does. Both clients now distinguish the two
  statuses by behaviour; `spikes/perm_fallback.sh` asserts our eight cells
  alongside omniORB's two, both byte orders. Decided 2026-08-19 on the
  record's recommendation; omniORB restarts on COMPLETED_MAYBE too, ours does
  not by design.

  **`Connection`은 처음 연결한 레퍼런스(`origin()`)를 보관하고, 임시
  `LOCATION_FORWARD` 이후 전달된 연결이 요청을 실행하지 않은 채
  실패하면**(CloseConnection, 쓰기 실패, 이미 오염된 연결) 호출당 한 번 — 먼저
  전달 정보 재사용, 그다음 원래 주소 — 에서 재시작한다(§9.6). 영구 포워드는
  origin을 교체하며 되돌아가지 않는다. 완료 여부를 알 수 없는 실패는 여전히
  오류이고 다음 호출이 재시작한다. `Reference`는 임시 포워드를 캐시하고(호출당
  왕복 1회, 이전 2회), 같은 방식으로 재시작하며 `LOCATION_FORWARD_PERM`에서는
  스스로를 재지정한다. 두 클라이언트 모두 두 상태를 동작으로 구별하며,
  `perm_fallback.sh`가 우리 셀 8개를 omniORB 셀 2개와 함께 단정한다.


- **A servant that deliberately does not implement a declared operation now
  answers `NO_IMPLEMENT`, not `BAD_OPERATION`.** The three answers are three
  different facts on the wire, with no document needed to tell them apart:
  `NO_PERMISSION` — the operation exists and the answer is no, as policy;
  `NO_IMPLEMENT` — declared, not implemented, on purpose; `BAD_OPERATION` —
  this interface does not declare that name at all.

  The IFR facade worked this out first and it stayed in one servant. Everywhere
  else a decision and an oversight gave the same answer, so the difference
  lived only in a document the client cannot read. Moved: CosNaming's
  `bind_context`, `rebind_context` and `destroy`; the event channel's whole
  pull model and its `destroy`; and `moe::Router::dispatch`.

  **`moe::Router::dispatch` is the one that mattered.** D006 approved excluding
  it on 2026-08-14, and the servant went on saying "no such operation" — a
  decision recorded in prose and contradicted on the wire, in exactly the class
  `PLAN-SERVICES.md` §8.1 exists to name. The new gate found it on its first
  green run.

  **Upgrading:** a client that treats `BAD_OPERATION` as "not implemented"
  should read `NO_IMPLEMENT` too; a client that treats it as "wrong object"
  now gets a more accurate answer.

  의도적 미구현은 이제 `NO_IMPLEMENT`로 답한다. 세 답이 와이어에서 세 사실이 되며,
  구분에 문서가 필요하지 않다. IFR만 갖고 있던 규칙을 다섯 서비스 전부에 적용했다.
  **`moe::Router::dispatch`가 핵심이다**: D006이 제외를 승인한 뒤에도 서번트는
  "그런 연산 없음"이라 답하고 있었다 — 산문의 결정을 와이어가 부정한 것.

- **The service sweep decides instead of counting, and fails on an absence.**
  A `BAD_OPERATION` from an object that *claims* the interface — measured by
  whether that object answers any other operation of it — is a servant
  half-serving something it says it is, and there is no longer a document to
  look it up in. An interface no object claims is reported as its own fact.
  `NO_IMPLEMENT` is also no longer counted as *dispatched*, which had
  overstated the IFR facade's served count by **14 operations**, in the
  direction that flatters.

  스윕이 세는 대신 판정하고, 부재에서 실패한다. `NO_IMPLEMENT`를 서빙으로 계수하던
  탓에 IFR의 서빙 수치가 **14만큼** 부풀려져 있었다 — 유리한 쪽으로.

### 🔒 Security / 보안

- **Twelve bytes bought sixty-four megabytes, and an overflow the release
  fuzzer could not see.** Two hazards reachable from peer bytes, both
  reproduced before being fixed.

  `csiv2` added a peer-supplied DER length to a cursor unchecked. Fed
  `60 88 FF FF FF FF FF FF FF FF` it panicked in debug and, in release,
  **wrapped and returned "GSS token is truncated"** — an error message that was
  a lie about what happened. `wire-fuzz` runs `--release`, where overflow
  checks are off, so it was **structurally blind to the class**: its "0 panics"
  was silent about it, not clearing it. The harness now carries the one run in
  this tree that can see an arithmetic overflow at all.

  And a GIOP 1.2 header declaring `message_size = 67,108,863` followed by
  silence committed and zeroed 67,108,875 bytes before a body byte arrived. The
  body is now read in 64 KiB chunks: the same header peaks at 65,548.

- **An array length from an agent's document bought 206 GB.** A 198-byte
  document declaring `array<octet, 4294967295>` as a union discriminator made
  `decode_at` reserve before reading — then refuse the stream as truncated a
  moment later, which is why nothing looked wrong. The `Sequence` arm fourteen
  lines above has carried the guard since Phase 0 with a comment naming the
  rule; the `Array` arm did not need it while every TypeCode it decoded against
  had been compiled here. AnyJSON v1.1 made that length a field in a document
  an agent sends.



- **An argument value a content stage saw could reach the audit ledger.** The
  `SEAT_SAFETY_CONTENT` interceptor seat reads argument *values* — that is what
  it is for — and it can also refuse. Its refusal is `Denied::Intercepted`,
  whose `reason` is free prose written by whatever stage a deployment
  installed, and `AuditInterceptor` rendered that prose verbatim. A content
  filter's most natural sentence names the thing it objected to, so the payload
  landed in the one artifact this crate writes to disk, retains, and greps.

  Measured, not reasoned: a real session with a marker in an argument produced

  ```
  REFUSE caller=alice … why=the safety.content stage refused this call:
         this looked like a credential: {"cents":"pin-s3cret-4242"}
  ```

  The channel was opened on 2026-08-14 by the batch that filled the seat, which
  checked that a stage *could* see the arguments and did not check its own
  second condition — that the audit must not thereby gain a way to log one.
  `guard.rs`'s claim that an audit line "can carry no credential material:
  nothing here holds one" was false from that day and left standing.

  **The ledger now takes the stage's name and drops its prose.** Typed refusals
  are unchanged — their fields are repository ids, operation and scope names,
  quota arithmetic, none of which can hold a byte the agent sent. The full
  sentence still reaches the caller, the dry-run report and every observer
  stage: readers who already hold the arguments. The gate did not move; a
  refusal still precedes anything being sent.

  **A gate that has to see a secret must not thereby be a gate that publishes
  one.** Two harness checks: the property, and the shape — an `audit_entry`
  call site taking a `Denied`'s `Display` — that would reintroduce it.

  내용 좌석은 인자 **값**을 읽는다(그러라고 있는 자리다). 그 좌석의 거부 사유는
  배포자가 설치한 스테이지가 쓴 자유 산문이고, 원장은 그것을 그대로 실었다 —
  디스크에 남고 사람이 grep 하는 유일한 산출물에. 실측: 인자에 넣은 PIN이 `why=`에
  그대로 찍혔다. 좌석을 채운 2026-08-14 배치가 **자기 두 번째 조건을 검사하지
  않았고**, `guard.rs`의 "자격증명은 담기지 않는다"는 주장은 그날부터 거짓이었다.
  이제 원장은 스테이지 **이름**만 싣는다. **비밀을 봐야 하는 게이트가 비밀을
  퍼뜨리는 게이트가 되어서는 안 된다.**

### Decided / 결정

- **D010 — what remains, and which of it cannot be measured here** (PROPOSED).
  Written from the current gap columns rather than from memory, because the
  session that produced it found four "gaps" already closed and four gates
  green while measuring nothing — progress was wrong in both directions. It
  splits the remainder into four classes: **A** buildable and measurable here,
  **B** buildable but the oracle is absent (lands only as a SKIPPED harness
  group naming its fixture, never as `ok`), **C** deferred with a trigger that
  has not fired (building early is the defect), **D** a claim in a document
  that cannot be tested. Six A items ordered by cost of defect, six B items
  each naming what is missing, eleven C items each naming where its reason
  lives, and the five D rows yesterday's plan batch left. Two process
  proposals: a gap-column symbol check, and a rule that a new harness group
  lands with its negative control in the commit message.

  기억이 아니라 현재의 공백 열에서 썼다. 남은 것을 네 부류로 가른다 — 여기서
  짓고 잴 수 있는 것, 지을 수 있으나 오라클이 없는 것(`ok`가 아니라 픽스처를
  이름 붙인 SKIP으로만 착지), 방아쇠 달린 유예(일찍 짓는 것이 결함), 시험 불가한
  문서 주장.

### Added / 추가

- **CosNaming serves `bind_context`, `rebind_context` and `destroy`.** Two of
  the three deferral reasons were **descriptions of the servant rather than
  obstacles**: contexts lived as long as the process *because* nothing removed
  a key, and binding a context this dispatch already serves is a map insert,
  not a call over the wire — it was also the only way the already-served
  `new_context` produced anything reachable. Binding a **foreign** context stays
  deferred with a rewritten reason: it is implementable now, and that is a
  reason it is possible, not a reason to do it.

  A peer drove it — 20 labelled rows, every expected value measured against
  omniNames 4.3.4 first, and two deliberate divergences from omniNames recorded
  (it type-checks neither rebind, and accepts any reference for
  `bind_context`). The property the module rests on is now **checked**: the
  servant names no `Connection`, `Pool`, `Mux`, `invoke`, `TcpStream` or
  `connect(`, and all 16 operations run with nothing held.

  Its negative control changed a test: the lock sweep *passed* with a violation
  planted in `destroy`, because `destroy` at a populated root stops at
  `NotEmpty` and never reaches the removal.

  Landing it meant the generated-skeleton comparison had to follow, and that
  comparison caught the interesting part. `destroy` had sat near the top of the
  script as a deferral both halves refused; once served, it **destroyed the
  root before every other step ran** — both servants identically, so the byte
  comparison stayed green while value-carrying replies fell from 25 to 5.
  **Agreement by mutual destruction**, which is exactly what
  `the_comparison_is_not_vacuous` exists to catch. The lifetime steps moved to
  the end, and the `NOT_COMPARED` entry that recorded an ordering difference
  was **retired rather than deleted**: the difference existed *because*
  `bind_context` was deferred, and it left with the deferral.

- **CosEvent serves the pull model's consumer side** — `obtain_pull_supplier`,
  `connect_pull_consumer`, `pull`, `try_pull`, `disconnect_pull_supplier`. The
  deferral's reason was *"the same unbounded buffer this module spends its
  bounded queue avoiding, for no named consumer"*, and **only the second clause
  survived measurement**: a pull proxy holds events in the same bounded deque,
  moved by the same knob, dropped oldest-first into the same counter. Nine
  pushes into a limit of three give `queued=3, dropped=6` on both sides.

  It **drops at the bound and blocks at the empty end**, deliberately and at
  different ends. Blocking a supplier — CORBA's own answer to a full channel —
  would let one slow puller wedge the channel for every other consumer.
  Blocking a caller that asked to wait is what `pull` means; it is bounded,
  woken by an arriving event, and expires as `TIMEOUT` with **`COMPLETED_NO`**,
  the load-bearing half: nothing was consumed, so calling again is safe.

  The **supplier** side stays deferred with a rewritten reason — there the
  channel is the puller, `PullSupplier::pull` is specified to block, and the
  channel would hold a thread per supplier on somebody else's clock, for no
  named supplier. `destroy` stays too, its reason moved from outbound calls
  (which `guarded` now answers) to authorization: it is an **unauthenticated
  remote operation that ends the channel for every other client**.

  CosEvent 19 → **24 served**, `NO_IMPLEMENT` 9 → 4. **No peer verified any of
  it** — omniEvents is absent and omniORBpy ships no `ProxyPullSupplier` stubs.

- **The sweep was measuring the wrong object.** It probed the pull operations
  against a *push* proxy, because no pull proxy could be obtained when that
  code was written — so the moment they were served it reported the whole
  `ProxyPullSupplier` interface as **unserved**. A false absence produced by
  asking the wrong reference, and one that appears exactly when the underlying
  thing gets better.

- **F5 was already served, so what was missing was the second direction.**
  `PLAN-SERVICES` §10 listed LifeCycle/Property as never started;
  `COMPONENTS.md` had it ✅ since 2026-08-14 and coverage measured 16/16 — the
  fourth "gap" this session that turned out to be closed. What was genuinely
  missing is one section down in that same document: *no cross-ORB direction*.
  An omniORB client now calls **all sixteen** through its own stubs. A hole it
  found: `bind_expert`/`set_policy` take references **no operation returns**.

- **S4 reads an `#include` from the item's own directory.** The thirteen-file
  estate scored **1/13 (8%)** first-pass, each contract refused for a file
  sitting beside it, and nothing was red because `estate/run.sh` amalgamates
  first. **13/13** now, with the exposure worksheet byte-identical to the
  amalgamated one — the equivalence that script had been assuming.

  The shape is a `Source`, not a path: a model writes IDL that was never a
  file. Text with no origin still says so in the same words, pinned by a test.
  Two sites shared the cause, including `DiffOutcome::compared` recording
  **`true` when the baseline failed to parse** — an unmeasured check reported
  as a pass, the third found this session.

- **The property sweep takes every value across AnyJSON and back.**
  `orbweaver-test`'s `one_case` now does `to_json` → text → `from_json` and
  re-encodes in the same byte order; the bytes must equal the CDR-only leg's
  (both encoders and the mapping are ours, so byte equality is legitimate here
  — and not redundant with value equality: `-0.0 == 0.0` yet encodes
  differently). Six `json/*` error classes fail `contract-check`;
  `json/unmapped` (advice) names the types the mapping documents as not
  crossing (`fixed`, `Principal`, `void`), pinned over golden to gc21's
  `Amount`/`Invoice`. The summary line and `--json` print how many CDR round
  trips also crossed (golden 5248 of 5248; 12,928 over four corpora), and the
  harness pins the floor. First pass: **0 findings**; two negative controls
  red (2712 and 70). Closes 1b6b4c8's report that the sweep never called
  `anyjson`.

  **속성 스윕이 모든 값을 AnyJSON으로 건넜다 돌아온다.** `to_json` → 텍스트 →
  `from_json` → 같은 바이트 순서로 재인코딩, 바이트가 CDR 전용 왕복과 같아야
  한다(두 인코더와 매핑 모두 우리 것이므로 바이트 동일이 정당하고, `-0.0`은
  값은 같아도 바이트가 다르므로 값 비교와 중복이 아니다). `json/*` 오류 6종은
  실패, `json/unmapped`는 매핑이 건너지 못한다고 문서화한 타입(gc21의 `fixed`)만.
  요약 줄이 건넌 수를 찍고 하네스가 바닥을 고정(golden 5248/5248, 네 코퍼스
  12,928). 1차 통과 0건; 음성 대조 2건 적색(2712, 70).

- **`--dry-run-handle <name>=<IOR|file>` (repeatable): the CLI's
  value-carrying dry run holds an object reference without dialing it.** The
  IOR is parsed and issued into the session's capability table through the
  serving path's own `issue_checked`; `--dry-run-args` names it as
  `{"_ref":"<name>"}` (D008's notation) and the CLI rewrites the name to the
  issued token before the library sees the document. With it,
  `heartbeat(in Expert e, …)` predicts `allow`/`marshals` from the command
  line; without it, `marshal`/`would_not_marshal` naming the parameter and
  handle — every answer the CLI could give before. The document carries
  `handles`; the target reaches no output and every ledger line is `DRYRUN-`
  (`tests/dryrun_handle.rs`: a listener the test owns is never contacted).
  Closes the gap 4bb9742 reported.

  **`--dry-run-handle <이름>=<IOR|파일>`(반복 가능): CLI의 값 동반 드라이런이
  객체 참조를 다이얼하지 않고 보유한다.** IOR은 파싱만 되어 라이브 경로와 같은
  `issue_checked`로 세션 테이블에 발급되고, `--dry-run-args`는 D008 표기
  `{"_ref":"<이름>"}`으로 이름을 대며 CLI가 라이브러리에 넘기기 전에 토큰으로
  바꾼다. 있으면 `allow`/`marshals`, 없으면 `marshal`/`would_not_marshal`(음성
  대조군). 대상의 host/port/key는 어떤 출력에도 닿지 않고 원장은 전부 `DRYRUN-`.
- **The CLI's dry run takes values, and the static guard's dry run resolves
  the session's handles.** `orbweaver-mcp-server --dry-run=<id>.<operation>
  [--dry-run-args <json>]` asks about one operation with values: the document
  carries `payload`/`payload_why`/`raises` and `would: marshal` past the gate;
  surveys unchanged (`allow=10 need_scope=1 stray=0`). `Guarded::dry_run_with`
  resolves declared handles against the session's table, shared with the
  bridge (`Bridge::handles()` returns `RefMut`; `Bridge` is `!Send`), a forged
  handle predicts `would_not_marshal` naming it, nothing dials. Mapping errors
  inside an argument now name the parameter. `tests/ifr_reaches_the_agent.rs`
  witnesses non-empty sequences and asserts it — the third empty recursive
  witness, closed; nothing went red under it.

  **CLI 드라이런이 값을 받고, 정적 가드의 드라이런이 세션 핸들을 해석한다.**
  `--dry-run=<id>.<operation> [--dry-run-args <json>]`은 연산 하나를 값과
  함께 묻는다 — `payload`/`payload_why`/`raises`가 실리고 게이트 통과 후 `would:
  marshal`; 서베이는 그대로. `Guarded::dry_run_with`는 선언된 핸들을 브리지와
  공유하는 세션 테이블에 대해 해석하고(`Bridge`는 `!Send`), 위조 핸들은 이름을
  붙여 `would_not_marshal`, 다이얼 없음. IFR 증인이 비어 있지 않은 시퀀스를
  만들고 단언한다 — 세 번째 빈 재귀 증인 닫힘.
- **The content seat sees a static call's payload** (D010 A3, 2026-08-19).
  `Guarded` reads a stub's own bytes back through the contract and hands the
  chain the same AnyJSON document the dynamic path hands it — no stub, trait
  or emitted file changed, so stubs already compiled are covered; the
  three-crate `Invoker::invoke` change D010 named was not needed. A payload the
  guard cannot read is refused `MARSHAL`/`BAD_OPERATION` after the gate and
  before the wire, never forwarded. The leak test now has a static arm,
  red-then-green in the same commit. **A dry run can take values**:
  `Bridge::dry_run_with` / `Guarded::dry_run_with` hand them to the content
  seat and predict marshalling from the contract's `TypeCode`s, both byte
  orders, into a dropped buffer (`Would::Marshal`, `payload`, `raises`); a
  `string<8>` given nine characters predicts `marshal` where it predicted
  `allow`. Nothing dials. Pinned rather than fixed: a stub's over-bound
  argument is refused by the stub's probe before the guard hears of it
  (nothing sent, nothing audited).

  **정적 호출의 페이로드가 내용 좌석에 도달한다** (D010 A3). 가드가 스텁이 쓴
  바이트를 계약에 따라 되읽어 동적 경로와 같은 AnyJSON 문서를 체인에 건넨다 —
  스텁·트레이트·생성 파일은 바뀌지 않았으므로 이미 컴파일된 스텁도 적용되며,
  D010이 이름 붙인 세 크레이트 변경은 필요 없었다. 읽을 수 없는 페이로드는
  게이트 뒤·와이어 앞에서 `MARSHAL`/`BAD_OPERATION`으로 거절되고 전달되지
  않는다. 누출 시험에 정적 경로 팔이 같은 커밋에서 red→green으로 추가되었다.
  **드라이런이 값을 받는다**: 내용 좌석에 값을 건네고 같은 `TypeCode`로 양쪽
  바이트 순서 마샬링을 예측한다(`Would::Marshal`); `string<8>`에 아홉 글자는
  `allow`였던 자리에서 `marshal`을 예측한다. 아무것도 다이얼하지 않는다.
  고치지 않고 못 박은 것: 스텁의 상한 초과 인자는 가드가 듣기 전에 스텁의
  프로브가 거절한다.
- **Fallback-on-failure oracle for `LOCATION_FORWARD` vs
  `LOCATION_FORWARD_PERM`** (`spikes/perm_fallback.sh`,
  `crates/orbweaver-gen/tests/forward_fallback.rs`): two servers, the
  forwarded-to one killed by PID. omniORB 4.3.4 restarts at the original after
  a temporary forward (§9.6 "shall") and stays on the dead address after a
  permanent one (§9.6 "may replace") — the first peer measurement that tells
  the two statuses apart, and through which a server that downgrades status 4
  to 3 goes red. Ours measured and pinned: `Connection` returns Err under both
  (holds no original address); `Reference` re-asks the original on every call
  under both (never caches a forward, never replaces on permanent).
  `spike-server` gains `ORBWEAVER_FORWARD_TO/_STATUS/_PING_ANSWER`.

  **`LOCATION_FORWARD`와 `LOCATION_FORWARD_PERM`을 구별하는 장애-시-복귀
  오라클**(`spikes/perm_fallback.sh`, `tests/forward_fallback.rs`): 서버 두 대,
  전달받은 쪽을 PID로 종료. omniORB 4.3.4는 임시 전달 뒤에는 원래 주소로
  되돌아가고(§9.6 "shall") 영구 전달 뒤에는 죽은 주소에 머문다(§9.6 "may
  replace") — 두 상태를 구별하는 최초의 피어 측정이며, 상태 4를 3으로 낮추는
  서버가 이것으로 빨갛게 된다. 우리 쪽: `Connection`은 두 상태 모두 Err(원래
  주소를 갖고 있지 않음); `Reference`는 두 상태 모두 매 호출마다 원래 주소에
  다시 묻는다.
- **The pool says which forward it followed.** `mux::Sent::Forward` carries
  `Forward`, not a bare `Ior`; `Pool::invoke_tracking` returns the reply and
  the last hop followed; `Reference::forwarded()` is the pooled
  `Connection::forwarded()`. Following is unchanged. Measured: 2 kinds × 3
  versions × 2 reply byte orders against a scripted peer, plus the same matrix
  through our own `Server` (native order) — permanent reported only for 1.2 ×
  permanent. Negative controls: interpret forced all-temporary and
  all-permanent, `Reference` recording nothing — each red once.

  **풀이 어떤 포워드를 따랐는지 말한다.** `mux::Sent::Forward`가 `Ior` 대신
  `Forward`를 나른다. `Pool::invoke_tracking`은 응답과 마지막으로 따라간 홉을
  돌려주고, `Reference::forwarded()`는 `Connection::forwarded()`의 풀 판이다.
  따라가는 동작은 그대로다. 측정: 스크립트 피어로 2종 × 3버전 × 2응답 바이트
  순서, 같은 행렬을 우리 `Server`로(네이티브 순서) — 1.2 × permanent에서만
  permanent로 보고. 음성 대조: 전부 temporary/전부 permanent 강제, `Reference`
  미기록 — 각각 한 번 빨강.
- **`LOCATION_FORWARD_PERM` is reachable from a servant** (D010 A1).
  `Forward { Temporary, Permanent }` and a defaulted `redirect` on
  `Dispatch`/`SharedDispatch` (giop) and on every generated servant trait
  (gen); `Served::Forward` carries it and the server puts status 4 on the wire
  to a 1.2 peer, 3 to a 1.0/1.1 peer whose status enumeration has no 4.
  `Connection::forwarded()` reports which a client followed. Measured: raw
  status off the wire from a generated skeleton, both byte orders, all three
  versions; omniORB 4.3.4 following our status 4. Request count at the old
  reference is **1 under both statuses** for our client and for omniORB — the
  count is not an oracle, the status byte is. `Servants` delegates `redirect`
  explicitly; the trait default was silent through it. D010 A1's review
  correction was itself wrong (`rt::Dispatch` is giop's trait); the batch was
  giop + gen, and D010 §3 now says so.

  **`LOCATION_FORWARD_PERM`을 서번트가 말할 수 있다** (D010 A1). `Forward {
  Temporary, Permanent }`와 기본 구현이 있는 `redirect`가 giop의
  `Dispatch`/`SharedDispatch`와 gen의 모든 생성 서번트 트레이트에 추가됐다.
  서버는 1.2 피어에게 상태 4를, 상태 열거형에 4가 없는 1.0/1.1 피어에게는 3을
  보낸다. `Connection::forwarded()`로 클라이언트가 무엇을 따랐는지 알 수 있다.
  측정: 생성 스켈레톤에서 와이어로 읽은 원시 상태값(양쪽 바이트 순서, 세 버전),
  omniORB 4.3.4가 우리의 상태 4를 따름. 옛 참조에 도달한 요청 수는 우리
  클라이언트와 omniORB 모두 **두 상태에서 1로 같다** — 횟수는 오라클이 아니고
  상태값이 오라클이다. D010 A1의 검토 정정 자체가 틀렸고(`rt::Dispatch`는
  giop의 트레이트), 배치는 giop + gen이었다.

- **moe v1.1 — the Capability gap closed additively (D010 A2).**
  `corpus/golden/22` gains `MeasuredCapability` (composes the released
  `Capability` + `specialization` + `latency_p50_ms`) and
  `ExpertRegistry::register_measured` / `heartbeat_measured`; `idl-diff` exit 0
  against the frozen release `corpus/evolution/moe/v1.0`, exit 1 for the
  in-place edit kept at `corpus/evolution/moe/v1.1-in-place` — both in the
  harness. `orbweaver-trading`: `Selection::unranked` + `is_complete()` — an
  offer with no value for the `ORDER BY` field is set aside and named, no
  longer sorted last (which still picked an unmeasured expert when nothing was
  measured). `orbweaver-object`: both operations served, both byte orders; a
  v1.0 `heartbeat` keeps the two members it cannot mention (it used to erase
  them, and an out-of-band `declare_specialization` with them — same root
  cause: a message with no room for a fact cannot withdraw it). `spike-experts`
  windows 4–5: refused → refused with the unmeasured one named → complete
  answer by measurement. The generated coverage block moved 12→14 declared on
  the control plane by itself, which is what it is for.

  **moe v1.1 — Capability 간극을 추가만으로 닫음 (D010 A2).** golden 22에
  `MeasuredCapability`와 `register_measured`/`heartbeat_measured` 추가; 동결
  릴리스 `corpus/evolution/moe/v1.0` 대비 `idl-diff` exit 0, 제자리 수정
  대조군 exit 1 — 둘 다 하네스에. trading: `Selection::unranked`·
  `is_complete()` — 순위를 매길 수 없는 오퍼는 마지막이 아니라 따로 명명.
  object: 두 오퍼레이션 양쪽 바이트 순서로 서비스, v1.0 heartbeat는 언급할 수
  없는 두 멤버를 지우지 않음. spike-experts 4·5번 창: 거부 → 부분 측정 거부 →
  측정에 의한 완전한 답.

- **`_rt.py` reads and writes AnyJSON v1.1's structural `_t`** (D010 A4). An
  `any` carrying a struct, enum, union, exception or typedef crosses to Python
  and back; a type the package never declared is synthesised from the document
  and registered, so no prior copy is needed at the reader — the property D008
  chose the structural form for, now true of both implementations. Generated
  modules carry each type's IDL name (`_idl_name`, `register_alias(id, desc,
  name)`, `register_name`) because a rebuilt TypeCode names the type beside
  its id. Round-trip sweep: golden 78/132 → **158/132**, services 35/46 →
  **70/46**, 0 divergences; the new cases are red against the old runtime with
  the D008 refusal (85 + 35 divergences), and the harness now pins the counts.
  Found on the way: `anyjson` (Rust) does not resolve `TypeCode::Recursive`,
  so a non-empty value under a recursion marker cannot cross the reference
  mapping — reported, not fixed here.

  **`_rt.py`가 AnyJSON v1.1의 구조적 `_t`를 읽고 쓴다** (D010 A4).
  struct·enum·union·exception·typedef를 실은 `any`가 Python으로 건너갔다
  돌아온다. 패키지가 선언한 적 없는 타입은 문서에서 합성해 등록하므로 읽는
  쪽에 사본이 미리 있을 필요가 없다 — D008이 구조적 형식을 택한 이유가 이제
  두 구현 모두에서 참이다. 왕복 스윕: golden 78/132 → **158/132**, services
  35/46 → **70/46**, 불일치 0; 새 케이스는 이전 런타임에 대해 D008 거부로
  빨간불(85 + 35). 도중 발견: Rust `anyjson`이 `TypeCode::Recursive`를 풀지
  않아 재귀 마커 아래의 비어 있지 않은 값은 기준 매핑을 건너지 못한다 — 보고만
  하고 여기서 고치지 않음.
- **`docs/SERVICES-COVERAGE.md` §8 is generated from the sweep and diffed by
  the harness** (D010 A5, batch 1). `spikes/coverage_tables.py` renders
  `service_sweep.sh --raw` into per-service tables, totals, the interfaces no
  object claimed, and — new — the interfaces the IDL declares that the sweep
  **probed against no object** (`BindingIterator`, 21 of `ir.idl`'s), which
  used to be silent and read as coverage. `--check` fails the harness with the
  diff when the wire and the file disagree; the fix is to regenerate, never to
  edit the block. §3–§7 are re-framed as the dated first reading kept for the
  reasons quoted there, and their headings lose the counts they carried by
  hand: every one was stale, and D010 A5's own sentence about "§2's naming row
  says 13 of 16" named a number that occurs in no document — the class it was
  describing, in the description. Negative control: one served count edited in
  the block → `FAIL … no longer says what the wire says` with the one-line
  diff; regenerated → ok.
- **`spikes/gap_symbols.py`** — D010 §7.1's report, deliberately not a gate:
  per `COMPONENTS.md` gap row, the symbols it names and whether each exists in
  its crate. Measured today: 12 of 12 exist, all legitimately — which is the
  number that demoted it from a gate, printed at the bottom of every run.

  **`SERVICES-COVERAGE.md` §8을 스윕이 생성하고 하네스가 diff한다.** 선언되었으나
  어떤 객체에도 프로브하지 않은 인터페이스가 처음으로 드러난다(전에는 침묵이
  커버리지로 읽혔다). §3–§7의 손으로 적은 수치는 모두 낡아 있었고 제목에서
  뺐다. `gap_symbols.py`는 게이트가 아닌 리포트 — 오늘 12/12가 존재하며 그
  숫자가 게이트에서 강등한 이유다.

- **`idl-diff` resolves what it is asked to diff.** The §5.3 release gate was
  given two revisions whose root file is byte-identical and whose two breaking
  changes both live in the header they share, and it printed *"no change"* and
  exited **0**. One call — `orbweaver_idl::parse`, the string entry point —
  across **19 sites**, of which 12 used `parse` (silent) and 6 used `check`
  (loud about the include and equally unable to resolve it). All are now
  decided per site, with the ones that correctly stay on a raw parse carrying
  their reason. An unresolvable include is **exit 2**, never a verdict: a diff
  of two partial graphs says nothing about the contracts.

  `orbweaver-mcp-server` turns out not to have served an estate at all — it
  refused to start, and `spikes/estate/run.sh`'s amalgamation step was a
  load-bearing workaround that read as a convenience.

  Measured cost of the old silence: stripping `#include` from the thirteen-file
  estate drops **27 references without a word** — 8 base interfaces and 19
  raised exceptions.

- **The estate goes in as the thirteen files it is stored as.** `run.sh`
  amalgamated them first, and that step turned out to be **load-bearing rather
  than a convenience**: without it `orbweaver-mcp-server` did not serve less, it
  refused to start. A direct stage now runs beside the amalgamated one and the
  two are measured to agree — 12 interfaces, 76 operations, identical interface
  by interface (and, as it happens, identical as whole documents, recorded but
  not asserted, because the two inputs genuinely differ).

  `amalgamate.py`'s docstring had rotted in place: it said the front end
  *skips* `#include` and cited a line of `lex.rs` that today reads *"it used to
  be in that list, and being in it was a defect… It is resolved before the
  lexer runs now."* The script stays, for a reason that is still true — some
  consumers take a translation unit rather than a path — and stage 7b measures
  the equivalence instead of assuming it.

  Two more things were found to rest on the amalgamation without saying so:
  `forge-pipeline`'s S4 supplies its item as **text**, which has no directory
  for a quoted `#include` to resolve against, so pointed at the thirteen files
  it exits 1 — the same class as the nineteen call sites, reported and not
  fixed here; and `gen-corpus`'s output compiles against a hand-written servant
  written for the single amalgamated module, which is a genuine dependency and
  now stated.

- **DynAny**, over `Value`/`TypeCode`: navigation whose cursor is a path
  re-resolved at every operation, so nothing exists below the focus and
  past-the-end is representable but never readable. 76 of 78 golden types are
  taken apart and reassembled into identical CDR, both byte orders, all eight
  alignment phases; the two it cannot are `fixed`, deferred by §4.4, and the
  test asserts that list is exactly those two.

  Its first oracle was worthless and only mutation showed it: the source value
  was generated with DynAny too, so breaking `next()` to skip every second
  component **still passed**. A producer and a consumer sharing a defect agree
  about the result.

- **`agent-fuzz`**, for the parsers a `tools/call` reaches. AnyJSON v1.1 put a
  recursive parser on the agent boundary and nobody had fuzzed it, including
  whoever wrote it. Seven targets, zero panics over 50k/50k/200k at three
  seeds; the one finding was the array reservation above.

- **CSIv2 fuzz targets, and the reach that made them worth having.** Two of the
  three were reached **zero times in 50,000 cases** before seeding — the
  green-and-worthless case, stated. Twenty seeds took them to 659 / 1930 /
  2811, and the dilution that cost the GIOP half is reported rather than
  hidden. Twenty-five hostile literals run on every invocation whatever
  `--cases` says, because a class the shipping build cannot see should not also
  depend on a random draw.

- **`call-bench`**, the LAN echo benchmark §8 has cited since v0.2 and did not
  have. One loopback connection shared by both clients, calls interleaved with
  the order swapped, every sample checked against the expected answer. The
  dynamic path costs **+2.0 µs p50 on a 64-string payload (1.06×)** — about
  31 ns per string, and per *string* rather than per byte.

- **A generated skeleton is compared to the hand-written servant it must
  match**, byte for byte: 59 scripted steps × 2 byte orders over CosNaming,
  every structured reply decoded back by `orbweaver-giop`'s own readers,
  because two servants can agree on wrong bytes. It is what forced D009's L2
  early — the naming server began publishing `TAG_CODE_SETS` and the generated
  reference did not.

- **`#include` inside a module**, which no corpus file had. Eight new roots
  produced 32 repository ids, **7 of which diverged from omniidl — and JacORB
  agreed with omniidl against us on all seven.** The resolver had implemented a
  file boundary as an injected `#pragma prefix`, and prefix *replaces* the id
  path, so it could express neither half of a save/restore once the path held a
  module.

- **A decision's status is checked, not just written.**
  `spikes/decision_status.py` reads the authoritative status out of each
  `docs/decisions/D00N-*.md` and holds every other mention to it, in the
  harness. Dated records — `pipeline-runs/`, `PHASE*.md`, released sections of
  this file — are out of scope by construction: they state what was true on a
  date, and editing them to match today would falsify them rather than repair
  them. The gate also refuses a citation to a decision that does not exist.

  결정의 상태는 `docs/decisions/`에 한 번만 산다. 나머지 언급은 하네스가 그것과
  대조한다. 날짜가 붙은 기록(실행 기록·PHASE·릴리즈된 절)은 그 시점의 사실이므로
  범위 밖이다 — 오늘 기준으로 고치는 것은 수리가 아니라 위조다.

### Fixed / 수정

- **The §5.3 differ compares a union's members by role, not by position.**
  Labelled cases by label, the default member by `default_index` — type, then
  name — wherever it sits; a discriminator type change is one BREAKING
  finding. Before: the default's empty label was compared as a discriminator
  value and member types were zipped positionally, so a frozen TypeCode of the
  pre-expansion folded shape against today's expanded one read "case
  added"/"case removed" with nothing changed on the wire, and a default
  retyped behind an inserted case was **missed** (`corpus/evolution/
  union-default/`, exit 1 naming one of two edits). New verdicts, reasoned
  from the generated stub (omniORB reads no member after an unlabelled
  discriminator and raises nothing — the brief's "compatible" guess was
  measured wrong): default added → conditionally breaking, default removed →
  BREAKING, member renamed → compatible (the name does not travel).

  **§5.3 differ가 union 멤버를 위치가 아니라 역할로 비교한다.** 라벨 있는
  case는 라벨로, default 멤버는 `default_index`로 — 어디에 있든 타입, 그다음
  이름 — 비교하고 판별자 타입 변경은 BREAKING 한 건이다. 이전에는 default의 빈
  라벨을 판별자 값처럼 비교하고 타입은 위치로 zip해서, 접힌 모양의 동결
  TypeCode와 확장된 모양이 "case 추가/제거"로 읽혔고, case를 앞에 끼워 넣고
  default 타입을 바꾼 리비전에서 타입 변경을 **놓쳤다**. 새 판정 — 생성 스텁
  기준(omniORB는 라벨 없는 판별자 뒤에서 멤버를 읽지 않고 아무것도 던지지
  않는다): default 추가 → 조건부 파괴, 제거 → BREAKING, 이름 변경 → 호환.

- **`spike-events` read a counter another thread was about to increment.**
  Its phase-2 "the dead consumer's backlog must be counted as dropped" check
  snapshotted `stats.dropped` the instant `disconnected_for_failure` read 1,
  while the relay thread counts the backlog *after* disconnecting; every other
  phase-2 condition waited with a deadline, this one did not. It fired once on
  a loaded CI runner (run for 46ccaae) and passed on the next commit with the
  same code — a race, not a transient: it now waits like the rest. The
  harness's "holding event channel never came up" line prints the fixture's
  log tail so the next such failure has something to read; that half did not
  reproduce and is not claimed diagnosed.

  **`spike-events`가 다른 스레드가 막 올리려는 카운터를 읽었다.** 죽은
  소비자의 백로그를 dropped로 세는 검사가 `disconnected_for_failure`가 1이
  되는 순간 스냅샷을 찍었는데, 릴레이 스레드는 연결을 끊은 *뒤에* 센다 —
  2단계의 다른 조건은 모두 기한을 두고 기다렸고 이것만 아니었다. CI 러너에서
  한 번 발화(46ccaae), 같은 코드의 다음 커밋에서 통과 — 이제 나머지처럼
  기다린다. "holding event channel never came up" 줄은 픽스처 로그 꼬리를
  찍는다; 그 절반은 재현되지 않았고 진단됐다고 주장하지 않는다.

- **The live dynamic call's marshalling error names the argument and the path
  inside it** (`at key.tag[2]: string is bounded at 8 but 9 were given`), on
  the marshaller's own `Path`; `encode_named`/`decode_named` (+`_with`) added
  to `orbweaver-dynamic`, and the mcp dry run uses the same call so a
  prediction and a refusal read alike (it used to prepend the name itself,
  joined with a dot the live path never wrote). Reported by f47ddcd.
- **A union branch that is both labelled and `default:` keeps its labels** —
  in the Python emitter, the Python runtime (form reader and writer) **and the
  Rust emitter, which had emitted the variant twice and did not compile**;
  `corpus/golden/29-labelled-default.idl`; Python sweep 158/132 → **170/137**,
  0 divergences, pins moved. Reported by 50a4d12. **Found on the way, not
  fixed here (a fix batch is in flight):** a union TypeCode with a bare
  `default:` encodes zero label bytes while the decoder reads discriminator
  width — our own encode→decode of golden 06's `WithDefault` TypeCode fails,
  never red because every comparison ran both sides through the same encoder;
  and the registry collapses `case 1: default:` to one member where omniidl
  produces two.

  **동적 호출의 마샬링 오류가 인자 이름과 내부 경로를 명시한다**(`at
  key.tag[2]: …`), 마샬러의 `Path` 그대로; `encode_named`/`decode_named` 추가,
  mcp 드라이런도 같은 호출을 써서 예측과 거절이 같은 문장을 쓴다. **라벨과
  `default:`를 동시에 가진 유니온 분기가 라벨을 유지한다** — Python 이미터·
  런타임·**Rust 이미터**(변형을 두 번 내보내 컴파일되지 않았다) 모두;
  `corpus/golden/29`; Python 스윕 158/132 → **170/137**. 도중 발견(수정 배치
  진행 중): 맨 `default:` 케이스의 유니온 TypeCode가 라벨 바이트를 0개
  쓴다 — 우리 자신의 encode→decode가 실패하는데 양쪽이 같은 인코더를 지나므로
  한 번도 빨갛지 않았다.

- **Two class-B rows were `note` lines the verdict did not count.** D010 §2
  says every B row lands as a `SKIPPED — unmeasured, not passing` group; B2
  (identity through a real provider) and B3 (SSLIOP against a peer) were
  prose after an `ok`. Both are counted SKIPPED groups now, each naming its
  fixture: a peer advertising a CSIv2 mechanism list plus an issuer
  (`ORBWEAVER_IDP_URL`) — and FAIL, not ok, on the day both exist and nothing
  measures them; `from omniORB import sslTP` as the probe, with a distinct
  line if the module is present somewhere and the peer proof still is not
  built. Harness SKIPPED count 3 → 5. **The first version of that probe was
  itself the gate-green-measuring-nothing class:** it grepped its own marker
  out of an `ImportError` traceback that echoes the source line, and reported
  `sslTP` present where it is not — caught on the first run by reading the
  harness's line against the shell's; the probe is the interpreter's exit code
  now.

  **B류 두 행이 판정이 세지 않는 `note`였다.** B2(실제 제공자 통한 신원)와
  B3(피어 대상 SSLIOP)는 `ok` 뒤의 문장이었다. 이제 둘 다 픽스처를 이름 붙인
  SKIPPED 그룹이며, 픽스처가 있는데 재지 않는 날에는 ok가 아니라 FAIL이다.
  하네스 SKIPPED 3 → 5.

- **AnyJSON: a value beneath a `TypeCode::Recursive` marker now crosses in
  both directions** (found by D010 A4; `to_json`/`from_json` resolved aliases
  only, so `Reading { kids: sequence<Reading> }` with a child failed "is not a
  value of"). The mapping walks on the marshaller's own `Path`, so recursion
  and the 64-level nesting bound are one mechanism for CDR and JSON; a
  document nesting past the bound is refused, not followed. **Why nothing was
  red:** every recursive witness was the empty list — the property sweep's
  `TreeSeq` was `[]` on every valued case and `None` on 22 of 32 (skipped
  through a bare `continue` while the summary line said 32), and the Python
  sweep terminated at the first re-entry. The sampler's depth predicate now
  mirrors the sampler, a valueless case is a `prop/unmeasured` finding the
  harness fails on, and the Python witness follows a marker one level (all
  five Python tests pass; `_rt.py` needed nothing). Found and not fixed: the
  property sweep never calls `anyjson`, so it could not have caught this
  regardless of witness; a third empty recursive witness lives in
  `orbweaver-mcp/tests/ifr_reaches_the_agent.rs`.

  **AnyJSON: `Recursive` 마커 아래의 값이 양방향으로 건너간다**(D010 A4 발견;
  `to_json`/`from_json`은 별칭만 풀었다). 매핑이 마샬러의 `Path` 위를 걷게 되어
  재귀와 64단계 중첩 한도가 CDR·JSON 하나의 메커니즘이 되었다. **왜 아무것도
  빨갛지 않았나:** 모든 재귀 증인이 빈 목록이었다 — 속성 스윕의 `TreeSeq`는
  값 있는 모든 케이스에서 `[]`, 32건 중 22건은 `None`(요약 줄은 32라 말하며
  조용히 건너뜀). 샘플러의 깊이 술어를 샘플러와 일치시키고, 값 없는 케이스는
  하네스가 실패시키는 `prop/unmeasured` 소견으로 남기며, Python 스윕 증인은
  마커를 한 단계 따라간다. 보고만: 속성 스윕은 `anyjson`을 호출하지 않는다;
  세 번째 빈 재귀 증인이 `ifr_reaches_the_agent.rs`에 있다.

- **README — the sixth class-D row the PLAN batch named.** The S1–S7 stage
  table lost its *Target* column (95/90/80/100/100/85/90 — now only PLAN §11
  A9); *Targets / 목표 지표* no longer copies PLAN §11 without its instrument
  column and points there instead; the Phase 0 bullet B stops carrying the
  spike threshold (≥ 60 / ≥ 95 %, home `PHASE0.md`) beside a plan that carries
  the standing target (≥ 85 / ≥ 98 %, home PLAN §11) — two facts, each now
  with one home. `docs/plan-page.html` (hand-maintained, dated v0.3) gets a
  kept-as-written card and its TAO/omniORB/JacORB line annotated historical,
  mirroring PLAN §12. Headings 18 → 18.

  **README — 계획서 배치가 지목한 클래스 D 여섯째 행.** S1–S7 단계 표의 *목표*
  열 삭제(계획서 §11 A9만 보유); *목표 지표* 절은 계측기 열 없이 §11을 베끼던
  표를 지우고 그곳을 가리킴; Phase 0 가정 B 줄은 스파이크 기준값(≥ 60 /
  ≥ 95 %, 집은 `PHASE0.md`)과 상시 목표(≥ 85 / ≥ 98 %, 집은 계획서 §11)를
  각각의 집으로 돌려보내고 어느 수치도 담지 않음. `docs/plan-page.html`
  (수작업, v0.3 날짜)에는 기록 카드와 TAO 줄 역사 주석. 헤딩 18 → 18.

- **PLAN §5, §8, §11, §12 — the five class-D rows D010 §6 named.** The §5
  *Automation target* column (seven percentages, no instrument) moved to §11
  as aspiration A9; *contract tests auto-generated* audited (nothing generates
  a test) and kept at **none** with A8; the first-pass / three-rounds rows
  restated against `forge-pipeline`'s actual per-stage `first-pass:` /
  `rounds:` / `result:` lines; the §8 CDR row no longer asks for byte-identity
  against a reference ORB — it names the decoded-value, re-encoded-outside-
  padding and derived-TypeCode comparisons that exist; §12 action 3 annotated
  as historical with A6 / D010 B6 as the trigger's home. Found and not fixed:
  `README.md` restates both target tables without instruments, and its
  assumption-B targets (≥ 60 % / ≥ 95 %) differ from PLAN's — a sixth row of
  the class, outside this batch.

  **PLAN §5·§8·§11·§12 — D010 §6이 지목한 클래스 D 다섯 행.** §5의 *자동화
  목표* 열은 §11의 지향 A9로 이동; *계약 테스트 자동 생성률*은 감사 후(테스트를
  생성하는 것이 없음) **없음**으로 유지하고 A8; 1차 통과율·3회 내 통과율 행은
  `forge-pipeline`이 실제로 찍는 단계별 줄에 맞춰 재기술; §8 CDR 행은 더 이상
  참조 ORB 대상 바이트 동일을 요구하지 않고 실재하는 비교 셋을 명시; §12
  액션 3은 역사적 기록으로 주석. `README.md`가 같은 표를 계측기 없이 다시
  적고 있음 — 같은 클래스의 여섯째 행, 이 배치 밖.

- **CI had been red for ten consecutive runs while the local harness was
  green, on three causes, none of them in the code.** Found by reading the
  runs before planning the next batch rather than after landing it.

  1. `spikes/union_label_capture.py` compared the recorded omniORB union
     TypeCodes to the live peer **byte for byte, exempting only offsets 9..12**
     — the padding the local peer happened to leave non-zero. CI's omniORB,
     built from source on Linux, leaves different garbage after the
     repository-id string and before every 8-aligned label (`U` at 29; `W`
     at 52, 76, 118, 119), and the script called that a change in the peer.
     Every one of those offsets is padding the specification says nothing
     about — CLAUDE.md's *compare decoded values, never raw buffers* rule,
     broken by a script written the same week the rule was cited. The mask is
     now **derived by walking the encapsulation** (§9.4.2 layout, alignment
     restarting at the encapsulation's first byte); a byte the walker does not
     understand raises rather than being treated as padding. Negative control:
     flipping a repository-id byte → `FAIL … differ at [16]`; flipping byte 29
     → `ok`.
  2. The R7 endpoint-rewriting fixture bound a **fixed port inside Linux's
     ephemeral range** (40404 ∈ 32768–60999). The harness makes a few thousand
     outbound connections first, so in two of ten runs the kernel had already
     lent that port to one of them: *"Failed to bind to address :: port
     40404. Address in use?"* Now 24404, below both kernels' ranges. Negative
     control: holding 24404 reproduces the exact CI text; releasing it, the
     fixture serves.
  3. `spikes/jacorb/setup.sh` fetched five jars with a single `curl -f`; one
     transient mirror error (exit 22) failed the whole interop job. `--retry 3`.

  **CI가 열 런 연속 빨강이었고 로컬 하네스는 초록이었다 — 원인 셋, 코드에는
  없음.** 유니온 레이블 캡처는 패딩을 오프셋 목록(9..12)으로 예외 처리했고
  CI의 omniORB는 다른 자리에 쓰레기를 남겼다 — 이제 인캡슐레이션을 걸어서
  마스크를 만든다. R7 픽스처의 고정 포트가 리눅스 임시 포트 범위 안에 있었고
  (40404), 이제 두 커널 범위 아래(24404). JacORB jar 다운로드에 재시도.

- **The §5.3 release gate accepted a struct member with no type, and refused a
  contract both oracles accept.** Two faces of one omission, found by asking
  what a marker means rather than by fixing what was reported.

  `Registry`'s resolver joined an unqualified name onto *lexical* prefixes
  only, so a `raises` declared in a base interface did not resolve from a
  derived one. `corpus/services/gen-naming-subset.idl` — `NamingContextExt :
  NamingContext`, exactly as the OMG writes it — produced five unresolved
  markers and **exit 2**, while `omniidl` and JacORB both accept the file. A
  gate that cries wolf gets bypassed, which makes that worse than the defect
  the include work had just fixed.

  CORBA 3.4 §7.19.2 fixes the order and the order *is* the rule: an
  unqualified name is searched in the current scope, then in **inherited**
  scopes, then outward through enclosing ones — so a base's declaration beats
  an enclosing module's declaration of the same name. A base's base counts, a
  diamond contributes one name, and a cycle terminates.

  Asking what `Unresolved` meant then found the other face. It recorded bases
  and `raises` and **not types**, so an unresolved type name silently became
  `TypeCode::Void`: `module n04 { struct S { Widget w; }; };` diffed as *"no
  change … nothing here breaks a deployed peer"*, **exit 0**. `void` marshals
  nothing where a peer expects a value. The marker now means one thing — *this
  translation unit declares no such name* — and both cases exit 2.

  The negative control is the point of the design: a **sibling** interface
  using another's `typedef` must still fail, because a resolver that "fixed"
  inheritance by searching every interface in the unit passes every positive
  case and only fails that one. Both oracles reject it too.

  Nothing had ever run `idl-diff` over the corpus, which is why a gate could
  refuse a valid contract unnoticed. The harness does now, with both controls.

  §5.3 게이트가 **타입 없는 구조체 멤버를 통과시키고**, 두 오라클이 받아들이는
  계약을 거부하고 있었다. 하나의 누락이 가진 두 얼굴이다 — 보고된 것을 고치는
  대신 *마커가 무엇을 뜻하는가*를 물어서 나왔다. 음성 대조군이 설계의 핵심이다:
  **형제** 인터페이스의 스코프는 여전히 새면 안 된다.

- **Ten stale status claims, and the four remaining-work lists no gate can
  see.** Four decisions the user approved on 2026-08-14 — D003, D004, D005,
  D006 — were still being called open in five documents. `PLAN.md` §7.2 still
  listed three Phase 4 items as remaining — a servant that cannot raise a system exception,
  a missing server-side static-equals-dynamic oracle, and no Python target —
  when all three had landed, and stream E carried an un-struck second copy of
  its own last three items through three passes. Nothing here was a code
  defect and nothing could go red: the measured cost was **a planning pass
  spent proposing work that was already finished.** §7.2 no longer restates a
  remaining-work list at all; it links the `COMPONENTS.md` row that owns one.

  다섯 문서가 이미 승인된 결정 넷을 PROPOSED로 불렀고, §7.2는 이미 착지한 세
  항목을 남은 일로 열거하고 있었다. 코드 결함은 하나도 없고 빨개질 수 있는
  것도 없었다 — 실측된 비용은 **이미 끝난 일을 다음 작업으로 제안한 계획 한
  번**이다.

- **`D003` said APPROVED in English and 제안 in Korean, four lines apart.** The
  approval edit overwrote the head of its own PROPOSED block and left the tail,
  in the file that is the source of truth — and every document that copied it
  copied the English half. The gate now requires a decision's status markers to
  agree with each other before it checks anyone against them.

- **The gate's own two blind spots, found by running it.** It read documents
  line by line, so a claim whose reference and status word fell either side of
  a markdown wrap was invisible — `PLAN-MOE.md` passed while saying the wrong
  thing, and only its Korean twin was caught, because that one happened not to
  wrap. It also attached every status word to every decision named in the same
  sentence, which reported two correct sentences as drift. Passages are now
  built per paragraph, and a status word binds to the decision most recently
  named before it. First run 6 findings, after both repairs 10, false positives
  0. It then caught the first draft of this very entry, which quoted the old
  status without naming the new one — the entry was reworded rather than the
  gate loosened.

---

### Known limits / 알려진 한계

- **The `char` conversion list stays empty, and that is now measured rather
  than cautious** (D009 §8 row 4, **BLOCKED**). The row conditioned a non-empty
  list on a peer that cannot reach UTF-8. Eleven configurations of the two
  installed ORBs were probed and ten measured: **every one reaches UTF-8**.
  Neither ORB exposes an option that *names* its conversion list — omniORB has
  `nativeCharCodeSet`/`defaultCharCodeSet` and accepts only ISO-8859-1 and
  UTF-8 as a native set; JacORB's list follows its build. The one setting that
  removes UTF-8, `jacorb.codeset=off`, removes the component entirely, and an
  absence is not an advertisement.

  What growing the list *would* cost was measured too, and it is not nothing.
  Offered ISO-8859-1 from a listener we wrote, against clients we did not:
  omniORB keeps sending UTF-8, and **JacORB configured native ISO-8859-1 moves
  down to it** — `café` as `63 61 66 e9` instead of `63 61 66 c3 a9`, and
  `함정 전투체계` as **each character truncated to its low octet, raising
  nothing**. §7.10.2.6 leaves that case open and the two ORBs resolve it in
  opposite directions; the empty list is what keeps the ambiguity unreachable.
  A guard test fails if anyone grows the list, and names the probe to run
  first.

  광고하지 않는 편이 옳다는 것이 **조심이 아니라 측정**이 되었다. 설치된 두 ORB의
  열한 구성 중 열을 측정했고 전부 UTF-8에 닿는다. 그리고 목록을 키웠을 때의 비용도
  쟀다: JacORB는 제안하는 즉시 **내려가고**, 한글을 **각 문자의 하위 옥텟으로 잘라
  보내면서 아무 예외도 올리지 않는다.**

- **`_rt.py` reads only a named type in an `any`'s `_t`.** The Rust half reads
  and writes the structural form; the Python half refuses it by name, with the
  decision cited, rather than accepting the document and marshalling `_v` as
  something else — the same rule the Rust side follows, applied to whichever
  implementation is behind.

## v0.4.0 — 2026-08-14

The release a **consumer-shaped input** produced. Thirteen legacy contracts
that include each other, four `#pragma prefix` styles, an acquired company's
prefix inheriting our base, one file with no prefix at all, and nothing
annotated anywhere — the shape an estate actually has, rather than the shape
`corpus/` has. It found eight root causes in one pass, **six of which no test
in this repository could ever have gone red on**, because every corpus file is
self-contained and every corpus file is annotated.

Two of them change what a deployment does. They lead.

이번 릴리즈는 **소비자 모양의 입력**이 만들었다. 서로 include 하는 13개의 레거시
계약, prefix 스타일 넷, 주석은 어디에도 없음. 근본원인 8건이 한 번에 나왔고 그중
**6건은 이 저장소의 어떤 테스트도 빨갛게 될 수 없던 것**이다. 코퍼스 파일은 전부
self-contained이고 전부 주석이 달려 있기 때문이다.

### ⚠ Behaviour changed / 동작 변경

- **An unannotated operation is now refused, where it used to be allowed.**
  The bridge asked `annotations.get("ai_effect")?`, so a *misspelled* effect
  required approval and a *missing* one did not. Expose the estate's twelve
  interfaces with no scopes and the answer was **`allow=76, refuse=0`** —
  `SHUTDOWN`, `purge`, `void_invoice` and a writable attribute's setter among
  them. `COMPONENTS.md` recorded this hole for *ingested* contracts; it was a
  property of **any** unannotated one, which is what every legacy estate is.
  And S4 advised fifty-two times over that estate that the bridge *"must assume
  it needs approval"* — measured false. One part of the system stated the safe
  rule while the mechanism did the opposite.

  Closed means **refused**, with its own `Denied::EffectUnstated`. Not
  approval-required: an approval is a human saying yes to a *described* call,
  nobody can say yes to a call whose effect nobody stated, and one `--approve`
  would otherwise unlock a whole estate at once. Not not-exposed: the operator
  did expose it, and saying otherwise sends them into the allowlist to debug a
  contract problem.

  **Upgrading:** a contract that annotates its operations is unaffected
  (`end_to_end.sh` still reads `allow=2 need_scope=1`, unmoved). A contract
  that does not needs either `//@ ai_effect:` on its operations or the
  exposure's new **`--assume-effect <value>`** — the operator's single
  declaration of what that estate's silence means, run through the same
  recognition a contract's own value gets. Not seventy-six approvals. Every
  allow resting on it says so: `assumed: true`, `effect_stated_by: "exposure"`,
  and a startup line counts the silence.

  Absent and unrecognised are now deliberately different, in the direction that
  costs something: a typo reaches a human, because somebody was annotating and
  got one wrong; a silence goes back to the contract, because there is nothing
  there to say yes to.
  *주석 없는 오퍼레이션은 이제 거부된다. 침묵은 허가가 아니다.*

- **A `CloseConnection` between fragments is neither corruption nor a clean
  retry.** Both existing answers were false statements about the stream.
  `Desynchronized` says the peer is broken and the connection is corrupt, about
  a well-framed orderly goodbye — the client gives up on a service that was
  merely restarting. But `ConnectionClosed` hands the caller §13.5.1's promise
  that outstanding messages *were not processed and may be safely resent*, and
  that promise is about messages **without replies**. A peer that had already
  sent the leading piece of a reply demonstrably processed that request;
  re-sending a non-idempotent operation on that promise runs it twice, silently,
  on a pooled connection.

  So the retry fact is **per caller, not per connection**.
  `Error::InterruptedMidReassembly` carries what arrived and what was
  outstanding, `Fault::unsent` is false for the cut call and true for every
  other waiter, and `Error::is_orderly_close()` decides teardown against
  corruption on a value rather than on a message string. At GIOP 1.1
  `FragmentUnsupported` wins and the close is left *unread* — 1.1's refusal is
  permanent for that peer and a close is retryable, so preferring the close
  sends the pool round to hear the same thing again. A `MessageError` mid-chain
  is a report rather than corruption, but §9.4.8 gives it no body, so it names
  nothing and makes **no** request re-sendable. The serving loop stopped
  answering a goodbye with a `MessageError`, and stopped answering a
  `MessageError` with a `MessageError`, which was a two-ORB loop.

  **Unmeasured, and named as such:** the shape needs a peer to shut down
  between two writes of one reply, and neither fixture exposes that window.
  What *is* measured is the regression — omniORB 4.3.4's real 1.2 fragments
  still reassemble.

- **`describe_interface` reports the resolved surface.** Ten of twelve estate
  interfaces inherit; the agent was shown 11 operations, the guard judged 13,
  the servant served 13. Nothing was allowed that policy forbade — but the
  description an agent plans against was not the surface it could reach, and a
  reviewer reading it did not know the rest was there. Three walks became one,
  public, so nothing has an excuse to write a fifth.

### Added / 추가

- **`#include` resolves.** Eleven of thirteen estate files failed our own S4
  while `omniidl` accepted all thirteen, and the ~90 diagnostics were one
  thing: a name declared in an included file reported as undeclared. Quoted
  includes resolve against the including file's own directory, angled against
  `-I`, and the working directory is **never** searched — a validator run from
  a build directory would otherwise resolve differently from the same validator
  run from the source tree, and the difference would surface as a repository id
  rather than as an error. Once-only by canonical path, so **guards are not
  required** (the estate measured that real IDL in the wild has none), with
  non-blocking advice when an unguarded file repeats, because a deployed
  compiler rejects what we accept and being quietly laxer is worse than saying
  so. Cycles name the loop; a missing file lists every path searched; a
  diagnostic reports the included file's own line with the chain.

  The prefix rule across a file boundary was **measured, not reasoned** — two
  probes through `omniidl -Wbinline` say the includer's prefix does not enter
  and the included file's does not escape. Thirteen roots, 49 ids, all agreeing
  with `omniidl`. Full C preprocessing stays out and is **refused rather than
  skipped**: skipping `#if` compiles every arm at once, which is a silent
  misparse.

  `sidl-validate` takes `-I` and resolves before validating, which moves the
  estate's per-file acceptance from **2/13 to 13/13**. One of the original two
  passed *because* its include was silently skipped — the defect wearing a pass.
  *`#include`가 해석된다. 코퍼스 파일이 전부 self-contained이라 여섯 페이즈 동안
  아무 테스트도 빨갛게 되지 않았다.*

- **A Python client target.** The point of a second target language is not the
  language — it is that a second target is the only thing separating the IDL
  mapping from what happened to be convenient in Rust. It paid immediately: the
  **Rust** emitter's keyword list was missing Rust's own reserved words, so a
  contract naming `yield` emitted `fn yield()` and did not compile. No
  emitter's escaping had ever been *executed*, because no corpus file named a
  target language's reserved word until `28-target-keywords.idl` did.

  The seam is a **local process, not FFI**: generated Python renders arguments
  as AnyJSON v1 and hands them to `orbweaver-py-bridge`. No new dependency, no
  second wire implementation, `cargo tree` unchanged. **D007 is PROPOSED**, not
  adopted, and compares it against a PyO3 extension module (a new dependency
  class *and* an `unsafe_code` question at the boundary) and pure-Python
  CDR/GIOP. Mapping implemented against the OMG specification and *compared*
  against `omniidl -bpython` — 129 scope names and 100 operation names agree,
  and no name exists on our side that `omniidl` does not also produce.
  Comparing is not deriving.

  Measured: 28 golden contracts and 12 services generate, import and execute —
  73 values and 100 calls round-tripped, both byte orders, 0 divergences; a
  generated Python client completed 12 real calls against the omniORB fixture.

- **D005 option B — a regeneration is diffed against what is registered.**
  "Registered" is a directory the run is pointed at, which is what a previous
  run's output already is. The S5 catalog was rejected because `register()`
  builds it *after* S4 out of the artifacts S4 just gated, so an item would be
  diffed against itself. An absent contract is **counted and printed**, never
  silence; a registered-but-unreadable one is an error.

  What B cannot see is measured rather than asserted: a regeneration keeping
  every identifier and changing only `//@ ai_authz` is compatible by §5.3 and
  passes, with option C refusing it in the same test. That is why C landed
  first, and why the harness runs both.

- **Declared bounds are enforced by generated code**, at the same refusal point
  the dynamic path uses, and the §8 oracle gained the reading that would have
  caught the gap: for a value that violates its contract, the two paths must
  write the same bytes and reach the same verdict. Byte equality over valid
  values never could — which is how the divergence survived until D006 measured
  it while arguing about something else.
- **D005 option C**: a scope-shaped token the requirement states verbatim must
  survive to the `//@ ai_authz` S3 emits, checked by string equality with no
  model. Finding recorded rather than smoothed: **none of the twenty frozen
  requirements states a scope literally**, so the benchmark cannot exercise the
  rule at all — and of six naturally-phrased new ones, **three do not trip it
  either**. Korean prose, `ROLE_REGISTRAR` and `warehouse/robot/estop` are all
  ordinary ways to state a permission and all invisible to the rule, so a
  project with any of those house styles gets no binding and gets it silently.
  Not patched: accepting `ROLE_REGISTRAR` accepts every upper-case constant.
- **D005 and D006 approved.** `Expert::process` and `Router::dispatch` are
  excluded; the bound is their return path via a new versioned interface.
- **The content interceptor seat can read the arguments.** The chain was *not*
  moved after mapping, which is the fix people reach for: a chain there answers
  a mapping error before a policy refusal, turning failures into an oracle for
  the shape of operations the caller may not call. `CallContext` carries the
  agent's arguments unmapped instead. The static path supplies `None`, which is
  §4.7's bypass wearing a safety label and is now in the module docs rather
  than left to be discovered.
- **IF2's two stores are joinable** (`CallStats::merge`) and deliberately not
  joined: a static call is evidence a path *already has* a stub, so folding it
  in would have the promotion policy recommend promoting it again.

### Fixed / 수정

- **A gate found only what it could not load.** `Workspace::load` resolves
  `<id>.sidl.idl` **or** `<id>.idl` — a documented fallback for a contract no
  annotation stage ever saw — but discovery globbed only the annotated suffix,
  so the fallback was unreachable and `--only s4` over a directory of legacy
  `.idl` found nothing. The estate's workaround was to rename, which then made
  the run's attribution line report a contract with **no annotations in it** as
  annotated, because that line asked the filename; and because `gen-corpus`
  derives its module name from the file stem, the rename put a pipeline-stage
  suffix into a generated module name (`f_ESTATE_sidl`). Three symptoms, one
  cause — a name treated as evidence about content. The third needed no fix of
  its own.
- **The fix hint mangled qualified names**, printing `qualify it with
  ``Module::::```. The cause was in the *parser*, not the printer: `scoped_name`
  gave a scoped name the span of its **first token**, so
  `::MFS::Common::StringList` sliced to `"::"`. Three sites had it; the estate
  saw one. Splitting `unknown-scoped-name` off `unknown-name` makes the advice
  right as well as the text — "qualify it with its module" is meaningless for a
  name that is already qualified.
- **`validate(&str)` was the corpus's self-contained shape written into an
  API.** Handing it a resolved unit's *text* refuses it: thirteen files' guard
  directives are still in there, and four `#ifndef` blocks in one string is
  conditional compilation rather than an include guard. `validate_unit` takes
  the unit.
- **A file-scope `#pragma prefix` leaks across a splice.** A prefix runs to the
  end of its *file*; after concatenation there is one file, so a deliberately
  prefix-less contract inherited the previous one's prefix and five repository
  ids came out **well-formed and wrong**. Nothing errors on our side — only a
  peer disagrees, and then `_is_a` fails and an `--expose` allowlist names an
  interface no object has, which reads exactly like a permissions mistake. The
  rule is one line, *each file begins with the empty prefix*, and it was
  written nowhere because until a set existed there was nowhere to write it.
- **`spikes/echo.idl` states an effect on every operation.** They are all reads
  — the interface has no state — but "obviously a read" is not something a
  bridge can see. The harness caught it; the fixture was written when silence
  was an allow.

### Known limits / 알려진 한계

- **An agent cannot read an Interface Repository through AnyJSON.** §4.5 has no
  form for `::CORBA::TypeCode`, so `corpus/services/ir_subset` loses ten items
  including `describe_interface` — and the MCP agent path speaks the same
  mapping. Recorded, not fixed: §4.5 is a specification decision, not a Python
  problem.
- **The dry-run survey is correct and hard to read.** Closing the effect gate
  took it from 7,253 bytes of uniform `allow` to 31,957 bytes with real signal,
  and the growth is *entirely* 64 verbatim copies of one `why` sentence. One
  reason per **class** rather than per row would put it near 8 KB with strictly
  more signal. A report shape, not a gate.
- **`--assume-effect` is a real widening and is meant to be.** It is one
  operator declaration covering every silence in an exposure. That is the point
  — the alternative is seventy-six approvals and a human who learns to click
  through them — but it is written into every allow that rests on it so an
  audit can find them.
- The Python target is **clients only**. A Python servant needs the bridge to
  call *back* into Python, which is a second protocol direction.
- Cross-front-end portability of the include semantics is **unmeasured** — TAO
  and JacORB were absent. An `#include` inside a module whose file sets a
  prefix is exercised by no file we have. Hard links to the same content are
  two files to the resolver.

---

## v0.3.0 — 2026-08-14

The wire-behaviour section leads again, for the same reason. Everything in it
was found by a reader we did not write, or by composing two parts that had only
ever been measured apart.

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **`::CORBA::TypeCode` loaded as `void`.** The front end predeclares the name
  for checking, the registry resolves against the spec's own definitions where
  it is absent, and it fell through to `void` — silently, for months. An
  operation returning a TypeCode generated `-> ()` and **marshalled nothing at
  a peer expecting one**. `CLAUDE.md` requires that spelling, which is what
  made the gap read as support. Found by the generated-servant batch, which
  could not express `describe_interface` and said so rather than emitting an
  empty reply.
- **A refusal claimed the call had completed.** `Guarded` raised system
  exceptions with a literal `0` — `COMPLETED_YES` — so every refused caller was
  told its call had run. A separate path from the transposed enum fixed in
  v0.2.0, and now that quota refusals are `TRANSIENT`, it was an invitation to
  retry attached to "it already happened".
- **A deferred IFR operation answered "no such operation".** `ifr.rs` refused
  the ten IR operations it defers with `BAD_OPERATION`, which is byte-for-byte
  what an operation nobody thought about answers — so a client could not tell a
  decision from a gap, and `SERVICES-COVERAGE.md` could only tell them apart by
  searching this repository for a written reason. They answer **`NO_IMPLEMENT`**
  now: `NO_PERMISSION` is policy, `NO_IMPLEMENT` is deferred, `BAD_OPERATION` is
  "not an operation of the object you addressed". Affects `contents`, `lookup`,
  `lookup_name`, `describe_contents`, `describe`, `_get_defined_in`,
  `_get_containing_repository`, `get_canonical_typecode`, `get_primitive`,
  `_get_type`.
  *유예 연산은 이제 `NO_IMPLEMENT`로 답한다 — 와이어가 유예와 누락을 구분한다.*

### Added / 추가

- **`Contained::_get_version` is served.** It answered `BAD_OPERATION` while its
  write half `_set_version` answered `NO_PERMISSION` — "the operation exists and
  the answer is no" — on a version the facade parses out of every repository id
  it handles. The two were the wrong way round by `ifr.rs`'s own argument. The
  read answers now; the write is still refused. `corpus/services/ir-subset.idl`
  declares the attribute, so the generated skeleton serves it too and the
  byte-for-byte comparison in `ifr_shape.rs` covers it.
- **Constants carry their value, and are generated.** `Entry::Const` recorded
  the type and not the value, so `orbweaver-gen` skipped every constant — which
  the end-to-end run met concretely when a contract declared its authorization
  scope as a `const string` so a servant could name it, and the servant could
  not. The registry now folds the expression **once, where the names are**
  (`const long OFFSET = MAX_RETRIES * 2;` needs both arithmetic and IDL's
  outward scope resolution), coerces it to the declared type, and stores an
  evaluated `ConstValue`. An expression it cannot fold stores **no value** and
  the generator skips that item with a reason — never a guessed zero. A
  `string` constant is emitted as `&str`, since Rust has no `const String`.
  *상수는 값을 갖는다 — 레지스트리가 한 번 접고, 접을 수 없으면 값을 만들지 않는다.*

- **`CosNaming::NamingContextExt::to_url`**, measured against **two** producers:
  omniORB's own client resolved a URL ours built, and omniNames was run as a
  second producer over 14 argument pairs (11 identical, 3 differing only in
  hex-digit case, each parser reading the other's). One behaviour changed
  *because* of that comparison: an empty name returns the bare `corbaname:`
  form, as omniNames does.
- **`moe::Router::select`**, delegating to the trading engine. When a
  constraint names a field a wire-registered offer cannot answer it refuses the
  **whole call** with `NO_IMPLEMENT`: a shorter list would say *these are all
  the experts that qualify*, which is the sentence three-valued matching exists
  to prevent.
- **Constants are generated**, with the value folded once in the registry
  rather than an expression every consumer would have to fold — three folders
  that will disagree, and the one that disagrees silently ships. An expression
  that cannot be folded stores nothing rather than a guessed zero.
- **A deferral answers `NO_IMPLEMENT`, an oversight still answers
  `BAD_OPERATION`.** Ten Interface Repository operations moved, so the
  distinction `SERVICES-COVERAGE.md` opens by saying the wire cannot make is
  one the wire now makes for that service — and `BAD_OPERATION` there is zero.
- **The audit ledger is bounded** and spends one slot on an in-band `ELIDED`
  marker naming what it dropped, because a dropped hour and a quiet hour read
  identically exactly when somebody is reading the log to tell them apart.
  `verify_promotion` refuses a truncated history rather than concluding from a
  gap, and checks before parsing so the marker is not reported as a formatting
  bug.
- **A trace can name what failed**: `CallResult` carries the exception's
  repository id, so D004's `outcome` column stops saying `-` for a failure it
  simply had not been told about.
- **D006** (PROPOSED): the data-plane rule, and the finding that no option can
  enforce it — a bound constrains size, not frequency, and "never per token" is
  the operative phrase.
- **Attribute accessors are gated and visible.** `allow_interface` made
  `_get_balance` callable while the policy resolved scopes only through
  declared operations, so an `ai_authz` written on an attribute bought nothing
  and a `_set_` was never approval-gated — invisible *and* ungated, in the
  instrument built so an operator can see what an agent may reach. An
  `ai_effect` gates the setter only; a scope guards the value and so guards
  both accessors.
- **Token → `Caller` exchange** with the verifier as a trait this crate does
  not implement, and a **scope audit** that reports a contract scope no issued
  token can satisfy as an outage, naming the operations that go dark, before
  any call is made (D005's class).
- **`describe_interface` generated byte-identically** to the hand-written
  Interface Repository facade — 77 cases × 2 byte orders, with omniidl
  accepting the contract and the oracle's own decoder reading the reply back.
- **`docs/SERVICES-COVERAGE.md`** — all 107 declared operations probed over the
  wire, classified served / refused-with-a-reason / absent. **12 answered
  `BAD_OPERATION` with no reason written anywhere**; `PLAN-SERVICES` §8.1 now
  writes the sentence each was missing, including two that turned out to be
  defects and one that is recorded as undecided.
- The fuzz reaches the **text** parsers too (stringified IORs, D004 trace
  lines, the ingestion validators): 450,000 cases × 17 targets across five
  seeds, zero panics, with every target's reach asserted.
- **Dispatch runs concurrently** (`SharedDispatch`). One lock per servant, at
  most one held per thread, nothing blocking inside it — so lock *ordering* is
  vacuous rather than documented, since a deadlock cycle needs a thread holding
  two locks. Enforced structurally: a guard cannot escape its closure, a
  per-thread marker refuses a second section, and dialling asserts nothing is
  held (panicking in debug, so a violation fails the test that wrote it). The
  existing `Dispatch` trait and every generated skeleton are unchanged and
  still serialized. **The finding**: `ServerStats::peak_dispatching` was written
  to witness overlap and could not — it sits outside the servant's own lock and
  counted callers queued for it, reaching N on a *serialized* server. The
  negative control caught it; review had not.
- **Multi-object generated skeletons**: `knows()` required with no default,
  identity as an explicit `Target` argument, and **71 pinned cases answering
  byte-identically to the hand-written Interface Repository facade**, down to a
  minted reference's object key and port — with `ifr.rs` unmodified as the
  oracle.
- **IOR rewriting for NAT and containers** (R7): profiles and
  `TAG_ALTERNATE_IIOP_ADDRESS` rewritten, the object key refused as a target
  because it is identity rather than a route, an undecodable profile preserved
  byte for byte. Both real socket failures constructed. The container probe is
  written and **has never executed** — counted as a skip.
- **The quota interceptor seat**, with the window taken as a host-supplied
  label rather than from a clock, and refusals typed `TRANSIENT` when they
  renew and `NO_PERMISSION` when they do not.
- **The audit ledger leaves the process** in serving mode; it used to be
  drained only under `--dry-run`.
- **`#pragma prefix`/`version`/`ID`** and an S4 report for an explicit ID that
  is not a repository id.
- **D005** (PROPOSED): contract stability, and the argument that semantic drift
  is worse than identifier drift.

### Fixed / 수정

- **An unknown latency is not a fast one.** An offer registered over the wire
  carried `latency_p50: 0.0`, which did not merely fail to match
  `latency_p50 < 20` — it **matched**, so a latency-ordered router preferred
  exactly the experts nobody had measured. Matching is now three-valued and
  unknown sorts after every known value.
- The generator inverted a repository id into a Rust module path, emitting
  `pub mod acme.com` for prefixed IDL.
- **`_get_version` answered "no such operation" while `_set_version` answered
  "you may not"** — backwards by the servant's own argument, on data the
  registry parses out of every repository id it holds.
- **Annotations on types were never checked.** The contract checker walked
  interfaces only, so a typo on a `typedef` was silent — contradicting its own
  premise that an `ai_*` key nobody reads is what it reports.
- **A bound change is no longer reported as the silent class.** `idl-diff`
  described `sequence<octet>` becoming `sequence<octet, 64>` as "the encoded
  form differs, and CDR gives a receiver no way to notice". Both halves are
  false: the bytes are identical, and a peer that exceeds the bound is refused
  loudly. The verdict stays Breaking; the reason now says which kind of
  breaking, because the sentence being borrowed describes §5.3's measured case
  of a peer returning the wrong member and raising nothing.
- The `*.log` ignore swallowed the end-to-end provenance record, which only a
  fresh clone could reveal — the third time a blanket ignore has taken a
  committed artefact.

---

## v0.2.0 — 2026-08-14

Phase 4 substantially landed; five CORBA services served on our own POA; the
specification pipeline split into stages that can be measured one at a time.

### ⚠ Wire behaviour changed / 와이어 동작 변경

Four defects, in three groups, that change the bytes we put on the wire or
what we accept from a peer. Two of the three groups were found by a reader we
did not write — a foreign ORB, and the specification itself where no peer
could serve as one.

- **`completion_status` was transposed.** `COMPLETED_YES` is ordinal 0 per
  §4.11.4 (confirmed against omniORB before changing anything); our
  `Completion` had `No = 0, Yes = 1`. **A servant reporting "the operation did
  not run" reached every foreign ORB as "it ran"** — so a call refused before
  it started looked like a mutation that had happened, and a client that could
  have safely re-sent concluded it must not. Every servant uses the symbolic
  names, so the fix corrects the naming, event, IFR, expert and tenant services
  at once. `MAYBE` was 2 either way, which is why only two of the three were
  wrong and why nothing local caught it: our own client compared against the
  same enum and agreed with itself, including the test that asserted the
  encoded byte equalled `Completion::No as u32` and therefore moved with the
  bug. It now asserts the literal ordinal.
  **재시도 안전성을 결정하는 두 값이 뒤바뀌어 있었다.** 우리 클라이언트는 같은
  enum으로 비교하므로 스스로와는 늘 일치했고, 외부 ORB만이 이견을 낼 수 있었다.

- **Recursive types could not be marshalled at all.** Every non-empty recursive
  value was refused with "expected a value of type an indirection", and nothing
  noticed because the only generator that could have produced such a value was
  the one reporting the arm as unmeasured. Markers now resolve against the
  enclosing type the error path is already standing on; nesting is bounded at
  64 in both directions, because on decode the depth is the sender's choice.

- **Two fragment-reception defects**, found against hand-built §9.4.9 streams
  since no available peer emits fragments: a stray leading `Fragment` was
  returned as a message, and a fragment at a different GIOP version was
  accepted as a continuation (in 1.1 the bytes read as a request id are body,
  so a match would have been coincidence).

### Added / 추가

- **The end-to-end path, measured as one path** (`spikes/end_to_end.sh`, in the
  harness): a fresh requirement → S1–S5 → both generated halves → a servant on
  our POA → an agent-shaped caller through the guard, with a scope refusal
  visible in the transcript. **185 hand-written product lines against 778
  generated.** Composing it produced the release's most useful finding — see
  *Known limits*.
- **`#pragma prefix` / `version` / `ID`** — repository ids now match omniidl on
  a 25-id corpus, prefixes and all. Before this, every legacy IDL file (the OMG
  recommends a reverse-DNS prefix) would have given us a different identity for
  every type than the peer had, while looking correct locally.
- **`orbweaver-console`** — catalog, contract diff and D004 traces as
  self-contained HTML, no web framework and no template engine.
- **D004 tier 1 telemetry** — one span record per decision, no clock, and a
  credential structurally unable to reach a line.
- **S3i** — annotations inferred for ingested contracts, which never occupy a
  key a gate reads until a human approves them.
- **CORBA services on our POA**: CosNaming server, CosEvent push channel
  (bounded queue, dead consumers disconnected with **drops counted**),
  Trading wire surface for the MoE control plane, a read-only Interface
  Repository facade, and LifeCycle/tenancy with the tenant in every object key.
- **Remote IFR ingestion** — JacORB 3.9's Interface Repository served us. A
  contract can now be taken off the wire with no IDL file, with provenance
  marked and **contagious upwards**.
- **Server skeletons** (`orbweaver-gen`), driven by omniORB's own python
  client, with a servant fault surface whose `#[must_use] Raising` cannot
  become a `SystemException` without naming the completion status.
- **Server-side static-equals-dynamic oracle** — 204 reply-byte comparisons,
  three GIOP versions × two byte orders × two reply origins.
- **Concurrent connections** — cap 64, refusal spoken as §9.4.7's
  `CloseConnection`. Dispatch remains serialized and the documentation says so.
- **The guard's interceptor chain** (F4) and **dry-run**: an exposure can be
  read before it is deployed, audited under its own `DRYRUN-` token, and unable
  to diverge from the live gate by construction.
- **S1–S3 as distinct pipeline stages**, each a producer plus the gate that
  judges it, each runnable alone.
- **Property and contract testing** (`orbweaver-test`), including a wire fuzz
  measuring panic-freedom over the decoders a peer reaches before any policy
  runs: 0 panics in 50,000 cases × 10 targets.
- **Vector search** behind `search_interfaces` via an external command (D003-A)
  — no new dependency. The synonym class remains **UNMEASURED** without a key.
- Decisions **D003** (approved) and **D004** (approved): both adopted zero
  Cargo dependencies.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
  [`docs/PLAN-SERVICES.md`](docs/PLAN-SERVICES.md),
  [`docs/PLAN-DEFERRED.md`](docs/PLAN-DEFERRED.md),
  [`docs/PLAN-MOE.md`](docs/PLAN-MOE.md).

### Fixed / 수정

- The generator bound its encoder closure as `e`, so an IDL parameter named `e`
  shadowed it into `e.put(e)`. Generated locals are now `__`-prefixed and the
  corpus keeps a parameter named `e` on purpose.
- `--expose IDL:spike/Echo:1.0` split at the version's dot, so the
  bare-interface form allowlisted an interface nobody had. Found by dry-run on
  its first run.
- The registry's union `default_index` was computed against the unexpanded case
  list, and **the existing test asserted the buggy semantics**.
- `Poa::new` took an incarnation from a freed `Box` address, which the
  allocator then reused, so two POAs could share one.
- The offer store could lag the residency machine, so under memory pressure the
  loading policy returned an empty decision list — silently.

### Changed / 변경

- The harness takes a **machine-wide lock** and kills fixtures by process
  group. Two concurrent runs used to destroy each other's peers and report
  failures that were about the scheduling; that cost two diagnoses.
- `PLAN.md` / `PLAN.ko.md` at **v0.7**; the streams are scope and the status
  lives in `COMPONENTS.md`, which is refreshed after every wave.

### Known limits / 알려진 한계

Stated because an absence that is not written down reads as a feature:

- Nothing in the token exchange has been through a **real identity provider**;
  it is unit-tested against hand-built claims — the same shape as CSIv2 being a
  per-peer claim rather than a feature.
- **No rewritten IOR has been put in front of a foreign ORB**, and port
  translation has never been dialled. R7 is now measured across a real routing
  boundary (a second host), but both ends of that measurement were ours.
- **Multiplexing is refused below GIOP 1.2**, deliberately: a 1.1 `Fragment`
  carries no request id, so a fragmented 1.1 reply is attributable only by
  position. Against a stock omniORB that is a live limit, not a theoretical
  one — it fragments a 1 MB reply at 1.1, and we refuse it.
- A `CloseConnection` arriving **between fragments** surfaces as
  `UnexpectedMessage` rather than as a retryable close.
- Ingested contracts carry no SIDL, so the guard's gates have nothing to key
  on — a second, independent reason exposure stays off.
- The embedding synonym class, the TAO oracle column and the SSLIOP peer proof
  are **unmeasured**, each for a stated reason, and the harness counts them as
  skips rather than passes.
- **The pipeline is not reproducible across runs, and nothing catches it.**
  Re-running S1–S3 on the same requirement with the same prompts passed every
  gate 1/1 again and produced a different contract: different module and
  operation names, a different parameter type, and an authorization scope that
  drifted from the one the requirement literally states. An identity provider
  issuing the stated scope against such a contract refuses every legitimate
  caller. Recorded in `docs/pipeline-runs/2026-08-14-end-to-end.md`; the fix
  needs a decision about what S2 may choose, so it is named rather than
  patched.

---

## v0.1.0 — 2026-08-13

Phases 0–3.5. A from-scratch MIT ORB interoperating with omniORB 4.3.4 and
JacORB 3.9 in both directions at GIOP 1.0/1.1/1.2; IDL 4.2 front end and type
registry in full oracle agreement; POA and object model; dynamic invocation and
AnyJSON; the MCP triad over stdio with default-deny exposure and capability
handles; the S4 validation gate; CSIv2 wire and delegation policy.
