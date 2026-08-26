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

**Source files are scanned too, under a stricter vocabulary.** Measured
2026-08-26: **D007 had been APPROVED for eight days while four files still
called it PROPOSED** — `crates/orbweaver-gen/Cargo.toml`,
`crates/orbweaver-gen/src/python.rs`, `crates/orbweaver-gen/src/bin/py_bridge.rs`
(all three repaired in b7836b5) and `spikes/run_checks.sh`. None went red,
because this gate globbed `*.md` and `docs/**/*.md` while the fact it pins is
workspace-scoped: *a pin whose scope is narrower than its fact's is a pin that
will go green over the drift* (CLAUDE.md, "Where a fact lives"). 150
non-markdown files cite a decision; a status restated in any of them is now
compared with the decision, exactly as a markdown restatement is.

But **source prose is not documentation prose**, and reading it with the
markdown vocabulary is unusable: measured over those same 150 files, the loose
rule produced **13 findings of which 1 was real**. Twelve were the same two
shapes, and neither is a claim about a state:

  * the vocabulary used as a **verb about the decision's content** — "the exact
    defect D008 was drafted from", "the one place D006 proposed putting a
    plane-rule marker", "D005's option E — rejected"; and
  * a status word about **something else entirely** in a passage that names a
    decision incidentally — a skeleton that "accepted what the dynamic one
    rejected", beside a citation of D006.

So a source file is read under two extra rules, and both are measured below:

  1. **English counts only when it is SHOUTED.** `PROPOSED` asserts a state;
     `proposed` is this project's ordinary past tense. That is the convention
     the decision files themselves keep (`**STATUS: APPROVED**`), all four
     drifted restatements used it, and it removes 10 of the 12. Korean has no
     case, so Korean status words count either way — which means the Korean
     verb shape ("D006이 제안한") is a hole this rule does not close, and the
     honest measurement of the exposure is: **77 of the scanned files carry
     Hangul, and 4 passages put a Korean status word beside a decision
     reference.** None of the four fires, and three of them do not fire for
     reasons that are not this rule — the passage also names the current state,
     or the word is quoted. It is written down rather than guarded because
     guarding it would need a guess about Korean verb endings and there is no
     instance to tune against; the point is that the second sighting is
     recognised instead of rediscovered.
  2. **A quoted status word is a quotation, not an assertion.** A word inside
     backticks or double quotes is being shown, not claimed. This removes the
     last 3 — all of them in *this file's own docstring*, which necessarily
     quotes failing passages. Note that it is not a self-exemption: an unquoted
     stale claim written here still fails. The **reference** is not masked, only
     the status word, because a real restatement habitually puts the file name
     in backticks and the status outside them — `` `D007-...md` states the
     options and is PROPOSED `` is precisely the shape that drifted.

**Measured false-positive rate of the widening: 0 of 1 finding over 150
non-markdown files at HEAD** (2026-08-26). The one finding is
`spikes/run_checks.sh` and it is true.

**A source file has no dated-record class, and that is a rule, not an
oversight.** `docs/pipeline-runs/` and `PHASE*.md` are out of scope because
they carry a publication date and editing them would falsify them. A comment
has no publication date; it is read as current by everyone who opens the file,
and a comment describing a status that has since changed is the defect itself,
not a record of one. History may still be named — the gate's rule is that a
passage may use status vocabulary freely **as long as the current state is
among the words it uses** — but the repair that b7836b5 chose is better:
delete the restatement and point at the decision.

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
import argparse
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

#: The same vocabulary in **declarative** form: SHOUTED English, or Korean,
#: which has no case to shout with. Used for source files only — see the
#: docstring's measurement of what the loose form costs there.
STRICT_RE = re.compile("|".join(
    sorted([w for w in WORDS if w.isascii() and w.isupper()], key=len, reverse=True)
    + sorted([w for w in WORDS if not w.isascii()], key=len, reverse=True)
))

