#!/usr/bin/env python3
"""How many servers in this tree can be asked to stop, and how many cannot.

`Server::serve(dispatch, stop)` runs until `stop()` answers true. A site that
passes `|| false` has a server **nothing can stop**: not the caller, not a
signal handler, not a test that wants it back. D029 §6.1's Lifecycle cell has
carried a count of those since 2026-08-27 —

    Also unchanged: 17 of this workspace's 63 serve sites pass `|| false`
    — fixable rather than fixed.

— and **nothing computed it**. It was typed once and quoted since, which is the
class this project calls *a floor is not a figure*: a number in prose drifts in
silence while every reader takes it for today's measurement. A crude grep on
2026-08-29 answered 28 of 39, which measures the grep and not the tree, and that
is exactly the trouble with a figure whose method was never written down.

This is the method, written down and runnable.

# What it counts

A **serve site** is a call to `.serve(` or `.serve_shared(`. Its **stop
argument** is the second one, taken by walking balanced parentheses rather than
by a regex, because a stop predicate routinely contains commas and parentheses
of its own (`move || flag.load(Ordering::SeqCst)`).

A site is **unstoppable** when that argument is spelled as a constantly-false
closure: `|| false`, `move || false`, or the same with whitespace.

# What it cannot see, and this is not a caveat but the shape of the answer

It classifies **spellings, not behaviour**. `server.serve(&mut Ping, stop)`
passes a variable; whether that variable is a closure that never answers true is
a question about the enclosing function and this does not ask it. So the count
of unstoppable sites is a **lower bound**, and a site that became unstoppable by
being handed a constant through a binding is invisible here. Said rather than
left for a reader to assume the number is tight.

It is a **report, not a gate**. There is no defensible number for how many
servers may be unstoppable — a test fixture that serves for the length of one
assertion has no use for a stop predicate, and refusing it would be a rule about
tests wearing a rule about lifecycles. The same reason `entry_cost.py` and
`plan_numbers.py` report and do not gate.

*그 수를 계산하는 것이 없었다. 한 번 타이핑되고 이후 인용되었을 뿐이며, 그것이
이 프로젝트가 "하한은 수치가 아니다"라 부르는 부류다. 이것이 그 방법이다. 이것은
**철자**를 분류하지 행동을 분류하지 않으므로 개수는 하한이며, 게이트가 아니라
보고다 — 서버 몇 개까지 멈출 수 없어도 되는지에 방어 가능한 수는 없다.*
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CALL = re.compile(r"\.serve(?:_shared)?\s*\(")
#: A closure that is constantly false, however it is spelled.
NEVER_STOPS = re.compile(r"^\s*(?:move\s*)?\|\s*\|\s*false\s*$")


def code_only(src):
    """`src` with comments, strings and char literals blanked to spaces.

    **Without this the count is nonsense, and the first run proved it**: 513
    sites and 150 unstoppable, against a hand-typed 63 and 17. This repository's
    doc comments quote `server.serve(&mut d, || false)` dozens of times while
    explaining why a stop predicate matters, and every one of those was being
    counted as a server. Blanking rather than deleting keeps every byte offset,
    so a line number computed afterwards is still the line in the file.

    Raw strings (`r"..."`, `r#"..."#`) are handled because this tree has them;
    a lifetime (`'a`) is not a char literal and must not open one.

    *이것이 없으면 개수는 무의미하다 — 첫 실행이 증명했다. 주석이 예시로 적어둔
    `serve(..., || false)`가 전부 서버로 세어지고 있었다.*
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "r" and i + 1 < n and src[i + 1] in '"#':
            k = i + 1
            hashes = 0
            while k < n and src[k] == "#":
                hashes += 1
                k += 1
            if k < n and src[k] == '"':
                close = '"' + "#" * hashes
                j = src.find(close, k + 1)
                j = n if j < 0 else j + len(close)
                for q in range(i, j):
                    if out[q] != "\n":
                        out[q] = " "
                i = j
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "'" and i + 2 < n and (src[i + 2] == "'" or src[i + 1] == "\\"):
            j = src.find("'", i + 1)
            j = i + 1 if j < 0 else j + 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def _args(text, open_paren):
    """The argument list at `open_paren`, split on top-level commas."""
    depth, out, cur, i = 0, [], [], open_paren
    while i < len(text):
        ch = text[i]
        if ch in "([{":
            depth += 1
            if depth == 1:
                i += 1
                continue
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                out.append("".join(cur))
                return out
        elif ch == "," and depth == 1:
            out.append("".join(cur))
            cur = []
            i += 1
            continue
        if depth >= 1:
            cur.append(ch)
        i += 1
    return None  # unbalanced: the file is not what this expects


