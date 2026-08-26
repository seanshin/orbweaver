# Changelog / 변경 이력

Measurements live in [`docs/COMPONENTS.md`](docs/COMPONENTS.md); this file
records what changed and, where it matters, what it changes on the wire.

측정은 `COMPONENTS.md`에, 여기에는 무엇이 바뀌었는지와 — 중요한 경우 — 그것이
와이어에서 무엇을 바꾸는지를 적는다.

---

## Unreleased

### ⚠ Wire behaviour changed / 와이어 동작 변경

- **A `LocateReply` can say where the object went, and a `Reply` can carry a
  service context list.** Two shapes §9.4.6 and §9.4.3.1 require that this ORB
  could name and could not put on the wire.

  `LocateStatus::ObjectForward` had no body: `encode_locate_reply` wrote a
  request id and a status word and stopped, and the serve loop decided the
  answer by asking `Dispatch::knows`, a boolean. Measured before the change,
  with a servant whose object had moved: `Connection::locate()` answered
  `Ok(Unknown)` — **"nowhere", not "elsewhere"**. The status now carries its
  `Forward` inside the variant, so a forward cannot be said without saying
  where, and `Dispatch::locate` / `SharedDispatch::locate` let a servant say it
  (defaulting to the previous answer, so no existing servant changes). On the
  reading side `LocateResult::Forward` now carries a `Forward` rather than a
  bare `Ior`: statuses 2 and 3 used to collapse, losing the permanence the peer
  had sent.

  `encode_reply` wrote a hard `0` where §9.4.3.1 puts an
  `IOP::ServiceContextList`, and `decode_reply` walked the peer's list only to
  move its cursor — while `decode_request` kept the identical list. One decoder
  written twice with one copy losing the data; there is now one
  `read_service_contexts` and `Reply::service_contexts` is populated. §9.7.2's
  *"ignored, but preserved"*, the rule already applied to `TaggedComponent`.
  **Not an attachment API**: nothing in the workspace emits a non-empty list,
  and who may is `PLAN-DEFERRED` §21. An empty list is byte-identical to the
  zero that was there — asserted, in every version layout and both orders, not
  assumed.

  Also found while re-measuring: `handle_request` retyped the reply body offset
  as `HEADER_LEN + 12` beside a comment saying it was right only while the
  context list stayed empty. `reply_body_start` is now its one home.

  Open and named rather than left looking closed: no external peer has been
  made to emit an `OBJECT_FORWARD` at us, so this change has no recorded peer
  bytes. D029 §6.1's Location row and `docs/COMPONENTS.md` carry it.

- **`serve_one` asks `redirect` before `knows`, so a moved object is forwarded
  on the *request* path too.** The second message of the root cause above.
  `knows` returning `false` used to end the request as `OBJECT_NOT_EXIST`
  before `redirect` was consulted — and a key the servant does not answer to is
  exactly and only the set a forward is *for*. A servant that had moved an
  object could either lie in `knows` or refuse a caller whose object exists
  elsewhere; a caller that probed was told "elsewhere" and a caller that
  invoked was told "nowhere". The order is now `redirect`, `knows`, `dispatch`,
  and it has one home rather than two copies: `server::serve_one_ordering()`
  returns it as data, and both `serve_one` implementations are asserted against
  that function instead of against a comment.

  **No servant here changes.** None of the five overrides `redirect`, and every
  skeleton `orbweaver-gen` emits opens its `redirect` with
  `self.refs.oid_of(&req.object_key)?`, returning `None` for an unknown key
  before the servant is reached. Measured under the reverted order: exactly one
  test in `locate_forward_and_reply_contexts.rs` changes answer and the other
  eleven are identical. What a servant now sees that it did not: `redirect` is
  asked about keys whose `knows` is `false`, so a `redirect` with a **side
  effect** has it on unknown keys too.

  **What this made possible**, and what it did not:
  `crates/orbweaver-giop/tests/forward_for_a_name.rs` is a redirect emitted for
  a **name** — a servant holding names and no objects, which four records name
  as the blocker on D029 §6.1's lifecycle row. It was unwritable before the
  reorder, because a forwarder could only forward everything or refuse
  everything. **The lifecycle row still does not move**: a forward is a reply
  and a reply needs a listener, so it can never be emitted by the party that
  went away, and closing the row needs a decision — named X in D029 §6.1's new
  lifecycle subsection — that the reference `Orb::server` hands out is
  *indirect*. No new wire shape: a forward produced by a name resolving is
  byte-for-byte the message an object move produces, checked rather than
  asserted, so no new peer leg is owed.

  *위 근본원인의 **두 번째 메시지**. `knows`가 `false`면 `redirect`를 묻기 전에
  `OBJECT_NOT_EXIST`로 끝났는데, 서번트가 답하지 않는 키야말로 포워드가
  존재하는 이유인 바로 그 집합이다. 조사한 호출자는 "다른 곳", 그냥 호출한
  호출자는 "아무 데도 없음"을 들었다. 이제 순서는 `redirect`, `knows`,
  `dispatch`이고, 복사본 둘이 아니라 집 하나를 갖는다 —
  `server::serve_one_ordering()`이 순서를 데이터로 돌려주고 두 구현이 주석이
  아니라 그 함수에 대해 검증된다. **여기 서번트는 하나도 바뀌지 않는다**: 다섯 중
  `redirect`를 재정의한 것이 없고, 생성된 스켈레톤은 모두
  `self.refs.oid_of(&req.object_key)?`로 시작한다. 순서를 되돌린 상태에서 측정:
  정확히 한 테스트만 답이 바뀌고 나머지 열한 개는 동일했다. 새로 보이는 것:
  `redirect`가 `knows`가 거절하는 키에 대해서도 질문받으므로, **부작용이 있는**
  `redirect`는 이제 모르는 키에서도 그 부작용을 갖는다. **이것이 가능하게 한 것**:
  `forward_for_a_name.rs` — 이름에 대해 발행되는 리다이렉트이며, 네 개의 기록이
  생애주기 행의 차단 요인으로 지목한 것이다. 재정렬 전에는 쓸 수 없었다.
  **그럼에도 생애주기 행은 움직이지 않는다**: 포워드는 응답이고 응답에는 듣는 쪽이
  필요하므로 떠난 쪽이 발행할 수 없다. 닫으려면 결정이 필요하다 — D029 §6.1의 새
  생애주기 절에서 **X**로 이름 붙였다: `Orb::server`가 내주는 참조가 *간접적*이라는
  결정. 새 와이어 모양은 없다 — 이름이 낳은 포워드는 객체 이동이 낳은 메시지와
  바이트 단위로 같으며, 주장이 아니라 검사했다. 그래서 새 피어 검사는 빚지지
  않았다.*

  *§9.4.6과 §9.4.3.1이 요구하지만 이 ORB가 **이름 부를 수는 있고 와이어에 실을
  수는 없던** 두 가지 형태. `LocateStatus::ObjectForward`에는 본문이 없었고 서브
  루프는 불리언 `knows`로 답을 정했다 — 변경 전 측정: 이동한 객체에 대해
  `Connection::locate()`가 `Ok(Unknown)`, 즉 **"다른 곳"이 아니라 "아무 데도
  없음"**. 이제 상태가 `Forward`를 변이체 안에 담으므로 **어디인지 말하지 않고
  포워드를 말할 수 없다**. 읽는 쪽의 `LocateResult::Forward`도 `Ior`가 아니라
  `Forward`를 담는다 — 상태 2와 3이 합쳐지며 피어가 보낸 영속성이 사라지고
  있었다. `encode_reply`는 §9.4.3.1이 목록을 두는 자리에 하드코딩된 `0`을 썼고
  `decode_reply`는 피어의 목록을 커서만 옮기며 버렸다 — `decode_request`는 같은
  목록을 보관하는데도. 디코더 하나를 두 번 쓴 것이고 한 사본이 데이터를 잃고
  있었다. §9.7.2의 "무시하되 보존한다". **부착 API가 아니다** — 누가 붙일 수
  있는가는 `PLAN-DEFERRED` §21. 빈 목록이 그 자리에 있던 0과 바이트 단위로 같음은
  가정이 아니라 모든 버전 레이아웃과 양쪽 바이트 순서에서 **단언**했다. 재측정
  중 발견: `handle_request`가 응답 본문 오프셋을 `HEADER_LEN + 12`로 다시 타이핑해
  두었고, 그 옆 주석이 "컨텍스트 목록이 비어 있는 동안에만 맞다"고 적고 있었다 —
  `reply_body_start`가 이제 그 사실의 유일한 집이다. **닫히지 않았고 이름 붙여 둔
  것**: 외부 피어에게 `OBJECT_FORWARD`를 보내게 한 적이 없으므로 이 변경에는
  기록된 피어 바이트가 없다.*

- **`::CORBA::Principal` was recorded as `void` and marshalled zero bytes.**
  The name is predeclared by `sema.rs` and the registry answered
  `TypeCode::Void` for it, so a member, parameter, return or sequence element
  the author had typed put **nothing** on the wire — and a peer that writes a
  Principal there hands us octets every field after it is then mis-parsed
  from. Nothing was red: `sidl-validate` rejected 0, `contract-check` saw a
  `void`, both emitters produced the member (`pub who: ()`, `("who","who",
  "void")`), and the §5.3 differ told the only lie anyone would have read —
  *"not declared in this unit; a missing `#include`"* — about a name its own
  front end declares. It is now `TypeCode::Principal`, refused **by name** at
  generation and in the dynamic path, and `idl-diff` calls the member's type
  change BREAKING. This was the same defect fixed for `::CORBA::TypeCode` and
  left in its neighbour: a fix scoped to the keyword that reported it fixed a
  keyword. `orbweaver_idl::sema::PREDECLARED_CORBA` now publishes the table and
  the registry sweeps all four rows — two answered, two refused at parse
  (`object` and `valuebase` are lexer keywords, so `::CORBA::Object` and
  `::CORBA::ValueBase` cannot be written at all). Every marshalling layer had
  already refused `Principal` by name; **none of those arms was reachable from
  a contract until now.** `corpus/golden/34-corba-principal.idl`, kept golden
  because `omniidl -b dump` accepts the file.

  **`::CORBA::Principal`이 `void`로 기록되어 0바이트를 마샬링하고 있었다.** 전단이
  선언하는 이름을 레지스트리가 `TypeCode::Void`로 답했으므로, 작성자가 적은 멤버가
  와이어에 **아무것도** 싣지 않았다. 아무것도 빨갛지 않았다 — 두 에미터 모두
  멤버를 생성했고, §5.3 차이기는 자기 전단이 선언하는 이름을 두고 *"이 단위에
  선언되지 않음"* 이라는, 읽을 사람이 있는 유일한 거짓을 말했다. 이제
  `TypeCode::Principal`이고 생성과 동적 경로 양쪽에서 **이름으로** 거부된다.
  이것은 `::CORBA::TypeCode`에 대해 고치고 이웃에 남겨둔 같은 결함이다 — 보고한
  키워드에 맞춘 수정은 키워드 하나를 고쳤을 뿐이다. 마샬링 계층들은 이미
  `Principal`을 이름으로 거부하고 있었으나, **지금까지 어떤 계약도 그 팔에
  도달할 수 없었다.**

- **`_get_def_kind` told a conformant IFR client that three definitions it
  holds do not exist.** `ifr::RepositoryServer::def_kind` ended in
  `_ => DefinitionKind::None`, so `IRObject::_get_def_kind` answered `dk_none`
  — *no such definition* — for a `valuetype` and a `native`, and answered
  `dk_Interface` for an `abstract interface` with nothing looking at whether it
  was abstract. The doc comment above that arm asserted the opposite of the
  code and had been **false for five days**: it said the registry cannot tell
  them apart, which stopped being true when a valuetype became `TypeCode::Value`
  and a native `TypeCode::Native`. Nothing went red, because the catch-all took
  both new variants the moment they existed, so the registry's new distinction
  never reached the wire. Measured against omniORB 4.3.4's own `omniORB.ir_idl`
  client over TCP: `dk_none → dk_Value (20)`, `dk_none → dk_Native (23)`,
  `dk_Interface → dk_AbstractInterface (24)`, with six controls unmoved.
  `def_kind` is now exhaustive over all 33 `TypeCode` variants with no `_` arm;
  `TypeCode::Recursive` is the only `dk_none` and carries the reason true of it
  alone. `ifr::DefinitionKind` names 0..24 and stops where the measurement
  stops — the peer's own enumeration carries 25 members — while
  `corpus/services/ir-subset.idl` declares all 36 of §14.5.1 for the opposite
  reason: a decoder must accept what a conformant sender may write.

  **`_get_def_kind`가 적합한 IFR 클라이언트에게 레지스트리가 가진 정의 셋이 없다고
  답했다.** 포괄 팔 `_ => DefinitionKind::None` 때문에 `valuetype`과 `native`가
  `dk_none`을, `abstract interface`가 `dk_Interface`를 받았다. 그 위의 주석은
  코드의 반대를 주장했고 **닷새 동안 거짓**이었다 — 포괄 팔이 새 변형 둘을
  생기는 즉시 삼켰으므로 레지스트리의 새 구분이 와이어에 닿은 적이 없고, 답은
  어느 쪽이든 `dk_none`이라 아무것도 빨갛지 않았다. omniORB 4.3.4 자신의
  `omniORB.ir_idl` 클라이언트로 측정했다. 이제 33개 변형 전부에 판정이 있고 `_`
  팔은 없다.

- **The Interface Repository's browse half answers instead of `NO_IMPLEMENT`:
  9 of 44 served → 19 of 44, 25 refused `NO_PERMISSION`, 0 deferred.**
  `SERVICES-COVERAGE` §5's ten `NO_IMPLEMENT` operations were not ten decisions
  but one — `Container::contents`, `lookup`, `lookup_name`,
  `describe_contents`, `Contained::describe`, `_get_defined_in`,
  `_get_containing_repository`, `Repository::get_canonical_typecode`,
  `get_primitive` and `IDLType::_get_type` are the walk that lets a client
  browse rather than look one entry up by an id it already had to know, and
  nine are unusable without the tenth. Three objects had to be minted first:
  `ModuleDef` (derived from the scopes entries sit in, the segment count taken
  from the **qualified name** and never the id path, so `IDL:acme.com/bank/`
  `Money:1.0` yields `IDL:acme.com/bank:1.0` and not a module that does not
  exist), `OperationDef`/`AttributeDef` (`member_id` is now the one home for a
  derivation `describe_interface` already did, `member_for` its inverse, so a
  member reachable by a description is reachable by a key), and `PrimitiveDef`
  (§14.5.14 gives it no repository id, so its key carries `pk:<kind>`).
  omniORB's own IR client walks the whole repository — every leg answered, the
  `any`s extracting as the structs §14.5 names. **The peer found two defects an
  in-tree test had not**: `lookup("gc10")` on a top-level module answered nil
  because `Registry::load` removes a module's qualified name from `by_name`
  after walking into it, and the first probe's gate asserted `dk_Module < dk_all`
  at a root where every object is a module — a gate that could only ever be red.

  **IFR의 브라우즈 절반이 `NO_IMPLEMENT` 대신 답한다: 44개 중 9 → 19 서빙, 25
  거부, 유예 0.** 열 개의 `NO_IMPLEMENT`는 열 개의 결정이 아니라 하나였다 —
  아이디를 이미 알고 있어야 하나를 조회하는 대신 저장소를 걸을 수 있게 하는
  절반이며, 아홉은 열 번째 없이 쓸 수 없다. `ModuleDef`·`OperationDef`/
  `AttributeDef`·`PrimitiveDef`를 먼저 만들어야 했다. omniORB 자신의 IR
  클라이언트가 저장소 전체를 걸었고, **피어가 사내 테스트가 찾지 못한 결함 둘을
  찾았다.**

- **The event channel's pull-supplier side answers instead of `NO_IMPLEMENT`:
  CosEvent goes from 14 of 18 served to 17 of 18.** `obtain_pull_consumer`,
  `connect_pull_supplier` and `disconnect_pull_consumer` left `is_deferred`,
  which now holds `destroy` alone. They carried **both** blocked models at
  once, so the 2×2 of supplier/consumer models is complete: push/push and
  push/pull were served, pull/push and pull/pull are new, and
  `all_four_models_carry_the_event_they_were_given` creates each pair over the
  wire and asserts an event crosses it. The channel asks with **`try_pull`,
  never `pull`**: `pull` is specified to block until the supplier has
  something and the source round is shared, so one silent supplier would be
  every other supplier's outage. The price is an interval the channel has to
  invent — `DEFAULT_SOURCE_POLL` 100 ms, moved by `set_source_poll` — and a
  round that finds an event does not sleep, so a backlog drains at socket
  speed. `ChannelStats` gains `sourced` (so `accepted - sourced` is what
  suppliers pushed in), `pull_failures` kept apart from `push_failures` — one
  number is what the channel could not send, the other what it could not fetch
  — and the `pull_suppliers_connected` gauge. **No drop cause joined the
  split**: a `ProxyPullConsumer` holds no queue, so there is nothing to drop.
  A supplier answering the user exception `Disconnected` is released
  immediately and **not** counted as a failure; it did not fail, it said it was
  finished. Oracle is an omniORBpy `CosEventComm::PullSupplier` that our
  channel dials and invokes, both byte orders.

  **이벤트 채널의 pull 공급자 쪽이 `NO_IMPLEMENT` 대신 답한다: CosEvent 18개 중
  14 → 17 서빙.** 세 연산이 `is_deferred`를 떠났고 이제 `destroy`만 남았다. 그 셋이
  막힌 두 모델을 한꺼번에 지고 있었으므로 공급자/소비자 2×2가 완성된다.
  채널은 **`pull`이 아니라 `try_pull`로** 묻는다 — `pull`은 블록하도록 규정되어
  있고 소스 라운드는 공유되므로, 말 없는 공급자 하나가 다른 모든 공급자의 장애가
  된다. `Disconnected`를 답한 공급자는 즉시 해제되고 실패로 세지 **않는다** —
  실패한 것이 아니라 끝났다고 말한 것이다. 오라클은 omniORBpy 공급자다.

- **Generated code moves an identifier wherever a contract could shadow one.**
  Six causes over both emitters, measured by **2793 probes** — 147 identifiers
  × 19 positions, every emitted Rust crate compiled out of workspace and every
  emitted Python package imported. First pass **Rust 92 failures / 2793
  (96.7%), Python 39 / 2793 (98.6%)**; after, 0 and 0, in two rounds. The rule
  the batch was scoped to is *every identifier the generator emits is spelled
  by one function, every site that looks one up calls the same function, and
  everything the generated code does not itself define is reached by a path no
  contract can bind* — four of the six causes are not in the list of five
  defects it was handed. **Escaping moves the name** where the offender is the
  contract's own and no qualification reaches it (`r#Self` is not a raw
  identifier; `r#Ok` resolves to the same `Ok` a pattern matches): `ident`
  gains `CANNOT_BE_RAW`, `CANNOT_BE_A_BINDING` and `PRIMITIVES`.
  **Qualification moves the reference** where the offender is the generator's,
  because a list of library paths inside `ident` would be a second home for a
  fact `skeleton.rs` owns — so the site writes `::std::result::Result`, the
  runtime imports as `__rt`/`__Cdr`, and every hand-written binding is
  `__`-prefixed. An IDL identifier cannot begin with an underscore, so `__rt`
  is unshadowable and a bare `rt::` written tomorrow no longer compiles.
  Also fixed on the way and unrelated to any keyword: **a declaration outside
  any `module` emitted a file with no runtime import at all** — every corpus
  file opens a module, which is the whole reason nothing was red.

  **생성된 코드는 계약이 가릴 수 있는 식별자를 옮긴다.** 두 에미터에 걸쳐 여섯
  원인, **2793개 탐침**(식별자 147 × 위치 19)으로 측정. 1차 통과 Rust 96.7%,
  Python 98.6% → 두 라운드 뒤 각각 0 실패. 규칙은 *생성기가 내보내는 모든
  식별자는 하나의 함수가 적고, 찾는 쪽도 같은 함수를 부르며, 생성된 코드가 스스로
  정의하지 않은 것은 계약이 붙잡을 수 없는 경로로 가리킨다* 이며, 여섯 원인 중
  넷은 건네받은 다섯 결함 목록에 없었다. 도중에 함께 고침: **어떤 `module`에도
  들어 있지 않은 선언이 런타임 임포트가 전혀 없는 파일을 내보내고 있었다** —
  모든 코퍼스 파일이 모듈을 열기 때문에 아무것도 빨갛지 않았다.

### Added / 추가

- **A third target: Java clients, and the suite accepts it (D030 §5 L2, D032
  §4).** `orbweaver_gen::java` emits client stubs, the types they carry and a
  hand-written runtime (`_Rt.java`), and `spikes/bindings/java.manifest` is the
  suite's second instance. The verdict is the suite's: **cells run 3, skipped 3,
  red 0**, with `client × little` read from omniORB, `client × big` read from
  JacORB, and clause 6 met in the client direction — every one of those an order
  read off GIOP §15.4.1's flag byte rather than inferred from a peer's host.

  **The `client × jacorb` cell is the gap Python's own manifest names.** That
  file says nothing drives generated Python at a JacORB server, *"and that is
  precisely the cell that would give the client direction a big-endian reading
  off a foreign peer's flag byte — D030 §3.1's 'not established by that batch and
  not by this one', as a cell rather than a sentence."* It is established here
  for Java: 12 generated calls against `spikes/jacorb/Server.java` through the
  recording tap, JacORB's replies BE and omniORB's LE. Python's row is untouched
  — closing a gap for one binding does not close it for another.

  **No `org.omg.CORBA` anywhere, and that is a licence position rather than a
  taste.** JDK 11 removed CORBA (JEP 320), so the only `org.omg.CORBA` on a
  machine like this is JacORB's jar, and JacORB is an **LGPL fixture, never a
  dependency**. Generated Java speaks AnyJSON v1 over pipes to
  `orbweaver-py-bridge`, which owns the wire — the same seam D007 settled for
  Python, and it needed no second protocol. `client-jacorb.sh` asserts
  `cargo tree --workspace` is clean on every run, reading the producer's exit
  status before anything it printed.

  Scope, stated rather than discovered: **clients only.** A Java servant needs
  the bridge's serving direction to carry a dispatch into a Java process, which
  is the seam `pyservant.rs` implements for one language and has not been
  generalised; all three servant cells are counted `SKIPPED` naming that.

  *세 번째 대상: Java 클라이언트, 그리고 스위트가 그것을 받아들였다. 판정은
  스위트의 것이다 — 실행 3칸, 건너뜀 3칸, 빨강 0. `client × little`은 omniORB에서,
  `client × big`은 JacORB에서 **플래그 바이트를 읽어** 얻었고, 클라이언트 방향의 절
  6이 충족됐다. 이 `client × jacorb` 칸은 Python 매니페스트가 스스로 지목한 바로 그
  구멍이며, Java에 대해 성립했다 — 한 바인딩의 구멍을 메웠다고 다른 바인딩의 구멍이
  메워지지는 않으므로 Python의 행은 그대로 둔다. `org.omg.CORBA`는 어디에도 없다:
  JDK 11이 CORBA를 제거했으므로 이 기계의 유일한 구현은 JacORB의 jar이고, JacORB는
  의존성이 아니라 픽스처다. 범위는 **클라이언트 전용**이며 서번트 3칸은 이유를
  이름으로 부르는 SKIPPED다.*

