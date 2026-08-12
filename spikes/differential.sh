#!/usr/bin/env bash
# Differential conformance: our IDL front end against every deployed compiler
# we can get hold of.
#
# One oracle tells you whether you agree with omniORB. A second tells you
# something no amount of agreeing with the first can: where the *oracles*
# disagree with each other, which is where a corpus file does not mean the same
# thing to every deployed compiler.
#
# omniORB, TAO and JacORB are LGPL/GPL/DOC. They are invoked here as external
# programs whose text output we read, and are never linked into or shipped with
# Orbweaver. See docs/PLAN.md section 10.
#
# usage: differential.sh [--require omniidl,jacorb_idl,tao_idl]
#
# Absent oracles are SKIPPED and counted as unmeasured, except where --require
# names them, in which case absence is a failure. CI requires two; a laptop
# often has one.
#
# Written for bash 3.2, which is what macOS ships: no associative arrays.
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
# the Any and TypeCode files we have no use for. Unlike the other two it sets a
# non-zero exit status on error, which is the verdict.
tao_idl_verdict() {
  local out="$TMP/tao"
  rm -rf "$out"; mkdir -p "$out"
  tao_idl -Sa -St -o "$out" "$1" >/dev/null 2>&1
}

# JacORB's IDL compiler: a second deployed front end, independently written in
# Java, and already a project fixture as the second wire peer. Like omniidl it
# exits 0 on a parse error — measured, not assumed — so the verdict is again
# whether it wrote a diagnostic.
JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JLIB="$ROOT/spikes/jacorb/lib"
JIDL_CP="$JLIB/jacorb-idl-compiler.jar:$JLIB/jacorb-omgapi.jar:$JLIB/jacorb.jar:$JLIB/slf4j-api-1.7.36.jar"

have_jacorb_idl() {
  [ -x "$JH/bin/java" ] && [ -s "$JLIB/jacorb-idl-compiler.jar" ]
}

jacorb_idl_verdict() {
  local out="$TMP/jacorb" err
  rm -rf "$out"; mkdir -p "$out"
  err=$("$JH/bin/java" -cp "$JIDL_CP" org.jacorb.idl.parser -d "$out" "$1" 2>&1 >/dev/null)
  [ -z "$err" ]
}

have_oracle() {
  case "$1" in
    jacorb_idl) have_jacorb_idl ;;
    *) command -v "$1" >/dev/null 2>&1 ;;
  esac
}

ORACLES=""
for o in omniidl tao_idl jacorb_idl; do
  if have_oracle "$o"; then
    ORACLES="$ORACLES $o"
  elif requires "$o"; then
    echo "  FAIL required oracle '$o' is not available"
    fails=$((fails+1))
  else
    echo "  SKIPPED  $o absent — its column is unmeasured, not passing"
    skipped=$((skipped+1))
  fi
done
[ -n "$ORACLES" ] || { echo "  FAIL no oracle available at all"; exit $((fails+1)); }

cargo build -q --bin idl-check 2>/dev/null || { echo "  FAIL cannot build idl-check"; exit 1; }

# ── The recorded divergences ─────────────────────────────────────────────────
# A file where the oracles disagree is a finding. Some of those findings have
# been investigated and will not change — a compiler that is lax where the
# specification is strict does not become strict because we noticed. Those live
# in corpus/divergences.tsv with a reason, and are reported without failing.
DIVERGENCES="$ROOT/corpus/divergences.tsv"

# The verdict corpus/divergences.tsv records for this file and oracle, if any.
recorded_verdict() {
  [ -f "$DIVERGENCES" ] || return 1
  awk -F'\t' -v f="$1" -v o="$2" \
    '$1 == f && $2 == o { print $3; found = 1; exit } END { exit !found }' "$DIVERGENCES"
}
seen_keys=""

