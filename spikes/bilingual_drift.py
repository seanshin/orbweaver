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

# The scope, widened 2026-08-26, and what that cost

The list was six documents and did not include `docs/decisions/` — the
directory the rule came from. Widening it to all 33 decisions plus
`CHANGELOG.md` takes the report from 77 comparable sections to **309** and from
11 findings to **14**, at the unchanged `--days 3` (measured 2026-08-26; the
77 is what the six documents grew to from the 56 measured on 2026-08-25).

**Over the 232 sections the widening added, the distribution is bimodal in the
same way the original was, and emptier: 229 under a day, none between 1 and 5,
3 over five days.** The threshold did not have to move. That is the strongest
evidence available that `--days 3` was not fitted to the six files it was first
measured on — it was chosen to sit in an empty band, and the band is still
empty four times further out.

All three are the shape this reports: the English half re-measured, the Korean
half not touched since.

    D010 §8   EN 2026-08-25 (the Landed column, B1–B6 rewritten)
              KO 2026-08-18 — 6.5d
    D006 §"What was verified"  EN 2026-08-19 ("enforced since 526b355")
              KO 2026-08-14 — 5.2d
    D006 §2   EN 2026-08-19 ("Static generated path: the bound is dropped
              *(as of this draft…)*")   KO 2026-08-14 — 5.2d

**False-positive rate over the widening: 0 of 3, against the claim this script
makes** — which is that one half moved without the other, not that the stale
half is wrong. Read for content by hand, all three Korean halves are
*incomplete* rather than false: none of them contradicts its English twin, and
none of them carries the re-measurement its twin gained. That is the honest
verdict and it is why this stays a report.

# What it cannot see

A half rewritten *for style* in a later commit resets its clock and hides a
stale fact underneath — the check would call that section clean. A file
rewritten wholesale looks like both halves moved together, which is true and
uninformative. And a fact that is wrong in **both** languages is not this
script's business. It reports where to look; it cannot read Korean.

**And it cannot see D003.** The instance CLAUDE.md quotes to justify the rule —
D003's approval leaving English APPROVED and Korean 제안 four lines apart — is
in a decision document, is now in scope, and is still invisible: blamed at the
approval commit `dd2da66`, the section reads **0.0d**, because that commit
added a *new* Korean line to the same section while leaving the old one, so the
newest Korean line and the newest English line are the same commit. Section
granularity cannot resolve a drift inside a section. That class is pinned
instead by `decision_status.py`'s `bilingual_halves()`, which compares the
declared *states* rather than the commit times, and it is the reason this
widening is not a duplicate of that gate: one reads what the halves say about a
status, this one reads when they were written, about anything.

*이 스크립트가 만들어진 계기인 D003 자체는 여전히 보이지 않는다 — 한 섹션 안의
어긋남은 섹션 단위로 풀리지 않는다. 그 부류는 `decision_status.py`가 고정한다.*

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
#:
#: The first version of this list was six hand-typed paths and it did not
#: include `docs/decisions/` — **the directory that produced the rule.** D003's
#: approval, the instance CLAUDE.md quotes to justify the rule's existence, is
#: a decision document, and this report could not see it. Found 2026-08-26 when
#: two decision documents landed with bilingual corrections the report never
#: looked at. A list of files is a scope, and a scope narrower than the fact it
#: covers goes green over the drift; the fix is that the decisions are globbed
#: rather than named, so the next one is in scope on the day it is written.
#:
#: `CHANGELOG.md` is here whole, released sections included, which differs from
#: `decision_status.py` cutting it at the first released heading on purpose:
#: that gate asks whether a sentence's *claim* is current, and a released
#: section's claim is dated. This one asks whether a *commit* touched both
#: halves, and a released section edited on one side later is that defect
#: whatever the section says.
#:
#: *목록이 곧 범위다. 규칙을 만들어 낸 디렉터리가 그 목록에 없었다.*
DOCS = sorted(
    str(p.relative_to(ROOT))
    for p in (ROOT / "docs" / "decisions").glob("D0*.md")
) + [
    "docs/SERVICES-COVERAGE.md",
    "docs/PLAN-SERVICES.md",
    "docs/PLAN-MOE.md",
    "docs/PLAN-DEFERRED.md",
    "docs/ARCHITECTURE.md",
    "docs/COMPONENTS.md",
    "CHANGELOG.md",
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
