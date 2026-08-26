#!/usr/bin/env python3
"""A report, not a gate: which public items a branch changes, and who names them.

    python3 spikes/crossing_facts.py                        # HEAD vs main, dependents in main
    python3 spikes/crossing_facts.py --branch <ref>
    python3 spikes/crossing_facts.py --branch <ref> --base <ref> --against <ref> [--against <ref>]
    python3 spikes/crossing_facts.py --worktree             # uncommitted changes vs main
    python3 spikes/crossing_facts.py --branch <ref> --against worktree

D028 §4 M2. Breaks 1, 5 and 6 of 2026-08-26 share one mechanical signature: a
**public item changed in crate A and named in crate B**, where the two batches
held disjoint footprints, no line was touched twice, `git merge-tree` reported
no conflict, and the merged tree did not compile. That signature is computable
from the diff without building anything, which is what this prints.

**Why it is a report and not a gate.** D028 §4 M2 says so, and the reason is
that "names it" is not "breaks on it": a crate that names `ChannelStats` in a
doc comment is a true hit and a false alarm. The output is for the person
commissioning batches — *"this branch changes `Server::bind`'s visibility;
N files across M compilation units name it, including 3 binary targets"* is
exactly what neither footprint list said. Exit code is 0 whether it finds
something or not. Do not wire it into `run_checks.sh` as a gate.

**A binary is its own crate root, and that is the whole point of the unit
column.** Break 6: `Server::bind` became `pub(crate)` in `orbweaver-giop`, and
`crates/orbweaver-giop/src/bin/spike_trading.rs` broke — while
`naming_server.rs`, `trading_server.rs`, `event_server.rs` and `orb.rs` in the
same crate were fine. The human sweep for break 5 excluded
`crates/orbweaver-giop/src`, which is the directory containing `src/bin`, and
reported the rule holding workspace-wide over a break sitting inside it. So
this script never groups by directory: it groups by **compilation unit**, and
`src/bin/*.rs`, `tests/`, `benches/` and `examples/` are separate crate roots
because they are.

*이진 파일은 자기 크레이트 루트다. 그래서 디렉터리가 아니라 컴파일 단위로 묶는다.*

WHAT IT DETECTS (each class is exercised by a control; see the commit message)

  visibility       `pub` → `pub(crate)`/`pub(super)`/private, and the reverse
  signature        a fn's generics, parameters, return type or where clause
  fields           a struct's or union's field set, names, types or field vis
  variants         an enum's variant set or a variant's payload
  trait items      a method, associated type or associated const added to,
                   removed from or changed in a trait others implement
  alias            a `type` alias's target
  const/static     the declared type — and, reported apart, **the value**,
                   which is break 1's cousin: nothing about the type moved and
                   every reader's expectation did, so nothing fails to compile
  attributes       `#[non_exhaustive]` added or removed, a `derive` gained or
                   lost — both of which break callers with no signature change
  re-export        a `pub use` line whose text changed
  macro            a `macro_rules!` body
  removal          a public item that is gone

WHAT IT CANNOT SEE — printed at the end of every run, not only here

  1. **Break 3's class.** Its two authorities were a shell glob
     (`differential.sh` globbing `spikes/*.idl`) and a Rust constant; the
     change that broke it was **a file's location**, not a Rust item. Nothing
     here reads a file list, so that break prints nothing. Verified, not
     assumed: running this on `bc443cb` reports no crossing fact.
  2. **Behaviour.** A merge that compiles and behaves differently. A function
     body, a `Default` impl's chosen value, an ordering — invisible.
  3. **Macros and `cfg`.** Items produced by a macro are not parsed; items
     behind `#[cfg(...)]` are read as present on every target.
  4. **Moves.** An item moved between files reads as removed-and-added. A
     `pub use` whose *target* moves while its own text does not is invisible.
  5. **Module privacy.** A `pub` item inside a private module is reported as
     public; reachability is not resolved.
  6. **Names, not resolution.** A dependent is a file that writes the name.
     A method hit through `.name(` is a guess and is labelled one; a caller
     reaching the item only through a re-export, a trait object or a glob
     import is found only if the name appears literally.
  7. **Cargo target names** are derived from paths; a `[[bin]]` block that
     renames a target is not read.
  8. Rust source is parsed by regex over a comment- and string-masked copy,
     with brace matching. It assumes rustfmt's line-anchored item headers.
  9. Two `pub use` lines starting at the same first path segment share a key
     and are told apart only by line number.

HOW THIS WAS CHECKED, AND WHAT THAT FOUND

Three historical controls, because a control that reproduces an error actually
made beats one that was invented. Each is the real pre-merge pair — the branch
as it stood, and the tree it was about to be merged into:

    python3 spikes/crossing_facts.py --branch 8cd3d10 --base 74a0d46 --against 74a0d46
    python3 spikes/crossing_facts.py --branch 22ef9fc --base 9dfaf3f --against 9dfaf3f
    python3 spikes/crossing_facts.py --branch bc443cb --base bc443cb~1 --against bc443cb~1

The first prints `ChannelStats` gaining three fields and names
`crates/orbweaver-console/src/orb.rs` — the file that failed `E0063` on the
merged tree. The second prints `Server::bind`'s narrowing and names
`bin:spike_trading` **inside orbweaver-giop itself** alongside
`bin:spike_seeded_trading` in orbweaver-test. The third — break 3 — prints
nothing, and says why: `0 of them .rs`. Limit 1 is measured, not assumed.

A noise floor, because a tool that warns about everything is a tool nobody
reads: the same edit applied to a private fn and to a `pub` one in the same
file. The private edit reports `0 public item(s) changed` over `53 + 53
item(s) parsed` — the quiet is a measurement, not an absence, which is what
the read: line exists to say.

And a probe over four claimed classes, which found **three defects in this
script**: (a) the trait class did not fire at all — a trait's header text does
not change when a method is added to its body, so `pub trait Dispatch` gaining
`fn nc_probe` reported as unchanged while every implementor in the workspace
would have stopped compiling; (b) a `pub use` was searched by the *module* its
path starts at rather than by the names it re-exports, so one changed line
matched 148 files in 11 packages; (c) a `Cargo.toml` hit was reported as a
compilation unit. A claimed class that has never been fired is documentation,
not a detector — which is why the docstring above lists no class that was not
made to print.
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Item headers, line-anchored over the masked copy. `mods` eats the qualifiers
# that may precede a keyword so that `const fn f` reads as a fn and
# `const N: u8` reads as a const — the regex backtracks between the two.
ITEM = re.compile(
    r"""(?m)^(?P<indent>[ \t]*)
        (?P<vis>pub(?:[ \t]*\([^)\n]*\))?[ \t]+)?
        (?P<mods>(?:(?:default|const|async|unsafe)[ \t]+|extern[ \t]+"[^"\n]*"[ \t]+|extern[ \t]+)*)
        (?P<kind>fn|struct|enum|union|trait|type|const|static|mod|impl|use|macro_rules!)
        (?![A-Za-z0-9_])""",
    re.X,
)
NAME = re.compile(r"[ \t]*(?:mut[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)")
ATTR = re.compile(r"^[ \t]*#\[")
# Method names too common for `.name(` attribution to mean anything.
COMMON = {"new", "main", "run", "len", "next", "get", "set", "id", "name", "kind", "value",
          "from", "into", "default", "clone", "fmt", "push", "pop", "insert", "remove",
          "add", "count", "read", "write", "start", "stop", "close", "open", "call", "send"}
OUTSIDE = "(outside crates/)"
SEARCH_RS = ["*.rs"]
SEARCH_OTHER = ["*.py", "*.sh", "*.toml", "*.md"]
EXCLUDE = [":!.claude/**", ":!target/**"]


# ---------------------------------------------------------------- git plumbing

def git(*args, ok_fail=False):
    r = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    if r.returncode != 0 and not ok_fail:
        raise RuntimeError("git %s failed: %s" % (" ".join(args), r.stderr.strip()))
    return r.stdout


def rev_parse(ref):
    out = git("rev-parse", "--verify", "--quiet", ref, ok_fail=True).strip()
    return out or None


def blob(rev, path):
    """File text at `rev`, or "" if it is absent there. `rev` may be WORKTREE."""
    if rev == "worktree":
        p = ROOT / path
        try:
            return p.read_text(errors="replace")
        except OSError:
            return ""
    r = subprocess.run(["git", "-C", str(ROOT), "show", "%s:%s" % (rev, path)],
                       capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ""


def changed_files(base, tip):
    if tip == "worktree":
        out = git("diff", "--no-renames", "--name-only", base)
        out += git("ls-files", "--others", "--exclude-standard")
    else:
        out = git("diff", "--no-renames", "--name-only", "%s...%s" % (base, tip), ok_fail=True)
        if not out:
            out = git("diff", "--no-renames", "--name-only", base, tip)
    return sorted({l for l in out.splitlines() if l.strip()})


def grep_files(rev, pattern, globs, word=False, fixed=False):
    """Files at `rev` matching `pattern`. Returns [] on no match (exit 1)."""
    cmd = ["git", "-C", str(ROOT), "grep", "-l", "-I"]
    cmd += ["-F"] if fixed else ["-E"]
    if word:
        cmd += ["-w"]
    cmd += ["-e", pattern]
    if rev != "worktree":
        cmd += [rev]
    cmd += ["--", *globs, *EXCLUDE]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode not in (0, 1):
        return None                      # could not search — reported, never silent
    out = []
    for line in r.stdout.splitlines():
        if rev != "worktree" and line.startswith(rev + ":"):
            line = line[len(rev) + 1:]
        out.append(line)
    return sorted(set(out))


# ------------------------------------------------------------------ rust scan

def mask(src):
    """A copy with comment and string *contents* blanked, newlines preserved.

    Brace counting and item matching run over this, so a `{` inside a string
    literal or a doc comment cannot move the parser.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j] == "/" and j + 1 < n and src[j + 1] == "*":
                    depth += 1
                    j += 2
                elif src[j] == "*" and j + 1 < n and src[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, min(j, n)):
                if src[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "r" and nxt in ('"', "#") and not (i and (src[i - 1].isalnum() or src[i - 1] == "_")):
            j, hashes = i + 1, 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                term = '"' + "#" * hashes
                e = src.find(term, j + 1)
                stop = n if e < 0 else e
                for k in range(j + 1, stop):
                    if src[k] != "\n":
                        out[k] = " "
                i = stop + len(term)
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    break
                j += 1
            for k in range(i + 1, min(j, n)):
                if src[k] != "\n":
                    out[k] = " "
            i = min(j, n) + 1
        elif c == "'":
            m = re.match(r"'(\\.[^'\n]*|[^'\\\n])'", src[i:])
            if m:                                  # a char literal, not a lifetime
                for k in range(i + 1, i + m.end() - 1):
                    out[k] = " "
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def match_brace(masked, p):
    depth, n = 0, len(masked)
    while p < n:
        if masked[p] == "{":
            depth += 1
        elif masked[p] == "}":
            depth -= 1
            if depth == 0:
                return p + 1
        p += 1
    return n


def extent(masked, start, kind):
    """(shape, brace_pos, end) for the item beginning at `start`."""
    if kind == "use":
        e = masked.find(";", start)
        return ("decl", -1, (e + 1) if e >= 0 else len(masked))
    p, pd, bd, n = start, 0, 0, len(masked)
    while p < n:
        c = masked[p]
        if c == "(":
            pd += 1
        elif c == ")":
            pd -= 1
        elif c == "[":
            bd += 1
        elif c == "]":
            bd -= 1
        elif c == "{":
            if pd == 0 and bd == 0:
                return ("block", p, match_brace(masked, p))
            p = match_brace(masked, p) - 1
        elif c == ";" and pd == 0 and bd == 0:
            return ("decl", -1, p + 1)
        p += 1
    return ("eof", -1, n)


def split_top(text):
    """Split at top-level commas, tracking (), [], {} and generic <>."""
    parts, depth, cur, prev = [], 0, [], ""
    for ch in text:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == "<" and prev not in ("-", "=", "<"):
            depth += 1
        elif ch == ">" and prev not in ("-", "=", ">") and depth > 0:
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
        prev = ch
    if "".join(cur).strip():
        parts.append("".join(cur))
    return [norm(p) for p in parts if norm(p)]


def norm(text):
    return " ".join(text.split())


def strip_attr_lines(text):
    return "\n".join(l for l in text.splitlines() if not ATTR.match(l))


def attrs_above(src_lines, line_idx):
    """Contiguous single-line `#[...]` attributes directly above an item."""
    got, i = [], line_idx - 1
    while i >= 0 and ATTR.match(src_lines[i]):
        got.append(norm(src_lines[i]))
        i -= 1
    return sorted(got)


def impl_display(header):
    """`impl<T> Trait for Type<T> where ...` → ("Type", "Trait for Type")."""
    h = norm(header)
    h = re.sub(r"^impl\s*(<[^>]*>)?\s*", "", h)
    h = re.split(r"\bwhere\b", h)[0].strip()
    m = re.search(r"\bfor\b(.*)$", h)
    self_ty = m.group(1).strip() if m else h
    self_ty = re.sub(r"<.*$", "", self_ty).strip()
    self_ty = self_ty.split("::")[-1].strip()
    return self_ty, h


def use_leaves(sig):
    """The names a `pub use` actually re-exports, not the module it came from.

    `pub use parse::{ParseError, parse};` re-exports two names. Searching for
    the *first* path segment instead — which is what taking the item's "name"
    does — matched 148 files across 11 packages on the first run of this
    script, which is the tool-nobody-reads failure with a plausible face.
    """
    out = []
    for seg in re.split(r"[{},]", sig):
        seg = seg.strip().rstrip(";").strip()
        if not seg or seg in ("pub use", "use"):
            continue
        seg = re.sub(r"^use\s+", "", seg)
        alias = re.search(r"\bas\s+([A-Za-z_][A-Za-z0-9_]*)", seg)
        leaf = alias.group(1) if alias else seg.split("::")[-1].strip()
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", leaf) and leaf not in ("self", "crate", "super"):
            out.append(leaf)
    return sorted(set(out))


def file_modpath(path):
    """The module path a file contributes, and its compilation unit."""
    parts = Path(path).parts
    if len(parts) >= 3 and parts[0] == "crates":
        rest = parts[2:]
        if rest[:2] == ("src", "bin"):
            return ["bin", Path(rest[-1]).stem]
        if rest[0] == "src":
            inner = [p for p in rest[1:]]
            if inner and inner[-1] in ("lib.rs", "mod.rs", "main.rs"):
                inner = inner[:-1]
            elif inner:
                inner = inner[:-1] + [Path(inner[-1]).stem]
            return inner
        return [rest[0], Path(rest[-1]).stem]
    return [Path(path).stem]


def unit_of(path):
    """(package_dir, compilation unit). `src/bin/*.rs` is its own crate root."""
    parts = Path(path).parts
    if len(parts) >= 3 and parts[0] == "crates":
        pkg, rest = parts[1], parts[2:]
        if rest[:2] == ("src", "bin"):
            return pkg, "bin:" + Path(rest[-1]).stem
        if rest[0] == "src":
            return pkg, "bin:" + pkg if rest[-1] == "main.rs" else "lib"
        if rest[0] in ("tests", "benches", "examples"):
            head = {"tests": "test", "benches": "bench", "examples": "example"}[rest[0]]
            return pkg, "%s:%s" % (head, Path(rest[1]).stem)
        if rest[0] == "build.rs":
            return pkg, "build"
        return pkg, rest[0]
    return OUTSIDE, str(Path(path).parent)


def parse(path, src):
    """{key: item} for every item in one file. Local items are dropped."""
    masked = mask(src)
    lines = src.splitlines()
    starts = [0]
    for l in src.splitlines(keepends=True):
        starts.append(starts[-1] + len(l))

    def line_of(pos):
        lo, hi = 0, len(starts) - 1
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if starts[mid] <= pos:
                lo = mid
            else:
                hi = mid - 1
        return lo

    raw = []
    for m in ITEM.finditer(masked):
        kind = m.group("kind")
        vis = norm(m.group("vis") or "")
        shape, brace, end = extent(masked, m.start(), kind)
        if kind == "impl":
            name, disp = impl_display(src[m.start():brace if brace > 0 else end])
        else:
            nm = NAME.match(masked, m.end())
            if not nm:
                continue
            name, disp = nm.group(1), nm.group(1)
        raw.append(dict(start=m.start(), end=end, brace=brace, shape=shape, kind=kind,
                        name=name, disp=disp, vis=vis, line=line_of(m.start())))

    raw.sort(key=lambda it: (it["start"], -it["end"]))
    items, stack = {}, []
    for it in raw:
        while stack and stack[-1]["end"] <= it["start"]:
            stack.pop()
        parents = list(stack)
        stack.append(it)
        if any(p["kind"] in ("fn", "macro_rules!") for p in parents):
            continue                                    # a local item
        mods = [p["name"] for p in parents if p["kind"] == "mod"]
        if any(m in ("tests", "test") for m in mods) or it["name"] in ("tests", "test") and it["kind"] == "mod":
            continue                                    # unit-test scaffolding
        holder = next((p for p in reversed(parents) if p["kind"] in ("impl", "trait")), None)
        container = holder["name"] if holder else ""
        if it["kind"] == "use" and not it["vis"].startswith("pub"):
            continue
        # Structure comes from the masked copy, so a doc comment inside a
        # struct body is not read as a field. A `const`'s *value* comes from
        # the real source, because masking would blank two different string
        # literals to the same thing and the value class would go quiet.
        body = masked[it["brace"] + 1:it["end"] - 1] if it["shape"] == "block" and it["brace"] > 0 else ""
        head = norm(masked[it["start"]:(it["brace"] if it["brace"] > 0 else it["end"])])
        head = re.sub(r"^pub(\s*\([^)]*\))?\s+", "", head).rstrip("{;").strip()

        sig, extra = head, {}
        if it["kind"] in ("struct", "union", "enum"):
            members = split_top(strip_attr_lines(body)) if body else []
            sig = "; ".join(members) if members else head
            extra["members"] = members
        elif it["kind"] in ("const", "static"):
            text = re.sub(r"^pub(\s*\([^)]*\))?\s+", "", norm(masked[it["start"]:it["end"]]))
            sig = norm(text.partition("=")[0])
            extra["value"] = norm(src[it["start"]:it["end"]].partition("=")[2]).rstrip(";")
        elif it["kind"] in ("type", "use"):
            sig = re.sub(r"^pub(\s*\([^)]*\))?\s+", "", norm(masked[it["start"]:it["end"]]))
            if it["kind"] == "use":
                extra["leaves"] = use_leaves(sig)
        elif it["kind"] == "macro_rules!":
            sig = norm(body)
        elif it["kind"] == "impl":
            sig = it["disp"]

        key = "::".join([x for x in file_modpath(path) + mods + ([container] if container else [])
                         + [it["name"]] if x])
        key = "%s %s" % (it["kind"], key)
        if key in items:
            key = "%s@%d" % (key, it["line"])
        items[key] = dict(kind=it["kind"], name=it["name"], container=container, vis=it["vis"],
                          sig=sig, attrs=attrs_above(lines, it["line"]), path=path,
                          line=it["line"] + 1, holder=holder["kind"] if holder else "",
                          **extra)

    # A trait's members are its contract. Its header text is not: adding
    # `fn nc_probe(&self);` to `pub trait Dispatch` changes nothing before the
    # `{`, so a header-only signature reports a trait as unchanged while every
    # implementor in the workspace stops compiling. Measured on this script's
    # own first run — the class was claimed in the docstring and did not fire.
    for it in items.values():
        if it["kind"] == "trait":
            it["members"] = sorted(
                c["sig"]                       # already carries its own keyword
                for c in items.values()
                if c["holder"] == "trait" and c["container"] == it["name"] and c["path"] == path)
    return items


# --------------------------------------------------------------- change model

def vis_rank(v):
    if v.startswith("pub(crate)") or v.startswith("pub(in"):
        return 2
    if v.startswith("pub(super)"):
        return 1
    if v.startswith("pub"):
        return 3
    return 0


def classify(a, b, moved_to=None):
    """Every way `a` (before) differs from `b` (after) that a namer can feel."""
    out = []
    if b is None:
        if moved_to is not None:
            out.append(("moved", "%s -> %s (same kind, container and name)"
                        % (a["path"], moved_to["path"])))
            b = moved_to
        else:
            return [("removed", "the item is gone from %s" % a["path"])]
    if vis_rank(a["vis"]) != vis_rank(b["vis"]):
        va, vb = a["vis"] or "private", b["vis"] or "private"
        word = "narrowed" if vis_rank(b["vis"]) < vis_rank(a["vis"]) else "widened"
        out.append(("visibility", "%s: %s -> %s" % (word, va, vb)))
    if a["kind"] in ("struct", "union", "enum", "trait") and a.get("members") != b.get("members"):
        was, now = a.get("members", []), b.get("members", [])
        added = [m for m in now if m not in was]
        gone = [m for m in was if m not in now]
        word = {"enum": "variant", "trait": "item"}.get(a["kind"], "field")
        bits = []
        if added:
            bits.append("%d %s(s) added: %s" % (len(added), word, ", ".join(added[:6])))
        if gone:
            bits.append("%d %s(s) removed: %s" % (len(gone), word, ", ".join(gone[:6])))
        if not bits:
            bits.append("%s set reordered or retyped" % word)
        out.append(("%ss" % word, "; ".join(bits)))
    elif a["sig"] != b["sig"]:
        if a["kind"] in ("const", "static"):
            out.append(("declared type", "%s -> %s" % (a["sig"], b["sig"])))
        elif a["kind"] == "trait":
            out.append(("trait body", "the trait's item list or a member's signature changed"))
        elif a["kind"] == "use":
            out.append(("re-export", "%s -> %s" % (a["sig"], b["sig"])))
        elif a["kind"] == "macro_rules!":
            out.append(("macro body", "the expansion changed"))
        else:
            out.append(("signature", "%s -> %s" % (a["sig"][:110], b["sig"][:110])))
    if a["kind"] in ("const", "static") and a.get("value") != b.get("value") and a["sig"] == b["sig"]:
        out.append(("value only", "%s -> %s — nothing fails to compile"
                    % (a.get("value", "")[:60], b.get("value", "")[:60])))
    if a["attrs"] != b["attrs"]:
        added = [x for x in b["attrs"] if x not in a["attrs"]]
        gone = [x for x in a["attrs"] if x not in b["attrs"]]
        bits = []
        if added:
            bits.append("gained %s" % ", ".join(added))
        if gone:
            bits.append("lost %s" % ", ".join(gone))
        out.append(("attributes", "; ".join(bits)))
    return out


# --------------------------------------------------------------- who names it

def dependents(item, against, notes, name=None, container=None):
    """Files at `against` that write this item's name, with a confidence."""
    name = item["name"] if name is None else name
    container = item["container"] if container is None else container
    strong, weak = [], []
    if container and item["holder"] == "impl":
        pat = r"%s\s*::\s*%s" % (re.escape(container), re.escape(name))
        hits = grep_files(against, pat, SEARCH_RS + SEARCH_OTHER)
        if hits is None:
            notes.append("could not search %s for %s::%s" % (against, container, name))
            hits = []
        strong = hits
        if name not in COMMON:
            m = grep_files(against, r"\.%s\s*\(" % re.escape(name), SEARCH_RS)
            holders = grep_files(against, container, SEARCH_RS, word=True)
            if m is not None and holders is not None:
                weak = sorted(set(m) & set(holders) - set(strong))
                if len(weak) > 40:
                    notes.append("`.%s(` matched %d files that also name %s — too common to "
                                 "attribute; not listed" % (name, len(weak), container))
                    weak = []
    else:
        hits = grep_files(against, name, SEARCH_RS + SEARCH_OTHER, word=True)
        if hits is None:
            notes.append("could not search %s for %s" % (against, name))
            hits = []
        strong = hits
    return strong, weak


def verdict(changes, hits_by_unit, def_pkg, def_unit):
    """What the change means for the units that name it — visibility only.

    Only units under `crates/` are reasoned about: a hit in a document or a
    script is a mention, not a compilation unit, and saying "out of reach" of
    `CHANGELOG.md` would be the report inventing a fact.
    """
    vis = next((d for k, d in changes if k == "visibility"), None)
    if not vis or "narrowed" not in vis:
        return []
    lines, out_pkg, other_unit = [], [], []
    for (pkg, unit) in sorted(hits_by_unit):
        if pkg == OUTSIDE:
            continue
        if pkg != def_pkg:
            out_pkg.append("%s [%s]" % (pkg, unit))
        elif unit != def_unit:
            other_unit.append(unit)
    if out_pkg:
        lines.append("after this change %d unit(s) in other packages can no longer see it: %s"
                     % (len(out_pkg), ", ".join(out_pkg[:8])))
    if other_unit:
        lines.append("and %d unit(s) of %s ITSELF — a binary, test, bench or example is its own "
                     "crate root, so `pub(crate)` does not reach it: %s"
                     % (len(other_unit), def_pkg, ", ".join(other_unit[:8])))
    if not out_pkg and not other_unit:
        lines.append("every unit that names it is the defining crate's own library, which "
                     "`pub(crate)` still reaches")
    return lines


# --------------------------------------------------------------------- report

def main(argv):
    ap = argparse.ArgumentParser(add_help=True, description=__doc__.splitlines()[0])
    ap.add_argument("--branch", default="HEAD")
    ap.add_argument("--base", default="main")
    ap.add_argument("--against", action="append", default=[],
                    help="tree(s) to search for dependents; 'worktree' allowed (default: --base)")
    ap.add_argument("--worktree", action="store_true", help="analyse uncommitted changes")
    ap.add_argument("--all", action="store_true",
                    help="also list changed items that reach nobody outside their own unit")
    args = ap.parse_args(argv[1:])

    tip = "worktree" if args.worktree else args.branch
    base_sha = rev_parse(args.base)
    if base_sha is None:
        print("  FAIL --base %s does not resolve" % args.base)
        return 0
    if tip == "worktree":
        base = args.base
    else:
        mb = git("merge-base", args.base, tip, ok_fail=True).strip()
        base = mb or args.base
    against = args.against or [args.base]

    files = changed_files(base, tip)
    rs = [f for f in files if f.endswith(".rs")]
    print("crossing_facts — a report, not a gate (D028 §4 M2)")
    print("  branch   %s%s" % (tip, "" if tip == "worktree" else
                               "  (%s)" % git("log", "-1", "--format=%s", tip).strip()[:70]))
    print("  base     %s  (%s)" % (base[:12], args.base))
    print("  against  %s" % ", ".join(against))
    print()

    before, after, unparsed = {}, {}, []
    for f in rs:
        for rev, store in ((base, before), (tip, after)):
            text = blob(rev, f)
            if not text:
                continue
            try:
                store.update(parse(f, text))
            except Exception as exc:                    # never silent
                unparsed.append("%s at %s: %s" % (f, rev[:12], exc))

    keys = sorted(set(before) | set(after))
    public = [k for k in keys
              if (k in before and vis_rank(before[k]["vis"]) > 0)
              or (k in after and vis_rank(after[k]["vis"]) > 0)]
    # An item that changed file reads as removed-and-added unless it is matched
    # up again by (kind, container, name). That is limit 4 mitigated, not
    # closed: a genuine delete plus a same-named addition elsewhere reads as
    # a move, and is labelled one.
    landing = {}
    for k, v in after.items():
        landing.setdefault((v["kind"], v["container"], v["name"]), []).append(v)
    changed, added_new, moves = [], 0, 0
    for k in public:
        a, b = before.get(k), after.get(k)
        if a is None:
            added_new += 1
            continue
        moved_to = None
        if b is None:
            cands = [c for c in landing.get((a["kind"], a["container"], a["name"]), [])
                     if c["path"] != a["path"]]
            moved_to = cands[0] if len(cands) == 1 else None
            if moved_to is not None:
                moves += 1
        cs = classify(a, b, moved_to)
        if cs:
            changed.append((k, a, b or moved_to or a, cs))

    notes, reported, silent = [], 0, []
    for key, a, b, cs in changed:
        item = b or a
        def_pkg, def_unit = unit_of(item["path"])
        # A `pub use` re-exports names; the item's own "name" is the module the
        # path starts at, and searching for that matches the world. Search the
        # leaves that were added or removed instead.
        probes = [(item["container"], item["name"])]
        if item["kind"] == "use":
            was, now = (a.get("leaves") or []), (b.get("leaves") or [])
            moved = [l for l in now if l not in was] + [l for l in was if l not in now]
            probes = [("", l) for l in moved if l not in COMMON]
            if not moved:
                notes.append("`%s` changed but re-exports the same names; nobody was searched"
                             % item["sig"][:60])
            for l in moved:
                if l in COMMON:
                    notes.append("re-exported name `%s` is too common to attribute; not searched"
                                 % l)
        rows = []
        for rev in against:
            for cont, nm in probes:
                strong, weak = dependents(item, rev, notes, name=nm, container=cont)
                for f in strong:
                    rows.append((rev, f, "names %s" % nm))
                for f in weak:
                    rows.append((rev, f, "`.%s(`" % nm))
        rows_rs = [r for r in rows if r[1].endswith(".rs")]
        non_rs = sorted({f for _, f, _ in rows if not f.endswith(".rs")})
        own = sorted({f for _, f, _ in rows_rs if unit_of(f) == (def_pkg, def_unit)})
        outside = [r for r in rows_rs if unit_of(r[1]) != (def_pkg, def_unit)]
        if not outside and not args.all:
            silent.append(key)
            continue
        reported += 1
        label = "%s::%s" % (item["container"], item["name"]) if item["container"] else item["name"]
        print("%s  %s  (%s, %s:%d)" % (def_pkg, label, item["kind"], item["path"], item["line"]))
        for kind, detail in cs:
            if len(detail) > 190:
                detail = detail[:190] + " …"
            print("    %-12s %s" % (kind, detail))
        by_unit = {}
        for rev, f, why in outside:
            by_unit.setdefault(unit_of(f), []).append((rev, f, why))
        units = {u: h for u, h in by_unit.items() if u[0] != OUTSIDE}
        elsewhere = sorted({f for u, h in by_unit.items() if u[0] == OUTSIDE
                            for _, f, _ in h} | set(non_rs))
        print("    named by     %d file(s) in %d compilation unit(s) across %d package(s)"
              % (len({f for u, h in units.items() for _, f, _ in h}), len(units),
                 len({p for p, _ in units})))
        roots = [u for u in units if u[1] != "lib"]
        if roots:
            print("    !            %d of those unit(s) are separate crate roots (bin/test/bench/"
                  "example), which `pub(crate)` does not reach" % len(roots))
        for (pkg, unit), hits in sorted(units.items()):
            names = sorted({f for _, f, _ in hits})
            shown = ", ".join(names)
            if len(shown) > 130:
                shown = ", ".join(names[:2]) + " … (%d file(s))" % len(names)
            weak_only = all(w.startswith("`.") for _, _, w in hits)
            print("      %s %-30s %s%s" % ("!" if unit != "lib" else " ",
                                           "%s [%s]" % (pkg, unit), shown,
                                           "   [method-name guess]" if weak_only else ""))
        for line in verdict(cs, set(by_unit), def_pkg, def_unit):
            print("    =>           %s" % line)
        if own:
            print("    own unit     %d file(s) in %s [%s] also name it — the defining batch's own "
                  "footprint" % (len(own), def_pkg, def_unit))
        if elsewhere:
            shown = ", ".join(elsewhere[:5]) + (" …" if len(elsewhere) > 5 else "")
            print("    elsewhere    %d mention(s) outside crates/ (documents, scripts, spike "
                  "sources): %s" % (len(elsewhere), shown))
        print()

    print("  read: %d changed file(s), %d of them .rs; %d + %d item(s) parsed on the two sides; "
          "%d public item(s) changed (%d of them a move between files); %d reported, %d reach "
          "nobody outside their own compilation unit, %d newly added and named by nobody yet"
          % (len(files), len(rs), len(before), len(after), len(changed), moves, reported,
             len(silent), added_new))
    if silent and not args.all:
        print("        not listed (no dependent outside their own unit): %s%s"
              % (", ".join(s.split("::")[-1] for s in silent[:8]),
                 " …" if len(silent) > 8 else ""))
    for u in unparsed:
        print("  COULD NOT PARSE %s" % u)
    for n in dict.fromkeys(notes):
        print("  COULD NOT DETERMINE %s" % n)
    if not rs:
        print("  No .rs file changed on this branch: this report has nothing to say about it,"
              " which is not the same as the branch being safe to merge.")
    print()
    print("  This is a report. Naming an item is not breaking on it — a doc comment that"
          " mentions a struct is a true hit and a false alarm. Read the units, not the count.")
    print("  What it cannot see, every run: (1) an authority that is not a Rust item — break 3's"
          " shell glob against a Rust constant prints nothing here; (2) behaviour that changes"
          " with no signature change; (3) macro-generated and `cfg`-gated items; (4) a move,"
          " which reads as removed-and-added, and a `pub use` whose target moves under it;"
          " (5) module privacy — `pub` inside a private `mod` reads as public; (6) resolution —"
          " a caller reaching the item through a re-export, trait object or glob import is found"
          " only if the name appears literally; (7) `[[bin]]` renames — target names come from"
          " paths; (8) Rust parsed by regex over a comment- and string-masked copy.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
