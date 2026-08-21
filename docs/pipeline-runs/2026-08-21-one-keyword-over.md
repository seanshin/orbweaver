# 2026-08-21 — one keyword over, and the sentence that was written ten times / 키워드 하나 옆, 그리고 열 번 쓰인 문장

> Five batches in one day, each landed serially through the harness. Kept
> because four of the five found the *same shape* the batch before it had
> closed, one keyword or one layer over — and because three of the day's
> findings are about how a gate fails green, not about CORBA.
>
> 다섯 배치, 전부 하네스를 통해 직렬 착지. 다섯 중 넷이 **바로 앞 배치가 닫은 것과
> 같은 모양**을 키워드 하나 옆, 계층 하나 옆에서 다시 찾았다.

## 1. The five / 다섯

| | what it closed | what it cost to find |
|---|---|---|
| W3 | the AnyJSON layer refused a §4.4 instance without naming the rule — the one layer a peer-fed document actually meets | it also reported `ValueBase` marshalling as an object reference, and did not fix it |
| W4 | **D013**: two references to one object, decided by measuring instead of building | the premise was refuted by the ORB it named |
| W1 | `native` and `ValueBase` were object references — an IOR on the wire for a type with no wire form | two harness groups went red, one of them a real finding |
| W5 | the `native` refusal, written ten times in thirteen places, twice falsely | a negative control came back **green** |
| W2 | a `fixed` constant had no value, and `idl-diff` was blind to every one of them | *not landed at time of writing — see §6* |

## 2. Twice, the same defect one keyword over / 두 번, 같은 결함이 키워드 하나 옆에서

2026-08-20 closed *"a `valuetype` and an abstract interface go on the wire as
object references"*. The batch that closed it **reported** that `native X;` was
still `TypeCode::ObjRef` and stopped, with an honest reason that was the
defect: *"§4.4 does not name it, so a change here would be a claim no gate
checks."* The fix for that is a rule, not a comment. `ValueBase` was the same
defect in the one spelling with no declaration behind it.

W1 asked the peer before choosing a representation, and **for `native` the
measurement is a refusal by all four routes omniORB has** — `-b dump` accepts
the declaration, `-bcxx` exits 1 on it, `-bpython` ignores it and leaves a
`typeMapping` entry that raises `KeyError` one import later, and the ORB has no
`create_native_tc` at all. So `TypeCode::Native` carries no `TCKind`. For
`ValueBase` the measurement is bytes: `tk_value`, **VM_NONE** — not
VM_ABSTRACT, which is the field a reasoned answer gets wrong.

*피어에게 먼저 물었고, `native`의 측정은 네 경로 모두에서의 거부였다. 추론으로
답했다면 VM_ABSTRACT라고 썼을 자리가 VM_NONE이다.*

## 3. Three ways a gate was green while measuring nothing / 게이트가 아무것도 재지 않은 채 초록이던 세 가지

All three were found by **running** a control, none by reading.

1. **The AnyJSON native arm.** Its first control removed the arm and came back
   green: the property's JSON leg carries *values*, not TypeCodes, so the arm
   was unmeasured. It became load-bearing only once a native joined the test
   that asserts a deferred type's description crosses. A change whose control
   is green is not landed.
2. **The S4 rule's `fix()`.** W5 replaced it with the exact falsehood — *"wait
   for §4.4 to land natives…"* — and the pin passed, because the string
   contains neither "yet" nor the deferral claim it grepped for. **Two
   substrings are not a rule.** Widened to require a negation within 40
   characters of every `§4.4` mention, then red. Written by someone who had
   just read `CLAUDE.md`'s section on exactly this.
3. **The generated Python runtime's `fixed` sentence** was measured by nothing
   at all, found by breaking it and watching the whole `orbweaver-gen` suite
   stay green.

## 4. A rule id that meant two things / 두 가지를 뜻하게 된 규칙 id

The harness group behind `prop/unmeasured` is about the sampler contradicting
its own predicate — *a case produced no value and ran nothing*, the shape that
once let 22 of golden 15's 32 cases count as passing. W1 filed a second fact
under the same id: a sequence whose element cannot be sampled is empty every
time. That one is an honest limit — the type has exactly one value and it *is*
measured — and the day it arrived the group went red for it.

