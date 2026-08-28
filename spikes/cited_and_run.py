#!/usr/bin/env python3
"""Every executable a document cites either runs, or says it does not.

Four times on 2026-08-28 a document named a script as its evidence and nothing
ran that script, so the evidence was never taken:

  spikes/c_peer.sh          cited by C-PEER-STATUS and D029's Backend row. It
                            had never been compiled on Linux. Its first harness
                            run failed on a glibc `-Werror=format-truncation`
                            the author's macOS clang cannot produce.
  spikes/event_by_name.sh   cited by D029's "Location, for event channels" as
                            the half that makes the claim "a measurement rather
                            than a self-test". `grep -c` over `run_checks.sh`
                            and `ci.yml` both returned 0.
  spikes/scope_controls.sh  the negative control for two scope widenings. Run
                            by nothing — and it had also stopped being able to
                            run, because the widening it controls gained a
                            `git ls-files` scan and the control feeds it a tree
                            `git archive` extracted, which has no `.git`.
  spikes/half_reply.sh      cited by COMPONENTS.md and D017. Its own row in
                            COMPONENTS said "not yet a `run_checks.sh` group",
                            and had said so since the day it was written.

**Three of the four said so in their own headers**, and that is the finding
rather than a detail. `c_peer.sh`'s status record named "the recommended group";
`event_by_name.sh` wrote *"Wiring it in is one `hr` group and is named as undone
in the report"*; `half_reply.sh`'s row said "not yet a group". Naming a debt is
not paying it, and a debt narrated in a header is a debt nobody counts.

# What this refuses, and what it accepts

It accepts a script that is **run** — by `run_checks.sh`, by `ci.yml`, or by
another script that is itself run (one level: `service_sweep.py` is invoked by
`service_sweep.sh`, which is a group).

It accepts a script that **states a refusal**: `crossing_facts.py`,
`gap_symbols.py`, `plan_numbers.py` and `bilingual_drift.py` all say *"a report,
not a gate"*, and CLAUDE.md says why — there is no defensible threshold for what
they measure. A decision not to gate is a decision, and this asks only that it
be written down.

It refuses a script whose header **defers** the wiring — "not wired into",
"named as undone", "the recommended group", "wiring it in is". That is the exact
sentence three of today's four findings carried, and the difference between it
and a refusal is the whole point of this gate: one is a choice, the other is an
IOU nothing was counting.

# What it does not do

It does not read `tests/*.rs`. Those run under `cargo test --workspace`, which
is a group, so a Rust test cited in a document IS taken — the first sweep that
found this class reported 53 hits and 47 of them were that false positive.

It does not check that a script that runs is run *usefully*. A group that
executes a script and asserts nothing is the class this project calls
green-while-measuring-nothing, and no scan of citations can see it.

*문서가 인용하는 실행물은 돌거나, 돌지 않는다고 자기 안에 적어야 한다. 2026-08-28에
네 번 나왔고, 그중 셋은 자기 헤더에 "아직 안 했다"고 적어두고 있었다 — 서술된 빚은
아무도 세지 않는 빚이다. **거절은 결정이고 유예는 차용증이다.** `tests/*.rs`는 읽지
않는다: `cargo test --workspace`가 그룹이므로 인용된 러스트 테스트는 이미 돈다.*
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Documents whose citations count. `docs/pipeline-runs/` and `PHASE*.md` are
#: dated records — they state what was true on a day, so a script they name may
#: since have been deleted on purpose, and reading them would turn history into
#: a debt.
DOC_GLOBS = ["docs/*.md", "docs/decisions/*.md", "CLAUDE.md", "README.md"]

#: A header that says the script is deliberately not a gate. Accepted.
REFUSES = re.compile(
    r"report,? not a gate|not a gate|control,? not a gate|never a gate", re.I
)

#: A header that says the wiring is owed. Refused — this is the sentence all
#: three of 2026-08-28's findings carried.
DEFERS = re.compile(
    r"not wired into|named as undone|recommended group|wiring it in is|"
    r"is one `?hr`? group and",
    re.I,
)

CITE = re.compile(r"spikes/[A-Za-z0-9_./-]+\.(?:sh|py)")


def cited():
    """Every `spikes/…` executable a living document names, and where."""
    out = {}
    for g in DOC_GLOBS:
        for d in sorted(ROOT.glob(g)):
            try:
                text = d.read_text()
            except (OSError, UnicodeDecodeError):
                continue
            for m in CITE.finditer(text):
                rel = m.group(0)
                if (ROOT / rel).is_file():
                    out.setdefault(rel, set()).add(d.name)
    return out


def runners():
    """Text of everything that could invoke a spike: the harness and CI."""
    parts = []
    for rel in ("spikes/run_checks.sh", ".github/workflows/ci.yml"):
        p = ROOT / rel
        if p.is_file():
            parts.append(p.read_text())
    return "\n".join(parts)


def main(argv):
    probe = "--probe" in argv
    run_text = runners()
    if not run_text.strip():
        print("  FAIL neither the harness nor the workflow could be read — this scan")
        print("       measured nothing, which is a failure and never a pass")
        return 2

    # `spikes/**`, not `spikes/*`. The first draft globbed one level and reported
    # `spikes/jacorb/setup.sh` as owing a group while `ci.yml` runs it by that
    # exact path — a scan that cannot see a file cannot be trusted about its
    # silence either. Keyed by path-relative-to-root AND by basename, because a
    # runner may spell it either way.
    spikes = {
        str(q.relative_to(ROOT)): q.read_text(errors="replace")
        for q in ROOT.glob("spikes/**/*")
        if q.is_file() and q.suffix in (".sh", ".py")
    }

    def named_in(text, rel):
        return rel in text or pathlib.Path(rel).name in text

    # TRANSITIVE, not one level. `trading_client.py` is invoked by
    # `service_sweep.py`, which is invoked by `service_sweep.sh`, which is a
    # group — three deep. A fixed depth is a threshold in disguise, and this one
    # was wrong at depth 1.
    reached = {rel for rel in spikes if named_in(run_text, rel)}
    changed = True
    while changed:
        changed = False
        for rel in spikes:
            if rel in reached:
                continue
            if any(named_in(spikes[r], rel) for r in reached):
                reached.add(rel)
                changed = True

    owed, refused, ran = [], [], []
    for rel, where in sorted(cited().items()):
        head = "\n".join(spikes.get(rel, "").splitlines()[:24])
        if rel in reached:
            ran.append(rel)
        elif DEFERS.search(head):
            owed.append((rel, sorted(where), "its header defers the wiring"))
        elif REFUSES.search(head):
            refused.append(rel)
        else:
            owed.append((rel, sorted(where), "no group, and its header says nothing"))

    if probe:
        return 0

    print(f"  {len(ran)} cited spike(s) run; {len(refused)} state a refusal;"
          f" {len(owed)} owe a group")
    if owed:
        print("  FAIL a document cites an executable that nothing runs, and its header")
        print("       neither refuses the gate nor was ever wired in. A debt named in a")
        print("       header is a debt nobody counts — three of these were found by hand")
        print("       on 2026-08-28 and each had been sitting since the day it landed:")
        for rel, where, why in owed:
            print(f"         {rel}  ← {', '.join(where[:3])}")
            print(f"           {why}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
