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
7. **Where the 202s would go, if it is the probe**: `spikes/nat/Dockerfile`
   does `COPY . .` and `cargo build --release -p orbweaver-giop --bin spike-nat`
   **inside the container**, from a `rust:1.85-slim` image — a cold release
   build of the crate and its dependencies, on every push, with no cache, to
   produce a binary the runner has already built. Read off the Dockerfile; not
   yet timed on CI, because nothing on CI prints the probe's own lines.

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

And if the probe is running and passing, **it has never had its negative
control run**, because nobody knew it was running — and if it is not, the 192s
is something else and this plan's S3 is the wrong repair. Either way the first
step is the same: make the harness say what happened. Its header says *expect to fix it, not to confirm
it, and treat a first green run with suspicion.* Every run so far has been a
first green run nobody treated with anything.

*세 기록이 어긋나고 어느 것도 빨갛지 않다. 하네스의 오귀속은 **안전한 방향의 초록**이다
— 실제보다 적게 측정했다고 보고한다. 거짓 빨강은 한 시간 안에 발견되고 거짓 건너뜀은
전기 요금으로 발견된다. 그리고 통과하는 탐침은 **부정 대조군이 한 번도 돌지 않았다** —
도는 줄을 아무도 몰랐기 때문이다.*

---

## 3. The work, in order / 작업, 순서대로

### S1 — the harness asks the script which probe skipped, and which ran

`nat_rewrite.sh` prints one `skip` line per absent probe (vm, container, k8s),
each naming what is missing, and one `pass` line per probe that ran. The
harness reads the count and guesses. It reads the **lines** instead, and prints
one counted `SKIPPED` per absent probe, naming it — and one `ok` per probe that
ran, quoting the probe's own pass line, so that **a probe running on CI is
visible in the CI log for the first time**. This is the step that turns §1.5's
circumstantial case into a reading.

- **Measurement:** on this machine (docker absent, multipass present with the
  VM running) the group reads `ok vm … SKIPPED container … SKIPPED k8s`; on CI
  it reads whatever is true there, and for the first time the log says which.
- **Control:** feed the group synthesised `nat_rewrite.sh` output — all three
  skipped, none skipped, and each alone. Five shapes, five different verdict
  lines. The harness's current code produces one sentence for all of them.

### S2 — the container probe's header stops lying, and its control runs once

Strike `THIS SCRIPT HAS NEVER BEEN RUN` and replace it with when and where it
first did (CI, ubuntu-24.04 runner, the first push after docker landed in the
runner image — find the run, do not estimate it). Then run the negative control
the header asks for: the probe with the rewrite **disabled** must fail on the
naive publish, on CI, once, and the run is cited.

- **Measurement:** a CI run where the probe is deliberately broken and goes red.
- **Why on CI:** there is no docker here, and a control that cannot run where
  the subject runs is a claim.

### S3 — the 202s becomes ~5s, without changing what is measured

The Dockerfile builds `spike-nat` from source inside the container. The runner
has already built it for `cargo test`. Two ways to stop paying twice:

| | | cost |
|---|---|---|
| A | `COPY` the runner's `target/release/spike-nat` into a `debian-slim` image — no Rust in the container at all | one `cargo build --release --bin spike-nat` the harness already does elsewhere; image build ~seconds |
| B | keep the in-container build and add a BuildKit cache mount | still a cold build on a fresh runner; saves only on a warm one |

**A, and the reason is the measurement, not the minutes.** The probe's claim is
about *routing* — a client that cannot reach the servant's bound address — and
a binary built on the host measures that identically. The in-container build
measures nothing the probe is about; it is there because the Dockerfile was
written on a machine that could not run it, by someone who could not know what
the runner would have.

- **Measurement:** the group's stamped time on CI drops from ~200s to under
  20s **and the probe's pass line is unchanged**.
- **What must not happen:** the probe passing because the container can reach
  the host's loopback. That is the naive case and it must still **fail**; if A
  changes the network shape, the control in S2 catches it, which is why S2
  lands first.

### S4 — the six SKIPPED become an honest count

After S1 the harness's verdict on CI reads one fewer `SKIPPED` (the container
probe measured) and one more on this machine that was already there (the VM).
`docs/PLAN-FIRST-COMPLETION.md` §D lists *docker* among the conditions this
machine lacks; it stays true of this machine and becomes false of CI, and the
document says which.

*S1 하네스가 스크립트에게 어느 탐침이 건너뛰었는지 **묻는다**(세지 않고 읽는다). S2
헤더의 거짓말을 지우고 **부정 대조군을 CI에서 한 번** 돌린다 — 대상이 도는 곳에서
돌 수 없는 대조군은 주장이다. S3 202초를 ~5초로 — 분 때문이 아니라 측정 때문이다:
컨테이너 안의 빌드는 탐침이 주장하는 것(라우팅)에 대해 아무것도 재지 않는다. S4
SKIPPED 수가 정직해진다.*

---

## 4. What would make this plan wrong / 이 계획이 틀리는 경우

- **If the container probe is not what runs on CI.** The first draft of this
  plan cited a log line as proof and the line was another group's — recorded in
  §1.5 rather than smoothed over. **S1 is what settles it**, on its first CI
  push, before S2 edits any header or S3 touches the Dockerfile. If the probe
  has never run, the header was right, the 192s is elsewhere, and this plan
  collapses to S1 plus a fresh reading of the group stamps.
- **If option A changes the network shape.** A binary copied in is the same
  binary; the image's base and the `docker run` flags are what decide routing,
  and S3 changes neither. S2's control is the check.
- **If 202s is not the build.** §1.7 reads it off the Dockerfile; S3's stamped
  time on CI is the measurement, and if it does not move the cause is elsewhere
  and S3 is reverted rather than kept for tidiness.

---

## 5. What this excludes / 제외하는 것

- The **k8s probe** (`spikes/nat/k8s/`), which is equally "unrun" and may be
  equally wrong about that — but the runner has no cluster, so it is a
  condition and stays one.
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

S1 is shell in one group and a four-case control. S2 is a header and one CI
push that must go red. S3 is a Dockerfile. S4 is a sentence. The largest item is
the CI push in S2, because it has to fail on purpose and be cited.
