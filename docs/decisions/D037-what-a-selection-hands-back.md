# D037 — What a selection hands back, and whether it may name places

**STATUS: APPROVED 2026-08-31 — option C.** The owner chose **C: accept the
exposure as a named floor**, which is §5's recommendation unchanged, with B
recorded as available and refused for now.

The approval carries §5's three conditions, and they are the whole of what
separates an accepted floor from silence: the floor is **stated where the
contract is** (`corpus/golden/22`'s comment becomes a statement of an accepted
limit rather than an observation about a hazard); there is **a test that the
addresses are what the caller sees**, so the day somebody proxies them the row
moves deliberately rather than quietly; and **the reason is recorded as a reason
and not as an intention** — not *"until we get to it"* but *"closing it requires
`Router::dispatch`, and that refusal is a separate decision"*.

§6.4's cost is accepted with it and not waived: a criterion whose rows are mostly
named floors is measuring the shape of the repository rather than the
transparency. This is the second such floor, after D035's lifecycle one.

**상태: 승인 2026-08-31 — 선택지 C.** 소유자가 **C: 노출을 이름 붙인 바닥으로
수용**을 골랐고, 이는 §5의 권고 그대로이며 B는 가능하나 지금은 거절된 것으로
기록한다.

승인은 §5의 조건 셋을 함께 지고 가며, 그 셋이 바로 **수용된 바닥과 침묵을 가르는
전부**다: 바닥은 **계약이 있는 자리에** 적히고(`corpus/golden/22`의 주석이 위험에
대한 관찰이 아니라 받아들인 한계의 진술이 된다), **주소가 호출자에게 보이는 것임을
주장하는 테스트**가 있어야 하며(누군가 프록시하는 날 행이 조용히가 아니라
의도적으로 움직이도록), **이유는 의도가 아니라 이유로** 기록된다 — "나중에 할
때까지"가 아니라 "닫으려면 `Router::dispatch`가 필요하고 그 거절은 별개의
결정이다". §6.4의 비용도 함께 받아들인다: 행 대부분이 이름 붙인 바닥인 기준은
투명성이 아니라 저장소의 모양을 재는 것이다.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. This is about one open leak under its Location row.

---

## 1. What raises this / 무엇이 이 문서를 불렀나

`moe::Router::select` returns `ExpertSeq` — a sequence of `Expert` object
references. Each is an `Ior` stored verbatim by `register_expert` and marshalled
back inline, **host, port and object key**. One authorised call and a caller
knows where every candidate expert runs.

D029's Location row says a caller must not be able to tell where a target is.
`corpus/golden/22`'s own comment beside the operation says the same thing before
any of this was measured:

> Hands back a whole sequence of dialable experts. Reading nothing and
> **widening reach by N addresses at once is precisely the case §4.7's
> bearer-address rule exists for.**

So the contract's author flagged it, the specification half of the same fact is
§4.7, and the row has carried it as *recorded, not changed* since 2026-08-26.

*계약의 저자가 이미 표시해 두었고, 같은 사실의 명세 절반이 §4.7이며, 행은 그것을
"기록했으나 바꾸지 않음"으로 지고 있었다.*

---

## 2. What is **not** the question / 질문이 아닌 것

**Whether the caller may know which experts exist.** It may: `Router` is
`//@ ai_desc: Control-plane gate`, `select` is `//@ ai_effect: read_only` behind
`//@ ai_authz: moe.router.select`, and a gate whose callers may not know what it
selected from is not a gate. The Activation row already reasons this way about
*load state* — a right to be **told**, not a licence for a side channel.

**That reasoning does not carry to addresses, and the difference is the whole
of this document.** Load state has two contract homes where it is a value a
caller *asks for* — `ExpertLoader::status` and `Capability::state` through
`Expert::describe`. **An address has none.** Nothing in `corpus/golden/22`
declares a member, an operation or an attribute whose value is where an expert
runs. So a caller learning it is not being told; it is reading a fact the
contract never offers, off the marshalled form of something it was given for
another purpose.

*호출자가 무엇이 선택되었는지 알아도 되는가는 질문이 아니다 — 되어야 한다. 부하
상태는 계약에 **묻는 자리**가 두 곳 있고, **주소는 하나도 없다**. 그러므로 주소를
알게 되는 것은 들은 것이 아니라, 다른 목적으로 받은 것의 마샬 형태에서 읽어낸
것이다.*

---

## 3. What D035 has already settled / D035가 이미 정리해 둔 것

The obvious repair is to hand back references that point at **this** service,
which then forwards — N addresses become one. That is the shape D035 asked the
owner about, in a different row, and the answer was **displacement is not
closure**.

It applies here without translation. A caller that only ever calls `select`
would learn one address instead of N; a caller that obtains an `Expert`
reference from `ExpertRegistry`, from an earlier call, or from a string never
went through `select` at all. **Proxying changes how many addresses a caller
learns per call, never whether it can learn one.**

This is not an argument that the repair is worthless. It is an argument that it
must be proposed as *reducing exposure*, and refused as *closing the row* — and
the two are told apart here rather than after the work.

*명백한 수리는 N을 1로 만든다. D035가 그 형태를 이미 물었고 답은 **변위는 폐쇄가
아니다**였다. 노출을 줄이는 것으로 제안되어야지, 행을 닫는 것으로 제안되면 안
된다.*

---

## 4. The candidates / 후보

