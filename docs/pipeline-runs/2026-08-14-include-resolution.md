# `#include` resolution, and the span that made every hint wrong

> **Measured 2026-08-14** in `crates/orbweaver-idl`, against the thirteen-file
> estate in `spikes/estate/`, the `omniidl` on this machine (`omniidl -V` says
> `omniidl version 1.0`, Homebrew `omniorb`), and the whole corpus. Every
> number below came out of a command named beside it. Repairs for RC-1 and
> RC-2 of `docs/pipeline-runs/2026-08-14-estate.md`.
>
> **2026-08-14 측정.** 아래 수치는 전부 옆에 적힌 명령의 출력이다. 위 자산
> 보고서의 RC-1·RC-2에 대한 수정이다.

The estate found two defects in the IDL front end that **the corpus could never
have found**, because every file in `corpus/` is self-contained. This closes
both, and adds the corpus directory whose absence is the actual reason they
survived six phases.

---

## 1. What was wrong / 무엇이 틀렸나

### RC-1 — `#include` was skipped, not resolved

`crates/orbweaver-idl/src/lex.rs` dropped every `#` line except `#pragma`. A
file naming a type declared one file away therefore failed semantic analysis on
its own: **11 of 13 estate files rejected here and accepted by `omniidl`, ~90
diagnostics, every one of them `unknown-name`.**

### RC-2 — the fix hint was corrupted for a `::`-qualified name

The hint read *"`::` is not declared … qualify it with `Module::::`"*. The
cause was **not** in the code that prints it. `Parser::scoped_name` gave a
scoped name the span of its **first token only**, so `::MFS::Common::StringList`
had a span covering the leading `::` and nothing else, and every consumer that
slices the source with a span read the text `"::"`.

That is why it is fixed here and not where it was noticed. The same defect made
the relative form wrong and quieter about it — `MFS::Common::StringList` sliced
to `MFS`, which looks like a name and is not the one in the message — and it was
present at **three** sites: `scoped_name` and both arms of `primary_expr`. The
estate only saw one of the three.

> RC-1: `#include`를 해석하지 않고 건너뛰었다(13개 중 11개 실패, 진단 90건).
> RC-2: 힌트를 찍는 코드가 아니라 **파서의 스팬**이 원인이었다 — 스코프 이름의
> 스팬이 첫 토큰만 덮어서, 스팬으로 원문을 잘라 쓰는 모든 소비자가 `"::"`를
> 읽었다. 같은 결함이 **세 군데**에 있었고 자산은 그중 하나만 보았다.

---

## 2. Batch, first pass, rounds / 배치·1차 통과율·라운드

Batch: **both repairs written in one pass**, together with 15 tests, then the
whole set through every gate at once. The two numbers below measure different
things and are stated separately, per the operating model.

| | measured |
|---|---|
| batch size | 13 estate roots + 81 corpus/spike files + 13 new manifest rows + 25 new tests |
| **first pass — the batch oracle** | **94/94.** The first run of the whole-set oracle after the one-pass repair: estate 13/13, golden 27/27, negative 12/12 rejected, pragma 16/16, requirements+services+spikes 26/26 |
| **first pass — the tests written with the repair** | **12/15 (80%)** on the first `cargo test -p orbweaver-idl` |
| **rounds** | **4** |

The batch oracle passing first time is not the flattering number it looks like:
the three failures were caught by the tests written in the same pass, *before*
the oracle ran, which is what tests written alongside a repair are for. Reported
separately so neither number hides the other.

Ten further tests came in later rounds and are **not** in the first-pass
denominator: four pinning RC-2's span and rule directly (the fix itself was in
the first pass; its dedicated tests were not), and the six of
`tests/include_corpus.rs`, which could not exist until `corpus/include/` did.

> 배치: 두 수정을 **한 번에** 작성하고 테스트 15개를 같이 쓴 뒤 전체를 한 번에
> 검증. 오라클 1차 94/94, 같이 쓴 테스트 1차 12/15(80%), 라운드 4. 오라클이 1차에
> 통과한 것은 같은 패스에서 쓴 테스트가 먼저 세 건을 잡았기 때문이며, 두 수치를
> 따로 적는 이유가 그것이다. 이후 라운드에서 추가된 테스트 10건(RC-2 전용 4건,
> 코퍼스 게이트 6건)은 1차 통과율 분모에 넣지 않았다.