- **`corpus/golden/28-target-keywords.idl` grew Java's positions and a
  template-locals section, and the coverage instrument stopped reading its own
  runtime.** Measured before the section existed: **17 of 59** words the Java
  emitter escapes were executed by the corpus, every one of them by accident,
  because it is also a Rust or Python keyword. After: **38 of 59**, and the 21
  remaining are the 20 words IDL reserves too plus `_`, each with a row in
  `spikes/bindings/keywords-not-executed.tsv`.

  Java's four **contextual** keywords are in the type-name position on purpose:
  `record`, `var`, `sealed` and `permits` are legal as variable names and fatal
  as class names, which is exactly where an IDL type lands, so a list holding
  only the 50 reserved words would have said Java was covered while
  `public final class record` failed to compile.

  **A compiler that is not ours agrees, by failing.** `org.jacorb.idl.parser`
  3.9 accepts the file and writes 179 Java files, 20 of which do not compile
  under JDK 21: `'sealed' is not allowed here` ×9, `'yield'` ×5, `'var'` ×3,
  `'record'` ×3. JacORB escapes the 50 reserved words and none of the contextual
  ones — the same class D030 §5 L2 recorded for its `catch (…IOException e)`
  template, in a second instance. Ours escapes all four and compiles, and that
  is a fact about the corpus section rather than about the emitter: without
  those six declarations nothing would have executed that escaping either.

  *우리 것이 아닌 컴파일러가 실패함으로써 동의한다: JacORB 3.9의 IDL 컴파일러는 이
  파일을 받아들여 Java 179개를 쓰지만 그중 20개가 JDK 21에서 컴파일되지 않는다 —
  예약어 50개는 이스케이프하고 **문맥 키워드는 하나도** 하지 않기 때문이다.*

  The template-locals section is D030 §5 L2's third consequence, executed rather
  than asserted: JacORB 3.9's own stub template writes `catch (java.io.IOException
  e)` into the same scope as an operation's parameters, so **the hazard is every
  identifier the template puts in scope, not the reserved words**. This emitter
  answers it by construction — every local, parameter and field it binds begins
  with `_`, and an IDL identifier can never begin with one — and the section
  names contract members `e`, `o`, `v`, `name`, `value`, `invoker` so the claim
  is compiled rather than believed. Writing it found the second form of the same
  hazard: an escaped member name **is** `_` plus the IDL name, so a constructor
  parameter spelled `_class` for the member `class` assigned the field to itself,
  silently and only for members whose names are Java keywords. Two prefixes are
  needed because one of them is already the escape.

  And the instrument's own defect: `targets::keyword_coverage` read the
  **verbatim runtime** along with the generated files, and both mappings escape
  with a leading underscore — so `java_rt.java`'s local `_default` made `default`
  read as covered by a contract that never names it, and `python_rt.py`'s
  `_lambda` did the same. A runtime is the same bytes for every contract and
  cannot be evidence about any of them; `targets::without_runtime` excludes it.
  Python's clause-5 verdict is unchanged at 28 of 37 by that fix, which is what
  makes it a repair rather than a re-tuning.

  *28번 계약이 Java의 위치들과 **템플릿 지역 변수** 절을 얻었다. 이전: 59개 중 17개
  실행 — 전부 우연히(Rust·Python 예약어이기도 해서). 이후: 38개, 남은 21개는 IDL도
  예약하는 20개와 `_`이며 각각 이유가 있는 행을 갖는다. Java의 **문맥 키워드** 넷은
  타입 이름 자리에 둔다 — 변수 이름으로는 합법이고 클래스 이름으로는 치명적이며, IDL
  타입이 앉는 자리가 바로 거기다. 위험은 예약어가 아니라 **템플릿이 스코프에 넣는 모든
  식별자**이고, 이 방출기는 모든 지역 이름을 `_`로 시작시켜 구조적으로 막는다 — 그리고
  그것을 쓰다 같은 위험의 두 번째 형태를 찾았다: 이스케이프된 멤버 이름 자체가 `_` +
  IDL 이름이므로, 생성자 매개변수를 같은 철자로 쓰면 필드가 자기 자신에게 대입된다.
  계측기 자체의 결함도 있었다 — 커버리지 검사가 **런타임을 함께 읽고** 있었고, 런타임은
  어떤 계약에 대해서도 증거가 될 수 없다. 이 수정으로 Python의 판정은 28/37 그대로이며,
  그것이 이 수정이 조정이 아니라 수리인 이유다.*

- **The Java emitter's cross-implementation oracle
  (`crates/orbweaver-gen/tests/java_target.rs`).** Value → Rust `to_json` → Java
  `_fromJson` → Java `_toJson` → Rust `from_json` → CDR, compared as **bytes** in
  both byte orders, over the whole golden corpus at once, plus every stub method
  driven by name through `_Rt.Loopback`. Measured 2026-08-26: **131 values and
  167 calls cross, both orders, over 37 golden contracts**; not measured and said
  out loud — 48 items the emitter refuses with a published sentence, 2 types this
  sweep has no witness for, 52 operations whose arguments or multi-value result
  the driver does not build.

  First pass: **89 failures, four root causes**, two of them in the emitter. A
  typedef's Java type was the alias *holder class* rather than the aliased type,
  so `typedef string Name` produced `String cannot be cast to ShortName` in 20
  contracts at once; and `::CORBA::TypeCode` as a member reached a catch-all and
  was refused by Java while Rust and Python both carry it — one contract of 37.
  The catch-all is gone and `descriptor` is exhaustive over `TypeCode`, so a
  thirty-fourth construct is a build error rather than a silent divergence.

  *Java 대상의 교차 구현 오라클. 값 → Rust → Java → Rust → CDR을 **바이트로**, 양쪽
  순서로, 골든 코퍼스 전체에 대해 한 번에 비교한다. 2026-08-26 측정: 37개 계약에서
  값 131개·호출 167개가 양쪽 순서로 건넌다. 1차 통과에서 실패 89건 → **근본원인 4개**,
  그중 둘은 방출기의 것: typedef의 Java 타입이 별칭 홀더 클래스였고(20개 계약),
  `::CORBA::TypeCode`가 포괄 분기로 떨어져 Java만 거부했다(37개 중 1개). 이제 그
  match는 망라적이며, 서른네 번째 구문은 조용한 어긋남이 아니라 빌드 오류다.*

- **A caller can no longer tell whether the target was loaded — and the leak
  was not where D029 §6.1 said it was.** The Activation row named
  `moe::Router::select`. `select` is a **contract, not a leak**, argued from
  corpus/golden/22: `Constraints` declares no residency member, so filtering
  would apply a constraint nobody expressed; the contract already gives load
  state two homes where it is a value a caller *asks for*
  (`moe::ExpertLoader::status`, `//@ ai_effect: read_only` with no `ai_authz`,
  and `moe::Capability::state` through `Expert::describe`); `Router` being
  `//@ ai_desc: Control-plane gate` settles that its callers **may know**,
  which is a right to be *told* and not a licence for a side channel; and —
  decisively — **a filter could not have closed it.** `select` answers at T and
  the caller dials at T+ε, so an expert RESIDENT at T can be evicted before the
  dial, and a reference from `Expert::delegate`, from an earlier call, or
  destringified never went through the operation at all. A filter changes how
  *often* a caller can tell, never *whether*.

  The reason the code gave for the omission was separately false and is
  corrected in `select`'s rustdoc: *"the caller's cue to `prefetch`"* names an
  action the cued caller generally cannot take — `prefetch` is `oneway`, it
  lives on `ExpertLoader`, an object `Router` never hands over, and it is gated
  `//@ ai_authz: moe.loader.residency`, which `moe.router.select` does not
  imply. The position it defended was right; its reason was not.

  **The leak was one layer down, in what the *reference* does across an
  eviction.** `residency::MissPolicy`'s two variants both answered
  `Located::Unknown` for an OFFLOADED expert, the POA turned that into
  `OBJECT_NOT_EXIST`, and the same reference invoked twice by the same caller
  answered differently. **`MissPolicy::Activate`** demand-loads inside `locate`
  and answers `Located::Here`; being a POA-level fact it holds for *any*
  target, which is what §6's criterion asks and what a fix inside one
  application contract could never have given. The refusal that had ruled
  demand loading out is **kept, with both its variants and its default**, and
  quoted verbatim in the new rustdoc — every clause of it is about **cost**,
  and priority zero ranks a cost below a leak. An id the loader never
  registered stays `Located::Unknown` under every policy, `Activate` included:
  that is not a load state, and inventing one on demand would answer for
  references nobody minted.

  Measured by `crates/orbweaver-object/tests/what_a_caller_can_tell_about_load.rs`
  — one live `Connection`, one reference, the expert evicted from the test
  thread, and the next observation compared whole (status, version, byte order,
  every byte). **Its control is in the tree**:
  `the_refusing_miss_policies_are_the_leak` runs the identical scenario under
  the two refusing variants and requires it to fail naming `OBJECT_NOT_EXIST`,
  so a green run is evidence about a leak rather than about a switch that has
  stopped working. It is also **the first thing in this workspace that routes a
  wire request through `Poa::dispatch_target`** — `USE_SERVANT_MANAGER`, the
  locator and the activation had been reached only by unit tests calling it
  directly, so a leak there could not have been seen by any caller because no
  caller could reach it. `spikes/leak_tests.sh`'s activation leg is MEASURED
  rather than a counted `SKIPPED`.

  **Not closed, and named rather than left to be found.** *Time*: a
  demand-loaded call is slower than a resident one and a caller with a clock
  can tell; in one process the alternatives are forwarding to a node where the
  target is loaded (there is no second node) or making every call as slow as
  the slowest. And in this repository a load is a state transition plus an
  opaque blob — two map writes — so the latency the refusal is about is real in
  a deployment and **absent from every test here**. *Deployment*: nothing in
  the tree mounts `ExpertLocator` on a served POA, so which variant production
  would run is undecided, and a deployment on either refusing variant leaks.

  **One line is owed in `spikes/run_checks.sh` and this batch does not own that
  file.** Each leak-test group carries a static `tp_measures_nothing`
  declaration while its leg is a counted `SKIPPED`, and `leak_leg` **fails** a
  `MEASURED` row whose group still declares it — deliberately, so a leg that
  starts measuring cannot be swallowed by a stale declaration. The activation
  leg now measures, so the harness is red on that group until the bare
  `tp_measures_nothing` between `bears_on activation` and `leak_leg activation`
  (line 4318 as written) is deleted. The alternative was to leave a counted
  `SKIPPED` naming a blocker that no longer exists, which is the
  green-while-measuring-nothing class. `spikes/leak_tests.sh`'s verdict prints
  the fix in the words the harness itself will print.

  *활성화 행이 지목한 곳에 구멍은 없었다. `select`는 구멍이 아니라 **계약**이며,
  거르기로는 애초에 막히지 않는다 — T에 답하고 T+ε에 걸기 때문이다. 코드가 적어
  둔 사유(*"호출자가 `prefetch`하라는 신호"*)는 따로 거짓이었다: `prefetch`는
  `oneway`이고 `Router`가 건네주지 않는 객체에 있으며 다른 권한으로 막혀 있다.
  구멍은 한 층 아래, 축출을 사이에 두고 **참조**가 하는 일에 있었고
  `MissPolicy::Activate`가 POA 수준에서 막는다. 요구 적재를 배제했던 거절문은
  변형과 기본값째로 남기고 그대로 인용했다 — 그 논거는 전부 **비용**이고 0순위는
  비용을 구멍보다 아래에 둔다. 대조군은 커밋 메시지가 아니라 테스트 파일 안에
  있다. 닫히지 않은 것: **시간**(이 저장소에서는 측정되지도 않는다)과, 서빙되는
  POA에 `ExpertLocator`를 올리는 것이 트리에 없다는 사실.*

- **The seed migration D026 §5 S1 owed, and the three answers D028 §4 M3
  ordered.** The seed corpus landed without its migration — *"byte-identity:
  not run, zero fixtures migrated"* — because all five fixtures lived in crates
  other batches held. It has now been run, and the reasons three fixtures still
  cannot be migrated are structural rather than scheduling.

  **The key collision is fixed.** `spike_experts` bound its `Server` to
  `b"MoE/registry"` and handed `ExpertService` the base `b"MoE"`, from which it
  derives `MoE/registry` — the same bytes by two routes, and nothing could go
  red because a `Server`'s key is read only by `Server::ior`, which that fixture
  never calls. `expert_service::MOE_BASE_KEY` is now the one spelling and a
  single `plane()` builds the server and servant together, so neither identity
  can be chosen without the other.

  **The node namespaces are two domains, not one vocabulary spelled twice** —
  the outcome D028 predicted as *"the cheaper answer"*, and it changed no
  fixture code. **A** is the operator's declared estate: out of band, closed,
  because `check_residency`'s default-deny is not a refusal unless membership
  is decidable. **B** is reported placement: `moe::Capability.placement_node`,
  written by the expert about itself, open, because a set whose membership is
  decided by the thing being admitted is not closed — checking it against the
  estate would turn `heartbeat` into admission control, a change to what the
  contract *does*. `moe-estate.json`'s first version listed the union of both
  under one `nodes` key and a gate passed over it; what that gate asserted was
  the one-domain answer, compiled in as bookkeeping, over a list built to make
  it pass.

  **`vision` was never a defect** — absent-by-decision in the tenancy world,
  pinned and `Resident` in the placement world, two questions about two
  objects. What was wrong is that no document said so. Both decisions are in
  `corpus/state/README.md`, EN and KO.

  **Migrated:** `spike_tenants` fully (tenants, regions, capabilities, costs,
  adapter deltas, policy domains, the grant, declared nodes) — **byte-identical**;
  `spike_experts` in its shared half — one added line, and that line exists
  because a seeded value reaching no output cannot be shown reaching anything,
  which made its first negative control come back green.

  **Not migrated, and why:** `orbweaver-test` sits *above* every crate the
  fixtures live in, so a fixture cannot name it without a cycle, and Cargo has
  no bin-only dependency. `orbweaver-object`'s two reach the loader by
  `#[path]` — one file, two compilations, no copy to drift. The three in
  `orbweaver-giop` and `orbweaver-registry` cannot reach it at all, because
  `orbweaver-dynamic`'s JSON parser is above them too. The structural fix is
  named in the README: move that parser down to `orbweaver-cdr`.

  **`spike_events` cannot be measured by this oracle at all**: its output is
  not a function of its inputs (10 runs of the untouched binary, 9 gave
  `dropped=3`, 1 gave `dropped=2`). Not diagnosed, and not claimed to be. The
  oracle's own determinism check had been two consecutive runs, both of which
  landed on the 1/10 variant — two runs is not evidence.

  *시드 이전이 **돌았다**. 키 충돌은 철자 하나로 고정되었고, 노드 이름공간의 답은
  **두 도메인**이며(픽스처 코드는 한 줄도 바뀌지 않았다), `vision`은 애초에 결함이
  아니라 **다르다고 적은 문서가 없었던 것**이다. 둘은 이전, 셋은 불가 — 이유는
  일정이 아니라 로더가 픽스처보다 의존성 그래프 **위에** 있다는 구조다.
  `spike_events`는 출력이 입력의 함수가 아니어서 이 오라클로 잴 수 없다.*

- **A leak test per transparency, and the harness now counts the ones that do
  not exist** (D029 §5 O0, D031 H2/H4). `crates/orbweaver-test/tests/what_a_caller_can_tell.rs`
  is the first instrument in this project that holds a **live caller** while the
  property a transparency names changes underneath it; `spikes/leak_tests.sh`
  reads the five names from their owner and gives one leg per transparency;
  `spikes/leak_controls.sh` puts each leak back and requires the test to see it,
  by exit code and in the test file's own sentence. Two legs measure — a target
  moved under a live caller, and the implementation behind one reference
  replaced mid-session. **Three are counted `SKIPPED`s naming a specific
  blocker**, which is the half that matters: a Python servant mountable as a
  `Dispatch` in a server the test owns; a POA-level activation path that reloads
  an evicted target; a redirect emitted for a **name** rather than for an object.

  Those scripts landed with nothing running them, because the batch that wrote
  them was given a footprint that excluded `spikes/run_checks.sh` — its report
  said so: *"the three SKIPPED are not counted by the harness verdict and do not
  reach D031's ledger"*. They are now **eight harness groups**, and wiring them
  in needed the ledger to be able to say something it could not. A group that
  declares `bears_on activation` is a group the ledger counts, so five counted
  skips would have flipped `activation` from UNMEASURED to *"measured by 1
  group(s), 0 red"* over a row D029 §6.1 calls the one with the most machinery
  and the least measurement — **the ledger swallowing a leak by being told about
  it.** `tp_measures_nothing` is the declaration that stops it: a transparency
  whose only declaring groups measured nothing still reads UNMEASURED, and the
  blockers they name become the load-bearing column instead of the reason the
  row left it. It is written at column 0 beside `bears_on` so that
  `spikes/ledger_control.sh`, which lifts those declarations and replaces every
  group body with an `echo`, reads a run the same way the run does.

  `spikes/orb_shutdown.sh` is a group too — the D034 lifecycle spike, whose peer
  imports no ORB and builds its §9.4 requests by hand in both byte orders. Its
  exit code is the measurement and the fixture's own counters are printed beside
  it, never allowed to vouch for it. So the lifecycle row moves from *no
  declaring group* to declared and measured, and **not** to held: what became
  measurable is the removal, not the transparency of the removal (D034 §8), and
  its leak leg stays a counted SKIPPED naming the redirect-for-a-name that would
  close it. Both lines print under the row every run.

  **Found by running the controls rather than by reading them.**
  `ledger_control.sh` control 5 had been **red since the acceptance suite gave
  `language` a second tag**, and nothing noticed, because that script was not a
  group — the instrument that answers *can the ledger be green while measuring
  nothing* was itself green while measuring nothing. Its assertion was a
  hand-typed group count; it is now scoped to the rule it exists for. Two more:
  the lift's `awk` anchors were title *prefixes*, so naming a new group
  `transparency ledger — its own negative controls` made the lift swallow it and
  the driver then ran the group, which ran the script — **unbounded recursion
  that hung rather than failing**, and a hang is the one diagnostic nobody can
  read; and `/^tp_measures_nothing/` in the lift also matched the function
  *definition*, producing a driver with an unterminated `{`. Anchors are full
  titles now and `build` refuses a driver that would re-enter.

  **Named rather than fixed**, because `spikes/leak_tests.sh` is another batch's
  file today: `--raw` is documented as one TSV row per transparency and, on a
  RED leg, also writes its failure extract to stdout — the stream is well formed
  exactly when nothing is wrong. The harness therefore sifts rows by shape. A
  consumer that trusted the line count read `8 row(s)` on the one run that
  mattered.

  What the ledger reads after this, from a standalone lift of the new groups:
  `location` and `backend` measured, `language` and `activation` **UNMEASURED
  with a named blocker each**, `lifecycle` measured by one group with its leak
  leg still unmeasured beside it. No score, and the empty case still reads as
  unmeasured rather than as passing.

  *투명성마다 구멍 테스트 하나, 그리고 **아직 없는 것을 하네스가 세기 시작했다.**
  `what_a_caller_can_tell.rs`는 이 프로젝트에서 처음으로 **살아 있는 호출자**를 붙든
  채 성질을 바꾸는 계기이고, `leak_tests.sh`는 다섯 이름을 소유 문서에서 읽어 다리를
  하나씩 놓으며, `leak_controls.sh`는 각 구멍을 되돌려 넣고 테스트가 그것을 보는지를
  종료 코드와 **테스트 파일 자신의 문장**으로 요구한다. 재는 다리는 둘, **구체적
  장애물을 이름 붙인 계수되는 `SKIPPED`가 셋**이며 후자가 중요한 절반이다.
  그 스크립트들은 아무것도 실행하지 않는 상태로 착지했다 — 그것을 쓴 배치의
  footprint가 `run_checks.sh`를 제외했기 때문이고, 그 보고서가 스스로 그렇게 적었다.
  이제 **하네스 그룹 여덟 개**다. 그런데 그렇게 연결하려면 원장이 할 수 없던 말을 할
  수 있어야 했다: `bears_on activation`을 선언한 그룹은 원장이 **세는** 그룹이므로,
  계수되는 스킵 다섯이 `activation`을 미측정에서 *"1개 그룹이 쟀고 붉음 0"*으로
  뒤집었을 것이다 — §6.1이 "기계는 가장 많고 측정은 가장 적은 행"이라 부르는 바로 그
  행 위에서. **구멍을 알려 줬다는 이유로 원장이 그 구멍을 삼키는 것이다.**
  `tp_measures_nothing`이 그것을 막는 선언이며, 선언한 그룹이 전부 아무것도 재지
  않았다면 그 투명성은 여전히 UNMEASURED로 읽히고 그들이 이름 붙인 장애물이 원장을
  지탱하는 열이 된다. 그 선언은 `bears_on` 옆 0열에 적힌다 — 그룹 본문을 `echo`로
  갈아 끼우는 `ledger_control.sh`가 실행과 같은 방식으로 읽어야 하기 때문이다.
  `orb_shutdown.sh`도 그룹이 되었다(D034). 그래서 생애주기 행은 **선언 그룹 없음**에서
  선언·측정됨으로 옮겨가되 **유지됨으로는 옮겨가지 않는다**: 측정 가능해진 것은
  제거이지 제거의 투명성이 아니며(D034 §8), 그 구멍 다리는 이름에 대한 리디렉션을
  기다리는 계수되는 SKIPPED로 남는다. **읽어서가 아니라 돌려 보고 찾은 것**: 인수
  스위트가 `language`에 두 번째 태그를 준 이래 `ledger_control.sh`의 대조군 5가
  **붉어 있었고 아무도 몰랐다** — 그룹이 아니었기 때문이다. "원장이 아무것도 재지
  않으면서 초록일 수 있는가"에 답하는 계기가 스스로 아무것도 재지 않으면서 초록이었다.
  그 단언은 손으로 적은 그룹 개수였고, 이제 존재 이유인 규칙으로 범위를 옮겼다. 두 가지
  더: 리프트의 `awk` 기준이 제목 **접두사**여서 새 그룹 이름 하나가 무한 재귀를
  만들었고 — 실패가 아니라 **멈춤**이었으며, 멈춤은 아무도 읽을 수 없는 유일한 진단이다
  — `/^tp_measures_nothing/`이 함수 **정의**까지 잡아 닫히지 않은 `{`를 만들었다.
  **고치지 않고 이름만 붙인 것**: `leak_tests.sh --raw`는 투명성당 TSV 한 줄로
  문서화돼 있으나 붉은 다리에서는 실패 발췌도 표준출력에 쓴다 — 아무 문제가 없을 때만
  형식이 온전하다. 그 파일은 오늘 다른 배치의 것이므로 하네스가 모양으로 걸러 읽는다.*

- **One acceptance suite, parameterised by language — `spikes/binding_suite.sh`
  (D032 §5 B3, D033 §3.1).** A binding is accepted by passing a suite, not by
  being written, and the suite is one suite rather than a copy per language: a
  per-language copy of an instrument drifts exactly the way a per-language copy
  of a *sentence* does. There is **no language name in the driver** — a language
  is `spikes/bindings/<name>.manifest` plus what it names, and the axis values
  live once in `spikes/bindings/AXES`. Python is its first instance and the
  instruments the cells run are unchanged, which is how "byte-identical results
  as an instance" is a property of running the same thing rather than a claim.

  The derivation is the design. **D032 §4's six clauses are not six checks:**
  clauses 3/4/5 are language-scoped (no peer, no wire), clause 1 is one
  measurement ranged over a (direction × peer) grid, and clauses 2 and 6 are
  **coverage requirements over that grid**. A suite with a "both byte orders"
  line would print `ok` for Python today off `python_target.rs` and
  `python_servant.rs`, which walk both orders with no peer in either — and, in
  `python_target.rs`'s case, no socket either. So an order **read** off GIOP
  §15.4.1's flag byte is `observed`, one inferred from the peer's host or
  language is `claimed`, and only `observed` counts toward clause 2.

  What it says for Python on a host with both fixtures: 5 of 6 cells supplied,
  0 red. `servant × big` is read off the wire from a foreign peer (JacORB, at
  IIOP 1.2 and 1.1); `servant × little` and `client × little` are exercised by
  omniORB but **never read**; `client × big` is reported by nothing; the client
  direction has **no foreign-peer reading at all**; GIOP 1.0 is reached by no
  cell in either direction; and `client × jacorb` — a JacORB *server* that
  generated Python dials — is the missing cell, D030 §3.1's prose printed as a
  counted `SKIPPED` on every run. No score, no percentage, no N-of-M: the
  verdict names what is unmeasured and does not count what is measured.

  *언어 바인딩은 작성됨이 아니라 **스위트 통과**로 인정되며, 그 스위트는 언어마다
  복사한 것이 아니라 **언어로 매개변수화된 하나**다 — 구동기에는 언어 이름이 없다.
  D032 §4의 여섯 절은 여섯 검사가 아니다: 셋은 언어만의 성질, 하나는 격자 위의 측정,
  둘(바이트 순서·외부 피어)은 그 격자에 대한 **커버리지 요구**다. 그래서 §15.4.1
  플래그 바이트에서 **읽은** 순서만 `observed`로 세고, 피어의 언어나 호스트에서
  추론한 것은 `claimed`으로 따로 적는다. 점수도 백분율도 없다.*

