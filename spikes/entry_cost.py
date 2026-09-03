#!/usr/bin/env python3
"""A report, not a gate: what a newcomer has to know to serve an object and to
make a call, taken from the tree rather than asserted.

    python3 spikes/entry_cost.py              # the whole report
    python3 spikes/entry_cost.py --json       # the same figures, machine-readable
    python3 spikes/entry_cost.py --no-cargo   # skip the doctest run; say so

Why a report, and why it exists. D027 §5 E4: *"because 'easier' without a
number is the one claim this project would let through unmeasured."* §1 of that
document counted 324 public functions in `orbweaver-giop` beside zero compiled
examples and observed that nobody had decided that — it accumulated, and
nothing counted it until 2026-08-26. This is the counting. It follows
`spikes/gap_symbols.py` and `spikes/plan_numbers.py`: it exits 0 whatever it
finds, it prints what it could not measure, and it decides nothing. D027 §7 is
explicit that the surface may be the right size; this file counts it and does
not judge it, and its output should not be read as a complaint.

**The hard part is what counts as the shortest path, so this prints four
numbers and not one.** A spike is 483 lines because it prints evidence, parses
five environment variables and drives a fixture; none of that is the cost of
serving an object, and a single "lines" figure hides which part is which. The
four, per program:

  total     every line of the file, D027 §1's figure. Captures the whole
            artifact; captures the module doc and the unit tests too.
  code      non-blank, non-comment lines outside `#[cfg(test)]`. Captures what
            a reader must read; still counts argument parsing and printing.
  api       code lines that name something from an `orbweaver-*` crate.
            Captures the part that is about this ORB. Under-counts: a `match`
            arm on a variant of an ORB enum, or a line operating on a value an
            earlier line produced, names nothing and is not counted.
  named     distinct public items of the `orbweaver-*` crates the program
            names. This is the figure that answers "what does a newcomer have
            to know", because eleven imports is a cost a line count hides.

And a fifth, which is the one worth watching:

  floor     the items named by **every** program in the tree that does that
            job — the obligation set. No program avoids these, so they are the
            entry cost proper, and they are computed from an intersection
            across independently written programs rather than from anyone's
            opinion about which spike is representative. A spike's private
            baggage falls out of an intersection on its own.

**What `named` does not capture**, said plainly because a number whose
definition lives in a commit message is the drift this project keeps finding:

  - It counts *names*, not difficulty. `Server::bind` and `Ior::parse` count
    one each; one of them can fail eleven ways and one cannot.
  - It cannot see an item reached without naming it — a trait method called on
    a value whose type is inferred, a `?` that converts an error type, a
    `Default::default()` that constructs an ORB type. Those are real knowledge
    and are invisible here.
  - Receiver calls (`x.resolve(..)`) are attributed **by method name**, so a
    method that shares a name with a `std` method is over-attributed. The
    report prints how many attributions were made that way and lists the
    colliding names, rather than quietly filtering them: a check tuned until it
    is quiet is the failure mode `spikes/bilingual_drift.py` was killed for.
  - It is a count of items, not of crates. Ten items from one crate and ten
    from four are the same number and are not the same cost, so the crate
    spread is printed beside it.

**How it survives D019 step 4.** That change makes `Orb` the only route to a
transport and a root POA, retires `Server::bind` and `Poa::new` as the public
way in, and migrates thirteen call sites — that is, it changes exactly what
this measures. So nothing here is keyed to a call site. A program's job is
decided by ANCHORS below, each anchor is checked against the public surface
before it is used, and **an anchor that no longer names a public item, or that
matches no program, is printed as STALE rather than reported as a zero.** A
report that cannot tell "nobody serves objects" from "the API moved" is the
green-while-measuring-nothing class with better manners. The floor is the
figure whose movement across step 4 is the interesting quantity: items enter
and leave it, and E1's doctest is judged against it.

Exit code is 0 whenever the tree parsed. Not a gate. `run_checks.sh` does not
run it and should not without someone first arguing what its red would mean —
there is no defensible threshold for "too many items to learn", which is the
same reason `gap_symbols.py` stayed a report.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"

# --------------------------------------------------------------------------
# The role anchors.
#
# A program's job is decided by what it names, not by its filename: a list of
# filenames is exactly the "encodes today's call sites" failure D027 warns E4
# about. Each anchor is a public item whose presence in a program means the
# program does that job, with the reason it means that. Anchors are validated
# against the measured public surface before use, so an anchor the API has
# retired is reported rather than silently contributing nothing.
# --------------------------------------------------------------------------
ANCHORS = {
    "serve": [
        ("Dispatch", "implementing the serving trait is what makes a program a servant"),
        ("Server", "the bind/accept/dispatch loop"),
        ("Poa", "the object adapter a servant is activated in"),
        ("ObjectOps", "the _is_a/_non_existent half every servant has to answer"),
        ("NamingServer", "a servant for CosNaming is still a servant"),
        ("EventChannelServer", "likewise for the event service"),
    ],
    "call": [
        ("Connection", "the client transport: a program holding one is calling out"),
        ("invoke", "the request-send call, under any of its spellings"),
        ("invoke_nullary", "the same, for the no-argument shape"),
        ("string_to_object", "CORBA 3.4 §8.2.2, from a stringified IOR to a reference"),
        ("NamingContext", "resolving a name is done in order to call what it names"),
        ("Guarded", "the guarded client wrapper"),
    ],
}

# Doc fence tags that rustdoc does *not* compile. Counted separately because
# D027 §1 reported "~14 doctests across the workspace" and the workspace has
# two: the other fences are `text`, which is prose in a box. Named here rather
# than inferred, so the list is arguable.
UNCOMPILED_FENCE_TAGS = {"text", "ignore", "console", "idl", "json", "sh", "bash", "toml", "tsv"}


# --------------------------------------------------------------------------
# Lexical helpers
# --------------------------------------------------------------------------
def strip_rust(src):
    """Blank out comments, string and char literals, keeping line structure.

    Brace counting and identifier scanning both lie in the presence of a `{`
    inside a doc comment or a `"}"` in a string, and this crate's sources have
    both. Returns text of the same line count with those spans replaced by
    spaces, so every line number below is the real one.
    """
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
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
            out.append("".join(ch if ch == "\n" else " " for ch in src[i:j]))
            i = j
        elif c == "r" and i + 1 < n and src[i + 1] in '#"':
            j = i + 1
            hashes = 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                close = '"' + "#" * hashes
                k = src.find(close, j + 1)
                k = n if k < 0 else k + len(close)
                out.append("".join(ch if ch == "\n" else " " for ch in src[i:k]))
                i = k
            else:
                out.append(c)
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
            out.append("".join(ch if ch == "\n" else " " for ch in src[i:j]))
            i = j
        elif c == "'":
            # A char literal, or a lifetime. `'a` followed by a non-quote is a
            # lifetime and must be left alone.
            m = re.match(r"'(?:\\.|[^\\'])'", src[i:])
            if m:
                out.append(" " * m.end())
                i += m.end()
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


IDENT = r"[A-Za-z_][A-Za-z0-9_]*"


# --------------------------------------------------------------------------
# Public surface
# --------------------------------------------------------------------------
def module_path_of(rel):
    """`src/foo/bar.rs` -> ('foo', 'bar'); `src/lib.rs` -> ()."""
    parts = list(rel.parts)
    assert parts[0] == "src"
    parts = parts[1:]
    if parts[-1] in ("lib.rs", "mod.rs"):
        parts = parts[:-1]
    else:
        parts[-1] = parts[-1][:-3]
    return tuple(parts)


def public_module_paths(crate_dir):
    """Module paths reachable from the crate root through `pub mod` only."""
    declared = {}  # parent path -> {name: is_pub}
    for f in sorted(crate_dir.glob("src/**/*.rs")):
        rel = f.relative_to(crate_dir)
        if rel.parts[1:2] == ("bin",):
            continue
        here = module_path_of(rel)
        text = strip_rust(f.read_text(errors="replace"))
        d = declared.setdefault(here, {})
        for m in re.finditer(r"(?m)^\s*(pub(?:\s*\([^)]*\))?\s+)?mod\s+(" + IDENT + r")\s*[;{]", text):
            vis, name = m.group(1) or "", m.group(2)
            d[name] = vis.strip().startswith("pub") and "(crate)" not in vis and "(super)" not in vis
    reachable = {()}
    frontier = [()]
    while frontier:
        cur = frontier.pop()
        for name, is_pub in declared.get(cur, {}).items():
            if is_pub and (cur + (name,)) not in reachable:
                reachable.add(cur + (name,))
                frontier.append(cur + (name,))
    return reachable


TYPE_KW = ("struct", "enum", "trait", "type", "union")


def scan_items(crate, crate_dir, reachable, notes):
    """Public items of one crate: a list of dicts.

    Counting rule, stated because every alternative rule gives a different
    number and the number is the point:
      - a *type* is a `pub struct|enum|trait|type|union` in a publicly
        reachable module;
      - a *function* is a `pub fn` in a publicly reachable module, a `pub fn`
        in an inherent `impl` of a public type, or a method **signature**
        declared in a `pub trait` (a caller has to know it to implement or to
        call it);
      - methods of a `impl Trait for Type` block are **not** counted: they add
        no name a caller has not already met on the trait. Their count is
        printed as a note, because a different rule would report a bigger
        surface and someone will want to know by how much;
      - `pub const` and `pub static` are counted apart from both;
      - anything under `#[cfg(test)]`, and any item inside a function body, is
        out.
    """
    items = []
    trait_impl_fns = 0
    for f in sorted(crate_dir.glob("src/**/*.rs")):
        rel = f.relative_to(crate_dir)
        if rel.parts[1:2] == ("bin",):
            continue
        here = module_path_of(rel)
        if here not in reachable:
            notes.append("%s: module `%s` is not `pub mod` from the crate root; its items are "
                         "not counted" % (crate, "::".join(here) or "crate"))
            continue
        text = strip_rust(f.read_text(errors="replace"))
        items.extend(_scan_file(crate, here, text, rel))
        trait_impl_fns += _count_trait_impl_fns(text)
    return items, trait_impl_fns


def _scan_file(crate, base, text, rel):
    """Walk one file's brace structure, emitting public items."""
    found = []
    stack = []          # (kind, name, public, brace_depth_at_open)
    depth = 0
    i, n = 0, len(text)
    pending_cfg_test = False
    line = 1

    def mod_public():
        return all(s[2] for s in stack if s[0] == "mod")

    def in_fn():
        return any(s[0] == "fn" for s in stack)

    def cur_mod():
        return base + tuple(s[1] for s in stack if s[0] == "mod")

    while i < n:
        ch = text[i]
        if ch == "\n":
            line += 1
            i += 1
            continue
        if ch == "}":
            depth -= 1
            while stack and stack[-1][3] == depth:
                stack.pop()
            i += 1
            continue
        if ch == "{":
            depth += 1
            i += 1
            continue
        if ch == "#" and text.startswith("#[cfg(test)]", i):
            pending_cfg_test = True
            i += len("#[cfg(test)]")
            continue
        m = re.match(
            r"(pub(?:\s*\((?P<scope>[^)]*)\))?\s+)?"
            r"(?P<kw>mod|struct|enum|trait|union|impl|fn|const|static|type)\b",
            text[i:],
        )
        if not m or (i > 0 and re.match(r"[A-Za-z0-9_:]", text[i - 1])):
            i += 1
            continue
        vis = m.group(1) or ""
        scope = m.group("scope")
        is_pub = vis.strip().startswith("pub") and scope in (None, "")
        kw = m.group("kw")
        rest = text[i + m.end():]
        name_m = re.match(r"\s*(" + IDENT + r")", rest)
        name = name_m.group(1) if name_m else "?"

        if pending_cfg_test:
            # Skip the whole item: jump past its body or its `;`.
            pending_cfg_test = False
            i = _skip_item(text, i + m.end())
            continue

        if in_fn():
            i += m.end()
            continue

        if kw == "mod":
            stack.append(("mod", name, is_pub, depth))
            i += m.end()
            continue
        if kw == "impl":
            # `impl<..> Trait for Type {` or `impl<..> Type {`
            head = rest.split("{", 1)[0]
            is_trait_impl = re.search(r"\bfor\b", head) is not None
            target = _impl_target(head)
            stack.append(("impl", target, not is_trait_impl, depth))
            i += m.end()
            continue
        if kw == "trait":
            if is_pub and mod_public():
                found.append(dict(crate=crate, module="::".join(cur_mod()), name=name,
                                  kind="type", sub="trait", file=str(rel), line=line))
            stack.append(("trait", name, is_pub and mod_public(), depth))
            i += m.end()
            continue
        if kw == "fn":
            owner = stack[-1] if stack else None
            visible = False
            qual = "::".join(cur_mod())
            if owner and owner[0] == "impl":
                visible = is_pub and owner[2] and mod_public()
                qual = owner[1]
            elif owner and owner[0] == "trait":
                visible = owner[2]          # trait methods need no `pub`
                qual = owner[1]
            else:
                visible = is_pub and mod_public()
            if visible:
                # Whether it takes a receiver. `.foo()` in a caller can only be
                # a function that does, and `Ior::parse` does not — without
                # this, `"1.2".parse()` was attributed to `Ior::parse` and the
                # client-side counts were nonsense.
                after = rest[name_m.end():] if name_m else ""
                open_paren = after.find("(")
                params = after[open_paren + 1:open_paren + 80] if open_paren >= 0 else ""
                takes_self = bool(re.match(r"\s*(&\s*(?:'\w+\s*)?(?:mut\s+)?|mut\s+)?self\b",
                                           params))
                found.append(dict(crate=crate, module=qual, name=name, kind="fn",
                                  sub=("method" if owner and owner[0] in ("impl", "trait")
                                       else "free"),
                                  takes_self=takes_self,
                                  file=str(rel), line=line))
            stack.append(("fn", name, visible, depth))
            i += m.end()
            continue
        if kw in TYPE_KW:
            if is_pub and mod_public() and not (stack and stack[-1][0] == "impl"):
                found.append(dict(crate=crate, module="::".join(cur_mod()), name=name,
                                  kind="type", sub=kw, file=str(rel), line=line))
            i += m.end()
            continue
        if kw in ("const", "static"):
            if is_pub and mod_public():
                found.append(dict(crate=crate, module="::".join(cur_mod()), name=name,
                                  kind="const", sub=kw, file=str(rel), line=line))
            i += m.end()
            continue
        i += m.end()
    return found


