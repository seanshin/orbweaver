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

**The criterion is `ORB_init`, not the file's name or where it lives.** That is
`orbexit`'s own: it says it is "not for scripts that do not create an ORB —
`coverage_tables.py` and `service_sweep.py` matched an early, broader sweep and
have no `ORB_init` at all, which is why they are not here." A file that names
`ORB_init` anywhere — in its own code or in a child program it carries as a
string constant, which is where the crash actually lived — owes the import.

*하나의 집이 있었고, 스윕이 그 집을 채운 날의 범위가 곧 누가 덮였는지의 기록이
되었다. 넷이 빠졌고 아무것도 그렇게 말하지 않았으며, 2026-08-28에 그중 하나가
크래시를 가져갔다 — 하네스는 "프로브가 돌지 않았다"고만 적었고 이유는 크래시
리포터에만 있었다. 기준은 파일 이름이 아니라 `ORB_init`이며, 문자열 상수로 품은
자식 프로그램도 포함한다 — 크래시는 바로 거기 있었다.*
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
HOME = "orbexit"
#: The thing that makes a file owe the rule. Deliberately a plain text search:
#: the child program that crashed lives in a string constant, so an AST walk
#: over the file's own code would not have seen it.
CREATES_AN_ORB = re.compile(r"\bORB_init\b")


def main(argv):
    probe = "--probe" in argv
    files = sorted(p for p in ROOT.glob("spikes/**/*.py") if p.is_file())
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
        if not CREATES_AN_ORB.search(text):
            continue
        (ok if HOME in text else owes).append(p.relative_to(ROOT).as_posix())

    if probe:
        return 0

    print("  %d fixture(s) create an ORB; %d leave through %s.leave" % (
        len(ok) + len(owes), len(ok), HOME))
    if owes:
        print("  FAIL a fixture creates an omniORB ORB and does not leave through")
        print("       spikes/orbexit.py. Falling off the end of __main__ runs")
        print("       Py_Finalize into omniORB's thread scavenger, which is a SIGSEGV")
        print("       that reads as a failed measurement rather than as a crash:")
        for rel in owes:
            print("         %s" % rel)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
