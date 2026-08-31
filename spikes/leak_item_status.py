#!/usr/bin/env python3
"""A document that cites D029 §6.1.1's items must agree with what §6.1.1 says.

# What this is for

`records_keep_up.py` checks that `COMPONENTS.md` was *opened* recently, and
CLAUDE.md says plainly why it can do no more: *a script cannot check either for
truth.* That is right in general and wrong for one narrow class, which is the
class that keeps going false.

D029 §6.1.1 is a numbered table of the differences a caller can still tell about
a servant's language. Each item is either open or struck through and marked
closed, and **that table is the home of those facts**. A sentence elsewhere
saying *"item 4 is still open"* is a restatement, and this repository's rule is
that a restatement drifts from its home on the next change, silently, because
nothing compiles a sentence.

Measured 2026-09-01: `COMPONENTS.md` said *"Still open under this row and
unchanged: the inbound half — a reference arriving is a handle the far side
cannot invoke … and has no message in this protocol"* while §6.1.1's item 4 had
been struck through and marked closed the day before, by the batch that added
the message. Two other sentences in the same file were in the same position.
Nothing was red, because nothing was looking.

# What it checks, and what it deliberately does not

It checks **citations**, not prose. A document that writes `§6.1.1 item 4` beside
words like *open* or *closed* is asserting that item's state and is compared
against the table. A document that describes the same fact without citing the
item is **not** checked and cannot be — that is prose, and pretending to check
it would be the tuned-until-quiet defect wearing a gate's coat.

So this gate gets stronger the more the tree cites, which is the direction worth
rewarding: *a claim whose home is elsewhere should name the home.*

*문서가 D029 §6.1.1의 항목을 인용하면 그 표가 하는 말과 일치해야 한다. 산문이
아니라 **인용**을 검사한다 — 항목을 대지 않고 같은 사실을 서술한 문장은 검사하지
않으며, 검사하는 척하는 것이 바로 조용해질 때까지 조이는 결함이다. 그래서 이
게이트는 트리가 더 많이 인용할수록 강해진다.*
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "decisions" / "D029-what-a-complete-orb-would-mean.md"
SECTION = "6.1.1"

#: `§6.1.1 item 4`, `§6.1.1's item 4`, `D029 §6.1.1 item 4`.
CITE = re.compile(r"§\s*6\.1\.1(?:'s)?\s+item\s+(\d+)", re.I)
#: What the citing sentence claims. Read from the sentence the citation sits in.
OPEN_WORDS = re.compile(r"\bstill open\b|\bremains open\b|\bis open\b|\bnot closed\b", re.I)
CLOSED_WORDS = re.compile(r"\bclosed\b|\bis now closed\b", re.I)


def items():
    """§6.1.1's numbered rows, mapped to whether the table calls them closed.

    A row struck through (`~~…~~`) is closed — that is how this table marks one,
    and it marks it in the row rather than in a column, so the strike IS the
    state.
    """
    if not DOC.is_file():
        return None
    text = DOC.read_text(encoding="utf-8")
    m = re.search(r"^#{3,4}\s+%s\b" % re.escape(SECTION), text, re.M)
    if not m:
        return None
    rest = text[m.end():]
    end = re.search(r"^#{1,4}\s", rest, re.M)
    table = rest[: end.start()] if end else rest

    out = {}
    for line in table.splitlines():
        row = re.match(r"^\|\s*(\d+)\s*\|(.*)$", line)
        if not row:
            continue
        out[int(row.group(1))] = "~~" in row.group(2)
    return out or None


def sentence_around(text, at):
    """The sentence a citation sits in, so the claim is read near the citation."""
    start = max(text.rfind(". ", 0, at), text.rfind("\n", 0, at)) + 1
    stop = text.find(". ", at)
    return text[start : stop if stop != -1 else min(len(text), at + 400)]


def scan(state, files):
    bad, seen = [], 0
    for rel in files:
        p = ROOT / rel
        if not p.is_file():
            continue
        text = p.read_text(encoding="utf-8", errors="replace")
        for m in CITE.finditer(text):
            n = int(m.group(1))
            if n not in state:
                bad.append((rel, n, "cites an item §%s does not have" % SECTION))
                continue
            seen += 1
            says = sentence_around(text, m.start())
            closed_here = bool(CLOSED_WORDS.search(says))
            open_here = bool(OPEN_WORDS.search(says))
            if open_here and state[n]:
                bad.append((rel, n, "calls it open; §%s has it struck through as closed" % SECTION))
            elif closed_here and not open_here and not state[n]:
                bad.append((rel, n, "calls it closed; §%s still has it open" % SECTION))
    return bad, seen


#: Synthesised text, checked against the shipped matchers. Each is a way this
#: scan was wrong or could be.
PROBES = [
    ("an open claim about a closed item", "The seam's §6.1.1 item 4 is still open today.", 4, True, True),
    ("a closed claim about a closed item", "§6.1.1 item 4 was closed on 2026-08-31.", 4, True, False),
    ("an open claim about an open item", "§6.1.1 item 1 is still open.", 1, False, False),
    ("a closed claim about an open item", "§6.1.1 item 1 is closed.", 1, False, True),
    ("a citation with no claim either way", "See §6.1.1 item 2 for the shape.", 2, False, False),
]


def probe():
    for what, text, n, closed_in_table, must_flag in PROBES:
        bad, seen = scan({n: closed_in_table}, [])
        # scan() reads files; drive the matchers directly for synthesised text.
        says = sentence_around(text, CITE.search(text).start())
        open_here = bool(OPEN_WORDS.search(says))
        closed_here = bool(CLOSED_WORDS.search(says))
        flagged = (open_here and closed_in_table) or (
            closed_here and not open_here and not closed_in_table
        )
        if flagged != must_flag:
            print("  FAIL the probe %r was %s and must be %s, so this scan cannot"
                  % (what, "flagged" if flagged else "passed",
                     "flagged" if must_flag else "passed"))
            print("       tell a citation that agrees with §%s from one that does not" % SECTION)
            return 2
    state = items()
    if not state:
        print("  FAIL §%s could not be read out of the decision that owns it, so this" % SECTION)
        print("       scan has nothing to compare citations against")
        return 2
    if not any(state.values()) or all(state.values()):
        print("  FAIL §%s reads as all-open or all-closed (%s). This scan tells the two"
              % (SECTION, state))
        print("       apart by a strike-through, and a table showing only one of them")
        print("       has not shown that it can show the other")
        return 2
    return 0


def main(argv):
    if "--probe" in argv:
        return probe()

    state = items()
    if not state:
        print("  FAIL D029 §%s could not be read, so every citation of it is unchecked" % SECTION)
        return 2

    files = [
        "docs/COMPONENTS.md",
        "docs/PLAN-FIRST-COMPLETION.md",
        "docs/ARCHITECTURE.md",
        "docs/PLAN.md",
    ]
    files = [f for f in files if (ROOT / f).is_file()]
    bad, seen = scan(state, files)
    if bad:
        print("  FAIL %d citation(s) of D029 §%s disagree with what §%s says:"
              % (len(bad), SECTION, SECTION))
        for rel, n, why in bad:
            print("         %s: item %d %s" % (rel, n, why))
        print("       §%s is the home of those states; a sentence elsewhere that" % SECTION)
        print("       restates one drifts from it silently. Edit the sentence, or")
        print("       move the fact.")
        return 1

    closed = sum(1 for v in state.values() if v)
    print("  ok   %d citation(s) of D029 §%s agree with it (%d item(s), %d closed)"
          % (seen, SECTION, len(state), closed))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
