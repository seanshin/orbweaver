#!/usr/bin/env bash
# Negative control for doc_symbols.py. It SYNTHESISES the tree rather than
# pointing at today's — a control pinned to a live subject stops being a control
# when the subject moves, measured in this repository twice in one day.
#
# It runs the shipped script's own bytes. It does not restate its rules.
set -u
SCAN="${1:-$(dirname "$0")/doc_symbols.py}"
# `${TMPDIR:-/tmp}`, not `/private/tmp`: that path is a macOS fact, and this
# control failed in CI on its first run because the repair for `find` on a
# symlinked /tmp was applied where it does not belong. The template stays
# explicit, which is what the harness's mktemp gate asks for.
T=$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-symctl.XXXXXX") || exit 3
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/docs/decisions" "$T/crates/orbweaver-gen/src" "$T/corpus/golden"
( cd "$T" && git init -q . && git config user.email c@x && git config user.name c )

cat > "$T/crates/orbweaver-gen/src/seam.rs" <<'RS'
pub struct SeamChild {}
RS
cat > "$T/corpus/golden/t.idl" <<'IDL'
module moe { typedef sequence<octet> Tensor; };
IDL

fails=0
say() { echo "  $1"; [ "$1" = "${1#ok}" ] && fails=$((fails+1)); }

# 1 — the defect itself: a live claim naming a symbol nothing defines.
cat > "$T/docs/COMPONENTS.md" <<'MD'
| x | `orbweaver_gen::pychild::PythonChild` is what the test mounts |
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 1 ] && grep -q "PythonChild" <<<"$out"; then
  say "ok   1 a live claim on an undefined symbol is caught"
else say "FAIL 1 the defect was not caught (rc=$rc): $out"; fi

# 2 — a rename record claims a CHANGE, not an existence.
cat > "$T/docs/COMPONENTS.md" <<'MD'
| x | `orbweaver_gen::pychild::PythonChild` became `SeamChild` that day |
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 0 ]; then say "ok   2 a rename record is not a claim of existence"
else say "FAIL 2 a rename record was reported: $out"; fi

# 3 — a dated record is out of scope by construction, not by being quiet.
cat > "$T/docs/COMPONENTS.md" <<'MD'
nothing here
MD
cat > "$T/docs/decisions/D001-x.md" <<'MD'
`orbweaver_gen::pychild::PythonChild` is the shape we chose.
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 0 ]; then say "ok   3 a dated record is out of scope"
else say "FAIL 3 a dated record was scanned: $out"; fi

# 4 — IDL declares a typedef's name LAST; the keyword-then-name form cannot see it.
cat > "$T/docs/COMPONENTS.md" <<'MD'
the plane rule counts every operation carrying a `moe::Tensor` across the wire
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 0 ]; then say "ok   4 an IDL typedef counts as a definition"
else say "FAIL 4 an IDL typedef was called undefined: $out"; fi

# 5 — the control's own control: if the scan stops reading documents at all,
#     cases 2-4 pass for the wrong reason. Case 1 must go red when it is empty.
cat > "$T/docs/COMPONENTS.md" <<'MD'
| x | `orbweaver_gen::pychild::PythonChild` is what the test mounts |
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
rm -f "$T/crates/orbweaver-gen/src/seam.rs"
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 1 ]; then say "ok   5 the scan is still reading documents (case 1 red without the crate)"
else say "FAIL 5 the scan went quiet when the tree changed: $out"; fi

# 6 — a rename record whose verb WRAPS to the next line. Markdown wraps wherever
#     it likes; the line-by-line draft of this scan reported exactly this as a
#     live claim, and it was this repository's own document that caught it.
cat > "$T/crates/orbweaver-gen/src/seam.rs" <<'RS'
pub struct SeamChild {}
RS
cat > "$T/docs/COMPONENTS.md" <<'MD'
four sites named `orbweaver_gen::pychild::PythonChild`, a type renamed to
`SeamChild` the day before, and nothing was red
MD
( cd "$T" && git add -A >/dev/null 2>&1 )
out=$(python3 "$SCAN" --root "$T"); rc=$?
if [ $rc -eq 0 ]; then say "ok   6 a rename record still counts when it wraps a line"
else say "FAIL 6 a wrapped rename record was reported: $out"; fi

echo
[ "$fails" -eq 0 ] && { echo "doc_symbols control: 6 of 6"; exit 0; }
echo "doc_symbols control: $fails FAILED"; exit 1
