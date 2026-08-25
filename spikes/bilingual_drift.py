#!/usr/bin/env python3
"""Where one language of a bilingual section was edited and the other was not.

CLAUDE.md has said *"a bilingual fact is one fact in two languages: edit both
or neither"* since D003's approval left English APPROVED and Korean 제안 four
lines apart. The rule did not stop it: swept 2026-08-25, **four more
instances**, every one the same shape — the English half re-measured, the
Korean half left asserting the pre-measurement fact:

    SERVICES-COVERAGE §5  EN "and since re-measured by this same sweep"
                          KO "이 스윕으로 재측정되지 않음"
    SERVICES-COVERAGE §9  EN "Both binaries have `--hold` now"
                          KO "`--hold`가 없다 … 하지 않고 보고한다"
    PLAN-SERVICES    §1   EN "12 of 107 on 2026-08-14 and 0 of 106 on 2026-08-19"
                          KO "현재 선언 107개 중 12개"
    PLAN-SERVICES    §3   EN "closed 2026-08-19 the §5.3 way"
                          KO "`latency_p50`도 없다 … F1의 계약 문제다"

Five instances of one cause is this project's threshold for building something,
and a rule in a document is not a gate.

# What it measures, and the design that failed first

The rule *is* an invariant about commits: **the two halves of one fact should
be touched by the same change.** So `git blame` each section, take the newest
author time on its Korean lines and on its English lines, and report the gap.

The first design measured **date literals** instead — the idea being that a
re-measurement is dated and the un-updated language cannot carry a date it
never received. Its negative control killed it: run against the tree as it
stood before the repairs, it flagged **34 sections of which 4 were real**, and
tightening it to cut that noise (a floor on how much Korean a section carries,
to tell a translation from this project's habitual one-sentence gloss) removed
**exactly the four instances it existed to find** — every one of them sat at a
Korean-to-English ratio of 0.07–0.34, below any threshold that suppressed the
glosses. A check tuned until it is quiet, tested only against a tree with no
defect in it, is the "green while measuring nothing" class with better
manners. *조용해질 때까지 조인 검사는 아무것도 재지 않는 게이트다.*

Commit-time asymmetry has no such problem, because it does not care what a
sentence says. Measured over the 56 bilingual sections at `HEAD` on
2026-08-25, the distribution is **bimodal and empty in the middle**: 45
sections under a day (edited together, the norm), 2 between 1 and 3 days, 1
between 3 and 5, and **8 over five days** — and all four instances above are in
that last group, alongside four more that the same sweep had flagged for other
reasons. Hence `--days 3`: below the empty band, above the noise.

# What it cannot see

A half rewritten *for style* in a later commit resets its clock and hides a
stale fact underneath — the check would call that section clean. A file
rewritten wholesale looks like both halves moved together, which is true and
uninformative. And a fact that is wrong in **both** languages is not this
script's business. It reports where to look; it cannot read Korean.

*두 반쪽이 같은 커밋에서 만져졌는지 본다. 문장의 내용을 보지 않으므로 문체
수정으로 시계가 초기화되면 놓친다 — 어디를 볼지 알려줄 뿐 한국어를 읽지는
못한다.*

Usage:  python3 spikes/bilingual_drift.py [--days N] [--all]
Exit:   0 always — a report, not a gate (see the closing note it prints).
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: Documents maintained bilingually inside one file, Korean beside English.
DOCS = [
    "docs/SERVICES-COVERAGE.md",
    "docs/PLAN-SERVICES.md",
    "docs/PLAN-MOE.md",
    "docs/PLAN-DEFERRED.md",
    "docs/ARCHITECTURE.md",
    "docs/COMPONENTS.md",
]

HANGUL = re.compile(r"[가-힣]")
HEADING = re.compile(r"^#{1,6}\s+(.*)$")
FENCE = re.compile(r"^\s*```")

#: A section needs this many lines in each language before its two halves are
#: comparable at all. One Korean line beside forty English ones is a gloss, and
#: a gloss legitimately outlives the paragraph it summarises.
MIN_LINES = 2


def blamed(rel):
    """[(author_time, text)] for every line of a tracked file, or None."""
    out = subprocess.run(
        ["git", "blame", "--line-porcelain", "HEAD", "--", rel],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        return None
    rows, when = [], 0
    for line in out.stdout.splitlines():
        if line.startswith("author-time "):
            when = int(line.split()[1])
        elif line.startswith("\t"):
            rows.append((when, line[1:]))
    return rows


def drift(rel):
    """[(gap_days, heading, ko_newest, en_newest)] per bilingual section."""
    rows = blamed(rel)
    if rows is None:
        return None
    found, head, buf, fenced = [], "", [], False

    def close(h, lines):
        ko = [t for t, x in lines if x.strip() and HANGUL.search(x)]
        en = [t for t, x in lines if x.strip() and not HANGUL.search(x)]
        if len(ko) >= MIN_LINES and len(en) >= MIN_LINES:
            found.append((abs(max(ko) - max(en)) / 86400.0, h, max(ko), max(en)))

    for when, text in rows:
        if FENCE.match(text):
            fenced = not fenced
            buf.append((when, text))
            continue
        m = None if fenced else HEADING.match(text)
        if m:
            close(head, buf)
            head, buf = m.group(1).strip(), []
        else:
            buf.append((when, text))
    close(head, buf)
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--days", type=float, default=3.0, help="report gaps at or above this")
    ap.add_argument("--all", action="store_true", help="print every section, sorted")
    args = ap.parse_args()

    rows, scanned, missing = [], 0, []
    for rel in DOCS:
        found = drift(rel)
        if found is None:
            missing.append(rel)
            continue
        scanned += len(found)
        rows += [(g, rel, h, k, e) for g, h, k, e in found]
    rows.sort(reverse=True)

    shown = rows if args.all else [r for r in rows if r[0] >= args.days]
    for gap, rel, head, ko, en in shown:
        which = "KO trails EN" if ko < en else "EN trails KO"
        print(f"  {gap:6.1f}d  {which}  {Path(rel).name} § {head[:52]}")

    print()
    print(
        f"  {len(shown)} of {scanned} bilingual section(s) across {len(DOCS) - len(missing)} "
        f"document(s) have halves last edited {args.days:g}+ days apart."
    )
    for rel in missing:
        print(f"  (could not blame {rel} — untracked or absent; **counted as unmeasured**)")
    print(
        "  A report, not a gate. The gap says one half moved without the other, which is\n"
        "  how every measured instance of this drift looked — it does not say the stale\n"
        "  half is wrong, and a half rewritten for style resets its clock and hides one.\n"
        "  Read both halves of what it names; if they disagree on a fact, edit both."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
