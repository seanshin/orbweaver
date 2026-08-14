# A second target language: Python clients, and what the second one found (2026-08-14)

`orbweaver-gen` emitted Rust. It now also emits Python. The point was never
Python — it was that **one target cannot tell the IDL mapping apart from what
was convenient in Rust**, and writing the mapping twice is the only thing that
can. This record is what the second one found.

Footprint: `crates/orbweaver-gen/`, `corpus/golden/28-target-keywords.idl`,
`docs/decisions/D007-python-wire-seam.md` (**PROPOSED**), and this file. No
other crate. `spikes/run_checks.sh` untouched — the harness lines this run
wants are at the end, to be applied from main.

---

## 1. What was built

| | |
|---|---|
| The emitter | `crates/orbweaver-gen/src/python.rs` — a Python **package** per IDL file, one module per IDL module |
| The runtime | `crates/orbweaver-gen/src/python_rt.py` — hand-written, shipped verbatim as `_rt.py`; AnyJSON v1 and nothing else |
| The seam | `crates/orbweaver-gen/src/bin/py_bridge.rs` — `orbweaver-py-bridge`, one JSON document per line over stdin/stdout |
| The CLI | `crates/orbweaver-gen/src/bin/gen_python.rs` — `gen-python --out <dir> <file.idl>...` |
| The oracle | `crates/orbweaver-gen/tests/python_target.rs` — 4 tests, all of which **execute** generated Python |
| The live proof | `crates/orbweaver-gen/python/echo_client.py` — a generated client against the omniORB fixture |
| The corpus case | `corpus/golden/28-target-keywords.idl` — identifiers that are keywords in a target language |

### Where the wire is: not in Python

