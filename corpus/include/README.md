# `corpus/include/` — the first multi-file corpus case

**English canonical. 한국어 요약은 각 절 아래.**

## Why this directory exists

Every other corpus directory holds **single self-contained files**. That was
never a decision; it was what a corpus made one file at a time looks like. It
had a consequence nobody could see from inside it: `crates/orbweaver-idl`
skipped `#include` along with every other `#` directive, and **no corpus case
could ever exercise a cross-file reference**, so the gap was invisible by
construction. It stayed invisible for six phases.

The thirteen-file estate in `spikes/estate/` found it in one pass: 11 of 13
files rejected by us and accepted by `omniidl`, ~90 diagnostics, all of them
one cause (`docs/pipeline-runs/2026-08-14-estate.md`, RC-1). The estate is a
*consumer* and deliberately not a fixture, so the fixture is here.

> 다른 코퍼스 디렉터리는 전부 **자기완결적인 단일 파일**이다. 그래서
> `#include` 미해석 결함을 코퍼스 규모에서는 **구조적으로** 발견할 수 없었고,
> 여섯 페이즈 동안 발견되지 않았다. 13파일 자산이 한 번에 찾아냈다(13개 중 11개
> 실패, 진단 90건, 원인 1개). 자산은 소비자이고 픽스처가 아니므로, 픽스처는
> 여기에 있다.

## How it is run

The manifest is `cases.tsv`: one row per **root** file, with the verdict we
give it, the diagnostic rule expected, and **what `omniidl` said about the same
file when it was measured on 2026-08-14**. The gate is
`crates/orbweaver-idl/tests/include_corpus.rs`, which runs on `cargo test` and
needs no oracle installed.

Nothing here is swept by a glob. `spikes/run_checks.sh` and
`spikes/differential.sh` enumerate `corpus/golden`, `corpus/negative`,
`corpus/requirements/generated`, `corpus/pragma`, `corpus/services` and
`corpus/annotations` by name, and this directory is in none of them — which is
correct, because half the files here are leaves that only mean something inside
a unit, and validating a leaf on its own would be measuring nothing.

By hand:

```bash
cargo run -q --bin idl-check -- corpus/include/service.idl        # accepts, with advice
cargo run -q --bin idl-check -- -I corpus/include corpus/include/angled.idl
cargo run -q --bin idl-check -- -E corpus/include/types.idl       # the resolved unit
```

> 매니페스트는 `cases.tsv`, 게이트는 `crates/orbweaver-idl/tests/include_corpus.rs`
> 이고 오라클 없이도 `cargo test`에서 돈다. 이 디렉터리는 어떤 glob에도 걸리지
> 않는다 — 절반이 단독으로는 의미가 없는 잎 파일이기 때문이며, 그것이 맞다.

## What each decision is, and why

| decision | what we do | why |
|---|---|---|
| search path | `"x.idl"` tries the including file's directory then `-I`; `<x.idl>` tries `-I` only | the C convention CORBA IDL inherits, and what `omniidl -I` implements. Nothing implicit — the process's working directory is never searched, or the same file would resolve differently depending on where the validator was started, and the difference would surface as a repository id rather than as an error |
| idempotence | a file is spliced **once per unit**, keyed by canonical path; guards are not required | the estate measured that real IDL has no guards. Including an IDL file twice can only duplicate a declaration, never add one. Where the repeated file is unguarded we raise advice, because `omniidl` **rejects** that file and being quietly laxer than a deployed compiler is how a project ships something that fails at the peer |
| cycles | terminate, and name the loop in a non-blocking diagnostic | a guarded cycle is legal IDL that `omniidl` compiles, so an error would reject valid input; silence would leave a real mistake unreported |
| missing file | error, listing **every path searched** | a silent skip is the defect this whole directory exists to remove |
| `#pragma prefix` across an include | the whole **id path** is saved on entry, restarted empty, and restored on exit — unconditionally, and by a marker rather than by a `#pragma prefix` | measured against `omniidl` on 2026-08-14 in both directions: the includer's prefix does not enter, the included file's prefix does not escape. `types.idl` pins it — same module, two files, one prefixed id and one not. Re-measured on 2026-08-18 with the `#include` **inside a module**, where "reset the prefix" and "reset the id path" stop being the same instruction, against both oracles: `inc-scope-*.idl` |
| `#if`/`#ifdef`/`#define X v`/`#undef` | **refused** with a diagnostic naming what to run first | ignoring conditional compilation compiles every arm at once, which is a silent misparse. `omniidl` compiles `conditional.idl`; we decline. Out of scope for this pass, and said so rather than guessed |
| diagnostic position | the included file's own line, with the include chain underneath | pointing at the includer's `#include` line would make every diagnostic in a large estate point at the same handful of lines |

> 결정 요약: 탐색 경로는 C 관례(암묵적 경로 없음) · 정준 경로 기준 **1회만** 삽입
> (가드 불요, 무가드 중복은 조언) · 순환은 종료 + 이름 지적 · 누락은 탐색 경로를
> 전부 나열하는 오류 · `#pragma prefix`는 파일 경계에서 저장/복원(양방향 측정) ·
> 조건부 컴파일은 **거부**(조용한 오독보다 명시적 거부) · 진단 위치는 인클루드된
> 파일의 줄 + 인클루드 체인.

