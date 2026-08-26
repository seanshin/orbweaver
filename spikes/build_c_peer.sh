#!/usr/bin/env bash
# Builds `spikes/c_peer.c`, and says what it found rather than what it assumed.
#
# A C compiler is not guaranteed. CLAUDE.md: *a fixture that will not build is a
# counted failure or a counted SKIPPED naming what is missing — never silence*,
# and *an unmeasured check is a failure, never a pass*. So the two outcomes are
# told apart by exit code and neither of them is quiet:
#
#   0  built; the path is on stdout
#   1  a compiler is present and the peer did not compile — a FAILURE, because
#      the only thing that could be wrong is our own source
#   2  no C compiler on this machine — UNMEASURED, and the runner turns that
#      into a counted SKIPPED naming `cc`
#
# The distinction is D010 §2's, and it is the one that decides whether a red
# board means "we broke it" or "this machine cannot see it".
#
# ── No ORB, and the build proves it ─────────────────────────────────────────
#
# There is no `-I`, no `-l` and no `pkg-config`: the peer includes C99 and POSIX
# sockets and nothing else. That is not an accident of convenience, it is
# CLAUDE.md's licensing boundary — omniORB, TAO and JacORB are fixtures, never
# dependencies, and a C peer that linked `libomniORB` would be our code calling
# a fixture rather than an independent peer. `--check-independence` re-reads the
# link line and the source and fails if either has grown one, which is the
# negative control for the claim rather than a sentence asserting it.
#
# *C 컴파일러는 보장되지 않는다. 없으면 SKIPPED(2), 있는데 안 되면 실패(1) — 침묵은
# 없다. 링크 줄에 ORB가 없다는 것은 주장이 아니라 검사다.*

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
src="$here/c_peer.c"
out="${C_PEER_BIN:-$root/target/c_peer}"

check_only=0
[ "${1:-}" = "--check-independence" ] && check_only=1

# ── the independence check, run first so it gates the build ─────────────────
#
# Reads exit status before matching, and matches with a herestring rather than a
# pipe: CLAUDE.md measured 76 pipelines in this tree that lied two different
# ways, and the one that mattered was the licence boundary's own gate, which
# could not go red because finding a forbidden dependency is exactly when
# `grep -q` SIGPIPEs its producer.
if ! src_text="$(cat "$src" 2>&1)"; then
    echo "FAIL  cannot read $src: $src_text" >&2
    exit 1
fi

# The file's own header names omniORB in order to say it is NOT used, so the
# check looks at CODE lines only — block-comment bodies and line comments are
# stripped before matching. A check that could not tell the two apart would
# either fire on the explanation or be turned off, and both are worse than this.
# Code lines only — strip block-comment bodies and line comments before matching,
# so the header's own explanation of why omniORB is absent cannot trip its check.
code_only="$(python3 - "$src" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
text = re.sub(r"//[^\n]*", " ", text)
sys.stdout.write(text)
PY
)"
py_status=$?
if [ "$py_status" -ne 0 ]; then
    echo "FAIL  could not strip comments from $src to check it for ORB references" >&2
    exit 1
fi

if grep -nEi 'omniorb|omniidl|include *[<"]tao/|include *[<"]ace/|jacorb' <<<"$code_only" >/dev/null; then
    echo "FAIL  spikes/c_peer.c references an ORB in code, not only in prose:" >&2
    grep -nEi 'omniorb|omniidl|include *[<"]tao/|include *[<"]ace/|jacorb' <<<"$code_only" >&2
    echo "FAIL  the peer must be first-party C written from the OMG specification" >&2
    exit 1
fi

if [ "$check_only" = 1 ]; then
    echo "ok    spikes/c_peer.c includes no ORB header and links no ORB library"
    exit 0
fi

# ── the compiler, probed rather than assumed ────────────────────────────────
cc_bin="${CC:-}"
if [ -z "$cc_bin" ]; then
    for candidate in cc clang gcc; do
        if command -v "$candidate" >/dev/null 2>&1; then
            cc_bin="$candidate"
            break
        fi
    done
fi

if [ -z "$cc_bin" ] || ! command -v "$cc_bin" >/dev/null 2>&1; then
    echo "SKIPPED  no C compiler on this machine: tried \$CC, cc, clang, gcc" >&2
    echo "SKIPPED  the C peer is unmeasured here, which is not the same as passing" >&2
    exit 2
fi

cc_version="$("$cc_bin" --version 2>&1 | head -1)"

mkdir -p "$(dirname "$out")" || {
    echo "FAIL  cannot create $(dirname "$out")" >&2
    exit 1
}

# `-std=c99` so the peer cannot quietly depend on a compiler extension, and
# every warning on because a hand-written CDR encoder is exactly where a
# sign-extension or a truncating conversion turns into a wire defect.
# `-D_DEFAULT_SOURCE`/`-D_DARWIN_C_SOURCE` are what strict c99 needs for the
# POSIX socket declarations; neither pulls in anything but libc.
flags=(-std=c99 -O2 -Wall -Wextra -Wconversion -Wsign-conversion -Wshadow
       -Wpointer-arith -Wcast-qual -Wstrict-prototypes -Werror
       -D_DEFAULT_SOURCE -D_DARWIN_C_SOURCE -D_POSIX_C_SOURCE=200809L)

if ! build_err="$("$cc_bin" "${flags[@]}" -o "$out" "$src" 2>&1)"; then
    echo "FAIL  $cc_bin is present and spikes/c_peer.c did not compile:" >&2
    printf '%s\n' "$build_err" >&2
    exit 1
fi

# The link line, read back off the binary. A peer that had grown a dependency on
# somebody's ORB would show it here, and this is the check rather than the claim.
linked=""
if command -v otool >/dev/null 2>&1; then
    linked="$(otool -L "$out" 2>/dev/null | tail -n +2 | awk '{print $1}')"
elif command -v ldd >/dev/null 2>&1; then
    linked="$(ldd "$out" 2>/dev/null | awk '{print $1}')"
fi
if [ -n "$linked" ] && grep -Ei 'omniorb|libACE|libTAO|jacorb' <<<"$linked" >/dev/null; then
    echo "FAIL  the built peer links an ORB:" >&2
    printf '%s\n' "$linked" >&2
    exit 1
fi

echo "built with $cc_bin ($cc_version)"
if [ -n "$linked" ]; then
    echo "links:"
    printf '  %s\n' $linked
fi
echo "$out"
exit 0