A generated Python client renders its arguments as **AnyJSON v1** (`PLAN.md`
§4.5 — this project's own normative JSON ↔ CDR mapping) and hands them to
`orbweaver-py-bridge`, which invokes through `orbweaver_dynamic::invoke`. So
CDR, GIOP, alignment, byte order and codeset negotiation still exist exactly
once, in Rust: **the second target language did not buy a second ORB.**

What *is* duplicated, stated plainly rather than hidden: `_rt.py` is a second
implementation of §4.5. That is unavoidable for a target in any language —
something in Python has to turn Python objects into whatever crosses — and it
is the smallest thing that could be duplicated, because §4.5 is a written
specification with a round-trip acceptance criterion. The oracle holds the two
implementations to the same verdicts over the whole golden corpus.

The alternatives — a PyO3 or `cdylib` extension module, and pure-Python
CDR/GIOP — are compared in **D007, left PROPOSED**. An extension module is a
new dependency class *and* a question about `unsafe_code = "forbid"` at the
boundary, and this project does not adopt one of those by writing code.

---

## 2. The batch → oracle → repair → codify run

**Batch: 27 golden files, generated in one pass, no oracle consulted during
it.** (`corpus/services` was generated in the same pass and is reported
separately below; the 28th golden file did not exist yet — it was created
during repair, as a codification.)

### First-pass rate

**21 of 27 golden files (77.8%) passed the round-1 oracle unmodified.** Six
files produced at least one diagnostic. The number is a signal about the
emitter and is reported separately from the round count, which is a signal
about the oracle.

Both the generator and this oracle were written by the same model in the same
session. The round-trip halves are independent implementations (Rust's
`anyjson` was not written here), which is what makes that comparison worth
something; the *choice of what to compare* was not, so read the number as
**indicative**.

### Root causes, clustered — never by file

| # | Cause | Where | Affected |
|---|---|---|---|
| RC-1 | An enumerator's owning type was identified **by class identity** in the runtime and **by repository id** by the emitter, so no enumerator ever matched its own enum | `python_rt.py` | 4 golden files (05, 06, 19, 22) + `ir_subset`; 20 diagnostics |
| RC-2 | The **base64 rule was applied to arrays as well as sequences**. §4.5 gives base64 to `sequence<octet>` because a megabyte of binary must not become a million JSON numbers; an IDL array has its length in its type and crosses as an array | `python_rt.py` | 1 golden file (08); 3 diagnostics |
| RC-3 | *(oracle)* A failed item was **dropped instead of holding its place**, so every later item in that file was compared against the wrong expectation | `tests/python_target.rs` driver | ~30 of the 66 round-1 messages were spurious |
| RC-4 | *(oracle)* The plan included items the generator had **skipped with a reason**, so a deferred type was reported as a missing class | `tests/python_target.rs` | 2 files; 8 diagnostics |
| RC-5 | A **Rust naming artifact copied into a Python file**: a declared-and-never-defined type was emitted as `MoneyRef`, because Rust needs a distinct name for the alias. Python does not, and `omniidl` calls it `Money` | `python.rs` | 2 items in 1 file |
| RC-6 | **Keyword escaping went the wrong way.** This emitter used a trailing underscore on a reasoned argument (a leading one means "private" in Python); the OMG mapping and `omniidl -bpython` both use a **leading** one. The same cause has a second half: the runtime read struct members by their *wire* name, which is wrong exactly when the name is escaped | `python.rs`, `python_rt.py` | **0 corpus items** — nothing in the corpus named a keyword, which is why only the name oracle could see it |
| RC-7 | **The Rust emitter's keyword list was missing Rust's reserved words.** `yield` is a legal IDL operation name and `fn yield(&mut self)` does not compile — measured with `rustc`, not supposed | `lib.rs` | every future contract naming one; 0 until the corpus case existed |
| RC-8 | *(oracle)* The driver assumed **the Python method name equals the wire name** — caught within minutes by the corpus case that codified RC-6 | `tests/python_target.rs` | 2 operations |

Four causes are the generator's (RC-1, RC-2, RC-5, RC-6), one is the *other*
target's (RC-7), three are the oracle's (RC-3, RC-4, RC-8).

**The two oracle causes in round 1 mattered as much as the generator's.** RC-3
alone invented about thirty failures. A misaligned check is worse than no check
for the same reason `CLAUDE.md` gives about unmeasured ones: it reports with
confidence, and the confidence is what gets believed.

### Round count: 3

1. **Round 1 — the execution oracle.** Import every generated package, then
   cross-implementation round trip over every named type and every operation.
   Found RC-1, RC-2 (generator) and RC-3, RC-4 (oracle).
2. **Round 2 — the `omniidl -bpython` name oracle**, run after round 1's
   repairs. Found RC-5 and RC-6. Also found three artifacts in the *comparison*
   (nested modules not counted as names in their parent scope; omniORBpy's
   inherited methods invisible in the text; underscore-prefixed methods
   filtered out) — each fixed in the comparison, because a false divergence
   spends the same attention as a real one.
3. **Round 3 — everything re-run, plus the new corpus case.** No new generator
   causes. The corpus case immediately found RC-7 in the *Rust* emitter and
   RC-8 in the oracle, which is exactly what a codification is for.

### Final measurements

```
corpus/golden:   28 file(s), 73 value(s) and 100 call(s) crossed to Python and back, 0 divergence(s)
corpus/services:  1 file(s), 12 value(s) and 12 call(s) crossed to Python and back, 0 divergence(s)
```

`omniidl -bpython` name conformance over `corpus/golden`, comparing text output
only (nothing from omniORB is imported, linked or copied):

```
scope names agreed:     129     only ours: 0
operation names agreed: 100     only ours: 0
only omniidl: 38 × <module>__POA.<Interface>   servants — scoped out, §5
              4 items + 5 operations           our skips, each with a printed reason
```

**Zero names exist on our side that omniidl does not also produce**, and every
name omniidl produces that we do not is either a servant (scoped out) or an
item we skipped out loud.

### The live call — the real proof

A generated Python client, against the stock omniORB fixture in `spikes/`,
through `orbweaver-py-bridge`. **Twelve cases, all passing on the first
attempt:**

```
  ok   ping() -> 42
  ok   add(1000000, 337) -> 1000337
  ok   echo_string('generated python') -> 'generated python'
  ok   scale(1.5, 4.0) -> 6.0
  ok   echo_ragged(Ragged(...)) -> Ragged(a=170, b=-7, c=9, d=2.5, e=187)
  ok   echo_wstring('정적 스텁') -> '정적 스텁'
  ok   blob(64) -> b'\x00\x01\x02...'
  ok   blob_sum(blob) -> 2016
  ok   echo_any((double, -0.125)) -> ('double', -0.125)
  ok   get_self() is a handle -> True
  ok   same_as(get_self()) -> True
  ok   a forged handle is refused: no reference is held under handle "local-9999"

python target: PASS
```

Every alignment case, the codeset path, an `any`, an octet sequence, and the
reference path including its refusal. The machine-wide harness lock was taken
for this measurement and released after it; another agent's harness held it
first and this run waited rather than removing it.

**Timing, one-sided and so labelled:** 500 `ping()` calls through the seam cost
0.097 ms each (debug build of the bridge, loopback, same machine). **No
baseline was measured** — the same calls through the Rust stub were not timed —
so this number says the seam is not obviously a bottleneck and says nothing
about how it compares. D007 asks for exactly that comparison before option B is
re-opened, and this is not it.

---

## 3. What the second target found about the first

This is the part that justifies the exercise.

1. **The `Ref` suffix is Rust's, not the mapping's** (RC-5). `pub type MoneyRef
   = rt::ObjRef;` is right for Rust and was copied into Python because it was
   there, not because the mapping says so. One target could never have told
   the difference.

2. **The Rust keyword list had never been executed** (RC-7). It listed used
   keywords and not reserved ones, so a contract with an operation named
   `yield` would have generated Rust that does not parse. Nothing in the corpus
   had ever named a keyword in *any* target language — writing a second
   emitter, with a second keyword list, is what made that absence visible.

3. **AnyJSON has no form for `::CORBA::TypeCode`**, and that is not a Python
   limitation. §4.5 maps *values*; a `TypeCode` is a description of a type. The
   consequence is larger than this target: `corpus/services/ir_subset` — the
   Interface Repository subset — loses **10 items**, including
   `InterfaceDef::describe_interface`, and the same gap applies to the MCP
   agent path, which speaks the same mapping. **An agent cannot read an
   Interface Repository through AnyJSON either.** Recorded, not fixed: changing
   §4.5 is a specification decision, not a generator change.

4. **A declared bound lives in the Rust type and nowhere in the Python one.**
   `sequence<octet, 64>` is `rt::Bounded<Vec<u8>, 64>` in Rust, where the check
   is in one `Cdr` impl. In Python the bound is a fact in the descriptor and
   the refusal happens at the seam, where the dynamic path already checks it.
   Deliberate: adding a second check in Python would make the two targets
   refuse different sets of values, which is the divergence D006 §2 measured
   pointing the other way.

5. **A single-value operation returning a nil reference is indistinguishable
   from `void`** in the Python API — both answer `None`. Stated rather than
   fixed: the caller knows the contract statically, and a wrapper object to
   disambiguate would cost every call to remove an ambiguity nobody can hit
   without already knowing which operation they called.

---

## 4. The mapping, and where it deliberately diverges

Implemented against the OMG Python language mapping, and **compared** against
`omniidl -bpython` (never derived from it — omniORBpy is LGPL, and only its
text output was read):

| IDL | Python | Agrees with omniidl |
|---|---|---|
| module | package (`pkg/<module>/__init__.py`) | yes |
| struct / exception | class, members in declaration order in `__init__` | yes |
| enum | class + enumerators as objects **in the enclosing scope** | yes |
| union | `_d` / `_v` plus named branch accessors | yes |
| interface | one class per interface | **name yes, shape no** — see below |
| attribute | `_get_x()` / `_set_x()` | yes |
| operation with `out`/`inout` | tuple: result, then outs in declaration order (§7.9.1) | yes |
| identifier that is a Python keyword | **leading** underscore (`_yield`) | yes (after RC-6) |
| `sequence<octet>` | `bytes` | — |
| object reference | `_rt.ObjectRef` handle, **never an IOR** (§4.7) | no: omniORBpy hands out proxies |

Two deliberate divergences, both about the same missing thing — there is no ORB
in this Python process:

- **The stub is constructed, not narrowed.** omniORBpy separates `Echo` (the
  type) from `_objref_Echo` (the proxy you get from `narrow`). Here there is no
  narrowing to get one from, so `spike.Echo(invoker)` is the constructor.
- **Inherited operations are flattened**, not expressed as Python inheritance.
  The resolved member set is the same set the Rust stub is built from, so one
  interface cannot answer for two different surfaces depending on which target
  generated it. The cost is that `isinstance(derived, Base)` does not hold.

And one measured curiosity, kept out of the corpus deliberately: for an
attribute named `pass`, `omniidl -bpython` emits `pass = property(_get_pass)`,
which is **not valid Python** in any version. The oracle is not always right,
which is the argument for having more than one.

---

## 5. What was scoped out, and why

- **Servants.** This is a client target. A Python servant needs the bridge to
  accept connections and call *back* into Python, which doubles the protocol
  and gives the seam a second direction to be wrong in, while `skeleton.rs`
  already answers for the serving half of every contract. This is the single
  largest omission and it is the 38 `__POA` entries in the name-oracle table.
- **`valuetype`, abstract interfaces, `fixed`.** §4.4 defers them at the wire;
  the Python target skips them for the same reason and prints it.
- **`::CORBA::TypeCode` as a value.** §4.5 has no form for it (finding 3).
- **`any` of a constructed type on the way *in*.** `anyjson::from_json` accepts
  only primitives in an `any`; the Python side mirrors that limit exactly
  rather than inventing a wider one.
- **Dialing a returned reference.** A handle can be passed back as an argument
  to the bridge that issued it; it cannot be dialled, stored across bridge
  lifetimes, or narrowed. That is §4.7 working as designed, not a gap to close.
- **A guarded seam.** The bridge is *not* a security boundary; `orbweaver-mcp`
  is. A second, weaker gate at the bridge would be the §4.7 bypass in a new
  place.

---

## 6. What was codified

| Cause | Codified as |
|---|---|
| RC-1, RC-2, RC-5 | `tests/python_target.rs` — the cross-implementation round trip over the whole corpus, both byte orders. Any recurrence fails a test rather than a user |
| RC-3 | the driver appends a placeholder for a failed item, so positions hold |
| RC-4 | the plan is built from what the generator emitted, never from what it skipped |
| RC-6, RC-7 | **`corpus/golden/28-target-keywords.idl`** — the first contract in this project to name a target language's reserved words, plus `python::python_name` made public so the escaping is part of the mapping rather than an implementation detail |
| RC-8 | the oracle now asserts that the request names the **IDL** operation even when the Python method is escaped |
| the seam decision | `docs/decisions/D007-python-wire-seam.md`, PROPOSED |

**Proposed for `CLAUDE.md`** (not applied — that file is outside this
footprint), under *IDL rules the compiler enforces*:

> - **A target language's reserved words are the generator's problem, not the
>   contract's.** `yield`, `lambda` and `None` are legal IDL and reserved
>   somewhere; every emitter escapes, and until
>   `corpus/golden/28-target-keywords.idl` existed no emitter's escaping had
>   ever been executed — the Rust list was missing `yield`. Adding a target
>   means adding its keyword list to that file's coverage.

---

## 7. Gates

```
cargo test --workspace            1143 passed, 0 failed   (1139 before; +4 python target)
cargo fmt --check                 clean
cargo clippy --workspace --all-targets   0 warnings
RUSTFLAGS="-D warnings" cargo test -p orbweaver-gen       green
unsafe_code = "forbid" / #![deny(missing_docs)]           unchanged
./spikes/run_checks.sh            exit 0 — all measured checks green,
                                  12 group(s) SKIPPED (JacORB, TAO, docker,
                                  VOYAGE_API_KEY absent — pre-existing)
```

The harness confirms the new corpus file went through every gate that owns it:
*"accepts all 28 golden files"*, *"contract-check: 28 file(s), 78 type(s) × 32
case(s) × 2 byte orders: 0 property defect(s)"*, and — the one that matters for
RC-7 — *"every generated stub compiles outside the workspace / and under
-D warnings"* with `yield` in the corpus.

**The first harness run of this session failed three groups and every one was a
disk, not the code.** The machine ran out of space mid-run (`rustc-LLVM ERROR:
IO failure on output stream: No space left on device`), which surfaced as *"the
servant does not compile against the generated trait"*, *"generated code does
not compile"* and *"idl-diff refuses a revision that breaks nothing"* — three
plausible-looking regressions, none real. Recorded because the failure mode is
worth recognising: **a full disk fails as a compilation error and a wrong
verdict, not as a disk error**, and two of those three messages name a
subsystem that was working. After freeing space the same harness exited 0 with
nothing changed.

### Harness lines this change wants (apply from main)

```bash
# ── Python target: the generated client against the omniORB fixture ─────────
hr "Python client target (stream B, second language)"
start_server   # the omniORB fixture, NOT start_our_server
rm -rf /tmp/orbweaver-pytarget && mkdir -p /tmp/orbweaver-pytarget
cargo run -q --bin gen-python -- --out /tmp/orbweaver-pytarget spikes/echo.idl
cargo build -q --bin orbweaver-py-bridge
out=$(python3 crates/orbweaver-gen/python/echo_client.py \
        /tmp/orbweaver-pytarget spikes/echo.idl spikes/echo.ior \
        ./target/debug/orbweaver-py-bridge 2>&1)
echo "$out" | tail -20
case "$out" in *"python target: PASS"*) : ;; *) fail_total=$((fail_total+1));; esac
cleanup

# ── Python target: every golden contract generates and imports ──────────────
rm -rf /tmp/orbweaver-pybatch && mkdir -p /tmp/orbweaver-pybatch
cargo run -q --bin gen-python -- --out /tmp/orbweaver-pybatch corpus/golden/*.idl >/dev/null
imported=$(cd /tmp/orbweaver-pybatch && python3 -c '
import importlib, pathlib, sys
sys.path.insert(0, ".")
ok = 0
for d in sorted(p.name for p in pathlib.Path(".").iterdir() if p.is_dir()):
    importlib.import_module(d); ok += 1
print(ok)')
if [ "$imported" -lt 28 ]; then fail_total=$((fail_total+1)); fi
echo "  generated Python packages imported: $imported"
```

---

## 8. 한국어 요약 / Korean summary

**무엇을 만들었나.** `orbweaver-gen`이 러스트에 이어 **파이썬 클라이언트**를
생성한다. 목적은 파이썬이 아니라, 타깃이 하나뿐이면 *IDL 매핑*과 *러스트에서
편했던 것*을 구분할 수 없다는 데 있다. 매핑을 두 번 쓰는 것만이 그 구분을 만든다.

**와이어는 파이썬에 없다.** 생성된 파이썬은 인자를 **AnyJSON v1**(§4.5, 이
프로젝트의 정식 JSON↔CDR 매핑)로 만들어 `orbweaver-py-bridge` 프로세스에 넘기고,
그 프로세스가 기존 동적 경로로 호출한다. CDR·GIOP·정렬·바이트 순서·코드셋은 여전히
러스트에 **한 번만** 존재한다. 중복되는 것은 `_rt.py`의 §4.5 구현 하나뿐이며,
§4.5는 왕복 수용 기준을 가진 명세이므로 **검사 가능한** 중복이다. 확장 모듈(PyO3)과
순수 파이썬 CDR/GIOP 대안은 **D007(제안 상태)**에 비교해 두었다 — 새 의존성 종류와
경계에서의 `unsafe_code` 문제는 코드를 써서 채택할 일이 아니다.

**배치 결과.** 골든 27건 일괄 생성, **1차 통과율 21/27 = 77.8%**, **라운드 3회**.
근본원인 8건: 생성기 4건(RC-1 열거자 소유 식별 방식 불일치, RC-2 base64를 배열에도
적용, RC-5 러스트용 `Ref` 접미사를 파이썬에 복사, RC-6 키워드 이스케이프 방향),
러스트 생성기 1건(RC-7 예약어 목록 누락 — `yield`), 오라클 3건(RC-3 실패 항목을
건너뛰어 이후 전부 오정렬, RC-4 스킵 항목까지 검사, RC-8 와이어 이름과 파이썬
메서드 이름을 동일 취급). **RC-3 하나가 가짜 실패 약 30건을 만들었다** — 어긋난
검사는 검사가 없는 것보다 나쁘다.

**최종 측정.** 골든 28파일 73값·100호출, 서비스 1파일 12값·12호출, **발산 0**.
omniidl 이름 대조: 스코프 이름 129건, 오퍼레이션 이름 100건 일치, **우리 쪽에만
있는 이름 0건**. omniidl에만 있는 것은 서번트(범위 밖) 또는 이유를 출력한 스킵뿐.
**실호출**: 생성된 파이썬 클라이언트가 omniORB 픽스처를 상대로 12건 전부 1차 통과
(정렬·코드셋·any·octet 시퀀스·참조 핸들과 위조 핸들 거부 포함).

**두 번째 타깃이 첫 번째에 대해 알아낸 것.** ① `Ref` 접미사는 매핑이 아니라
러스트의 사정이었다. ② 러스트 키워드 목록은 **한 번도 실행된 적이 없었고**
`yield`가 빠져 있었다 — 컴파일되지 않는 코드를 생성했을 것이다(rustc로 실측).
③ **AnyJSON에는 `::CORBA::TypeCode`의 형태가 없다.** 파이썬의 한계가 아니라 §4.5의
한계이며, 같은 매핑을 쓰는 MCP 에이전트 경로도 **인터페이스 저장소를 읽을 수 없다**
(`ir_subset` 10개 항목 손실). 기록만 하고 고치지 않았다 — 명세 결정이다.

**범위 밖(이유와 함께).** 서번트(클라이언트 타깃이며, 브리지가 파이썬을 역호출해야
해 프로토콜이 두 배가 된다), `valuetype`/추상 인터페이스/`fixed`(§4.4), 값으로서의
`TypeCode`(§4.5), 반환된 참조로 직접 다이얼(§4.7 설계대로), 브리지의 보안 게이트
(그것은 `orbweaver-mcp`의 몫).

**영구화한 것.** 전 코퍼스 교차 구현 왕복 오라클, 스킵/정렬을 존중하는 드라이버,
그리고 **`corpus/golden/28-target-keywords.idl`** — 이 프로젝트에서 타깃 언어의
예약어를 이름으로 쓴 최초의 계약. `CLAUDE.md`에 넣을 규칙 초안은 §6에 있다(해당
파일은 이 작업의 범위 밖이라 적용하지 않았다).

**게이트.** `cargo test --workspace` 1143건 통과, `cargo fmt --check` 무결,
`clippy` 경고 0, `-D warnings`로 `orbweaver-gen` 통과, `./spikes/run_checks.sh`
**exit 0 — 측정된 검사 전부 통과**(12개 그룹은 픽스처 부재로 SKIP, 기존과 동일).

첫 하네스 실행은 3개 그룹이 실패했는데 **전부 디스크 문제였다**. 실행 도중 머신
디스크가 가득 차(`No space left on device`) 컴파일 오류로 표면화되었고, 그 결과
"서번트가 생성된 트레이트에 대해 컴파일되지 않는다", "생성 코드가 컴파일되지
않는다", "idl-diff가 깨뜨리지 않는 개정을 거부한다"라는 **그럴듯하지만 사실이 아닌**
판정 3건이 나왔다. 공간을 확보한 뒤 코드 변경 없이 같은 하네스가 exit 0으로
통과했다. **꽉 찬 디스크는 디스크 오류가 아니라 컴파일 오류이자 잘못된 판정으로
나타난다**는 사실은 기록할 가치가 있다.
