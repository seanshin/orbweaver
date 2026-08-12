# Phase 3 — dynamic invocation, the AI pipeline and the MCP bridge

`docs/PLAN.md` calls this the headline phase: an agent describes what it wants
and a call happens, with nothing generated in between. Phases 0–2 built what
that rests on — the wire, the IDL front end, the type registry, object
references, and a differ that stops a contract change corrupting data silently.

Phase 0–2가 만든 것 위에서, 생성물 없이 호출이 일어나게 하는 단계다.

---

# Batch 1: value marshalling and dynamic invocation

```
dynamic invocation — calls built from IDL text alone
  ok   omniORB answered 8 dynamically built calls, both byte orders
  ok   wrong arguments are refused locally, before anything is sent
  ok   a refused call leaves the connection usable
  ok   JacORB answered them too — a second, independent decoder
```

Every call in that group is built at runtime from IDL text. The registry says
what `add` looks like, `orbweaver_dynamic::encode` turns the arguments into
bytes, and an ORB we did not write answers. No stub, no generated type, no
operation name known at compile time.

## Why this had to come first

Everything before this marshalled by hand. That works when the types are known
when the code is written, and it is exactly what `invoke_operation` cannot do:
it receives an operation name and a bag of values chosen at runtime, and has
only the registry's description to work from.

The gap was visible in Phase 2. Proving that swapping two struct members
corrupts data on the wire meant hand-writing two `put_i32` calls in a
particular order, because nothing could take a type and a value and produce
bytes. That is the hole this batch fills.

지금까지는 전부 손으로 마샬링했다. 컴파일 시점에 타입을 아는 경우에만 되는 방식이고,
`invoke_operation`이 할 수 없는 바로 그것이다.

## Diagnostics are the product here

A caller that is guessing gets nothing from "marshalling failed". Four mistakes
an agent will actually make are named:

| Mistake | What it says |
| --- | --- |
| An argument left out | `add takes (a, b); missing b` |
| A value of the wrong type, nested | `at lines[1].qty: expected a value of type long, got a string` |
| An `out` parameter passed in | `split takes (p); x, y are an out parameter and is not passed in` |
| A wrong operation name | `has no operation "Add"; did you mean "add"?` |

The last one is only unambiguous because IDL forbids two names differing in
case — the rule that has tripped this project up five times is, for once, the
thing that makes a suggestion safe.

All four are refused **before anything reaches the connection**, so a bad call
is a local error rather than a half-written message and a poisoned connection.
The spike checks that too: after three refusals the connection still answers.

## Two refusals that are about the wire, not about typos

- **Members supplied out of order are refused, not encoded.** CDR is
  positional, so a caller who gets the order wrong would otherwise produce a
  message that decodes cleanly into the wrong fields — the silent corruption
  §5.3 measured against omniORB. This is the last point at which it can still
  be caught.
- **A declared sequence length is validated against the bytes actually
  present.** Twelve bytes buying a multi-gigabyte allocation was the worst
  finding of the Phase 0 spec audit, and a new marshaller is exactly how it
  would come back.

An unknown enumerator gets its own message: *the sender may be built against a
newer contract*. That is §5.3's conditionally-breaking verdict arriving in
person rather than as a generic CDR error.

## The failure: knowledge the project had already paid for

The spike's first run was 14 of 16, and both failures were one cause. Against a
big-endian client omniORB returned the text with a leading U+FEFF; against a
little-endian one it raised `UNKNOWN`.

`wstring` is the one part of CDR whose encoding is not determined by the
`TypeCode`. It depends on the GIOP version and the negotiated wchar codeset,
and Phase 1 established the hard way that **peers do not infer wide-character
byte order from the message byte order** — a BOM is what settles it.
`WideCodec` already encoded all of that, and this module re-implemented
`wstring` from scratch instead of using it.

Routing wide types through the codec fixed both, and immediately exposed a
second bug of the same kind: the `wchar` *decoder* was still reading a bare
`u16` while the encoder had started writing GIOP 1.2's octet-count prefix.

The codec is now taken from the connection rather than from a constant.
Defaulting to 1.2 would have put a length in octets on a 1.1 connection that
counts characters — the same latent bug, one version away.

**Duplicating knowledge the project has already paid for is how a fixed cause
comes back.** Nothing in the rules covered it, because it is not a rule about
CDR; it is a rule about reaching for the module that already knows.

첫 실행은 16건 중 14건. 실패 2건은 원인 하나였다 — `WideCodec`을 쓰지 않고 wide
문자열을 다시 구현한 것. Phase 1이 어렵게 알아낸 사실(피어는 메시지 바이트 순서로
wide 문자 순서를 추론하지 **않는다**)을 우회한 셈이다. 고치자마자 같은 부류의 두 번째
버그가 드러났고, 코덱을 상수가 아니라 연결에서 가져오도록 바꿔 세 번째를 예방했다.
**이미 값을 치르고 얻은 지식을 중복 구현하는 것이, 고친 원인이 돌아오는 경로다.**

## What `Connection` gained

`endian()`, and `invoke_oneway()` as a separate method rather than a flag. The
two differ in what a caller may conclude: a oneway carries no reply, so there
is nothing to correlate and no `LOCATION_FORWARD` to follow — §9.4.3.2's
redirect needs a reply to travel in, which is why `Server` refuses to forward
one either. A successful return means the bytes were written, and nothing more.

## Scope

`valuetype` and `fixed` are still absent, as §4.4 defers them. The invoker is
deliberately not a general DII: no `Request` object to populate field by field
and no deferred synchronous mode, because neither serves the agent path and
both are surface to get wrong.

Still to come in this phase: AnyJSON (§4.5) converting JSON into these values,
and the MCP bridge (§4.6) — which must land together with capability handles
(Phase 3.5), not after.