### Root causes found during repair, clustered / 라운드별 근본원인

| round | cause | affected | fix |
|---|---|---:|---|
| 1 | **"the prefix moved" was computed in one place and consumed in another with a different meaning.** The restore after an include fired only when the *included subtree* set a prefix, missing the case where only the entry reset moved it; and a file's own `#pragma prefix` never marked the file as having moved it at all | 2 sites, 3 tests | one predicate: restore whenever `reset || touched`, and set `touched` on the file's own pragma too |
| 1 | a test asserted on relative file names where the resolver reports absolute ones | 1 test | assert on the shape of the chain, not on its spelling (a test defect, recorded as its own cause rather than folded into the one above) |
| 2 | **dropping a directive line broke the 1:1 line map**, so two `#include`s in one file reported the same position | every file with more than one include — 7 of the 13 estate files | a directive keeps an output line: it is replaced by an inert `// [orbweaver-idl] …` stub rather than deleted |
| 3 | `#endif` was classified as refusable conditional compilation, so one `#ifdef` produced two diagnostics | every refused file | an `#endif` says nothing on its own and the `#if` it closes is reported already |
| 4 | — | — | no new cause; the round that ends the batch |

---

## 3. What `#include` resolution now means / 해석의 정의

Decided explicitly, because each of these is a place where a wrong default is
invisible until a peer disagrees. `crates/orbweaver-idl/src/include.rs` carries
the same list in its module docs, and `corpus/include/README.md` carries it in
the table a reader of the fixtures will find.

| question | answer | why |
|---|---|---|
| **search path** | `"x.idl"` → the including file's directory, then `-I` dirs. `<x.idl>` → `-I` dirs only. Nothing implicit; the process's working directory is **never** searched | the C convention CORBA IDL inherits and `omniidl -I` implements. Searching the cwd would make the same estate resolve differently depending on where the validator was started, and the difference would surface as a repository id rather than as an error |
| **who supplies it** | `idl-check -I <dir>`; the library takes a `SearchPath` | see §6 for the tools that cannot pass one yet |
| **idempotence** | a file is spliced **at most once per unit**, keyed by canonical path. Guards are **not required** | the estate measured that real IDL has none — six of thirteen files were rejected by `omniidl` for exactly that before their author added them. Including an IDL file twice can only duplicate a declaration, never add one, so once-only loses nothing |
| **…and the honesty cost of that** | a repeat of an **unguarded** file raises non-blocking advice naming the file and the guard to add | it makes us laxer than `omniidl`, which rejects such a file. Being quietly laxer than a deployed compiler is how a project ships something that fails at the peer. The advice is what stops the divergence being quiet |
| **cycles** | terminate, and name the whole loop in a non-blocking diagnostic | a guarded cycle is legal IDL that `omniidl` compiles, so an error would reject valid input; silence would leave a real mistake unreported. Tested from **both** ends, because a loop that only terminates from one direction is not terminating |
| **missing file** | error, listing **every path searched**, in order | a silent skip is the defect this whole change removes |
| **`#pragma prefix` across an include** | reset to empty on entry, the includer's own restored on exit | **measured, not reasoned** — see §4 |
| **diagnostic position** | the included file's own line and column, with the `#include` chain underneath, innermost last | pointing at the includer's `#include` line would make every diagnostic in a large estate point at the same handful of lines. `omniidl` reports it the same way (`pb.idl:1: …` for an error in an included file, measured) |

> 결정 요약: 탐색 경로는 C 관례이며 암묵 경로 없음(작업 디렉터리 미탐색) ·
> 정준 경로 기준 **1회만** 삽입(가드 불요, 무가드 중복은 조언) · 순환은 종료하고
> 루프 전체를 이름으로 지적(양방향 테스트) · 누락은 탐색 경로를 전부 나열하는
> 오류 · `#pragma prefix`는 파일 경계에서 저장/복원(§4에서 측정) · 진단 위치는
> 인클루드된 파일의 줄 + 인클루드 체인.

### Scoped out, explicitly / 명시적으로 범위 밖