- **D032 §4 clause 5 gets an instrument, and it is a finding.** *"Its keyword
  escaping is exercised by `28-target-keywords.idl`"* had nothing measuring it:
  both emitters' keyword lists were private consts, so a word was covered by
  accident and a **new** word would be uncovered by accident with nothing red
  either way — which is how `yield` went missing from the Rust list in the
  first place. `orbweaver_gen::targets::TARGETS` publishes each target's
  reserved words, its **own** escaping function and a uniform emit-to-text;
  `binding-words --language L` asks, per word, whether the escaped spelling is
  in what the emitter actually wrote. Measured 2026-08-26: of Python's 37
  reserved words the contract set executes **28**, of Rust's 66 it executes
  **42**. `spikes/bindings/keywords-not-executed.tsv` is the home for the rest,
  checked in **both** directions — a word that stops being uncovered makes its
  own row a failure telling you to delete it. Three computed classes and only
  one is a gap: **eleven Rust primitives (`bool`, `u8`, `f64`, …) are exercised
  by nothing at all** — not the corpus file, and `one_spelling_for_an_identifier.rs`
  covers only `i32` of the twelve. Written down rather than fixed, because
  giving them a home is a corpus batch.

  *절 5에는 계측기가 없었다. 각 대상의 예약어와 **이미터 자신의** 이스케이프 함수를
  공개하고, 단어마다 "이스케이프가 실제로 실행되었는가"를 묻는다. 실제 구멍은
  하나 — Rust 원시 타입 이름 열한 개는 어디서도 실행되지 않는다.*


- **The ORB can stop what it handed out** (D029 §5 O1; decision
  [`D034`](docs/decisions/D034-stopping-what-the-orb-handed-out.md)).
  `Orb::shutdown`, `Orb::is_shutdown`, `Orb::wait_until_stopped(deadline)`,
  `Server::stop_flag`, `Server::stop_requested`, `ServerStats::serving`,
  `Pool::close`, `Pool::is_closed`, `Error::Stopped`.

  D019 step 4 made `Orb::server` and `Orb::pool` the only public way to obtain
  transport, and **that created the gap rather than revealing it**: before it,
  the caller held every `Server` it built and stopping was its own business.
  D019 §5's refusal of *"`ORB::run`/`shutdown` semantics"* is intact in the half
  that was refused — `run`, an event-loop model with a main thread parked in the
  ORB. Nothing about the serving model moves: the caller still owns the thread,
  `serve_shared` still takes the caller's own predicate, the ORB joins nothing,
  and its flag is OR'd with that predicate **with neither privileged**.

  **Shutdown is graceful and the unit of grace is one request, not one
  connection.** A request already inside the servant is answered in full; a
  request whose bytes had arrived but which had not been read is left unread;
  every live connection ends with `CloseConnection` (§9.4.10). The third makes
  the second obligatory rather than tidy — §9.4.7 makes that goodbye mean *"not
  processed, re-send elsewhere"*, so a request read after the flag and then
  dropped would make the goodbye a lie about a request that had been processed,
  and a peer acting correctly on it would run the operation twice. Immediate
  shutdown is **refused, not deferred**: GIOP has no message meaning *"I started
  this and stopped"*.

  The bound is on `Orb::shutdown`'s rustdoc and is deliberately restated
  nowhere. Measured from a peer's own socket — three GIOP versions × two byte
  orders, values compared decoded — in
  `crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`, with four
  negative controls each run red.

  **No behaviour change by default**, and that is a property of the expression
  rather than a claim about it: an ORB nobody asks to stop never raises its
  half. `orbweaver-object` 124 tests before and after; `orbweaver-giop` 438 →
  448, the ten being exactly the new file.

  *ORB가 내어준 것을 거둘 수 있다. 거절된 절반은 `run`이다 — 서빙 모델은 움직이지
  않는다. 우아함의 단위는 연결이 아니라 요청 하나이며, 즉시 종료는 미룬 것이 아니라
  거절했다: 시작해 놓고 멈췄다는 뜻의 GIOP 메시지가 없기 때문이다.*
- **The second protocol direction: a request our ORB decoded, carried into a
  Python servant and back.** `orbweaver-gen` emitted Python **clients** for six
  phases and could not emit a servant, so a target's language decided whether
  it could be a target at all. `crates/orbweaver-gen/src/pyservant.rs` is that
  direction — `PyServant` is a `Dispatch` that turns a `Request` into one JSON
  line and a reply line back, `orbweaver-py-bridge --serve` writes the call
  document and Python answers it, and `python.rs` emits a `<Name>Servant` base
  from `client_operations` so a Python servant answers exactly the names a
  Python client of the same contract can send.

  **It is a dispatch and not a wire**, which is the refusal D030 §4 states:
  GIOP framing, CDR, alignment, byte order, codeset negotiation, reply status
  and the repository id on a user exception all stay on the Rust side, and what
  crosses is an operation name and already-decoded values. The seam's refusals
  are the **published constructors** — `SystemException::bad_operation`,
  `::marshal`, `::object_not_exist` — called rather than reproduced, so a
  fourth wording cannot appear the way the generated Python runtime once wrote
  its own for `fixed`.

  Measured by `crates/orbweaver-gen/tests/python_servant.rs`: a Python servant
  and the generated Rust servant for the same contract answer **byte-identically
  over 19 calls × 3 GIOP versions × 2 byte orders**, with a negative control
  that perturbs five answers and asserts each is seen; `python_servant_wire.rs`
  adds our generated Rust client and omniORB's client against it. **Named
  unmeasured, not counted:** the omniORB leg runs in that peer's **native byte
  order only** — omniORB marshals native-endian and exposes no override — so
  the foreign-peer half is one byte order, which is a property of the peer and
  is recorded as a limit rather than as coverage.

  What this closes and what it leaves is D029 §6.1's Language row and §6.1.1,
  and is not restated here.

  *`orbweaver-gen`은 여섯 페이즈 동안 파이썬 **클라이언트**만 냈고 서번트를 낼 수
  없었다 — 대상의 언어가 대상이 될 수 있는지를 결정했다. 이제 두 번째 방향이
  있으며, **와이어가 아니라 디스패치**를 나른다: GIOP·CDR·정렬·바이트 순서는
  모두 러스트 쪽에 남는다. 거부 문장은 공개된 생성자를 **호출**한다. 19호출 ×
  3버전 × 2바이트 순서에서 러스트 서번트와 바이트 단위로 동일. **미측정으로
  명명:** omniORB 다리는 그 피어의 네이티브 순서뿐이다.*

- **A channel is reached by its name, and the address is not handed over**
  (Event E3). `publish_channels` binds every channel of an `EventChannelServer`
  into a naming context the caller supplies, under the single component
  `{ id: <name>, kind: "EventChannel" }`. The mapping is a decision stated on
  `CHANNEL_BINDING_KIND` rather than implied by the code: the name goes in `id`
  verbatim so the map stays injective, and the constant kind partitions the
  bindings inside a context a deployer may be sharing. A sub-context was
  rejected for having a lifecycle; an empty kind was rejected because a channel
  may legally be called `NameService`. It is a **free function and not a
  method** — binding is an outbound call, and a servant that makes one is the
  shape `crate::guarded` polices — and the registry lock is dropped before the
  loop that dials, so the no-lock rule is structural rather than careful.

  `crates/orbweaver-giop/tests/channel_found_by_name.rs`, 8 tests: a client
  holds an `Orb`, `corbaloc:rir:NameService` and a channel name, and **no
  channel IOR — enforced by `reach_by_name`'s signature rather than by a
  comment.** The experiment stops the channel's server and restarts it at a
  different address with the same object keys; `Observed` deliberately has no
  address field, so the assertion is *nothing the client sees changed*. Three
  negative controls, each run, each moving the counter: removing the shutdown
  so the pre-move IOR still works, holding a lock across the bind, and pushing
  a different event to show `before == after` is not vacuous. The peer half is
  `spikes/event_by_name.sh` — omniORB resolves the name, narrows to
  `CosEventChannelAdmin::EventChannel` and receives an event over a reference
  whose address it was never given.

  **What E3 closed and the four leaks it did not** are in D029 §6.1's Location
  material and are not restated here. One limit belongs with the change: the
  test re-runs the whole bootstrap, so it measures that a **new** client is
  unaffected and measures **nothing** about an existing connection surviving a
  move. That is stated in the test's own module docs where its next reader is.

  *채널은 이름으로 도달하고 주소는 건네지지 않는다. 매핑은 코드가 암시하는 것이
  아니라 `CHANNEL_BINDING_KIND`에 진술된 결정이다. 바인딩은 아웃바운드 호출이므로
  메서드가 아니라 자유 함수다. 부정 대조군 셋이 각각 붉어졌다. **한계:** 테스트는
  새 클라이언트가 영향받지 않음을 재고, 기존 연결의 생존은 재지 않는다.*

- **Two ORB features that had no chapter now have one, and one of them is half
  a transparency.** `docs/PLAN-DEFERRED.md` gains §21 Portable Interceptors and
  §22 BiDirectional GIOP. §22 is the one that bears on priority zero: it is a
  half of location transparency we hold, which is why it is a deferral with a
  reason rather than an omission. Both chapters graduate by §9's rule like any
  other; neither is scheduled here.

  *장이 없던 두 ORB 기능에 장이 생겼다. §22는 우리가 쥐고 있는 위치 투명성의
  절반이며, 그래서 누락이 아니라 사유 있는 보류다.*

- **A fact that crosses a crate boundary now names its dependents, and a binary
  is its own crate root.** `spikes/crossing_facts.py` (D028 §4 M2) prints which
  public items a branch changes and who names them. Three of 2026-08-26's merge
  breaks share one mechanical signature — **a public item changed in crate A
  and named in crate B**, where the two batches held disjoint footprints, no
  line was touched twice, `git merge-tree` reported no conflict, and the merged
  tree did not compile. That signature is computable from the diff without
  building anything.

  It groups by **compilation unit and never by directory**, because break 6 was
  `Server::bind` becoming `pub(crate)` and breaking
  `crates/orbweaver-giop/src/bin/spike_trading.rs` while four other binaries in
  the same crate were fine — and the human sweep for break 5 excluded
  `crates/orbweaver-giop/src`, the directory containing `src/bin`, and reported
  the rule holding workspace-wide over a break sitting inside it.

  **It is a report and not a gate, by instruction**, and its exit code is 0
  whether it finds something or not: *"names it"* is not *"breaks on it"*, so a
  crate naming a type in a doc comment is a true hit and a false alarm. It is
  for the person commissioning batches, which is what neither footprint list
  was.

  *브랜치가 바꾸는 공개 항목과 그것을 이름 부르는 곳을 출력한다. 디렉터리가
  아니라 **컴파일 단위**로 묶는다 — 이진 파일은 자기 크레이트 루트다. 게이트가
  아니라 보고서이며 종료 코드는 항상 0이다.*

- **The coverage sweep can see the trader, and now says which services it
  measures from a fixture's files.** `CosTrading::Lookup` had been on the wire
  since D022 T4 and outside `SERVICES-COVERAGE` §8 the whole time; the missing
  piece was never the servant but a first-party contract to derive an operation
  list from — `spikes/service_sweep.py` names its IDL inputs literally, and for
  the standard services those inputs are omniORB's **installed**
  `CosNaming.idl`, `CosEventComm.idl`, `CosEventChannelAdmin.idl` and `ir.idl`.
  So **what the document reported about our own servants was derived from a
  fixture's files**, and that fact lived in four literal paths inside one
  function.

  `corpus/services/trading-lookup-subset.idl` (written from the OMG
  specification; omniORB's `CosTrading.idl` was not opened) closes the first
  half: `spike-trading` joins the fixture list and the sweep measures **21
  declared, 21 served, 0 unmeasured** on the trader — `query` with a nil
  `offer_itr` for five matches under `how_many` 10, `NO_IMPLEMENT` for the same
  query under `how_many` 2, `UnknownServiceType` for a type nothing declares.

  The second half is `#SOURCES`: one row per service saying which file its
  operation list came from and whether that file is `first-party` or a
  `fixture`'s, classified by **where the file is** rather than by a hand-kept
  label, and rendered into §8 by `coverage_tables.py`. Three services are
  fixture-derived; the sweep names them every run instead of leaving it to a
  sentence. A declaration source that cannot be read is now a counted
  `UNMEASURED` naming the file rather than a traceback — a traceback is loud
  for a person and leaves no row for the renderer, so the document would keep
  whatever it last said.

  *`CosTrading::Lookup`은 D022 T4부터 와이어에 있었지만 §8은 그것을 볼 수 없었다.
  빠진 것은 서번트가 아니라 연산 목록을 끌어올 **일급 계약**이었다. 표준 서비스의
  입력은 omniORB **설치본**의 IDL이므로, 우리 서번트에 관한 보고가 픽스처의
  파일에서 파생되고 있었다. 이제 서비스마다 출처를 파일 위치로 분류해 매 실행
  출력한다.*

- **The harness gained a dimension: it now says what a caller can still tell**
  (D031 H1/H2, PROPOSED — the ledger lands, the decision's status is its own).

  81 groups answered *did anything regress* and none of them knew D029 §6's
  five transparencies existed. When that criterion was asked about directly, the
  answer had to be assembled from four batch reports, a decision document and a
  `grep` — a reading, not a measurement.

  **`spikes/transparency.py` reads the five names out of D029 §6.1 and does not
  contain them.** A `TRANSPARENCIES` list in the harness, or in a crate, would
  be a second home for names §6.1 already owns; the slug a group tags itself
  with is derived from the table's own first column (`**Activation / load**` →
  `activation`), so renaming a transparency there makes every stale tag fail by
  name rather than be ignored. A group declares `bears_on <name>`, and a name
  §6.1 does not have is a **failure naming the group and the bad name**.

  **The ledger** prints, before the verdict and computed from the run: per
  transparency, how many groups measured it, how many went red, and — the
  load-bearing column — what is named unmeasured, from the groups' own `SKIPPED`
  text and from §6.1's status cell, **both read at run time rather than copied**.
  No group moved, no group's verdict changed, `fail_total` and `skipped` keep
  their exact meanings: a group's redness is a delta of `fail_total` taken when
  the next `hr` starts, so none of the 81 had to be edited to report itself.

  **No score, deliberately.** The verdict line names the unmeasured
  transparencies instead of counting the measured ones — *a floor is not a
  figure*, and "3 of 5" is quoted as sixty per cent by the first person to
  repeat it, while today's honest movement was negative: one leak closed and
  three revealed. And **the empty case reads as unmeasured, not as success**:
  with nothing tagged the ledger prints `NO GROUP IN THIS RUN DECLARED A
  TRANSPARENCY` and the verdict prints `transparency: NONE measured in this
  run`, because a ledger whose empty state looks like a pass is the
  green-while-measuring-nothing class wearing a report's coat.

  **`spikes/ledger_control.sh`** runs the ledger's seven negative controls in
  about a second, starting no fixture and taking no lock — it cuts the ledger,
  `hr` and `bears_on` out of `run_checks.sh` with `awk` and runs those bytes
  over the real tag set, so it runs the harness's changes rather than a copy of
  them. Batches are told not to run `run_checks.sh`; a prohibition without its
  replacement is an instruction to skip the check.

  Ten groups declare, measured 2026-08-26: seven location, two backend, one
  language; activation and lifecycle are declared by nobody. **The MoE
  residency group was proposed for `activation` and declined** — it drives
  residency from the control plane, which is the one layer allowed to know load
  state, and never asks whether a caller holding only a reference can tell.
  The reason is written at that group.

  *하네스는 81개 그룹으로 "퇴행했나"에 답했고, 그중 무엇도 D029 §6의 다섯 투명성을
  알지 못했다. 다섯 이름의 집은 D029 §6.1이며 `spikes/transparency.py`는 그것을
  **읽을 뿐 담지 않는다**. §6.1에 없는 이름을 단 그룹은 무시되지 않고 **실패한다**.
  원장은 투명성별로 몇 그룹이 쟀는지, 몇이 빨간지, 그리고 무엇이 측정되지 않았는지를
  실행에서 계산해 찍는다. 점수는 없다 — "5분의 3"은 처음 인용하는 사람이 60%로 읽고,
  오늘의 정직한 이동은 **음수**였다. 그리고 **아무것도 태그되지 않은 실행은 통과가
  아니라 미측정으로 읽힌다**.*

- **The ORB owns the transport, and the configuration is live** (D019 step 4,
  the step the §5 shape approval gated; approved one-way 2026-08-26).

  `Orb::server` and `Orb::pool` are now the **only** public ways to a listener
  and a connection pool: `Server::bind`, `Pool::new` and `Pool::with_limits`
  became `pub(crate)`, `Pool`'s derived `Default` — a public constructor
  wearing another name — was removed, and `Poa::new` became `pub(crate)`
  behind `orbweaver_object::OrbPoa`, an extension trait carrying `create_poa`
  and `root_poa`. It is a trait rather than a method because `orbweaver-object`
  depends on `orbweaver-giop`, so the ORB's own crate cannot name a `Poa`; the
  ORB **hands out** a root POA rather than owning one, and that difference from
  D019 §5's picture is stated rather than papered over.

  **What this changes that step 3 did not.** Step 3 gave the eight numbers a
  home and tested it thoroughly — parsing, zero-refusal, round-trip, unset
  answering the compiled constant — and every one of those tests passed while
  **`-ORBmaxMessageSize 4096` changed nothing a peer could observe**, because
  every call site of `OrbConfig`'s eight getters was a unit test or a spike
  printing them. Held is not applied. The numbers now reach a `Server` through
  `Server::apply_orb_config` and every dialled connection through
  `Pool::acquire`, and `tests/orb_config_reaches_the_wire.rs` asserts a
  **difference a peer can see** for five of the eight, each against its own
  control. Closing the second constructor is what makes the gap impossible
  rather than merely fixed: a `Server` built beside the ORB was a place the
  configuration provably did not arrive.

  **A defect found on the way.** `Connection::move_to` — the forward and §9.6
  restart path — does `*self = next` and restores a hand-written list of
  fields. `max_message_size` and `fragment_threshold` were not on that list,
  so **a connection silently reverted to the compiled defaults on the far side
  of any redirect**, and both had public setters at the time. Nothing was red
  because nothing measured a limit *after* a forward. The five ORB numbers are
  now one `ConnectionLimits` value that moves as one thing.

  **Also.** `read_message_limited` threads the fragment ceiling that
  `read_message` read off a constant — the size ceiling was always a parameter,
  so half of one reassembly bound was configurable and half was not.
  `-ORBListenEndpoints`' refusal reason was rewritten: it said *"the ORB does
  not own the transport yet; construct a Server directly"*, and this commit
  made both halves of that sentence untrue. It now names the real remaining
  limit — a `Server` holds one `TcpListener` and the argument takes a list.

  **Not done, named rather than silent.** `pool::Limits`' five numbers still
  have no `-ORB…` key; `max_forward_hops` and `follow_timeout` are wired
  through to `Connection` and `Pool` but have no behavioural test; `stop_poll`
  and `fragment_threshold` are wired and observable only as timing and as
  outbound framing respectively.

- **The second half of the agent boundary: four IDL tools, and the edge that
  had to be reversed to write them** (D024 §5, 2026-08-26). The MCP surface
  advertised three tools — find a contract, read it, call it — and the whole
  S1–S5 pipeline reached an agent through none of them. It now advertises
  seven: `validate_contract` (S4), `diff_contract` (the §5.3 differ),
  `describe_type` (the registry), `preview_generation` (`gen`).

  **The blocker was a dependency pointing the wrong way.** `orbweaver-forge`
  depended on `orbweaver-mcp`, so the boundary crate sat *upstream* of the
  pipeline it exists to expose and could not call S4 or `gen` without a cycle.
  The whole source-level coupling was **one function** — `exposable_interfaces`,
  a pure question about a catalog — which moved to
  `Registry::exposable_interfaces`; `orbweaver-forge`'s dependency on
  `orbweaver-mcp` became a dev-dependency, and `orbweaver-mcp` now depends on
  `orbweaver-forge` and `orbweaver-gen`. The same inversion is what let the
  annotate-or-assume sentence acquire an owner (below): two tasks, one root
  cause.

  **Every tool returns findings, never a verdict** (D024 §3) — `Report::to_json`
  plus `repair_prompt`, because the caller is a generator that will quote the
  answer back and `{"ok": false}` throws away the position and the fix at the
  boundary where they matter most. `preview_generation` reports both targets and
  **what would be skipped and why**, in the §4.4 sentences `orbweaver-dynamic`
  already owns.

  **Each passes through the same interceptor chain as `invoke_operation`**,
  which is what makes this a trust-boundary change. The tool surface is itself
  a contract — `IDL:orbweaver/ContractTools:1.0`, four operations, each
  annotated `//@ ai_effect: read_only` — so every stage answers out of
  machinery that already existed with nothing special-cased, and an operator
  allowlists the tools (or one of them) with the `--expose` grammar they
  already use. **They are default-deny like everything else here.** What each
  stage means for a tool that takes IDL text rather than an object id is argued
  stage by stage in `orbweaver-mcp/src/contract.rs`.

  `describe_type` is gated **twice**, and the second gate is not a duplicate:
  the chain decides whether this agent may use the tool, and
  `type_is_reachable` decides whether *this type* is one an exposed interface
  reaches. Without it a tool allowed once would enumerate the data model of
  every interface the operator did not allow — a question the chain cannot ask,
  because it was asked about the tool and this is about the argument. Refusals
  do not distinguish "not reachable" from "does not exist", exactly as
  `describe_interface`'s do not.

  **`describe_type` and the IFR's `Contained::describe` agree, and the way two
  answers agree is by being one answer**: `ifr::contained_of` was a private
  method and is now published, so the name/`defined_in`/version triple is one
  function both halves call. `describe_type_agrees_with_the_ifr.rs` proves the
  rest end to end over real GIOP in both byte orders. They agreed on every
  field of every case on first measurement — an honest result, and not a strong
  one, since the derived fields agree by construction.

  The tool-list pin now asserts the triad followed by the four, in order, and
  **names `register_contract` as a tool that must never appear**: D024 §5
  excludes registration because an agent that can register a contract can
  change what other agents see.

  **One defect found by driving the shipped binary, with the whole suite
  green.** `describe_type` answered every id it would not describe with
  `Denied::InterfaceNotExposed`, so asking it about `IDL:bank/Account:1.0` —
  an interface passed to `--expose` on the same command line — replied *"is not
  exposed"*, sending the reader into the allowlist after a problem that was in
  the request. That is the RC-4 misdirection this codebase already refuses
  once, arriving by a third road. `Denied::NotAType { id, kind }` now answers
  it, **only for an id the exposure already exposes** — so it leaks nothing the
  caller could not have got from `describe_interface` — while a type nothing
  reaches and an id nobody declared keep the one indistinguishable answer. The
  compiler asked for the new variant's remedy and for its classification in
  `dryrun::Would::of`, which is that exhaustive match doing the job it was
  written for.

  *에이전트 경계의 나머지 절반. 막고 있던 것은 반대로 향한 의존이었다 — 함수
  하나가 전부였고, 그것을 옮기자 두 과제의 공통 원인이 사라졌다. 모든 도구는
  판정이 아니라 진단을 돌려주고, 모두 같은 인터셉터 체인을 지난다.*

- **The annotate-or-assume sentence has one home** (`orbweaver_forge::effect`,
  2026-08-26). An operation whose contract states no `ai_effect` has exactly
  two ways out, and **six sites said so in four vocabularies**: S4's
  `sidl/missing-ai_effect` fix (three values), S3's `s3/missing-ai_effect` fix
  (three, byte-identical — equal by luck, in another file), S3's
  `s3/effect-unknown` (four, the only site naming `safe`), the gate's
  `Denied::remedy` (two), the server's startup summary (none, names the flag),
  the console's catalog legend (none, and no remedy at all).

  **The difference in what each offers is real and survives**: it is a
  parameter, not a fork. S3 and S4 speak to a contract's author and offer the
  author's three values; the gate speaks to a refused caller and offers the two
  poles, because a remedy is not a menu of ways past a gate; the server and the
  console speak to an operator who is not editing the contract and name the
  flag instead. Flattening three into two would have been a regression dressed
  as a cleanup.

  The **vocabulary** went the same way: `policy::is_harmless` — the predicate
  the gate actually asks — had two hand-kept mirrors, one of which carried a
  doc comment admitting it was a mirror. They are now the same constant, so
  there is **nothing left to test** about their agreement; the drift is
  impossible rather than detectable.

  `orbweaver-test/tests/one_home_for_the_effect_sentence.rs` computes every
  expectation by **calling the function the layer is supposed to call**, in the
  shape `one_home_for_a_wire_refusal.rs` established — and two existing tests
  that had retyped these sentences went red on the way in, which is the rule
  catching itself.

  *여섯 군데가 네 가지 어휘로 말하고 있었고 아무것도 빨갛지 않았다 — 사실의
  범위는 워크스페이스인데 고정이 없었기 때문이다. 계층마다 제시하는 값이 다른
  것은 실제 차이이므로 매개변수로 남는다.*

- **The trading service is open: `CosTrading::Lookup::query` answers a client
  that is not ours.** D022 T3 and T4. `PLAN-SERVICES` §3 deferred the standard
  facade *until a foreign trading client is named*, and the naming is measured
  rather than asserted: omniORB 4.3.4 ships `CosTrading.idl` and generated
  Python COS stubs, so `spikes/trading_client.py` narrows to
  `IDL:omg.org/CosTrading/Lookup:1.0` and calls `query` with an ORB none of
  this repository wrote — **45 assertions, 0 failures** against
  `spike-trading` (2026-08-26). The licence boundary is untouched: omniORB
  runs as a separate-process wire peer over TCP, `cargo tree` is unchanged.

  **T3, the service type.** `orbweaver-trading::service_type` adds
  `ServiceType` — a name, an interface repository id, a property schema — and
  `TypedOfferStore`, which wraps `OfferStore` with one side table rather than
  an eleventh `Offer` field, so nothing that builds an `Offer` changes. The
  schema is checked at registration and refuses, each naming the offender: a
  declared kind that disagrees with the engine's (refused where the schema is
  *written*, not when a query runs), a property outside the closed ten, the
  same property twice, an offer missing a mandatory property (before the store
  is touched, so a refused registration leaves nothing behind), a heartbeat
  that moves a readonly property, and **super types** — there is no
  inheritance graph, so a query on a super type would quietly not match. **No
  `ServiceTypeRepository` servant**, per D022 §7.

  **T4, the wire.** `orbweaver-giop::trading_server` serves `Lookup` and the
  twenty attributes of the three interfaces it inherits. `offer_itr` is
  **always nil and the servant never truncates to make that true**: an
  `OfferIterator` is the POA-hosted-object-per-query lifecycle
  `COMPONENTS.md` records as deliberately not built for `DynAny`, so a query
  whose answer would not fit is refused with `NO_IMPLEMENT` — this
  workspace's own rule that `NO_IMPLEMENT` means *declared and deliberately
  not served*. A nil iterator from this trader therefore always means "that is
  all of them". The bound reaches the client as `max_return_card`, the
  specification's own name for it, answered from the same constant the refusal
  sentence quotes.

  What a client learns about this trader on the wire rather than from a
  comment: `register_if`/`link_if`/`proxy_if`/`admin_if` and `type_repos` are
  nil, the three `supports_*` are false, both hop counts are zero and both
  follow policies are `local_only`.

  **Finding — the engine has three result lists and the wire has one.**
  `Selection::unranked` (matched the constraint, the preference could not
  place it) was going to be dropped on the way out, which reports fewer
  matches than matched: the same false statement as truncating, since the
  caller cannot tell the answer is short and cannot ask again for the rest.
  They now go last, which keeps the argument the engine actually made — that
  argument was about being *first*, so a router taking the head of the list
  still never gets an unmeasured expert.

  **Finding — a test that asserts against the constant it is testing measures
  nothing.** Un-nesting `ILLEGAL_PREFERENCE_ID` to
  `IDL:omg.org/CosTrading/IllegalPreference:1.0` — the exact mistake its own
  doc comment warns about — left every `trading_server` test **green**, and
  omniORB went red at once with `UNKNOWN(UNKNOWN_UserException)`. The
  authority for those strings is the published OMG IDL, not us, so
  `every_repository_id_is_the_one_the_omg_idl_declares` now writes all eleven
  out a second time by hand: two independently typed copies of a string we do
  not own is the right shape, and deriving one from the other was the
  tautology it looked like.

