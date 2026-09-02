# Orbweaver — working instructions

AI-driven CORBA/IDL interface automation. An MIT ORB core written against the
published OMG wire specification, plus an AI specification pipeline on top.

**Priority zero — what "a finished ORB" means.** Set by the project owner
2026-08-26. Its home is `docs/decisions/D029-what-a-complete-orb-would-mean.md`
§6 and it is **not restated here**: read it there. In one line, so you know
whether your work touches it — *the ORB is complete when there is no leak in
the transparency that a caller can invoke any target holding only a reference,
knowing nothing of its location, backend, language or load state, and that
this survives targets being added, removed, moved, loaded or evicted at
runtime.* Every plan document is subordinate to it and each records how its own
work bears on it. **Transparency is not confirmed, it is hunted** — a proposal
that closes a leak outranks one that adds a capability.

*0순위 기준의 집은 D029 §6이다. 여기서 다시 적지 않는다 — **투명성은 확인하는 것이
아니라 구멍을 사냥하는 것**이며, 구멍을 막는 제안이 기능을 더하는 제안보다 앞선다.*

Plan: `docs/PLAN.md` (EN) · `docs/PLAN.ko.md` (KO) · Phase 0 results: `docs/PHASE0.md`

---

## The operating model: batch → oracle → repair → codify

**Work the whole set at once, verify the whole set at once, fix by root cause,
then make the cause impossible.** Never item-by-item.

**전체를 한 번에 작업하고, 한 번에 검증하고, 근본원인 단위로 고치고, 그 원인이
다시 발생하지 못하게 만든다.** 건건이 처리하지 않는다.

This is not a style preference. Phase 0 measured it: 20 IDL files generated in
one pass produced 7 failures, and **all 7 had a single root cause**. Item-by-item
work would have produced 7 separate patches and never surfaced the rule.
Batching made the cause visible; one fix took the batch from 65% to 100%.

이것은 취향이 아니다. Phase 0이 측정했다: 20건 일괄 생성 → 실패 7건 → **7건 전부
동일한 근본원인**. 건건이 고쳤다면 패치 7개가 나왔을 뿐 규칙은 드러나지 않았다.

### The four steps

1. **Batch** — produce every item in the set in one pass. **Do not consult the
   oracle mid-pass.** Peeking contaminates the first-pass measurement and, worse,
   lets you patch symptoms one at a time so the shared cause never appears.
   *일괄 생성 중에는 오라클을 보지 않는다. 중간에 보면 1차 통과율이 오염되고
   공통 원인이 드러나지 않는다.*

2. **Oracle** — run every deterministic check across the whole batch at once.
   Then **cluster the diagnostics by root cause, not by item.** The output of
   this step is a list of causes with their affected items, never a list of items.
   *진단을 항목이 아니라 근본원인으로 묶는다.*

3. **Repair** — one fix per root cause, applied across every affected item.
   If a "fix" only helps one item, it is not a root-cause fix; find the real
   cause or record it as a genuine one-off with a reason.
   *원인 하나당 수정 하나. 한 항목에만 듣는 수정은 근본 수정이 아니다.*

4. **Codify** — turn every confirmed cause into something permanent: a lint
   rule, a prompt constraint, a corpus case, or a rule in this file. A cause
   that is only fixed will come back; a cause that is codified cannot.
   *확인된 원인은 반드시 영구 산출물로 바꾼다. 고치기만 한 원인은 돌아온다.*

Repeat until a round yields no new root causes. Report the first-pass rate and
the round count separately — they measure different things.

### Reporting a batch

Always state: batch size, first-pass rate, root causes found (with affected
counts), what was codified. Never report only the final number — the first-pass
rate is the signal about the generator, the round count is the signal about the
oracle.

---

## Hard rules

### Licensing boundary — non-negotiable / 라이선스 경계 — 타협 불가

This project ships MIT. omniORB, ACE/TAO and JacORB are **LGPL/GPL/DOC and are
fixtures, never dependencies.**

- **Never** `import`, link, vendor, copy from, or redistribute any part of them.
- They may only be (a) run as separate-process wire peers over TCP, and
  (b) invoked as external programs whose text output we read (`omniidl` as a
  conformance oracle).
- `cargo tree` must stay free of them. Anything under `crates/` is original work
  written against the OMG specification.
- CI images containing them are built or pulled inside CI and **never published**
  as project artifacts — publishing is redistribution.

**Amended 2026-08-12 (D001, approved).** MIT for everything we write. Where a
component is **data we cannot originate** — a character mapping table, a
timezone database — permissive-with-attribution is accepted, disclosed in
`NOTICE`, and recorded under `docs/decisions/`.

The distinction is the point, not a loophole. Logic defined by a published
specification we implement ourselves and owe nobody for; that is why the ORB
core is first-party. A mapping table is somebody's compilation of facts with no
specification to implement from, so retyping it produces the same derived work
rather than an original one — *a table derived from an incompatibly-licensed
source is not laundered by being retyped.*

우리가 쓰는 것은 MIT. **우리가 원저작할 수 없는 데이터**는 귀속 표시 조건의 관대
라이선스를 허용하되 `NOTICE`에 공개하고 결정으로 기록한다. 로직과 데이터의 구분이
핵심이며 빠져나갈 구멍이 아니다.

Currently accepted under this clause: `encoding_rs` for EUC-KR, behind the
default-on `euc-kr` feature. `--no-default-features` removes it and the
obligation. Both configurations are tested by `run_checks.sh`.

Before adding any dependency, check its licence against this rule — and check
the provenance of its *data*, not only its declared licence. A crate declaring
MIT over a table it does not account for is a worse position than an honestly
disclosed BSD-3-Clause.

### IDL rules the compiler enforces / 컴파일러가 강제하는 IDL 규칙

- **Identifier clashes are case-insensitive.** A member, parameter or operation
  may not share a name with a type or an enclosing scope, ignoring case.
  `Position position`, `Value value`, `module inventory { interface Inventory }`
  and `struct Version { unsigned long version; }` are all illegal. This is
  natural naming in every other language, which is exactly why it is the
  dominant generation failure. *가장 흔한 실패 원인.*
  Run `cargo run -q --bin sidl-validate -- <files>` before the oracle — it
  catches this class with an actionable message. (The regex lint
  `spikes/idl_lint.py` this rule used to name retired in Phase 2 batch 2, when
  semantic analysis subsumed it; the instruction outlived the file by several
  phases, which is its own small lesson about codifying a command instead of a
  capability.) Documenting the rule does not prevent it:
  it has since caught two corpus files and two fixtures, one written by someone
  who had just described the rule in that same file's header.
- **A target language's reserved words are the generator's problem, not the
  contract's.** `yield`, `lambda` and `None` are legal IDL and reserved
  somewhere. Every emitter escapes them, and until
  `corpus/golden/28-target-keywords.idl` existed no emitter's escaping had ever
  been *executed* — the Rust list was missing `yield`, so `fn yield()` was
  emitted and did not compile. Adding a target means adding its keyword list to
  that file's coverage. *대상 언어의 예약어는 계약이 아니라 생성기의 문제다.*
- `TypeCode` must be qualified as `::CORBA::TypeCode`.
- v1 wire support excludes `valuetype`, abstract interfaces and `fixed`. The
  parser accepts them; the wire does not. See `docs/PLAN.md` §4.4.