def _impl_target(head):
    head = re.sub(r"^\s*<.*?>\s*", "", head)
    if re.search(r"\bfor\b", head):
        head = head.split(" for ", 1)[1]
    head = head.split("where")[0]
    m = re.search(r"(" + IDENT + r")\s*(?:<|$|\s)", head.strip())
    return m.group(1) if m else "?"


def _count_trait_impl_fns(text):
    n = 0
    for m in re.finditer(r"(?m)^\s*impl\b[^{;]*\bfor\b[^{;]*\{", text):
        body, depth, i = [], 0, m.end() - 1
        while i < len(text):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            body.append(text[i])
            i += 1
        n += len(re.findall(r"(?m)^\s*(?:pub\s+)?(?:async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s",
                            "".join(body)))
    return n


def _skip_item(text, i):
    """From just after an item keyword, skip to the end of the item."""
    depth = 0
    while i < len(text):
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        elif c == ";" and depth == 0:
            return i + 1
        i += 1
    return i


# --------------------------------------------------------------------------
# Doc fences and doctests
# --------------------------------------------------------------------------
def fence_census(crate_dir):
    """Fenced blocks in doc comments, by tag. A `text` fence is prose in a box."""
    tags = {}
    for f in sorted(crate_dir.glob("src/**/*.rs")):
        open_tag = None
        for raw in f.read_text(errors="replace").splitlines():
            s = raw.strip()
            if not (s.startswith("///") or s.startswith("//!")):
                continue
            body = s[3:].strip()
            if not body.startswith("```"):
                continue
            if open_tag is None:
                tag = body[3:].strip() or "rust"
                open_tag = tag
                tags[tag] = tags.get(tag, 0) + 1
            else:
                open_tag = None
    return tags