**An honest limit silencing a real inconsistency is the failure mode a shared
id produces, and it produced it.** The limit got its own id
(`prop/empty-by-construction`) and the group stayed exactly as strict. The
alternative — loosening the grep — would have left the group unable to say the
one thing it exists to say.

*정직한 한계에 규칙 id를 따로 주었다. grep을 느슨하게 했다면 그룹은 자기가 말하려던
단 하나를 말할 수 없게 되었을 것이다.*

## 5. Measuring instead of building, twice / 짓는 대신 재기, 두 번

- **D013** (PROPOSED, recommends building nothing). The finding it came from
  said *"omniORB deduplicates by object key… so a real ORB makes even those
  agree"*. Driven: three independently created references over seven calls cost
  **3 requests at the address the object left and 7 at the object**, both reply
  orders — one forward per reference, once — and **omniORB 4.3.4 charges the
  same 3 of 7** with `_is_equivalent` answering true. The premise was refuted by
  the ORB it named. Designing the map anyway found the trap that makes a naive
  one a wrong-string bug: `pool::Key` carries the published codeset, so two
  IORs for one object with different `TAG_CODE_SETS` would let the second
  reference inherit the first's profile.
- **The AnyJSON leg count.** W1 left the corpus at `5824 of 5952` CDR round
  trips crossed, against a group that demands every one. The two missing types
  are sequences whose element cannot be sampled — one value each, the empty
  one, which AnyJSON carries. The repair was to let the leg run rather than to
  widen the pin: **5952 of 5952**, 128 round trips more measured than before.

## 6. What is on a branch and not on main / 브랜치에 있고 main에 없는 것

**W2 — `worktree-agent-a932604947d1e0d74`, commit `9a27659`.** Measured before
deciding, and the measurement corrected the row that commissioned it: two of
`COMPONENTS.md`'s `orbweaver-idl` gap sentences are **false today**, closed by
the `const_type` batch of 2026-08-20 without the gap column being edited with
it. 67 constant shapes and 25 neighbours through `omniidl -b dump`, **26
divergences from three causes**: the lexer chose a Rust type and lost what it
could not hold (5, including `unsigned long long` literals *refused*), a
constant's value was never checked against its type at all (16), and no wide
literal existed (5). The load-bearing one: **`idl-diff` was blind to every
`fixed` constant** — both sides folded to `None`, so a released rate could
change and §5.3 printed "no change".

Landing it needs: the corpus file renumbered (`31-const-values.idl` collides
with `31-native-type.idl`), the COMPONENTS gap cell replaced rather than
appended to, and a harness run. Two divergences are recorded where we now
follow CORBA 3.4 over omniidl, and the `L`-prefixed literals are **unmeasured
against a second front end** — JacORB and TAO are absent on that machine.

*W2는 브랜치에 있다. 착지에 필요한 것은 위 세 가지다.*

## 7. Numbers / 숫자

|  | start of day | end of day |
|---|---:|---:|
| harness groups | 77 | 78 |
| golden / negative corpus | 33 / 19 | 35 / 19 (37 / 23 with W2) |
| declarations the wire cannot carry, over golden | 20 (§4.4) | 30 (§4.4 and natives) |
| CDR round trips also crossed AnyJSON | 5248 of 5248 | 5952 of 5952 |
| decisions | 12 | 13 (**all four of D010–D013 PROPOSED, none approved**) |

What the numbers do not say: `orbweaver-test/src/prop.rs` still hand-writes
eight refusal sentences of its own, the generated Python runtime refuses a
`fixed` and a `native` at the *type form* while Rust refuses only the value —
an asymmetry against D008 with one cause for both families — and no decision
written in the last three days has been approved.

*숫자가 말하지 않는 것: `prop.rs`에는 아직 자기 문장 여덟 개가 있고, 파이썬 런타임과
러스트는 D008에 대해 서로 다른 지점에서 거부하며, 최근 사흘간 쓰인 결정 중 승인된
것은 없다.*
