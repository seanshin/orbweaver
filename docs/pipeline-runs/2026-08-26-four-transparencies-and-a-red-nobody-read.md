# 2026-08-26 — four transparencies in parallel, and a red nobody could read / 병행 네 투명성, 그리고 아무도 읽을 수 없던 붉음

> Four batches run in parallel against D029 §6.1's five rows, all four landed,
> v0.7.0 cut. Kept for three things that are not about CORBA: **two batches
> deliberately declined to move their row**, the harness's own control pair
> broke in opposite directions when the project made progress, and CI had been
> red for eight pushes in a form that produces **no failing step and no
> fetchable log**.
>
> D029 §6.1의 다섯 행을 겨냥한 네 배치를 병행으로 돌려 전부 착지, v0.7.0 태그.
> CORBA와 무관한 세 가지 때문에 남긴다: **두 배치가 자기 행을 일부러 움직이지
> 않았고**, 하네스 자신의 대조군 짝이 프로젝트가 진전하자 반대 방향으로 깨졌으며,
> CI가 **실패한 단계도 가져올 수 있는 로그도 남기지 않는** 형태로 여드레치 붉었다.

## 1. The four / 넷

| | footprint | what it did | the row |
|---|---|---|---|
| P1 | `orbweaver-object` | `ExpertHost` — a POA and the loader for its ids under one lock | **did not move**, on purpose |
| P2 | `orbweaver-giop` | `Dispatch::knows`'s accept-every-key default decided wrong | Backend moved |
| P3 | `orbweaver-gen` + the bridge | the servant seam stopped being Python's; an object reference crosses it | Language moved |
| P4 | `spikes/` | omniORB forwards our client to a second process at a different address | **did not move**, on purpose |

Footprints were disjoint by crate, which held: the only merge conflicts were
D029 §6.1's table (three batches, three different rows — resolved by taking each
batch's own row) and CHANGELOG (two independent entries, both kept). No textual
conflict in code, and `cargo check --workspace --all-targets` was run after each
merge rather than at the end.

## 2. The two that declined / 움직이지 않은 둘

This is the day's most transferable result, and it was not asked for.

P1 mounted `ExpertLocator` — the closure a previous batch had left *available
and unchosen*, with zero references outside `residency.rs`. It could have
claimed the Activation row. It did not: **a mount is an adoption, not a second
closure**, the row already read *closed at the POA*, and what actually changed
is that the row's open item (2) — *"nothing in the tree mounts it"* — became
false and was struck in both language halves in one commit.

P4 made a foreign ORB forward us, which no run had ever done: every
`LOCATION_FORWARD` this ORB had followed, it had written itself. It could have
claimed the Location row. It did not: **neither named leak is touched**, and
what changed is the evidence base behind the word "measured".

Both are right, and the discipline is worth stating as a rule: *recording a row
as unmoved is worth more here than moving it*, because a row that moves on an
adoption or on better evidence stops meaning what §6 says it means.

*마운트는 두 번째 닫기가 아니라 채택이고, 외부 피어의 포워드는 구멍이 아니라
"측정됨"이라는 단어 뒤의 증거를 바꾼다.*

## 3. What a batch found by being told to verify its own premise / 전제를 검증하라고 시켰더니

Each brief said to verify the leak it was given before building on it. Three
came back with the premise **wrong in the direction of worse**:

- P3 was told "an object reference cannot cross the seam". It found **three**
  leaks, not one — the foreign servant could not mint, could not tell which
  object it was, and claimed every key in the process. Two calls addressed to
  *different object keys* produced identical call documents. It was not a leaf;
  it was a **singleton** leaf. And §6.1.1's recorded reason — *"having no POA on
  its side"* — named the wrong absence: §4.7 forbids the far side holding an
  address at all, so what was needed was for **this** side to mint one.
- P2 was told the `knows` default might simply be correct for the shape it was
  written for. It found the specification makes the permissive policy
  **unreachable by omission** (§15.3.8.6 requires the POA be *created with*
  `USE_DEFAULT_SERVANT` and a servant *registered*, raising `OBJ_ADAPTER`
  otherwise) — and that this ORB had already decided it twice the other way, in
  `policy.rs` and `skeleton.rs`, each in writing. **The fact had three homes and
  the GIOP one disagreed with the other two.**
- P1 was told to answer *who owns an expert's server*. The answer is an
  argument rather than a preference: a POA consults a locator only for ids
  inactive **in that POA**, so owning the POA without the loader means answering
  `Here` for what you cannot load, and owning the loader without the POA means
  loading what nothing will dispatch to.

P4 found three things nobody asked about, of which one would have produced a
false claim of coverage: **`ForwardRequest` cannot express permanence** — it
carries a reference and no flag — so a `ServantLocator` reaches status 3 only,
and raising omniORB's `LOCATION_FORWARD` from `preinvoke` gives
**SYSTEM_EXCEPTION (2)**, not status 4. A fixture assuming one mechanism served
both would have measured status 3 twice and called it coverage of both.

