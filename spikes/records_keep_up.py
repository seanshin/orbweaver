#!/usr/bin/env python3
"""Have the records that describe the code been touched since the code was?

`COMPONENTS.md` states what is measured *now* and `CHANGELOG.md` states what
changed. Neither can be checked for truth by a script. What can be checked is
whether they were opened at all, and on 2026-08-18 the answer was **thirty-nine
commits of behaviour change ago** — six of them wire-behaviour changes — while
three `COMPONENTS.md` rows had become false: a crate's gap column claiming work
that had landed, and a row calling a measurement unmeasured.

This is the crude half of a rule whose precise half is not automatable. It does
not read a word of either file; it counts commits that touched `crates/` and
`spikes/` against the last commit that touched each record, and it fails when
that distance exceeds a threshold no batch should reach. A batch that lands
with its record is at zero.

기록이 참인지는 스크립트가 볼 수 없다. 볼 수 있는 것은 **열어보기라도 했는가**이며,
2026-08-18의 답은 "행동이 바뀐 커밋 39개 전"이었고 그동안 COMPONENTS의 세 행이
거짓이 되어 있었다.
"""
import subprocess
import sys

# One wave of parallel batches is four or five commits plus their merges. Ten
# is comfortably past "I will write it up with the next one" and comfortably
# short of the thirty-nine that prompted this.
ALLOWED = 10

RECORDS = ["CHANGELOG.md", "docs/COMPONENTS.md"]
WATCHED = ["crates/", "spikes/"]


def run(*args):
    return subprocess.run(args, capture_output=True, text=True).stdout.strip()


def main():
    bad = 0
    for record in RECORDS:
        last = run("git", "log", "-1", "--format=%H", "--", record)
        if not last:
            print(f"  FAIL {record} has no history — it is not in this repository")
            bad += 1
            continue
        behind = run("git", "log", "--oneline", f"{last}..HEAD", "--", *WATCHED)
        n = len([line for line in behind.split("\n") if line.strip()])
        if n > ALLOWED:
            print(f"  FAIL {record} is {n} commit(s) behind the code it describes")
            for line in behind.split("\n")[:5]:
                print(f"         {line}")
            print(f"         (allowed: {ALLOWED}. A batch lands with its record, not after it.)")
            bad += 1
        else:
            print(f"  ok   {record} is {n} commit(s) behind the code (allowed {ALLOWED})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
