#!/usr/bin/env python3
"""A decision's status is written in one place and restated in many.

`docs/decisions/D00N-*.md` carries the authoritative status. Every other
document that names a decision tends to say what state it is in, and those
restatements drift: measured 2026-08-18, four of them still said PROPOSED for
decisions the user had already approved, and one of those four sent a planning
pass down the wrong branch.

The rule this enforces is not "never mention a status". It is:

    a passage naming D00N may use status vocabulary freely, as long as the
    decision's CURRENT status is one of the words it uses.

so "proposed 2026-08-13, approved 2026-08-14" passes (it names the current
state as well as the history) and "D003 drafted (PROPOSED)" fails when D003 is
approved (it names only a state that is over).

Text is split into passages at sentence ends, newlines and table-cell pipes,
because a COMPONENTS.md row is one line and a whole-line rule would let any
status word in the row vouch for any other.

**What is deliberately not checked, and why.** A dated record of a moment —
`docs/pipeline-runs/*`, `docs/PHASE*.md`, and every released section of the
CHANGELOG — states what was true then. v0.3.0 shipped with D005 and D006
proposed and its entry says so; editing that to today's status would falsify
the release record rather than repair it. Both classes produced findings on
this gate's first run and both were correct as written. Only living documents
and the CHANGELOG's `## Unreleased` section make a *current* claim, so only
those are scanned.

**A known limit, left unguarded rather than pre-solved.** A status word about
an *option inside* a decision ("D005's option E — rejected") reads to this gate
as a claim about the decision. It fired exactly once, in a pipeline-run record
that is now out of scope for the separate reason above. Guarding it would take
a guess about where an option's name ends, so it waits for a second sighting in
a document this gate actually reads.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DECISIONS = ROOT / "docs" / "decisions"

# Status vocabulary, English and Korean, mapped to the state it asserts.
WORDS = {
    "PROPOSED": "PROPOSED",
    "DRAFTED": "PROPOSED",
    "DRAFT": "PROPOSED",
    "제안": "PROPOSED",
    "초안": "PROPOSED",
    "APPROVED": "APPROVED",
    "ADOPTED": "APPROVED",
    "승인": "APPROVED",
    "채택": "APPROVED",
    "REJECTED": "REJECTED",
    "기각": "REJECTED",
    "SUPERSEDED": "SUPERSEDED",
    "WITHDRAWN": "WITHDRAWN",
}
WORD_RE = re.compile("|".join(sorted(WORDS, key=len, reverse=True)), re.IGNORECASE)
REF_RE = re.compile(r"\bD0\d\d\b")


def bilingual_halves():
    """Every status marker inside a decision must name the same state.

    A status is prose spanning several lines in two languages, so an edit that
    approves a decision updates the block the editor is reading and leaves the
    other one behind. Measured 2026-08-18: D003's approval overwrote the head
    of its PROPOSED block and left the tail, so the file said APPROVED in
    English and \uc81c\uc548 in Korean, four lines apart. The English half is what
    every other document had been copying.
    """
    for path in sorted(DECISIONS.glob("D0*.md")):
        text = path.read_text(encoding="utf-8")
        marks = re.findall(r"\*{0,2}(?:STATUS|Status|\uc0c1\ud0dc)[:\*]{1,3}\s*\**([^\s*]+)", text)
        states = {WORDS.get(m.upper()) for m in marks}
        states.discard(None)
        if len(states) > 1:
            yield path, sorted(states)


def authoritative():
    """The status each decision file declares, from its own STATUS line."""
    out = {}
    for path in sorted(DECISIONS.glob("D0*.md")):
        name = path.name.split("-")[0]
        text = path.read_text(encoding="utf-8")
        m = re.search(r"\*{0,2}(?:STATUS|Status)[:\*]{1,3}\s*\**([A-Za-z]+)", text)
        if not m:
            out[name] = None
            continue
        out[name] = WORDS.get(m.group(1).upper())
    return out


def passages(text):
    """(line number, passage) pairs at paragraph-then-sentence granularity.

    Paragraphs are joined before splitting, because markdown wraps prose at
    about 78 columns and a per-line rule loses every claim whose reference and
    whose status word fall either side of a wrap. That was not hypothetical:
    the first version of this gate read `PLAN-MOE.md` line by line, missed
    `D006-plane-rule-tensor.md` on one line and `(**PROPOSED**)` on the next,
    and passed a document that was wrong — while catching the Korean twin four
    lines below only because that one happened not to wrap. A gate with a blind
    spot shaped like line wrapping would go on passing the same class forever.

    Table rows stay one passage per row and split further at the cell pipes: a
    `COMPONENTS.md` row is a single very long line, and letting a status word
    in one cell vouch for a reference in another is the same blindness in the
    other direction.
    """
    block, start = [], 1
    for lineno, line in enumerate(text.splitlines() + [""], 1):
        is_table = line.lstrip().startswith("|")
        if not line.strip() or is_table:
            if block:
                yield from sentences(start, " ".join(block))
                block = []
            if is_table:
                for cell in line.split("|"):
                    yield from sentences(lineno, cell)
            continue
        if not block:
            start = lineno
        # Blockquote and list markers are prose, not structure, once joined.
        block.append(re.sub(r"^\s*(?:>|[-*+]|\d+\.)\s*", "", line))


def sentences(lineno, chunk):
    for piece in re.split(r"(?<=[.:;?!])\s+", chunk):
        if piece.strip():
            yield lineno, piece


def living(path):
    """Whether this file makes a claim about *now* rather than about a date."""
    if DECISIONS in path.parents:
        return False  # the source of truth does not restate itself
    if path.parent.name == "pipeline-runs":
        return False  # a run record is dated; today cannot edit it
    if path.name.startswith("PHASE"):
        return False  # likewise, a phase is a record of one
    return True


def bound(piece):
    """Which decision each status word in this passage is talking about.

    A word describes the decision most recently named before it, falling back
    to the first named after it for the "PROPOSED: D00N ..." shape. Attaching
    every word to every reference instead reads "D003-A's vector union ...,
    D004 drafted PROPOSED" as a claim about D003 as well, which it is not —
    that shape put two false findings in this gate's second run, and a gate
    that cries about correct sentences is one people learn to skip.
    """
    refs = [(m.start(), m.group()) for m in REF_RE.finditer(piece)]
    out = {}
    if not refs:
        return out
    for m in WORD_RE.finditer(piece):
        before = [r for r in refs if r[0] < m.start()]
        owner = before[-1][1] if before else refs[0][1]
        out.setdefault(owner, set()).add(WORDS[m.group().upper()])
    return out


def scanned():
    """(path, text) for every living document, CHANGELOG cut to Unreleased."""
    for path in sorted(ROOT.glob("*.md")) + sorted(ROOT.glob("docs/**/*.md")):
        if not living(path):
            continue
        text = path.read_text(encoding="utf-8")
        if path.name == "CHANGELOG.md":
            # Everything from the first released heading down is history.
            m = re.search(r"^## v", text, re.MULTILINE)
            if m:
                text = text[: m.start()]
        yield path, text


def main():
    truth = authoritative()
    unknown = [d for d, s in truth.items() if s is None]
    findings = []
    missing = set()
    for path, text in scanned():
        for lineno, piece in passages(text):
            for ref, claimed in bound(piece).items():
                if ref not in truth:
                    # A citation to a decision that does not exist reads exactly
                    # like one that does, and is the cheapest of all these
                    # mistakes to make in a hurry.
                    missing.add((path.relative_to(ROOT), lineno, ref))
                    continue
                current = truth[ref]
                if current is None:
                    continue  # decision file exists but declares no status; reported below
                if current not in claimed:
                    findings.append(
                        (path.relative_to(ROOT), lineno, ref, current,
                         sorted(claimed), piece.strip()[:120])
                    )
    for path, lineno, ref, current, claimed, quote in findings:
        print(f"  DRIFT {path}:{lineno} {ref} is {current}, "
              f"passage says {'/'.join(claimed)}")
        print(f"        {quote}")
    for d in unknown:
        print(f"  DRIFT docs/decisions/{d}-*.md has no parsable STATUS line")
    for path, lineno, ref in sorted(missing):
        print(f"  DRIFT {path}:{lineno} cites {ref}, which has no decision file")
    split = list(bilingual_halves())
    for path, states in split:
        print(f"  DRIFT {path.relative_to(ROOT)} states {'/'.join(states)} "
              f"in different halves of one file")
    total = len(findings) + len(unknown) + len(split) + len(missing)
    print(f"  {len(truth)} decisions, {total} drifted status claim(s)")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