- **The ORB has an object.** Three steps of D019, landed in order because each
  is specified in terms of the one before.

  **Step 1 — the initial references table.** `naming.rs` has parsed
  `corbaloc:rir:NameService` since Phase 1 and thirty lines later
  `ObjectUrl::to_ior` answered `InitialReference(_) => return None`.
  `Corbaloc` and `Corbaname` both worked because the caller supplied an
  address; `InitialReference` is exactly the case where no address is given and
  the ORB is supposed to know, and **nothing in the workspace knew**. Not
  deferred with a reason — there was no reason, anywhere. CORBA 3.4 §8.5.2
  fixes four things this did not get to choose: the sixteen reserved
  `ObjectId`s are OMG's (transcribed as `orb::RESERVED_OBJECT_IDS`);
  `list_initial_services` sits beside `resolve_initial_references` in the same
  sub clause, and a table that resolves but cannot be listed leaves a client
  guessing names; **the refusal is `InvalidName`, never a nil reference**, so
  `None` was not merely unhelpful but the one answer the sub clause forbids;
  and the namespace is flat by the specification's own sentence.
  `register_initial_reference` is §16.10.1, with both its `InvalidName`
  conditions on the method. **`to_ior` does not gain the table** — the answer
  to `corbaloc:rir:` is not in the URL, which is the entire difference between
  the three variants, and a lookup behind a name that says *convert* would make
  a pure conversion depend on ORB state every caller must then thread through.
  Its `Option` stops meaning *unanswerable* and starts meaning *this form's
  answer belongs to the ORB*.

  **Step 2 — `string_to_object` / `object_to_string` (§8.2.2).** `Ior::parse`
  read `IOR:`, `ObjectUrl::parse` read `corbaloc:`/`corbaname:`, and the table
  read `corbaloc:rir:` — three entry points in two modules, and **the caller
  had to already know which one it was holding**, which is the one thing a
  stringified reference exists to remove. They delegate; nothing outside
  `orb.rs` changed behaviour. §8.5.2's *"the application is responsible for
  narrowing"* is why a URL comes back with an **empty `type_id`** rather than
  one this function invented — an invented id is a claim the caller cannot
  check and would carry onto the wire.

  **Step 3 — `OrbConfig` and `-ORB` arguments (§8.5.1).** Seven limits a
  network operator reaches for first — `DEFAULT_MAX_MESSAGE_SIZE`,
  `DEFAULT_FRAGMENT_THRESHOLD`, `MAX_FRAGMENTS`, `MAX_FORWARD_HOPS`,
  `FOLLOW_TIMEOUT`, `DEFAULT_MAX_CONNECTIONS`, `STOP_POLL` — and **not one
  could be reached from outside the process**, so D015's acceptance sentence
  *"without editing Rust, without a rebuild"* was still false one layer below
  where that batch made it true. The syntax is the specification's and so is
  half the behaviour: `from_orb_args` returns `(config, surviving_args)`
  because §8.5.1 requires the ORB to remove what it consumed, and an
  unrecognised `-ORB…` is a **refusal, not a shrug** — §8.5.1 says `BAD_PARAM`,
  which hands over the *refused whole* property by standard. §8.5.3.2 fixes
  `-ORBInitRef <ObjectID>=<ObjectURL>` exactly, including the exclusion that is
  easy to miss and is implemented: a `rir` URL would tell the table to resolve
  a name out of itself. Four standard arguments this ORB does not implement —
  `-ORBid`, `-ORBServerId`, `-ORBListenEndpoints`, `-ORBDefaultInitRef` — are
  refused **by name with their reason and sub clause**, because "unrecognised"
  is a poor thing to say to an operator who typed a real argument. Every
  setting is an `Option`, so *no configuration changes nothing* is a property
  of the type; a `0` is refused for every cap and every duration, because a
  zero message ceiling refuses every message and a `0` in a configuration is
  almost always an absence that passed through a layer without this type.
  `Orb::with_config` builds the table **to one side** and moves it in only once
  every entry has been read, so one bad URL leaves no half-populated ORB.

  **ORB에 객체가 생겼다.** D019 세 단계. **1단계** — 초기 참조 테이블. 파서는
  Phase 1부터 `corbaloc:rir:`를 읽었고 서른 줄 아래에서 `to_ior`가 `None`을
  답했다. 이유를 적고 유예한 것이 아니라, 어디에도 이유가 없었다. §8.5.2가 네
  가지를 정해 주었다 — 예약 아이디 열여섯은 OMG의 것, `list_initial_services`는
  같은 절에 있고, **거부는 `InvalidName`이지 nil 참조가 아니며**(그러므로 `None`은
  절이 금지하는 바로 그 답이었다), 이름공간은 평평하다. **2단계** —
  `string_to_object`/`object_to_string`. 진입점 셋에 두 모듈, 그리고 **호출자가
  자기가 무엇을 들고 있는지 이미 알아야 했다** — 문자열화된 참조가 없애려고
  존재하는 바로 그것이다. **3단계** — `OrbConfig`와 `-ORB` 인자. 운영자가 가장
  먼저 손대는 일곱 한계 중 **하나도 프로세스 밖에서 닿을 수 없었다.** 문법도
  동작의 절반도 규격의 것이며, 인식되지 않는 `-ORB…`는 **어깨를 으쓱하는 것이
  아니라 거부**다.

- **The trading service's two languages.** D022 T1 and T2, engine only — no
  wire surface, no new crate, `cargo tree -p orbweaver-trading` still prints
  one line.

  **T1 grows the §4.3 constraint subset**: `OR`, `NOT`, parentheses and
  `EXIST`, with precedence written down rather than implied and `AND`/`OR`
  chains parsed flat, so a fifty-thousand-conjunct query costs a loop instead
  of fifty thousand stack frames. The finding is that **`AND`/`OR` cannot tell
  three-valued logic from two, and `NOT` can**: with monotone connectives only,
  an expression is `Yes` exactly when it is true with every unknown replaced by
  false, so for the whole grammar as it stood "three-valued matching" and "a
  field nobody recorded does not match" returned the *same* offers. `NOT`
  breaks that in the dangerous direction — under *missing means false*,
  `NOT specialization == 'math'` **returns the expert nobody ever described**,
  the original bug `Truth` was built to prevent arriving through the new
  operator. Here it stays `Unknown`, lands in `Selection::unanswerable`, and
  `EXIST` turns a gap the report could only *name* into one a query can close.
  The three-valued tables are **chosen, not cited**: they are Kleene's strong
  logic, which is also SQL's; TCL is a separate OMG document and the copy of
  Part 1 available carries no TCL grammar, so no normative text is quoted for
  this behaviour.

  **T2 adds the preference expression** — `MAX`, `MIN`, `WITH`, `RANDOM`,
  `FIRST` — as its own module, because `CosTrading::Lookup::query` takes
  constraint and preference as two parameters of two grammars. Five semantics
  were decided here with their reasons and **none is quoted from a text nobody
  read**. `MAX`/`MIN` take a bare numeric field and refuse `residency` and the
  text fields by name even though they have a total order, because reading
  "the largest value of a number" as "the last enumerator" would be this engine
  deciding what a standard word means. `WITH` over an unanswerable offer places
  it in **neither** group and goes to `Selection::unranked`, which makes the
  consequence worth stating plainly: **the constraint decides membership and
  the preference decides order, and their gaps land in different places.**
  `RANDOM` is a seeded permutation with the seed in the text, because `replay`
  reproduces a trace bit for bit and an unseeded shuffle would end that in the
  place easiest not to notice — `shuffle_key` is written out rather than
  reached for, since `DefaultHasher` is explicitly not stable across Rust
  releases. **`RANDOM` alone is refused**, and an empty preference is refused
  rather than defaulted to `FIRST`, because inventing a documented default
  would be a semantic nobody could check. `MAX f`/`MIN f` are `ORDER BY f
  DESC`/`ASC` **exactly**, pinned offer-for-offer over seven pairs including
  the gapped field; a query carrying both is refused by name rather than having
  one win.

  **트레이딩 서비스의 두 언어.** D022 T1·T2, 엔진 한정. T1은 `OR`·`NOT`·괄호·
  `EXIST`를 더한다. 발견은 **`AND`/`OR`는 3치 논리와 2치를 구별하지 못하고 `NOT`은
  한다**는 것 — 단조 결합자만으로는 "미기록은 거짓"과 결과가 같았고, `NOT`이
  그것을 위험한 방향으로 깬다(*아무도 기술한 적 없는 전문가를 반환한다*). 3치 표는
  **인용이 아니라 선택**이며, 읽지 않은 문서를 인용하지 않는다. T2는 선호 표현식을
  더한다. **제약은 소속을, 선호는 순서를 정하며 각자의 공백은 다른 곳에 떨어진다.**
  `RANDOM`은 씨앗이 본문에 있는 재현 가능한 순열이고, 씨앗 없는 `RANDOM`과 빈
  선호는 거부된다.

- **The console's read half of administration.** D024 §6 item 1: three
  commands — `services`, `config`, `stats` — as **subcommands of
  `orbweaver-console`, not a second `orbctl` binary**. The deciding argument is
  `tests/escaping.rs`, which asserts *structurally* that no page carries an
  element this crate did not write, over an allowlist of eighteen literal tags;
  a second binary would have duplicated `Output`, the `--html`/`--text`
  contract and the usage text, and would have sat outside that proof until
  somebody remembered to extend it. Nothing here ends a channel, deactivates a
  POA, drops a connection or registers anything. **What an operator cannot see
  is said in words**: `PoolStats`, `ServerStats` and `ChannelStats` live inside
  a running process and D024 §7 refuses a wire interface for administration
  until a caller model exists, so `orb::Snapshot` is the input in both honest
  forms — live, for a caller inside the process, and a file that process
  writes. **Nothing in this workspace writes one yet**, so today an operator can
  point this at a snapshot and cannot point it at a running server, and learns
  that from the tool's own refusal rather than from an empty page. **The
  sixteen reserved `ObjectId`s are deliberately not in this crate**: the
  snapshot's *writer* states reservedness per id, because the writer is the
  ORB, and where the writer said nothing the row renders `not stated` — a third
  state and not a `no`, which is behavioural rather than cosmetic, since
  omniORB answers `NO_RESOURCES` for a reserved id with nothing bound and
  `BAD_PARAM` for a name it never heard of. The seven ORB values are read from
  the constants that own them and every one says **compiled default**. The drop
  split is never re-summed: five causes, five rows, and the reconciliation is
  `ChannelStats::split_adds_up()` *called*, not re-implemented.

  **콘솔이 운영의 읽기 절반을 갖는다.** D024 §6-1: `services`·`config`·`stats`를
  **두 번째 바이너리가 아니라 `orbweaver-console`의 하위 명령**으로. 결정 근거는
  `tests/escaping.rs`다 — 두 번째 바이너리는 누군가 기억해 확장할 때까지 그 증명
  밖에 앉아 있었을 것이다. **아직 이 워크스페이스의 무엇도 스냅샷을 쓰지 않으므로**
  오늘 운영자는 실행 중인 서버를 가리킬 수 없고, 그것을 빈 페이지가 아니라 도구
  자신의 거부에서 배운다. **예약 아이디 열여섯은 의도적으로 이 크레이트에 없다** —
  쓰는 쪽이 ORB이므로 예약 여부도 쓰는 쪽이 말하고, 말하지 않은 자리는 `not
  stated`라는 셋째 상태로 그려진다.

- **A deployment's numbers have a home that is not a source file.**
  `orbweaver-mcp` gains `--config <policy.json>`, **named and never
  discovered** — a file this process found on its own could start applying to a
  deployment nobody changed. Parsed with `orbweaver_dynamic::json`, so no
  dependency and the workspace third-party set is unchanged. The rule it was
  scoped to is not "the TTL, the quota and the exposure" but *a number or a
  policy only a deployment can know has one home*, so the neighbours were
  re-measured and that **changed the count in both directions**: of twenty-one
  hard-coded values, **seven moved** and fourteen stayed with the reason
  written where they live. Verified before building rather than after — and
  D015 §3.1 was wrong in three different ways. *How long* was **worse than
  stated**: `CapabilityTable::with_ttl` is a *consuming* builder while a
  `Bridge` builds its own table and shares it with every `Guarded` it issues,
  so the one door that existed could not reach the one table that matters — the
  policy was not merely unwired, it was unreachable by construction. *How
  often* was **half-stated**: `--quota` had installed the seat from the command
  line since the ledger batch, so the operator had somewhere to put the number,
  just not a file. *Who may call what* was **one word narrower** than stated:
  exposure was already populated from `argv`, so it needed a restart, never a
  rebuild. Three properties, each the reason for the next: **absent is not
  zero** (every setting is an `Option` and no default is restated in the new
  module — it references the constants that own them); **default-deny cannot be
  widened by an absence** (a missing, empty or absent `expose` leaves the
  allowlist where the command line put it); **refused whole or applied whole**
  (a key no setting is named by stops the process naming the file, the key and
  what was expected, because `handles.ttl_second` is a setting an operator
  believes is in force, and ignoring it is the harness's silent skip arriving
  through a config file).

  **배포만 아는 수치의 집은 소스 파일이 아니다.** `--config <policy.json>` — 이름을
  주어야 하고 스스로 찾지 않는다. 규칙을 "TTL·쿼터·노출"이 아니라 *배포만 알 수
  있는 수치는 집이 하나다*로 잡아 이웃을 다시 측정했고, 그 결과가 **개수를 양쪽으로
  바꿨다**: 스물하나 중 일곱이 옮겨가고 열넷은 이유와 함께 남았다. D015 §3.1은 세
  가지로 틀려 있었다 — 하나는 명시된 것보다 나빴고(구조적으로 도달 불가), 하나는
  절반만 맞았으며, 하나는 한 단어만큼 좁았다.

- **A refused call now says what would make it legitimate.** `Denied::remedy()`
  gives each of the twelve refusals an agent can receive a next step — which id
  is not allowlisted, which scope the contract asked for, which annotation is
  missing, who may approve — and **nothing in it is inferred, discovered or
  guessed**: every clause is built from a field the refusal was already
  carrying when it was raised. It is **a second sentence and not a field, and
  that is a decision about reach**: every reader of a refusal in this crate
  already takes it as prose through one rendering, so a field would have taught
  exactly the readers somebody rewrote to ask for it and silently not the
  others — a fact with two homes. **The rule the batch could not break is
  written at the site**: a remedy names an act belonging to somebody who is not
  the caller, and never a route the agent can take by itself.
  `REMEDY_ACTORS`/`REMEDY_FORBIDDEN` are published beside `remedy()` and read
  by the tests rather than retyped. The apparent exception is a renewing
  budget, where waiting *is* legitimate — that gate bounds a rate and not a
  permission. `remedy()` returns `String` with an exhaustive match and no `_`
  arm, and that is the codification: a rule about diagnostics that lives only
  in a document is a rule the next variant's author will not read.
  `EffectUnstated`'s `Display` lost its own second sentence, because two copies
  of one sentence is how a sentence goes false in one of them.

  **거부된 호출이 무엇이면 정당해지는지 말한다.** 에이전트가 받을 수 있는 거부
  열둘 각각에 다음 걸음이 붙는다. **추론·발견·추측은 하나도 없다** — 모든 절은
  거부가 제기될 때 이미 들고 있던 필드에서 만들어진다. **필드가 아니라 두 번째
  문장이며 그것은 도달 범위에 대한 결정이다.** 깨뜨릴 수 없는 규칙은 현장에 적혀
  있다 — 구제책은 호출자가 아닌 누군가의 행위를 이름하고, 에이전트가 스스로 갈 수
  있는 경로는 결코 이름하지 않는다. `_` 팔 없는 전수 매치가 그 성문화다.

- **The POA's seven policies are written down, cited, with the honest word for
  each.** D020 Stage A. `crates/orbweaver-object` cited CORBA 3.4 **zero times**
  while being the half of CORBA a server author meets, and a POA has the seven
  policies of §15.3.8 whether or not anyone names them — so not naming them did
  not make the choices absent, it left seven facts with no home. **No signature
  changes and no behaviour changes**; `Poa::policies()` computes its answer from
  fields that already existed. Two corrections came out of writing it down.
  **Servant Retention is RETAIN, not NON_RETAIN**: D020 §3 read it off the name
  `ServantLocator`, which is the specification's NON_RETAIN half, but
  `dispatch_target` inserts the located id into `active` and it survives the
  request, so the next request is served with no locator passed at all — that
  is RETAIN with a `ServantActivator` under a name borrowed from the other
  half. And **`USE_SERVANT_MANAGER` with no manager diverges** (§15.3.8.6 says
  `OBJ_ADAPTER` minor 4; we answer `OBJECT_NOT_EXIST`), recorded in the doc
  comment and deliberately not fixed, because Stage A changes no behaviour.
  `IdAssignmentPolicy::Either` is **ours, not the specification's** — §15.3.8.4
  makes it a per-POA choice and one adapter here answers to both models — named
  in the type and documented as the backward-compatible mode a new POA should
  not want. `Policies::spec_violations()` compiles the three constraints
  §15.3.8 states *between* policies; it reports and refuses nothing, and it
  went red twice under the controls, on combinations it was written for. **Two
  claims have no behavioural test and say so in their own documentation rather
  than being covered by a test that would pass whatever they said**: Thread
  (§15.3.8.1) is not observable from this crate, and Object Id Uniqueness
  (§15.3.8.3) is not observable *in principle* — a policy about servants, in a
  map that holds none.

  **POA의 일곱 정책이 인용과 함께, 각자에 맞는 정직한 단어로 적혔다.** D020 A단계.
  이 크레이트는 서버 작성자가 만나는 CORBA의 절반이면서 CORBA 3.4를 **한 번도**
  인용하지 않았다. 아무도 이름하지 않아도 정책은 있으므로, 이름하지 않은 것은
  선택을 없앤 것이 아니라 사실 일곱을 집 없이 둔 것이다. **시그니처도 동작도
  바뀌지 않는다.** 적으면서 둘이 교정되었다 — 잔류 정책은 NON_RETAIN이 아니라
  RETAIN이었고, 매니저 없는 `USE_SERVANT_MANAGER`는 규격과 어긋난다(기록만, 수정
  없음). **행동 테스트가 없는 두 주장은 무엇을 말하든 통과할 테스트로 덮는 대신
  자기 문서에 그렇게 적는다.**

- **`ai_example` and `ai_precond` reach a reader.** Both were in SIDL's
  known-key list — so writing one tripped no `unknown key` — with **no consumer
  anywhere in `crates/` and no user anywhere in `corpus/`**: the contract
  language had a slot for a worked example and a slot for a precondition, the
  two things a prompt most needs and a type contract least carries, and both
  were empty and unread. `Subject::to_prompt()` renders an authored
  precondition **above** the signature it constrains and an authored example
  **below** the one it instantiates, and **where in the prompt is the whole
  decision**: a precondition read after the signature is advice about a call
  the reader has already composed; read before it, it is a guard the signature
  is read through. An example is the opposite — above the line it is a literal
  with nothing to be a literal of. Both are marked `[authored]`, because they
  are the only text on that page a person wrote and D025 §7 forbids inferring
  into either slot, which is what makes the marker safe to trust. The preamble
  is conditional for the same reason: *"No IDL file, no comments and no source
  exist for it"* stops being true the moment one operation carries a
  hand-written precondition. Eight corpus operations gain one, hand-written —
  and `27-bounds`'s pair is not a restatement of its typedefs, because
  `render_type` prints `sequence<Tag>` for `TagSeq` and `string` for a
  `string<8>`, so a reader shown the rendered signature is shown a contract
  with no bounds in it at all.

  **`ai_example`과 `ai_precond`가 독자에게 닿는다.** 둘 다 알려진 키 목록에 있어
  적어도 `unknown key`가 나지 않았고, **`crates/` 어디에도 소비자가, `corpus/`
  어디에도 사용자가 없었다.** `Subject::to_prompt()`가 전제조건을 시그니처 **위에**,
  예시를 **아래에** 그린다 — **프롬프트의 어디인가가 결정 전부다.** 시그니처 뒤에
  읽는 전제조건은 이미 구성한 호출에 대한 조언이고, 앞에 읽으면 시그니처를 통과해
  읽는 가드다. 예시는 그 반대다. 둘 다 `[authored]`로 표시되며, D025 §7이 두 칸에
  대한 추론을 금지하는 것이 그 표시를 믿을 수 있게 만든다.