#: A span that is being shown rather than claimed. Backticks and double quotes
#: only: an apostrophe is possessive in this project's prose ("D005's option"),
#: and masking single-quoted spans would swallow half of every sentence. The
#: double-backtick alternative comes first because markdown's ``span with a
#: `backtick` inside`` is how this file quotes the drifted lines themselves,
#: and the single-backtick rule reads it as two empty spans and a claim.
QUOTED_RE = re.compile(r"``.*?``|`[^`\n]*`|\"[^\"\n]*\"|“[^”\n]*”")

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

    **A comment marker is stripped before the join, and `//!` is why.** Rust's
    inner-doc marker ends in `!`, `sentences` splits on `[.:;?!]` followed by
    whitespace, and a joined comment block therefore breaks into a new
    "sentence" at every line — which put the reference and the status word of
    `python.rs`'s D007 restatement into different pieces and lost it. That is
    the line-wrapping blind spot above, rebuilt out of the comment syntax, and
    it was invisible until the widening's own historical control went red on
    two of the four files it was built for. A control that only confirms is not
    one. *주석 표지를 먼저 벗긴다 — `//!`의 `!`가 문장 끝으로 읽혔다.*
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
        # Comment markers first, then blockquote and list markers: both are
        # structure around prose, and neither is prose once the block is joined.
        line = re.sub(r"^\s*(?://[/!]?|#+|;+)\s*", "", line)
        block.append(re.sub(r"^\s*(?:>|[-*+]|\d+\.)\s*", "", line))


def pinpoint(text, lineno, strict):
    """The first line at or after `lineno` that actually carries a status word.

    `passages` reports the line a *paragraph* starts on, which is right for a
    claim spread over a wrapped sentence and wrong for a reader following the
    diagnostic to an edit — `run_checks.sh`'s restatement is on line 2901 and
    its comment block starts on 2898. A fix hint that names the wrong line is
    the kind of small dishonesty that trains people to stop trusting the whole
    line.

    The search stops at the blank line that ends the block, which is the same
    boundary `passages` used to build it, so it cannot wander into the next
    paragraph and name a line that has nothing to do with the finding.
    """
    rx = STRICT_RE if strict else WORD_RE
    lines = text.splitlines()
    for i in range(lineno - 1, len(lines)):
        if i > lineno - 1 and not lines[i].strip():
            break
        if rx.search(lines[i]):
            return i + 1
    return lineno


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


def bound(piece, strict=False):
    """Which decision each status word in this passage is talking about.

    A word describes the decision most recently named before it, falling back
    to the first named after it for the "PROPOSED: D00N ..." shape. Attaching
    every word to every reference instead reads "D003-A's vector union ...,
    D004 drafted PROPOSED" as a claim about D003 as well, which it is not —
    that shape put two false findings in this gate's second run, and a gate
    that cries about correct sentences is one people learn to skip.

    `strict` is the source-file vocabulary: declarative words only, and none
    that sits inside a quoted span. The docstring states what each of those two
    rules removed, and what the first one is known not to reach.
    """
    refs = [(m.start(), m.group()) for m in REF_RE.finditer(piece)]
    out = {}
    if not refs:
        return out
    quoted = [(m.start(), m.end()) for m in QUOTED_RE.finditer(piece)] if strict else []
    for m in (STRICT_RE if strict else WORD_RE).finditer(piece):
        if any(a <= m.start() < b for a, b in quoted):
            continue
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


PRUNE = {".git", "target", "node_modules", ".claude", "venv", ".venv", "__pycache__"}


def walk():
    """Every **tracked** file in the tree, build and tooling directories pruned.

    Tracked, not merely present. The first widening of this gate walked the
    filesystem and counted 32 unreadable files — JacORB's jars and `.class`
    files under `spikes/jacorb/`, which are a gitignored **fixture**. The gate
    therefore went red on any machine where the fixture was installed, which is
    a verdict about somebody's setup rather than about the repository.

    An untracked file is not somewhere this repository states a fact, so it
    cannot restate a decision's status. Asking git also makes the binary
    question moot: a jar is untracked here, and a tracked binary would still be
    reported as unreadable, which is the honest answer for a file that might
    contain the claim and could not be read.
    """
    import subprocess

    r = subprocess.run(
        ["git", "ls-files", "-z", "--cached"], cwd=ROOT, capture_output=True, text=True
    )
    if r.returncode != 0:
        # A producer that could not run is an unmeasured check, never a pass.
        raise SystemExit(
            f"  FAIL git ls-files did not run (exit {r.returncode}) — "
            "the tree was NOT scanned, which is a failure and not a pass"
        )
    for rel in r.stdout.split("\0"):
        if not rel:
            continue
        p = ROOT / rel
        if any(part in PRUNE for part in p.relative_to(ROOT).parts[:-1]):
            continue
        if p.is_file():
            yield p


