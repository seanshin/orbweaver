#!/usr/bin/env python3
"""Nothing in CI is billable while this repository is public.

Measured 2026-08-31 against the account's own billing, not against a policy
page: in August `orbweaver` shows `grossAmount $43.93` and **`netAmount $0.00`
on every line**, and in July it does not appear at all. The same month
`werubworker` — also public — ran 145 minutes of macOS 3-core at $0.062/min and
also netted $0.00, which is this account's own evidence that a public
repository's standard runners are free whatever the SKU.

# Why this is a gate and not a sentence

**Public does not make everything free.** Two things are billed regardless of
visibility, and a single edited line starts either of them silently:

* a **larger runner** — `ubuntu-latest-4-core`, a `runs-on: [self-hosted, ...]`
  group, a `runs-on:` block naming `group:`, or a `macos-14-xlarge`. GitHub
  bills these on public repositories too.
* **Git LFS** storage and bandwidth, which visibility does not discount.

Everything else in this workflow — three standard `ubuntu-latest` jobs, the
caches, one 4 KB artifact a run — is free on a public repository, and the
billing rows above are how that is known rather than assumed.

So the check is narrow on purpose: it does not try to predict GitHub's pricing.
It asserts that **this repository has not acquired the two surfaces that bill a
public repo**, and that its runners are the standard ones the measurement was
taken over.

# It is an allow-list, and its first draft was a blocklist

The draft matched `\\d+-core|larger|group:|self-hosted` and let everything else
through. Its own negative control, run before it landed, took it apart in two
arms neither of which is exotic:

* `runs-on: ubuntu-22.04-arm64-xl` — **a larger runner with no `-core` in its
  name**. An organisation names its own larger runners, so the hazard's
  namespace is open, and *a gate over an open namespace is green on every name
  nobody thought of*.
* `runs-on: ${{ matrix.os }}` with `ubuntu-latest-8-core` in the matrix — the
  hazard moved one line away and the scan reported `all standard`.

So the set that is checked is the one that is **closed**: the standard labels
GitHub publishes and does not bill on a public repository. Anything else is
red, including an expression this scan cannot see through — *an unmeasured
check is a failure, never a pass*, and refusing an expression costs nothing
today because no job here uses one.

The blocklist is not deleted for tidiness. It is deleted because it was the
half that could go green over the change it existed to catch.

*"비용을 만들지 않는다"를 문장으로 두지 않는다. public이어도 청구되는 것이 둘
있고(larger runner, LFS), 한 줄이면 조용히 시작된다. 근거는 정책 페이지가 아니라
이 계정의 8월 명세다 — public 저장소는 macOS를 써도 net $0.00이었다.
그리고 초안은 **차단 목록**이었다: 위험의 이름공간은 열려 있어서(조직이 자기
larger runner의 이름을 짓는다) `-core`가 없는 이름 하나와 `${{ matrix.os }}` 한 줄
앞에서 초록이 되었다. 그래서 검사하는 집합은 **닫힌 쪽** — GitHub이 공표하고
public에 청구하지 않는 표준 레이블 — 이고, 그 밖은 전부 빨강이다.*
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"

#: The runner labels GitHub does not bill on a public repository — an ALLOW
#: list, for the reason in the module docstring. `macos-14-xlarge`,
#: `ubuntu-latest-4-core` and any organisation-named runner fall outside it by
#: construction rather than by being enumerated.
STANDARD = re.compile(
    r"^(?:"
    r"ubuntu-(?:latest|24\.04|22\.04)(?:-arm)?"
    r"|windows-(?:latest|2025|2022|11-arm)"
    r"|macos-(?:latest|15|14|13)(?:-intel)?"
    r")$"
)
#: A `${{ ... }}` anywhere in the value: the scan cannot see what runs.
EXPRESSION = re.compile(r"\$\{\{")


def runs_on_values(text):
    """Every `runs-on:` value in a workflow, block form included.

    A value is returned as written. `runs-on:` with an empty remainder takes
    the more-indented lines that follow it, which is how `group:` and
    `labels:` are spelled — a scan that only reads the rest of the line calls
    a runner group no runner at all.
    """
    lines = text.splitlines()
    out = []
    for i, line in enumerate(lines):
        m = re.match(r"^(\s*)runs-on:\s*(.*?)\s*$", line)
        if not m:
            continue
        indent, rest = m.group(1), m.group(2)
        if rest and not rest.startswith("#"):
            out.append(rest)
            continue
        block = []
        for nxt in lines[i + 1:]:
            if not nxt.strip():
                continue
            lead = len(nxt) - len(nxt.lstrip())
            if lead <= len(indent):
                break
            block.append(nxt.strip())
        out.append(" ".join(block) if block else "")
    return out


def labels(value):
    """The labels a `runs-on:` value names, or None if it names none we can read."""
    v = value.strip()
    if not v:
        return None
    if v.startswith("[") and v.endswith("]"):
        v = v[1:-1]
    parts = [p.strip().strip("\"'") for p in v.split(",")]
    return [p for p in parts if p]


def verdict(value):
    """`None` if this value is free on a public repository, else why it is not."""
    if EXPRESSION.search(value):
        return ("an expression this scan cannot see through; name the runner "
                "literally, or teach the scan to resolve it")
    got = labels(value)
    if got is None:
        return "a `runs-on:` naming nothing this scan could read"
    for label in got:
        if not STANDARD.match(label):
            return ("%r is not one of the standard runners the $0.00 was "
                    "measured over" % label)
    return None


def main(argv):
    if "--probe" in argv:
        return probe()

    if not WORKFLOWS.is_dir():
        print("  FAIL .github/workflows is not a directory, so this scan read")
        print("       nothing — an unmeasured check is a failure, never a pass")
        return 2

    files = sorted(WORKFLOWS.glob("*.yml")) + sorted(WORKFLOWS.glob("*.yaml"))
    if not files:
        print("  FAIL no workflow files were found; this scan measured nothing")
        return 2

    bad = []
    seen = 0
    for f in files:
        text = f.read_text()
        for value in runs_on_values(text):
            seen += 1
            why = verdict(value)
            if why is not None:
                bad.append("%s: runs-on: %s — %s" % (f.name, value or "(empty)", why))

    if seen == 0:
        print("  FAIL %d workflow file(s) declare no runner at all, so this scan"
              % len(files))
        print("       had nothing to read — unmeasured, not clean")
        return 2

    lfs = ROOT / ".gitattributes"
    if lfs.is_file() and "filter=lfs" in lfs.read_text():
        bad.append(".gitattributes tracks files with Git LFS")

    if bad:
        print("  FAIL CI has acquired a surface that bills even on a public repo:")
        for b in bad:
            print("         %s" % b)
        print("       Larger runners and Git LFS are charged regardless of")
        print("       visibility. Measured 2026-08-31, this repository billed")
        print("       $0.00 net; that stops being true here.")
        return 1

    print("  ok   %d runner declaration(s), all standard; no Git LFS — nothing here"
          % seen)
    print("       bills on a public repository (net $0.00 measured 2026-08)")
    return 0


#: Each case is (what it is, the workflow text, what the verdict must say).
#: `None` means the value must be accepted; a string must appear in the reason.
#:
#: **The reason, not merely the refusal.** The first draft of this probe asked
#: only *was it refused*, and three separate strips of the code below left it
#: green — with the allow-list gone the expression arm is still refused for
#: naming an unknown label, and with block-form reading gone the `group:` arm is
#: still refused for naming nothing at all. A control that asserts the verdict
#: and not the reason is green over every defect that gets the right answer by
#: the wrong route. Each case names the sentence it must produce.
PROBES = [
    ("a standard runner", "jobs:\n  a:\n    runs-on: ubuntu-latest\n", None),
    ("a quoted standard runner", "jobs:\n  a:\n    runs-on: 'macos-14'\n", None),
    ("a list of standard runners", "jobs:\n  a:\n    runs-on: [ubuntu-22.04]\n", None),
    ("a larger runner", "jobs:\n  a:\n    runs-on: ubuntu-latest-4-core\n",
     "'ubuntu-latest-4-core' is not one of the standard runners"),
    ("a larger runner named without `-core`",
     "jobs:\n  a:\n    runs-on: ubuntu-22.04-arm64-xl\n",
     "'ubuntu-22.04-arm64-xl' is not one of the standard runners"),
    ("a larger macOS runner", "jobs:\n  a:\n    runs-on: macos-14-xlarge\n",
     "'macos-14-xlarge' is not one of the standard runners"),
    ("self-hosted", "jobs:\n  a:\n    runs-on: [self-hosted, linux]\n",
     "'self-hosted' is not one of the standard runners"),
    ("a runner group in block form",
     "jobs:\n  a:\n    runs-on:\n      group: my-big-runners\n    steps: []\n",
     "'group: my-big-runners' is not one of the standard runners"),
    ("labels in block form",
     "jobs:\n  a:\n    runs-on:\n      labels: [self-hosted]\n    steps: []\n",
     "'labels: [self-hosted]' is not one of the standard runners"),
    ("the hazard moved into a matrix",
     "jobs:\n  a:\n    strategy:\n      matrix:\n        os: [ubuntu-latest-8-core]\n"
     "    runs-on: ${{ matrix.os }}\n",
     "an expression this scan cannot see through"),
]


def probe():
    """The scan must see each hazard, for the reason it exists to see it by.

    Silence over `ci.yml` is evidence only if these come back as written; a
    scan that has stopped recognising a shape reports the same `ok` as one
    that read a clean workflow.
    """
    for what, text, want in PROBES:
        values = runs_on_values(text)
        if len(values) != 1:
            print("  FAIL the probe %r yielded %d runs-on values, not 1, so this"
                  % (what, len(values)))
            print("       scan is not reading the shape it is being shown")
            return 2
        why = verdict(values[0])
        if want is None:
            if why is not None:
                print("  FAIL the probe %r was refused (%s), so this scan would"
                      % (what, why))
                print("       refuse a standard runner and mean nothing")
                return 2
            continue
        if why is None:
            print("  FAIL the probe %r was accepted; this scan cannot see the"
                  % what)
            print("       thing it exists to see, so its silence over ci.yml")
            print("       means nothing")
            return 2
        if want not in why:
            print("  FAIL the probe %r was refused for the wrong reason, which" % what)
            print("       means the route it exercises is gone even though the")
            print("       answer came out right:")
            print("         wanted: %s" % want)
            print("         got:    %s" % why)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