def sites(text):
    """Every serve site in `text`, as (line, stop-argument-or-None)."""
    for m in CALL.finditer(text):
        args = _args(text, m.end() - 1)
        if args is None or len(args) < 2:
            yield text.count("\n", 0, m.start()) + 1, None
            continue
        yield text.count("\n", 0, m.start()) + 1, args[1]


def tracked_rs(root):
    """Every tracked `*.rs`, from `git ls-files` and not from a directory walk.

    **A walk sees the agent worktrees.** `.claude/worktrees/` holds eight full
    copies of this repository, so the first version of this script answered
    **509 sites** where the tree has a fraction of that — roughly the real
    number times eight, which is what a count of *this repository plus every
    checkout of it* looks like. CLAUDE.md already says a sweep runs over
    `git ls-files` rather than a directory walk, for exactly this; it was
    written after a sweep measured a file instead of a rule, and this is the
    same lesson arriving from the other side.

    *순회는 에이전트 워크트리를 본다 — 저장소의 전체 사본 여덟 개다. 첫 판은 509를
    냈고, 그것은 "이 저장소 더하기 그 모든 체크아웃"의 모습이다.*
    """
    import subprocess

    try:
        out = subprocess.run(
            ["git", "-C", str(root), "ls-files", "*.rs"],
            capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return [root / line for line in out.splitlines() if line]


def scan(root):
    total, unstoppable, unreadable = [], [], []
    files = tracked_rs(root)
    if files is None:
        return None, None, None
    for p in sorted(files):
        rel = p.relative_to(root).as_posix()
        try:
            text = p.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        for line, stop in sites(code_only(text)):
            where = f"{rel}:{line}"
            total.append(where)
            if stop is None:
                unreadable.append(where)
            elif NEVER_STOPS.match(stop):
                unstoppable.append(where)
    return total, unstoppable, unreadable


#: Two sites with a known answer, so silence over the tree means something.
PROBE = """
fn a() { server.serve(&mut d, || false).unwrap(); }
fn b() { server.serve_shared(&svc, move || flag.load(Ordering::SeqCst)).unwrap(); }
"""


def main(argv):
    found = list(sites(PROBE))
    stops = [s for _, s in found if s is not None and NEVER_STOPS.match(s)]
    if len(found) != 2 or len(stops) != 1:
        print("  FAIL the probe's two synthetic sites did not come back as one")
        print("       unstoppable and one stoppable — this scan is not reading")
        print("       serve sites, so its count over the tree means nothing")
        return 2
    if "--probe" in argv:
        return 0

    total, unstoppable, unreadable = scan(ROOT)
    if total is None:
        print("  FAIL `git ls-files` could not be run, so this scan does not know")
        print("       which files are the repository's — and a directory walk here")
        print("       counts every agent worktree as another copy of the tree")
        return 2
    print("  %d serve site(s); %d pass a constantly-false stop, so nothing can "
          "stop them" % (len(total), len(unstoppable)))
    if unreadable:
        print("  %d site(s) whose argument list could not be walked: %s"
              % (len(unreadable), ", ".join(unreadable[:4])))
    if "--list" in argv:
        for w in unstoppable:
            print("    unstoppable  %s" % w)
    print("  A lower bound: a stop handed in through a binding reads as "
          "stoppable here. A report, not a gate.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