**A — `select` stops returning references.** It answers capability descriptions
(an id, a `Capability`) and the caller reaches an expert some other way.

- *For.* It is the only candidate that removes the address from the reply
  entirely, so it is the only one that could close the row rather than narrow
  it.
- *Against, and it is decisive today.* **There is no other way.** The operation
  that would be it is `Router::dispatch`, which is refused for a reason that
  lives in `expert_service.rs`'s module docs and in D006 — it carries an
  `Activation`, and serving it would silently commit the project to a reading of
  `Tensor` that binds nothing and is enforced by nothing. A closes the leak by
  requiring an operation this project has declined to serve, so A is a proposal
  about `dispatch` wearing `select`'s name.

**B — `select` returns references that name this service.** The registry mints
a reference to itself per expert and forwards on invocation.

- *For.* Fewer addresses per call, and the mechanism exists — `LOCATION_FORWARD`
  is implemented, served and measured.
- *Against.* §3: displacement, not closure. And it makes the control plane the
  data path for every expert call, which is the coupling `Router::dispatch`'s
  refusal exists to avoid — arriving through the back door of a different
  operation.

**C — accept it, record it as a named floor, and say why.** The row keeps the
leak with its reason: `select` is a control-plane gate, its callers are
authorised, and closing it needs an operation the project has declined to serve.

- *For.* It is true, it is checkable, and it is what the row has been doing
  informally since 2026-08-26 — this makes it a decision instead of a silence.
- *Against.* A recorded leak is still a leak, and a row that accumulates named
  floors stops being a criterion. **This is the real cost and it is not
  rhetorical**: D029's Lifecycle row already carries two.

**D — narrow the contract's authorisation.** Leave the reply and require a
stronger `ai_authz` for `select`.

- *Against.* It changes who can read the addresses, not whether the addresses
  are there. This project's own rule for that shape is that a permission is not
  a property.

---

## 5. Recommendation / 권고

**C, with B named as available and refused for now.**

Not because C is comfortable — §4 says what it costs — but because A requires
serving an operation the project has declined on grounds this document does not
reopen, and B buys displacement that D035 has already ruled is not closure while
coupling the control plane to every call.

What C must include to be worth more than silence:

1. **The floor stated where the contract is**, not only in a ledger cell:
   `corpus/golden/22`'s comment becomes a statement of an accepted limit rather
   than an observation about a hazard.
2. **A test that the addresses are what the caller sees**, so the day somebody
   proxies them the row moves *deliberately*. A floor nobody asserts is a floor
   that can quietly stop being one — which is the argument D035's approval made
   for the lifecycle floor, and the reason L5 was worth measuring.
3. **The reason recorded as a reason and not as an intention**: this is not
   *"until we get to it"*. It is *"closing it requires `Router::dispatch`, and
   that refusal is a separate decision"*.

*A는 프로젝트가 거절한 연산을 요구하고, B는 D035가 이미 폐쇄가 아니라고 판정한
변위를 사는 대신 제어 평면을 모든 호출에 결합시킨다. C가 침묵보다 낫기 위해
필요한 것 셋: 계약 자리에 적힌 바닥, 그것을 **주장하는 테스트**, 그리고 의도가
아니라 이유로 기록된 이유.*

---

## 6. What would refute this / 무엇이 이것을 반증하는가

1. **`Router::dispatch` becoming servable.** A's whole objection is that reply
   without a reference leaves the caller no route. If the `Tensor` reading is
   ever settled and `dispatch` is served, A stops being `dispatch` in disguise
   and becomes the candidate that closes the row.
2. **A consumer that needs the address.** If something in this tree dials an
   `Expert` reference obtained from `select` and could not work with a
   forwarding one, B is refuted on its own terms rather than on §3's.

   **Swept 2026-08-30, and the sweep's own first answer was wrong.** Grepping
   for `select` across the workspace returns `orbweaver-trading`'s
   `Query::select`, `select_reporting` and `select_preferring` — the offer-store
   engine, a **different operation with the same name**, reached by no wire at
   all. Reading that as consumers of `Router::select` would have been the
   classifier defect this project names elsewhere: matching a name instead of
   asking what owns it. Filtered to the wire operation, **nothing in this
   repository dials a reference that `Router::select` returned**:
   `spikes/service_sweep.py` calls the operation and checks the reply's shape,
   and does not invoke on what comes back.

   *Nothing found is not nothing there* — the sweep covered callers in this
   repository, which is where the argument is being made and is not where a
   deployment lives.
3. **A second contract home for the address.** If a future operation declares
   *where an expert runs* as a value a caller asks for, §2's distinction
   collapses and this becomes the Activation row's argument rather than its
   counter-example.
4. **The floor count.** If D029 accumulates a third and fourth named floor, the
   objection in §4's C stops being a cost and becomes the finding: a criterion
   whose rows are mostly floors is measuring the shape of the repository rather
   than the transparency.

---

## 7. What this document does not claim / 주장하지 않는 것

- It does not claim the leak is small. One authorised call yields **every**
  candidate's address; that is wider than most leaks this project has closed.
- It does not reopen `Router::dispatch`'s refusal. §4's A depends on it and says
  so; the refusal's home is elsewhere.
- It does not claim the sweep in §6.2 was exhaustive beyond this repository.

*누출이 작다고 주장하지 않는다. 인가된 호출 한 번이 **모든** 후보의 주소를 준다.*
