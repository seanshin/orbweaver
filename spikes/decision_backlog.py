#!/usr/bin/env python3
"""How many decisions say PROPOSED, and which of them the tree has acted on.

# What this is for, and what it deliberately cannot do

`spikes/decision_status.py` checks that every **restatement** of a status matches
its decision. It cannot ask the other question — *does a document saying
`PROPOSED` describe something that has in fact been decided?* — because that is
judgement about content, not agreement between two strings.

This does not answer that question either. It reports the **computable signals**
a person needs to answer it, and says so: the count, how widely each is cited,
and whether the document itself records having been acted on. The judgement stays
in `docs/PLAN-FIRST-COMPLETION.md` §1.9, where a person wrote it.

**A report, not a gate.** There is no defensible number for "too many open
decisions" — a project that decided nothing and a project that decided everything
would both be suspicious, and neither is a threshold. The same reason
`entry_cost.py` and `plan_numbers.py` report and do not gate.

# The instance that raised it

Measured 2026-09-01: **24 of 39 decisions say `PROPOSED`**, and essentially every
one has its recommendation standing in the tree — D015 recommended cutting a
release and seven have been cut; D011's fan-out ships; D012 and D013 recommended
building nothing and are cited as limits.

The sharpest is **D029**. `CLAUDE.md` opens by saying priority zero was *"set by
the project owner 2026-08-26"* and names D029 §6 as its home; D029 says
`STATUS: PROPOSED`. Five row standings, a plan and the harness ledger all rest on
it. `decision_status.py` cannot see that, and correctly: `CLAUDE.md` does not
restate a status marker, it says the owner set the direction, which is a
different sentence about a different thing.

*`decision_status.py`는 **다시 적힌** 상태가 결정과 맞는지 검사하지, `PROPOSED`라고
적힌 문서가 사실은 결정되었는지는 묻지 못한다 — 그건 문자열 일치가 아니라 내용에
대한 판단이다. 이 스크립트도 그 질문에 답하지 않는다: 사람이 답하는 데 필요한
**계산 가능한 신호**를 보고할 뿐이고, 판단은 계획서 §1.9에 사람이 적는다. 게이트가
아니라 보고다 — "열린 결정이 너무 많다"에 방어 가능한 수는 없다.*
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DECISIONS = ROOT / "docs" / "decisions"

#: Words a decision uses about itself when it has been acted on. Read from the
#: document, never inferred from the tree: a recommendation that happens to
#: match something in `crates/` is a coincidence this scan must not promote.
ACTED = re.compile(
    r"\bResult, appended\b|\bDone\b|\blanded\b|\bCorrected \d{4}-|\bamended\b", re.I
)


def decisions():
    out = []
    for p in sorted(DECISIONS.glob("D0*.md")):
        text = p.read_text(encoding="utf-8", errors="replace")
        head = "\n".join(text.splitlines()[:40])
        # `**STATUS: APPROVED**` is the common form and `**Status:** APPROVED`
        # is D001's — the oldest decision, written before the convention
        # settled. Matching only the shouted spelling made this scan unable to
        # read it, which its own probe refused before the report was believed.
        m = re.search(r"\*\*Status:?\*?\*?:?\s*\**\s*([A-Za-z]+)", head, re.I)
        out.append((p.name.split("-")[0], p, m.group(1) if m else None, text))
    return out


def cited_by(num):
    """Living documents that mention it, excluding the decision itself."""
    r = subprocess.run(
        ["git", "grep", "-l", num, "--", "docs/"], cwd=ROOT, capture_output=True, text=True
    )
    return [f for f in r.stdout.split() if "/decisions/%s-" % num not in f]


def main(argv):
    if "--probe" in argv:
        # The scan must find decisions, read statuses, and see both values. A
        # scan that read one status for everything has shown nothing.
        ds = decisions()
        if len(ds) < 10:
            print("  FAIL only %d decision(s) were read; this scan is not looking at the"
                  % len(ds))
            print("       directory that holds them")
            return 2
        states = {s for _, _, s, _ in ds}
        if None in states:
            missing = [n for n, _, s, _ in ds if s is None]
            print("  FAIL %d decision(s) carry no STATUS marker this scan can read: %s"
                  % (len(missing), ", ".join(missing)))
            return 2
        if len(states) < 2:
            print("  FAIL every decision reads as %s. A scan that only ever gives one"
                  % states.pop())
            print("       answer is not evidence for that answer")
            return 2
        return 0

    ds = decisions()
    proposed = [(n, p, t) for n, p, s, t in ds if s == "PROPOSED"]
    approved = [n for n, _, s, _ in ds if s == "APPROVED"]

    print("  %d decision(s): %d APPROVED, %d PROPOSED"
          % (len(ds), len(approved), len(proposed)))
    print("  PROPOSED, with the signals a person needs to classify each —")
    print("  `acted` means the DOCUMENT records having been acted on, never that")
    print("  something in the tree resembles its recommendation:")
    for num, path, text in proposed:
        title = text.splitlines()[0].lstrip("# ").strip()
        n_cites = len(cited_by(num))
        acted = "acted" if ACTED.search(text) else "  -  "
        print("    %-5s %-5s cited-by=%-2d  %s" % (num, acted, n_cites, title[:60]))
    print("  A report, not a gate: there is no defensible number for how many")
    print("  decisions should be open, which is why none is asserted here.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
