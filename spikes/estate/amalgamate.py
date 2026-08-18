#!/usr/bin/env python3
"""Flatten an estate of `#include`-ing IDL files into one translation unit.

    amalgamate.py <dir> > estate.idl
    amalgamate.py <dir> --order        # print the topological order only
    amalgamate.py <dir> --naive        # splice without the prefix reset (see below)

This existed because `orbweaver-idl` **skipped** `#include` rather than
resolving it, so no file of a real estate that named a type declared in another
file could be validated, registered, generated from or exposed on its own —
flattening in dependency order was the only thing an operator could do with the
shipped tools. **That is no longer true.** `crate::include` resolves the
directive before the lexer runs, `Contract::load` is the entry point that takes
a path, and `sidl-validate`, `repository-ids`, `gen-corpus` and
`orbweaver-mcp-server` all take the thirteen files of this estate directly.

The docstring said otherwise for several batches after the fact, and cited
`lex.rs` line 19 — where the lexer's own comment had by then been rewritten to
say `#include` "used to be in that list, and being in it was a defect". A stale
reason is worse than no reason: it is why `spikes/estate/run.sh` kept routing
everything through here, and why nobody noticed that for `orbweaver-mcp-server`
the splice was not a convenience but the only thing making the process start at
all (`83eba89`).

So the script stays, for a reason that is true today rather than one that was:
**some consumers take a translation unit, not a file set.** `forge-pipeline`'s
S4 stage supplies its item as *text*, which has no directory for a quoted
`#include` to resolve against, and refuses the estate's thirteen files with
`[include-not-found]`; `gen-corpus` derives a module name per file stem, so
thirteen inputs give thirteen modules where a single unit gives one. For those,
this is still the operator's hand, written down. For everything that takes a
path, the files themselves are now the better input, and
`spikes/estate/run.sh` stage 7b measures the two against each other rather than
assuming they agree.

# The prefix reset, which is the whole reason this file is not four lines long

A file-scope `#pragma prefix` is in force to the end of its **file**. After a
naive splice there is only one file, so a prefix set by the file at line 200 is
still in force when the declarations of the file at line 400 arrive — and every
estate file that carries no prefix of its own silently acquires its
predecessor's.

That is not cosmetic. A repository id is identity on the wire: `_is_a`, an
IOR's `type_id`, the IFR facade, remote ingestion's matching and the guard's
per-interface `--expose` all key on it. Measured on this estate with `--naive`,
five of Billing's ids come out as `IDL:meridian.com/MFS/Billing/...` where every
peer built from the files themselves says `IDL:MFS/Billing/...`. An operator who
allowlists the id our catalog printed would be allowlisting an interface no
deployed object has, and the refusal would look like a policy mistake.

So each spliced file's own body is preceded by `#pragma prefix ""`, which is the
state it would have begun in had it been compiled alone. `--naive` keeps the old
behaviour so the driver can measure the difference rather than assert it.

`--order` prints the dependency order alone, so the driver can report the graph
it found without re-deriving it.
"""

import pathlib
import re
import sys

INCLUDE = re.compile(r'^\s*#\s*include\s+"([^"]+)"')
GUARD = re.compile(r"^\s*#\s*(ifndef|define|endif)\b")


def body(path: pathlib.Path) -> list[str]:
    """The file's lines with its include guard removed. Includes stay."""
    return [line for line in path.read_text().split("\n") if not GUARD.match(line)]


def walk(path: pathlib.Path, seen: set, order: list, reset: bool) -> list[str]:
    """`path`'s dependencies, then `path`'s own body. Each file at most once."""
    key = path.resolve()
    if key in seen:
        return []
    seen.add(key)

    before: list[str] = []
    own: list[str] = []
    for line in body(path):
        m = INCLUDE.match(line)
        if not m:
            own.append(line)
            continue
        dep = (path.parent / m.group(1)).resolve()
        if not dep.exists():
            print(f"amalgamate: {path.name}: no such include {m.group(1)}", file=sys.stderr)
            sys.exit(2)
        before += walk(dep, seen, order, reset)

    order.append(path.name)
    banner = ["", f"// ==== {path.name} " + "=" * max(4, 60 - len(path.name))]
    # The state a file would have begun in had it been compiled alone.
    lead = ['#pragma prefix ""'] if reset else []
    return before + banner + lead + own


def main() -> int:
    argv = sys.argv[1:]
    if not argv or argv[0].startswith("-"):
        print(__doc__, file=sys.stderr)
        return 2
    root = pathlib.Path(argv[0])
    only_order = "--order" in argv
    reset = "--naive" not in argv

    seen: set = set()
    order: list = []
    chunks: list[str] = []
    for path in sorted(root.glob("*.idl")):
        chunks += walk(path, seen, order, reset)

    if only_order:
        for i, name in enumerate(order, 1):
            print(f"{i:2d} {name}")
        return 0

    print("// Generated by spikes/estate/amalgamate.py — do not edit.")
    print("// One translation unit, for the consumers that take a unit rather than a path.")
    if not reset:
        print("// --naive: no prefix reset. Repository ids WILL drift. See the module docs.")
    print("\n".join(chunks))
    return 0


if __name__ == "__main__":
    sys.exit(main())
