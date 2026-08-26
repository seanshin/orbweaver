# D028 — Work that is correct apart and wrong together

**STATUS: PROPOSED** — drafted 2026-08-26 on a request to review the solutions
in detail. Every figure was measured that day, on the tree as merged. Not
self-approvable: §5 proposes a change to how batches are commissioned and
landed, which is a rule about every future batch.

**상태: 제안** — 2026-08-26, 해결책을 디테일하게 검토하라는 요청에서 작성. 모든
수치는 병합된 트리에서 그날 측정했다.


> **Priority zero, set 2026-08-26.** This document is subordinate to the ORB
> completion criterion, whose home is
> [`D029`](D029-what-a-complete-orb-would-mean.md) §6: *no leak in the
> transparency that a caller can invoke any target holding only a reference,
> without knowing its location, backend, language or load state, and that this
> survives targets being added, removed, moved, loaded or evicted at runtime.*
> The criterion is stated there and **not restated here** — what is recorded
> below is only how this document's work bears on it.
>
> *0순위 기준의 집은 D029 §6이며 여기서 다시 적지 않는다. 아래에 적는 것은 이
> 문서의 작업이 그 기준에 **어떻게 닿는지**뿐이다.*

> **How this bears on it.** Break 1 is the worked example: `Connection::move_to`
> restored a hand-written field list and dropped two configured limits, so a
> caller's limits silently changed when its target *moved*. That is a location
> transparency leak that arrived through a merge, which is why §4's M1 and M2
> are transparency work and not only hygiene.

---

## 1. The shape both problems share / 두 문제가 공유하는 모양

Today produced two findings that look unrelated and are one thing.

**Five merge breaks**, each a pair of batches that were individually green:

| # | What broke | Why nothing caught it |
|---|---|---|
| 1 | `ChannelStats` gained three fields; `orbweaver-console` builds it as a literal | no line touched twice |
| 2 | `emitted/f_ir_subset.rs` — two wanted changes to one blessed artifact | resolved by re-blessing |
| 3 | `dkprobe.idl` placed in `spikes/`; the corpus gate mirrors `differential.sh`, which globs `spikes/*.idl` | each batch read a different authority |
| 4 | the harness-groups branch reported merged and was not — my check did not verify the branch was ahead | the merge succeeded, emptily |
| 5 | `spike-seeded-trading` calls `Server::bind`; step 4 made it `pub(crate)` the same day | no line touched twice |

**Three population disagreements**, verified independently today:

- **Node namespaces are three disjoint sets for one modelled domain.**
  `spike_tenants` declares `gpu-eu-1`, `gpu-us-1`, `gpu-nowhere`;
  `spike_experts` places every expert on `gpu-04`; the trading seed uses
  `node-a`, `node-b`, `node-c`. Nothing checks a node against another
  fixture's declaration, because they are separate processes.
- **`vision` disagrees in kind**, not in value: one fixture asserts it is a
  capability a tenant is *refused*, the other registers it as a resident
  expert. Both are true of their own process.
- **A key collides with itself.** `spike_experts` binds its server to
  `b"MoE/registry"` and hands `ExpertService` the base `b"MoE"`, from which it
  derives a registry key — the same bytes, arrived at twice.

**The shape:** *every piece is correct in the frame its author could see, and
the defect exists only in a frame nobody occupied.* `git merge-tree` cannot see
it because no line is touched twice. Review cannot see it because each diff is
right. Only **running the combined thing** sees it, and all five breaks were
found that way — three by `cargo test` on the merged tree, one by the harness,
one by CI.

*모든 조각이 저자가 볼 수 있던 틀 안에서 옳고, 결함은 아무도 있지 않았던 틀에만
존재한다. 병합 도구도 리뷰도 볼 수 없다 — **합쳐진 것을 실행하는 것**만 본다.*

## 2. Why this is worth a document / 왜 문서인가

Because the project's answer to every other defect class has been *make it
impossible rather than detectable*, and this class currently has neither.

The rate is the argument. Five in one day is not bad luck: it is what happens
when parallel batches are commissioned by footprint — *"you hold `crates/x`,
they hold `crates/y`"* — which is a rule about **files** applied to a hazard
about **meaning**. Breaks 1 and 5 are the same defect: a type or a function
changed in one crate and named in another, where the footprints were disjoint
and the fact was not.

**The population disagreements are the same rule failing at a longer range.**
Each fixture owns its process, so each invents its world, and "the same node"
is a claim no boundary was ever asked to check.

## 3. What must not happen / 해서는 안 되는 것

- **Do not stop running batches in parallel.** Five breaks cost roughly an hour
  today against a day of work that could not otherwise have happened. The
  answer is a cheaper way to find them, not fewer of them.
- **Do not make one population the only population.** D026 §3 already says it
  and it bears repeating: `wire-fuzz` and the property tests exist because a
  fixed population is a fixed set of paths. Unifying the fixtures' worlds must
  not delete their ad-hoc cases.
