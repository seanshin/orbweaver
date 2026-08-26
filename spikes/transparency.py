#!/usr/bin/env python3
"""The five transparencies, read from the one document that owns them.

D029 §6 is priority zero and D029 §6.1 is the table that names the five
transparencies and says where each leaks today. This file does **not** contain
that list. It reads it.

That is the whole design argument. A `TRANSPARENCIES = ["location", ...]`
constant anywhere else — in this file, in a Rust crate, in the harness — would
be a second home for names §6.1 already owns, and CLAUDE.md's *a classifier is
a sentence too* says what happens next: the two drift, silently, when §6.1
changes for a good reason. Asking the owner makes the drift **impossible**
rather than detectable, which is the same reason `orbweaver_cdr` publishes
`IMPLAUSIBLE_LENGTH` instead of letting three callers retype the prefix.

The slug a harness group tags itself with is *derived* from the table's own
first column, never typed here:

    **Location**            -> location
    **Activation / load**   -> activation
    **Lifecycle stability** -> lifecycle

So if §6.1 renames a transparency, every `bears_on` carrying the old slug fails
loudly with "not one of the names in §6.1" rather than being quietly ignored.
That is the `dk_peer` lesson — the expected table checked against the owner's
own enum before any leg ran, so a typo failed as *our* table.

Modes
  --names               one slug per line, in §6.1's order
  --title  <slug>       the display name §6.1 uses for it
  --tell   <slug>       §6.1's "the caller must not be able to tell" cell
  --cite   <slug>       §6.1's "status today" cell, wrapped, for the ledger
  --check  <file>...    every `bears_on <name>` in those files, validated
                        against §6.1 without running the harness

Every mode exits 2 and says why if §6.1 cannot be read or does not hold exactly
five rows. Silence is not one of the outcomes: a reader that cannot find the
table must not hand back an empty list that a caller reads as "no names".
"""

import os
import re
import sys
import textwrap

DOC = os.path.normpath(
    os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..",
        "docs",
        "decisions",
        "D029-what-a-complete-orb-would-mean.md",
    )
)
SECTION = "6.1"
CITE = "D029 §6.1"


def _die(msg):
    sys.stderr.write("transparency.py: %s\n" % msg)
    sys.stderr.write(
        "  the five transparency names live in %s §%s and nowhere else;\n"
        "  this reader has no fall-back list on purpose.\n" % (DOC, SECTION)
    )
    raise SystemExit(2)


def _demarkup(s):
    s = s.replace("**", "")
    return s.strip()


def _slug(display):
    """The tag a group writes, derived from §6.1's own first column.

    First word only, letters only, lower case. "Activation / load" and
    "Lifecycle stability" are two words in the table and one word here, which
    is a choice this function owns and the table does not have to know about.
    """
    word = _demarkup(display).split("/")[0].strip().split()[0]
    return re.sub(r"[^a-z]", "", word.lower())


def rows():
    try:
        with open(DOC, encoding="utf-8") as fh:
            text = fh.read()
    except OSError as exc:
        _die("cannot open the decision that owns the names: %s" % exc)

    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if re.match(r"^#{2,4}\s+%s\b" % re.escape(SECTION), line.strip()):
            start = i + 1
            break
    if start is None:
        _die("no §%s heading in the decision" % SECTION)

    out = []
    for line in lines[start:]:
        # ANY deeper heading ends the table, including a `####` subsection.
        #
        # This stopped at `#{1,3}` until 2026-08-26, on the reasoning that §6.1's
        # own `####` subsections are part of §6.1 — which is true of their prose
        # and false of their tables. §6.1 grew a subsection whose table has one
        # row per transparency, and this reader collected eleven rows and
        # refused, exactly as `_die` promises: *"the table shape changed and this
        # reader must be looked at before any harness tag is trusted"*. Loud, and
        # correct, and the fix is to say what the table IS rather than to make
        # the count elastic — **the first table under the heading**, which is the
        # one §6 defines. A subsection table is somebody citing the rows, not
        # declaring them.
        if re.match(r"^#{1,4}\s", line):
            break
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 3:
            continue
        if set("".join(cells)) <= set("-: "):
            continue
        if cells[0].lower().startswith("transparency"):
            continue
        out.append(
            (_slug(cells[0]), _demarkup(cells[0]), _demarkup(cells[1]), _demarkup(cells[2]))
        )

    if len(out) != 5:
        _die(
            "§%s holds %d transparency row(s), not the five D029 §6 defines — "
            "the table shape changed and this reader must be looked at before "
            "any harness tag is trusted" % (SECTION, len(out))
        )
    slugs = [r[0] for r in out]
    if len(set(slugs)) != 5 or not all(slugs):
        _die("§%s produced non-unique or empty slugs: %s" % (SECTION, slugs))
    return out


def _find(slug):
    for r in rows():
        if r[0] == slug:
            return r
    _die("%r is not one of the names in §%s: %s" % (slug, SECTION, " ".join(r[0] for r in rows())))


def main(argv):
    if len(argv) < 2:
        _die("no mode given (--names, --title, --tell, --cite, --check)")
    mode = argv[1]

    if mode == "--names":
        for r in rows():
            print(r[0])
        return 0

    if mode in ("--title", "--tell", "--cite"):
        if len(argv) < 3:
            _die("%s needs a name" % mode)
        r = _find(argv[2])
        if mode == "--title":
            print(r[1])
        elif mode == "--tell":
            print(r[2])
        else:
            for line in textwrap.wrap(r[3], 62) or ["(§6.1 says nothing here)"]:
                print(line)
        return 0

    if mode == "--check":
        known = [r[0] for r in rows()]
        bad = 0
        seen = 0
        for path in argv[2:]:
            try:
                with open(path, encoding="utf-8") as fh:
                    src = fh.read().splitlines()
            except OSError as exc:
                sys.stderr.write("cannot read %s: %s\n" % (path, exc))
                bad += 1
                continue
            for n, line in enumerate(src, 1):
                m = re.match(r"^\s*bears_on\s+([A-Za-z_][A-Za-z0-9_]*)\s*(#.*)?$", line)
                if not m:
                    continue
                seen += 1
                name = m.group(1)
                if name not in known:
                    print(
                        "%s:%d: bears_on %s is not one of %s's names: %s"
                        % (path, n, name, CITE, " ".join(known))
                    )
                    bad += 1
        if bad:
            print("%d bad tag(s); %d tag(s) read in total" % (bad, seen))
            return 1
        print("%d bears_on tag(s), all named by %s" % (seen, CITE))
        return 0

    _die("unknown mode %r" % mode)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