def source(skipped):
    """(path, text) for every non-markdown file that names a decision.

    A file that never writes `D0NN` cannot restate a decision's status, so the
    reference is the filter — it keeps the walk cheap and, more usefully, keeps
    the scanned set the same set a reader can reproduce with `grep -rl 'D0[0-9]'`.

    `skipped` collects the files that could not be read as text (binaries,
    captures), because a file this gate silently failed to open is exactly the
    unmeasured-check class. There are none today; the counter exists so that
    stays a measured fact rather than an assumption.
    """
    for path in walk():
        if path.suffix == ".md":
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            skipped.append(path)
            continue
        if REF_RE.search(text):
            yield path, text


def cited_out_of_scope():
    """Markdown that names a decision and lives outside this gate's globs.

    The globs read the repository root and `docs/`. `corpus/requirements/`,
    `corpus/include/` and `spikes/tls/` also carry prose, and a status claim
    written there would never be compared with anything. Reported rather than
    scanned: widening the globs changes what the gate covers and would want its
    own false-positive measurement first, exactly as `gap_symbols.py` did
    before proposing itself. What is not reported at all cannot be decided
    about, which is why the count is printed either way.

    **This is now the only class left out.** Every *non*-markdown file in the
    tree is scanned (see `source`), so the remaining gap is markdown outside
    two globs — which reads backwards, and is left that way deliberately: the
    source widening was measured against the four drifted restatements it was
    built for, and widening the markdown globs in the same change would land an
    unmeasured one beside a measured one.
    """
    reachable = set(in_glob())
    out = []
    for p in walk():
        if p.suffix == ".md" and p not in reachable:
            if REF_RE.search(p.read_text(encoding="utf-8", errors="replace")):
                out.append(p)
    return sorted(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", help="scan this tree instead of the repository "
                                   "containing this script — how a control runs "
                                   "today's rules over an older tree")
    args = ap.parse_args()
    if args.root:
        global ROOT, DECISIONS
        ROOT = pathlib.Path(args.root).resolve()
        DECISIONS = ROOT / "docs" / "decisions"

    marked = list(decisions())
    truth = authoritative(marked)
    unknown = [d for d, s in truth.items() if s is None]
    findings = []
    missing = set()
    read = 0
    skipped_docs = []
    unreadable = []
    corpus = [(p, t, False) for p, t in scanned(skipped_docs)]
    n_docs = len(corpus)
    corpus += [(p, t, True) for p, t in source(unreadable)]
    for path, text, strict in corpus:
        read += 1
        for lineno, piece in passages(text):
            for ref, claimed in bound(piece, strict).items():
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
                        (path.relative_to(ROOT), pinpoint(text, lineno, strict),
                         ref, current, sorted(claimed), piece.strip()[:120], strict)
                    )
    n_source = read - n_docs
    for path, lineno, ref, current, claimed, quote, strict in findings:
        print(f"  DRIFT {path}:{lineno} {ref} is {current}, "
              f"passage says {'/'.join(claimed)}")
        print(f"        {quote}")
        if strict:
            # The diagnostic feeds the repair, so it names the repair. b7836b5
            # took this advice for three of the four files that raised it.
            print(f"        fix: delete the status word — a decision's status "
                  f"lives in docs/decisions/{ref}-*.md and")
            print(f"             nowhere else. Cite the file and say what it "
                  f"decided, not what state it is in.")
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
    for path in unreadable:
        print(f"  UNREAD {path.relative_to(ROOT)} could not be read as text and "
              f"was not scanned")
    total = (len(findings) + len(unknown) + len(split) + len(missing)
             + len(unread) + len(half) + len(unreadable))
    n_marks = sum(len(m) for _, m in marked)
    print(f"  {len(truth)} decisions, {n_marks} status marker(s) read, "
          f"{len(unread)} unread; {n_docs} living document(s) and {n_source} "
          f"non-markdown file(s) scanned, "
          f"{len(skipped_docs)} dated record(s) out of scope")
    outside = cited_out_of_scope()
    print(f"  {len(outside)} document(s) cite a decision from outside this gate's "
          f"globs and are not scanned"
          + (": " + ", ".join(str(p.relative_to(ROOT)) for p in outside) if outside else ""))
    print(f"  {total} drifted status claim(s)")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
