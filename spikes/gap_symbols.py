#!/usr/bin/env python3
"""A report, not a gate: what each `docs/COMPONENTS.md` gap row names, and
whether the thing it names exists in its crate today.

    python3 spikes/gap_symbols.py            # every component row
    python3 spikes/gap_symbols.py giop mcp   # only rows whose crate matches

Why a report. D010 §7.1 measured this before proposing it as a gate: 11 of the
17 symbols the gap columns named on 2026-08-19 *exist* in their crate, nearly
all legitimately — `Dispatch::forward` is named because it exists and the gap
is about what it cannot say. A 65 % false-positive rate is not a gate; it is
the fourth check that week that would be red-or-green while measuring nothing,
and people learn to skip those. So this prints the facts and decides nothing:
the person re-reading a gap row before planning against it has, in front of
them, whether the symbol the row names is in the tree — which is the check
that would have caught the five already-closed gaps of 2026-08-18 (`knows()`,
`LOCATION_FORWARD`, `SEAT_SAFETY_CONTENT`, F5, in-module `#include`), every
one of which cost either a planning pass or a batch that found the work done.

What it reads: the "What is missing" column of the Components table, per row.
What it looks for: each backticked token that looks like an identifier
(`Foo`, `foo_bar`, `Foo::bar`, `foo()`, `Foo<T>` → `Foo`), grepped as a word
in `crates/<crate>/` (the row's crate; `(poa)` and similar suffixes dropped),
falling back to the whole `crates/` tree and saying which. Strikethrough
(`~~…~~`) rows are already marked closed and are reported as such.

Exit code is 0 whenever the document parsed. Not a gate. Do not wire it into
`run_checks.sh` as one without re-measuring the false-positive rate first.

**It now says what it did not read.** `rows()` drops any table line whose first
cell is not a backticked lowercase crate name, and `symbols()` drops any
backticked token that is not identifier-shaped, is in `SKIP`, or has a leaf
under three characters — all of it silently, so a row renamed out of the
pattern and a row with an empty gap column printed identically: nothing. A
report that cannot say how much of the document it read is a report that reads
as coverage, which is the failure this file's own preamble is about one level
up. The last line now carries rows read, rows skipped, and tokens passed over,
and each skipped row is named. `grep` also names the file types it searched:
a symbol that lives only in a `.sh` or a `.md` reports as "not found under
crates/", which is true of the search and not of the tree.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs/COMPONENTS.md"
IDENT = re.compile(r"^~?~?([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)(?:<[^`]*>)?(?:\(\))?~?~?$")
SKIP = {"ok", "true", "false", "None", "Some", "self", "crates", "docs", "spikes", "corpus",
        "Option", "Result", "Vec", "any", "long", "string"}   # prelude and IDL keywords, not gap symbols


def rows(skipped):
    """(crate, gap_text) for every row of the Components table.

    `skipped` collects every table line this pattern could not turn into a
    row, with the reason — a row is either read or named, never dropped.
    """
    out = []
    in_table = False
    for lineno, line in enumerate(DOC.read_text().splitlines(), 1):
        if line.startswith("| Component |"):
            in_table = True
            continue
        if in_table and not line.startswith("|"):
            break
        if not in_table or line.startswith("|---"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            skipped.append((lineno, "%d cell(s), a row needs 4" % len(cells), line.strip()))
            continue
        m = re.match(r"`([a-z0-9-]+)`", cells[0])
        if not m:
            skipped.append((lineno, "first cell is not a backticked crate name", line.strip()))
            continue
        out.append((m.group(1), cells[3]))
    return out


def symbols(gap, passed_over):
    """Identifier-shaped backticked tokens in a gap column.

    Everything backticked that is not one is appended to `passed_over` with
    its reason, so the count of what this report declined to look up is
    printed beside the count of what it looked up.
    """
    seen = []
    for tok in re.findall(r"`([^`]+)`", gap):
        m = IDENT.match(tok.strip())
        if not m:
            passed_over.append((tok, "not identifier-shaped"))
            continue
        name = m.group(1)
        leaf = name.split("::")[-1]
        if leaf in SKIP:
            passed_over.append((tok, "in SKIP — prelude or IDL keyword"))
            continue
        if len(leaf) < 3:
            passed_over.append((tok, "leaf under three characters"))
            continue
        struck = tok.startswith("~~")
        if (name, struck) not in seen:
            seen.append((name, struck))
    return seen


def grep(word, path):
    r = subprocess.run(
        ["grep", "-rlw", "--include=*.rs", "--include=*.py", "--include=*.idl", word, str(path)],
        capture_output=True, text=True,
    )
    return [str(Path(f).relative_to(ROOT)) for f in r.stdout.split() if f]


def main(argv):
    only = [a for a in argv[1:] if not a.startswith("-")]
    skipped_rows = []
    table = rows(skipped_rows)
    if not table:
        print("  FAIL no Components table found in %s" % DOC)
        for lineno, why, line in skipped_rows:
            print("       line %d skipped: %s" % (lineno, why))
        return 1
    exist = named = 0
    passed_over = []
    no_crate_dir = []
    for crate, gap in table:
        if only and not any(o in crate for o in only):
            continue
        crate_dir = ROOT / "crates" / crate
        if not crate_dir.is_dir():
            no_crate_dir.append(crate)
        syms = symbols(gap, passed_over)
        print("%s — %d symbol(s) in its gap column" % (crate, len(syms)))
        for name, struck in syms:
            named += 1
            leaf = name.split("::")[-1]
            where = "crates/%s" % crate
            hits = grep(leaf, crate_dir) if crate_dir.is_dir() else []
            if not hits and (ROOT / "crates").is_dir():
                hits = grep(leaf, ROOT / "crates")
                where = "crates/"
            if struck:
                verdict = "marked closed (~~)"
            elif hits:
                verdict = "exists in %s (%d file(s), e.g. %s)" % (where, len(hits), hits[0])
                exist += 1
            else:
                verdict = "not found under crates/"
            print("    %-40s %s" % (name, verdict))
    print()
    for lineno, why, line in skipped_rows:
        print("  SKIPPED row at %s:%d — %s" % (DOC.relative_to(ROOT), lineno, why))
        print("          %s" % line[:120])
    for crate in no_crate_dir:
        print("  SKIPPED crates/%s does not exist; its symbols were searched across "
              "the whole crates/ tree" % crate)
    print("  %d Components row(s) read, %d skipped; %d symbol(s) named by gap columns,"
          " %d exist in the tree, %d backticked token(s) passed over"
          % (len(table), len(skipped_rows), named, exist, len(passed_over)))
    if passed_over:
        shown = ", ".join("`%s` (%s)" % (t, why) for t, why in passed_over[:6])
        print("          passed over: %s%s" % (shown, " …" if len(passed_over) > 6 else ""))
    print("  Searched *.rs, *.py and *.idl only — a symbol living in a shell script or a"
          " document reports as not found, which is true of the search and not of the tree.")
    print("  This is a report: a symbol that exists is usually named *because* it exists,"
          " and the gap is about what it cannot yet do — read the row, do not count the line.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