- SIDL v1 uses **structured comments** (`//@ ai_desc: ...`), not IDL 4
  `@annotation` — deployed compilers reject the latter (Phase 0 assumption C).

### Wire and test rules / 와이어·테스트 규칙

- **Compare decoded values, never raw buffers.** CDR padding content is
  undefined by the specification and omniORB does not zero it, so byte-for-byte
  message comparison against a reference ORB produces false failures.
- **Test both byte orders.** An encoder that only works native-endian passes
  every local test and fails in the field.
- **Alignment origin matters.** A GIOP message aligns from the first byte of its
  12-byte header; an encapsulation restarts alignment at its own first byte.

### Harness rules / 하네스 규칙

Each of these produced a phantom failure during Phase 0. They will recur.

- **Wait loops must sleep.** `for i in $(seq 1 500); do [ -f f ] && break; done`
  finishes in microseconds and does not wait at all. This caused the initial
  assumption A failure; the protocol was correct the whole time.
- **Never pipe into `grep -q`** when the producer matters, and know that there
  are **two** independent ways such a pipeline lies. `grep -q` exits on first
  match and SIGPIPEs upstream (141). And under `set -o pipefail` — which
  `run_checks.sh` sets on line 9 — a pipeline's status is its *rightmost
  non-zero* exit, so **a producer that fails makes the pipeline fail, and an
  `if` reads that as "no match".** Measured 2026-08-25, in this harness's own
  first three gates: `cargo test --workspace --quiet 2>&1 | grep -q "^error"`
  printed **`ok cargo test --workspace` over a red workspace** for as long as
  it had existed — its FAIL branch required `cargo test` to *pass* while
  printing an error line — and `cargo tree --workspace | grep -qiE
  "omniorb|jacorb"` could not report a forbidden dependency, because finding
  one is exactly when SIGPIPE fires. **The licence boundary this file calls
  non-negotiable had a gate that could not go red.** A short producer fits the
  64 KB pipe buffer and never sees SIGPIPE, which is why hand-checking it
  always looked fine. Capture to a variable, then match with a **herestring**
  (`grep -q … <<<"$out"`) — never `printf … | grep -q`, which this file called
  the sanctioned form and which **has the same defect**: capturing the output
  first saves the *data*, and `grep -q` still exits early, still SIGPIPEs the
  `printf`, and `pipefail` still turns that into "no match". Swept the same
  day: **76 of them in this harness**, and the one that mattered was the
  concurrent-dispatch group's own `printf '%s' "$cd_out" | grep -q "^test
  result: FAILED"` over three crates' test output — non-deterministically, by
  where in the output the failure fell. A group whose whole argument is *"five
  runs, because one green run is not evidence"* could not see a failing run
  when the failure came early. Also **read the producer's own exit status
  first** — a producer that could not run at all is an unmeasured check, which
  is a failure and never a pass. A `grep` without `-q` reads its whole input,
  never SIGPIPEs, and is safe in a pipeline; only the early-exit forms (`-q`,
  `-m1`, `head`) are the hazard. *두 가지 방식으로 거짓말한다 — SIGPIPE와
  `pipefail`. 변수로 캡처해도 파이프면 똑같이 거짓말한다; herestring을 쓴다.
  "다섯 번 도는 이유는 한 번의 초록이 증거가 아니기 때문"이라던 그룹이 바로 그
  형태 때문에 실패한 실행을 못 보고 있었다.*
  **What fires it is a complete line, not a full buffer.** `grep -q` cannot
  decide before it has a whole matching line, so the early exit that kills the
  producer only happens once one has arrived. Measured 2026-08-27: a marker
  alone on the first line lies from ~96 KB of tail, and *races* at 64 KB —
  status 141 while the `if` still took THEN. **The same marker at the front of
  one unbroken 1 MB line does not lie at all**, because grep must read the line
  to its end. That is why, in a five-site sweep, `spikes/nat/vm/run.sh`'s two
  checks were the pair telling the truth — a stringified IOR is one line — and
  it is why hand-checking these always looks fine. Convert them anyway: the
  form is not a judgement call about today's payload. But **do not record a
  site as "was lying" without measuring it.** *방아쇠는 버퍼가 아니라 완전한 한
  줄이다. 표지가 첫 줄에 단독이면 꼬리 ~96 KB부터 거짓말하고 64 KB에서 경합하지만,
  끊기지 않은 1 MB 한 줄의 앞에 있으면 거짓말하지 않는다 — grep이 줄 끝까지 읽어야
  하기 때문이다. 형태는 그래도 바꾸되, 재보지 않고 "거짓말하고 있었다"고 적지
  않는다.*
- **A sweep is scoped to a rule; a sweep that names a file will sweep that
  file.** 2026-08-25's `printf | grep -q` sweep reported **76 instances** and
  touched **only `spikes/run_checks.sh`**. Five survived in three scripts it
  never opened, one of them carrying a comment asserting immunity in exactly
  the words this file refutes. The number was true and useless — **it measured
  the file, not the rule.** A sweep lands with the scan that produced it,
  runnable over the whole tree, or the next reader inherits a count instead of
  a check. **And the unit a rule is asked of is not always a file.** Measured
  2026-08-28, by the gate written that morning to close this very class: it
  asked whether a *file* mentions `orbexit`, and printed `27 fixture(s) create
  an ORB; 27 leave through orbexit.leave` while **eight programs those files
  carry as string constants** — `-c` children run with `[sys.executable, "-c",
  …]`, of which eight of nine call `ORB_init` — did not. A crash report four
  hours after the first one named the difference: `Parent Process: Python`, and
  thread 0 in `__cxa_finalize_ranges`, which `os._exit` does not reach. *A rule
  about programs, checked against files, is green over every program a file
  carries.* The repair is to ask the thing the rule is about — here **the
  launch, never the string**, because that holds whether the child is a name, a
  literal or a template assembled at run time. The first rewrite got that wrong
  too and walked the AST for string constants that parse as Python: it found
  **nothing** in the one fixture whose child is templated, and stayed green
  when a spawn site was unwrapped. Only the control said so. That scan is now a group in `run_checks.sh`, over `git ls-files`
  rather than a directory walk, with a synthesised two-line probe — the defect
  and its repair — that it must report as exactly one hit before its silence
  over the tree is allowed to mean anything. *스윕의 범위는 규칙이지 파일이
  아니다. 76은 참이었고 쓸모없었다 — 규칙이 아니라 파일을 잰 수였기 때문이다.
  스윕은 그것을 만든 스캔과 함께, 트리 전체에서 돌 수 있는 형태로 착지한다.
  그리고 **규칙이 묻는 단위가 항상 파일인 것은 아니다** — 프로그램에 대한 규칙을
  파일에 대고 물으면, 파일이 품은 모든 프로그램 위에서 초록이 된다. 문자열이
  아니라 **기동**에 물어야 하며, 그 재작성의 첫 판도 대조군이 죽였다.*
  **And two gates can be green with neither being wrong, when each is scoped to
  a place and the rule is about a claim.** Measured 2026-09-02: `gap_symbols.py`
  printed `22 symbol(s) named by gap columns, 22 exist in the tree` while four
  sites named a type renamed the day before — it asks its question of the gap
  **columns**, and the stale names were in prose, a table row and a plan
  document. `cited_and_run.py` printed `0 owe a group` while **two rows of
  `COMPONENTS.md`** said a spike was *"not yet a `run_checks.sh` group"* over a
  group that had been running for days — it reads spike **headers**, and the IOU
  was in the document. Neither gate could have caught the other's miss and
  neither was mistuned. `spikes/doc_symbols.py` asks it of the claim, and its
  drafts are the lesson: the first reported **0 over the known defect**, because
  the renamed leaf still occurred in a comment recording the rename — **an
  occurrence is not a definition**; the second went silent whenever a crate was
  deleted, having derived what-we-own from what currently exists, and only
  control 5 said so; the third reported a rename record whose verb had wrapped
  to the next line, because it read the document line by line and markdown wraps
  where it likes. *두 게이트가 초록이면서 둘 다 틀리지 않을 수 있다 — 각자
  **장소**에 범위를 맞추고 규칙은 **주장**에 대한 것일 때다. 초안 셋이 교훈이다:
  **출현은 정의가 아니고**, "우리 것"을 현재 존재하는 것에서 끌어내면 지워질 때
  조용해지며, 마크다운은 아무 데서나 줄을 바꾼다.*
