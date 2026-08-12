#!/usr/bin/env python3
"""Pre-oracle lint for the IDL rule that keeps costing us.

IDL identifier clashes are case-insensitive: a member, parameter or operation
may not share a name with a type or an enclosing scope, ignoring case. This is
idiomatic naming in every other language, which is exactly why it is the
dominant generation failure — it accounted for 7/7 failures in the Phase 0
assumption B benchmark, and it has since caught two hand-written corpus files
and two hand-written fixtures, one of them written by someone who had just
documented the rule in that same file's header.

Documentation demonstrably does not prevent it. A check does.

This is the interim home for the rule. It moves into `orbweaver-idl` as a real
lint pass once that exists (PLAN §7 Phase 2); `omniidl` stays the authority on
conformance either way. The value here is catching it *earlier*, with a message
that says what to do rather than what is wrong.

Usage: idl_lint.py <file.idl> [more.idl ...]
Exit code is the number of files with findings.
"""

import re
import sys
from pathlib import Path

# Keywords that introduce a named type or scope.
DECLARING = ("struct", "union", "exception", "interface", "enum", "module", "valuetype")

# IDL primitives can never be the *declared* side of a clash.
PRIMITIVES = {
    "void", "boolean", "char", "wchar", "octet", "short", "long", "float",
    "double", "string", "wstring", "any", "Object", "TypeCode", "unsigned",
    "sequence", "fixed", "native",
}


def strip_noise(text: str) -> str:
    """Remove comments and string literals so they cannot produce findings."""
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = re.sub(r"//[^\n]*", "", text)
    text = re.sub(r'"(?:[^"\\]|\\.)*"', '""', text)
    return text


def declared_types(text: str) -> dict[str, str]:
    """Map lowercase name -> as-written name, for every declared type or scope."""
    found: dict[str, str] = {}
    for kw in DECLARING:
        for m in re.finditer(rf"\b{kw}\s+([A-Za-z_]\w*)", text):
            found[m.group(1).lower()] = m.group(1)
    # typedef ends with the new name, optionally followed by array dimensions.
    for m in re.finditer(r"\btypedef\b[^;]*?\b([A-Za-z_]\w*)\s*(?:\[[^\]]*\]\s*)*;", text):
        found[m.group(1).lower()] = m.group(1)
    return found


def check(path: Path) -> list[str]:
    raw = path.read_text(encoding="utf-8")
    text = strip_noise(raw)
    types = declared_types(text)
    findings: list[str] = []

    def line_of(pos: int) -> int:
        return text.count("\n", 0, pos) + 1

    # 1. `TypeName identifier` where the two differ only by case.
    #    Covers struct and exception members, and operation parameters.
    for m in re.finditer(r"\b([A-Za-z_]\w*)\s+([A-Za-z_]\w*)\s*(?=[;,)\[])", text):
        type_name, ident = m.group(1), m.group(2)
        if type_name in PRIMITIVES or ident in PRIMITIVES:
            continue
        if type_name.lower() not in types:
            continue
        if type_name.lower() == ident.lower() and type_name != ident:
            findings.append(
                f"{path}:{line_of(m.start())}: '{ident}' clashes with type "
                f"'{type_name}' — IDL identifier comparison ignores case. "
                f"Rename the member, not the type: try '{ident}_value', "
                f"'{ident}_field', or a domain word such as 'pos' for 'Position'."
            )

    # 2. Anything declared inside a scope whose name matches it, ignoring case.
    #    The scope may be a module, but it may equally be the struct itself:
    #    `struct Version { unsigned long version; }` is illegal for the same
    #    reason, and is missed entirely if only modules are considered.
    for m in re.finditer(rf"\b({'|'.join(DECLARING)})\s+([A-Za-z_]\w*)\s*(?::[^{{]*)?\{{", text):
        kind, scope = m.group(1), m.group(2)
        body = text[m.end():]
        depth, end = 1, len(body)
        for i, ch in enumerate(body):
            depth += (ch == "{") - (ch == "}")
            if depth == 0:
                end = i
                break
        inner_text = body[:end]

        # Nested type or scope declarations sharing the enclosing name.
        for inner in re.finditer(
            rf"\b(?:{'|'.join(DECLARING)})\s+([A-Za-z_]\w*)", inner_text
        ):
            if inner.group(1).lower() == scope.lower():
                findings.append(
                    f"{path}:{line_of(m.end() + inner.start())}: '{inner.group(1)}' "
                    f"clashes with its enclosing scope '{kind} {scope}' — comparison "
                    f"ignores case. Rename one of them; renaming the inner name is "
                    f"usually right because the outer path is what callers depend on."
                )

        # Members and parameters sharing the enclosing scope's name. Only the
        # scope's own level, so a nested struct's members are attributed to it.
        depth = 0
        for decl in re.finditer(r"([A-Za-z_]\w*)\s*(?=[;,)\[])|[{}]", inner_text):
            tok = decl.group(0)
            if tok == "{":
                depth += 1
                continue
            if tok == "}":
                depth -= 1
                continue
            if depth != 0:
                continue
            ident = decl.group(1)
            if ident and ident.lower() == scope.lower() and ident != scope:
                findings.append(
                    f"{path}:{line_of(m.end() + decl.start())}: '{ident}' clashes "
                    f"with its enclosing scope '{kind} {scope}' — comparison ignores "
                    f"case. Rename the member: try '{ident}_number', '{ident}_value', "
                    f"or a domain word."
                )

    # Two rules can reach the same spot; report each location once.
    return sorted(set(findings), key=findings.index)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__.strip().splitlines()[-2], file=sys.stderr)
        return 2
    bad = 0
    for arg in argv[1:]:
        findings = check(Path(arg))
        if findings:
            bad += 1
            for f in findings:
                print(f)
    return bad


if __name__ == "__main__":
    sys.exit(main(sys.argv))
