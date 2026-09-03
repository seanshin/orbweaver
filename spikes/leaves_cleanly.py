#!/usr/bin/env python3
"""Every fixture that creates an omniORB ORB leaves through `orbexit.leave`.

`spikes/orbexit.py` is the one home for *how an omniORB fixture leaves*: flush,
then `os._exit`, skipping the `Py_Finalize` that races omniORB's own C++ thread
scavenger. Twenty-three fixtures called it. Four did not, and nothing said so —
the module was written, adopted by a sweep on the day, and the sweep's scope
became the record of who was covered.

On 2026-08-28 one of the four took the crash:

    Thread 0   __exit <- exit <- dyld4::LibSystemHelpers::exit  (finalization)
    Thread 1   bind_gilstate_tstate <- _PyThreadState_Attach
               <- PyGILState_Ensure <- omnipyThreadScavenger::run_undetached
               EXC_BAD_ACCESS (SIGSEGV) at 0x1cd8

It was the `-c` child inside `native_capture.py`, which the harness reported as
`FAIL the omniORB runtime probe did not run` — a red run that said nothing at
all about the reason, because the probe discarded the exit status. The crash
reporter had the diagnosis and the harness did not.

**The unit is a PROGRAM, not a file, and the first draft of this gate got that
wrong in a way that cost a second crash four hours later.** It asked whether the
FILE mentions `orbexit`, and every one of these parents does — so it read
`27 fixture(s) create an ORB; 27 leave through orbexit.leave` while EIGHT
embedded children, carried as string constants and run with
`[sys.executable, "-c", …]`, did not. Seven fixtures carry such a child; only
the one that had just been repaired by hand was leaving cleanly. The second
report named the shape the first had not: `Parent Process: Python`, and thread 0
in `__cxa_finalize_ranges` running omniORB's C++ static destructors — which
`os._exit` does not reach.

*A rule about programs, checked against files, is green over every program a
file carries.* That is *a sweep is scoped to a rule, not a file* one layer
down, and it was written into CLAUDE.md the day before this gate repeated it.

So this walks the AST for string constants that are programs, and asks of each:
does it name `ORB_init`, and does it reach `leave`? A child reaches `leave`
either by carrying it or by being handed to `orbexit.wrap_child` at its spawn
site, which is where they are wrapped now — one home, applied once per launch
rather than eight times by hand.

*하나의 집이 있었고, 스윕이 그 집을 채운 날의 범위가 곧 누가 덮였는지의 기록이
되었다. 넷이 빠졌고 아무것도 그렇게 말하지 않았으며, 2026-08-28에 그중 하나가
크래시를 가져갔다 — 하네스는 "프로브가 돌지 않았다"고만 적었고 이유는 크래시
리포터에만 있었다. 기준은 파일 이름이 아니라 `ORB_init`이며, 문자열 상수로 품은
자식 프로그램도 포함한다 — 크래시는 바로 거기 있었다.*
"""

import ast
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
HOME = "orbexit"
#: The thing that makes a file owe the rule. Deliberately a plain text search:
#: the child program that crashed lives in a string constant, so an AST walk
#: over the file's own code would not have seen it.
CREATES_AN_ORB = re.compile(r"\bORB_init\b")

#: Comments and docstrings quote `ORB_init` while creating nothing. Asked of the
#: file's own code only — a string constant is handled separately, as a program.
_STRINGS = re.compile(r'"""[\s\S]*?"""|\'\'\'[\s\S]*?\'\'\'|#[^\n]*')


def strip_strings(text):
    return _STRINGS.sub("", text)


def unwrapped_launches(text):
    """Every `-c` launch in `text` whose program is not handed to `wrap_child`.

    **Asked of the launch, never of the string.** The first attempt at this
    walked the AST for string constants that parse as Python, and it found
    nothing in `union_label_capture.py` — that child is assembled at run time
    from a template, so the constant is not a program and never will be. The
    control caught it: unwrapping a spawn site left the gate green over exactly
    the case it exists for.

    A launch is a list containing the constant `"-c"`; the element after it is
    the program. That is true whether the program is a name, a template
    substitution or a literal, which is why this is the question to ask.
    """
    try:
        tree = ast.parse(text)
    except SyntaxError:
        return
    for node in ast.walk(tree):
        if not isinstance(node, ast.List):
            continue
        for i, el in enumerate(node.elts[:-1]):
            if not (isinstance(el, ast.Constant) and el.value == "-c"):
                continue
            prog = node.elts[i + 1]
            wrapped = (isinstance(prog, ast.Call)
                       and isinstance(prog.func, ast.Name)
                       and prog.func.id == "wrap_child")
            if not wrapped:
                yield node.lineno


def main(argv):
    probe = "--probe" in argv
    # `git ls-files`, never a directory walk. This read `ROOT.glob("spikes/**/*.py")`
    # until 2026-09-03, when `spikes/tls/setup.sh` first built omniORBpy into an
    # ignored directory under spikes/ and the walk handed this scan a Python 2
    # file (`2147483647L`) it could not parse — so the gate could not run at all,
    # over a tree with no defect in it. CLAUDE.md already says why `git ls-files`
    # is right: it is what keeps a scan out of an ignored vendor tree. The rule
    # was written about a 532 MB one and applied here by a 7 MB one.
    tracked = subprocess.run(
        ["git", "ls-files", "spikes/*.py", "spikes/**/*.py"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    files = sorted(ROOT / f for f in tracked if (ROOT / f).is_file())
    if not files:
        print("  FAIL no spikes/**/*.py were found — this scan measured nothing,")
        print("       which is a failure and never a pass")
        return 2

    owes, ok = [], []
    for p in files:
        if p.name == "orbexit.py":
            continue  # the home itself
        try:
            text = p.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        rel = p.relative_to(ROOT).as_posix()

        # 1 — the file's own program.
        if CREATES_AN_ORB.search(strip_strings(text)):
            (ok if HOME in text else owes).append((rel, "the script itself"))

        # 2 — every program it LAUNCHES. The file mentioning `orbexit` says
        #     nothing about these, which is the hole the second crash found.
        #     Every `-c` launch is required to go through `wrap_child`, whether
        #     or not this scan can tell that its child creates an ORB: eight of
        #     the nine did, and the one that does not is not worth a second
        #     rule that would have to be kept true by hand.
        seen_launch = False
        for node in ast.walk(ast.parse(text)) if text.strip() else []:
            if isinstance(node, ast.List) and any(
                    isinstance(e, ast.Constant) and e.value == "-c" for e in node.elts):
                seen_launch = True
        for lineno in unwrapped_launches(text):
            owes.append((rel, "the `-c` child launched at line %d" % lineno))
        if seen_launch and not list(unwrapped_launches(text)):
            ok.append((rel, "every `-c` child it launches"))

    if probe:
        return 0

    print("  %d program(s) create an ORB; %d leave through %s.leave" % (
        len(ok) + len(owes), len(ok), HOME))
    if owes:
        print("  FAIL a program creates an omniORB ORB and does not leave through")
        print("       spikes/orbexit.py. Falling off the end runs Py_Finalize and")
        print("       __cxa_finalize_ranges into omniORB's live thread scavenger,")
        print("       which is a SIGSEGV that reads as a failed measurement:")
        for rel, what in owes:
            print("         %s — %s" % (rel, what))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
