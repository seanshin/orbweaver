#!/usr/bin/env python3
"""No fixture is waited for by "its IOR file exists, then sleep a guess".

**A published IOR is not an accepting listener.** A fixture writes its IOR
before `the_POAManager().activate()` returns on some ORBs and before `accept()`
is reached on others, so a wait that stops at the file and then sleeps a fixed
amount is a guess about a race — and CLAUDE.md records the two ways it is
already known to lose: `ping(): io: Resource temporarily unavailable (os error
35)` against JacORB (harness 34) and against omniORB (harness 51).

# Why this scan exists rather than just the repair

`81cc546` fixed this on 2026-08-29 — **for the one group that had gone red.**
Swept 2026-08-31: **eighteen sites had the shape and seventeen still did**, six
of them against the same JacORB peer with the same 0.5s guess. That is this
project's own rule arriving as a cost: *a sweep is scoped to a rule; a sweep
that names a file will sweep that file.* So the sweep lands with the scan, over
`git ls-files`, and the rule's implementation lives in one place
(`spikes/lib/accepting.sh`) rather than being restated at each good site.

# What this scan does NOT see, and how that was found

**It hunts a spelling, and the rule is wider than the spelling.** On 2026-08-31
harness 59 failed `LOCATION_FORWARD vs _PERM` with *"the client never made its
first call"* while four standalone runs passed. The cause was in
`spikes/perm_fallback.sh`, which waits with its own `wait_file` helper and then
a `sleep 0.3` — the same defect in a shape this regex cannot match, and the
script **named it in its own comment**: *"The IOR file is written before the
accept loop starts; give it a beat (the same 0.3 s run_checks.sh gives
spike-server)."* The diagnosis was right, the remedy was a guess, and the
precedent it cited had just been deleted.

That site is converted. What is recorded here is the limit: *a batch scoped to a
keyword will fix a keyword.* A wait that stops at the IOR file can be written as
`[ -s x.ior ] && sleep`, as a helper called with the path, or as anything else,
and only the first is caught. Widening the regex to every `.ior` near every
`sleep` was tried and rejected — it flags correct loops that sleep BETWEEN
tries, which is the shape the rule asks for. The durable defence is that the
helper exists and is the obvious thing to reach for, not that this scan is
exhaustive; it is not, and saying so is better than a number that implies it is.

# What is excluded, by the rule and not by oversight

`spikes/nat/` measures an address that is deliberately NOT dialable from where
the harness runs — that is the entire point of a NAT fixture. An accept-probe
from here would score those fixtures' purpose as a failure, so they wait on the
file and sleep *between* tries, which is correct for what they measure. They are
excluded by name here and the reason is this paragraph.

*발행된 IOR은 accept하는 리스너가 아니다. 2026-08-29의 수리는 빨개진 그룹 하나에만
범위가 맞춰져 있었고, 열여덟 곳 중 열일곱이 그대로였다 — 그중 여섯은 같은 피어에
같은 0.5초 추측. **스윕은 그것을 만든 스캔과 함께 착지한다.** NAT 픽스처는 제외하되,
그 이유는 규칙이다: 그 주소가 여기서 닿지 않는 것이 바로 그 픽스처가 재는 것이다.*
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: `[ -s …ior… ]` (or `if [ -s … ]`) whose success branch sleeps a fixed amount.
#: The `sleep` must be on the SAME logical branch — a loop that sleeps between
#: tries is a correct wait and must not be flagged.
SHAPE = re.compile(
    r"\[\s*-s\s+[^]]*\.ior[^]]*\]\s*(?:&&\s*\{|\]?;?\s*then)[^\n]*?sleep\s+[0-9.]+",
)

#: Excluded by the rule stated in this module's docstring, not by convenience.
EXCLUDED = ("spikes/nat/",)


def offenders(files):
    out = []
    for rel in files:
        p = ROOT / rel
        if not p.is_file():
            continue
        if any(rel.startswith(x) for x in EXCLUDED):
            continue
        try:
            text = p.read_text(errors="replace")
        except OSError:
            continue
        for i, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            if SHAPE.search(line):
                out.append((rel, i, line.strip()))
    return out


def tracked():
    """The shell scripts under `spikes/`, and only those.

    **`.sh` and not `.py`, by the rule rather than for quiet.** The shape this
    hunts is shell syntax — `[ -s x.ior ] && { sleep 0.2; }` — and only a shell
    executes it; a Python fixture that waits does it in Python and would need a
    rule of its own. Checked before narrowing, because narrowing a scan until it
    stops complaining is the tune-until-quiet defect this project names: of the
    tracked `spikes/*.py`, **none waits for an IOR in any form** (the only `.ior`
    hits are this file's own probe table and one `os.path.exists` in
    `service_sweep.py`, which is a read and not a wait). If a Python fixture ever
    grows a wait, this scan does not cover it and this paragraph is the record
    of that.

    It also settles a self-reference the first version had: this file carries the
    defective shapes as probe literals, so scanning `.py` made the gate fail on
    its own probe table — correctly, and uselessly. Excluding it by name would
    have been a pin on a live subject; excluding it by *what a shell can run* is
    the rule.
    """
    return [
        r
        for r in subprocess.run(
            ["git", "ls-files", "spikes"], cwd=ROOT, capture_output=True, text=True
        ).stdout.split()
        if r.endswith(".sh")
    ]


#: (what it is, one line of shell, must it be flagged)
PROBES = [
    ("the defect: file exists, then a fixed guess",
     '  [ -s "$ROOT/spikes/echo.ior" ] && { sleep 0.2; return 0; }', True),
    ("the same in `if … then` form",
     '  if [ -s "$D/j.ior" ]; then sleep 0.5; started=1; break; fi', True),
    ("the repair: a call to the shared helper",
     '  wait_accepting "$ROOT/spikes/echo.ior" --deadline 10 && return 0', False),
    ("a correct loop that sleeps BETWEEN tries",
     '    [ -s shared/server.ior ] && return 0', False),
]


def probe():
    # This file carries the defect as literals, so a scan that read it would
    # flag its own probe table. It reads shell scripts; assert that, or the
    # next reader learns it from a red run.
    if any(r.endswith(".py") for r in tracked()):
        print("  FAIL this scan is reading Python files, where the shape it hunts")
        print("       cannot execute — and where its own probe table lives, so it")
        print("       would flag itself and mean nothing")
        return 2
    for what, line, want in PROBES:
        got = bool(SHAPE.search(line))
        if got != want:
            print("  FAIL the probe %r was %s and must be %s, so this scan cannot"
                  % (what, "flagged" if got else "passed",
                     "flagged" if want else "passed"))
            print("       see the shape it exists to see and its silence means nothing")
            return 2
    return 0


def main(argv):
    if "--probe" in argv:
        return probe()

    files = tracked()
    if not files:
        print("  FAIL `git ls-files spikes` returned nothing, so this scan read no")
        print("       files — an unmeasured check is a failure, never a pass")
        return 2

    bad = offenders(files)
    if bad:
        print("  FAIL %d wait(s) stop at the IOR file and then sleep a fixed guess." % len(bad))
        print("       A published IOR is not an accepting listener; use")
        print("       `wait_accepting` from spikes/lib/accepting.sh:")
        for rel, line, text in bad:
            print("         %s:%d  %s" % (rel, line, text[:70]))
        return 1

    print("  ok   %d spike script(s) scanned; no fixture is waited for by its IOR"
          % len(files))
    print("       file plus a fixed sleep (spikes/nat/ excluded — its addresses are")
    print("       deliberately not dialable from here, which is what it measures)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
