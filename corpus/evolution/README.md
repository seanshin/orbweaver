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

---

## `moe/` — the §5.3 pair for corpus/golden/22 (moe v1.0 → v1.1)

`moe/v1.0/moe.idl` is the **frozen** release of the control-plane contract;
`corpus/golden/22-moe-control-plane.idl` is the additive v1.1 revision
(`idl-diff` exit 0); `moe/v1.1-in-place/moe.idl` is the same two members added
to the released `Capability` in place, kept as the negative control (exit 1).
Neither file under `moe/` is served by anything and neither is edited — the
measurement, and why the version bump was paid, is PLAN-MOE §4.5.1.

`moe/v1.0/moe.idl`은 **동결된** 릴리스, golden 22가 추가만 한 v1.1 (exit 0),
`moe/v1.1-in-place`는 제자리 수정 음성 대조군 (exit 1). 둘 다 서비스되지 않고
수정되지 않는다. 측정과 이유는 PLAN-MOE §4.5.1.

---

## `union-default/` — the §5.3 pair for a union's default member

Driven by `crates/orbweaver-registry/tests/union_default_pair.rs`.

`union-default/v1.0/payload.idl` is the **frozen** release: one union with a
labelled case and a `default:` member. `v1.0-default-first/` is the same union
with the default written first — a different member list and `default_index`,
the same encoding of every value, so `idl-diff` must say "no change" (exit 0);
`v1.1-retyped-default/` inserts a case ahead of the default **and** changes the
default's type, and the gate must exit 1 naming both:

| change | verdict | why |
| --- | --- | --- |
| `case 2: long extra` inserted | conditionally breaking | an old receiver has no branch for the new discriminator |
| `default: string text` → `default: long text` | **BREAKING** | the receiver reads the old branch's bytes |

Until 2026-08-19 the differ compared union members by position and treated the
default's empty label as a discriminator value. Against this pair it reported
only the inserted case — the release was refused, for half the reason — and a
frozen `TypeCode` of the pre-f8daa21 shape (the default folded onto its label)
against today's expanded one read as "case added" one way and "case removed"
the other, with nothing changed on the wire. Members are now compared by role:
labelled cases by label, the default by `default_index`. The frozen-TypeCode
half is held by the unit tests in `crates/orbweaver-registry/src/diff.rs`; this
directory holds the half a person can produce from IDL. Nothing under
`union-default/` is served and nothing is edited.

`union-default/v1.0`은 **동결된** 릴리스, `v1.0-default-first`는 default만 앞에
쓴 같은 유니언 (exit 0), `v1.1-retyped-default`는 default 앞에 case를 끼워 넣고
default의 타입을 바꾼 음성 대조군 (exit 1, 두 변경 모두 이름으로). 위치로
비교하던 차분기는 끼워 넣은 case만 보고했다. 이제 라벨 있는 case는 라벨로,
default는 `default_index`로 — 역할로 비교한다. 서비스되지 않고 수정되지 않는다.

---

## Approvals — what `idl-diff --approve` writes beside a proposed contract

Every pair here is a negative control: the in-place edit must exit 1. Since
2026-08-19 there is a way to make it exit 0 that is not editing the code, and
this section is about why none of it lives in this directory.

`idl-diff <released> <proposed> --approve <reason> --approver <name>` appends
one row per blocking finding to an **approval store** — `--approvals <file>`,
or by default `<proposed>.approvals.tsv` beside the proposed file — with these
columns:

```text
released  proposed  released_sha256  proposed_sha256  id  verdict  what  reason  approver  approved_at
```

- **`approver` is required** (`--approver`, or `ORBWEAVER_APPROVER`); without
  it the run exits 2 before reading a file. A store with a blank approver or a
  blank reason in any row is refused whole, exit 2. Nothing is signed: this is
  the name that used to be in a chat log, put where `git blame` and the console
  can see it.
- **A row binds to bytes.** The two digests are SHA-256 over every file in each
  translation unit, root first — for a single file, what `shasum -a 256` prints;
  for `v1/ledger.idl`, `cat v1/ledger.idl v1/common.idl | shasum -a 256`. Edit
  either side, including a shared header, and the row is reported as *given for
  a different revision* and the gate refuses again.
- **A re-run reads the store** when it exists: covered findings print
  `[approved by <who>: <reason>] <when>` and do not fail the exit code; a
  finding with no row, or only a stale one, still exits 1. Without `--approve`
  nothing is written.
- **Replay is byte-identical apart from `approved_at`**, because findings come
  out of the differ in a stable order and a finding already covered is not
  written twice; `SOURCE_DATE_EPOCH` pins the timestamp for a harness that
  wants the whole file identical.

The console's `diff` page reads the same store (`--approvals`, or the default)
and renders who, why, when and whether it still applies — as words. It writes
nothing and refuses nothing.

**No store is committed under `corpus/evolution/`**, and
`crates/orbweaver-registry/tests/approval_replay.rs` fails if one appears: a
`moe.idl.approvals.tsv` beside `moe/v1.1-in-place/moe.idl` would turn the
harness's negative control green with no code changed. Approvals for corpus
pairs are written to a scratch directory and thrown away.

`idl-diff --approve <이유> --approver <이름>` 은 이제 출력 한 줄이 아니라
`<proposed>.approvals.tsv` 에 파괴적 판정 하나당 한 행을 기록한다. 승인자는
필수이며(없으면 exit 2), 이름이 빈 행이 하나라도 있으면 저장소 전체를 거부한다.
행은 경로가 아니라 두 번역 단위의 바이트(SHA-256, 포함 헤더까지)에 묶이므로
한 바이트만 고쳐도 다시 거부된다. 재실행은 시각 열을 빼면 바이트까지 같다.
콘솔 `diff` 페이지는 같은 저장소를 읽어 누가·왜·언제를 글로 그리고, 쓰지도
결정하지도 않는다. **이 디렉터리에는 저장소를 커밋하지 않는다** — 음성
대조군 옆의 저장소는 코드 변경 없이 하네스를 초록으로 만든다.