- **Every IDL rule id is a documented constant with one construction site.**
  `orbweaver_idl::rules` names each rule id, says which single diagnosis it
  names, and publishes `ALL`; every site in `lex`/`parse`/`sema`/`include` uses
  one, and a test scans this crate's own source and fails on a site that spells
  an id itself. `ALL` is what lets a consumer's hint table be checked against
  the rules that exist at all — the comparison nobody could make before.
  `tests/negative_corpus_rules.rs` is the table: the rule every file in
  `corpus/negative/` files under, every file rejected, the table and the
  directory holding the same files, and every rule in `ALL` either reaching a
  file or **named with the reason it does not**.

  **모든 IDL 규칙 아이디가 문서화된 상수이며 구축 지점은 하나다.** `ALL`은 소비자의
  힌트 표를 *존재하는 규칙 전체*와 대조할 수 있게 하며, 그 대조는 이전에 아무도 할
  수 없었다. 음성 코퍼스의 규칙 표가 함께 착지한다.

- **A server stops being a channel.** `EventChannelServer` held one
  `Arc<Shared>` and three fixed keys, so a *process* was a channel. It now
  holds a map from a channel name to that same `Arc<Shared>` and those same
  three keys, and every operation is answered through the channel its object
  key routed to — **no new state and no new wire surface**; what a second
  channel needed was a map and a rule about keys. **No factory, and why**:
  `CosEventChannelAdmin` declares none — the factory in the standard is
  `CosNotifyChannelAdmin::EventChannelFactory`, which belongs to CosNotification
  and is deferred — so creation is a Rust API and a deployment decision exactly
  as `Poa` creation is, and an Orbweaver-specific factory interface is refused
  as a fifth wire surface nobody asked for. **The key rule carries its proof in
  its own doc comment**, because two names that mint the same object key are
  two channels that are one channel and the symptom is silent: a supplier
  pushing into one is fanned out to the other's consumers with every counter
  agreeing. A name must be non-empty, contain no `/`, and not be a segment this
  module mints for itself. Routing is exact membership, never a prefix match —
  a prefix match would make the naming rule a suggestion, since `base/x/pps1`
  begins with `base` too. **Two outbound threads per channel, not per server**:
  channels are the unit a slow peer can wedge, so one shared delivery thread
  would make one channel's dead consumer every other channel's latency — the
  failure this module is built around avoiding, one level up. A channel created
  *after* delivery started spawns its own pair there and then, because a
  channel with no outbound threads is invisibly wrong: it answers every
  operation and reports rising `accepted`, looking exactly like a channel whose
  consumers are all slow. `total_stats()` answers the one question a *process*
  is asked — did anything here lose an event — and **states its limit where it
  lives**: it cannot say *which* channel, and nothing divides by the channel
  count to guess. A server built the old way is a server with one channel whose
  keys are its `base_key` verbatim, and `spike-events` is byte-for-byte
  unchanged across the commit.

  **서버가 채널이기를 그만둔다.** 프로세스 하나가 곧 채널이었다. 이제 채널 이름에서
  같은 `Arc<Shared>`로 가는 지도를 들고, 모든 연산은 객체 키가 라우팅한 채널을 통해
  답해진다 — **새 상태도 새 와이어 표면도 없다.** 표준의 팩토리는 CosNotification의
  것이라 유예 중이므로 생성은 `Poa` 생성과 똑같이 Rust API이자 배포의 결정이다.
  **키 규칙은 자기 주석에 증명을 달고 있다** — 같은 키를 만드는 두 이름은 하나인 두
  채널이고 증상이 조용하기 때문이다. 라우팅은 접두사 일치가 아니라 정확한 소속이며,
  **송출 스레드는 서버당이 아니라 채널당 둘**이다.

### Measured / 측정

- **A big-endian peer calls a Python servant, and the order comes off the flag
  byte.** The servant seam landed with omniORB's client calling a Python
  servant behind our ORB and named the half it could not reach: omniORB emits
  its native order and `orbweaver-giop` replies in the *request's* order, so on
  a little-endian host every byte of that exchange is little-endian, and D030
  §3's *"both byte orders against a peer that is not us"* was **not met**.
  JacORB reaches it. `spikes/jacorb/GaugeDriver.java` drives the same contract
  through a recording relay inside
  `crates/orbweaver-gen/tests/python_servant_wire.rs`, and the assertion is
  over §15.4.1's flag bit of every request the peer actually wrote — never over
  what the peer was told or what its language is said to do. Measured at IIOP
  1.2 and 1.1: **12 requests from JacORB, big-endian; 11 replies from our
  server, big-endian** — the twelfth request is the oneway, so §9.4.1's "no
  reply at all" is now visible on a foreign wire — and the Python servant's
  replies are **byte-identical to a Rust servant's for the same driver run,
  11/11 at each version**. Comparing raw bytes is the exception `CLAUDE.md`
  names, not a breach of it: both encoders are ours, so a difference in the
  bytes is a difference a caller could observe; the peer's own bytes are read
  and never compared. Two negative controls, both run:
  `--expect-order little` makes the order assertion name what was on the wire
  (`left: ["big"] right: ["little"]`), and `--perturb` makes a Rust servant
  answer one `sequence_no` the Python one would not, which the byte comparison
  catches as *reply 2 of 11* with both hex strings printed. **What this does
  not close**, named rather than left looking closed: the *client* direction's
  both-order test is a loopback with no peer in it and its live peer writes its
  native order (D030 §3.1); GIOP 1.0 is unmeasured against JacORB; and there is
  one peer per order, so a difference that is really "which ORB" rather than
  "which order" would be invisible (D029 §6.1.1).
  Group: `./spikes/jacorb_python_servant.sh`.

  *빅엔디언 피어가 Python 서번트를 호출하며, 순서는 피어의 언어가 아니라 요청
  헤더의 플래그 바이트에서 읽는다. IIOP 1.2·1.1에서 요청 12·응답 11이 모두
  빅엔디언이고(열두 번째 요청은 oneway라 응답이 없다), Python 서번트와 Rust
  서번트의 응답은 각 버전에서 11/11 바이트까지 동일하다. 부정 대조군 둘 다 붉게
  만들었다. **닫히지 않은 것:** 클라이언트 방향의 양쪽 순서, JacORB에 대한 GIOP
  1.0, 그리고 순서마다 피어가 하나뿐이라는 것.*

- **JacORB's own IDL compiler emits Java that does not compile for a parameter
  named `e`.** Found while building the fixture above.
  `org.jacorb.idl.parser` 3.9 writes `catch (java.io.IOException e)` into every
  operation's stub body, in the same scope as that operation's parameters,
  while every other local it emits is `_`-prefixed — so
  `long scale_all(in double e)` produces two compile errors and nothing in the
  package builds. `corpus/golden/24-skeleton-surface.idl` has that parameter on
  purpose (*"`e` is what a hand-written encoder would have called its
  encoder"*), and it turns out to catch a third-party emitter too. The hazard
  is **not** the reserved-word class `28-target-keywords.idl` covers — `e` is
  not a Java keyword — it is every identifier an emitter's own template puts in
  scope. Recorded in D030 §5 L2 against the prediction it falsifies. The
  fixture's copy of the contract renames the parameter, which costs the
  measurement nothing because a parameter name is not on the wire; a guard
  asserts the corpus file still contains exactly the one string being replaced,
  so the copy cannot silently rename nothing.

  *JacORB 3.9의 IDL 컴파일러가 `e`라는 매개변수 이름에 대해 **컴파일되지 않는
  Java**를 낸다 — 스텁 템플릿이 매개변수와 같은 스코프에 `catch (...IOException
  e)`를 두기 때문이다. 예약어 위험이 아니라 **방출기 템플릿이 스코프에 넣는
  식별자** 위험이며, `e`는 Java 예약어가 아니다.*

- **The census that would have inverted D006 was taken once, and four
  operations crossed the line it drew.** `moe::Router::dispatch` **stays
  refused** — that was the question, and the answer is not the finding.

  D006 rests on one falsifiable claim it calls the consumer census, with an
  explicit inversion clause: *"if the count is zero, E is right… if it is
  nonzero, E is wrong today and the recommendation inverts."* It was recorded
  as zero on 2026-08-14 and not re-run until 2026-08-26. **It is four.** Five
  operations across `corpus/golden/22` and `23` carry a `moe::Tensor` on the
  wire; four are served and one is refused. The served four include
  `moe::Expert::process` — **one of the two operations option E excluded** —
  and `spikes/f5_peer_client.py` has an omniORB peer send it a real
  `Activation` while the harness asserts every declared operation was called,
  so the harness does not merely permit the breach, it gates that it stays.
  **A measurement with a stated inversion condition, taken once and never
  re-taken, is the floor-quoted-as-a-figure class with higher stakes: the
  figure did not drift in silence, it crossed the threshold its own author
  wrote down.**

  D006 §1's P3 — that no predicate distinguishes a handle from a payload —
  **does not have to be settled** to read it. `infer` ends at `Ok(x)`, and both
  `process` arms end at `x.write_to(out)`, echoing an unbounded
  `sequence<octet>` back without interpreting a byte: under the payload reading
  that is the data plane, under the handle reading the servant returns a handle
  it never dereferences and the operation does no work at all.

  **The finding is that `dispatch`'s stated reason was false, under a green
  harness.** Two sentences repaired in `expert_service.rs`, both about that one
  operation: `:737` said it answers `BAD_OPERATION` when it answers
  `NO_IMPLEMENT` — four lines from the code that refutes it, in a module whose
  own docs record this exact polarity failure being fixed in *themselves* on
  2026-08-18/19 — and `:449` said the decision keeps `Expert::process`
  unimplemented, which it is not.

  Codified so it cannot be re-derived by reading:
  `orbweaver_object::plane::TENSOR_BEARING` is the one home, five rows with
  direction, status and reason. `tests/one_plane_rule_for_a_tensor.rs` computes
  census **membership** by parsing both contracts with our own front end (a
  typedef/struct/sequence fixpoint, so a `Tensor` one struct deeper still
  joins) and checks the recorded **status** against what the servants answer
  over a socket, so the next operation to start carrying a `Tensor` joins on
  the next `cargo test` rather than on the next reading. Three negative
  controls, each run, each red.

  **Not settled here, and not this batch's to settle:** `PLAN-SERVICES` §1
  rule 2 and D006 option E point opposite ways at `Expert::process`, and four
  documents state both halves — §8.1.1 holds both in one paragraph.
  D006's STATUS is unchanged and remains the owner's. Also recorded and not
  changed: `Router::select` returns N IORs with host, port and object key, and
  is a **location**-transparency leak with live consumers; `dispatch` is not
  the operation that would close it.

  **Not measured:** the workspace test count before and after. A clean baseline
  was started, discarded when it was found to be compiling sources edited
  mid-run, restarted, and then stopped so a long harness run could hold the
  machine. Counted unmeasured, not a gap.

  *D006이 스스로 적어둔 반증 조건 — 세면 0이면 E가 옳고, 0이 아니면 권고가
  뒤집힌다 — 을 2026-08-14 이후 다시 재지 않았다. **넷이다.** 옵션 E가 제외한
  `Expert::process`가 그중 하나이며 omniORB 피어가 실제로 호출하고 하네스가 그
  유지를 게이트한다. **주어진 질문의 답은 "계속 거부"였고, 답은 발견이 아니었다**
  — 발견은 그 거부의 명시된 사유가 초록 하네스 아래에서 거짓이었다는 것이다.
  이제 인구조사의 집은 하나이고 테스트가 계약에서 계산한다. 규칙 차원의 충돌과
  D006의 상태는 소유자의 것으로 남는다. **워크스페이스 테스트 수는 미측정.**

- **Two references to one object cost one request each — and that is what
  omniORB costs too.** `cd9f88f` shared a permanent forward across every
  *clone* of a `Reference` and reported that two `Pool::reference` calls for
  one IOR still do not agree, proposing an identity map because "omniORB
  deduplicates by object key". Measured before deciding: three independently
  created references, seven calls — **3 requests at the address the object
  left and 7 at the object, both reply byte orders**. A second reference pays
  on its own first call and then re-points itself, so the cost is one request
  **per reference, once**, not one per call. omniORB 4.3.4 in the identical
  shape (two `string_to_object` calls on one IOR string, plus a third after
  the move, over TCP) pays **the same 3 of 7** under both forward statuses,
  with `_is_equivalent` answering true — **the premise the identity map rested
  on is refuted by the ORB it named**.
  `docs/decisions/D013-reference-identity-in-the-pool.md` (PROPOSED) therefore
  recommends **building nothing**, records the weak-reference map as the shape
  if its trigger fires, and names the trap that makes a naive map a wrong-string
  bug: `pool::Key` carries version and published codeset, so two IORs naming
  one `(endpoint, object key)` with different `TAG_CODE_SETS` would let the
  second reference inherit the first's profile — D012 §3's class through
  another door. The harness pin moved 5 → 6.

  **한 객체를 가리키는 레퍼런스 둘의 비용은 각 한 번이며, omniORB도 같은 값을
  문다.** `cd9f88f`은 "omniORB는 객체 키로 중복 제거한다"를 근거로 식별 지도를
  제안했다. 결정 전에 측정했다 — 독립 생성 레퍼런스 셋, 호출 일곱 번, **떠난
  주소에 3, 객체에 7, 두 바이트 순서 모두**. 두 번째 레퍼런스는 자기 첫 호출에서
  한 번 물고 스스로 재지정되므로 비용은 호출당이 아니라 **레퍼런스당 한 번**이다.
  omniORB 4.3.4도 같은 모양에서 **같은 3/7**을 물고 `_is_equivalent`는 참을
  답한다 — **지도가 딛고 선 전제를 그 지도가 이름한 ORB가 반증했다.** 따라서
  D013(제안)은 **짓지 않기**를 권고하고, 방아쇠가 당겨질 때의 모양으로 약한 참조
  지도를 기록하며, 순진한 지도를 "틀린 문자열" 버그로 만드는 `pool::Key` 코드셋
  함정을 적는다.

- **The peer a `CloseConnection` mid-reply needed is not an ORB, and no defect
  was found.** D010 §4 B5's second half said `InterruptedMidReassembly`'s shape
  *"needs a peer to shut down between two writes of one reply, and neither
  fixture exposes that window"*. **Both halves of that are true and the
  conclusion is wrong**: the peer this needs is a socket, and a socket needs
  nothing that is missing here. B5 was the only class-B row whose fixture was
  buildable — B1 wants an API key, B2 an identity provider, B4 docker and a
  second host, B6 TAO. Two peers, answering different questions: sixteen
  in-process peers whose bytes are built from §9.4 and **not from this crate's
  encoders**, for the reason `fragment_reception.rs` gives — an encoder and a
  decoder that share a bug agree with each other — and sixteen more from a
  separate process in another language, stdlib only, no ORB imported, which
  adds the one thing an in-process peer cannot: **the id the client says was
  cut is checked against the id the peer says it cut, by two processes
  separately.** The in-process arm varies every axis the existing measurement
  holds constant, and the first is the one this project has a rule about — *the
  reply's byte order, chosen independently of the request's*, because GIOP sets
  the order per message and a peer that echoes the request (which is what every
  scripted peer in the tree does) leaves one of the two orders unmeasured on
  any one machine, and the request id that decides which caller hears what is
  read out of *the reply's* header. **MEASURED: the claim holds in all 32
  cases, both byte orders.** The caller whose reply had begun hears
  `InterruptedMidReassembly` naming its own request id and is **not** told it
  may re-send; the other caller multiplexed on the same connection hears
  `ConnectionClosed` and **is**. **No defect was found, so nothing was fixed**
  — `mux.rs`'s module doc carried the stale sentence *"Still not observed from
  a peer…"* and now carries what was measured instead. Reported and **not
  diagnosed**: one case in ~450 failed with `Connection refused` against a peer
  that had demonstrably bound, listened and published its port, and it has not
  reproduced in 608 subsequent cases; `SO_REUSEADDR` was the obvious suspect
  and was **measured innocent** (0 refusals in 6000 cycles with it, 487 failed
  *binds* in 3000 without it, so removing it is strictly worse). What changed
  is that the runner can no longer report it as a refutation — exit 3 is *"nothing
  was measured"* as distinct from 1, *"the claim did not hold"*, because
  collapsing them would point a false diagnosis at the code under test on a run
  where nothing happened.

  **한 답신의 두 쓰기 사이에 끊는 피어는 ORB가 아니라 소켓이며, 결함은 발견되지
  않았다.** D010 §4 B5 후반의 전제 두 쪽은 참이고 결론이 틀렸다. 질문이 다른 피어 둘
  — 이 크레이트의 인코더가 아니라 §9.4에서 바이트를 만든 인프로세스 피어 열여섯과,
  다른 언어·다른 프로세스의 표준 라이브러리 소켓 피어 열여섯(**클라이언트가 끊겼다고
  말한 아이디와 피어가 끊었다고 말한 아이디를 두 프로세스가 따로 대조한다**).
  **32건 전부, 두 바이트 순서 모두에서 주장이 성립한다.** **결함이 없었으므로 아무것도
  고치지 않았다.** 재현되지 않은 이상 1건은 진단되지 않았다고 기록하며,
  `SO_REUSEADDR`는 측정으로 무죄가 밝혀졌다.

- **The SSL peer neither ORB can be is a socket — and it now runs, 21 of 21,
  exit 0.** `run_checks.sh` has printed `SKIPPED no SSL peer` for the life of
  the project and D010 §4 B3 files SSLIOP peer proof as blocked on *"an omniORB
  build with SSL, or JacORB's SSL transport configured"*. The premise is true —
  `from omniORB import sslTP` still raises `ImportError` here — **and the
  conclusion does not follow**, for the same reason it did not for B5. SSLIOP
  is not a protocol an ORB implements on top of IIOP: the Security Service's
  chapter defines unmodified GIOP over a TLS connection plus one component
  saying where the TLS listener is — no handshake of its own, no negotiation,
  no framing. So what it needs is **a peer that speaks IIOP over TLS, not an
  ORB that does**, and Python's `ssl` is in the standard library while the
  certificates have been in `spikes/tls/` since 2026-08-13.
  `spikes/ssliop_peer.py` builds every GIOP and IOR octet by hand from §7.6.9,
  §9.4 and the SSLIOP chapter; it reports what it observed and **judges
  nothing**, and the runner, which knows the trust configuration, judges. Part
  A reads octets our encoder did not write, over both IOR byte orders and both
  component byte orders **independently** — an encapsulation restarts alignment
  and carries its own order octet, so a little-endian component inside a
  big-endian IOR is a shape a deployment produces and our own encoder never
  does. Part B is a real cross-implementation handshake, rustls to OpenSSL,
  negotiating TLS_AES_256_GCM_SHA384. And five refusals, which are what a
  security-shaped claim owes: an unsigned certificate refused **and named**; a
  plaintext peer at the advertised SSL port refused, with the peer's own
  account as the evidence — it saw a TLS ClientHello arrive in cleartext, so
  the client attempted TLS and **did not downgrade**; an advertisement pointing
  at a dead port does not fall back to the live cleartext listener beside it;
  no advertisement is not a licence to dial cleartext; and an unreadable
  advertisement is not an absent one. **The measurement was itself unmeasured
  for two commits and said so**: the part B driver had nowhere cargo would
  build it, so `ssliop.sh` counted its fifteen cases UNMEASURED and exited
  **3** — nothing measured, as distinct from 1, the claim did not hold. Four
  lines of `Cargo.toml` and a move to
  `crates/orbweaver-giop/src/bin/spike_ssliop.rs` closed it; no `pub` was
  missing, the file was simply somewhere cargo does not look. Licence boundary
  re-measured because `Cargo.toml` changed: `cargo tree --workspace` runs and
  names no fixture.

  **어느 ORB도 될 수 없는 SSL 피어는 소켓이며, 이제 21건 중 21건, exit 0으로
  돈다.** 전제(여기 omniORBpy에 sslTP가 없다)는 참이고 **결론은 따라 나오지 않는다**.
  SSLIOP은 IIOP 위의 별도 프로토콜이 아니라 TLS 위의 평범한 GIOP와 리스너 위치를
  말하는 컴포넌트 하나이므로, 필요한 것은 **IIOP를 TLS로 말하는 피어이지 그렇게 하는
  ORB가 아니다.** 피어는 관찰만 하고 **판정하지 않으며**, 신뢰 설정을 아는 러너가
  판정한다. IOR 바이트 순서와 컴포넌트 바이트 순서를 **독립적으로** 돌린다. 거부 다섯
  건이 보안 성격의 주장이 치러야 할 값이다. **측정 자체가 두 커밋 동안 미측정이었고
  그렇게 말했다** — 드라이버를 카고가 보는 자리로 옮기기 전까지 15건이 UNMEASURED,
  exit **3**(측정 없음)이었지 1(반증)이 아니었다.

- **`string_to_object` against omniORB found three places we had guessed, and a
  test written from our side could have found none of them.** §8.2.2 promises
  that `string_to_object(object_to_string(obj))` names the same object *"even
  if the two operations are performed on different ORBs"* — which is why the
  oracle is a peer: a convention both halves of this crate share cannot be
  refuted by our own round trip. **(1) `corbaname:` with a name in it — the
  peer proved us wrong.** Part 2 §7.6.10.5 says such a URL denotes the object
  *bound under that name*, not the naming context holding it, and omniORB
  **dials**. Routing our existing code through unchanged would have returned
  **the naming context**, because `ObjectUrl::to_ior` ignores the `name` field
  entirely — the wrong object, silently, and the one answer this operation must
  never give. It is refused by name instead, citing §7.6.10.5 and pointing at
  the two-step a caller can see; dialling inside a conversion is real new
  behaviour with a timeout to choose. omniORB draws the line in the same place.
  **(2) A second `corbaloc:` address is a lost fallback.** We build one profile
  per address; omniORB builds one profile at IIOP 1.0 and folds the rest into
  `TAG_ALTERNATE_IIOP_ADDRESS` components. Both are legal. But
  `parse_iiop_profile` reads components only `if minor >= 1`, because §7.6.2
  gives `ProfileBody_1_0` no components field — so **we silently drop
  omniORB's alternate address, and failover never happens** for any reference
  omniORB produced this way. **Not fixed**: reading components after a 1.0
  profile body changes how every reference from every peer is parsed, which is
  a wire-behaviour batch with its own peer measurement. It is **pinned**
  instead, so the day it is fixed the assertion goes red and the divergence
  record has to be updated rather than quietly rotting. **(3) Two refusal
  classes were guessed and corrected by the run**: `IOR:zzz` is `MARSHAL`, not
  `BAD_PARAM`, and `corbaloc:rir:/TradingService` is `NO_RESOURCES`, not
  `BAD_PARAM` — the same reserved-versus-invented line drawn in step 1, now
  confirmed by a second implementation drawing it too. **The census is a
  weaker claim than proposed and it is the measured one**: 179 sites spell a
  conversion by hand and **nothing in Rust dispatches on the form today**, so
  `string_to_object` removes no existing duplication — it removes the *need*
  for the next one.

  **`string_to_object`을 omniORB에 대어 우리가 짐작하던 세 자리를 찾았고, 우리 쪽에서
  쓴 테스트로는 하나도 찾을 수 없었다.** §8.2.2의 약속이 **다른 ORB 사이에서도**
  성립한다고 말하기 때문에 오라클은 피어다. (1) 이름이 붙은 `corbaname:`은 그 이름에
  묶인 객체를 뜻하는데 기존 코드를 그대로 태웠으면 **네이밍 컨텍스트**를 조용히
  돌려줬을 것이다 — 이 연산이 절대 주면 안 되는 답. (2) 두 번째 `corbaloc:` 주소는
  잃어버린 폴백이다 — omniORB가 만든 참조에서 대체 주소를 조용히 버리고 있다.
  **수정하지 않고 고정했다.** (3) 거부 분류 둘은 짐작이었고 실행이 교정했다.
  **인구 조사는 제안보다 약한 주장이며 그것이 측정된 주장이다** — 179곳 어디도 형식에
  따라 분기하지 않으므로, 이것은 있던 중복을 없애는 것이 아니라 다음 중복의 *필요*를
  없앤다.

- **A null result with a reason: the frozen benchmark cannot see the two SIDL
  keys that now reach a reader.** `Subject::to_prompt` has exactly one caller,
  the S3i path, and `corpus/requirements/{inputs,inputs-v2}` is driven through
  three other prompts entirely — `Subject::parse` fails on IDL, so the stage is
  **never even constructed** on that path. That is checkable rather than
  arguable, so it was checked: a capture stub in place of the producer recorded
  the exact prompt and input the pipeline handed each item over the whole
  frozen set, on both arms — 46 S1 items, 20 S3 items, **92 prompt/input pairs
  per arm** — and `diff -r before after` found **no differences, exit 0**. Zero
  bytes of what the frozen benchmark shows a producer changed. Pass rates are
  therefore identical **by construction**, and no model was called to produce a
  number that could not have been attributed to anything. *An inert key that
  has been measured inert is a different thing from one nobody looked at.*
  **The instrument was controlled too**, since a diff that finds nothing is the
  shape a broken diff also has: one word appended to the S3 prompt made 20 of
  20 differ, `diff -rq` exit 1. **What a real measurement would take, stated so
  it is not mistaken for done: there is no frozen S3i benchmark at all**, and
  the nearest thing is unannotated by design so its two arms would also be
  identical. It needs a frozen set of ingested interfaces where M of N
  operations carry authored keys from their real contracts, then two runs
  against a live producer with the rendering on and off, compared on the gate
  pass rate and `Proposal::unknown_rate()`. Cost: 2N model calls plus repair
  rounds — and when it is run, one model family generating and evaluating makes
  that number **indicative**, said in the same breath as the number.

  **이유 있는 영(null) 결과: 얼어붙은 벤치마크는 독자에게 닿게 된 SIDL 키 둘을 볼 수
  없다.** `Subject::to_prompt`의 호출자는 S3i 경로 하나뿐이고 얼어붙은 요구사항 집합은
  다른 세 프롬프트로만 돈다 — 그 경로에서 이 단계는 **생성되지조차 않는다.** 논증이
  아니라 확인 가능한 사안이므로 확인했다: 양쪽 팔에서 **92쌍**을 포착해 `diff -r` →
  **차이 없음, exit 0.** 통과율은 **구성상** 동일하며, 무엇에도 귀속시킬 수 없는 수치를
  만들려고 모델을 부르지 않았다. 아무것도 못 찾는 diff는 고장 난 diff와 모양이 같으므로
  **계측기도 대조했다**(한 단어를 넣자 20/20이 달라졌다). **진짜 측정에 필요한 것을,
  끝난 일로 오해되지 않게 적는다 — 얼어붙은 S3i 벤치마크는 아예 없다.**

- **Three instructions were re-measured and found wrong, in the batches they
  were handed to.** *"Six sites in `nat.rs` do `.unwrap()`"* on
  `ObjectUrl::to_ior` — they do not: those are a different function of the same
  name, with no arguments and a `Result` return, and all six are in that
  module's own test code. `ObjectUrl::to_ior` has **exactly two non-test call
  sites, both already handling the `None`**, so nothing that passes a `rir:`
  URL exists and nothing could begin to panic; the count is now written on the
  function with the date it was taken. The proposed oracle for `corbaloc:rir:`
  had **the direction backwards** — handing the URL to omniORB's client
  measures *omniORB's* table, since §8.5.2's whole point is that `rir` is
  local, and it was confirmed live that omniORB resolves it out of its own
  configuration and never asks the far end anything; the direction that
  measures ours is the reverse. And a `csiv2.rs` literal named as a number to
  sweep is **the specification's** — an `IOP::ServiceId` — so a configuration
  key for it would be a way to address a service context nobody reads; the
  verdict is recorded in the module docs so the next reader does not spend the
  same half hour. Separately, SIDL's vocabulary is **eight keys, not nine**:
  the key listed as the ninth is the test suite's canonical example of a key
  **outside** the vocabulary, chosen precisely because nothing reads it, and
  the type four lines above the paragraph declares the count.

  **지시 셋을 다시 재어 틀렸음을 확인했다 — 그 지시를 받은 배치 안에서.**
  "`nat.rs`의 여섯 곳"은 같은 이름의 다른 함수였고 전부 그 모듈의 테스트 코드였다.
  `corbaloc:rir:`의 오라클 방향은 **거꾸로**였다 — 그렇게 하면 omniORB의 표를 잰다.
  그리고 훑으라고 지목된 상수 하나는 **규격의 것**이었다. 또한 SIDL 어휘는 아홉이
  아니라 **여덟**이며, 아홉 번째로 적힌 키는 어휘 **바깥**의 정전(正典) 예시다.

- **Five negative controls came back green, and each one is the finding.** A
  control that passes is not a formality; it is a measurement of the check.
  (1) A test pinning a keyword list **iterated the very list it was pinning**,
  so removing a name removed the case — rewritten to spell the names out; the
  same class as a gate that measures nothing. (2) A `FIRST` ordering mutated to
  `Ordering::Greater` passed, **not because the test measures nothing** but
  because an always-`Greater` comparator is not a total order at all and
  `sort_by` over three elements leaves them alone — *a mutation that is not a
  valid ordering is not a negative control for an ordering*; two valid
  replacements were both red. (3) A console value retyped **at the same value**
  came back green: the test pins agreement, not provenance, and what makes the
  drift impossible rather than detectable is reading the constant. (4) An
  equivalence test stayed green under a control that broke absence, because
  both runs gained the same line — *equivalence and absence are two pins and
  the batch needs both*. (5) A relationship batch's whole first round of
  controls **measured nothing and said so only because its output was read**:
  the controls ran with unqualified `--exact` names, so all 91 tests filtered
  out and every mutation "passed" with exit 0; the harness now counts selected
  tests and marks any control that did not run exactly one as UNMEASURED. A
  sixth was a **hang** — a `Delivery::drop` stopping only the default channel
  blocked joining another channel's threads and was killed at 120 s, recorded
  as the honest symptom rather than dressed up as an assertion.

  **부정 대조군 다섯이 초록으로 돌아왔고, 각각이 곧 발견이다.** 통과한 대조군은
  요식이 아니라 **검사 자체에 대한 측정**이다. (1) 목록을 고정하는 테스트가 **바로 그
  목록을 순회**했다. (2) 전순서가 아닌 변이는 순서에 대한 부정 대조군이 아니다.
  (3) **같은 값으로** 다시 타이핑한 것은 초록이었다 — 테스트는 출처가 아니라 일치를
  고정한다. (4) 동치와 부재는 서로 다른 고정이며 둘 다 필요하다. (5) 한 라운드 전체가
  **아무것도 재지 않았고**, 출력을 읽었기 때문에만 드러났다. 여섯 번째는 **행(hang)**
  이었고 정직한 증상 그대로 기록했다.

- **A disconnect that had returned was not a disconnect that had stopped.**
  `ProxyPullConsumer::disconnect_pull_consumer` set `connected = false` under
  the state lock, but the source thread snapshots its round, **releases the
  lock** — it must, a network call cannot be made holding it — and only then
  invokes. A round that had snapshotted first still asked. The window contained
  a whole connect timeout, so "one more" was a bound in principle and not one
  anybody had stated.

  Two failures were this one defect from opposite sides: a CI test asserting
  *a disconnected proxy is not asked* saw `left: 2 right: 1`, and
  `spikes/event_pull_supplier.py` printed `PASS` and then **aborted with
  SIGABRT** because a late `try_pull` landed while CPython was tearing down and
  a servant raising a non-CORBA exception makes omniORB call `FATAL: exception
  not rethrown`.

  The source loop now has a **commit point** — `source_still_wanted`, taken
  under the state lock with **no I/O between it and the request going out**;
  `disconnect_pull_consumer` and `ChannelHandle::stop` take that same lock. The
  guarantee is now stated rather than hoped for: once either has returned, the
  only `try_pull` that can still reach that supplier is one whose commit point
  had already been passed, and one source thread runs one round at a time, so
  that is **at most one further call, landing within the outbound timeout** —
  never a stream and never a second one. Later rounds are cancelled where they
  stand and counted in the new `ChannelStats::pull_rounds_cancelled`. One
  predicate serves both callers, because *"the channel no longer wants this
  round"* is one fact; it compares the supplier IOR and not only the flag, so a
  disconnect-then-reconnect-elsewhere is never asked a question meant for the
  old peer. `ChannelHandle::wait_source_idle` makes the escape clause
  observable in process; a peer over the wire has the time bound and nothing
  else.

  Rejected, with the reason recorded at the arm that owes it: making the
  disconnect *wait* for the in-flight round. The thread it would wait for is
  blocked in an outbound call to the very supplier whose process is free to be
  the one calling the disconnect, so a servant would be held for an outbound
  timeout by the peer it is answering.

  **The control is deterministic, which the bug was not** — it never reproduced
  on macOS in 20 serial runs, 5 concurrent whole-suite runs, or with the poll
  forced to 200 µs. A test seam holds a round after it is taken and before the
  commit point, with nothing on the wire: with the commit point deleted the
  assertion fails **20 of 20 runs**, and with it **0 of 20**. The test helper
  was controlled too — pointed at a supplier still being polled it reports
  `left: 20 right: 5`, so it is not vacuous — and the fixture's teardown,
  removed, prints *"the channel asked 26 more times after
  `disconnect_pull_consumer` returned; the guarantee is at most one"*. **The
  SIGABRT itself is still "not reproduced in 5 runs on macOS"**: its mechanism
  is diagnosed and its cause closed at the source, and that is a different
  claim from having watched it stop.

  *반환한 disconnect가 멈춘 disconnect는 아니었다. 소스 스레드는 라운드를
  스냅샷하고 **락을 놓은 뒤** 호출한다 — 네트워크 호출을 락 안에서 할 수는 없다.
  CI 테스트 실패와 픽스처의 SIGABRT는 반대편에서 본 같은 결함이었다. 이제 소스
  루프에 **커밋 포인트**가 있다: 상태 락 아래서 잡히고 요청이 나가기까지 **I/O가
  없다**. 보장은 이제 서술된다 — 커밋 포인트를 이미 지난 호출 **하나만**, 아웃바운드
  타임아웃 안에. 술어 하나가 두 호출자를 섬기고 플래그가 아니라 IOR을 비교한다.
  대조군은 결정적이다(버그는 아니었다): 커밋 포인트를 지우면 **20/20 실패**, 두면
  **0/20**. SIGABRT 자체는 여전히 "macOS 5회 재현 안 됨"이며, 기전이 진단되고
  원인이 닫힌 것과 멈추는 것을 지켜본 것은 다른 주장이다.*

- **Both emitters kept their own list of what the wire cannot carry, and both
  disagreed with the type mapper.** `orbweaver-gen`'s `rust_type` ends in
  `other => Err(..)` — it refuses what it has no arm for — while the walker
  `representable` ended in `_ => Ok(())`, which cleared it. Every construct in
  the gap between the two lists was **skipped at its declaration and emitted at
  every container that named it**: `corpus/golden/34-corba-principal.idl`
  produced `pub sealed: crate::f_34_corba_principal::gp34::Envelope` for an
  `Envelope` the file never declares, and did not compile. `python.rs` had the
  identical split and **was not red at all**, writing
  `("ref", "IDL:gp34/Envelope:1.0")` for a class its package never defines —
  found by the caller, at the first call. Both walkers now ask the mapper at
  every node instead of relisting four families, so the gap is unrepresentable
  rather than detectable.

- **Eight corpus files had never met both front ends.** `corpus/golden/34` and
  `corpus/negative/n23`–`n30` landed with the batches that motivated them, as
  the rule requires, but the differential runs only inside the harness and the
  batches that added them were told not to run it — so the comparison waited
  days for a coordinator. Seven unexplained divergences and one non-compiling
  golden file were the result. Each divergence now has a measured reason in
  `corpus/divergences.tsv`; **JacORB 3.9 predeclares no `CORBA` scope at all**
  (established by a `typedef ::CORBA::Principal PA;` at true global scope,
  where there is no enclosing scope to prepend and the message is still
  `Undefined name: CORBA.Principal`), which also **refutes the mechanism
  `12-any-typecode.idl`'s existing row had asserted** — that row is corrected
  and hands the measurement to the new one rather than restating it. The six
  negative files share one cause and are **six rows, not one**, because their
  outcomes differ: two are caught by `javac`, one throws at class
  initialisation, and **three compile and run wrong** — `n26`'s two union
  defaults both take discriminator 0, so a value set through `fallback(7)`
  goes on the wire as 0. The gate no longer depends on remembering: the
  differential's verdict is checked-in data (`corpus/differential-results.tsv`)
  and an oracle-free test in `cargo test --workspace` compares the corpus
  against it.

  *두 방출기가 각자 "와이어가 못 나르는 것" 목록을 들고 있었고 둘 다 타입 매퍼와
  어긋났다. 매퍼는 거부하고 순회는 통과시켜, 그 틈의 구성물은 선언에서 스킵되고
  그것을 이름하는 모든 컨테이너에서 방출됐다. Rust 절반은 컴파일에 실패했고
  **파이썬 절반은 아무것에도 안 잡혔다** — 패키지가 정의하지 않는 클래스를
  참조했고, 호출자가 첫 호출에서 발견한다. 그리고 코퍼스 파일 여덟이 두 프런트엔드를
  만난 적이 없었다: 규칙대로 배치와 함께 착지했으나 differential은 하네스 안에서만
  돌고 그 배치들은 하네스를 돌리지 말라고 들었다. 이제 판정이 체크인 데이터가 되고
  오라클 없는 테스트가 대조한다. 부정 코퍼스 여섯은 원인이 하나지만 **행이 여섯**
  이다 — 결과가 다르기 때문이다: javac가 잡는 것 둘, 클래스 초기화에서 던지는 것
  하나, **컴파일되고 틀리게 도는 것 셋**.*

### Fixed / 수정

- **The harness's five failures were two causes, and both were the
  coordinator's.** A clean two-hour run on a frozen tree (`d4aaf00`, `dirty=0`
  at the end, so the verdict is uncontaminated) came back exit 5: **294 ok,
  5 FAIL, 6 SKIPPED**. All five cluster into two causes and **neither is a
  behaviour regression.**

  **Cause A — a fact changed and a pin in another home went stale (3).**
  `::CORBA::Principal` became a fifth wire-refusal family and the MCP surface
  gained D024 §5's four IDL tools; three pins outside those batches' crates
  went stale — the `--wire v1` file list, the deferred-wire count over golden,
  and `mcp_session.py`'s `tools/list`. **This was caused by the instruction,
  not by the batches:** each was told not to touch `spikes/run_checks.sh`
  because another batch held it, which left no way to update the pin its own
  change invalidated. **A footprint that bounds files does not bound facts.**

  The count pin failed in the more interesting way — it parsed the number by
  retyping a prefix of a sentence `contract-check` owns, `(§4.4 and natives)`,
  which had become `(§4.4's three, natives, and what CORBA withdrew)`. The
  match found nothing and the group reported `'absent'`: **the classifier
  defect `CLAUDE.md` names, in the harness itself.** The extraction now matches
  only the two numbers and the words carrying them, and the `ok` line no longer
  restates that parenthetical at all.

  **Cause B — `Server::bind` and `Poa::new` are `pub(crate)`, and two Rust
  files outside the workspace still called them (2).** `spikes/e2e/servant.rs`
  and `spikes/estate/servant.rs` are compiled standalone, so `cargo check
  --workspace --all-targets` **cannot see them by definition** — and that check
  had been reported as evidence the one-way rule held workspace-wide. It was
  the second time a sweep for this class was wrong; the first excluded
  `crates/orbweaver-giop/src`, which is where `src/bin` lives. There are
  exactly four `.rs` files outside `crates/`, and the sweep is now over all
  four.

  Each repaired group was lifted verbatim and re-run standalone on a quiet
  machine: S4 wire-v1 ok, contract-check ok (35/21), MCP stdio ok, estate PASS,
  end-to-end PASS. The MCP pin gained a **negative clause** it did not have —
  `register_contract`, `register` and `compile_idl` must never appear, because
  D024 §5 refuses registration by name and a silent addition is exactly what a
  pinned list exists to catch.

  *동결된 트리에서 2시간 실행: 294 ok, 5 FAIL, 6 SKIPPED. 다섯은 두 원인이고
  **어느 것도 동작 퇴행이 아니다.** 원인 A는 사실이 바뀌었는데 다른 집의 고정이
  낡은 것 — 각 배치에 `run_checks.sh`를 건드리지 말라고 지시한 결과이며,
  **파일을 묶는 발자국은 사실을 묶지 못한다.** 그중 하나는 `contract-check`가
  소유한 문장의 접두사를 다시 타이핑해 분류하던 것으로, `CLAUDE.md`가 이름 붙인
  분류자 결함이 하네스 자신에게 있었다. 원인 B는 워크스페이스 밖 두 파일이라
  `cargo check --workspace`가 **정의상** 볼 수 없었다.*