def doctest_counts():
    """What `cargo test --doc --workspace` actually runs, per crate.

    Asked of rustdoc rather than counted from the source, because the question
    "is this fence compiled" has exactly one authority and it is not a regex.
    Returns (counts, error): `error` non-empty means unmeasured, not zero.
    """
    # `stderr=STDOUT`, not `capture_output` + concatenation: cargo prints the
    # `Doc-tests <crate>` header on **stderr** and the `test result:` line on
    # **stdout**, so joining the two streams after the fact loses the
    # interleaving and every crate reads as zero. It did — and zero is the
    # answer this report was half expecting, which is exactly when a parsing
    # defect survives review.
    try:
        r = subprocess.run(["cargo", "test", "--doc", "--workspace"],
                           cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                           text=True, timeout=1800)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {}, "cargo could not be run: %s" % e
    out = r.stdout
    counts, cur = {}, None
    for lineno, line in enumerate(out.splitlines()):
        m = re.match(r"\s*Doc-tests (\S+)", line)
        if m:
            cur = m.group(1).replace("_", "-")
            counts.setdefault(cur, 0)
            continue
        m = re.match(r"test result: \w+\. (\d+) passed; (\d+) failed", line)
        if m and cur:
            counts[cur] += int(m.group(1)) + int(m.group(2))
    if not counts:
        return {}, ("`cargo test --doc --workspace` produced no `Doc-tests` line "
                    "(exit %d) — the workspace may not build" % r.returncode)
    return counts, ""