**Full C preprocessing is not implemented, and a file that needs it is refused
rather than misparsed.** `#if`, `#ifdef`, `#elif`, `#else`, `#undef`, `#error`
and a `#define` with a replacement all produce an error naming what to run
first (`cpp -P`, or `omniidl -E`). Only the `#ifndef G` / `#define G` /
`#endif` **include-guard idiom** is recognised, and it is recognised rather
than evaluated — idempotence comes from the canonical-path rule, so a guard
neither helps nor is needed.

The reason for refusing rather than skipping is the only interesting part:
skipping `#ifdef DEBUG` compiles **every arm at once**. That is a silent
misparse, and it is worse than either of the other two answers. `omniidl`
compiles `corpus/include/conditional.idl` and we decline to; the divergence and
its direction are recorded in `corpus/include/cases.tsv`.

`# 12 "f.idl"` line markers are still ignored rather than honoured, so feeding
already-preprocessed text back in gives positions in that text. Named as a
limit, not fixed.

> 전체 C 전처리는 구현하지 않으며, 필요한 파일은 **오독하지 않고 거부**한다.
> 조건부 컴파일을 건너뛰면 모든 분기를 동시에 컴파일하게 되고, 그것이 세 선택지
> 중 가장 나쁘다. 가드 관용구만 인식하되 평가하지는 않는다 — 멱등성은 정준 경로
> 규칙에서 나오므로 가드는 필요하지 않다.

---

## 4. The prefix boundary, measured / 접두사 경계 — 측정

This is the trap the estate paid for (RC-4 there): a file-scope `#pragma
prefix` runs to the end of **its file**, so splicing files silently hands one
file's prefix to the next one's declarations, and *both* answers are
well-formed repository ids that nothing warns about.

The rule was **measured against `omniidl`**, not inferred, with two probe files
and `-Wbinline` (which is what makes the oracle emit ids for included
declarations at all):

```text
a.idl                        b.idl                    omniidl said
──────────────────────────   ──────────────────────   ─────────────────────
#pragma prefix "aaa"         module N { interface J   IDL:aaa/M/I:1.0
#include "b.idl"                       ... };         IDL:N/J:1.0
module M { interface I ... };                         ← the includer's prefix
                                                        does NOT enter

#pragma prefix "aaa"         #pragma prefix "bbb"     IDL:aaa/Q/L:1.0
#include "c.idl"             module P { interface K   IDL:bbb/P/K:1.0
module Q { interface L ... };          ... };         ← the included prefix
                                                        does NOT escape
```

So the boundary is a save on entry and a restore on exit, and the splice
implements it by injecting `#pragma prefix ""` before an included body and the
includer's own prefix after it — and only when injecting would change
something, so a unit whose files carry no prefix is spliced byte-for-byte.

**Verified against the oracle over the whole estate, per file:**

```bash
# for each of the 13 roots: our resolved unit vs omniidl -Wbinline on the same root
idl-check -E <root> | repository-ids   ⟷   omniidl -bpython -Wbinline -Wbstdout <root>
→ per-file identity: 13/13 files agree with omniidl
→ 49 distinct ids across the estate (MAX_BAYS excluded by name, as before)
```

This is a **stronger** check than the estate driver's stage 4, which compares
one hand-spliced translation unit against the union of thirteen separate
`omniidl` runs. Per file, a prefix that leaked across one include boundary is
attributable to that boundary instead of being averaged into a union. The
sharpest case is `05-billing.idl`, which carries no prefix of its own and
follows a file that sets one:

```text
naive concatenation (estate RC-4)          include resolution (this change)
IDL:meridian.com/MFS/Billing/Invoice:1.0   IDL:MFS/Billing/Invoice:1.0
                                           ↑ what omniidl says, ×5 ids
```

> 파일 스코프 `#pragma prefix`는 **그 파일 끝까지** 유효하다. 규칙은 추론이
> 아니라 `omniidl -Wbinline`으로 **측정**했다 — 인클루드하는 쪽의 접두사는 들어
> 가지 않고, 인클루드된 쪽의 접두사는 새어 나오지 않는다. 자산 13개 루트 전부에
> 대해 **파일 단위로** 오라클과 대조했고 13/13 일치, ID 49개. 자산 드라이버의
> 4단계보다 강한 검사다(합집합이 아니라 파일 단위 귀속).

