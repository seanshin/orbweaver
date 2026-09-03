#!/usr/bin/env python3
"""Every scan over this tree enumerates tracked files, never walks a directory.

# The instance that raised it

2026-09-03: `spikes/tls/setup.sh` built omniORBpy into `spikes/tls/omniORBpy/`
— ignored by git, on the same terms as `spikes/tao/ACE_wrappers/`. That tree
carries a `python2/` directory. `spikes/leaves_cleanly.py` enumerated its
subjects with `ROOT.glob("spikes/**/*.py")`, walked into it, handed
`ast.parse` a file containing `2147483647L`, and **could not run at all** —
over a tree with no defect in it. The harness read that correctly as a
failure (*a gate that cannot run measures nothing*), and the gate's own text
named the rule it broke: CLAUDE.md says `git ls-files` is right because *it is
what keeps a scan out of an ignored vendor tree*. That rule was written about a
532 MB one; a 7 MB one applied it.

`spikes/entry_cost.py` had the identical walk over `*.rs` and survived only
because omniORBpy carries no Rust. Same rule, one sweep — which is what this
file is: the scan the sweep landed with, runnable over the whole tree.

# What is and is not a walk

A **walk** is `glob`/`rglob`/`os.walk` that can reach `spikes/`, because that
is where every ignored fixture build in this repository lives — `jacorb/lib`,
`jacorb/gen`, `jacorb/classes`, `tao/ACE_wrappers`, `tls/omniORBpy` — read off
`.gitignore` rather than assumed. A glob over `docs/` or `crates/*/src` walks
nothing ignored and is not this defect; the first draft of this scan reported
those and would have had the rule tuned until quiet, which is the wrong
direction. The rule is scoped to where the defect can occur, not to the verb.

Two shapes under `spikes/` are not walks either, and are excluded by reason:

- a walk over a directory the script **itself created** this run
  (`codeset_advertise_probe.py` walks the `gen` dir it just asked JacORB's IDL
  compiler to fill) — nothing ignored can be in it;
- the **fallback** of a scan that enumerates with `git ls-files` first and
  walks only where there is no `.git` (`decision_status.py`, for the tree
  `scope_controls.sh` extracts with `git archive`), guarded by a prune list.

Exclusions are listed here by file and reason. A new walk is a finding until
its reason is written beside it.

Exit 1 on any finding.

*트리를 훑는 모든 스캔은 추적 파일을 열거하지, 디렉터리를 걷지 않는다. 2026-09-03에
ignore된 픽스처 빌드가 `python2/`를 들여왔고 `leaves_cleanly.py`의 워크가 그리로
들어가 **결함 없는 트리 위에서 게이트가 아예 돌지 못했다.** CLAUDE.md는 이미 왜
`git ls-files`가 맞는지 적어두었다 — 532 MB짜리에 대해 쓴 규칙을 7 MB짜리가
적용했다. 제외 둘은 이유가 있다: 자기가 이번 실행에 만든 디렉터리를 걷는 것과,
`git ls-files`를 먼저 쓰고 `.git`이 없을 때만 걷는 폴백.*
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: A walk that can reach `spikes/`: a glob whose pattern names it, or an
#: `rglob`/`os.walk` from the tree root (which reaches everything). `HERE` is
#: what scripts living in spikes/ call their own directory.
WALK = re.compile(
    r"""\b(?:ROOT|HERE)\.glob\(\s*["'][^"']*spikes/"""   # ROOT.glob("spikes/**")
    r"""|\bHERE\.r?glob\("""                            # a spikes/ script globbing itself
    r"""|\bROOT\.rglob\("""                              # the whole tree, spikes/ included
    r"""|\bos\.walk\(\s*(?:ROOT|HERE)\b"""
)

#: Excluded by reason. Path → the reason, which must be true.
EXCLUDED = {
    "spikes/codeset_advertise_probe.py":
        "walks the directory it created this run for JacORB's generated stubs",
    "spikes/decision_status.py":
        "enumerates with git ls-files first; the walk is the no-.git fallback, pruned",
    "spikes/tracked_not_walked.py":
        "this scan: its pattern source and probe literal spell the defect on purpose",
}


def main(argv):
    probe = "--probe" in argv
    tracked = subprocess.run(
        ["git", "ls-files", "spikes/*.py", "spikes/**/*.py"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()

    if probe:
        # The scan must see a walk when one is written. A synthesised line, so
        # the control does not depend on a defect surviving in the tree.
        if not WALK.search('files = sorted(ROOT.glob("spikes/**/*.py"))'):
            print("  FAIL the probe line is a walk and the scan did not see it")
            return 2
        if WALK.search('files = subprocess.run(["git", "ls-files"])'):
            print("  FAIL the scan reports ls-files as a walk")
            return 2
        return 0

    findings = []
    for rel in sorted(tracked):
        if rel in EXCLUDED:
            continue
        text = (ROOT / rel).read_text(errors="replace")
        for ln, line in enumerate(text.splitlines(), 1):
            stripped = line.lstrip()
            if stripped.startswith("#"):
                continue
            if WALK.search(line):
                findings.append((rel, ln, line.strip()))

    print(f"  {len(tracked)} tracked python file(s) under spikes/; "
          f"{len(EXCLUDED)} walk(s) excluded by reason")
    if not findings:
        print("  ok   every scan enumerates tracked files; no directory walk over the tree")
        return 0
    print(f"  FAIL {len(findings)} directory walk(s) over the tree:")
    for rel, ln, line in findings:
        print(f"       {rel}:{ln}: {line}")
    print("       an ignored fixture build under spikes/ is handed to these as though it were ours")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