# --------------------------------------------------------------------------
# Programs and their entry cost
# --------------------------------------------------------------------------
GENERATED_MARKER = ("//! Generated by orbweaver-gen", "not yet read")


def generated_marker():
    """The header `orbweaver-gen` writes, read from the generator.

    CLAUDE.md: a classifier that decides which class a thing belongs to by
    matching a retyped substring of a sentence another function owns fails
    silently when that sentence changes for a good reason. Python cannot call
    `orbweaver_gen`, so it does the next thing: it reads the literal out of the
    `writeln!` that emits it, and says loudly when it could not.
    """
    lib = CRATES / "orbweaver-gen" / "src" / "lib.rs"
    if lib.is_file():
        m = re.search(r'writeln!\(\s*\w+\s*,\s*"(//! Generated by[^"]*)"',
                      lib.read_text(errors="replace"))
        if m:
            return m.group(1), "read from crates/orbweaver-gen/src/lib.rs"
    return ("//! Generated by orbweaver-gen",
            "FALLBACK, retyped — the literal could not be read out of "
            "orbweaver-gen; if the generator's header changed, generated files "
            "are being counted here as hand-written")


def programs():
    """Every self-contained Rust program in the tree, with how it got here.

    Binaries and examples are the headline: they are what a person writes.
    Integration tests are scanned too and reported apart — if the shortest
    serving program in the tree is a 40-line test, saying so is the honest
    answer and hiding it is not. `spikes/**/*.rs` are included because four of
    them are servants and clients that no crate holds.
    """
    out = []
    for f in sorted(CRATES.glob("*/src/bin/*.rs")):
        out.append((f, "bin"))
    for f in sorted(CRATES.glob("*/examples/*.rs")):
        out.append((f, "example"))
    # Tracked files, not a directory walk: `spikes/` holds ignored fixture
    # builds (`spikes/tao/ACE_wrappers/`, `spikes/tls/omniORBpy/`) and a walk
    # reads them as ours. This one survived 2026-09-03 only because omniORBpy
    # carries no `.rs`; `leaves_cleanly.py`'s identical walk over `*.py` did not.
    # Swept as one rule rather than one file.
    tracked = subprocess.run(
        ["git", "ls-files", "spikes/*.rs", "spikes/**/*.rs"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    for f in sorted(ROOT / t for t in tracked):
        out.append((f, "spike"))
    for f in sorted(CRATES.glob("*/tests/*.rs")):
        out.append((f, "test"))
    return out


def normalise(p):
    """`orbweaver_giop::server::Server` -> `orbweaver-giop::server::Server`.

    The crate is a directory name with a hyphen and a Rust path spells it with
    an underscore; without this the same item is counted twice, once under each
    spelling, which the first run of this script did.
    """
    head, _, rest = p.partition("::")
    if head.startswith("orbweaver_"):
        head = head.replace("_", "-", 1)
    return head + ("::" + rest if rest else "")


def parse_uses(stripped, test_span):
    """`local name -> full path` for every orbweaver import, plus the globs.

    Handles nested groups (`use a::{b::{C, D}, E}`), `as` renames and `self`.
    A glob is returned separately: it brings in names this scan cannot list,
    so a program using one is under-counted and says so.
    """
    leaves, globs = {}, set()
    for m in re.finditer(r"(?m)^\s*(?:pub\s+)?use\s+([^;]+);", stripped):
        if stripped[:m.start()].count("\n") in test_span:
            continue
        expand_use(m.group(1).strip(), "", leaves, globs)
    return leaves, globs


def expand_use(tree, prefix, leaves, globs):
    tree = tree.strip()
    if tree.startswith("::"):
        # `use ::orbweaver_gen::rt::{..}` — the leading `::` is how generated
        # code names an absolute path, and dropping it silently made a 584-line
        # generated stub report one named item and rank first as "the shortest
        # way to make a call". Found by reading the top of the table, which is
        # the only reason this comment exists.
        tree = tree[2:]
    if tree.startswith("{"):
        depth, part, parts = 0, [], []
        for ch in tree[1:-1] if tree.endswith("}") else tree[1:]:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            if ch == "," and depth == 0:
                parts.append("".join(part))
                part = []
            else:
                part.append(ch)
        parts.append("".join(part))
        for p in parts:
            if p.strip():
                expand_use(p, prefix, leaves, globs)
        return
    head, sep, rest = tree.partition("::")
    head = head.strip()
    if sep and rest.strip().startswith("{"):
        expand_use(rest.strip(), (prefix + "::" if prefix else "") + head, leaves, globs)
        return
    if sep:
        expand_use(rest, (prefix + "::" if prefix else "") + head, leaves, globs)
        return
    full = (prefix + "::" if prefix else "") + head
    if not full.startswith("orbweaver_"):
        return
    if head == "*":
        globs.add(prefix)
        return
    if head == "self":
        leaves[prefix.rsplit("::", 1)[-1]] = prefix
        return
    am = re.match(r"(" + IDENT + r")\s+as\s+(" + IDENT + r")", head)
    if am:
        leaves[am.group(2)] = (prefix + "::" if prefix else "") + am.group(1)
        return
    leaves[head] = full


def analyse(path, index):
    """The entry cost of one program.

    `index` maps a bare item name to the set of `crate::module::Name` paths the
    public surface has for it.
    """
    raw = path.read_text(errors="replace")
    stripped = strip_rust(raw)
    lines_raw = raw.splitlines()
    lines = stripped.splitlines()

    # `#[cfg(test)] mod tests { .. }` is not part of the program.
    test_span = set()
    m = re.search(r"#\[cfg\(test\)\]\s*mod\s+" + IDENT + r"\s*\{", stripped)
    if m:
        depth, i = 0, m.end() - 1
        while i < len(stripped):
            if stripped[i] == "{":
                depth += 1
            elif stripped[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        start = stripped[:m.start()].count("\n")
        end = stripped[:i].count("\n")
        test_span = set(range(start, end + 1))

    named = {}            # full path -> how it was attributed. Unambiguous only.
    via_method = {}       # attributed by receiver-method name; may include std
    ambiguous = {}        # bare name -> set of candidate paths
    unresolved = {}       # imported from an orbweaver crate, absent from the surface
    api_lines = set()

    # ---- pass 1: what the program imported ----------------------------------
    #
    # A bare `Request` in Rust means whatever the file's `use` list says it
    # means, and the first version of this scan did not ask: it matched every
    # bare identifier against the surface index, so `Result` matched three
    # `pub type Result` aliases in three crates and appeared in the floor of
    # every cohort. A local `struct Echo`, a `std::Result` and an ORB type are
    # told apart by the import list and by nothing else, so that is what is
    # used. The cost is glob imports (`use x::*`), which are counted as
    # unresolvable and reported.
    use_leaves, globs = parse_uses(stripped, test_span)

    def resolve(pathish):
        """A `use`-path or a fully-qualified path -> a surface path, or None."""
        p = normalise(pathish)
        if p in index_kind:
            return p
        leaf = p.rsplit("::", 1)[-1]
        cands = index.get(leaf, ())
        if len(cands) == 1:
            # The surface scan indexes a method under `crate::Type::method`
            # and a re-export under its defining module, so a `use` that goes
            # through a re-export will not match literally. One candidate is
            # still an answer; more than one is not.
            return next(iter(cands))
        return None

    resolved_use = {}
    for local, full in use_leaves.items():
        r = resolve(full)
        if r:
            resolved_use[local] = r
        else:
            unresolved[local] = full
    imported_types = {local for local, p in resolved_use.items()
                      if index_kind.get(p) == "type"}

    # ---- pass 2: what it names ---------------------------------------------
    for idx, line in enumerate(lines):
        if idx in test_span:
            continue
        hits = set()

        # (a) a fully-qualified path names exactly what it says.
        for pm in re.finditer(r"\borbweaver_" + IDENT + r"(?:::" + IDENT + r")+", line):
            r = resolve(pm.group(0))
            if r:
                named.setdefault(r, "fully-qualified path `%s`" % pm.group(0))
                hits.add(r)
            else:
                unresolved.setdefault(pm.group(0), pm.group(0))

        # (b) `Local::assoc` where `Local` was imported from an orbweaver crate.
        for qm in re.finditer(r"(?<![:\w])(" + IDENT + r")::(" + IDENT + r")\b", line):
            ty, fn = qm.group(1), qm.group(2)
            base = resolved_use.get(ty)
            if not base:
                continue
            named.setdefault(base, "imported name `%s`" % ty)
            hits.add(base)
            for p in index.get(fn, ()):
                if p.split("::")[-2:-1] == [ty]:
                    named.setdefault(p, "qualified call `%s::%s`" % (ty, fn))
                    hits.add(p)

        # (c) a bare imported name.
        for w in set(re.findall(r"(?<![:\w])" + IDENT + r"(?![\w])", line)):
            base = resolved_use.get(w)
            if base:
                named.setdefault(base, "imported name `%s`" % w)
                hits.add(base)

        # (d) `x.method(..)`. Two filters make this worth having, and both are
        #     Rust semantics rather than a threshold someone tuned:
        #       - only a function that takes a receiver can be called this way,
        #         which is what stops `"1.2".parse()` being attributed to
        #         `Ior::parse` — it did, and the client-side counts were wrong;
        #       - if exactly one *imported* type owns a method of that name,
        #         that is the one the program means. Two, or none, and it is
        #         not resolved and goes in the second column.
        #     What remains unresolved is counted, listed, and never filtered:
        #     `.as_str()` has candidates in the surface and is a `str` method
        #     in most of its occurrences, and both facts are printed.
        for mm in re.finditer(r"\.(" + IDENT + r")\s*\(", line):
            fn = mm.group(1)
            cands = [p for p in index.get(fn, ())
                     if index_kind[p] == "fn" and index_self.get(p)]
            if not cands:
                continue
            owned = [p for p in cands
                     if p.split("::")[-2:-1] and p.split("::")[-2] in imported_types]
            if len(owned) == 1:
                named.setdefault(owned[0], "receiver call `.%s()` on the one imported "
                                           "type that has it" % fn)
                hits.add(owned[0])
                continue
            if len(cands) > 1:
                ambiguous.setdefault(fn, set()).update(cands)
            key = cands[0] if len(cands) == 1 else "*::" + fn
            via_method.setdefault(key, "receiver call `.%s()`, %d candidate(s)"
                                  % (fn, len(cands)))
            hits.add(key)

        if hits:
            api_lines.add(idx)

    code_lines = [i for i, s in enumerate(lines) if i not in test_span and s.strip()]
    return dict(
        path=str(path.relative_to(ROOT)),
        total=len(lines_raw),
        code=len(code_lines),
        api=len(api_lines),
        named=sorted(named),
        via_method=sorted(via_method),
        why={**named, **via_method},
        crates=sorted({p.split("::")[0] for p in named}),
        use_leaves=use_leaves,
        uses=len(use_leaves),
        globs=sorted(globs),
        unresolved=sorted(unresolved),
        ambiguous={k: sorted(v) for k, v in ambiguous.items()},
        test_lines=len(test_span),
        route=route_of(stripped),
        generated=raw.lstrip().startswith(GENERATED_MARKER[0]),
    )


def route_of(stripped):
    """`stub` if the program reaches the ORB through generated code.

    This turned out to be the finding, not a detail. A program written against
    `orbweaver-gen`'s emitted stubs names a handful of ORB items because the
    stub names the rest; a program written against the ORB directly names
    everything itself. Ranking the two together answers neither question, so
    they are ranked apart. Note what follows: a stub-route program's `named`
    count is **not comparable** to a direct one's, because the generated crate
    lives outside `crates/` and its surface is not indexed here — the names its
    caller must learn are real and are not in that column.
    """
    if re.search(r"\borbweaver_genout\b|\borbweaver_gen::rt\b", stripped):
        return "stub"
    return "direct"


index_kind = {}
index_self = {}


def classify(prog, anchors_ok):
    """Which jobs a program does, and which anchor said so."""
    roles = {}
    leaves = {p.rsplit("::", 1)[-1] for p in prog["named"] + prog["via_method"]}
    leaves |= set(prog["use_leaves"])
    for role, anchors in ANCHORS.items():
        for name, _why in anchors:
            if name in leaves and anchors_ok.get(name):
                roles.setdefault(role, []).append(name)
    return roles


# --------------------------------------------------------------------------
def main(argv):
    global ROOT, CRATES
    want_json = "--json" in argv
    no_cargo = "--no-cargo" in argv
    if "--root" in argv:
        # A report has no negative control in the usual sense, so this one is
        # given the ability to be pointed at a *mutated copy* of the tree. The
        # controls that were run against it are in the commit message: delete
        # an `impl Dispatch` and the program must leave the serve table; plant
        # a twelve-line servant and it must become the shortest; rename a
        # public type and its anchor must go STALE. Without this switch those
        # could only be run by editing crates, which E4 may not touch.
        ROOT = Path(argv[argv.index("--root") + 1]).resolve()
        CRATES = ROOT / "crates"
    global GENERATED_MARKER
    GENERATED_MARKER = generated_marker()
    notes = []

    crate_dirs = sorted(d for d in CRATES.iterdir() if (d / "Cargo.toml").is_file())
    surface, trait_impls, fences = {}, {}, {}
    all_items = []
    for d in crate_dirs:
        crate = d.name
        reachable = public_module_paths(d)
        items, ti = scan_items(crate, d, reachable, notes)
        surface[crate] = items
        trait_impls[crate] = ti
        fences[crate] = fence_census(d)
        all_items.extend(items)

    index = {}
    for it in all_items:
        full = "%s::%s::%s" % (it["crate"], it["module"], it["name"]) if it["module"] \
            else "%s::%s" % (it["crate"], it["name"])
        index.setdefault(it["name"], set()).add(full)
        index_kind[full] = it["kind"]
        index_self[full] = index_self.get(full) or it.get("takes_self", False)

    anchors_ok = {}
    for role, anchors in ANCHORS.items():
        for name, _why in anchors:
            anchors_ok[name] = name in index

    progs = []
    for path, kind in programs():
        p = analyse(path, index)
        p["kind"] = kind
        p["roles"] = classify(p, anchors_ok)
        progs.append(p)

    anchor_hits = {name: 0 for role in ANCHORS for name, _ in ANCHORS[role]}
    for p in progs:
        for role, hit in p["roles"].items():
            for name in hit:
                anchor_hits[name] += 1

    doctests, doc_err = ({}, "--no-cargo given: doctests not measured") if no_cargo \
        else doctest_counts()

    # ---------------- the report ----------------
    if want_json:
        print(json.dumps(dict(
            root=str(ROOT),
            surface={c: dict(
                types=sum(1 for i in surface[c] if i["kind"] == "type"),
                functions=sum(1 for i in surface[c] if i["kind"] == "fn"),
                consts=sum(1 for i in surface[c] if i["kind"] == "const"),
                trait_impl_methods=trait_impls[c],
                fences=fences[c],
                doctests=doctests.get(c),
            ) for c in sorted(surface)},
            doctest_error=doc_err,
            anchors={n: dict(in_surface=anchors_ok[n], programs=anchor_hits[n])
                     for n in anchor_hits},
            programs=[{k: v for k, v in p.items() if k != "why"} for p in progs],
        ), indent=2, sort_keys=True))
        return 0

    print("entry cost — what a newcomer has to name, measured from the tree")
    print("=" * 74)
    print()
    print("PUBLIC SURFACE  (a type is a pub struct/enum/trait/type/union; a function is a")
    print("                 pub fn, or a method signature on a pub trait; trait-impl")
    print("                 methods are listed apart — see the module doc for the rule)")
    print()
    print("  %-24s %6s %6s %6s   %8s  %9s" %
          ("crate", "types", "fns", "consts", "traitimp", "doctests"))
    tot_t = tot_f = 0
    for c in sorted(surface):
        t = sum(1 for i in surface[c] if i["kind"] == "type")
        f = sum(1 for i in surface[c] if i["kind"] == "fn")
        k = sum(1 for i in surface[c] if i["kind"] == "const")
        tot_t += t
        tot_f += f
        dt = "unmeasured" if doc_err else str(doctests.get(c, 0))
        print("  %-24s %6d %6d %6d   %8d  %9s" % (c, t, f, k, trait_impls[c], dt))
    print("  %-24s %6d %6d" % ("(total)", tot_t, tot_f))
    print()

    print("COMPILED EXAMPLES")
    ex_dirs = [d.name for d in crate_dirs if (d / "examples").is_dir()]
    print("  examples/ directories: %s" % (", ".join(ex_dirs) if ex_dirs else "none, in any crate"))
    if doc_err:
        print("  doctests: UNMEASURED — %s" % doc_err)
    else:
        print("  doctests that `cargo test --doc --workspace` ran: %d" % sum(doctests.values()))
        for c, n in sorted(doctests.items()):
            if n:
                print("      %-24s %d" % (c, n))
    print("  doc fences by tag, across all crates — a fence is not a doctest unless")
    print("  rustdoc compiles it, and a `text` fence never is:")
    tot_fence = {}
    for c in fences:
        for tag, n in fences[c].items():
            tot_fence[tag] = tot_fence.get(tag, 0) + n
    for tag, n in sorted(tot_fence.items(), key=lambda kv: -kv[1]):
        mark = "compiled" if tag not in UNCOMPILED_FENCE_TAGS else "not compiled"
        print("      ```%-10s %4d   (%s)" % (tag, n, mark))
    print()

    print("ROLE ANCHORS  (a program's job is decided by what it names, never by its name)")
    stale = []
    for role in ("serve", "call"):
        for name, why in ANCHORS[role]:
            if not anchors_ok[name]:
                verdict = "STALE — names no public item in any crate today"
                stale.append((role, name, "not in the public surface"))
            elif anchor_hits[name] == 0:
                verdict = "STALE — public, but no program names it"
                stale.append((role, name, "public but unused by any program"))
            else:
                verdict = "%d program(s)" % anchor_hits[name]
            print("  %-5s %-18s %-42s %s" % (role, name, verdict, why[:42]))
    if stale:
        print()
        print("  A STALE anchor is this report saying the API moved under it, not that")
        print("  nobody does the job. D019 step 4 retires `Server::bind` and `Poa::new`")
        print("  as the public way in; when it lands, expect anchors here to go stale and")
        print("  replace them with `Orb`'s entry points rather than reading the zero.")
    print()

    def cost_row(p):
        return ("  %-46s %-6s %5d %5d %5d %5d %5d %3d"
                % (p["path"][-46:], p["kind"], p["total"], p["code"], p["api"],
                   len(p["named"]), len(p["via_method"]), len(p["crates"])))

    head = ("  %-46s %-6s %5s %5s %5s %5s %5s %3s"
            % ("program", "kind", "total", "code", "api", "named", "+meth", "crt"))

    def shortest(group, label):
        if not group:
            print("  %-24s (none)" % label)
            return
        by_named = min(group, key=lambda p: len(p["named"]))
        by_code = min(group, key=lambda p: p["code"])
        print("  %s — fewest items named: %s" % (label, by_named["path"]))
        print("      %d item(s) from %d crate(s), +%d by method name, %d code line(s)"
              " of %d total"
              % (len(by_named["named"]), len(by_named["crates"]),
                 len(by_named["via_method"]), by_named["code"], by_named["total"]))
        print("  %s — fewest code lines : %s" % (" " * len(label), by_code["path"]))
        print("      %d code line(s) of %d total, %d of them name the ORB, %d item(s)"
              % (by_code["code"], by_code["total"], by_code["api"], len(by_code["named"])))

    watch = []
    for role, title in (("serve", "SERVING AN OBJECT"), ("call", "MAKING A CALL")):
        rp = [p for p in progs if role in p["roles"]]
        direct = sorted((p for p in rp if p["route"] == "direct"),
                        key=lambda p: (len(p["named"]), p["code"]))
        stub = sorted((p for p in rp if p["route"] == "stub"),
                      key=lambda p: (len(p["named"]), p["code"]))
        print("%s — %d program(s) in the tree do this: %d against the ORB directly,"
              % (title, len(rp), len(direct)))
        print("%s   %d through generated stubs." % (" " * len(title), len(stub)))
        if not rp:
            print("  nothing measured. Check the anchor table above before reading this")
            print("  as an absence.")
            print()
            continue
        print()
        print("  DIRECT — against the ORB's own API. This is the column D027 §1 is about.")
        print(head)
        for p in direct[:22]:
            print(cost_row(p))
        if len(direct) > 22:
            print("      … and %d more" % (len(direct) - 22))
        print()
        print("  STUB — the caller was handed generated code. **Not comparable to the")
        print("  column above**: the generated crate lives outside `crates/`, its surface")
        print("  is not indexed here, and the names its caller must learn are real and")
        print("  absent from `named`. Ranked apart for that reason, not hidden.")
        print(head)
        for p in stub[:10]:
            print(cost_row(p))
        if len(stub) > 10:
            print("      … and %d more" % (len(stub) - 10))
        print()
        shortest([p for p in direct if p["kind"] != "test"], "direct, written as a program")
        shortest([p for p in direct if p["kind"] == "test"], "direct, written as a test  ")
        shortest([p for p in stub if p["kind"] != "test"], "via a generated stub       ")
        print()

        # ---- the floors ----
        #
        # An intersection across a heterogeneous cohort collapses to nothing
        # and says nothing, so the floor is computed **per anchor**: among the
        # programs that reach this job through one route, what does none of
        # them avoid? The whole-role floor is printed too, because its being
        # small is itself the finding — it means there is more than one way in.
        print("  FLOOR — items no program of a cohort avoids naming. This is the figure to")
        print("  re-take after an API change; items entering and leaving it are the change.")
        print("  Computed over the DIRECT programs only: a stub-route program names few")
        print("  ORB items by design, and one in a cohort empties that cohort's floor.")
        for name, _why in ANCHORS[role]:
            cohort = [p for p in direct if name in p["roles"].get(role, [])]
            if len(cohort) < 2:
                print("    via %-20s %d program(s) — too few to intersect" % (name, len(cohort)))
                continue
            fl = set(cohort[0]["named"])
            for p in cohort[1:]:
                fl &= set(p["named"])
            print("    via %-20s %2d program(s), floor %d:" % (name, len(cohort), len(fl)))
            watch.append((role, name, len(cohort), len(fl)))
            for it in sorted(fl):
                print("        %s" % it)
            if not fl:
                print("        (none — even inside this cohort there is more than one route,")
                print("         or the anchor is broader than the job. Not a zero to quote.)")
        whole = set(direct[0]["named"]) if direct else set()
        for p in direct[1:]:
            whole &= set(p["named"])
        print("    across all %d direct %s program(s): %d item(s)%s"
              % (len(direct), role, len(whole),
                 (" — " + ", ".join(sorted(whole))) if whole else
                 " (there is no single item every route names)"))
        print()

    both = [p for p in progs if len(p["roles"]) == 2]
    unclassified = [p for p in progs if not p["roles"]]
    print("WHAT THIS RUN DID NOT MEASURE")
    print("  %d program(s) scanned: %s." %
          (len(progs), ", ".join("%d %s" % (sum(1 for p in progs if p["kind"] == k), k)
                                 for k in ("bin", "example", "spike", "test"))))
    print("  %d do both jobs and appear in both tables; %d matched no anchor and are"
          % (len(both), len(unclassified)))
    print("  named below, because a program doing a job by a route the anchor table does")
    print("  not know is exactly what this report must not swallow:")
    for p in sorted(unclassified, key=lambda p: -p["total"])[:12]:
        print("      %-56s %4d lines, %2d ORB item(s)" %
              (p["path"][:56], p["total"], len(p["named"])))
    if len(unclassified) > 12:
        print("      … and %d more" % (len(unclassified) - 12))
    amb = {}
    for p in progs:
        for fn, cands in p["ambiguous"].items():
            amb.setdefault(fn, set()).update(cands)
    print("  %d method name(s) were attributed by name alone and had more than one"
          % len(amb))
    print("  candidate in the surface; they are counted once as `*::name` and listed")
    print("  here rather than filtered out:")
    for fn in sorted(amb)[:10]:
        print("      .%-22s %d candidate(s)" % (fn + "()", len(amb[fn])))
    if len(amb) > 10:
        print("      … and %d more" % (len(amb) - 10))
    globbed = [p for p in progs if p["globs"]]
    unres = {}
    for p in progs:
        for u in p["unresolved"]:
            unres[u] = unres.get(u, 0) + 1
    gen = [p for p in progs if p["generated"]]
    print("  %d program(s) use a glob import (`use x::*`) of an orbweaver crate. A glob"
          % len(globbed))
    print("  brings in names this scan cannot list, so those programs are under-counted:")
    for p in globbed[:5]:
        print("      %-56s %s" % (p["path"][:56], ", ".join(p["globs"])))
    if len(globbed) > 5:
        print("      … and %d more" % (len(globbed) - 5))
    print("  %d distinct orbweaver path(s) were imported or written out and could not be"
          % len(unres))
    print("  matched to a public item — a re-export through a private module, a generated")
    print("  crate outside `crates/`, or a scan defect. They are not counted:")
    for u, n in sorted(unres.items(), key=lambda kv: -kv[1])[:8]:
        print("      %-56s in %d program(s)" % (u[:56], n))
    if len(unres) > 8:
        print("      … and %d more" % (len(unres) - 8))
    print("  %d file(s) carry the generator's own header and are emitted, not written."
          % len(gen))
    print("      marker %r, %s" % (GENERATED_MARKER[0], GENERATED_MARKER[1]))
    print("  Not measured at all:")
    print("    - unit tests inside `src/*.rs`. Some serve and call, and one of them may")
    print("      be a shorter path than anything above; they are inside a crate that")
    print("      already imports everything, so their `named` count would not mean the")
    print("      same thing.")
    print("    - items reached without being named: an inferred trait method, a `?` that")
    print("      converts an error type, a `Default::default()`. Real knowledge, invisible")
    print("      here, and the reason `named` is a floor on what a newcomer must learn")
    print("      rather than the whole of it.")
    print("    - the surface of the crate `orbweaver-gen` emits. A stub-route program's")
    print("      cost is mostly names from a crate that is not in this tree, so its")
    print("      `named` column measures the ORB it *avoided*, not the API it used.")
    print("    - whether any of it is *hard*. This counts names. `Server::bind` and")
    print("      `Ior::parse` are one each; one of them can fail eleven ways.")
    print("    - `pub use` re-exports of items from private modules: the surface scan")
    print("      walks `pub mod` only, so an item re-exported out of a private module is")
    print("      missed. Modules skipped for that reason are listed as notes below.")
    if notes:
        for n in notes[:8]:
            print("      note: %s" % n)
        if len(notes) > 8:
            print("      … and %d more note(s)" % (len(notes) - 8))
    print()
    print("  This is a report. It counts; it does not judge. D027 §7 does not claim the")
    print("  surface is too large, and neither does this. The figure to re-take after a")
    print("  change is the FLOOR of each table — the items no program avoids — because")
    print("  that is the one a new entry point can actually move.")
    print()
    print("  All of these figures are today's measurement, not a floor pinned anywhere:")
    print("  re-run the script rather than quoting a number out of a document.")
    print()
    print("THE FIGURES A LATER BATCH RE-TAKES")
    print("  Nothing below is pinned in a gate. Each is what this run measured, and the")
    print("  point of each is its *movement*: D019 step 4 makes `Orb` the only route in,")
    print("  and D027 E1's doctest is the smallest program that serves and the smallest")
    print("  that calls. Both change these and nothing else in the tree does.")
    print("    public surface, orbweaver-giop  : %d types, %d functions"
          % (sum(1 for i in surface.get("orbweaver-giop", []) if i["kind"] == "type"),
             sum(1 for i in surface.get("orbweaver-giop", []) if i["kind"] == "fn")))
    print("    public surface, orbweaver-object: %d types, %d functions"
          % (sum(1 for i in surface.get("orbweaver-object", []) if i["kind"] == "type"),
             sum(1 for i in surface.get("orbweaver-object", []) if i["kind"] == "fn")))
    print("    doctests, whole workspace       : %s"
          % ("UNMEASURED (%s)" % doc_err if doc_err else sum(doctests.values())))
    print("    examples/ directories           : %d" % len(ex_dirs))
    for role, name, n, fl in watch:
        print("    floor, %-5s via %-18s: %2d item(s) over %d program(s)"
              % (role, name, fl, n))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