- **Do not "fix" a disagreement by picking a winner silently.** `gpu-04` and
  `gpu-eu-1` are not a typo; one fixture models a placement domain and another
  models a tenancy domain, and the honest outcome may be **two named domains**
  rather than one vocabulary.
- **Do not add a gate that only the coordinator runs.** The differential's
  lesson from yesterday: *a prohibition without its replacement is an
  instruction to skip the check.* Anything proposed here has to run for
  whoever is holding the branch.

## 4. What is proposed / 제안

### M1 — the merged tree is compiled before the merge, not after (`spikes/`)

Every break of type 1 and 5 was found by `cargo test` **after** the merge
landed on `main`. `git merge-tree --write-tree` already computes the merged
tree object without touching the working tree; nothing today builds it.

A script that, for a set of branches, materialises each pairwise merge and runs
`cargo check --workspace --all-targets` on it. **`check`, not `test`** — the
five breaks were all compile failures, and a check is minutes where a test run
is tens of minutes, which is the difference between a thing that runs and a
thing that is skipped.

**What it does not catch, and must say so:** a merge that compiles and behaves
differently. Break 3 was of that kind — `dkprobe.idl` compiles fine and the
gate reads a file list. So M1 is a floor, not a proof, and the script prints
that sentence.

### M2 — a fact that crosses a crate boundary names its dependents (`spikes/`)

Breaks 1 and 5 have a mechanical signature: a **public item changed in crate A
and named in crate B**, where the two batches held disjoint footprints. That is
computable from the diff without building anything — for each public item whose
signature or visibility a branch changes, list the crates that name it.

The output is for the person commissioning batches, not a gate: *"this branch
changes `Server::bind`'s visibility; six crates name it"* is exactly what
neither footprint list said. Break 5 would have printed one line.

**The honest limit:** this finds the class it was built from. It will not find
break 3, whose two authorities were a shell glob and a Rust constant.

### M3 — the domains get named, and only then unified (`corpus/state/`, docs)

The three disagreements are not one job. Taken in order of what is actually
known:

1. **The key collision is a defect and is fixed.** `MoE/registry` arrived at
   twice is a bug in `spike_experts` with no design question attached.
2. **The node namespaces need a decision before a seed.** Whether `gpu-eu-1`
   and `gpu-04` are one domain spelled twice or two domains is a modelling
   question, and D026's seed cannot state a population until it is answered.
   The answer may well be **two**, in which case the seed states both and the
   disagreement was never a defect — that is a real outcome and cheaper than
   the alternative.
3. **`vision` is the interesting one and probably not a defect at all.** A
   capability that is *absent* in a tenancy fixture and *resident* in a
   placement fixture may be exactly right; what is wrong is that no document
   says they are different worlds. It is D023's third row again — absent by
   accident rather than by decision — one scale down.

**Then, and only then, the migration D026 S1 still owes.** The seed batch
established the disagreements **by reading** and could not migrate the five
fixtures, because all five live in crates other batches held. Its byte-identity
oracle — every migrated fixture produces identical output or the difference is
explained — is unrun and still the right oracle.

### M4 — the landing order stops being a coordinator's memory (`spikes/`)

Break 4 was mine: I merged a branch that was zero commits ahead and reported
success, because my one-off command checked `merge-tree` and not `rev-list
--count main..branch`. The loop I had been using did check it; the one-off did
not, and I wrote the one-off because I was in a hurry.

One script for landing a branch, holding all the checks the loop had — ahead of
main, conflict-free, and (with M1) compiles merged. A rule that lives in a
coordinator's habit is a rule with a bad day in it.

## 5. The rule / 규칙

**A batch is commissioned by footprint *and* told which facts it may change
that others name.** The footprint rule stays — it works, and it prevented every
textual conflict today. What it does not do is bound *meaning*, so a batch that
will change a public signature, a struct's fields, a file list a gate reads, or
a constant another crate matches on, is told so and the coordinator is told
who else names it.

*배치는 footprint로 위임하되, **다른 배치가 이름하는 어떤 사실을 바꿔도 되는지**도
함께 듣는다. footprint 규칙은 남는다 — 오늘 텍스트 충돌은 전부 막았다. 다만 그것이
묶지 못하는 것이 **의미**다.*

## 6. What this document does not claim / 주장하지 않는 것

It does not claim five breaks in a day is a crisis: every one was found the
same day, by a gate or a run, and the cost was an hour. It does not claim M1
would have caught all five — it would have caught two, and §4 says which two
and why. It does not claim the node namespaces are a defect; §4 M3 says the
answer may be that they are two domains, and that outcome closes the finding
without changing a line of fixture code. And it does not claim the population
migration is cheap: it is five fixtures across three crates, and the reason it
has not happened is that those crates have been held by other work all day —
which is itself an instance of §1's shape, arriving in the schedule rather than
in the code.