# ── The corpus, with the verdict each file is filed under ────────────────────
files_accept=$(ls corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl 2>/dev/null)
files_reject=$(ls corpus/negative/*.idl 2>/dev/null)

ours_wrong=""     # we disagree with the corpus, which the oracles uphold
not_portable=""   # the oracles disagree and nobody has explained why
known=""          # they disagree and corpus/divergences.tsv says why
checked=0

verdict_word() { if [ "$1" -eq 0 ]; then echo accept; else echo reject; fi; }

examine() {
  local f="$1" expected="$2"
  local base; base=$(basename "$f")
  checked=$((checked+1))

  local want; if [ "$expected" = accept ]; then want=0; else want=1; fi

  orbweaver_verdict "$f"; local us=$?

  # One pass over the oracles: the votes for the report, whether they agree
  # with each other, and whether each divergence is one we have on record.
  local votes="" agree=1 first="" unexplained=""
  for o in $ORACLES; do
    "${o}_verdict" "$f"; local v=$?
    local word; word=$(verdict_word $v)
    votes="$votes $o=$word"
    if [ -z "$first" ]; then first=$v; elif [ "$v" -ne "$first" ]; then agree=0; fi
    if [ "$v" -ne "$want" ]; then
      if [ "$(recorded_verdict "$base" "$o" 2>/dev/null)" = "$word" ]; then
        seen_keys="$seen_keys $base/$o"
      else
        unexplained="$unexplained $o"
      fi
    fi
  done

  if [ "$agree" -eq 0 ]; then
    if [ -z "$unexplained" ]; then
      known="$known|$base —$votes"
    else
      not_portable="$not_portable|$base —$votes (unexplained:$unexplained)"
    fi
  elif [ "$first" -ne "$want" ]; then
    # Every compiler contradicts the directory the file sits in. The fixture is
    # wrong, and silently agreeing with the compilers would have hidden it.
    not_portable="$not_portable|$base — filed as $expected, every oracle says$votes"
  fi

  if [ "$us" -ne "$want" ]; then
    ours_wrong="$ours_wrong|$base — we say $(verdict_word $us), filed as $expected, oracles$votes"
  fi
}

for f in $files_accept; do examine "$f" accept; done
for f in $files_reject; do examine "$f" reject; done

# ── Report ───────────────────────────────────────────────────────────────────
show() { printf '%s' "$1" | tr '|' '\n' | grep -v '^$' | sed 's/^/       /'; }
count() { printf '%s' "$1" | tr '|' '\n' | grep -cv '^$'; }

echo "  $checked file(s) through:$ORACLES + orbweaver"

if [ -z "$ours_wrong" ]; then
  echo "  ok   our front end matches the corpus everywhere the oracles uphold it"
else
  echo "  FAIL our front end is wrong on $(count "$ours_wrong") file(s):"
  show "$ours_wrong"
  fails=$((fails+1))
fi

n_oracles=$(echo $ORACLES | wc -w | tr -d ' ')
if [ -n "$not_portable" ]; then
  echo "  FAIL $(count "$not_portable") corpus file(s) diverge with no recorded reason:"
  show "$not_portable"
  echo "       record the reason in corpus/divergences.tsv, or fix the file"
  fails=$((fails+1))
elif [ "$n_oracles" -gt 1 ]; then
  echo "  ok   no unexplained divergence between $n_oracles independent front ends"
else
  # With one oracle there is no cross-compiler claim to make, only the weaker
  # one that each file got the verdict the corpus filed it under.
  echo "  ok   every file got the verdict its directory claims (one oracle: portability untested)"
fi

if [ -n "$known" ]; then
  echo "  note $(count "$known") recorded divergence(s), see corpus/divergences.tsv:"
  show "$known"
fi

# An exemption that no longer describes reality is worse than none: it silently
# covers whatever moves into its place.
if [ -f "$DIVERGENCES" ]; then
  stale=""
  while IFS=$'\t' read -r file oracle _verdict _reason; do
    case "$file" in ''|'#'*) continue ;; esac
    case " $ORACLES " in *" $oracle "*) ;; *) continue ;; esac   # oracle not run
    case "$seen_keys" in *"$file/$oracle"*) ;; *) stale="$stale|$file/$oracle" ;; esac
  done < "$DIVERGENCES"
  if [ -n "$stale" ]; then
    echo "  FAIL $(count "$stale") recorded divergence(s) no longer happen — delete the entry:"
    show "$stale"
    fails=$((fails+1))
  fi
fi

[ "$skipped" -eq 0 ] || echo "  $skipped oracle(s) unmeasured"
exit "$fails"