- **The differential and the gate over it named the same four directories, so
  neither could report the one they both missed.** `corpus/services/` — the
  contracts that exist to be *served*, and therefore the ones a foreign ORB is
  most likely to compile — was outside `spikes/differential.sh`'s enumeration
  and outside `ENUMERATED` in
  `every_corpus_file_met_both_front_ends.rs` from the day it was created. The
  two lists agreed with each other and both were short; a gate that mirrors the
  list of the thing it gates is only as wide as what somebody put in both.

  The measured cost: `corpus/services/ir-subset.idl` is **rejected by JacORB
  3.9** — `Undefined name: CORBA.ParameterDescription.TypeCode` at line 159,
  the third instance of the cause already measured under
  `34-corba-principal.idl`, JacORB predeclaring no `CORBA` scope — and the
  divergence **could not be written down**, because the staleness loop fails
  any row naming a file the script never checks. The directory is now
  enumerated in both places (98 files through both front ends, was 95), the row
  is recorded citing the existing measurement rather than restating it, and
  nothing was loosened to let it in: the fix is that the files are checked.
  The header of `corpus/divergences.tsv` stopped naming the directories at all,
  since a third copy of that list is the one that drifts — and it had.

  Demonstrated red before it was recorded:
  `differential.sh --require omniidl,jacorb_idl` exit 1, *"1 corpus file(s)
  diverge with no recorded reason: ir-subset.idl — omniidl=accept
  jacorb_idl=reject"*; and with `ir-subset.idl`'s row cut from the record, the
  membership gate panicked with *"1 corpus file(s) have never been through both
  front ends"*.

  *differential과 그 위의 게이트가 같은 네 디렉터리를 적고 있었으므로, 둘 다
  놓친 하나를 어느 쪽도 보고할 수 없었다. 서빙되기 위해 존재하는 계약들이 바로
  그 디렉터리였다. 느슨하게 만든 것은 없다 — 파일들이 실제로 검사되는 것이
  수정이다.*

- **Two shipped layers told a peer to wait for a release that cannot come, for
  a type CORBA removed in 2002.** `anyjson::from_json` answered a peer-fed
  document naming a `Principal` with `"principal cannot cross yet"`, and the
  CDR read direction answered `"cannot decode principal yet"`. Those are the
  *same two sentences in the same two layers* that were repaired for `native`
  on 2026-08-21; they survived that sweep because the sweep was scoped to a
  keyword, and a batch scoped to a keyword fixes a keyword. `::CORBA::Principal`
  is now a **fifth refusal family** with one published head
  (`orbweaver_dynamic::withdrawn_wire_head`), and its wording says *withdrawn*
  rather than *deferred* or *never marshallable* because those are three
  different instructions to a contract author: §4.4's three wait on this
  project, a `native` never had a wire form, and a `Principal` had one that
  GIOP 1.1 dropped and CORBA 3.0 removed. Five layers in `orbweaver-dynamic`,
  both emitters (`withdrawn_principal`), the generated Python runtime
  (`_WITHDRAWN`, held equal to the Rust sentence across the crate boundary by
  `python_target`) and `orbweaver-test`'s two property limits all read that
  head; the type mappers became **exhaustive over `TypeCode`** in the process,
  so both `no static mapping for …` catch-alls are gone and a thirty-fifth
  variant fails to compile rather than acquiring a sentence that names no
  boundary. **S4 warns for the first time**: `wire/deferred-type` names the
  five `corpus/golden/34` declarations with a position and a fix that says
  where caller identity actually went (a CSIv2 `IdentityToken` in a service
  context) — the only fix in that rule's set that can name a replacement
  rather than a redesign. `contract-check` over that file went from
  `0 declaration(s) the wire cannot carry … 0 unmeasured` to `5 … 3
  unmeasured`. Three gates had been green over it, and the way
  `deferred_wire_agreement` was green is the lesson: it compares the rule's set
  with the emitters' set **through one filter that reads the published heads**,
  and a family with no published head is invisible to both sides at once — two
  empty sets are equal. The fix for that class is not a better filter but an
  assertion that classifies by *fixture* instead of by sentence
  (`one_home_for_a_wire_refusal::every_layer_that_meets_one_reads_a_head`).

  **CORBA가 2002년에 제거한 타입을 두고, 배포된 두 계층이 피어에게 오지 않을
  릴리스를 기다리라고 말하고 있었다.** `from_json`은 `"principal cannot cross
  yet"`, CDR 읽기는 `"cannot decode principal yet"`이라고 답했다. 2026-08-21에
  `native`에 대해 고친 *같은 두 계층의 같은 두 문장*이며, 그 정리가 키워드에
  맞춰져 있었기 때문에 살아남았다 — 키워드에 맞춘 배치는 키워드를 고칠 뿐이다.
  이제 `::CORBA::Principal`은 공표된 머리 하나를 가진 **다섯 번째 거부 계열**이고,
  그 문장은 *미뤄짐*이나 *애초에 없음*이 아니라 **철회**를 말한다 — 셋은 계약
  작성자에게 서로 다른 지시이기 때문이다. `orbweaver-dynamic`의 다섯 계층, 두
  에미터, 생성된 파이썬 런타임(`_WITHDRAWN`), 속성 검사의 두 한계 문장이 모두 그
  머리를 읽는다. 그 과정에서 두 타입 매퍼가 `TypeCode`에 대해 **전수적**이 되어
  catch-all이 사라졌다. **S4가 처음으로 경고한다** — 위치와, 호출자 신원이 어디로
  갔는지(CSIv2 서비스 컨텍스트의 `IdentityToken`)를 말하는 수정 힌트와 함께.
  세 게이트가 초록이었고, `deferred_wire_agreement`가 초록이던 방식이 교훈이다:
  **공표된 머리를 읽는 하나의 필터**로 양쪽 집합을 계산하므로, 머리가 없는 계열은
  양쪽에서 동시에 보이지 않는다 — 빈 집합 둘은 서로 같다. 이 부류의 해법은 더
  촘촘한 필터가 아니라 문장이 아닌 *픽스처*로 분류하는 단언이다.

- **Both emitters kept a second list of what the wire cannot carry, and the
  two lists disagreed.** `orbweaver-gen`'s type mapper ends in a catch-all that
  *refuses* anything it has no arm for; the cascade that decides whether a skip
  reaches the containers referring to the skipped type ended in a catch-all
  that *cleared* it. `TypeCode::Principal` landed in the gap: `struct Envelope
  { ::CORBA::Principal sender; }` was skipped with its reason while `struct
  Manifest { Envelope sealed; }` was emitted naming it, so the generated crate
  did not compile (`cannot find type Envelope`) — and the Python emitter had
  the identical split, writing a descriptor `("ref", "IDL:gp34/Envelope:1.0")`
  for a class its package never declares, which Python discovers at the first
  call and never at import. `representable` and `crossable` now ask `rust_type`
  and `descriptor` at every node instead of relisting four families each, so
  there is **one list and one catch-all per target** and a family the mapper
  cannot map cascades from the day it exists. The property behind it is pinned
  directly — `no_emitted_item_names_an_item_that_was_skipped`, over golden and
  services, both emitters, each half asking the emitter's own namer rather than
  a retyped path rule. The pin that existed could not have caught this: the
  corpus-wide skip test exempts a file that is allowed to skip from having its
  skip *set* checked at all.

  **두 에미터가 각각 "와이어가 실을 수 없는 것" 목록을 하나씩 더 들고 있었고, 두
  목록이 서로 달랐다.** 타입 매퍼의 마지막 갈래는 모르는 것을 *거부*하고,
  스킵을 컨테이너까지 전파하는 캐스케이드의 마지막 갈래는 그것을 *통과*시켰다.
  그 틈에 `TypeCode::Principal`이 떨어져, 스킵된 `Envelope`을 참조하는
  `Manifest`가 생성되어 크레이트가 컴파일되지 않았고 — 파이썬 쪽은 선언되지 않은
  클래스를 가리키는 서술자를 써서, 임포트가 아니라 첫 호출에서야 드러났다. 이제
  캐스케이드는 매 노드에서 매퍼에게 묻는다: 목록도 하나, 마지막 갈래도 하나.