## 4. The control pair that broke in opposite directions / 반대 방향으로 깨진 짝

The harness's release run failed three groups on two causes. The second is the
one worth keeping.

`ledger_control.sh`'s controls 2 and 8 demonstrate one property of the
transparency ledger — *a group that declares a transparency and measures none of
it must not flip the row to measured* — and both demonstrated it by **typing the
name `activation`**. That pin outlived its fact twice in a single day: first
activation went from undeclared to declared-and-measuring-nothing and the string
was edited to match; then P1's leg started actually measuring and
`tp_measures_nothing` came off it.

**Control 2 went red. Control 8 went green while exercising nothing** — and
control 8 exists precisely to prove control 2 is not tuned-until-quiet. It did
so by asserting a flip that no longer had anything to flip. Run as a negative
control afterwards, **four of its five assertions are still green in that
vacuum**; only the guard added today fails. One half broke loudly, the other in
silence, and only the loud half would ever have been looked at.

**Computing the name instead of typing it is the wrong repair, and it was tried
first.** Deriving "transparencies declared only by measures-nothing groups" from
the tags returns the **empty set** today: `language` and `lifecycle` hold the
two remaining markers and are both also declared by groups that measure them for
real. `activation` had satisfied the condition by *arithmetic accident* — it had
exactly one declaring group. A computed subject makes the control's existence
depend on what the project happens to still be waiting on, which is the thing
that broke in the first place.

The repair is a **synthesised** subject: two groups, one marked, one measuring a
different transparency for real. The companion is load-bearing rather than
decorative — with only the marked group the run measures nothing at all, the
verdict takes its `NONE measured in this run` branch, and the half this control
exists to read never prints. That was caught by running it, not by writing it.

*이름을 계산하는 수리는 틀렸고 먼저 시도해서 틀렸다. 대상은 합성한다.*

## 5. The other cause, and why it is a good sign / 다른 원인, 그리고 그것이 좋은 신호인 이유

Two of the three FAIL groups were one test: P4 added `spikes/foreign_forward.idl`
and nothing ran the differential, and the corpus gate globs `spikes/*.idl`. This
is the second time in one release that an IDL under `spikes/` did this.

It is nonetheless the codification **working**. When `dkprobe.idl` did it, the
gate read a shell glob and a Rust constant and the two authorities disagreed in
silence. The verdict is now checked-in data compared by an oracle-free test, so
it went red **for everybody** rather than staying green for whoever did not run
the harness.

## 6. CI: a red that produced no failing step / 실패한 단계를 남기지 않은 붉음

Found only because the release was being wrapped up. **The interop job had
failed on eight consecutive pushes, including the one tagged v0.7.0**, and every
local signal was green throughout.

The runner ran out of disk. Not a step failing — the runner *process* died:

```
Unhandled exception. System.IO.IOException: No space left on device
  : '/home/runner/actions-runner/cached/2.336.0/_diag/Worker_*.log'
```

So the `harness` step is left reading `in_progress` with a **null conclusion**,
`gh run view --log-failed` answers **`log not found`**, and the cause exists only
in the check-run annotation. Three separate readings of the run — job list, step
list, log fetch — each said something that was true and not the answer. The job
duration (22 min) sat inside the range of its own successful runs (17, 21), so
even the shape did not look like a timeout, and no `timeout-minutes` is declared
anywhere in the workflow to make one plausible.

**A red that takes an API call to read is a red nobody reads**, and this one
meant the interop harness had not run in CI for a day — an unmeasured check,
which this project's own rule calls a failure and never a pass.

Fixed by reclaiming the ~30 GB of toolchains `ubuntu-latest` ships and this job
never touches. The reclaim is the cheap half; the load-bearing half is that the
job now **prints the margin** — a floor with an `::error::` if the reclaim
leaves under 20 G, and an `always()` step printing `df` and the four directories
that grow, so the next crossing arrives as a legible number instead of a runner
crash.

## 7. What this record does not claim / 주장하지 않는 것

It does not claim the four batches closed the criterion: **two of the five rows
deliberately did not move**, one moved to a single remaining leak, and the fifth
is blocked on an owner decision (**X** — that the reference `Orb::server` hands
out is *indirect*; a forward is a reply, a reply needs a listener, and a removed
server is not listening). It does not claim the CI fix is verified — it is
pushed and the next run is its first measurement, which is stated here rather
than assumed. It does not claim `COMPONENTS.md` was re-reviewed for v0.7.0; that
document now says so itself, because `records_keep_up.py` checks only that a
record was *opened*, never that it is *true*. And it does not claim the parallel
scheme is free: the release run had to be **stopped and restarted** because
three agents plus the harness drove the machine to load 50, and a deadline-bound
group's FAIL under that load is not evidence.