## The `inc-*` files: an `#include` that is not at file scope

Every case above puts its `#include` at **file scope**. That is not a shape
anybody chose; it is what a directory written one root at a time looks like,
and it hid something for the same reason the self-contained corpus hid
`#include` itself for six phases. At file scope the id path is empty, so
"reset the prefix" and "reset the id path" are the same instruction and a
resolver can implement the file boundary as a `#pragma prefix` pair without
anything going red. Inside a module they are different instructions, and the
difference is a repository id.

Eight files (`inc-*.idl`, 2026-08-18) put the `#include` inside a module, at
depth two, in a file with a prefix, in a file without one, with a prefixed
leaf and an unprefixed one, and from two prefix scopes at once. **32
repository ids, measured against `omniidl` and against JacORB 3.9.** Seven of
them disagreed with both oracles, and all seven were one cause: the boundary
was expressed as `#pragma prefix`, and `#pragma prefix` *replaces* the id path
(`corpus/pragma/p02`), so the restore could name the includer's prefix but
never the modules the `#include` sat inside — `IDL:hub.example/Gate:1.0` where
both oracles said `IDL:hub.example/Yard/Gate:1.0`. We were **wrong, not
different**, and it is fixed: the boundary is now
`#pragma orbweaver include-enter` / `include-leave`, injected unconditionally
and understood by the parser as a save/restore of the whole path.

> 이 디렉터리의 기존 케이스는 전부 인클루드가 **파일 스코프**에 있다. 거기서는 ID
> 경로가 비어 있어 "접두사 리셋"과 "경로 리셋"이 같은 말이 되고, 그래서 결함이
> 보이지 않았다. `inc-*.idl` 여덟 개가 인클루드를 모듈 안으로 옮겨 저장소 ID 32개를
> 두 오라클에 대해 측정했다 — 7개가 어긋났고 원인은 하나였다. 우리가 틀렸고,
> 고쳤다.

### Where the two oracles disagree with each other

They agree on the **exit**: after an included file, the includer's prefix *and*
its enclosing modules resume. They disagree on the **entry**:

| | `inc-scope-control.idl`, leaf declared as `Parcel::TagNumber` |
|---|---|
| omniidl 4.3.4 | `IDL:Parcel/TagNumber:1.0` — the id path resets at a file boundary, prefix or no prefix |
| JacORB 3.9 | `IDL:Ledger/Parcel/TagNumber:1.0` — nothing resets; the includer's module is inherited |

**We follow omniidl**, and not only because it is this project's conformance
oracle and interop peer. Under JacORB's reading a leaf's identity depends on
which root reached it: `inc-leaf-plain.idl` compiled alone is
`IDL:Parcel/TagNumber:1.0` and compiled through `inc-scope-plain.idl` is
`IDL:hub.example/Yard/Parcel/TagNumber:1.0`. That is precisely what
`a_shared_file_keeps_its_identity_whichever_root_reaches_it` exists to forbid,
and an id that changes with the includer is an IOR no peer agrees with. The
divergence is recorded per row in `cases.tsv`.

> 두 오라클은 **나갈 때**는 일치하고 **들어갈 때** 갈린다. omniidl은 파일 경계에서
> ID 경로를 리셋하고 JacORB는 하지 않는다. omniidl을 따른다 — JacORB 해석에서는
> 잎의 정체성이 "누가 인클루드했는가"에 달리게 되고, 그것이 바로 이 디렉터리가
> 금지하려는 실패다.

### One caveat this measurement produced

`idl-check -E` output is **our** intermediate, not portable IDL for identity
purposes. It carries the boundary as a pragma only we read, so feeding it back
to `omniidl` measures the *concatenation* and not the unit: measured
2026-08-18, `omniidl` on `inc-scope-plain.idl` gives `IDL:Parcel/TagNumber:1.0`
and `omniidl` on that file's `-E` output gives
`IDL:hub.example/Yard/Parcel/TagNumber:1.0`. Compare ids by running each front
end on the **root**, never one of them on the other's splice.

> `idl-check -E` 결과는 우리 중간 산출물이다. 그것을 다시 `omniidl`에 넣으면 유닛이
> 아니라 단순 이어붙이기를 측정하게 된다. 두 front end는 각각 **루트**에 대해
> 돌려서 비교한다.

## The two deliberate divergences

Both are in `cases.tsv` with their reasons, and they point in opposite
directions, which is the honest shape:

- `service.idl` — **we accept what `omniidl` rejects.** Advice says so.
- `conditional.idl` — **we refuse what `omniidl` compiles.** The diagnostic
  says what to run first.

They are not recorded in `corpus/divergences.tsv` because `spikes/differential.sh`
never examines this directory, and a row there for a file it does not run would
be reported as a stale exemption on every run. `divergences.tsv` carries a
pointer here instead.

> 의도된 불일치 두 건은 방향이 서로 반대다 — 하나는 우리가 더 느슨하고 하나는 더
> 엄격하다. `differential.sh`가 이 디렉터리를 검사하지 않으므로
> `corpus/divergences.tsv`에는 행이 아니라 포인터만 둔다(없는 파일에 대한 행은
> 매 실행마다 "더 이상 발생하지 않는 등록"으로 보고된다).
