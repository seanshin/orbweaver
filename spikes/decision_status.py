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

**What it could not read is printed, and counts against it.** Measured
2026-08-25: `bilingual_halves()` captured the first non-space token after each
marker and dropped every token that mapped to no state — so `**상태: 승인됨**`
(the suffixed form ten decisions use) and `**상태:** 2026-08-12 승인·구현 완료`
(the date-first form D001 uses) were dropped, leaving one state in the set and
nothing to compare. **Eleven of thirteen decisions had never had their Korean
half checked**, D003 among them — the file whose split halves are why this
check exists. Nothing was red; the gate printed `0 drifted status claim(s)`
over eleven halves it had not read. A marker now takes the first status *word*
on its own line, and a marker carrying none is an `UNREAD` finding rather than
a silent drop. The verdict line states markers read, markers unread, documents
scanned and documents out of scope, because a gate that cannot say how much it
read cannot be told from one that read nothing.
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


MARKER_RE = re.compile(r"\*{0,2}(STATUS|Status|\uc0c1\ud0dc)[:\*]{1,3}")


def markers(text):
    """(lineno, language, state, line) for every status marker in a decision.

    `state` is the **first status word on the marker's own line**, not the
    first whitespace-delimited token after the marker. The token rule is what
    made this gate blind: `\uc2b9\uc778\ub428` is not a key of WORDS (`\uc2b9\uc778` is), and D001's
    Korean marker puts the date first, so eleven of thirteen Korean halves
    mapped to None and were discarded before the comparison. Reading the line
    for a word also keeps the declared state ahead of the history that follows
    it on the same line \u2014 `**STATUS: APPROVED** \u2014 drafted 2026-08-14` is
    APPROVED, which is the same rule `authoritative()` has always applied.

    `state` is None when the line carries no status word at all. That is not a
    skip: `main()` counts it, prints it, and fails on it.
    """
    for lineno, line in enumerate(text.splitlines(), 1):
        m = MARKER_RE.search(line)
        if not m:
            continue
        lang = "KO" if m.group(1) == "\uc0c1\ud0dc" else "EN"
        w = WORD_RE.search(line, m.end())
        state = WORDS[w.group().upper()] if w else None
        yield lineno, lang, state, line.strip()


def decisions():
    """(path, [marker, ...]) for every decision file, markers included."""
    for path in sorted(DECISIONS.glob("D0*.md")):
        yield path, list(markers(path.read_text(encoding="utf-8")))


def bilingual_halves(marked):
    """Every status marker inside a decision must name the same state.

    A status is prose spanning several lines in two languages, so an edit that
    approves a decision updates the block the editor is reading and leaves the
    other one behind. Measured 2026-08-18: D003's approval overwrote the head
    of its PROPOSED block and left the tail, so the file said APPROVED in
    English and \uc81c\uc548 in Korean, four lines apart. The English half is what
    every other document had been copying.
    """
    for path, marks in marked:
        states = sorted({s for _, _, s, _ in marks if s is not None})
        if len(states) > 1:
            yield path, states


def one_language_only(marked):
    """A decision carrying a status in one language and not the other.

    The bilingual comparison has nothing to compare when a half is absent, so
    an absent half reads exactly like an agreeing one. All thirteen files carry
    both today; the check exists so that stays a measured fact.
    """
    for path, marks in marked:
        langs = {lang for _, lang, _, _ in marks}
        for missing in sorted({"EN", "KO"} - langs):
            yield path, missing


def authoritative(marked):
    """The status each decision file declares, from its first English marker."""
    out = {}
    for path, marks in marked:
        name = path.name.split("-")[0]
        english = [s for _, lang, s, _ in marks if lang == "EN"]
        out[name] = english[0] if english else None
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


def in_glob():
    """Every markdown file this gate's globs can reach, living or dated."""
    return sorted(set(ROOT.glob("*.md")) | set(ROOT.glob("docs/**/*.md")))


def scanned(skipped):
    """(path, text) for every living document, CHANGELOG cut to Unreleased.

    `skipped` collects the dated records passed over, so the verdict line can
    say how many documents were deliberately not read. A deliberate skip that
    is never counted is indistinguishable in the output from a glob that
    matched nothing.
    """
    for path in in_glob():
        if not living(path):
            skipped.append(path)
            continue
        text = path.read_text(encoding="utf-8")
        if path.name == "CHANGELOG.md":
            # Everything from the first released heading down is history.
            m = re.search(r"^## v", text, re.MULTILINE)
            if m:
                text = text[: m.start()]
        yield path, text


PRUNE = {".git", "target", "node_modules", ".claude", "venv", ".venv"}


def cited_out_of_scope():
    """Markdown that names a decision and lives outside this gate's globs.

    The globs read the repository root and `docs/`. `corpus/requirements/`,
    `corpus/include/` and `spikes/tls/` also carry prose, and a status claim
    written there would never be compared with anything. Reported rather than
    scanned: widening the globs changes what the gate covers and would want its
    own false-positive measurement first, exactly as `gap_symbols.py` did
    before proposing itself. What is not reported at all cannot be decided
    about, which is why the count is printed either way.
    """
    reachable = set(in_glob())
    out, stack = [], [ROOT]
    while stack:
        for p in sorted(stack.pop().iterdir()):
            if p.is_dir():
                if p.name not in PRUNE:
                    stack.append(p)
            elif p.suffix == ".md" and p not in reachable:
                if REF_RE.search(p.read_text(encoding="utf-8", errors="replace")):
                    out.append(p)
    return sorted(out)


def main():
    marked = list(decisions())
    truth = authoritative(marked)
    unknown = [d for d, s in truth.items() if s is None]
    findings = []
    missing = set()
    read = 0
    skipped_docs = []
    for path, text in scanned(skipped_docs):
        read += 1
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
    split = list(bilingual_halves(marked))
    for path, states in split:
        print(f"  DRIFT {path.relative_to(ROOT)} states {'/'.join(states)} "
              f"in different halves of one file")
    # A marker the parser could not classify is the failure this gate is for:
    # it is the shape in which the gate went green over eleven Korean halves.
    unread = [(path, lineno, line)
              for path, marks in marked
              for lineno, _, state, line in marks if state is None]
    for path, lineno, line in unread:
        print(f"  UNREAD {path.relative_to(ROOT)}:{lineno} status marker names no "
              f"state this gate knows — it was not compared")
        print(f"         {line[:120]}")
    half = list(one_language_only(marked))
    for path, missing_lang in half:
        print(f"  UNREAD {path.relative_to(ROOT)} carries no {missing_lang} status "
              f"marker, so its bilingual halves cannot be compared")
    total = len(findings) + len(unknown) + len(split) + len(missing) + len(unread) + len(half)
    n_marks = sum(len(m) for _, m in marked)
    print(f"  {len(truth)} decisions, {n_marks} status marker(s) read, "
          f"{len(unread)} unread; {read} living document(s) scanned, "
          f"{len(skipped_docs)} dated record(s) out of scope")
    outside = cited_out_of_scope()
    print(f"  {len(outside)} document(s) cite a decision from outside this gate's "
          f"globs and are not scanned"
          + (": " + ", ".join(str(p.relative_to(ROOT)) for p in outside) if outside else ""))
    print(f"  {total} drifted status claim(s)")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
