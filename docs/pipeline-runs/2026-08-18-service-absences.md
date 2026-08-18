# 2026-08-18 — a decision and an oversight stop giving the same answer

> Batch 4, and the one originally agreed as batch 3. Closes what
> [`SERVICES-COVERAGE.md`](../SERVICES-COVERAGE.md) measured on 2026-08-14:
> twelve of 107 declared operations answering `BAD_OPERATION` with no reason
> written anywhere.

## 1. The re-measurement came first

Most of the twelve had been closed since. `to_url` and `_get_version` are
served; the IFR's deferrals already answer `NO_IMPLEMENT`; `Router::select`
dispatches; D006 recorded why `Expert::process` and `Router::dispatch` are
excluded. Re-running the sweep before writing anything is what kept this batch
from repairing work that no longer needed it — the same mistake `PLAN` §7.2's
stale "Remaining:" list produced two batches ago.

## 2. The root cause was never the twelve

`SERVICES-COVERAGE.md` states it in its own §1: *the wire could not tell a
considered refusal from an oversight*. Both said `BAD_OPERATION` — "no such
operation" — so the only thing separating a decision from a gap was whether
somebody had written a sentence in a document the client cannot read.

`orbweaver-registry`'s IFR facade had already solved this, in one servant, with
one line: a deferred operation answers **`NO_IMPLEMENT`**. Three answers, three
facts, no document required:

| answer | means |
|---|---|
| `NO_PERMISSION` | the operation exists and the answer is no, as policy |
| `NO_IMPLEMENT` | declared, and this servant does not implement it, on purpose |
| `BAD_OPERATION` | this interface does not declare that name at all |

The fix is that rule applied everywhere, not twelve sentences.

## 3. What moved

CosNaming's `bind_context`, `rebind_context`, `destroy`; the event channel's
whole pull model and its `destroy`; `moe::Router::dispatch`. Each already had
its reason written in its own module header — the reasons were never the
problem, their invisibility was.

**`moe::Router::dispatch` is the finding.** D006 approved excluding it on
2026-08-14 and the servant went on answering "no such operation" for four days.
A decision recorded in prose and contradicted on the wire is precisely the
class `PLAN-SERVICES.md` §8.1 was written to name, committed inside the section
that names it. Nothing could have caught it except a gate that reads the wire.

## 4. The gate, and the two versions of it

The sweep now **decides** instead of counting: a `BAD_OPERATION` from an object
that *claims* the interface is a servant half-serving something it says it is,
and it fails the harness. An interface no object claims is reported as its own
fact rather than as N missing operations.

The first version asked the object `_is_a` with a repository id built from the
scoped name — `moe::Expert` → `IDL:moe/Expert:1.0`. That is wrong for every COS
interface, whose `#pragma prefix` makes it `IDL:omg.org/CosNaming/…`, so every
claim came back false and **the gate passed a deliberately broken servant**.
The negative control caught it, not review. The second version reads the claim
out of rows already measured: an object that answers any operation of an
interface claims it. No extra round trips, no id to guess.

**Negative controls, both run:** reverting `destroy` to `BAD_OPERATION`
produces an `ABSENT` row and a red harness; the version-one gate did not, which
is how version one was found.

## 5. A measurement defect, in the direction that flatters

`NO_IMPLEMENT` was being counted as *dispatched*, so the IFR facade's served
count read **28 when it is 14**. A count that is wrong in the flattering
direction is the one worth re-deriving, and this one had been reported twice.

## 6. Measurements

| Service | probes | served | `NO_PERMISSION` | `NO_IMPLEMENT` | `BAD_OPERATION` |
|---|---:|---:|---:|---:|---:|
| CosNaming | 16 | 13 | 0 | 3 | **0** |
| CosEvent | 28 | 19 | 0 | 9 | **0** |
| Interface Repository | 66 | 14 | 38 | 14 | **0** |
| MoE enterprise | 28 | 28 | 0 | 0 | **0** |
| MoE control plane | 19 | 12 | 0 | 1 | **6** |

**Absences: 12 → 0.** The six remaining `BAD_OPERATION`s are correct answers
from objects that never claimed `moe::Expert`, now reported as an unserved
interface with the reason written in `PLAN-SERVICES.md` §8.1.1 — the sentence
`SERVICES-COVERAGE.md` observed was "nowhere written".

Workspace 1211 tests, `cargo fmt` clean, clippy 0.

## 7. 한국어 요약 / Korean summary

**재측정을 먼저 했다.** 12건 중 대부분은 이미 닫혀 있었다. 쓰기 전에 스윕을 다시
돌린 것이, 두 배치 전 `PLAN` §7.2의 낡은 목록이 만든 실수를 반복하지 않게 했다.

**근본원인은 12건이 아니었다.** 와이어가 숙고된 거부와 누락을 구분하지 못했다 —
둘 다 `BAD_OPERATION`("그런 연산 없음")이었으므로, 결정과 공백을 가르는 것은
클라이언트가 읽을 수 없는 문서에 누가 문장을 썼는지뿐이었다. IFR 파사드는 이미
`NO_IMPLEMENT`로 풀어 두었고, 그 규칙을 다섯 서비스 전부에 적용한 것이 이 배치다.

**핵심 발견: `moe::Router::dispatch`.** D006이 2026-08-14에 제외를 승인했는데
서번트는 나흘간 "그런 연산 없음"이라 답하고 있었다. 산문의 결정을 와이어가
부정한 것 — §8.1이 이름 붙이려던 실패를 §8.1 자신이 저질렀다.

**게이트의 두 판본.** 1판은 스코프 이름으로 저장소 id를 지어내 `_is_a`를 물었고,
`#pragma prefix` 때문에 COS 인터페이스마다 틀려서 **일부러 망가뜨린 서번트를
통과시켰다.** 음성 대조군이 잡았지 검토가 잡지 않았다. 2판은 이미 측정된 행에서
자처 여부를 읽는다 — 추가 왕복도 추측도 없다.

**유리한 쪽으로 틀린 계수.** `NO_IMPLEMENT`를 서빙으로 세는 바람에 IFR의 서빙
수치가 **28로 보였고 실제는 14**였다. 두 번 보고된 숫자였다.

**부재 12 → 0.**