- **Never edit a script while it is running.** `bash` reads a script
  incrementally, so an edit that shifts byte offsets can make a running shell
  resume at the wrong place. Done 2026-08-25 — three gate repairs written into
  `run_checks.sh` while a 43-minute run of it was in flight. Nothing visibly
  broke, and that is the problem: **whether the run was affected cannot be
  established after the fact**, so its verdict stopped being evidence and the
  run had to be repeated. Wait for the lock, or copy the script. *실행 중인
  스크립트를 편집하지 않는다. 영향 여부를 사후에 증명할 수 없으므로 그 실행의
  판정은 증거가 되지 못한다.*
- **A run's inputs are wider than its script, and that rule above is the
  narrow statement of this one.** Measured 2026-08-27, twice in one run, by
  the person who had just quoted the rule. A `/tmp` sweep during a harness run
  deleted `/tmp/orbweaver-f5-hold.log` — the file a wait loop was polling for
  its fixture's `HOLDING` line — and the group reported *"the holding tenant
  service never came up"* over a fixture that was up and had already written
  both its IORs. An age cutoff does **not** close this: the sweep stats the
  *stale* file, the fixture recreates one at the same path, and `rm` acts on
  the **path**, not on the inode that was measured. **The harness's fixture
  state lives in `/tmp` under the same `orbweaver*` prefix any cleanup
  targets**, so reclaim only when the lock is free — the lock was read and
  printed minutes before the sweep. Separately, a README commit landed mid-run
  and tripped `decision_status.py`, which reads it through `ROOT.glob("*.md")`;
  the check that had "proved" README was safe was a grep for the string
  `README` in the gates, and **a search for a filename cannot find a glob.**
  Two more from the same hour: `find /tmp` on macOS silently returns nothing
  because `/tmp` is a symlink (`ls` saw 61 files, `find` saw 0, and the
  cleanup reported success having done nothing — use `/private/tmp`), and a
  pipeline's exit code is its last stage's, so `script | head` reported `0`
  for a script that exits 4. *실행의 입력은 스크립트만이 아니다 — `/tmp`의 픽스처
  상태, 루트의 `*.md`, 포트를 쥔 프로세스가 모두 그 실행의 입력이다. 나이 경계로는
  TOCTOU가 닫히지 않는다: 낡은 파일을 재고 픽스처가 같은 **경로**에 새로 만들면
  `rm`은 새것을 가져간다. 락이 비었을 때만 회수한다. 그리고 **파일 이름을 찾는
  검색은 글로브를 찾지 못한다.***
- **Reaping a child is not reaping its tree.** Measured 2026-08-27:
  `orbweaver-py-bridge` leaked **twelve processes from one harness run** and
  fifty more from the days before, every one `ppid=1` and each holding a
  loopback port. The chain is `cargo test → python3 → the bridge`, and three
  layers each did their job inside their own scope: `python_rt.py` wrote a
  correct `close()` and a docstring naming *"forty orphaned peers"* as the
  cost of leaking one, but nothing ever called it — no `atexit`, no handler,
  no `__del__`; the Rust test owned its child *"so that every exit still kills
  it"*, which is correct one level above the leaf and killed with SIGKILL
  besides, which no handler can catch; and the harness `fkill`s every fixture
  it starts, which this one is not. **Nobody owned the span.** A test that
  leaks four bridges was green both before and after the fix — the leak was
  never visible to it, which is why the control counts processes rather than
  reading a verdict. Spawn long-lived children with `process_group(0)` and
  signal the **group** in `Drop`; register the runtime's own `close()` with
  `atexit` for the paths where the child exits by itself. *자식을 거두는 것은
  나무를 거두는 것이 아니다. 세 계층이 각자 자기 범위 안에서 옳았고, 그 사이를
  아무도 소유하지 않았다. 누수하는 테스트는 수정 전후 모두 초록이었다 — 그래서
  대조군은 판정을 읽지 않고 프로세스를 센다.*
- **A cleanup step that silently does nothing makes the next one load-bearing,
  and the next one is usually the dangerous one.** Measured 2026-08-30.
  `PythonChild::drop` began `let _ = self.child.stdin.take();` under a comment
  reading *"Close stdin first"* — but `spawn` had already taken it, the live
  `ChildStdin` was in `self.stdin`, and **the line closed nothing**. So the
  child never saw EOF, never left by its own route, and the `kill -TERM
  -<pgid>` underneath it became the thing actually reaping — a **process
  group** signal, in a file whose child spawns nothing and therefore has no
  tree. CI went from green in 22–29 minutes to **cancelled at four**, every run
  after the commit that added the type, all three as `cargo test --workspace`
  began, with the other two jobs green in each; **four harness runs here were
  green over the same tree**, which is the `ppid=1` shape again — it does not
  show locally. *Reaping a child is not reaping its tree* is a rule about
  children that HAVE trees; applying it where there is none buys nothing and
  puts the only cross-boundary signal in the file. NOT DIAGNOSED — a runner
  cannot be reproduced from here — and the honest claim is the checkable one:
  the only code that could signal outside its own child was removed, the no-op
  that made it necessary was fixed, and CI was asked. It went green.
  *조용히 아무것도 하지 않는 정리 단계는 다음 단계에 하중을 싣고, 그 다음 단계가
  보통 위험한 쪽이다. 주석은 "stdin을 먼저 닫는다"였고 그 줄은 아무것도 닫지
  않았다. 나무가 없는 자식에게 그룹 신호를 쓰는 것은 사는 것 없이 파일에서 유일한
  경계 밖 신호를 두는 일이다. 로컬 네 번 초록, CI 세 번 4분 취소.*
