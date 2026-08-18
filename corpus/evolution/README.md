# `corpus/evolution/` — two revisions that differ only in a shared header

Driven by `crates/orbweaver-registry/tests/include_resolution.rs`.

`v1/ledger.idl` and `v2/ledger.idl` are **byte-identical**. Everything that
changed between the two revisions changed in `common.idl`, which is where a real
estate keeps the types more than one contract shares:

| change | why it breaks a deployed peer |
| --- | --- |
| `Ledger::Posting::amount_minor` goes from `long` to `string` | the receiver reads the old type's byte count and alignment |
| `Ledger::Recorded::restamp` is removed | `Journal` inherits `Recorded`, so a caller that invokes it gets `BAD_OPERATION` |

## What this measured

`idl-diff` is the §5.3 release gate. It read its two files with
`orbweaver_idl::parse` — the *string* entry point, which by its own
documentation cannot resolve a relative `#include`. Pointed at this pair on
2026-08-18 it printed:

```text
no change between corpus/evolution/v1/ledger.idl and corpus/evolution/v2/ledger.idl

accepted: nothing here breaks a deployed peer
```

Exit 0. Two breaking changes, waved through by the gate that exists to catch
them. The same call was in eleven other binaries; this directory is the case
where being wrong about it is worst, which is why it is the case that gates.

After resolution the same command reports both changes and exits 1.

## Why it could not have been caught here before

Every other corpus directory holds single self-contained files, so no corpus
case could exercise a cross-file reference at all. `corpus/include/` covers
*resolution* — quoted and angled forms, guards, cycles, prefix scope. This
directory covers what a **consumer of a resolved unit** does with it, which is a
different question and the one the release gate asks.

---

# `corpus/evolution/` — 공유 헤더에서만 다른 두 리비전

`crates/orbweaver-registry/tests/include_resolution.rs` 가 구동한다.

`v1/ledger.idl` 과 `v2/ledger.idl` 은 **바이트까지 동일하다**. 변경은 전부
`common.idl` 에 있다 — 여러 계약이 공유하는 타입이 실제 자산에서 사는 곳이다.

| 변경 | 배포된 피어가 깨지는 이유 |
| --- | --- |
| `Ledger::Posting::amount_minor` 가 `long` → `string` | 수신자가 옛 타입의 바이트 수와 정렬로 읽는다 |
| `Ledger::Recorded::restamp` 제거 | `Journal` 이 `Recorded` 를 상속하므로 호출자는 `BAD_OPERATION` 을 받는다 |

## 무엇을 측정했나

`idl-diff` 는 §5.3 릴리스 게이트다. 두 파일을 `orbweaver_idl::parse` — 상대
`#include` 를 해석할 수 없다고 스스로 문서화한 **문자열** 진입점 — 으로 읽었다.
2026-08-18 이 쌍에 대해 "변경 없음"을 출력하고 0으로 종료했다. 파괴적 변경 두
건을, 그것을 잡으라고 존재하는 게이트가 통과시킨 것이다. 같은 호출이 다른 열한
개 바이너리에 있었다. 이 디렉터리는 그중 틀렸을 때 가장 비싼 경우이므로 게이트가
된다.

해석 이후 같은 명령은 두 변경을 모두 보고하고 1로 종료한다.

## 왜 여기서 미리 잡을 수 없었나

다른 코퍼스 디렉터리는 전부 자기완결적 단일 파일이라 교차 파일 참조를 시험할 수
없었다. `corpus/include/` 는 *해석* 자체를 다룬다. 이 디렉터리는 **해석된
번역 단위의 소비자**가 그것으로 무엇을 하는가를 다루며, 릴리스 게이트가 묻는
질문이 바로 그것이다.