- **A corpus addition met the second front end only when the coordinator ran
  the full harness.** `corpus/golden/34-corba-principal.idl` (`0b8a387`) and
  `corpus/negative/n23`–`n30` (`14228da`) landed without `differential.sh`;
  seven of the eight diverge between omniidl and JacORB 3.9, and nobody found
  out for days. The differential was not broken — it was never run, because
  agents are told not to run `run_checks.sh` and nothing named the standalone
  gate. Naming the command in a document is what already failed, so the
  verdict stopped being an event and became data: `differential.sh --record`
  writes `corpus/differential-results.tsv` (and **refuses with one oracle**,
  because a record made from omniidl alone would say *measured* about a file
  JacORB never saw), and `every_corpus_file_met_both_front_ends` — no oracle
  needed, inside the `cargo test --workspace` every batch already runs — goes
  red for a corpus file with no row. Membership only, said in the record's
  header, the test's docs and its failure message; the verdicts being today's
  is a claim only the differential can make.

  **코퍼스 추가가 두 번째 프런트엔드를 만나는 시점이 조정자의 전체 하네스 실행뿐
  이었다.** 여덟 파일 중 일곱이 두 오라클 사이에서 갈렸는데 며칠 동안 아무도
  몰랐다. differential이 고장난 게 아니라 실행되지 않았고, 실행할 독립 게이트를
  아무도 이름 대지 않았다. 문서에 명령을 적는 방식은 이미 실패했으므로 판정을
  사건이 아니라 데이터로 바꿨다 — `--record`는 오라클이 둘 다 있어야만 기록하고,
  오라클 없이 읽을 수 있는 게이트가 `cargo test --workspace` 안에서 빨개진다.
  검사하는 것은 **소속뿐**이며, 판정이 오늘의 것인지는 주장하지 않는다.

- **Four gates were reporting green over what they had never read.** One
  class, found four ways in one day, each by running a gate rather than
  reading it.
  - **The bilingual decision gate had never compared eleven of thirteen
    Korean halves** — `승인됨` is not a key (`승인` is) and D001's marker leads
    with a date, so `bilingual_halves()` captured a token it could not map and
    dropped it silently. **D003 was among the eleven**: the file whose split
    halves are the reason the check exists. It printed `13 decisions, 0
    drifted status claim(s)` over them. Five more parsers in `spikes/` dropped
    input the same way — `coverage_tables.py` compared a document it had
    regenerated from the same short read (a whole service could vanish and the
    line still said *says what the wire says*), `records_keep_up.py` discarded
    git's exit code so a failed `git log` printed `0 commit(s) behind`, and
    `service_sweep.py` dropped declaration lines it could not match, so an
    operation was neither probed nor counted as unmeasured. Repaired in
    CLAUDE.md's order: count what you could not classify, make it fatal only
    where the script is already a gate, widen a parser only against real input.
  - **The harness's own first three gates could not go red** — a pipeline into
    `grep -q` under `set -o pipefail`. `cargo test --workspace | grep -q
    "^error"` printed `ok` over a red workspace; the **licence boundary this
    project calls non-negotiable** could not report a forbidden dependency,
    because finding one is exactly when `grep -q` SIGPIPEs the producer.
  - **The same defect in the form this file had called sanctioned**, 76 times.
    Capturing to a variable saves the data and not the branch: the
    concurrent-dispatch group's own `printf '%s' "$cd_out" | grep -q "^test
    result: FAILED"` inverted **non-deterministically**, by where in three
    crates' output the failure fell — a group whose whole argument is *"five
    runs, because one green run is not evidence"* could not see a failing run
    when the failure came early. All 76 are herestrings now.
  - **`cargo fmt --check` lived only in CI**, so "landed through the harness"
    never included formatting — measured when a push whose local harness said
    *all measured checks green* went red on CI, in a wave where every agent
    had been required to run it and the coordinator who wrote the requirement
    had not.

  **게이트 넷이 한 번도 읽지 않은 것 위에서 초록을 보고하고 있었다.** 한
  계급, 하루에 네 가지 방식으로 발견, 전부 게이트를 읽지 않고 실행해서.
  이중언어 결정 게이트는 열셋 중 **열한 개의 한국어 반쪽을 비교한 적이
  없었고**, 그중에 **D003** — 이 검사가 존재하는 이유인 파일 — 이 있었다.
  하네스 자신의 첫 세 게이트는 `pipefail` 때문에 **빨개질 수 없었고**, 그중
  하나가 타협 불가라고 적힌 라이선스 경계다. 같은 결함이 이 파일이 권장
  형태라 부르던 모양으로 **76곳** 더 있었다 — 변수로 캡처하는 것은 데이터를
  구할 뿐 분기를 구하지 않는다. 그리고 포맷 검사는 **CI에만** 있어서 하네스를
  통과했다는 문장이 포맷을 포함한 적이 없었다.

- **The console's "what exists" page was drawing 57 of 208 entries.** Measured
  over every golden file by kind: 151 declarations (72.6%) reached no reader
  surface — 39 constants, 47 structs, 35 typedefs, 11 exceptions, 8 unions, 7
  enums, 3 valuetypes, 1 native — because `Entry::Interface` was the only
  variant the crate reached. Two golden files declare no interface, so the
  page said **"the catalog is empty"** in those words over 22 constants and a
  union; on the thirteen-contract estate it was 12 interfaces drawn and 38
  declarations hidden, eight of them the exceptions a caller has to handle.
  The batch was handed the word *constants*; the rule is *the catalogue shows
  what a contract declares*, so the test is a **partition** — every id the
  registry holds is an interface row or a declaration row, over every golden
  file — rather than a check that constants are present, which would have gone
  green with seven kinds still missing.

  **콘솔의 "무엇이 있는가" 페이지가 208개 중 57개를 그리고 있었다.** 선언
  151개(72.6%)가 어떤 독자 표면에도 닿지 않았다. 인터페이스를 선언하지 않는
  골든 파일 둘에서는 상수 22개와 union 하나 위에서 **"카탈로그가 비어 있다"**고
  말했다. 배치는 "상수"라는 단어를 받았지만 규칙은 *카탈로그는 계약이 선언한
  것을 보여준다*이므로, 테스트는 상수 확인이 아니라 **분할**이다.

- **`repair_prompt` sent every whole-file finding to line zero.** The line-0
  sentinel had three private readers and two of them rendered the raw fields,
  so a finding about a file as a whole — every `evolution/*`, `registry`,
  `released-unreadable` — was rendered `line 0, column 0` in the one string an
  agent is told to act on. `Finding::position()` is now the sentinel's single
  reader; a whole-file finding renders its source identifier and no position.

  **`repair_prompt`이 파일 전체에 대한 진단을 전부 0행으로 보냈다.** 센티널을
  읽는 사적 리더가 셋이었고 둘이 원시 필드를 렌더했다. 이제 리더는 하나다.

- **A refusal's subject carries the repository id, and the subject's spelling
  has one home per language.** The commissioning defect (2026-08-24, recorded
  as its own batch): a simple name is ambiguous — two modules declaring
  `Describable` produce one string, so a reader of two refusals could not
  tell whether they named one type or two. `orbweaver-dynamic` now publishes
  the spelling — `valuetype_subject`, `abstract_interface_subject`,
  `native_subject` (`{kind} {name} ({id})`, the id alone when a peer-built
  TypeCode has no name) and `fixed_subject` (digits and scale *are* its
  identity) — and `deferred_wire_name`/`unmarshallable_wire_name` build from
  them. Scoped to the rule, not the instance: **nine Rust sites in two other
  crates and seven Python sites were formatting their own subject**
  (`format!("valuetype {name}")`, `"native " + name`), the same defect the
  sentence *heads* had at `pub(crate)`, one layer down; all sixteen now ask
  the owner (Python's one home is `_subject`, held equal by `python_target`'s
  cross-language comparison), and the three sites rebuilding
  `fixed<{digits},{scale}>` were found by re-measuring the neighbours of the
  shape handed in. Test pins that had retyped a subject were repaired to
  *compute* it — the dynamic unit pin went red the moment the id joined the
  subject, which is that pin working — and
  `one_home_for_a_wire_refusal.rs`, built two batches ago to compute expected
  text by calling the owners, stayed green through the rewording untouched:
  the first live proof of the class it was built for.

  **거부 문장의 주어가 repository id를 담고, 주어의 철자는 언어당 한 집을
  갖는다.** 발주 결함(2026-08-24 기록): 단순 이름은 모호하다 — 두 모듈의
  `Describable`이 한 문자열이 되어, 두 거부를 읽는 독자가 한 타입인지 두
  타입인지 알 수 없었다. `orbweaver-dynamic`이 철자를 공개하고
  (`valuetype_subject` 등, `{kind} {name} ({id})`, 이름 없는 peer TypeCode는
  id만; `fixed_subject`는 digits/scale이 곧 정체성), 이름 함수들이 그것으로
  짓는다. 규칙 단위 범위: **다른 두 크레이트의 Rust 9곳과 Python 7곳**이
  주어를 직접 조립하고 있었다 — 문장 머리가 `pub(crate)`이던 것과 같은 결함,
  한 층 아래. 열여섯 곳 전부 이제 소유자에게 묻고(Python의 한 집은
  `_subject`, 교차 언어 등식으로 고정), `fixed<{digits},{scale}>`를 재조립하던
  세 곳은 받은 모양의 이웃 재측정으로 발견했다. 주어를 옮겨 적던 테스트 핀은
  계산하도록 수리 — dynamic 단위 핀은 id가 주어에 합류하는 순간 빨개졌고,
  그것이 핀이 일하는 모습이다 — 두 배치 전에 지어진
  `one_home_for_a_wire_refusal.rs`는 문구 변경을 손대지 않고 초록으로
  통과했다: 그 게이트가 지어진 목적 계급의 첫 실전 증명.

- **The generated Python runtime reads every description the Rust side
  writes, and refuses only the value — closing the D008 asymmetry.** The
  rule is D008's: a TypeCode is a value the wire carries; §4.4 defers the
  *instance*. The Rust side has answered accordingly since 2026-08-20 — on
  `{"_t": {"kind":"fixed",…}, "_v": …}` it reads `_t` and stops at `_v` with a
  sentence naming the type. The Python runtime refused the same document **at
  `_desc_of`**, the `_t` half, telling a peer their description was not
  understood — the opposite of the truth and of what the other end of the same
  bridge says. One cause for all affected kinds: the form reader answered a
  value question it was never asked. Scoped to the rule, not the keyword —
  every kind `tc_to_json` writes was enumerated against every kind `_desc_of`
  reads, which is how `principal` joined the named two: **three kinds
  (`fixed`, `native`, `principal`), both directions, plus `_form_of`'s
  write-back**. Now the form reads to a descriptor, `_form_of` returns the
  very document that arrived, a TypeCode *value* naming the family crosses
  whole, and the value legs refuse — directly and through an `any` — with
  `_DEFERRED`/`_UNMARSHALLABLE`, already held equal to the Rust sentences by
  `python_target`'s cross-crate comparison; the refusal's path names `_v`,
  never `_t` (`principal`, withdrawn from CORBA, has no shared sentence and
  falls to the generic value refusal on both sides). The rewritten test walks
  the whole division; its negative control — the old `_desc_of` arm put back —
  goes red at `python_target.rs:913`.

  **생성된 Python 런타임이 Rust 쪽이 쓰는 모든 기술을 읽고, 값만 거부한다 —
  D008 비대칭 해소.** 규칙은 D008의 것: TypeCode는 와이어가 나르는 값이고
  §4.4가 미루는 것은 *인스턴스*다. Rust 쪽은 2026-08-20부터 그렇게 답해왔다 —
  같은 문서에서 `_t`를 읽고 타입을 이름한 문장으로 `_v`에서 멈춘다. Python
  런타임은 같은 문서를 `_desc_of`, 즉 `_t` 절반에서 거부해 상대에게 *기술이
  이해되지 않았다*고 말했다 — 진실의 반대이고, 같은 다리 반대편의 답과도
  반대다. 원인 하나: form 리더가 묻지 않은 값 질문에 답했다. 키워드가 아니라
  규칙으로 범위 설정 — `tc_to_json`이 쓰는 모든 kind를 `_desc_of`가 읽는 모든
  kind와 대조해 `principal`이 합류했다: **세 kind, 양방향, `_form_of` 역방향
  포함**. 이제 form은 기술자로 읽히고, `_form_of`는 도착한 그 문서를 돌려주며,
  값 경로만 — 직접으로도 `any`를 통해서도 — Rust 문장과 등식으로 묶인
  `_DEFERRED`/`_UNMARSHALLABLE`로 거부하고, 오류 경로는 `_t`가 아니라 `_v`를
  이름한다. 재작성된 테스트의 부정 대조군(옛 `_desc_of` arm 복원)은
  `python_target.rs:913`에서 빨갛다.

- **A refusal's head is published, and every classifier that retyped a
  sentence now asks the function that writes it.** Two batches with one shape.
  First, scope: the four wire-refusal heads in `orbweaver-dynamic`
  (`deferred_wire_*`, `unmarshallable_wire_*`) went `pub(crate)` → `pub`,
  because the fact they own is workspace-scoped and the pin that guarded them
  (`deferred_sentence_agreement`) could only see one crate. Measured by running
  each layer: **twelve literals in two other crates** — `orbweaver-gen`'s four
  skip reasons, `orbweaver-test`'s `json_unmapped` and `why_unsupported`, four
  families each — and **one had gone false**: `prop.rs` told a contract-check
  reader that `from_json` answers `"cannot cross yet"` for a `fixed`, three
  days after that arm landed (2026-08-21). All twelve now call the heads; the
  new cross-crate gate `one_home_for_a_wire_refusal.rs` **computes** the
  expected text by calling the same functions, so a layer that keeps a literal
  fails at the next rewording, not at the next reading. Along the way the only
  construct with two call sites was spelled two ways — the type mapper said
  `abstract interface Describable`, the representability cascade
  `w2::Describable`, because an abstract interface *declaration* has no
  `TypeCode` in the registry to ask — unified by `abstract_name` (found, not
  fixed: a simple name is ambiguous across modules; qualifying all four
  families is `deferred_wire_name`'s batch, and the skip note keeps the
  repository id as its subject).
  Second, the same defect wearing the counting half's coat: **a classifier
  that matches a hand-written fragment of a sentence some other function
  owns.** Swept twelve crates: **five instances, three silent, one already
  losing in the product** — `LexError::rule` classified by a retyped prefix
  that one of its own three construction sites did not carry, so `"malformed
  fixed-point literal …"` filed under `parse` and never received the
  `fixed-literal` hint `orbweaver-forge` wrote for exactly that input. Now the
  sites and the classifier share `FIXED_LITERAL_SUBJECT` (three refusal shapes
  under one rule, tested with a `parse` negative control); the agent-reach
  sweep's two matches read published markers (`orbweaver_cdr::
  IMPLAUSIBLE_LENGTH`, `orbweaver_dynamic::json::NESTING_TOO_DEEP`) instead of
  retyped copies — no test added there, because a shared constant makes the
  drift impossible rather than detectable; and the two deferred-wire counters
  (`deferred_wire_gaps`, `deferred_wire_agreement`'s equality) compute the
  heads' markers by sentinel instead of matching `"§4.4"` — a `native` was in
  the count only because its old sentence named the section *in order to deny
  it*, and moving the wording into the shared head took the count 18 → 14
  with nothing else changed. **The harness caught that; nothing in the test
  suite did** — `a_native_is_counted_though_its_sentence_names_no_section`
  now does. Codified as two CLAUDE.md rules: a pin whose scope is narrower
  than its fact's goes green over the drift, and a classifier is a sentence
  too.

  **거부 문장의 머리는 공개되고, 문장을 옮겨 적던 분류자는 이제 문장을 쓰는
  함수에게 묻는다.** 한 모양의 배치 둘. 첫째, 범위: 와이어 거부 머리 네 개가
  `pub(crate)` → `pub` — 사실의 범위는 워크스페이스인데 핀의 범위는 크레이트
  하나였다. 계층을 실행해 측정: **다른 두 크레이트에 리터럴 열두 개**, 그중
  **하나는 이미 거짓** — `prop.rs`는 `from_json`이 `fixed`에 `"cannot cross
  yet"`이라 답한다고 인용했지만 그 계층은 사흘 전에 그 말을 그만두었다. 열두
  개 전부 이제 머리를 호출하고, 새 크레이트 횡단 게이트가 같은 함수를 호출해
  기대 문장을 **계산**하므로 리터럴을 유지한 계층은 다음 문구 변경에서
  실패한다. 유일하게 호출처가 둘인 abstract interface는 철자도 둘이었다
  (선언에는 레지스트리에 물을 `TypeCode`가 없어서) — `abstract_name`으로
  통일, 단순 이름의 모듈 간 모호성은 발견-미수정으로 기록. 둘째, 같은 결함이
  세는 쪽 외투를 입은 것: **다른 함수가 소유한 문장의 조각을 손으로 옮겨 적는
  분류자.** 열두 크레이트 스윕: **다섯 건, 셋은 침묵, 하나는 이미 제품에서
  지는 중** — `LexError::rule`이 옮겨 적은 접두사로 분류해 세 생성처 중
  하나가 `parse`로 잘못 접수되어 자기 앞으로 쓰인 `fixed-literal` 힌트를 받지
  못했다. 이제 생성처와 분류자가 `FIXED_LITERAL_SUBJECT`를 공유하고, 에이전트
  도달 스윕의 두 매치는 공개 표지(`IMPLAUSIBLE_LENGTH`, `NESTING_TOO_DEEP`)를
  읽으며 — 공유 상수는 어긋남을 탐지 대상이 아니라 불가능으로 만들므로
  테스트는 추가하지 않았다 — 지연 와이어 카운터 둘은 `"§4.4"` 매칭 대신
  센티널로 머리 표지를 계산한다. `native`는 옛 문장이 그 절을 *부정하려고*
  언급했기 때문에만 세어지고 있었고, 문구가 공유 머리로 옮겨가자 카운트가
  18 → 14로 움직였다. **그것을 잡은 것은 하네스였고 테스트 스위트는
  아니었다** — 이제 전용 테스트가 잡는다. CLAUDE.md 규칙 둘로 성문화.