- **`ppid=1` is not a proof of ownership, and the sentence that says it is was
  written by the person repairing the leak above.** That backstop's premise —
  *"a process in our group whose parent is init; neither this shell nor its
  ancestors can match, because they have real parents"* — is false in its last
  clause. **An ancestor is reparented to init the moment *its* parent exits**,
  so it carries `ppid=1` while still leading the process group this shell
  inherited, and it lands in the candidate set. That is the shape of a CI
  runner, and the cost was measured: every run after the backstop landed died
  with `Terminated` and `The operation was canceled` at whatever group called
  `cleanup` next, for two days, while the run immediately before it had taken
  the same harness to completion in 23 minutes. **It never showed locally,
  because a Terminal's `zsh` has a live parent** — the same green-here-red-
  there shape as the `mktemp -t` scan. Compute ownership by **walking this
  shell's ancestor chain and excluding it**, in one function that both the
  reaping half and the counting half call: they had been restating the same
  false sentence in two places, which is the `pub(crate)` rule wearing shell.
  And the control synthesises the ancestor (fork, let the middle process exit,
  `setpgid(0,0)`, run the subject as its child) — pointing it at a live process
  tree measures today's machine, not the rule. *`ppid=1`은 소유의 증거가 아니다.
  조상은 **그 부모**가 끝나는 순간 init에 재양자되어 `ppid=1`을 달고도 우리 그룹을
  이끈다. 로컬에서는 드러나지 않는다 — 터미널의 `zsh`에는 살아 있는 부모가 있기
  때문이다. 조상 사슬을 **걸어서** 제외하고, 대상은 **합성**한다.*
- **A run that records nothing about its conditions cannot be explained after
  it dies, and the reconstruction that saves you once will not be there twice.**
  Measured 2026-08-27: this machine froze at 15:17:50 KST — the unified log ends
  mid-second, there is no `.panic` report, and `ResetCounter` reads
  `btn_rst,finger_reset force_off`. The cause could be named **only** because
  the kernel happens to stamp `memorystatus_available_pages` onto unrelated
  idle-exit lines: available memory fell **6.42 GB → 0.75 GB in 33 seconds** and
  sat between **0.06 and 0.36 GB** with the compressor holding **11.8 GB of 16**
  for the eight minutes up to the stop. Nothing was recording on purpose. So the
  harness records: `spikes/memlog.sh` appends and flushes every 5s from before
  the first group, and the previous run's trace moves to `.prev` rather than
  being deleted — if a run died, that file is the only account of it. **The
  trace is a report and the kill query is the gate, and they are different
  claims.** There is no defensible number for "too little memory left", so no
  threshold is a gate; what *is* a gate is *did the kernel kill anything for
  memory while this run was measuring*, because a fixture that was shot cannot
  report that it was and an unmeasured check is a failure, never a pass. Two
  traps in that query, both measured here: `memorystatus: killing` is **not**
  the pattern — macOS reaps idle daemons through the same subsystem, **782 lines
  in fifty idle minutes** — so a gate on it is red forever and gets switched
  off, and the probe's second line is an idle-exit that must **not** count. And
  on Linux a window that cannot be applied is exit 3, not a wider answer: an OOM
  kill from last Tuesday reported as this run's is a false red, which kills a
  gate as surely as a true one nobody reads. **State what it does not cover**:
  this covers a run, not a machine — the freeze above happened while no harness
  was running, and what exhausted 16 GB was ~1,700 `node` processes from a Vite
  toolchain in a different repository. *실행 조건을 기록하지 않는 실행은 죽은 뒤에
  설명할 수 없고, 한 번 구해준 운 좋은 복원은 두 번째에는 없다. 추이는 보고이고
  kill 질의가 게이트이며 둘은 다른 주장이다 — "메모리가 너무 적다"에 방어 가능한
  수는 없다. `memorystatus: killing`은 패턴이 아니다(한가한 50분에 782줄). 적용할
  수 없는 창은 더 넓은 답이 아니라 미측정이다. 그리고 **덮지 못하는 것을 적는다**:
  이것은 실행을 덮지 머신을 덮지 않는다.*
- **A gate that enumerates with `git ls-files` reads a different tree before and
  after `git add`, and the run that blessed it was the one where it could not
  see itself.** Measured 2026-08-31. `spikes/ior_wait_shape.py` hunts
  `[ -s x.ior ] && sleep N` and carries that shape twice as probe literals. It
  ran green in harness 54 and red in harness 55 over the same logic, and CI
  failed the commit that landed it — because in 54 the file was **untracked**,
  so `git ls-files` never handed the scan its own source, and `git add` is what
  put it in scope. *The local run that proved the gate was a run against an
  input set the commit did not have.* The repair is the rule, not an exemption:
  the shape is shell syntax, so the scan reads `.sh` and a Python file cannot
  contain a shell wait — checked before narrowing, because narrowing until the
  complaint stops is the tune-until-quiet defect. **`git ls-files` is still
  right** (it is what keeps a scan out of an ignored 532 MB vendor tree); what
  is wrong is validating such a gate before staging it. Stage first, then run
  it. *`git ls-files`로 열거하는 게이트는 `git add` 전후로 다른 트리를 읽고, 그것을
  통과시킨 실행은 게이트가 자기 자신을 볼 수 없던 실행이다. 스테이징한 뒤에 돌린다.*
