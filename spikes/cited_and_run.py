#!/usr/bin/env python3
"""Every executable a document cites either runs, or says it does not.

Four times on 2026-08-28 a document named a script as its evidence and nothing
ran that script, so the evidence was never taken:

  spikes/c_peer.sh          cited by C-PEER-STATUS and D029's Backend row. It
                            had never been compiled on Linux. Its first harness
                            run failed on a glibc `-Werror=format-truncation`
                            the author's macOS clang cannot produce.
  spikes/event_by_name.sh   cited by D029's "Location, for event channels" as
                            the half that makes the claim "a measurement rather
                            than a self-test". `grep -c` over `run_checks.sh`
                            and `ci.yml` both returned 0.
  spikes/scope_controls.sh  the negative control for two scope widenings. Run
                            by nothing — and it had also stopped being able to
                            run, because the widening it controls gained a
                            `git ls-files` scan and the control feeds it a tree
                            `git archive` extracted, which has no `.git`.
  spikes/half_reply.sh      cited by COMPONENTS.md and D017. Its own row in
                            COMPONENTS said "not yet a `run_checks.sh` group",
                            and had said so since the day it was written.

**Three of the four said so in their own headers**, and that is the finding
rather than a detail. `c_peer.sh`'s status record named "the recommended group";
`event_by_name.sh` wrote *"Wiring it in is one `hr` group and is named as undone
in the report"*; `half_reply.sh`'s row said "not yet a group". Naming a debt is
not paying it, and a debt narrated in a header is a debt nobody counts.

# What this refuses, and what it accepts

It accepts a script that is **run** — by `run_checks.sh`, by `ci.yml`, or by
another script that is itself run (one level: `service_sweep.py` is invoked by
`service_sweep.sh`, which is a group).

It accepts a script that **states a refusal**: `crossing_facts.py`,
`gap_symbols.py`, `plan_numbers.py` and `bilingual_drift.py` all say *"a report,
not a gate"*, and CLAUDE.md says why — there is no defensible threshold for what
they measure. A decision not to gate is a decision, and this asks only that it
be written down.

It refuses a script whose header **defers** the wiring — "not wired into",
"named as undone", "the recommended group", "wiring it in is". That is the exact
sentence three of today's four findings carried, and the difference between it
and a refusal is the whole point of this gate: one is a choice, the other is an
IOU nothing was counting.

# What it does not do

It does not read `tests/*.rs`. Those run under `cargo test --workspace`, which
is a group, so a Rust test cited in a document IS taken — the first sweep that
found this class reported 53 hits and 47 of them were that false positive.

It does not check that a script that runs is run *usefully*. A group that
executes a script and asserts nothing is the class this project calls
green-while-measuring-nothing, and no scan of citations can see it.

*문서가 인용하는 실행물은 돌거나, 돌지 않는다고 자기 안에 적어야 한다. 2026-08-28에
네 번 나왔고, 그중 셋은 자기 헤더에 "아직 안 했다"고 적어두고 있었다 — 서술된 빚은
아무도 세지 않는 빚이다. **거절은 결정이고 유예는 차용증이다.** `tests/*.rs`는 읽지
않는다: `cargo test --workspace`가 그룹이므로 인용된 러스트 테스트는 이미 돈다.*
"""

import pathlib
import subprocess
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Documents whose citations count. `docs/pipeline-runs/` and `PHASE*.md` are
#: dated records — they state what was true on a day, so a script they name may
#: since have been deleted on purpose, and reading them would turn history into
#: a debt.
DOC_GLOBS = ["docs/*.md", "docs/decisions/*.md", "CLAUDE.md", "README.md"]

#: A header that says the script is deliberately not a gate. Accepted.
REFUSES = re.compile(
    r"report,? not a gate|not a gate|control,? not a gate|never a gate", re.I
)

#: A header that says the wiring is owed. Refused — this is the sentence all
#: three of 2026-08-28's findings carried.
DEFERS = re.compile(
    r"not wired into|named as undone|recommended group|wiring it in is|"
    r"is one `?hr`? group and",
    re.I,
)

CITE = re.compile(r"spikes/[A-Za-z0-9_./-]+\.(?:sh|py)")


def cited():
    """Every `spikes/…` executable a living document names, and where."""
    out = {}
    for g in DOC_GLOBS:
        for d in sorted(ROOT.glob(g)):
            try:
                text = d.read_text()
            except (OSError, UnicodeDecodeError):
                continue
            for m in CITE.finditer(text):
                rel = m.group(0)
                if (ROOT / rel).is_file():
                    out.setdefault(rel, set()).add(d.name)
    return out