- **A constant's value is the value that was written, and it is checked against
  its type.** Scoped to the rule rather than to `fixed`: **67 constant shapes
  and 25 of their neighbours outside const position**, one file each, through
  `omniidl -b dump` and through us — **26 divergences in both directions, three
  causes**. (1) *The lexer chose a Rust type and lost what it could not hold*
  (5): `9.9d` became `Float(9.90000000000000035…)` before anything ran, and
  `18446744073709551615` and `0xFFFFFFFFFFFFFFFF` were **refused**, in two
  different messages, though both are ordinary `unsigned long long`. The
  neighbour measurement is what makes this a rule about *literals* and not
  about constants: `case 18446744073709551615:` on an `unsigned long long`
  discriminator was refused by the same line of the same function. Now
  `Tok::Int(u64)`, `Tok::Fixed(FixedLit{unscaled,scale})`, `ConstExpr::Int(i128)`
  and exact decimal `+ - *` in the registry's fold — `/` folds to `None`,
  because there is no exact decimal quotient and IDL names no rounding rule to
  invent one. (2) *A constant's value was never checked against its type at
  all* (16): `registry::coerce` held the range half and its own doc comment
  called it "an IDL error the checker reports" while no checker reported it, so
  the rule's only effect was that the registry stored no value and both
  emitters skipped it in silence. Measured rather than assumed — omniidl is
  strictly typed here and converts nothing, width is not the axis (`char` and
  `octet` are one octet each and neither takes the other's literal), and
  `const double A = 5;` **is an error**. Now sema `const-value-type` /
  `const-value-range` / `not-a-const-type`, following typedefs, with fix hints
  in forge. (3) *No wide literal existed* (5): `L` lexed as an identifier, so
  `L'a'` failed with `expected ";", found 'a'` — naming neither `L` nor
  `wchar`. Now `Tok::WChar` / `Tok::WStr`, and `const_type`'s last two
  alternatives are written in `corpus/golden/30-const-type.idl`.

  **26 → 2**, and both survivors are places we follow CORBA 3.4 over omniidl,
  recorded in `corpus/divergences.tsv` with the measurement: `const long A =
  -2147483648;` (omniidl types an integer literal by the magnitude it reads
  before applying the unary minus, so the minimum of `long` and of `long long`
  cannot be written for it — `short`'s minimum is unaffected, which is what
  makes the pattern legible) and `case L'a':` (its union grammar admits no wide
  literal though its `const` does). Two more omniidl behaviours are recorded
  rather than copied: it **silently truncates a 32nd fractional `fixed` digit**
  — dropping a digit from a constant is the failure this batch exists to close,
  so we refuse it — and it reads a wide literal one *byte* at a time, so
  `L"aéb"` comes back as its UTF-8 bytes.

  What the fix unlocked, measured: **`idl-diff` was blind to every `fixed`
  constant.** Both sides folded to `None`, so a released rate could change and
  §5.3 printed "no change". It now separates `9.9d` from `9.91d` and correctly
  does *not* report `9.9d` against `9.90d` — the brief for this batch assumed
  the opposite and the oracle refuted it on the first query. `33-const-values.idl`
  also exposed a gap neither emitter had ever been asked for: `long long` and
  `unsigned long long` union discriminators, legal since `switch_type_spec ::=
  integer_type`, refused by both. Closed in both. Corpus: golden **34 → 36**,
  negative **19 → 23** (`n19` class, `n20` range, `n21` long double, `n22`
  literal shape), every one checked against `omniidl -b dump`. Seven new gates,
  each landed with its negative control run **red** (D010 §7.2).

  **상수의 값은 쓰인 그대로이며, 자기 타입에 대해 검사된다.** `fixed`가 아니라
  **규칙**으로 범위를 잡았다: 상수 형태 67개와 const 자리 밖 이웃 25개를 한 파일씩
  omniidl과 우리 양쪽에 통과시켜 **양방향 26건의 불일치, 원인 셋**을 얻었다.
  (1) *렉서가 러스트 타입을 골라 담지 못한 것을 잃었다*(5) — `9.9d`는 아무것도
  실행되기 전에 부동소수가 되었고, 평범한 `unsigned long long`인
  `18446744073709551615`와 `0xFFFFFFFFFFFFFFFF`는 서로 다른 두 메시지로 **거부**
  되었다. 이것이 상수가 아니라 **리터럴**에 관한 규칙임은 이웃 측정이 말한다:
  `unsigned long long` 판별자의 `case 18446744073709551615:`이 같은 함수의 같은
  줄에서 거부되었다. (2) *상수의 값을 타입에 대해 검사하는 코드가 아예 없었다*(16)
  — `registry::coerce`의 문서 주석은 "검사기가 보고하는 IDL 오류"라고 적혀 있었고
  보고하는 검사기는 없었다. 그래서 규칙의 유일한 효과는 레지스트리가 값을 저장하지
  않는 것이었고, 두 이미터는 그것을 조용히 건너뛰었다. 가정이 아니라 측정이다 —
  omniidl은 여기서 엄격하며 변환하지 않고, 폭은 축이 아니며(`char`와 `octet`은
  둘 다 1옥텟이지만 서로의 리터럴을 받지 않는다), `const double A = 5;`는 **오류다**.
  (3) *넓은 리터럴이 존재하지 않았다*(5) — `L`이 식별자로 렉싱되어 `L'a'`는 `L`도
  `wchar`도 이름하지 않는 메시지로 실패했다.

  **26 → 2.** 남은 둘은 우리가 omniidl 대신 CORBA 3.4를 따르는 자리이며 측정과
  함께 `corpus/divergences.tsv`에 기록했다. omniidl의 두 동작은 복사하지 않고
  기록했다: 32번째 **소수부** `fixed` 자리를 말없이 잘라내는 것(상수에서 자리 하나를
  말없이 잃는 것이 이 배치가 닫으려는 실패이므로 우리는 거부한다)과 넓은 리터럴을
  한 **바이트**씩 읽는 것. 고침이 드러낸 것: **`idl-diff`는 모든 `fixed` 상수에
  눈이 멀어 있었다** — 양쪽이 `None`으로 접혀, 배포된 요율이 바뀌어도 §5.3은
  "변경 없음"을 찍었다. 코퍼스는 golden 34 → 36, negative 19 → 23이고, 새 게이트
  일곱은 각각 음성대조를 **red로 돌린 뒤** 착지했다(D010 §7.2).

  **Landed four days after its base, and the landing took the measurement the
  batch's own machine could not.** The branch recorded the new file and its
  `L`-prefixed literals as **unmeasured against a second front end** — JacORB
  and TAO were absent there. JacORB 3.9 is a fixture here, and it disagrees
  twice, both recorded in `corpus/divergences.tsv` with what was measured and
  neither changing what we do. It **cannot lex a `fixed` literal whose written
  integer part begins with `0`**: `0.0d`, `0.5d`, `0.001d`, `0.10d`, `0d` and
  `000000001d` each stop the parse at the literal, while the same values are
  taken with the integer part absent (`.5d`), behind a sign (`-0.5d`) or with a
  nonzero first digit (`1.0d`, `2.50d`, `100d`), and `const double B = 0.0;`
  and `const long C = 010;` both compile — so it is the `d` suffix, not the
  leading zero, and a decimal type it otherwise supports cannot state zero.
  And it **accepts `const long double`**, which omniidl and we refuse: it
  writes `double value = 1.0;`, so the constant silently becomes narrower than
  it was declared — the outcome the refusal exists to prevent, which is a
  better argument for the refusal than the grammar was. Twenty-one one-line
  probes, 2026-08-24.

  **기반보다 나흘 늦게 착지했고, 착지가 배치의 기계에서는 할 수 없던 측정을 했다.**
  브랜치는 새 파일과 `L` 리터럴을 **두 번째 프런트엔드에 대해 미측정**으로 기록했다 —
  그 기계에 JacORB도 TAO도 없었기 때문이다. 여기에는 JacORB 3.9가 fixture로 있고,
  두 곳에서 갈린다. 하나, **정수부가 `0`으로 시작하는 `fixed` 리터럴을 렉싱하지
  못한다**: `0.0d`, `0.5d`, `0.001d`, `0d`, `000000001d`가 리터럴에서 파스를
  멈추는 반면 `.5d`, `-0.5d`, `1.0d`, `100d`는 통과하고 `const double B = 0.0;`와
  `const long C = 010;`은 컴파일된다 — 앞자리 0이 아니라 `d` 접미사의 문제이며,
  지원한다는 십진 타입으로 0을 적을 수 없다는 뜻이다. 둘, **`const long double`을
  받아들인다** — omniidl과 우리가 거부하는 것을. 그리고 `double value = 1.0;`을
  써낸다: 상수가 선언된 것보다 조용히 좁아진다. 거부가 막으려는 결과 그 자체이며,
  문법보다 나은 거부 근거다. 한 줄짜리 탐침 21개, 2026-08-24.

- **A `native` refusal comes from one place, as §4.4's three already did — and
  two of the ten sentences it replaced were false.** 41b352d gave the three
  deferrals one sentence across the CDR path, the AnyJSON path and the
  generated Python runtime; 22637a8 then added a fourth family whose refusals
  were written standalone, because the helper was not on its branch. Measured
  before repairing, by running each layer rather than reading it: **ten
  distinct sentences across thirteen call sites** for one type. Two of them lie
  to a reader, and both sit in the layer a *peer-fed* document meets — the
  AnyJSON read direction said a native `cannot cross yet`, promising a version
  that will never carry one, and the dynamic navigator's default pointed at
  §4.4, which does not defer a native and never will. Now five Rust layers and
  the Python runtime read two functions, held by equality across the crate
  boundary. Scoped to the rule rather than the keyword, it found three more of
  the same: `dynany.rs` named §4.4 for **all five** of its refusals including
  `Principal`, which is in no section at all; the generated Python runtime
  wrote a **fourth** wording for `fixed`, in the peer-facing layer, measured by
  nothing until it was broken on purpose; and `orbweaver-gen` kept the `fixed`
  sentence as three identical literals. The pin asserts the *distinction* too —
  the native sentence must not claim a §4.4 deferral and must not say "yet".

  **음성대조 하나가 초록으로 돌아왔다**, 그리고 그것이 이 항목에서 가장 중요한
  줄이다: S4 규칙의 `fix()`를 *"wait for §4.4 to land natives…"* 라는 바로 그
  거짓으로 바꿨는데 통과했다 — "yet"도 없고 유예 주장 문자열도 없었기 때문이다.
  부분문자열 둘은 규칙이 아니다. `§4.4` 언급 40자 이내에 부정이 있어야 한다는
  조건으로 넓힌 뒤에야 red가 되었다. 그 절을 방금 읽은 사람이 쓴 대조였고, 리뷰가
  아니라 실행이 잡았다. 나머지: `native` 거부가 호출 지점 **13곳에 문장 10개**로
  흩어져 있었고 그중 둘은 거짓이었다(AnyJSON 읽기 방향의 `cannot cross yet`,
  `dynany`의 §4.4 지시). 이제 러스트 다섯 계층과 파이썬 런타임이 함수 두 개를
  읽으며, 크레이트 경계를 넘어 동등성으로 고정된다. 규칙으로 범위를 잡자 같은
  부류가 셋 더 나왔다 — `dynany`는 `Principal`까지 §4.4로 돌려보냈고, 생성된
  파이썬 런타임은 `fixed`에 네 번째 문구를 따로 썼으며, `gen`은 같은 리터럴을
  세 번 복사해 두고 있었다.

- **A `native` and a `ValueBase` are no longer recorded as object references.**
  `native X;` was `TypeCode::ObjRef`, so both emitters generated a reference
  and the dynamic path put an **IOR** on the wire for a type that has no wire
  form at all; `ValueBase` was the same defect in the one spelling with no
  declaration behind it, and it is a valuetype. The peer was asked before a
  representation was chosen, and **for `native` the measurement is a refusal by
  all four routes omniORB has**: `-b dump` accepts the declaration, `-bcxx`
  exits 1 on it, `-bpython` ignores it and leaves a dangling `typeMapping`
  entry that raises `KeyError` one import later, and the ORB has no
  `create_native_tc` — `createTypeCode((tv_native, …))` raises `INTERNAL`. So
  `TypeCode::Native` has **no `TCKind`**, `encode` refuses it by name, and
  `from_u32` still has no arm for 31, refusing a peer that sends one. For
  `ValueBase` the measurement is bytes: `tk_value`, **VM_NONE** — not
  VM_ABSTRACT, which is what a reasoned answer gets wrong — `tk_null` base,
  zero members, byte-for-byte in both stream orders. S4's `wire/deferred-type`
  rule now closes over natives with **its own sentence** — a native is not
  deferred, there is nothing to defer — and `deferred_wire_agreement` holds the
  rule and both emitters to one set of thirty. Two corpus files, both accepted
  by `omniidl -b dump`; **JacORB rejects both with NullPointerExceptions rather
  than diagnostics**, recorded in `corpus/divergences.tsv`. Landing it also
  closed two things it uncovered: a native TypeCode crossed AnyJSON as the
  string `"void"` (the silent wrong answer D008's rule exists to prevent — the
  arm's first negative control came back **green**, because the property's JSON
  leg carries values and not TypeCodes, so the arm was unmeasured until a
  native joined that test), and a sequence whose element cannot be sampled has
  exactly one value, the empty one, which the CDR leg took and the JSON leg
  refused — **5824 of 5952** round trips. The leg now runs for it: **5952 of
  5952**, 128 more measured than before.

  **`native`와 `ValueBase`를 더 이상 객체 참조로 기록하지 않는다.** `native X;`가
  `TypeCode::ObjRef`였고, 두 이미터 모두 참조를 생성했으며 동적 경로는 와이어
  형식이 아예 없는 타입에 IOR을 실었다. `ValueBase`는 선언이 없는 철자에서 같은
  결함이 살아남은 경우이며, 그것은 valuetype이다. 표현을 정하기 전에 피어에게
  물었고, **`native`의 측정은 네 경로 모두에서의 거부다**: `-b dump`는 선언을
  받아들이고, `-bcxx`는 exit 1, `-bpython`은 선언을 무시한 뒤 끊어진
  `typeMapping`을 남겨 import에서 `KeyError`를 내며, ORB에는 `create_native_tc`가
  없다. 그래서 `TypeCode::Native`에는 `TCKind`가 없고, `encode`는 이름을 대며
  거부하며, `from_u32`에는 31 아암이 없어 31을 보낸 피어도 대칭적으로 거부된다.
  `ValueBase`의 측정은 바이트다: `tk_value`, **VM_NONE**(VM_ABSTRACT가 아니다 —
  추론으로 답하면 틀리는 자리), `tk_null` 기반, 멤버 0, 두 스트림 순서 모두 바이트
  단위 일치. S4의 `wire/deferred-type` 규칙은 native까지 폐쇄집합에 넣되 **문장은
  따로 둔다** — native는 미뤄진 것이 아니라 미룰 것이 없다. 코퍼스 두 파일은
  `omniidl -b dump`가 받아들이고 **JacORB는 둘 다 진단이 아니라 NPE로 거부**하여
  `corpus/divergences.tsv`에 기록했다. 착지 과정에서 드러난 둘도 함께 닫았다:
  native TypeCode가 AnyJSON을 문자열 `"void"`로 건너던 것(D008이 막으려던 조용한
  오답 — 이 아암의 첫 음성대조는 **초록으로 돌아왔다**. 속성 검사의 JSON 다리는
  값을 건네지 TypeCode를 건네지 않기 때문이며, native가 그 테스트에 들어가기
  전까지 아암은 측정되지 않고 있었다), 그리고 원소를 표본화할 수 없는 시퀀스의
  유일한 값인 빈 시퀀스를 CDR은 세고 JSON은 건너뛰던 것(**5824/5952**). 이제 다리가
  돌고 **5952/5952**, 이 배치 이전보다 128 왕복을 더 잰다.

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

- **Four catch-alls that would answer for a construct nobody has met.** One
  shape, swept across four crates, and in every case the repair is
  exhaustiveness carried by the compiler rather than by a comment.
  - **`anyjson::type_name` named nothing.** It listed fifteen primitives and
    asked everything else for a repository id, and **seven variants carry
    none** — `sequence`, `array`, `any`, `typecode`, `void`, `null`,
    `Principal` — so a **peer-fed** document naming a `void` was answered
    *"`<anonymous>` cannot cross yet"*, and a value of the wrong shape for a
    `sequence<long>` was answered *"is not a value of `<anonymous>`"*. Its own
    doc comment claimed the decoder *"says so rather than guessing"*; it said
    `<anonymous>`. **This class was diagnosed once already, in this file, and
    closed the wrong way**: a `fixed` was refused that way until 2026-08-21 and
    the repair was a guard *above* the mismatch arm rather than the function,
    which took the one witness out of reach and left the defect live for seven
    other variants for four more days. **A guard that stops one caller reaching
    a defect is not a fix for the defect.** The bound is now in the subject
    (`string<5>`, `sequence<octet, 7>`) because it is in the type.
  - **`tc_to_json`'s tail was `short_name(other).unwrap_or("void")`** under the
    comment *"Every remaining variant has a short name and returned above"* —
    true of all 33 variants the day it was written and a silent lie the day a
    34th arrives. Measured, not argued: with the repairs stashed, a local
    interface's *description* crossed the wire as the string `"void"`.
  - **`describe`'s tail named a 34th variant `an indirection`** — and both
    constructs this project has actually met late arrive with no `TcKind`, so
    that is a refusal a peer reads, and *"expected a value of type an
    indirection"* is a sentence this project has already shipped once.
  - **`render_type` ended in `_ => "<unnamed type>"`** under a comment claiming
    everything left *"v1 does not marshal or does not name"*. **Half of that
    was false and the false half reached a reader**: the catch-all swallowed
    nine of the 33 variants, two of which the v1 wire marshals in both
    directions — including the very type another fix had rescued from becoming
    a silent `void`. It feeds the operation signature line in the S3i subject,
    the prompt a model and a human read, so `long double price()` arrived as
    `<unnamed type> op(...)`. All 33 now carry a verdict: 26 unchanged, 2
    repaired and marshalled, 4 repaired and refused by the wire (spelled by
    calling the owning crate's `*_subject` functions, so the prompt shows the
    string the marshaller will refuse it by), and 3 keep a placeholder **with
    the reason true of it**. Swept for the same shape, `is_reference` had the
    defect pointing the other way — `_ => false` meant a reference travelling
    inside a `struct` or `union` was **not** marked, so `Object get_root()` was
    a caution and `struct Handle { Object it; } get_root()` was not.

  The controls for these are **build** controls, not red tests, because
  exhaustiveness has no assertion in it by construction: a 34th variant added
  to the owning crate turns each site into `error[E0004]`. A third home for the
  naming fact was found and **reported rather than reached into**, with a live
  wrong answer — a `native` parameter described to an agent as `<recursive>`.

  **아무도 만난 적 없는 구성물을 대신 답했을 포괄 팔 넷.** 한 모양, 네 크레이트.
  `type_name`은 아무것도 이름하지 않았고(**피어가 보낸** 문서가 `<anonymous>`라는 답을
  받았다), 그 계급은 **이 파일에서 이미 한 번 진단되고 틀린 방식으로 닫혔다** — 함수가
  아니라 그 위에 가드를 세워 목격자 하나를 치우고 결함은 일곱 변형에 나흘 더 살려
  두었다. **한 호출자가 결함에 닿지 못하게 막는 가드는 결함의 수정이 아니다.**
  `tc_to_json`은 34번째 변형의 *기술*을 문자열 `"void"`로 와이어에 실었을 것이고(측정),
  `render_type`의 주석은 절반이 거짓이었으며 **그 거짓 절반이 독자에게 닿았다.**
  대조군은 빨간 테스트가 아니라 **빌드** 대조군이다.

- **`SymbolKind::is_type` counted an exception as a type**, so
  `struct S { E field; };` validated clean here and omniidl refuses it. Found
  by asking which kinds a `not-a-type` file could be written from — because
  there was **no such file**. `raises` now asks only that the name resolve, and
  that nothing checks it resolves to an *exception* is written down rather than
  smuggled under a rule about types. Found with it: **a rule id names a class,
  and a second diagnosis joins it and inherits a hint written about the first**
  — five instances, two already losing in the product, where the span differs
  with the diagnosis so a hint written to quote one thing quotes another. And
  **a hint keyed to a rule no corpus file produces has never been executed** —
  three rules, the same gap the target-keywords file closed for escaping. All
  31 negative files are refused by both front ends, measured file by file.

  **`SymbolKind::is_type`가 예외를 타입으로 셌다** — 그래서 여기서는 깨끗이 통과하고
  omniidl은 거부하는 파일이 있었다. `not-a-type` 파일을 무엇으로 쓸 수 있는지 물어보다
  발견했다 — **그런 파일이 없었기 때문이다.** 함께 발견: 규칙 아이디 하나에 진단 둘이
  붙어 **뒤엣것이 앞엣것에 대해 쓰인 힌트를 물려받는다**(5건, 2건은 이미 제품에서
  지고 있었다), 그리고 **어떤 코퍼스 파일도 만들지 않는 규칙의 힌트는 실행된 적이
  없다**(3건).

- **Refusals in the constraint language that pointed at nothing, or at the
  wrong thing.** `unexpected character 'x'` gave a byte position and nothing to
  fix — half the bar that module's own docs set for every other refusal in it.
  **A float literal too large to represent parsed to `inf`**, because
  `f64::from_str` answers `Ok(inf)` rather than an error, so a bound written
  with four hundred nines **matched everything, silently**, while the counter
  field next door had always refused its own overflow by name. **A lowercase
  keyword where a field belongs was reported as an unknown field**: before
  `NOT` and `EXIST`, every lowercase keyword happened to fall where the parser
  was already expecting one, so the *"keywords are uppercase"* hint was reached
  by luck. And **`Selection::unanswerable`'s own sentence went false in both
  directions the moment the grammar grew** — the criterion it described was the
  whole truth under a chain of `AND`s and is not under `OR` or `EXIST`; both it
  and the operator-facing note now state the criterion that is actually
  computed. Nothing was red, because a sentence is not compiled. Added with
  the nesting constructs rather than after them: `MAX_DEPTH = 64`, because
  unbounded nesting over untrusted input is a **stack overflow, which is a
  crash and not a refusal**, and this parser's whole argument for being
  first-party is that it refuses with a position.

  **아무 데도, 혹은 엉뚱한 데를 가리키던 제약 언어의 거부들.** 표현 불가능한 부동
  소수 리터럴이 `inf`로 파싱되어 **모든 것에 맞는 한계가 조용히 만들어졌다.**
  소문자 키워드 힌트는 **운으로** 닿고 있었다. 그리고 문법이 자라는 순간
  `Selection::unanswerable`의 문장이 양방향으로 거짓이 되었다 — 문장은 컴파일되지
  않으므로 아무것도 빨갛지 않았다. 중첩 구성물과 **함께** 깊이 한계를 넣었다.

- **A refusal that named a byte in a string that no longer existed.** `WITH
  <constraint>` delegates to the constraint parser, whose positions are offsets
  into whatever text it was given; the obvious fix-up is to parse the substring
  and add the keyword's length to the returned position, and that is what the
  first pass did. It was wrong in a way worth writing down, because **some
  refusals name a *second* position inside their own sentence** — so `WITH
  (cost == 1` reported *"at byte 15: expected ')' to close the '(' at byte 1"*:
  one refusal naming two different places for one bracket, and the place it
  pointed at was a byte of a substring the caller never had. **Nothing about it
  looks wrong until you count.** The repair is not more arithmetic: the
  constraint is parsed over *this* text with the keyword blanked to spaces, so
  every offset is already an offset into what the caller holds and there is
  nothing left to keep in step — a fact reachable from one place cannot drift
  from itself.

  **이제 없는 문자열의 바이트를 가리키던 거부.** 부분 문자열을 파싱하고 키워드 길이를
  더하는 뻔한 보정이 틀렸다 — **어떤 거부는 자기 문장 안에서 *두 번째* 위치를
  이름하기 때문**이다. 하나의 괄호를 두고 서로 다른 두 곳을 말했고, 가리킨 자리는
  호출자가 가진 적 없는 부분 문자열의 바이트였다. **세어 보기 전에는 이상해 보이지
  않는다.** 산술을 더하는 대신 키워드를 공백으로 지우고 *이* 본문에서 파싱한다.

- **Comments asserting what another crate does, and one that had been false for
  eleven days.** A policy helper named in three sites of one module and one of
  another was **removed and split in two on 2026-08-14**, while what the
  comments described stayed true — so nothing was wrong except that they named
  a symbol that had not existed for eleven days. The set's real home is
  private, so the copy that mattered got a **behavioural pin through the public
  API** rather than a fourth sentence. Re-verified and left alone: the three
  neighbouring claims that are still true, one of them checked as `26 = 26 =
  26` and honestly recorded as **unpinned**. Also re-blessed here:
  `spikes/bench/stub.rs`, a checked-in generated stub outside `tests/emitted/`
  under the same freshness gate, which the identifier batch left stale **on
  purpose and reported rather than reaching outside its footprint**. Worth
  recording, because it is the same class one turn later: a decision document
  carried a row saying that stub had no `redirect` hook, which was **already
  false when it was written**; repairing that row replaced it with *"the gap is
  the re-blessing discipline, not a missing hook"* — and the discipline was
  claimed the same afternoon.

  **다른 크레이트가 무엇을 하는지 주장하는 주석들, 그중 하나는 열하루 동안 거짓.**
  기술한 내용은 참인 채로 **존재하지 않는 심볼을 이름하고 있었다.** 진짜 집이 비공개라
  네 번째 문장 대신 **공개 API를 통한 행동 고정**을 붙였다. 여전히 참인 이웃 셋은
  다시 확인해 두었고, 그중 하나는 **고정되지 않았다고 정직하게 적었다.**

### Known limits / 알려진 한계

- **A `TAG_SSL_SEC_TRANS` component produced by omniORB's or JacORB's own
  encoder is still unmeasured, and stays named as such.** Everything else B3
  claims is measured, but a component with the association-option bits and port
  convention *that* implementation chose is a claim about their encoder and
  **only they can make it**. A hand-built advertisement is not ours either —
  the octets come from the specification in another language in another process
  — which is what makes the rest of the measurement worth having.
- **A `corbaloc:` reference produced by omniORB loses its alternate
  addresses.** Components are read only when the profile's minor version is at
  least 1, and omniORB folds a multi-address `corbaloc:` into
  `TAG_ALTERNATE_IIOP_ADDRESS` on a **1.0** profile — so failover to the second
  address never happens. Pinned, not fixed: the fix changes how every
  reference from every peer is parsed.
- **`corbaname:` with a name fragment is refused, not resolved.** It denotes
  the object bound under that name (Part 2 §7.6.10.5), which means dialling
  inside a conversion — new behaviour with a timeout to choose. The refusal
  names the name it would have to resolve and points at the two-step a caller
  can see today.
- **The Interface Repository answers in repository-id order, not declaration
  order.** §14.5.4.1 asks for declaration order and `Registry` holds entries in
  `BTreeMap`s, so it is gone by the time the IDL is loaded. The order given is
  total, stable and identical in both byte orders, and is **recorded as a
  divergence rather than faked**. The facade also claims only Repository,
  ModuleDef and InterfaceDef as containers, though §14.5.10 and §14.5.20 also
  make a `StructDef` and an `ExceptionDef` ones — because a servant that
  refused the narrow `_is_a` and then honoured the operation would tell a
  client two different things about one reference. Widening means widening
  both, in one commit.
- **Two POA key spaces, one unstated invariant, deliberately not fixed.**
  `Poa::object_key` concatenates name, optional incarnation and id with nothing
  constraining the components, so under `Lifespan::Persistent` a POA named
  `Root` with id `POA/x` and one named `Root/POA` with id `x` mint the
  **identical object key**, and each POA's parser accepts the other's — while
  the same crate enforces exactly that rule for its other key space, and
  neither names the other. **No fix is behaviour-preserving**: refusing `/`
  changes behaviour for a data-driven caller that mints ids from expert ids and
  the minting function has no failure channel, and escaping the separator
  changes every key already minted, persistent ones included. No caller in this
  workspace puts a `/` in either today. Documented and pinned as measured.
- **One creation path does not apply the integrity rules its sibling
  enforces.** A manifest may name the same capability twice, and may name one
  whose expert already exists over a different base — both of which the
  explicit bind refuses with `BAD_PARAM` — so a model can be composed from an
  adapter its own base does not match. Same rule, two paths, one enforcing it:
  the shape that stays invisible until somebody measures both paths. **Pinned
  as measured and explicitly not endorsed** — making the paths agree turns that
  test red, which is the signal wanted.
- **`ai_unit` and `ai_idempotent` are read by checkers and by no consumer.** A
  weaker claim than inert: both are warned about when misapplied, and nothing
  converts, renders, validates or retries on either — the unit does not reach
  the stub, the docstring or the bridge, and the retry-safety a contract
  declares steers nothing. With the two keys that now reach a reader, **the
  live half of SIDL's eight-key vocabulary is four.**
- **One sentence still has four homes and the fix is out of one footprint.**
  The annotate-or-assume advice exists in an S4 fix hint, a guard-chain remedy,
  a server startup summary and a console legend; the crates do not depend in
  the direction that would let one publish it, **and the four are not the same
  string anyway** — one offers three values, another two. Reported rather than
  reached into; the fix is the owning crate publishing the hint from a function
  both call.

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
