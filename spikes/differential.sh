#!/usr/bin/env bash
# Differential conformance: our IDL front end against every deployed compiler
# we can get hold of.
#
# One oracle tells you whether you agree with omniORB. Two tell you something
# more useful — where the *oracles* disagree with each other, which is where a
# corpus file is not portable and no amount of agreeing with one of them helps.
#
# omniORB and TAO are LGPL/GPL/DOC. They are invoked here as external programs
# whose text output we read, and are never linked into or shipped with
# Orbweaver. See docs/PLAN.md section 10.
#
# usage: differential.sh [--require omniidl,tao_idl]
#
# Absent oracles are SKIPPED and counted as unmeasured, except where --require
# names them, in which case absence is a failure. CI requires both; a laptop
# usually has one.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

REQUIRED=""
while [ $# -gt 0 ]; do
  case "$1" in
    --require) REQUIRED="${2:-}"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
requires() { case ",$REQUIRED," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }

fails=0
skipped=0
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# ── The front ends ───────────────────────────────────────────────────────────
# Each takes a file and returns 0 for accept, 1 for reject.

orbweaver_verdict() {
  "$ROOT/target/debug/idl-check" "$1" >/dev/null 2>&1
}

# omniidl exits 0 even for some diagnostics, so the verdict is whether it wrote
# anything to stderr. Captured to a variable rather than piped: `grep -q` closes
# the pipe on first match and SIGPIPEs the compiler, which has produced a
# phantom pass in this project before.
omniidl_verdict() {
  local err
  err=$(omniidl -b dump "$1" 2>&1 >/dev/null)
  [ -z "$err" ]
}

# tao_idl writes C++ stubs, so it needs somewhere to put them; -Sa -St suppress
# the Any and TypeCode files we have no use for. Unlike omniidl it sets a
# non-zero exit status on error, which is the verdict.
tao_idl_verdict() {
  local out="$TMP/tao"
  rm -rf "$out"; mkdir -p "$out"
  tao_idl -Sa -St -o "$out" "$1" >/dev/null 2>&1
}

ORACLES=""
for o in omniidl tao_idl; do
  if command -v "$o" >/dev/null 2>&1; then
    ORACLES="$ORACLES $o"
  elif requires "$o"; then
    echo "  FAIL required oracle '$o' is not installed"
    fails=$((fails+1))
  else
    echo "  SKIPPED  $o absent — its column is unmeasured, not passing"
    skipped=$((skipped+1))
  fi
done
[ -n "$ORACLES" ] || { echo "  FAIL no oracle available at all"; exit $((fails+1)); }

cargo build -q --bin idl-check 2>/dev/null || { echo "  FAIL cannot build idl-check"; exit 1; }

# ── The corpus, with the verdict each file is supposed to get ────────────────
files_accept=$(ls corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl 2>/dev/null)
files_reject=$(ls corpus/negative/*.idl 2>/dev/null)

# Counters, and the two kinds of finding this script exists to separate.
declare -a ours_wrong=()      # we disagree with a consensus of the oracles
declare -a not_portable=()    # the oracles disagree with each other
checked=0

verdict_word() { [ "$1" -eq 0 ] && echo accept || echo reject; }

examine() {
  local f="$1" expected="$2"
  checked=$((checked+1))

  orbweaver_verdict "$f"; local us=$?
  local votes="" disagreement=0 first=""
  for o in $ORACLES; do
    "${o}_verdict" "$f"; local v=$?
    votes="$votes $o=$(verdict_word $v)"
    if [ -z "$first" ]; then first=$v; elif [ "$v" -ne "$first" ]; then disagreement=1; fi
  done

  if [ "$disagreement" -eq 1 ]; then
    # No consensus to be measured against: the file itself is the problem.
    not_portable+=("$(basename "$f") —$votes")
    return
  fi
  if [ "$us" -ne "$first" ]; then
    ours_wrong+=("$(basename "$f") — we $(verdict_word $us), oracles$votes")
    return
  fi
  # The corpus's own claim about the file, checked against the consensus rather
  # than assumed: a golden file every compiler rejects is a broken fixture, and
  # silently agreeing with the oracles would hide it.
  local want; want=$([ "$expected" = accept ] && echo 0 || echo 1)
  if [ "$first" -ne "$want" ]; then
    not_portable+=("$(basename "$f") — filed as $expected, oracles$votes")
  fi
}

for f in $files_accept; do examine "$f" accept; done
for f in $files_reject; do examine "$f" reject; done

# ── Report ───────────────────────────────────────────────────────────────────
echo "  $checked file(s) through:$ORACLES + orbweaver"

if [ ${#ours_wrong[@]} -eq 0 ]; then
  echo "  ok   our front end agrees with every oracle on every file"
else
  echo "  FAIL our front end disagrees with the oracles on ${#ours_wrong[@]} file(s):"
  printf '       %s\n' "${ours_wrong[@]}"
  fails=$((fails+1))
fi

n_oracles=$(echo $ORACLES | wc -w | tr -d ' ')
if [ ${#not_portable[@]} -eq 0 ] && [ "$n_oracles" -gt 1 ]; then
  echo "  ok   the oracles agree with each other, so the corpus is portable"
elif [ ${#not_portable[@]} -eq 0 ]; then
  # With one oracle there is no cross-compiler claim to make, only the weaker
  # one that each file got the verdict the corpus filed it under.
  echo "  ok   every file got the verdict its directory claims (one oracle: portability untested)"
else
  echo "  FAIL ${#not_portable[@]} corpus file(s) are not portable across compilers:"
  printf '       %s\n' "${not_portable[@]}"
  fails=$((fails+1))
fi

[ "$skipped" -eq 0 ] || echo "  $skipped oracle(s) unmeasured"
exit "$fails"