def runners():
    """Text of everything that could invoke a spike: the harness and CI."""
    parts = []
    for rel in ("spikes/run_checks.sh", ".github/workflows/ci.yml"):
        p = ROOT / rel
        if p.is_file():
            parts.append(p.read_text())
    return "\n".join(parts)


def main(argv):
    probe = "--probe" in argv
    run_text = runners()
    if not run_text.strip():
        print("  FAIL neither the harness nor the workflow could be read — this scan")
        print("       measured nothing, which is a failure and never a pass")
        return 2

    # `spikes/**`, not `spikes/*`. The first draft globbed one level and reported
    # `spikes/jacorb/setup.sh` as owing a group while `ci.yml` runs it by that
    # exact path — a scan that cannot see a file cannot be trusted about its
    # silence either. Keyed by path-relative-to-root AND by basename, because a
    # runner may spell it either way.
    #
    # **`git ls-files`, not a directory walk**, and the difference is not
    # hypothetical: `spikes/tao/setup.sh` builds a DOC-licensed front end into
    # `spikes/tao/ACE_wrappers/`, which is gitignored and holds **thousands** of
    # upstream `.sh` files. A walk enumerates all of them and asks whether the
    # harness runs `run_test.sh`. This repository already learned this from the
    # other side, when a serve-site count read the eight agent worktrees under
    # `.claude/` as eight more copies of itself: *ask git what this repository
    # is, rather than asking the filesystem what is under this path.*
    tracked = subprocess.run(
        ["git", "ls-files", "spikes"], cwd=ROOT, capture_output=True, text=True
    ).stdout.split()
    spikes = {
        rel: (ROOT / rel).read_text(errors="replace")
        for rel in tracked
        if rel.endswith((".sh", ".py")) and (ROOT / rel).is_file()
    }

    # **What needs a runner and what can BE one are different sets.** A cited
    # executable is a `.sh` or a `.py`; a thing that invokes one is any file the
    # harness reaches. `spikes/binding_suite.sh` is a group and invokes its cells
    # through `spikes/bindings/<language>.manifest`, whose rows are
    # `cell servant jacorb spikes/bindings/python/servant-jacorb.sh` — so the
    # chain to `jacorb_python_servant.sh`, which D029 and D032 both cite as the
    # big-endian servant reading, runs through a file this scan would not open.
    # Restricting the *runner* side to `.sh`/`.py` broke that chain the moment
    # the accidental basename match stopped covering for it.
    carriers = {
        rel: (ROOT / rel).read_text(errors="replace")
        for rel in tracked
        if (ROOT / rel).is_file() and not rel.endswith((".idl", ".tsv"))
    }

    # A basename identifies a spike only when it is the only one with that name.
    # **`spikes/tao/setup.sh` was reported as run the moment it existed**, because
    # `ci.yml` runs `spikes/jacorb/setup.sh` and the two share a basename — a
    # false green in the gate written to catch exactly this class of false green.
    # Five `run.sh`, two `agent.py` and two `client-omniorb.sh` are in the same
    # position and were before today. A name that does not identify a thing
    # cannot be used to say the thing was run.
    ambiguous = {
        name
        for name in (pathlib.Path(rel).name for rel in spikes)
        if sum(1 for r in spikes if pathlib.Path(r).name == name) > 1
    }

    def code_only(text):
        """The text with `#` comment lines removed.

        **A mention is not an invocation.** `differential.sh` explains in a
        comment that `spikes/tao/setup.sh` builds the fixture it looks for, and
        that sentence alone made this gate report the setup script as run — the
        gate is transitive, `differential.sh` is a group, and a name in a
        comment reads exactly like a name in a command. It is the shape
        CLAUDE.md already names for a different gate: *ask the launch, never the
        string.* Full-line comments are what carry prose here; stripping them is
        not a parser and does not pretend to be one, but it is the difference
        between reading code and reading commentary.
        """
        return "\n".join(
            l for l in text.splitlines() if not l.lstrip().startswith("#")
        )

    def named_in(text, rel):
        code = code_only(text)
        if rel in code:
            return True
        # **A directory a runner names, it drives.** `binding_suite.sh` reaches
        # its cells through `spikes/bindings/<language>.manifest`, a path
        # assembled at run time — `BDIR="$ROOT/spikes/bindings"` and the
        # language appended — so no literal for the manifest exists to match.
        # This is the shape CLAUDE.md already records for `leaves_cleanly.py`:
        # a child that is *"assembled at run time from a template and is not a
        # constant at all"*. Naming the directory is the strongest literal such
        # a runner can offer, and taking it is what keeps the chain to
        # `jacorb_python_servant.sh` — D029's and D032's big-endian servant
        # reading — from breaking at exactly one link. It errs toward calling
        # something run; the cost of that is a missed debt, against a false red
        # over evidence that IS run, which kills a gate faster.
        parent = str(pathlib.Path(rel).parent)
        if parent not in ("", ".", "spikes") and parent in code:
            return True
        name = pathlib.Path(rel).name
        return name not in ambiguous and name in code

    # TRANSITIVE, not one level. `trading_client.py` is invoked by
    # `service_sweep.py`, which is invoked by `service_sweep.sh`, which is a
    # group — three deep. A fixed depth is a threshold in disguise, and this one
    # was wrong at depth 1.
    # Reached directly by the harness or the workflow, then transitively through
    # anything already reached — including a manifest or a data file, which is
    # why the closure walks `carriers` and not just the candidates.
    reached = {rel for rel in carriers if named_in(run_text, rel)}
    changed = True
    while changed:
        changed = False
        for rel in carriers:
            if rel in reached:
                continue
            if any(named_in(carriers[r], rel) for r in reached):
                reached.add(rel)
                changed = True

    owed, refused, ran = [], [], []
    for rel, where in sorted(cited().items()):
        head = "\n".join(spikes.get(rel, "").splitlines()[:24])
        if rel in reached:
            ran.append(rel)
        elif DEFERS.search(head):
            owed.append((rel, sorted(where), "its header defers the wiring"))
        elif REFUSES.search(head):
            refused.append(rel)
        else:
            owed.append((rel, sorted(where), "no group, and its header says nothing"))

    if probe:
        # **The probe used to return 0 having asserted nothing** — it proved the
        # scan could execute, which is not the same as proving it can see. Each
        # case below is a way this scan was wrong on 2026-08-31, and each is run
        # against the shipped `named_in`/`code_only` rather than a restatement
        # of them.
        cases = [
            ("an invocation is a reach",
             "out=$(./spikes/x/y.sh --flag)\n", "spikes/x/y.sh", True),
            ("a COMMENT mention is not an invocation",
             "# see spikes/x/y.sh for why\n", "spikes/x/y.sh", False),
            ("a directory a runner names, it drives",
             'BDIR="$ROOT/spikes/bindings"\n', "spikes/bindings/p.manifest", True),
            ("a bare unambiguous basename still reaches",
             "bash uniquely_named_thing.sh\n", "spikes/uniquely_named_thing.sh", True),
        ]
        for what, text, rel, want in cases:
            got = named_in(text, rel)
            if got != want:
                print("  FAIL the probe %r came back %s and must be %s, so this scan"
                      % (what, got, want))
                print("       cannot see what it exists to see and its silence over the")
                print("       tree means nothing")
                return 2
        # The ambiguity rule needs the real ambiguous set, which is computed from
        # the tree: `setup.sh` is shared by the JacORB and TAO fixtures, and
        # matching it by basename reported `spikes/tao/setup.sh` as run the day
        # it was written, because `ci.yml` runs the other one.
        if "setup.sh" not in ambiguous:
            print("  FAIL `setup.sh` is not in this scan's ambiguous set, though two")
            print("       fixtures carry that name. The rule that stops a shared")
            print("       basename standing in for a path is not in force")
            return 2
        if named_in("bash spikes/jacorb/setup.sh\n", "spikes/tao/setup.sh"):
            print("  FAIL running one `setup.sh` counts as running another. That is the")
            print("       false green this gate exists to refuse, in this gate")
            return 2
        # And the walk must be `git ls-files`: the TAO fixture builds thousands
        # of upstream scripts into an ignored directory, and a scan that reads
        # them is asking whether the harness runs `run_test.sh`.
        if any("ACE_wrappers" in rel for rel in spikes):
            print("  FAIL this scan is reading the ignored TAO build tree, so it is")
            print("       enumerating upstream files as though they were ours")
            return 2
        return 0

    print(f"  {len(ran)} cited spike(s) run; {len(refused)} state a refusal;"
          f" {len(owed)} owe a group")
    if owed:
        print("  FAIL a document cites an executable that nothing runs, and its header")
        print("       neither refuses the gate nor was ever wired in. A debt named in a")
        print("       header is a debt nobody counts — three of these were found by hand")
        print("       on 2026-08-28 and each had been sitting since the day it landed:")
        for rel, where, why in owed:
            print(f"         {rel}  ← {', '.join(where[:3])}")
            print(f"           {why}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
