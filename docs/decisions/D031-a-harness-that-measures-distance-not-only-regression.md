# D031 — A harness that measures distance, not only regression

**STATUS: PROPOSED** — drafted 2026-08-26 on a direction to restructure the
harness into stages toward completion and to run feedback → improve →
re-measure as a loop. Measured that day against `spikes/run_checks.sh` as it
stands. Not self-approvable: §4 changes what the harness's verdict means, which
is the instrument every batch is judged by.

**상태: 제안** — 2026-08-26, 하네스를 완성도를 높이는 단계로 개편하고 피드백–개선–
재측정을 루프로 돌리라는 지시에서 작성.

> **Priority zero.** The completion criterion's home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6 and is **not restated
> here**. This document is about giving that criterion an instrument.

---

## 1. What the harness can and cannot answer / 답할 수 있는 것과 없는 것

Measured 2026-08-26: **81 groups, 3885 lines.** The verdict is a failure count,
a skip count, and — as of today — a split between skips that are absences and
skips that are replays.

**It answers "did anything regress?" extremely well.** It found six merge
breaks, a licence gate that could not go red, a peer probe that was green over
swapped fields, and a group that reported a failure it could not describe.

**It cannot answer "are we closer to complete?"** Nothing in it knows the five
transparencies exist. Today that question was asked directly and the answer had
to be assembled by hand from four batch reports, a decision document and a
`grep` — which is a reading, and this project spent the day learning what a
reading is worth.

**The gap is not a missing group. It is a missing dimension.** Every group
answers *did this break*; none answers *what can a caller still tell*.

*하네스는 "퇴행했나"에 아주 잘 답한다. "완성에 가까워졌나"에는 답할 수 없다 —
없는 것은 그룹 하나가 아니라 **차원 하나**다.*

## 2. What must not happen / 해서는 안 되는 것

- **Do not reorder the 81 groups.** They are in historical order, each landed
  with its own negative control, and several share fixtures started by an
  earlier group — the IFR `--hold` fixture serves three. Reordering is a large
  change with **no measurement behind it** and it would put the instrument at
  risk to make a document look tidy. The staging this direction asks for is a
  *reading* of the groups, not a rewrite of them.
- **No completion percentage.** *A floor is not a figure*, and a percentage is
  the worst version of that: it would move when a group is added, invite being
  quoted in prose, and be wrong the moment a leak is found rather than closed —
  which is what finding a leak *is*. Today's honest movement was **negative**
  and a percentage would have hidden it.
- **No group loses its verdict.** A group that fails must still fail. The
  ledger reads the run; it does not replace it.
- **No hand-maintained tag.** *A classifier is a sentence too* — a group tagged
  with a retyped transparency name will drift from D029 §6.1 silently. The five
  names have one home and both the groups and the ledger must read it.

## 3. What the harness knows and does not use / 이미 아는데 쓰지 않는 것

Several existing groups **already measure a transparency** and none says so:

| Group | Transparency it bears on |
|---|---|
| `LOCATION_FORWARD_PERM` — status 4 from a generated skeleton, omniORB following it | Location |
| `object-reference acquisition — corbaname: through a real naming service` | Location |
| `ORB initial references — corbaloc:rir: out of OUR table` | Location |
| `Python client target — generated Python against the omniORB fixture` | Language |
| the MoE residency spikes | Activation |
| `event channel — omniORB is the pull supplier` | Backend |

**So the first version of the ledger is mostly a re-reading of work already
done**, which is what makes it cheap and what makes it honest: it will report
that four transparencies are partly measured and one is not, from groups that
are already green.

## 4. What is proposed / 제안

### H1 — every group declares what it bears on, from one home (`spikes/`, `crates/orbweaver-test`)

The five transparency names become a **constant with one owner**, read by the
harness and by the ledger. A group declares `bears_on <name>` or declares
nothing; declaring nothing is normal and most groups will.

**A name that is not in the constant is a failure, not a typo** — that is the
`dk_peer` lesson, where an expected table was checked against the peer's own
enum before any leg ran so that a typo failed as *our* table.

### H2 — the ledger, computed from the run (`spikes/`)

A final section printing, **per transparency**: how many groups measured it,
how many of those went red, and — the load-bearing column — **what is named as
unmeasured**. Not a score. A row reads:

```
  Location      measured by 4 group(s), 0 red
                unmeasured: the reverse half (a client that cannot be dialled) — PLAN-DEFERRED §22
                unmeasured: LocateReply OBJECT_FORWARD cannot carry an IOR
```

The unmeasured lines are the ones a next batch is scoped from, and they come
from the groups' own `SKIPPED` reasons plus D029 §6.1 — **the ledger cites, it
does not restate.**

### H3 — the loop closes, mechanically (`spikes/`)

`--ledger` emits the same content machine-readably, so **commissioning the next
batch stops being a coordinator reading four reports.** That is exactly what
happened today and it took an hour and produced one wrong sweep.

The loop is then: run → ledger names leaks → a batch per leak, scoped from the
ledger → run again. **The measurement of the loop working is that the ledger's
unmeasured list shrinks between runs** — and it will sometimes grow, which is
finding a leak, and the ledger must make that legible rather than look like a
regression.

### H4 — the leak tests themselves (D029 §5 O0)

H1–H3 give the ledger something to *read*; O0 gives it something to read about
the transparency nobody measures. It is listed here so the order is stated:
**H1 and H2 are worth doing before O0**, because a leak test with nowhere to
report lands as one more green group.

## 5. The honest cost / 정직한 비용

The harness takes **over half an hour** under load and is a machine-wide lock.
Adding a ledger section costs nothing at run time — it reads what ran. Adding
O0's leak tests costs fixtures and minutes, and **that is the argument for the
ledger first**: it tells you which leak test is worth its minutes.

**And the loop has a cost this document should not hide.** Today the harness
was started twice: once on a tree I then changed under it, whose verdict I had
to discard, and once cleanly. A loop that runs a 35-minute instrument more
often will meet that hazard more often, and the discipline — *freeze the tree,
merge only after the verdict* — is what makes the loop honest rather than fast.

## 6. What this document does not claim / 주장하지 않는 것

It does not claim the 81 groups are badly organised — §2 refuses to reorder
them and §3 says six of them already measure transparencies without knowing
it. It does not claim the ledger will find anything new on its first run: the
likely first result is that it prints what four batch reports said today, which
is worth having precisely because it will then print it every time without
anybody assembling it. And it does not claim a shrinking unmeasured list means
progress — a list shrinks when a leak is closed **and** when somebody stops
looking, and only the first is progress, which is why every removed line has to
name the run that closed it.
