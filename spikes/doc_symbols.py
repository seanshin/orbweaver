#!/usr/bin/env python3
"""Every symbol this repository owns that a current-status document names, checked
against a definition in the tree.

Why this exists, and what it is NOT a wider version of. `gap_symbols.py` asks
the same question of the COMPONENTS gap *columns* and reported `22 named, 22
exist` on 2026-09-02 while four sites named `orbweaver_gen::pychild::PythonChild`,
a type renamed to `SeamChild` the day before. The gate was scoped to a column;
the rule is about a claim. That is this repository's own lesson — *a sweep is
scoped to a rule; a sweep that names a file will sweep that file* — wearing
symbols instead of shell.

Three exclusions, each a reason rather than a quieting:

  - **Dated records are out of scope by construction** (CLAUDE.md, "Where a fact
    lives"): `docs/decisions/`, `docs/pipeline-runs/`, `PHASE*`. They state what
    was true at a date, and editing them to match today would falsify them.
  - **A head segment we do not own** belongs to a dependency (`libc::kill`).
    Whether it exists is Cargo's job; this scan keeps no claim about it.
  - **A rename record** — ``X`` became ``Y`` — names the old symbol on purpose.
    Its claim is that the name CHANGED, not that it exists. This is the same
    split `cited_and_run.py` draws between a header that refuses a gate (a
    decision, passes) and one that defers it (an IOU, fails).

Exit 1 on any finding; the finding names every site.
"""
import re, subprocess, pathlib, sys

def ls(root, p):
    r = subprocess.run(["git","ls-files",p], cwd=root, capture_output=True, text=True)
    return r.stdout.split()

DEFKW = (r"(struct|enum|trait|type|union|const|static|fn|mod|macro_rules!"
         r"|class|def|interface|module|valuetype|exception|typedef)")
PATH  = re.compile(r"`([a-z_][a-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)`")
# The rename verb may sit behind a short clause — "`X`, a type renamed to `Y`" —
# but never behind another backticked symbol, which is what `[^`]` guards.
FORMER = re.compile(r"[^`]{0,60}?(became|was renamed|is now|renamed to|→|->)\s+`")

def scan(root):
    root = pathlib.Path(root)
    current = [d for d in ls(root,"docs") if d.endswith(".md")
               and not d.startswith(("docs/pipeline-runs/","docs/decisions/"))
               and not pathlib.Path(d).name.startswith("PHASE")]
    srcs = [f for f in ls(root,"crates")+ls(root,"spikes")+ls(root,"corpus")
            if f.endswith((".rs",".py",".java",".idl"))]
    blob = "\n".join((root/f).read_text(errors="ignore") for f in srcs)
    # Our namespace is a fact about NAMING, not about what happens to exist: an
    # `ours` derived only from the current tree goes silent exactly when a crate
    # is deleted or renamed, which is when the documents are most likely wrong.
    # Control 5 caught this — the first draft reported nothing once the crate
    # was removed, and read as green.
    ours = set()
    for f in ls(root,"crates"):
        parts = f.split("/")
        if len(parts) > 1: ours.add(parts[1].replace("-","_"))
        if len(parts) > 3 and parts[2] == "src": ours.add(parts[3].removesuffix(".rs"))
    for m in re.finditer(r"\bmodule\s+([A-Za-z_][A-Za-z0-9_]*)", blob): ours.add(m.group(1))

    missing = {}
    for d in sorted(current):
        # Read the document as text, not line by line: markdown wraps wherever it
        # likes, and the first version of this scan reported a rename record whose
        # verb had simply landed on the next line.
        text = (root/d).read_text()
        for m in PATH.finditer(text):
            sym = m.group(1); head, leaf = sym.split("::")[0], sym.split("::")[-1]
            if head not in ours and not head.startswith("orbweaver_"): continue
            if re.search(r"\b"+DEFKW+r"\s+"+re.escape(leaf)+r"\b", blob): continue
            # IDL puts a typedef's declared name LAST: `typedef X Leaf;`
            if re.search(r"\btypedef\b[^;]*\b"+re.escape(leaf)+r"\s*(\[[^\]]*\])?\s*;", blob): continue
            if FORMER.match(text[m.end():m.end()+140].replace("\n", " ")): continue
            missing.setdefault(sym, []).append((d, text.count("\n", 0, m.start()) + 1))
    return current, missing

def main():
    root = sys.argv[sys.argv.index("--root")+1] if "--root" in sys.argv else "."
    current, missing = scan(root)
    print("%d current-status document(s) scanned; dated records are out of scope by construction"
          % len(current))
    if not missing:
        print("  ok   every symbol this repository owns that they name is defined in the tree")
        return 0
    print("  FAIL %d symbol(s) named as existing, defined nowhere:" % len(missing))
    for sym, sites in sorted(missing.items()):
        print("       %-42s %s" % (sym, ", ".join("%s:%d" % s for s in sites)))
    return 1

if __name__ == "__main__":
    sys.exit(main())