- **A branch written against an absent oracle has never been executed, and it
  fails the way unexecuted code fails: confidently.** Measured 2026-08-31, the
  first time a `tao_idl` was present on this machine. `differential.sh` had had
  a `tao_idl_verdict` for as long as the script had existed, and two things in
  it were wrong in ways no review had caught. It ended in the bare compiler
  invocation, so it returned **TAO's own exit status — `2` on a parse error** —
  into a protocol `examine` compares with `-ne` against `0` or `1`; the two
  neighbouring verdict functions end in `[ -z "$err" ]` and are normalised by
  construction. And it asked the oracle about **IDL 3**, TAO's default, while
  this corpus is IDL 4.2 — *an oracle configured for another version of the
  specification is not a second opinion about this one.* The first run reported
  **37 unexplained divergences**; those two causes were **29** of them, and the
  real 8 were invisible underneath. The comment above the function asserted the
  exact behaviour that was wrong (*"it sets a non-zero exit status on error,
  which is the verdict"* — true, and not what the code did with it). **A
  `SKIPPED` for an absent fixture says the column is unmeasured; it does not say
  the code that would fill it has never run.** When a fixture arrives, the first
  run is a measurement of the harness, not of the tree. *부재한 오라클을 상대로
  쓰인 분기는 실행된 적이 없고, 실행되지 않은 코드가 그렇듯 자신 있게 틀린다.
  종료 코드 2가 0/1 프로토콜로 새고, 코퍼스가 4.2인데 오라클엔 IDL 3을 물었다 —
  37건 중 29건이 그 둘이었다. **부재 `SKIPPED`는 열이 미측정이라고 말할 뿐, 그
  열을 채울 코드가 한 번도 돌지 않았다고는 말해 주지 않는다.***
- **A document that cites an executable as its evidence owes a run, and a debt
  named in a header is a debt nobody counts.** Four instances in one day,
  2026-08-28: `spikes/c_peer.sh` had **never been compiled on Linux** (its first
  CI run failed on a glibc `-Werror=format-truncation` macOS clang cannot
  produce); `spikes/event_by_name.sh`, which D029 cites as what makes E3 *"a
  measurement rather than a self-test"*, was run by nothing — `grep -c` over the
  harness and over `ci.yml` both returned **0**; `spikes/scope_controls.sh`, the
  negative control for two scope widenings, was run by nothing **and had stopped
  being able to run**, because the widening it controls gained a `git ls-files`
  scan and the control feeds it a tree `git archive` extracted, which has no
  `.git`; and `spikes/half_reply.sh`'s own row in `COMPONENTS.md` said *"not yet
  a `run_checks.sh` group"*. **Three of the four said so in their own headers**,
  and that is the finding rather than a detail. The distinction the gate
  (`spikes/cited_and_run.py`) draws is the whole of it: a header that **refuses**
  the gate — *"a report, not a gate"* — is a decision and passes; one that
  **defers** it — *"not wired into"*, *"named as undone"*, *"the recommended
  group"* — is an IOU and fails. Its own first draft reported seven, of which
  five were its blindness: it globbed `spikes/*` and could not see
  `spikes/jacorb/setup.sh`, which `ci.yml` runs by that exact path, and its
  invocation check was one level deep where `trading_client.py` sits three.
  **It reports zero because it was fixed, not because it was tuned** — the
  difference this file names elsewhere as the green-while-measuring-nothing
  class with better manners. *문서가 실행물을 증거로 인용하면 실행을 빚진다.
  하루에 넷, 그중 셋은 자기 헤더에 적어두고 있었다 — **산문에 이름 붙인 빚은
  아무도 세지 않는다.** 거절은 결정이고 유예는 차용증이다. 게이트가 0을 내는 것은
  조인 것이 아니라 고친 것이다.*
- **A control must LIFT the code it controls, never restate it — and the
  restatement is usually invisible until the control is asked to go red.** Six
  instances in one day, 2026-08-28, every one caught only by trying to make the
  control fail: a probe used `grep -n` where the scan it validates uses
  `grep -rn`, so the comment filter anchored on `path:line:` matched nothing and
  the probe reported its own comment as a hit; a control stubbed `diag_out` with
  a four-line version and measured the stub, reporting 4 where the shipped
  function shows 30; a control script was **an empty file** — a sed quoting
  error swallowed by `2>/dev/null`, and `bash` exits 0 on an empty script, so it
  passed while executing nothing; a fallback written into a **generator** used
  `return` instead of `yield from`, which yields nothing, so a control reported
  five failures that read as *the widening does not catch what it was built
  for* when nothing had been handed to it. `spikes/ledger_control.sh` is the
  shape that works: it lifts `hr`, `bears_on` and the ledger out of
  `run_checks.sh` with `awk` and runs **those bytes**. *통제군은 통제 대상 코드를
  **들어 써야** 하고 다시 적으면 안 된다. 하루에 여섯 번, 전부 통제군을 빨갛게
  만들어 보려 할 때만 드러났다 — 빈 파일은 exit 0을 내고, 제너레이터의 `return`은
  아무것도 내지 않는다.*
- **Never conclude from a truncated read.** `transparency.py --cite location |
  head -4` showed a leak that the very next clause said was **fixed**, and half
  an afternoon was nearly spent on it; `diag_out … 8` cut a compiler error above
  the line that named the warning, so two CI runs were spent widening the
  diagnostic before the defect could be read at all; `cut -c1-155` hid the
  message on a third. The defaults are the hazard: a head, a tail count, a
  column cut. **Read the whole cell, the whole message, the whole verdict** —
  and when a tool truncates by default, that is the tool to fix. *잘린 읽기에서
  결론 내지 않는다. 기본값이 위험이다 — 잘린 바로 다음 절이 "Fixed"였다.*
- **`target/` grows without bound, and it costs time rather than disk.** Cargo
  writes a new hash per build and evicts nothing, and this repository builds the
  same code many ways — a different `RUSTFLAGS`, a different feature set, a
  worktree per agent. Measured 2026-08-27, after the harness had twice spent
  most of an hour in one group: **`target/debug/deps` held 858,966 files and
  25 G**, and `cargo test --workspace` took **over 50 minutes** locally against
  **194s in CI** for the same tree with no compilation in either. After
  `cargo clean`: 13,538 files, 2.4 G, and the same command took **221s, then
  85s**. Cargo reads that directory on every invocation; `cargo clean` itself
  took 287s, so it had grown too large to *delete* quickly. The 16x was never
  the tests — 2002 of them pass in 85 seconds. Diagnosed rather than suspected:
  the slow case was seen twice, the fix stopped it, and the fast case
  reproduced twice. `spikes/reclaim.sh` prints the count beside this
  measurement and `--cargo-clean` acts on it; **no threshold is a gate**,
  because there is no defensible number for "too many artifacts" — the same
  reason `entry_cost.py` reports and does not gate. *`target/`는 무한히 자라고,
  그 비용은 디스크가 아니라 시간이다. 86만 파일에서 50분, 청소 후 221초, CI는
  194초. cargo는 매 호출마다 그 디렉터리를 읽는다. 테스트가 느린 것이 아니었다 —
  2002개가 85초에 통과한다.*
- **A probe must not be a caller, and where everything reaching the subject is
  recorded there is no probe that is not one.** The rule above — *a published
  IOR is not an accepting listener, so wait until it accepts* — was converted
  in the JacORB group on 2026-08-29 and then tried on `spikes/wide_rust.sh` the
  same day, where it **made things worse**: that fixture's traffic is captured
  by a tap and compared against recorded octets, so `spike-dump` (which
  **dials**, and prints `sequential calls on one connection: 1` while doing it)
  injected a call and took the script from **0 failures to 10**; a bare TCP
  connect that sent nothing still took it to **6**. The fixed `sleep 0.2` there
  is a **refusal with a reason**, not an unconverted site, and it says so where
  it sits. `spike-dump --address` decodes and stops, because a decoder that can
  only decode by dialling leaves a shell no choice but to parse CDR out of hex,
  which this repository has already refused once. A sweep for this class found
  **53 sites in 18 files** (2026-08-29) — converting them by count would be *a
  batch scoped to a keyword rather than to the rule*, since some of those
  sleeps are this same refusal. *탐침은 호출자여서는 안 되며, 대상에 닿는 모든
  것이 기록되는 곳에는 호출자가 아닌 탐침이 없다. 규칙을 옮겨 적용했더니 0 실패가
  10이 되었다 — 실행해서 알았지 읽어서 알지 못했다. 53곳을 수로 세어 바꾸는 것은
  규칙이 아니라 키워드에 범위를 맞춘 배치다.*
- **A completed client `connect` does not mean the server can accept yet.**
  On macOS loopback a non-blocking single `accept()` misses fresh connections
  ~5% of the time (measured 25/500 in stream E batch 2). Accept-side checks
  wait with a sleeping, deadline-bounded loop — the same class as the wait
  rule above.
- **An unmeasured check is a failure, never a pass.** If a fixture will not
  start, increment the failure counter. A harness that reports green on an
  unmeasured assumption is worse than no harness.
- **A new harness group lands with its negative control in the commit
  message** — the command that was run to make it red, and what it printed
  (D010 §7.2). Five gates were green while measuring nothing in one week, and
  every one was found by a negative control, none by review; the fifth was
  written by the person who had just recorded the other four — a probe that
  grepped its marker out of a traceback echoing the source line. Probes use
  exit codes, not markers.
- **A peer's bytes are recorded with provenance and re-taken live.** A
  convention both ends apply cannot be refuted by a round trip, and a
  convention one end applies on read can hide the other end's defect on
  write; twelve wire changes in v0.5.0 were found this way and none by a test
  we could have written from the specification alone. One test per capture
  decodes it and re-encodes back to the peer's bytes; the harness re-takes
  every capture from the live fixture (`spikes/*_capture.py`).
- **Our own counters are not what a peer saw, and a two-process fixture must
  never let its own exit code vouch for the peer's.** Measured 2026-08-26
  (D034 §5.1): with the ORB's shutdown deliberately broken to drop in-flight
  work, `spike-orb-shutdown` printed `servers_stopped:1, went_quiet:true,
  serve_returned_ok:true` and **exited 0** — every number this side keeps said
  the shutdown was clean — while the peer on the other end of the same socket
  got a TCP reset and not one octet of GIOP. A lifecycle checked from its own
  counters is green on exactly the build it exists to refuse. Print both,
  verdict from the peer. And **a reset is an observation, not a failure to
  measure**: that peer's first draft filed one under `UNMEASURED` and exited 3,
  so the strongest refutation it could produce would have read as an unmeasured
  check rather than a failure — found by running the control, not by reading it.
  *우리 카운터는 피어가 본 것이 아니다. 자기 카운터로 검사하는 생애주기는 그것이
  거절하려는 바로 그 빌드에서 초록이 된다. 리셋은 측정 실패가 아니라 관측이다.*
- **A class-B claim lands as a counted `SKIPPED` group naming its fixture,
  never as a `note` and never as `ok`** (D010 §2).
- **A batch scoped to a keyword will fix a keyword; scope it to the rule.**
  Twice in one release the defect handed to a batch was one instance of a
  production-wide divergence: a signature takes `param_type_spec` (ten
  divergences from the oracle, eight closed by one function — a `fixed`-only
  fix would have closed three) and a constant takes `const_type` (seven
  shapes, one cause, two of seven). Both agents re-measured the neighbours of
  the shape they were given, which is why the count is known at all.
- **A record written by a script that fails is not a record.** A `python3 -
  <<EOF` whose anchor has drifted exits non-zero, and the `git commit` on the
  next line runs anyway: twice this release a records commit carried only part
  of what its message claimed, and `records_keep_up.py` read eighteen commits
  behind while the failure looked like a false alarm. Check the writer's exit
  status before staging, and read the gate's complaint as true until measured
  otherwise. The verdict line counts
  SKIPPED; prose after an `ok` is not counted and reads as coverage.
- **A control that names a live subject stops being a control when the subject
  moves — and the pair guarding it breaks in opposite directions, one of them
  silently.** Measured 2026-08-26 in `ledger_control.sh`. Controls 2 and 8
  demonstrate one property of the transparency ledger — *a group that declares
  a transparency and measures none of it must not flip the row to measured* —
  and both demonstrated it by typing the name `activation`. The pin outlived
  its fact twice in one day: activation went from undeclared to
  declared-and-measuring-nothing, the string was edited to match, then the
  activation leg started actually measuring and its `tp_measures_nothing` came
  off. **Control 2 went red; control 8 went green while exercising nothing** —
  and 8's whole job is to prove 2 is not tuned-until-quiet, which it did by
  asserting a flip that no longer had anything to flip. Four of its five
  assertions are still green in that vacuum, so only the loud half would ever
  have been looked at. Computing the name instead of typing it is the **wrong**
  repair and was tried first: it makes the control's existence depend on what
  the project happens to still be waiting on, which is the thing that broke.
  **Synthesise the subject**, and make the stripping control refuse when the
  strip removed nothing. *살아 있는 대상을 이름으로 박은 대조군은 대상이 움직이면
  대조군이기를 그만두고, 그것을 지키는 짝은 반대 방향으로 깨진다 — 하나는 빨갛게,
  하나는 아무것도 실행하지 않은 채 조용히 초록으로. 이름을 계산하는 것은 틀린
  수리다. 대상을 **합성**하고, 벗겨내는 대조군은 벗길 것이 없으면 거절하게 한다.*
- **Indistinguishability is evidence about transparency only beside a
  demonstration that distinguishing is possible.** Measured 2026-08-26: the
  test that a caller cannot tell one backend from another stayed **green** when
  `Dispatch::knows`'s default was made a blanket `false`, because a server that
  serves nothing answers both keys identically too. A *cannot tell* assertion
  passes in every world where nothing happens, so it is only worth something
  next to a counted companion showing the two answers **can** differ — which is
  why that group's control count is what it is. Same shape as the rule above,
  one level up: a green that means *nothing occurred* reads exactly like a
  green that means *the property held*. *구별불가능성은 구별이 가능하다는 시연이
  옆에 있을 때만 투명성의 증거다 — 아무 일도 일어나지 않는 세계에서도 통과하기
  때문이다. **아무것도 일어나지 않았다**는 초록과 **성질이 지켜졌다**는 초록은
  똑같이 보인다.*

### Where a fact lives / 사실이 사는 곳

Every fact has one home. A document that **restates** another document's fact
drifts from it on the next change, silently, because nothing compiles a
sentence. Measured 2026-08-18: ten stale decision-status claims and four stale
remaining-work lists across five documents, produced by nothing worse than
decisions being approved and work landing.

*사실마다 집은 하나다. 다른 문서의 사실을 **다시 적은** 문장은 다음 변경에서
조용히 어긋난다 — 문장을 컴파일하는 것은 없기 때문이다.*

- **A decision's status lives in `docs/decisions/D00N-*.md` and nowhere else.**
  Every other mention is checked against it by `spikes/decision_status.py` in
  the harness. Dated records — `docs/pipeline-runs/`, `PHASE*.md`, released
  CHANGELOG sections — state what was true at a date and are out of scope by
  construction: editing them to match today would falsify them, not repair them.
- **`PLAN` records what was planned and what landed against it; `COMPONENTS`
  records current status and what is still missing.** PLAN does not restate
  COMPONENTS' remaining-work column. It did, and all three items it named had
  landed while it still named them — the cost was a planning pass spent on
  finished work, which no test can go red on. *계획서는 상태를 다시 적지 않는다.*
- **A record lands with its batch, not after it.** `COMPONENTS.md` states what
  is measured now and `CHANGELOG.md` states what changed; a script cannot check
  either for truth, so `spikes/records_keep_up.py` checks the only thing it
  can — whether they were opened at all — and fails past ten commits. Measured
  2026-08-18: they had gone **thirty-nine commits**, six of them wire-behaviour
  changes, while three `COMPONENTS.md` rows became false. *배치는 기록과 함께
  착지한다.*
- **A bilingual fact is one fact in two languages: edit both or neither.**
  D003's approval overwrote the head of its own PROPOSED block and left the
  tail, so the file said APPROVED in English and 제안 in Korean four lines
  apart — and every document that had copied it copied the English half.
  **This rule alone did not stop it.** Swept 2026-08-25: four more instances,
  every one the English half re-measured and the Korean left asserting the
  pre-measurement fact — a sweep's Korean saying it had *not* re-measured
  while the English said it had, and two counts frozen at a figure their own
  English twin had already superseded. So the rule now has a script:
  `spikes/bilingual_drift.py` blames each section and reports where the two
  halves were last edited days apart, because *the rule is an invariant about
  commits* — one fact, one change, both languages. Its first design compared
  **date literals** and its negative control killed it: 34 findings of which 4
  were real, and the threshold that suppressed the noise removed exactly those
  4. **A check tuned until it is quiet, tested only against a tree with no
  defect in it, is the green-while-measuring-nothing class with better
  manners** — tune against the defect, then confirm the quiet.
  *규칙만으로는 막히지 않았다. 규칙은 커밋에 대한 불변식이므로 이제 스크립트가
  있다. 첫 설계는 부정 대조군이 죽였다 — 조용해질 때까지 조인 검사는 찾으려던
  것만 정확히 걸러낸다.*
- **A sentence many layers say is a fact, and `pub(crate)` is how a fact
  escapes its home.** A refusal, a limit, a diagnostic head that more than one
  layer must give in the same words belongs to one function, and that function
  has to be reachable from every layer that owes it — including the ones in
  other crates. Measured 2026-08-24: the two heads for the four constructs the
  wire cannot carry were `pub(crate)` in `orbweaver-dynamic`, so
  **twelve literals in two other crates** wrote them again, and one had gone
  false — `prop.rs` told a contract-check reader that `from_json` answers
  `"cannot cross yet"` for a `fixed`, three days after that layer stopped
  saying it. Nothing was red: the pin that existed was scoped to a crate and
  the fact is scoped to the workspace. **A pin whose scope is narrower than its
  fact's is a pin that will go green over the drift.** The gate for this class
  computes the expected text by calling the same function, so a layer that
  keeps a literal fails the moment the wording changes rather than at the next
  reading. *여러 계층이 말하는 문장은 사실이며, `pub(crate)`은 사실이 자기 집을
  빠져나가는 경로다. 고정의 범위가 사실의 범위보다 좁으면 그 고정은 어긋남 위에서
  초록으로 남는다.*
- **A floor is not a figure.** A gate pinned as `>= N` proves the property it
  was built for — nothing regressed — and proves *nothing* about the count, so
  every sentence that quotes `N` as if it were today's measurement drifts
  upward in silence as the corpus grows, and the gate stays green over the
  drift because green is all it was ever going to say. Measured 2026-08-25,
  two instances in one sweep: `COMPONENTS.md` said the AnyJSON leg crosses
  `5248/5248` (floor 5248, **actual 6016**) and that the Python sweep crosses
  `172 values / 137 calls` (floor 170/137, **actual 182/139**) — and the
  harness's own comment beside that floor said `170 / 137` too, so the row and
  the gate agreed with each other and both disagreed with the run. A floor
  keeps its rationale in the comment; a figure in prose carries **the date it
  was measured**, or comes from a script that writes it
  (`spikes/coverage_tables.py`). Where a document and a gate quote the same
  number, say which one is the floor. *하한은 수치가 아니다. `>= N` 고정은
  퇴행 없음을 증명할 뿐 개수에 대해서는 아무것도 증명하지 않으므로, `N`을
  오늘의 측정처럼 인용한 문장은 조용히 어긋나고 게이트는 그 위에서 초록으로
  남는다. 산문의 수치는 측정 날짜를 달거나, 그것을 쓰는 스크립트에서 온다.*
- **A classifier is a sentence too.** Code that decides *which class a thing
  belongs to* by matching a hand-written substring of a sentence some other
  function owns is the same defect wearing the counting half's coat, and it
  fails the same way: silently, when the sentence changes for a good reason.
  Swept 2026-08-24 across twelve crates — five instances, three of them silent
  and one **already losing in the product**: `LexError::rule` classified by a
  retyped prefix that one of its own three construction sites did not carry, so
  a fixed-point literal too long to parse filed under `parse` and never
  received the fix hint written for it. Ask the owner: either the owning crate
  publishes the marker (`orbweaver_cdr::IMPLAUSIBLE_LENGTH`) or the classifier
  computes it by calling the function that writes the sentence. Where the
  constant becomes shared there is **nothing left to test** — the drift is
  impossible rather than detectable — and a negative control there comes back
  green, which is a reason to record the fact rather than to add a test.
  *분류자도 문장이다. 소유 크레이트가 표지를 공개하거나, 분류자가 문장을 쓰는 함수를
  호출해 표지를 계산한다. 상수를 공유하게 되면 남는 테스트는 없다 — 어긋남이 탐지
  대상이 아니라 불가능해지기 때문이다.*
- **A cascade whose catch-all *clears* what its mapper *refuses* is a hole the
  shape of the gap between two lists.** `orbweaver-gen`'s `rust_type` ends in
  `other => Err(..)` and its walker `representable` ended in `_ => Ok(())`, so
  every construct in the gap was skipped at its declaration and **emitted at
  every container that named it** — `pub sealed: ...::gp34::Envelope` for an
  `Envelope` nothing declares. Two lists of "what the wire cannot carry", one
  per emitter, each maintained by hand against a mapper that already knew.
  Measured 2026-08-25: the Rust half at least failed to compile; the Python
  half **was not red at all**, writing `("ref", "IDL:gp34/Envelope:1.0")` for a
  class its package never defines, which the caller discovers at the first
  call. Exhaustiveness at the leaf does not survive a walker that is permissive
  at the node: the walker must **ask the mapper** at every node rather than
  keep its own list, which makes the gap unrepresentable instead of detectable.
  Note which half of this was found by a compiler and which by nothing — the
  target with the weaker type system is where the same defect goes quiet.
  *매퍼가 거부하는 것을 캐스케이드의 catch-all이 통과시키면, 두 목록 사이 틈
  모양의 구멍이 생긴다. 잎에서의 전수성은 노드에서 관대한 순회를 견디지 못한다 —
  순회가 자기 목록을 갖는 대신 **매퍼에게 물어야** 한다. 어느 쪽 절반이 컴파일러에
  잡혔고 어느 쪽이 아무것에도 안 잡혔는지 보라.*

### Honesty rules / 정직성 규칙

- Report what was measured, not what was intended. If something is unmeasured,
  say so and say why.
- When the generator and the evaluator are the same model, label the number
  indicative and say so in the same breath as the number.
- Do not claim a transient is diagnosed until it reproduces and the fix makes it
  stop. "Did not reproduce in N runs" is a valid, honest result.

---

## Commands

```bash
cargo test --workspace          # 1949 tests across twelve crates (measured
                                # 2026-08-26; ~1515 when this line was written,
                                # and a figure in prose carries its date)
./spikes/run_checks.sh          # the harness; exit code is the verdict, one run at a time
```

The harness takes a machine-wide lock (`/tmp/orbweaver-harness.lock`) and kills
fixtures by process group. Two runs at once used to destroy each other's peers
and report failures that were about the scheduling; that cost two diagnoses.
Wait for the lock rather than removing it. *하네스는 머신 전역 락을 잡는다.*

**Gates, in the order they get run:**

```bash
cargo run -q --bin sidl-validate -- [-I <dir>]... <files>.idl   # S4: syntax,
                                # semantics, fix hints; #include resolved first
cargo run -q -p orbweaver-test --bin contract-check -- corpus/golden/*.idl
                                # property (defects) + annotation advice (never gates)
cargo run -q --bin idl-diff -- <released>.idl <proposed>.idl   # §5.3, exit 1 on breaking
omniidl -b dump <file>.idl      # the conformance oracle for one file
./spikes/differential.sh        # three front ends over the corpus, divergences recorded
                                # (omniidl, tao_idl, jacorb_idl; the third needs
                                # spikes/tao/setup.sh, and is a counted SKIPPED
                                # rather than a silent pass where it is absent)
python3 spikes/decision_status.py  # every restated decision status vs its decision
```

**Measurement tools** — each prints what it could *not* measure:

```bash
cargo run -q --release -p orbweaver-test --bin wire-fuzz -- --cases 50000
                                # panic freedom over the decoders a peer reaches first
cargo run -q --bin repository-ids -- corpus/pragma/*.idl   # ids, to diff against omniidl
./spikes/service_sweep.sh       # every declared operation of the five servants, over the wire
./spikes/end_to_end.sh          # requirement → contract → both halves → guarded call
./spikes/nat_rewrite.sh         # R7: an IOR dialable from where the client actually is
./spikes/estate/run.sh --tsv    # thirteen legacy contracts, ingestion to agent call
./spikes/perm_fallback.sh --expect-temporary reask --expect-permanent stay
                                # the two forward statuses, told apart by behaviour — omniORB and ours
./spikes/orb_shutdown.sh        # what a peer MID-CALL sees when Orb::shutdown stops the
                                # server under it — the peer imports no ORB and builds its
                                # own §9.4 requests, both byte orders. Its exit code is the
                                # measurement; the fixture's own counters are printed beside
                                # it and never allowed to vouch for it (D034 §5.1)
./spikes/jacorb_giop11.sh · ./spikes/jacorb_wchar11.sh · ./spikes/wide_rust.sh
                                # GIOP 1.1/1.2 wide text against JacORB, version asserted from bytes
python3 spikes/union_label_capture.py · python3 spikes/union_default_capture.py
                                # re-take the peer's recorded TypeCode bytes from the live fixture
./spikes/service_sweep.sh --raw | python3 spikes/coverage_tables.py --check
                                # SERVICES-COVERAGE §8 says what the wire says
cargo run -q --bin gen-python -- --out <dir> <files>.idl   # the second target
cargo run -q -p orbweaver-console --bin orbweaver-console -- catalog <file>.idl --text
python3 spikes/gap_symbols.py   # before planning against a COMPONENTS gap row: what it
                                # names and whether that exists — a report, not a gate
python3 spikes/plan_numbers.py  # every hand-typed count in the plan documents beside
                                # today's computed figure — a report, not a gate
python3 spikes/bilingual_drift.py  # sections whose EN and KO halves were last edited
                                # days apart — a report, not a gate
python3 spikes/entry_cost.py    # what a newcomer must name to serve an object and to
                                # call one, and how short the shortest path is — a
                                # report, not a gate (D027 E4). No threshold for "too
                                # many items to learn" is defensible, which is why it
                                # is not one
```

Fixture setup: `brew install omniorb` (interop peer and oracle only);
`spikes/jacorb/setup.sh` for the second oracle and its Interface Repository;
`spikes/tao/setup.sh` for the third front end — it builds `tao_idl` from the
ACE+TAO source because Homebrew's `ace` formula fetches that tarball and builds
only `ace/`, so there is no packaged `tao_idl` to install. All three are
fixtures on the same terms: separate processes and external programs whose
output we read, never dependencies, never published.

## Layout

```
crates/orbweaver-cdr/       CDR encode/decode, and the JSON parser — it moved
                            down here 2026-08-26 so fixtures below `dynamic`
                            can read a seed; `orbweaver_dynamic::json` is a
                            re-export and removing it would widen the graph
crates/orbweaver-giop/      GIOP, IOR, TypeCode, Server/Dispatch, naming + event servants
crates/orbweaver-idl/       IDL 4.2 front end, SIDL structured comments
crates/orbweaver-registry/  types as data, the IFR facade, remote IFR ingestion, §5.3 differ
crates/orbweaver-object/    POA, references, MoE residency, tenancy
crates/orbweaver-dynamic/   value marshalling, DII/DSI shape, AnyJSON
crates/orbweaver-trading/   offer store, constraint queries, loading policy
crates/orbweaver-forge/     the S1–S5 pipeline, each stage a producer plus its own gate
crates/orbweaver-mcp/       the agent boundary: triad, handles, interceptor chain, dry-run
crates/orbweaver-gen/       client stubs and server skeletons
crates/orbweaver-test/      property, contract advice, wire fuzz
crates/orbweaver-console/   catalog, contract diff and trace pages — renders, decides nothing
corpus/golden/              must all compile — type-system and CDR coverage
corpus/negative/            must all be rejected — diagnostic quality material
corpus/services/            contracts that exist to be served (identity pragmas live here)
corpus/pragma/              repository-id cases, diffed against omniidl
corpus/requirements/        assumption B benchmark, frozen before generation
corpus/queries/             the frozen search benchmark (v1 stays frozen; v2 is widened)
corpus/annotations/         assumption C probes
corpus/include/             the first multi-file cases — resolution, prefix scope,
                            guards, cycles. Every other corpus file is
                            self-contained, which is why #include was skipped
                            rather than resolved for six phases with nothing red
corpus/divergences.tsv      where the front ends disagree, with which one we follow
spikes/estate/              thirteen legacy contracts that include each other,
                            four prefix styles, nothing annotated — consumer-
                            shaped, and a gate. It gates nothing itself, which
                            is what lets it measure the path
spikes/                     fixtures, servers, the harness, and the measurement scripts
docs/                       ARCHITECTURE (as built) · PLAN(.ko) · COMPONENTS (measured)
                            PLAN-MOE · PLAN-SERVICES · PLAN-DEFERRED · SERVICES-COVERAGE
                            PLAN-FIRST-COMPLETION (the open leaks, priority-zero order)
                            decisions/ · pipeline-runs/ · PHASE0–6
```

## Conventions

- Documents are maintained in **English and Korean**, kept structurally
  symmetric section by section.
- Corpus additions go in with the change that motivated them, never later —
  **and with `./spikes/differential.sh --require omniidl,jacorb_idl --record`,
  which `cargo test --workspace` now insists on.** Measured 2026-08-25: eight
  files had landed without either front end ever comparing them, and the
  harness found seven divergences and a golden file whose generated Rust did
  not compile. The cause was not carelessness — batches are told not to run
  `run_checks.sh`, because it takes a machine-wide lock, and **nobody named the
  standalone gate they should have run instead.** A prohibition without its
  replacement is an instruction to skip the check. So the gate stopped being a
  command to remember: the differential's verdict is checked-in data
  (`corpus/differential-results.tsv`) and an oracle-free test compares the
  corpus against it, which means it runs for everybody rather than for whoever
  runs the harness. *금지에 대체물이 없으면 검사를 건너뛰라는 지시다.*
- Rust: `unsafe_code = "forbid"` at the workspace level. Keep it that way —
  wire parsing is the classic memory-safety hazard and that is why the core is
  Rust in the first place.
