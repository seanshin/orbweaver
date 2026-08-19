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


def rows():
    """(crate, gap_text) for every row of the Components table."""
    out = []
    in_table = False
    for line in DOC.read_text().splitlines():
        if line.startswith("| Component |"):
            in_table = True
            continue
        if in_table and not line.startswith("|"):
            break
        if not in_table or line.startswith("|---"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            continue
        m = re.match(r"`([a-z0-9-]+)`", cells[0])
        if not m:
            continue
        out.append((m.group(1), cells[3]))
    return out


def symbols(gap):
    seen = []
    for tok in re.findall(r"`([^`]+)`", gap):
        m = IDENT.match(tok.strip())
        if not m:
            continue
        name = m.group(1)
        leaf = name.split("::")[-1]
        if leaf in SKIP or len(leaf) < 3:
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
    table = rows()
    if not table:
        print("  FAIL no Components table found in %s" % DOC)
        return 1
    exist = named = 0
    for crate, gap in table:
        if only and not any(o in crate for o in only):
            continue
        crate_dir = ROOT / "crates" / crate
        syms = symbols(gap)
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
    print("  %d symbol(s) named by gap columns; %d exist in the tree. This is a report:"
          " a symbol that exists is usually named *because* it exists, and the gap is about"
          " what it cannot yet do — read the row, do not count the line." % (named, exist))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