---

## 5. Before and after / 전후

### `s1-per-file`, the number that was asked for

```text
                       before (published, 2026-08-14 estate run)   after
s1-per-file accepted   2 / 13                                      1 / 13
```

**It went down, and that is the honest result.** `spikes/estate/run.sh` stage 1
runs `sidl-validate`, which lives in `crates/orbweaver-forge` — another agent's
footprint this batch, so it is unchanged and still hands the front end a
**string**, which has no directory for `#include "x.idl"` to be relative to.

Of the two files that used to pass, only `01-common.idl` has no `#include` at
all. The other was `08-depot-codes.idl`, which includes `02-mfs-types.idl` and
happens to reference nothing from it — **it passed because the include was
silently skipped**, which is the defect, not a pass. It now reports its
unresolved include like the other eleven.

What did change on that path, with no forge change at all:

```text
                                    before        after
diagnostics over the 13 files       ~90           19   (one per #include, 19 in the estate)
what they said                      unknown-name  include-not-found, naming the file
                                    ×90, with     and every path searched
                                    a corrupted
                                    hint on most
```

**With an include-aware entry point the same thirteen files are 13/13**, and
that is measured, not projected:

```bash
$ cargo run -q --bin idl-check -- spikes/estate/idl/*.idl
→ 13/13, exit 0
```

Every other estate row is unchanged and the driver still exits 0: `s2-oracle
13/13`, `s3-splice advice 104`, `s4-identity spliced-agrees yes`,
`naive-drift 5`, `s5-register exposable 12`, `s6-generate 0 skips / 12/12
halves`, `s7-dryrun 76/76`, `s8-serve published yes`, `s9-agent rc 0`.

> **수치가 내려갔고, 그것이 정직한 결과다.** 1단계는 `sidl-validate`(=forge,
> 이번 배치의 footprint 밖)를 쓰고, 그것은 여전히 **문자열**을 넘긴다 — 문자열에는
> `#include "x.idl"`가 상대할 디렉터리가 없다. 기존에 통과하던 두 파일 중 하나는
> `#include`가 **조용히 건너뛰어져서** 통과한 것이었다(그게 결함이다). forge를
> 건드리지 않고도 진단은 90건 → 19건으로 줄고 내용이 정확해졌으며, 인클루드를
> 아는 진입점으로는 **13/13**이다.

### The RC-2 hint

```text
before:  error: "::MFS::Common::StringList" is not declared [unknown-name]
             fix: "::" is not declared anywhere in scope. Declare it, qualify
                  it with its module (`Module::::`), or correct the spelling.

after:   error: "::MFS::Common::StringList" is not declared — "MFS" is not
                declared at global scope; if it is declared in another file,
                this translation unit has no `#include` that reaches that file
                [unknown-scoped-name]

         (and the bare form keeps the hint that was always right for it)
         error: "AuditStamp" is not declared — if it is declared in another
                file, … [unknown-name]
             fix: "AuditStamp" is not declared anywhere in scope. Declare it,
                  qualify it with its module (`Module::AuditStamp`), or
                  correct the spelling.
