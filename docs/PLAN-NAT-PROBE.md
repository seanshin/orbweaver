# A probe that says it has never run, a harness that cannot say whether it did

**한 번도 돌지 않았다고 말하는 탐침, 돌았는지 말할 수 없는 하네스**

**STANDING: a work plan, not a decision.** It executes no new decision; it
repairs a record the tree already keeps wrongly, and it charges the cost that
made the record findable. Where it needs the owner it says so and stops.

*결정이 아니라 작업 계획서다. 새 결정을 실행하지 않는다 — 트리가 이미 틀리게 지니고
있는 기록을 고치고, 그 기록을 찾게 만든 비용을 다룬다.*

> **Priority zero.** D029 §6's home is not restated here. This bears on the
> **Location** row — R7 is the row's own named instrument — and §6 below says
> how narrowly.

---

## 1. What was measured, in the order it was found / 측정된 것, 발견된 순서로

Every line here is a reading, not an inference. Dates are 2026-09-03.

1. **CI's interop job took 41 minutes** on `4641af7` against 24 on the commit
   before, and **nothing could say which group grew**: the runner's log stamps
   are flush times, not event times — ten groups share one second.
2. So the harness now stamps each group (`d0f3095`). **First CI reading**:
   `NAT rewriting` **202s**, where the same group is **10s locally**.
3. Not the dial. `spike-nat`'s `DIAL` is 3s, applied by
   `TcpStream::connect_timeout` per endpoint (`lib.rs:2907`); locally the
   unroutable case reports `6.00s` = 2 endpoints × 3s, exactly.
4. **The runner has docker.** `ci.yml`'s disk-reclaim step runs
   `docker image prune`. `nat_rewrite.sh:220` runs the container probe when
   `docker info` succeeds — so on CI it **would** run, and locally (no docker)
   it is skipped. Locally, with the VM probe running too, the whole script is
   10s; the docker branch is the only path CI has that this machine does not.
