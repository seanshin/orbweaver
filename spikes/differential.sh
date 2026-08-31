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
# usage: differential.sh [--require omniidl,jacorb_idl,tao_idl] [--record]
#
# Absent oracles are SKIPPED and counted as unmeasured, except where --require
# names them, in which case absence is a failure. CI requires two; a laptop
# often has one.
#
# `--record` writes what every file's oracles said to
# `corpus/differential-results.tsv`, which turns this run from an event into
# data. It is what makes "has this corpus file ever been through both front
# ends" answerable by something that has no oracle installed — see
# `crates/orbweaver-test/tests/every_corpus_file_met_both_front_ends.rs`, which
# is the gate, and `--record`'s own refusal below for why it insists on both.
#
# Written for bash 3.2, which is what macOS ships: no associative arrays.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

REQUIRED=""
RECORD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --require) REQUIRED="${2:-}"; shift 2 ;;
    --record) RECORD=1; shift ;;
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
#
# **The `if` is load-bearing and this function was wrong without it for as long
# as it had existed, which is exactly as long as no `tao_idl` was on this
# machine.** `examine` compares a verdict against `want`, which is 0 or 1, with
# `-ne` — an exact comparison, not a truthiness test — and TAO 4.0.7 exits **2**
# on a parse error. So every correct rejection read as a divergence: the first
# run with the oracle present reported **37 unexplained divergences**, of which
# the great majority were files all three front ends agree about, `tao_idl`
# among them. `agree` compares the raw values too, so `omniidl=1` beside
# `tao_idl=2` also read as disagreement.
#
# The other two verdict functions end in `[ -z "$err" ]` and are normalised to
# 0/1 by construction; this one ended in the command itself and leaked the
# compiler's own status into a protocol that has room for two values. *A branch
# written against an absent oracle owes a run with it present* — nothing here
# was ever executed until 2026-08-31.
tao_idl_verdict() {
  local out="$TMP/tao"
  rm -rf "$out"; mkdir -p "$out"
  #
  # **`--idl-version 4` because the corpus is IDL 4.2 and TAO defaults to 3.**
  # `tao_idl --default-idl-version` answers `3`, and under 3 an anonymous type
  # is an error — so `06-union.idl`, `23-moe-enterprise.idl` and
  # `30-const-type.idl` were reported as three divergences when what had
  # actually happened is that the oracle was asked a different question from
  # the one the corpus answers. An oracle configured for another version of the
  # specification is not a second opinion about this one.
  if tao_idl --idl-version 4 -Sa -St -o "$out" "$1" >/dev/null 2>&1; then return 0; else return 1; fi
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

# TAO is not on anybody's PATH by default and `brew` has no formula that ships
# `tao_idl`, so `spikes/tao/setup.sh` builds it in-tree. Look there as well as on
# PATH, and export what the binary needs to run — otherwise the fixture exists,
# the differential still reports `SKIPPED tao_idl absent`, and the only thing
# standing between them is whether somebody remembered to export four variables.
# That is this project's *a prohibition without its replacement* in fixture
# form: a check nobody can run without a ritual is a check that does not run.
ACE_LOCAL="$ROOT/spikes/tao/ACE_wrappers"

have_tao_idl() {
  command -v tao_idl >/dev/null 2>&1 && return 0
  [ -x "$ACE_LOCAL/bin/tao_idl" ] || return 1
  export ACE_ROOT="$ACE_LOCAL" TAO_ROOT="$ACE_LOCAL/TAO"
  export PATH="$ACE_LOCAL/bin:$PATH"
  export LD_LIBRARY_PATH="$ACE_LOCAL/lib:${LD_LIBRARY_PATH:-}"
  export DYLD_LIBRARY_PATH="$ACE_LOCAL/lib:${DYLD_LIBRARY_PATH:-}"
  command -v tao_idl >/dev/null 2>&1
}

have_oracle() {
  case "$1" in
    jacorb_idl) have_jacorb_idl ;;
    tao_idl) have_tao_idl ;;
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
#
# These directories are named literally, and the gate over the record —
# `crates/orbweaver-test/tests/every_corpus_file_met_both_front_ends.rs` —
# keeps the same list in its `ENUMERATED`. **Change one and change the other**:
# a directory in the script but not the gate is ungated, and a directory in
# neither is invisible to both while they agree with each other. That is what
# happened to `corpus/services/` — the contracts that exist to be served, which
# are the files a foreign ORB is most likely to compile — for as long as the
# directory existed. `ir-subset.idl` diverges, and the divergence could not even
# be *written down*: the staleness loop below fails any row naming a file this
# script never checked.
files_accept=$(ls corpus/golden/*.idl corpus/requirements/generated/*.idl \
  corpus/services/*.idl spikes/*.idl 2>/dev/null)
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

  # One line per file for `--record`, written whether or not anything diverged:
  # the record's job is to say the file *was measured*, and a file both oracles
  # agree about is exactly the one nobody would remember to write down.
  printf '%s\t%s\t%s\t%s\n' "$base" "$expected" "$(verdict_word $us)" \
    "$(printf '%s' "$votes" | sed 's/^ //; s/ /,/g')" >> "$TMP/records"
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

# ── The record ───────────────────────────────────────────────────────────────
# Only ever written with both front ends present. A record produced from one
# oracle would say a file was measured while nothing had asked the second, and
# the gate that reads it cannot tell the two apart — which is the hole this
# whole record exists to close, reopened one directory further along.
RESULTS="$ROOT/corpus/differential-results.tsv"
if [ "$RECORD" -eq 1 ]; then
  missing=""
  for o in omniidl jacorb_idl; do
    case " $ORACLES " in *" $o "*) ;; *) missing="$missing $o" ;; esac
  done
  if [ -n "$missing" ]; then
    echo "  FAIL --record needs both front ends and is missing:$missing"
    echo "       (omniidl: brew install omniorb · jacorb_idl: spikes/jacorb/setup.sh --jars-only)"
    fails=$((fails+1))
  else
    {
      echo "# What each corpus file's front ends said, written by"
      echo "# \`spikes/differential.sh --record\`. Generated: do not hand-edit."
      echo "#"
      echo "# This exists so that \"has this file ever been through both front ends\""
      echo "# can be answered by something with no oracle installed. The gate over it"
      echo "# is \`crates/orbweaver-test/tests/every_corpus_file_met_both_front_ends.rs\`,"
      echo "# and it checks **membership only** — that every file the differential"
      echo "# enumerates has a row and every row names a file that still exists. It"
      echo "# does not check that the verdicts below are today's; the differential"
      echo "# itself is the only thing that can, and it rewrites this file whole."
      echo "#"
      echo "# 오라클이 설치되지 않은 곳에서도 \"이 파일이 두 프런트엔드를 거쳤는가\"를"
      echo "# 답할 수 있게 하는 기록이다. 게이트는 **소속만** 검사한다 — 아래 판정이"
      echo "# 오늘의 것인지는 differential 자신만 말할 수 있고, 매번 전체를 다시 쓴다."
      echo "#"
      echo "# Columns: file <TAB> filed as <TAB> our verdict <TAB> each oracle's verdict"
      echo "# Measured: $(date +%Y-%m-%d) · oracles:$ORACLES"
      echo
      sort "$TMP/records"
    } > "$RESULTS"
    echo "  ok   recorded $checked file(s) in corpus/differential-results.tsv"
  fi
fi

[ "$skipped" -eq 0 ] || echo "  $skipped oracle(s) unmeasured"
exit "$fails"