```

Two changes, because there were two faults. The span fix makes the *text*
right. The rule split makes the *advice* right: "qualify it with its module" is
correct for `AuditStamp` and meaningless for a name that is already qualified,
and the renderer had no way to tell them apart from one shared rule name. A
qualified failure now files under `unknown-scoped-name`, carries advice written
where the analyser knows **which component** failed and **which scope** it
looked in, and no longer matches the generic template at all.

> 결함이 둘이었으므로 수정도 둘이다. 스팬 수정이 **문구**를 바로잡고, 규칙 분리가
> **조언**을 바로잡는다 — 이미 정규화된 이름에 "모듈로 정규화하라"는 조언은 의미가
> 없고, 렌더러는 규칙 이름 하나로는 두 경우를 구분할 수 없었다.

---

## 6. What is codified / 성문화한 것

A cause that is only fixed comes back. Each of these is a permanent artefact,
not a patch.

1. **`corpus/include/` — the first multi-file corpus case.** 12 IDL files,
   a manifest (`cases.tsv`) and a bilingual README. It pins every decision in
   §3 as a case: guards absent on purpose, a guarded control beside the
   unguarded case, the prefix-scope case, a cycle entered from both ends, a
   missing include, conditional compilation, and the quoted/angled asymmetry
   with and without `-I`.
   **The corpus had no multi-file case at all, and that is precisely why RC-1
   was invisible** — not by oversight but by construction, since no
   self-contained file can exercise a cross-file reference.
2. **`crates/orbweaver-idl/tests/include_corpus.rs`** — the gate that drives
   the manifest, plus five properties that need no oracle installed: the
   file-scope prefix boundary as **exact id strings**, the same boundary
   through a two-level chain, advice firing only where the guard is missing
   (asserted as a *pair* with the guarded control, so it cannot pass by
   advising on everything), rendering against the file a diagnostic was written
   in, and a shared declaration keeping one identity whichever root reaches it.
3. **19 unit tests in `crates/orbweaver-idl`** covering resolution, both search
   forms, once-only splicing, guarded and unguarded repeats, two- and
   three-file cycles, missing files, comment-embedded `#include`s, refusal of
   conditional compilation, position mapping through a chain, byte-identity for
   a self-contained file, and the span of a scoped name in all four shapes and
   both parsers.
4. **`corpus/include/cases.tsv` records both deliberate divergences** with the
   direction each one points, measured against `omniidl` on 2026-08-14 —
   `service.idl` where we accept what it rejects, `conditional.idl` where we
   refuse what it compiles.
5. **`corpus/divergences.tsv` gained a pointer, not rows.** A row there for a
   file `spikes/differential.sh` never examines would be reported as a stale
   exemption on *every* run — the file's own staleness check keys on the
   oracle's name being one it ran. The comment block says where those two rows
   live and why they are not here.