5. **`spikes/nat/run.sh`'s header says, in capitals, `THIS SCRIPT HAS NEVER
   BEEN RUN`.** Whether that is still true **cannot be read from the CI log**,
   and the first draft of this plan said it could: it cited
   `rewritten publish: endpoint 127.0.0.1:24404` as the probe's output. Traced,
   that line belongs to *assumption D — IOR endpoint publishing*, a Phase 0
   group, and is printed by `spike-nat` on the host. **The container probe's
   output, if any, is never in the log**: the harness captures
   `nat_rewrite.sh` into a variable and prints only its own summary. The
   evidence for "it runs on CI" is therefore circumstantial — docker is on the
   runner, the script's `docker info` branch is unconditional, and the group
   costs 192s more there than here — and §4 says what settles it.
6. **The harness cannot report it either way.** `run_checks.sh`'s NAT group
   prints `SKIPPED  the container probe has never run here — no docker`
   whenever `nat_rewrite.sh` reports `unmeasured (skipped): [1-9]`. That script
   has **three** skips — the VM probe, the container probe and the k8s probe —
   and the harness reads the count, not the lines. Locally, with the VM running,
   the script reports 2 skips (container, k8s) and the harness's sentence
   happens to be right; on CI, where multipass is absent, the count is 2 or 3
   and the sentence names the wrong probe or the right one by luck.
7. **Where the 202s would go, if it is the probe** — two places, read off the
   files while sizing this plan for parallel work:
   - `spikes/nat/compose.yaml` builds both services with `context: ../..` —
     **the repository root** — and the Dockerfile does `COPY . .`. **There is
     no `.dockerignore`.** On the runner that context includes `target/`,
     which `cargo test --workspace` has just filled: gigabytes streamed into
     the build daemon before a single instruction runs, twice per probe run
     (`up --build server`, then `run client` against the same image).
   - the Dockerfile then does `cargo build --release -p orbweaver-giop --bin
     spike-nat` **inside** a `rust:1.85-slim` container — a cold release build
     of the crate and its dependencies, with no cache, to produce a binary the
     runner has already built.

   Neither is timed on CI yet, because nothing on CI prints the probe's own
   lines. Both are repairs that can land **without knowing which one it was**,
   because each is wrong on its own terms: a context that ships build output is
   wrong whatever it costs, and an in-container build measures nothing the
   probe is about.

*41분 → 그룹별 시각 → `NAT rewriting` 202초(로컬 10초) → 다이얼이 아님 → 러너에
docker가 있음 → 그래서 컨테이너 탐침이 **돌 것이다** — 그런데 **첫 초안이 증거로 든
로그 줄은 다른 그룹의 것이었다**(Phase 0 가정 D). 하네스가 스크립트 출력을 변수로
삼키므로 탐침이 돌았는지는 어떤 로그로도 알 수 없다. 정황은 하나뿐이고 §4가 무엇이
그것을 확정하는지 적는다.*

---

## 2. What kind of defect this is / 어떤 부류의 결함인가

Three records disagree and none of them is red:

| record | says | what can be established |
|---|---|---|
| `spikes/nat/run.sh` header | never been run | unknown — and unknowable from any log the tree produces |
| `run_checks.sh` NAT group | `SKIPPED — no docker` | docker is present on the runner; the sentence is wrong there whether or not the probe ran |
| `nat_rewrite.sh` | `unmeasured (skipped): N` | true, and it says *which* — the harness discards that |

This is **a gate scoped to a place while the rule is about a claim**, the class
`doc_symbols.py` and `tracked_not_walked.py` were built for this week, with a
twist worth naming: the harness's misattribution is **green in the safe
direction**. It reports *less* measured than actually was. Nobody looks at a
skip that is really a pass, which is why it lasted — a false red is found in an
hour and a false skip is found by the electricity bill.

And if the probe is running and passing, **nobody has read its control**,
because nobody knew it was running. The control is not missing — `run.sh`
already runs two cases, `naive` (must **fail**) and `published` (must pass) —
it is *unread*: its lines never reach a log. That changes S2 from "write a
control" to "make the one that exists visible and cite it", which is the
same repair as S1 seen from the other end. Its header says *expect to fix it, not to confirm
it, and treat a first green run with suspicion.* Every run so far has been a
first green run nobody treated with anything.

*세 기록이 어긋나고 어느 것도 빨갛지 않다. 하네스의 오귀속은 **안전한 방향의 초록**이다
— 실제보다 적게 측정했다고 보고한다. 거짓 빨강은 한 시간 안에 발견되고 거짓 건너뜀은
전기 요금으로 발견된다. 그리고 통과하는 탐침은 **부정 대조군이 한 번도 돌지 않았다** —
도는 줄을 아무도 몰랐기 때문이다.*

---

## 3. The work, as two lanes / 작업, 두 차선으로

**What is genuinely serial, and what only looked it.** The first draft ran
S1→S2→S3 because S3 "should not land before S1 proves the probe runs". Sized
against the files, that dependency is not there: S3's two repairs are wrong on
their own terms (§1.7) and would be made even if the probe had never run. What
*is* serial is narrow — **the header (S2) may not be rewritten until S1 has
printed, on CI, what the probe did** — and that is one file, at the end.

So: two lanes with **disjoint footprints**, one merge point.

| lane | touches | must not touch |
|---|---|---|
| **A — the harness says what happened** | `spikes/run_checks.sh` (one group), a control script under `spikes/` | anything under `spikes/nat/` |
| **B — the probe stops paying for what it does not measure** | `spikes/nat/Dockerfile`, `spikes/nat/compose.yaml`, a `.dockerignore` | `spikes/run_checks.sh`, `spikes/nat/run.sh`'s header |
| **merge — the header, and the reading** | `spikes/nat/run.sh` header, `docs/PLAN-FIRST-COMPLETION.md` §D | — |

D028's rule for parallel work applies unchanged: *the merged tree is compiled
before the merge, not after*; each lane runs the whole harness on its own
branch, and the merge runs it once more. Neither lane's verdict is evidence for
the other's.

### Who runs which lane, and the rule that decides it

The parallel protocol this repository has used since 2026-08-13 has one hard
line: **nobody but the coordinator touches `spikes/run_checks.sh`**, because
the harness is the single merge gate (D010 §7.5) and a worktree agent editing
it is an agent editing the instrument its own work is judged by. Lane A's
footprint *is* that file. So:

| lane | runs as | why |
|---|---|---|
| **A** | the coordinating session, on `main`, serially | it edits the gate |
| **B** | one `general-purpose` agent, `isolation: "worktree"`, `spikes/nat/` and a `.dockerignore` only | disjoint from the gate; local checks only, no docker here |
| **merge** | the coordinating session | it reads lane A's CI log before touching the header |

Lane B's agent is told what every wave's agents are told: read `CLAUDE.md`
first; **report the harness group it recommends without applying it**; one
commit in the style of `git log -5`; and it may not run `run_checks.sh` (the
lock is machine-wide and the coordinator is holding it for lane A). Its local
oracle is the two checks it writes plus `bash -n`. Its wire oracle is the
merged harness run, which is serial and the coordinator's.

**What lands first.** Lane A, because its CI push is the reading everything
else is conditioned on (§4, first row). Lane B's branch is merged second, its
recommended group applied by the coordinator, and the merged tree runs the
harness once — that run is the one whose stamp lane B's claim rests on.

*프로토콜의 한 줄 — **코디네이터 외에는 아무도 `run_checks.sh`를 만지지 않는다** —
이 A 차선의 발자국을 결정한다. A는 코디네이터가 `main`에서 직렬로, B는 워크트리
에이전트 하나가 `spikes/nat/`만. 먼저 착지하는 것은 A다 — 그 CI 푸시가 나머지 모든
것의 조건이 되는 판독이기 때문이다.*

### Lane A — S1: the harness reads the script's lines, not its count

`nat_rewrite.sh` prints one `skip` line per absent probe (vm, container, k8s),
each naming what is missing, and one `pass` line per probe that ran. The harness
reads the count and guesses. It reads the **lines** instead — one counted
`SKIPPED` per absent probe, naming it, and one `ok` per probe that ran, quoting
the probe's own pass line — so that **what the probe did on CI is in the CI log
for the first time**. This is the step that turns §1.5's circumstantial case
into a reading.

- **Measurement:** here (docker absent, VM running) the group reads
  `ok vm … SKIPPED container … SKIPPED k8s`; on CI it reads whatever is true
  there, and the log says which.
- **Control:** a script that feeds the group's parser synthesised
  `nat_rewrite.sh` output — all three skipped, none, and each alone. Five
  shapes, five different verdict lines. The current code produces one sentence
  for all five. Lifts the parser out of `run_checks.sh` with `awk`, as
  `ledger_control.sh` does; does not restate it.
- **Also in this lane:** the group prints the script's `naive` and `published`
  lines when the container probe ran, so lane B's before/after can be read off
  the same log — the two lanes share a *reader*, not a file.

### Lane B — S3: the probe stops paying for what it does not measure

Two repairs, both in `spikes/nat/`, both landable without lane A:

1. **A `.dockerignore`** at the context root (`../..` → the repository root)
   excluding `target/`, `spikes/*/omniORBpy/`, `spikes/tao/ACE_wrappers/`,
   `spikes/jacorb/lib/` and the other ignored trees — read off `.gitignore`,
   not typed twice: the ignore file is *derived* from it by a line in
   `run.sh`, or the two drift. This alone may be most of the 192s, and it is
   correct regardless: a build context that ships build output is wrong at
   any size.
2. **The binary comes from the runner, not from a build inside the image.**
   The Dockerfile becomes `debian-slim` + `COPY` of the `spike-nat` the runner
   has **already built in debug** — `nat_rewrite.sh:143` runs
   `cargo build -q --bin spike-nat` before the probe, on the host, every time.
   No Rust toolchain in the image.

   The first draft said *"`--release`, which the release-profile group already
   does"*. Checked: that group runs `cargo test --workspace --release` and does
   produce `target/release/spike-nat` — but it runs **after** the NAT group, so
   lane B could not rely on it without reordering the harness, which is lane
   A's file. And release is the wrong profile to want here: the probe measures
   **routing**, and an optimiser setting is not a routing fact. The debug binary
   the script already builds is the right one, and it is there by construction.

   | | | cost |
   |---|---|---|
   | **A — copy the host's debug `spike-nat`** | no toolchain in the image; the probe measures routing with the binary `nat_rewrite.sh` just built | zero extra builds |
   | B — keep the in-container build, add a BuildKit cache mount | still cold on a fresh runner | saves only when warm |

   One thing this forces and the plan says out loud: **the image is
   linux/amd64 and the host binary must be too.** On the runner it is. On a
   macOS host the debug binary is `aarch64-apple-darwin` and would not run in
   the container — which is fine, because docker is not here; but a Linux
   developer on arm64 would hit it. The Dockerfile `COPY`s from a path
   `run.sh` chooses by `uname -m`, and refuses with a sentence rather than
   copying a binary the image cannot execute.

   **A, and the reason is the measurement, not the minutes.** The probe's claim
   is about *routing* — a client that cannot reach the servant's bound address
   — and a binary built on the host measures that identically. The in-container
   build is there because the Dockerfile was written on a machine that could not
   run it, by someone who could not know what the runner would have.

- **Measurement:** the group's stamped time on CI, before and after, **and the
  probe's `naive`/`published` lines unchanged** — which lane A makes readable.
  Until lane A merges, lane B's measurement is the stamp alone, and the plan
  says so rather than pretending the probe's lines can be read.
- **What must not happen:** the probe passing because the container can reach
  the host's loopback. That is the `naive` case and it must still **fail**; a
  base-image or compose change that altered the network shape would show there.
  Lane B does not touch `compose.yaml`'s `networks:` block for this reason, and
  says so.
- **Local check, without docker:** `docker build` cannot run here. What can:
  `.dockerignore` is checked against `.gitignore` by a script (the derivation
  above), and the Dockerfile is checked to name no `cargo` — both are
  `run_checks.sh` groups lane A does not touch, in a new file.

### Merge — S2 and S4: the header, and the count

**After both lanes are in and the merged tree has run on CI once**, and only
then: `spikes/nat/run.sh`'s header stops asserting `THIS SCRIPT HAS NEVER BEEN
RUN` and says what lane A's CI log showed — the run number, the runner image,
which case passed and which failed. If the log showed it did *not* run, the
header stays and §4's first row is what the plan collapses to.

`docs/PLAN-FIRST-COMPLETION.md` §D lists *docker* among this machine's absent
conditions; it stays true here and the sentence says CI is different.

*첫 초안은 S1→S2→S3를 직렬로 두었다 — "S3는 S1이 탐침이 돈다는 것을 증명하기 전에
착지하면 안 된다"고. 파일에 대고 재보니 그 의존성은 없다: S3의 두 수리는 **각각 그
자체로 틀린 것**이라 탐침이 한 번도 돌지 않았더라도 해야 한다. 진짜로 직렬인 것은
좁다 — **헤더(S2)는 S1이 CI에서 탐침이 무엇을 했는지 찍기 전에는 다시 쓸 수
없다.** 그래서 발자국이 분리된 두 차선과 병합점 하나: **A**는 `run_checks.sh`의 그룹
하나(스크립트의 개수가 아니라 **줄**을 읽는다), **B**는 `spikes/nat/`의
Dockerfile·compose·`.dockerignore`(**`.dockerignore`가 없어 `COPY . .`가 러너의
`target/`를 통째로 컨텍스트로 보낸다** — 이것이 192초의 유력 후보이고, 크기와
무관하게 틀린 것이다). 두 차선은 파일이 아니라 **판독기**를 공유한다. D028의 규칙
그대로: 병합된 트리는 병합 뒤가 아니라 **전에** 컴파일된다.*

---

## 4. What would make this plan wrong / 이 계획이 틀리는 경우

- **If the container probe is not what runs on CI.** The first draft of this
  plan cited a log line as proof and the line was another group's — recorded in
  §1.5 rather than smoothed over. **Lane A settles it** on its first CI push.
  If the probe has never run, the header was right, the 192s is elsewhere, the
  merge step does not happen — and **lane B still lands**, because its two
  repairs are wrong on their own terms and not because of the 192s.
- **If the two lanes turn out to share a file.** They are scoped above by
  path; the merge is where that is checked, by `git diff --name-only` between
  the branches having no intersection. A lane that finds it must edit the
  other's file stops and says so rather than editing it. **One known
  near-miss, sized rather than discovered later:** `run_checks.sh:3999` names
  `spikes/nat/Dockerfile` as the *age* anchor for the container probe's
  `SKIPPED` line. Lane B editing that file moves lane A's printed age. That is
  not a conflict — the age is derived from git, not typed — but it is a shared
  *reading*, and the merge run is where it is read once with both lanes in.
- **If option A changes the network shape.** A binary copied in is the same
  binary; the image's base and the `docker run` flags are what decide routing,
  and S3 changes neither. S2's control is the check.
- **If 202s is neither the context nor the build.** §1.7 reads both off the
  files; lane B's stamped time on CI is the measurement. If it does not move,
  the cause is elsewhere — but lane B is **not** reverted, for the reason above:
  each of its repairs is correct independently of the number that found them.

---

## 5. What this excludes / 제외하는 것

- The **k8s probe** (`spikes/nat/k8s/`), which is equally "unrun" and may be
  equally wrong about that — but the runner has no cluster, so it is a
  condition and stays one. (2026-09-04: that premise turned out to be the
  buildable kind — the runner has docker, and `kind` makes a cluster on it.
  The plan that challenged it is `PLAN-FIRST-COMPLETION` §G lane E, and the
  same day the probe **ran and demonstrated on its first execution** there;
  this record stays as written for its date. *그 전제는 지어서 없앨 수 있는
  부류였고, 같은 날 그 프로브는 첫 실행에서 실행되고 증명했다 — 이 기록은
  그 날짜의 것으로 남는다.*)
- Making the VM probe run on CI. It cannot (no multipass); it stays a counted
  `SKIPPED` there, correctly.
- The 41-minute run itself. It did not reproduce (24m on the next push) and
  202s does not account for 17 minutes; that run stays **not diagnosed**, and
  the group stamps are what will explain the next one.

---

## 6. What this bears on / 걸리는 곳

D029 §6.1's **Location** row names R7 as its instrument. This plan changes no
measurement — the probe already passes — and no standing. What it changes is
whether the repository *knows* the measurement is being taken, which is the
difference between a row with an instrument and a row with a rumour.

*측정도 표준도 바꾸지 않는다. 바꾸는 것은 저장소가 그 측정이 이루어지고 있음을
**아는가**이며, 그것이 계기가 있는 행과 소문이 있는 행의 차이다.*

---

## 7. Cost / 비용

Lane A is shell in one group and a five-shape control. Lane B is a
`.dockerignore` derived from `.gitignore`, a Dockerfile, and two local checks.
The merge is a header and a sentence. Run in parallel, the wall-clock is the
longer lane plus one merged harness run; the CI pushes are one per lane and one
for the merge, and the first of lane A's is the one that answers §4's first row.
