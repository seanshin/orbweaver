# Phase 4 / Stream B — static generation

Stream B of `docs/PLAN.md` §7.3, in five batches. The batch unit the plan
names: one backend target across the **whole golden corpus** at once — generate
every stub, compile every stub, run against the fixture — with §8's oracle,
*static result equals dynamic result*.

Batches 1–4 measure the calling half (stubs, I1's guard boundary, promotion and
I4's live gate). Batch 5 measures the answering half (skeletons, servant
faults, and §8's oracle in the serving direction), and is where the transposed
completion status was found — by an ORB we did not write, because no local
comparison could have seen it.

계획 §7.3의 스트림 B, 다섯 배치. 배치 1–4는 부르는 쪽(스텁·가드 경계·승격), 배치
5는 답하는 쪽(스켈레톤·서번트 예외·서빙 방향 오라클)을 측정한다. 완료 상태 전치가
발견된 곳이 배치 5이며, 우리가 쓰지 않은 ORB만이 그것을 볼 수 있었다.

---

# Batch 1: the Rust backend

```
static generation — stubs from the registry, oracle: static equals dynamic
  ok   77 item(s) generated from the golden corpus plus the fixture
  ok   every generated stub compiles outside the workspace
  ok   static bytes equal dynamic bytes: Ragged, wstring, any, sequence, both orders
  ok   the generated stub calls omniORB: 10/10 cases, both byte orders
```

## The dynamic path is the reference implementation

Generation starts only now because there had to be something trustworthy to be
equal to. The dynamic path is the one verified against two independent ORBs, so
a generated stub is correct exactly when it produces the same bytes for the
same values — and that is what the oracle compares, byte for byte, in both byte
orders, before any call is made.

## A generated file contains names, never rules

Every marshalling decision in a stub is a call into `orbweaver_gen::rt`, so the
wire knowledge exists once. Phase 3 measured what duplicating it costs (the
`wstring` BOM failure came from re-implementing instead of reusing), and a code
generator is a machine for duplicating things — the discipline has to be
structural. `rt` reuses `WideCodec` for wide text and the dynamic `Value` path
for `any`, because an `any` is dynamic by definition and a static mirror of it
would be a second implementation of the same rules.

생성된 파일에는 **이름과 순서**만 있고 규칙은 없다. 모든 마샬링 결정은 `rt` 호출이며,
와이어 지식은 한 곳에만 존재한다. Phase 3이 중복의 값을 측정했고, 코드 생성기는
중복을 만드는 기계이므로 규율은 구조여야 한다.

## The oracle found a registry bug on its first run

The generator's union test produced a Rust enum with **two variants named `s`**
— impossible code. The cause was upstream: the registry expands
`case 2: case 3:` into two cases but computed `default_index` against the
**unexpanded** AST list, so any multi-label branch before the default shifted
it onto the wrong case.

Every consumer inherited that: the dynamic invoker selects default branches
from this index, and the TypeCode we encode for peers carries it. It survived
because the existing test **asserted the buggy semantics** — `default_index ==
1, "index of the default *branch*"` — which is how a wrong implementation
outlives its own test suite. The test now asserts the expanded index and says
why, and a regression test pins the shifted case.

오라클이 첫 실행에서 레지스트리 버그를 찾았다. 다중 라벨 전개 후 `default_index`가
전개 **전** 목록 기준으로 계산되어 잘못된 case를 가리켰고, 동적 호출기의 default
분기 선택과 피어에게 보내는 TypeCode가 모두 물려받았다. 기존 테스트가 **버그의
의미론을 그대로 단언**하고 있었기에 살아남았다 — 틀린 구현이 자기 테스트를 통과하는
전형적 경로다.

## Skips cascade, with the reason attached

A struct whose member is a `fixed` typedef must not generate: it would compile
against a type that was never written, moving the failure to the consumer's
compiler with the §4.4 reason lost on the way. Representability is checked
transitively, so `Amount` → `Invoice` → `Billing` all skip, each naming
`fixed<9,2>` and the plan section. Constants are a named non-goal of this batch
(the registry records the type, not the value) and are reported the same way.

## What the generated crate proves by existing

`gen-corpus` writes a crate deliberately **outside** the workspace, with path
dependencies only on the published crates. Compiling it proves the stubs stand
on the public surface alone. The oracle binary inside it is a fixed template,
never generated — a test the generator writes for itself proves nothing.

## Scope

Rust only; Python is the next backend batch. User exceptions decode as errors,
not yet as typed values. Bounded strings/sequences are not enforced at the type
level. The wide-character codec is pinned to GIOP 1.2 + UTF-16 (the dynamic
default) rather than taken per-connection. The promotion engine and I1/I4
integration batches (stubs through the guard, identity preserved across
promotion) are separate batches, as §7.4 requires.

---

# Batch 2: integration point I1 — the same stub, both sides of the boundary

```
  ok   I1: the same stub through the guard — exposure, ai_authz scope and audit bind it
  ok   I1: a refused call never reaches the wire; the audit holds nothing dialable
```

## The bypass a generator would otherwise compile in

A generated stub calls `invoke("op", …)` directly. Hard-wired to `Connection`,
it can only ever run *around* the guard — past the exposure list, the scope
check, `destructive` approval and the audit log. That is §4.7's bypass
recreated in compiled form, and it would ship as a build artifact.

The fix is in the type, not in review discipline: stubs are generic over
`Invoker`, and **which side of the trust boundary a stub runs on is decided by
what it is handed, not by how it was generated.** Inside the boundary, hand it
a raw `Connection` (§4.7 explicitly keeps that path). At the boundary,
`Bridge::connect_static(handle, …)` resolves the capability handle, dials, and
returns `Guarded` — the address never reaches the caller.

우회는 리뷰 규율이 아니라 **타입**으로 막는다. 스텁은 `Invoker` 제네릭이고, 어느
신뢰 경계 쪽에서 도는지는 생성 방식이 아니라 **무엇을 손에 쥐여주는가**가 결정한다.

## The same checks, per operation, before anything is sent

`Guarded` runs exactly the checks the dynamic path runs — exposure, `ai_authz`
scopes against the caller, `destructive` approval — at call time, because the
operation name is right there in the `invoke` signature. A refusal is CORBA
`NO_PERMISSION`, the answer a native guard would give, so stub callers handle
policy the way they already handle the target's own refusals; the *why* goes to
the audit log, where §4.8 wants it.

Two details the tests pin because they are where this quietly goes wrong:

- **A refused call never reaches the transport.** Refusing after sending would
  be logging, not guarding. Proven with a recording fake invoker: the transport
  saw nothing.
- **Oneways are gated like everything else.** A oneway that skipped the gate
  would make fire-and-forget the way around the guard.

Live, against omniORB: `blob_sum` now carries `//@ ai_authz: echo:blob` in the
fixture contract, and the identical generated stub answers alice (who holds the
scope) and refuses bob (who does not) — the C×B seam, on the wire. The guard's
audit log is then searched for the host, the object key and `IOR:`, the same
transcript-leak rule the MCP session enforces.

같은 검사를 **연산 단위로, 전송 전에** 적용한다. 거부된 호출이 전송 후에 기록만
된다면 그것은 가드가 아니라 로깅이다 — 기록 전용 가짜 invoker로 전송량이 0임을
증명했다. oneway도 동일하게 게이트를 지난다: 건너뛰면 fire-and-forget이 우회로가
된다.

## Why `Guarded` owns its context

It clones the exposure, caller and approval rather than borrowing the bridge —
partly so holding a stub does not freeze the session, but mostly so the
confused-deputy pairing (one session's connection under another session's
policy, R13) cannot be assembled. The only constructor is
`Bridge::connect_static`, and the interface id comes from the capability table,
never from the stub: a stub asserting its own interface id would be asserting
its own permissions.

---

# Batch 3: the promotion engine, and I4's regression gate

Stream B's third batch, produced by a parallel worktree agent and landed
through the serial merge gate. **383 workspace tests.**

## Promotion is a recommendation, and the gate is the decision

`CallStats` counts per-(interface, operation) outcomes at the bridge — recorded
against what the capability handle names, never what the caller asserted, and
only for calls that passed policy, because a refused call says nothing about
how hot an operation is. `PromotionPolicy::recommend()` is count-based with no
clock on purpose: a recommendation that depends on wall time cannot be
reproduced, and a gate that cannot be reproduced cannot be tested.

## `IdentityDropped` is checked before the results are compared

`verify_promotion()` refuses a promotion in this order: malformed audit
(unmeasured is never a pass), **identity dropped**, operation mismatch, result
mismatch. The order is the point — a promotion that keeps the answer
byte-for-byte and loses the caller is §4.8's confused deputy returning through
an optimization, and it must not be reachable by having correct results. The
gate parses the guard's existing audit format rather than inventing one: two
formats for one fact eventually disagree.

승격 검증은 **결과 비교보다 신원 비교가 먼저**다. 답을 그대로 지키면서 호출자를
잃는 승격이 §4.8의 혼동된 대리인이 최적화를 타고 돌아오는 경로이며, 결과가 정확하다는
이유로 그 경로가 열려서는 안 된다.

## Scope

Verified at gate level with fakes (the Recorder pattern from guard.rs). The
live half of I4 — running a recommended promotion's static stub and dynamic
path against a real peer and feeding both audits through this gate — belongs
to the gen-corpus oracle and is not yet wired; the module says so.

---

# Batch 4: I4's live half — the gate meets a real peer

```
── promotion respects identity (I4) ──
  ok   I4: a live promotion passes the gate when identity is preserved
  ok   I4: the gate refuses a promotion that lost the caller, results identical
  ok   I4: after 3 recorded live calls the policy recommends (IDL:spike/Echo:1.0, add)
```

The same `add(40, 2)` runs down both paths against a live omniORB: the dynamic
invoker's real `Outcome`, and the generated stub through
`Bridge::connect_static` on behalf of alice, its audit line **captured** from
the real `Guarded`. Both feed `promote::verify_promotion`.

The negative control is what makes it a check rather than a demonstration: the
same static call rebuilt without a caller still receives 42 from the peer —
the wire is happy — and the gate refuses with `IdentityDropped`. An identical
answer with a missing caller is precisely what the gate exists to make
unshippable.

One seam stays named: the dynamic bridge path emits no audit lines yet, so its
line is *reconstructed* from `Bridge::caller` session state in the guard's
exact format. Capture replaces reconstruction when Bridge emits real lines —
which is a running batch as this is written. PLAN §7.4 I4: ●.

음성 대조군이 시연을 검사로 만든다: 호출자 없이 재구성한 같은 호출이 피어에게서
똑같이 42를 받는데도 게이트가 `IdentityDropped`로 거부한다. 답이 같고 호출자만
사라진 승격이야말로 게이트가 출하 불가능하게 만들어야 하는 것이다.

---

# Batch 5: the serving direction — skeletons, faults, and an oracle for the answer

Batches 1–4 measured generated **clients**. Everything a generated *server*
does was unmeasured, which mattered more than it sounds: every CORBA service
this project serves — naming, event, IFR, expert, tenancy — is a hand-written
servant, so "no hand-written stubs" was true of the calling half and merely
unexamined on the answering half.

배치 1–4는 생성된 **클라이언트**만 측정했다. 우리가 제공하는 CORBA 서비스는 전부
손으로 쓴 서번트였으므로, "손으로 쓴 스텁 없이"는 부르는 쪽에서만 참이었다.

## Three things a skeleton gets wrong, tested as three things

- **oneway** — the arm writes nothing at all. A skeleton that writes an empty
  reply to a oneway breaks the message framing for every later request on that
  connection, so the test sends a twoway *after* the oneway across all three
  GIOP versions and both byte orders and checks the answer, rather than
  checking the absence.
- **attributes** — `_get_x`/`_set_x` are operations on the wire; a readonly
  attribute generates no setter and refuses `_set_x`. Getting this wrong was
  live: attributes were not inherited by the client stub either, so a skeleton
  would have answered an inherited `_get_` with `BAD_OPERATION`.
- **alignment origin** — and here the honest finding is that the hazard is
  *latent* on the reply side: `Server` always hands over origin 24, which is
  already 8-aligned, so a zero-origin bug is invisible there. The test also
  dispatches at origin 20. On the request side it needs no contrivance, since
  GIOP 1.0/1.1 do not align the request body.

## A servant that cannot fail is not a servant

The first skeleton design gave the trait an error type of "the user exceptions
this interface declares", which for an interface with no `raises` clause is
**uninhabited**: such a servant could not fail at all. Every hand-written
servant we have needs `OBJECT_NOT_EXIST` for an unknown key, `NO_PERMISSION`
for a refusal, `TRANSIENT` for a temporary one — so a generated servant that
cannot express them can never replace one.

The fix is one enum rather than two channels, because the reply status is
exactly what must not be decided in two places. The part worth keeping is how
the completion status is obtained: `rt::raise::*` returns a `#[must_use]
Raising` with no `From`, no `Default`, and no method that yields a
`SystemException` without naming the status — `did_not_run()`,
`ran_to_completion()`, `may_have_run()`. A generator-picked default here is how
a retry loop corrupts state, and the negative control is a test where one
servant answers `COMPLETED_NO` for one raise and `COMPLETED_MAYBE` for another.

**서번트가 실패할 수 없으면 서번트가 아니다.** 완료 상태는 생성기가 고르는 상수가
아니라 서번트가 이름 붙여야 하는 값이다 — 여기서 기본값을 조용히 넣는 것이 재시도
루프가 상태를 망가뜨리는 경로다.

## The finding: a transposed enum that only a foreign ORB could see

Driving the generated servant with omniORB's own python client is what caught
it. `§4.11.4` declares `enum completion_status { COMPLETED_YES, COMPLETED_NO,
COMPLETED_MAYBE }`, so YES is ordinal 0; `orbweaver_giop::server::Completion`
had `No = 0, Yes = 1`. **A servant reporting "it did not run" reached every
foreign ORB as "it ran"** — a call refused before it started looked like a
mutation that had happened, and a client that could safely have re-sent
concluded it must not.

`MAYBE` is 2 either way, which is why only two of the three were wrong. Nothing
local caught it because every local comparison used the same enum on both
sides — including `giop`'s own test, which asserted the encoded byte equalled
`Completion::No as u32` and therefore moved with the bug. It now asserts the
literal ordinal, and the harness reads the value back through omniORB on every
run.

The batch that found it did **not** fix it: the defect was in another crate and
outside the stated footprint, so it was pinned *as measured* with a comment
telling the fixer what to change. That is the discipline working — a batch that
reaches outside its footprint to fix what it finds also lands unreviewed
changes to five servants' wire output.

**로컬 검사는 전부 같은 enum을 양쪽에 놓고 비교했으므로 버그와 함께 움직였다.**
우리가 쓰지 않은 ORB만이 이견을 낼 수 있었다.

## §8's rule in the direction nothing checked

The client oracle is "static bytes equal dynamic bytes". The serving direction
had no equivalent, so a skeleton could encode a reply correctly by accident.
`tests/skeleton_oracle.rs` drives each operation's reply through the generated
skeleton and compares the bytes against `orbweaver_dynamic::encode` of the same
values — 204 comparisons over three GIOP versions × two byte orders × two reply
origins, including user-exception bodies.

What it cannot compare is **named rather than skipped**, and a test fails if a
contract grows a member on neither list: oneway operations have no reply on
either side; a `SystemException` body is written by `giop` rather than by the
skeleton, so there is no generated encoding to hold to a dynamic one; and the
wide codec is pinned on both sides, so a `wstring` would compare equal where
the paths should differ — asserted absent rather than assumed.

## Scope / 범위

Landed: client stubs, server skeletons, user and system exceptions, the
promotion gate with I4's live half, and both directions of §8's oracle.

Not landed, and stated so the absence does not read as completeness: a
generated skeleton has **no object keys**, so one servant per process — the
naming server's multi-context shape is not yet generatable, which is the gate
on replacing our hand-written servants with generated ones. No
`LOCATION_FORWARD`. A oneway fault is dropped, since §9.4.1 leaves nowhere to
put it, though it is now logged rather than discarded silently. Python and
other target languages remain unwritten.