6. **The identity property is a byte-identity guarantee, not a habit.** A file
   with nothing to resolve comes out of the resolver unchanged, which is what
   keeps a diagnostic's span valid against the *original* source for the
   consumers that slice it (`orbweaver-forge`'s fix hints do). It has its own
   test.

> 성문화: `corpus/include/`(프로젝트 최초의 다중 파일 코퍼스, 매니페스트와
> 이중언어 README) · 오라클 없이도 도는 게이트 테스트 · 크레이트 테스트 19건 ·
> 의도적 불일치 2건을 방향과 함께 기록 · `divergences.tsv`에는 행이 아니라 포인터
> (검사되지 않는 파일에 대한 행은 매 실행마다 오탐이 된다) · "해석할 것이 없는
> 파일은 바이트 단위로 동일" 을 테스트로 보증.

---

## 7. The follow-ups this batch could not apply / 이번 배치가 적용할 수 없었던 후속

`crates/orbweaver-forge`, `-gen`, `-mcp` and `-giop` were other agents'
footprints. These are written out rather than applied, and each is small.

### 7.1 `sidl-validate` — the one that moves `s1-per-file` to 13/13

`crates/orbweaver-forge/src/bin/sidl_validate.rs`. Nothing in `run.sh` needs to
change: the quoted form resolves relative to the file's own directory, so the
estate resolves with no `-I` at all.

```rust
use orbweaver_idl::{SearchPath, preprocess_file};

// in the argument loop, beside --json:
    a if a.starts_with("-I") => {
        let d = if a.len() > 2 { a[2..].to_owned() } else { args.next().unwrap_or_default() };
        search.push(d);
    }

// replacing `let src = std::fs::read_to_string(path)`:
    let unit = match preprocess_file(std::path::Path::new(path), &search) {
        Ok(u) => u,
        Err(e) => { eprintln!("{path}: {e}"); return std::process::ExitCode::from(2); }
    };
    if !unit.is_ok() {
        for d in &unit.errors { println!("{}", unit.render(d)); }
        rejected += 1;
        continue;
    }
    for a in &unit.advice { println!("advice: {}", unit.render(a)); }
    let src = unit.text.clone();

// and, so a position is reported against the file it was written in rather
// than against a spliced unit nobody has — `locate` reads only line and column:
    let at = unit.locate(orbweaver_idl::lex::Span {
        start: 0, end: 0, line: f.line, column: f.column,
    });
    println!("{}:{}:{}: {}", at.file.display(), at.line, at.column, f);
```

Keeping `unit` alongside each `Report` is the only structural change: the
existing loop drops the source after validating, and a `Finding` carries a line
number rather than a span.

### 7.2 Everything else that takes one file of IDL

`repository-ids`, `gen-corpus`, `forge-pipeline` and the console all read a
single file. Until each grows a `SearchPath`, the bridge is
**`idl-check -E`**, which writes the resolved unit to stdout the way `cpp -P`
does and is what §4's identity comparison used:

```bash
cargo run -q --bin idl-check -- -E [-I <dir>] <root>.idl > unit.idl
```

It is not the concatenation of the files — the prefix is reset at each file
boundary and restored after it, which is the whole difference between a
repository id that is right and one that is merely well-formed.

### 7.3 `spikes/run_checks.sh` — a section for `corpus/include/`

Not applied (the harness is landed from main). `cargo test` already gates the
directory; this adds it to the harness's own reporting:

```bash
# ── corpus/include: the multi-file cases ────────────────────────────────────
echo "corpus/include — #include resolution, prefix scope, guards, cycles"
if cargo test -q -p orbweaver-idl --test include_corpus >/tmp/orbweaver-inc.log 2>&1; then
  echo "  ok   $(grep -c '^[a-z]' corpus/include/cases.tsv) manifest case(s), \
$(ls corpus/include/*.idl | wc -l | tr -d ' ') file(s)"
else
  echo "  FAIL"; tail -20 /tmp/orbweaver-inc.log; fails=$((fails + 1))
fi
```

> forge·gen·mcp·giop는 이번 배치의 footprint 밖이므로 적용하지 않고 적어 둔다.
> 7.1을 적용하면 `s1-per-file`이 13/13이 되며 `run.sh`는 손댈 필요가 없다
> (따옴표 형식은 파일 자신의 디렉터리를 기준으로 해석된다). 한 파일만 받는 다른
> 도구들에는 `idl-check -E`가 다리 역할을 한다.

---

## 8. What was not measured / 측정하지 않은 것

Named, because a stage nobody mentions is a gap and a stage named unmeasured is
a result.

- **`./spikes/run_checks.sh` was not run in full.** The machine's disk filled
  during the batch (100% on `/System/Volumes/Data`, with 47 sibling worktrees
  on it) and a full harness run could not be completed without a false failure
  that would have been about the disk. **This is an unmeasured check and counts
  as a failure, not a pass.** The four sections of it that touch the front end
  *were* run individually and all passed: `idl-check` silent over golden +
  benchmark + spikes (line 267), `sidl-validate` accepting all 52 valid files
  (654), rejecting all 12 negatives (663), and `repository-ids` agreeing with
  `omniidl` on all 25 `corpus/pragma` ids (1254). `spikes/differential.sh` ran
  in full: 64 files, no unexplained divergence, exit 0.
- **Portability of the include semantics across front ends is unmeasured.**
  `tao_idl` and `jacorb_idl` are absent from this machine, so
  `differential.sh` ran with one oracle and said so. Everything in §3 and §4 is
  measured against `omniidl` alone.
- **No wire, no peer, no foreign ORB.** This is a front-end change; nothing
  here was exercised over GIOP.
- **An `#include` written *inside a module* whose included file sets a prefix
  is not exercised by any file we have.** The restore pragma would be emitted
  at module scope, and a `#pragma prefix` inside a module does not mean what
  one at file scope means (`corpus/pragma/p02`, `corpus/divergences.tsv`). The
  injection is deliberately conditional to keep this case rare, and it is
  named here rather than claimed to work.
- **Two paths that are hard links to the same content are two files to the
  resolver.** `canonicalize` resolves symlinks and `.`/`..`; it does not
  resolve hard links, and nothing tried it.
- **`# N "file"` line markers are ignored, not honoured.** Validating the
  output of `cpp`/`omniidl -E` therefore reports positions in that output.
- **Generator and evaluator are the same model.** The corpus files in
  `corpus/include/` were written by the same author who chose what they should
  pin, so *their* coverage is an **indicative** number; the `omniidl` column of
  `cases.tsv` and the 13/13 per-file identity comparison are not, because those
  came from a compiler that is not ours.

> `run_checks.sh` 전체는 **실행하지 못했다**(디스크 100% 포화). 이는 통과가 아니라
> **미측정 실패**로 계수한다. 프런트엔드에 해당하는 네 구간은 개별 실행해 전부
> 통과했고 `differential.sh`는 완주했다. TAO·JacORB 부재로 다른 프런트엔드에 대한
> 이식성은 미측정, 와이어/피어 관여 없음, 모듈 스코프 안의 `#include` + 접두사
> 조합은 어떤 파일도 시험하지 않음, 하드링크 중복은 별개 파일로 취급, 라인 마커는
> 무시. 코퍼스 파일은 작성자와 평가자가 같으므로 **참고치**이며, `omniidl` 열과
> 13/13 동일성 비교는 그렇지 않다.

---

## 9. Gates / 게이트

```text
cargo test --workspace                        1164 passed, 0 failed
cargo fmt --check                             clean
cargo clippy --workspace --all-targets        0 warnings
RUSTFLAGS="-D warnings" cargo test -p orbweaver-idl   green
unsafe_code = "forbid" / #![deny(missing_docs)]       unchanged
./spikes/estate/run.sh --tsv                  exit 0 (see §5 for the rows)
./spikes/differential.sh                      exit 0, 64 files, 1 oracle
per-file repository-id identity vs omniidl    13/13 estate roots, 49 ids
corpus/include vs omniidl                     13 rows, 11 agree, 2 divergences on purpose
./spikes/run_checks.sh                        NOT RUN — see §8
```

1164 is 1139 (the baseline this batch started from) plus the 25 tests added
here — 19 unit tests in `crates/orbweaver-idl` and 6 in `tests/include_corpus.rs`.

## 10. Provenance of the numbers / 수치의 출처

Stated because two of them would otherwise be uncomparable later.

- **The baseline is this worktree's**, `80959f0` ("harness: the registered-contract
  diff is a gate, not a crate test"), where `cargo test --workspace` was 1139.
  Sibling waves were landing in `-forge`, `-gen`, `-mcp` and `-giop` at the
  same time, so a later total on main will not be 1164 + 0.
- **`spikes/estate/` was copied into this worktree to be measured against** and
  is **not** part of this change. Main has since moved it — stages 3 and 5–7 of
  `run.sh` changed when another wave repaired the estate's RC-6 and RC-7 — so
  the copy here is stale and must be discarded rather than landed. **Stage 1 is
  byte-identical between the two versions**, which is the only stage this batch
  moves, so the `s1-per-file` before/after in §5 compares like with like.
  `docs/pipeline-runs/2026-08-14-estate.md` was copied for the same reason and
  is unmodified.

> 기준선은 이 worktree의 `80959f0`(워크스페이스 테스트 1139)이며, 다른 웨이브가
> 동시에 forge·gen·mcp·giop에 착지 중이었으므로 이후 main의 합계는 1164와 다르다.
> `spikes/estate/`는 **측정을 위해 복사**한 것이고 이 변경의 일부가 아니다. main은
> 이미 그 파일을 수정했으므로 이 사본은 폐기해야 한다. 단 **1단계는 두 버전이
> 바이트 단위로 동일**하므로 §5의 전후 비교는 같은 것끼리 비교한 것이다.

## 11. Reproducing / 재현

```bash
cargo test -p orbweaver-idl                                   # includes the corpus gate
cargo run -q --bin idl-check -- spikes/estate/idl/*.idl       # 13/13
cargo run -q --bin idl-check -- corpus/include/service.idl    # accepts, with advice
cargo run -q --bin idl-check -- -I corpus/include corpus/include/angled.idl
cargo run -q --bin idl-check -- -E corpus/include/types.idl   # the resolved unit
```

The identity comparison needs `omniidl` (`brew install omniorb`) and is, per
root file:

```bash
idl-check -E <root> > unit.idl && repository-ids unit.idl | cut -f3 | sort -u
omniidl -bpython -Wbinline -Wbstdout <root> | grep -oE '"IDL:[^"]*"' | ...
```

`-Wbinline` is load-bearing: without it the Python back end emits ids for the
main file only, and an included file's identity — which is the entire question
— is invisible.
