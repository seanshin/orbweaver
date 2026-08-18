# 2026-08-18 → 19 — the session record

> Two days, 52 non-merge commits, 154 files, +25,373 / −875, 1198 → 1362 tests,
> 70 harness groups, D008 · D009 approved and closed, D010 proposed and revised.
> Sixteen parallel agent batches, every one landed serially through the harness.
> This is the record of what it found, kept because the individual commits say
> *what* and this says *what kept happening*.

## 1. The finding that recurred / 반복된 발견

> **A convention both ends apply cannot be refuted by a round trip.**

It appeared first as a union `TypeCode`'s case labels — stored in whatever byte
order the stream arrived in, so our encoder and decoder agreed with each other
in every order and 1200 tests were green while omniORB's `long long`
discriminated union **could not be decoded at all**. Then `long double`'s
octets, UTF-16 read from the message's order rather than its own, an
encapsulation's alignment origin leaking the enclosing offset, and a stub's
`wstring` written from a GIOP 1.2 constant on every connection. Five defects,
one shape, none visible to any test in the repository, all found by asking a
peer for its bytes.

The lesson was codified as a practice rather than a rule: every wire batch this
session recorded **the peer's bytes with their provenance**, and one test per
capture re-encodes back to them — the direction our own round trip can never
check.

**양쪽에 똑같이 적용되는 관례는 왕복 오라클로 반증되지 않는다.** union 레이블에서
시작해 다섯 결함이 한 모양이었고, 저장소의 어떤 테스트도 볼 수 없었으며, 전부
피어에게 바이트를 물어서 나왔다.

## 2. Gates that were green and measured nothing / 초록이면서 아무것도 재지 않던 게이트

Four, all found by a **negative control** and none by review:

| Gate | How it was worthless | How it was found |
|---|---|---|
| a `grep` for `why.to_string()` in the audit path | matched its own explanatory comment; missed a real violation | injected the violation, gate stayed green |
| the fuzzer's allocation check | a static model of what a decode *would* reserve — kept reporting after the guard landed, and its replacement `is_ok()` could never fire | removed the guard, gate stayed green |
| two CSIv2 fuzz targets | reached **0 times in 50,000 cases** | counted the reach |
| the generated-vs-hand-written naming comparison | `destroy` moved to the top of the script and destroyed the root for both halves — **agreement by mutual destruction**, value-carrying replies 25 → 5 | the vacuity test that pins the class counts |

The fourth is the one worth remembering: a byte comparison between two
implementations passes when both are broken identically, and the only defence
is a second test that asserts the comparison *reached* something.

## 3. Gaps that were already closed / 이미 닫혀 있던 공백

Five rows in `COMPONENTS.md` or `PLAN` said work was outstanding that had
landed: `knows()`/object keys, `LOCATION_FORWARD`, `SEAT_SAFETY_CONTENT`, F5
LifeCycle/Property, and `#include` inside a module. One of them cost a whole
planning pass proposing finished work. **Progress was wrong in both
directions** — §2 overstated it, this understated it — which is why D010 was
written from the gap columns and then checked row by row against the code
before being proposed.

Codified: `records_keep_up.py` fails when `CHANGELOG.md` or `COMPONENTS.md`
goes more than ten commits without being opened (they had gone thirty-nine).
The precise half — *is the row still true* — was measured as a gate candidate
and **demoted to a report**: 11 of 17 gap-column symbols exist in their crate,
nearly all legitimately.

## 4. Deferrals that were descriptions / 서술이었던 유예

Three service deferrals were re-examined and each reason turned out to be
**two claims, only some still true**. *"Contexts live as long as the process"*
was true because nothing removed a key. *"The same unbounded buffer this module
avoids"* was false because the pull proxy uses the same bounded deque. What
survived: chaining to a foreign context (implementable now, and that is a
reason it is *possible*), the supplier side of pull (a thread per supplier on
someone else's clock), `destroy` on the event channel (unauthenticated, ends
the channel for everyone). Each rewritten reason names today's facts.

And one deferral was **measured to be correct rather than cautious**: the
empty `char` conversion list. Eleven peer configurations, every one reaches
UTF-8; and offering ISO-8859-1 was measured to make JacORB **move down to it**
and truncate Korean to low octets, raising nothing. BLOCKED, and the answer is
stronger than the condition asked for.

## 5. What was decided / 결정된 것

- **D008** — AnyJSON v1.1: a type describes itself structurally; `_t` keeps its
  v1 name where one fits, becomes an object where v1 said nothing. Additive,
  tested. `ir-subset` 18+10 skipped → 28+0, and an agent can read an Interface
  Repository.
- **D009** — the transmission codeset reaches the marshaller through an owned
  `Arc<dyn TextCodec>` slot on the stream (`&dyn` was the first draft, and it
  would have changed 145 construction sites; the decision was corrected before
  a line was written). Four batches, all closed; the last BLOCKED on a
  measurement, per its own condition. The `dyn` cost was **measured** at ~31 ns
  per string, 1.06× on a 64-string payload — the benchmark §8 had cited since
  v0.2 and did not have.
- **D010** — proposed and revised: what remains, split by whether an oracle
  exists on this machine. Class B lands only as SKIPPED. Class C is not built
  until triggered, and building it early is the defect.

## 6. What the harness gained / 하네스가 얻은 것

From 44 groups to 70. Among them: the release profile **run** rather than only
built (six tests asserted a debug panic, so `cargo test --release` could not be
run clean, so nobody ran it, so release-only defects had no test pass — the
overflow class survived that way); an overflow-checked fuzz run, the only gate
in the tree that can see the arithmetic class; the §5.3 gate run over the
whole corpus with two negative controls (nobody had ever run `idl-diff` over
the corpus, which is how it refused a valid contract unnoticed); every peer
capture re-taken from the live fixture and compared to the recording.

## 7. Two process rules, both applied to the person who wrote them / 두 규칙, 둘 다 쓴 사람에게 적용됨

- **A record lands with its batch.** Three service batches landed without
  their CHANGELOG entry because a scripted edit's anchor matched four times and
  the assertion fired after the commit was staged. `records_keep_up.py` read
  six commits behind, and that is how it was noticed.
- **A new harness group lands with its negative control in the commit
  message.** Every landing this session did it; D010 proposes it as a rule and
  holds itself to it — A1's review correction is the negative control on the
  draft.

## 8. Numbers, and what they do not say / 숫자, 그리고 숫자가 말하지 않는 것

|  | v0.4.0 | now |
|---|---:|---:|
| tests | 1198 | 1362 |
| harness groups | 44 | 70 |
| service operations absent without a reason | 12 | **0** |
| decisions | 7 | 10 |
| peer-recorded wire captures | 0 | 5 files |

The single figure asked for was "about 80 %", given with the caveat that most
of the remainder is **unmeasurable here** rather than unbuilt — no model key, no
identity provider, no docker, no TAO, no peer that cannot reach UTF-8. D010
makes that split the plan's organising principle. And this session's own record
is the reason not to trust the figure much either way: five gaps were already
closed and four gates were green while measuring nothing.

"약 80%"라는 숫자를 냈지만, 남은 것 대부분은 **덜 만든 것이 아니라 여기서 잴 수
없는 것**이다. 그리고 이 세션 자신의 기록이 그 숫자를 양방향으로 의심할 이유다:
공백 다섯이 이미 닫혀 있었고, 게이트 넷이 초록이면서 아무것도 재지 않았다.
