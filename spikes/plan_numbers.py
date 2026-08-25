#!/usr/bin/env python3
"""A report, not a gate: every hand-typed count in the plan documents, beside
the number a script can compute for it today.

    python3 spikes/plan_numbers.py            # every plan document
    python3 spikes/plan_numbers.py --sweep sweep.tsv   # also compare "N of M declared"

Why a report. The plan review of 2026-08-19 found six hand-typed numbers stale
within five days of being written — "37 tests" (40), "20 tests (34 in the
crate)" (21 / 84), "17 tests" (30), "12 of 107 declared" (0 absent of 106),
"fifty served" (63) — and found that every number a script writes
(`spikes/coverage_tables.py`) had stayed current. This prints the pairs so the
person re-reading a plan row has the current figure in front of them. It is
not a gate: a count in prose is usually a dated statement ("N tests on
2026-08-14"), and a gate would either fail every dated record or teach people
to delete the dates. Same reasoning as `spikes/gap_symbols.py`.

What it reads: `docs/PLAN*.md`, `docs/PLAN-*.md`, `docs/decisions/*.md`.
What it recognises:
  - "`orbweaver-<crate>` ... N tests"  → `cargo test -p <crate> --lib -- --list`
  - "N tests (M in the crate)"         → the module count is not derivable
                                          without a module name; the crate
                                          count is
  - "N of M declared operations"       → the sweep totals, when --sweep is given
  - "N golden files" / "N corpus files"→ `ls corpus/golden/*.idl | wc -l`

What it recognises is not what it reads, and the difference used to be
invisible. A report headed "every hand-typed count" that lists four shapes and
says nothing about the fifth reads as coverage — the same way `service_sweep`'s
silence about unprobed interfaces read as coverage until 2026-08-19. So every
`<number> <countable noun>` phrase in these documents is now counted, the
recognised ones are subtracted, and the remainder is printed as UNCLASSIFIED
with the line it sits on. Those are not errors; they are the part of the claim
this tool does not keep. A crate whose count `cargo` could not produce is
listed separately as unmeasured rather than printed as `today None`.

This stays a report: it exits 0 whatever it finds, for the reason above.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = sorted(ROOT.glob("docs/PLAN*.md")) + sorted(ROOT.glob("docs/decisions/*.md"))

# The nouns a hand-typed count in these documents attaches to, and which a
# script could in principle go and count. Deliberately a closed list, and
# deliberately not `row`: `§10 row` is a cross-reference, not a quantity, and
# it alone produced eight of the first run's findings. The Korean counters
# 개/건 are out for the same reason in the other direction — `2026-08-12 개정`
# reads as "12 개". A report nobody reads measures as little as a gate nobody
# trusts, so the list stays at the shapes whose findings are worth a look.
COUNTABLE_RE = re.compile(
    r"(?<![.\-§0-9])\b\d+\s+\**(?:tests?|files?|cases?|operations?|interfaces?|crates?|"
    r"servants?|divergences?|symbols?|decisions?|gates?|groups?|contracts?|literals?|"
    r"kinds?|targets?|golden)\b", re.IGNORECASE)


def crate_test_count(crate):
    r = subprocess.run(
        ["cargo", "test", "-q", "-p", crate, "--lib", "--", "--list"],
        cwd=ROOT, capture_output=True, text=True,
    )
    out = r.stdout + r.stderr
    m = re.search(r"(\d+) tests?, \d+ benchmarks", out)
    if m:
        return int(m.group(1))
    # `-q --list` prints one "name: test" line per test and no summary.
    n = len(re.findall(r"^\S.*: test$", out, re.M))
    return n or None


def golden_count():
    return len(list((ROOT / "corpus/golden").glob("*.idl")))


def sweep_totals(path):
    tot = {}
    for line in Path(path).read_text().splitlines():
        if line.startswith("TOTAL\t"):
            parts = line.split("\t")
            tot[parts[1]] = parts[2:]
    return tot


def main(argv):
    sweep = None
    if "--sweep" in argv:
        sweep = sweep_totals(argv[argv.index("--sweep") + 1])
    crate_re = re.compile(r"`(orbweaver-[a-z]+)(?:::[a-z_]+)?`[^.\n|]{0,120}?\**(\d+)\**\s+tests?")
    tests_re = re.compile(r"\**(\d+)\**\s+tests?\**\s*\(\**(\d+)\** in the crate\)")
    declared_re = re.compile(r"\**(\d+)\s+of\s+(\d+)\**\s+declared")
    golden_re = re.compile(r"\b(\d+)\s+(?:golden|corpus)\s+files?")
    cache = {}
    rows = 0
    unmeasured = []
    unclassified = []
    for doc in DOCS:
        rel = doc.relative_to(ROOT)
        for n, line in enumerate(doc.read_text().splitlines(), 1):
            covered = []          # spans this pass reported on, for the sweep below
            for m in crate_re.finditer(line):
                crate, said = m.group(1), int(m.group(2))
                if crate not in cache:
                    cache[crate] = crate_test_count(crate)
                now = cache[crate]
                if now is None:
                    print(f"{rel}:{n}: `{crate}` {said} tests  —  UNMEASURED: `cargo test -p {crate} --lib -- --list` produced no count")
                    unmeasured.append((rel, n, crate))
                else:
                    mark = "=" if now == said else "≠"
                    print(f"{rel}:{n}: `{crate}` {said} tests  {mark}  today {now}")
                covered.append(m.span())
                rows += 1
            for m in tests_re.finditer(line):
                print(f"{rel}:{n}: \"{m.group(1)} tests ({m.group(2)} in the crate)\" — module count needs a module name; the crate is above if named")
                covered.append(m.span())
                rows += 1
            for m in declared_re.finditer(line):
                said = f"{m.group(1)} of {m.group(2)}"
                if sweep:
                    print(f"{rel}:{n}: \"{said} declared\"  — sweep TOTAL rows: " + "; ".join(f"{k}: {' '.join(v[:2])}" for k, v in sweep.items()))
                else:
                    print(f"{rel}:{n}: \"{said} declared\"  — pass --sweep <service_sweep --raw output> to compare")
                covered.append(m.span())
                rows += 1
            for m in golden_re.finditer(line):
                said = int(m.group(1)); now = golden_count()
                print(f"{rel}:{n}: \"{said} golden/corpus files\"  {'=' if said == now else '≠'}  today {now}")
                covered.append(m.span())
                rows += 1
            for m in COUNTABLE_RE.finditer(line):
                if any(a <= m.start() and m.end() <= b for a, b in covered):
                    continue
                unclassified.append((rel, n, m.group(0).strip(), line.strip()))
    for rel, n, phrase, line in unclassified:
        print(f"{rel}:{n}: UNCLASSIFIED \"{phrase}\" — no rule computes this one")
        print(f"        {line[:120]}")
    print()
    print(f"  {rows} hand-typed count(s) checked against a computed figure;"
          f" {len(unclassified)} count-like phrase(s) this tool has no rule for;"
          f" {len(unmeasured)} count(s) it could not measure —"
          f" in {len(DOCS)} plan/decision document(s). A report:"
          " a count in prose is usually dated; read the sentence before changing the number,"
          " and prefer a script that writes it (spikes/coverage_tables.py) over a corrected literal."
          " An UNCLASSIFIED line is not a defect; it is this tool saying what it did not read.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
