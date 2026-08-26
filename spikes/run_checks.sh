#!/usr/bin/env bash
# Project check harness: every Phase 0 feasibility assumption plus the
# Phase 1 wire and licence checks. Exit code is the verdict.
#
# Named run_checks.sh until Phase 1 outgrew it.
#
# The omniORB fixture is LGPL/GPL and is used only as a wire peer and a
# conformance oracle. Nothing here links it into Orbweaver. See docs/PLAN.md §10.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
fail_total=0
skipped=0
# Of the SKIPPED groups, how many stood a RECORDING of another day in the place
# of a live run. `skipped` keeps counting every skip, exactly as D010 §2
# requires; this counts the subset that made a claim from a recording, because
# "no fixture here" and "a measurement of twelve days ago" are different claims
# and the verdict line used to print them as one.
replays=0

# ── One harness at a time, machine-wide ──────────────────────────────────────
# The fixtures are killed by pattern (`pkill -f echo_server.py`) and the logs
# live at fixed /tmp paths, so two harnesses running at once destroy each
# other's fixtures and report failures that are about the scheduling, not the
# code. That has now happened twice — once in a worktree agent's run and once
# in the main tree, both times producing "Connection refused" against a peer
# that had been alive a moment earlier, and both times costing a diagnosis.
#
# Two fixes, because they cover different attackers. The lock stops a second
# harness; `fkill` below stops this harness from killing a fixture somebody
# started by hand in another checkout, which the lock cannot see. Neither
# touches the shared /tmp log paths, which are threaded through 46 places and
# are only a hazard for two concurrent harnesses — the case the lock removes.
#
# Refuse rather than queue: a harness that silently waits looks identical to a
# harness that hung, and the person who started the second one wants to know
# the first is running.
LOCK=/tmp/orbweaver-harness.lock
if ! mkdir "$LOCK" 2>/dev/null; then
  holder=$(cat "$LOCK/owner" 2>/dev/null || echo "unknown")
  # A holder that is gone is a crashed run, not a running one. Taking the lock
  # over is safe and refusing would make one killed harness wedge the machine
  # until somebody read this file — the failure mode of every lock that only
  # ever waits.
  holder_pid=$(printf '%s' "$holder" | awk '{print $2}')
  if [ -n "$holder_pid" ] && ! ps -p "$holder_pid" >/dev/null 2>&1; then
    echo "note: taking over a stale lock from a run that is no longer alive ($holder)"
    rm -rf "$LOCK"
    mkdir "$LOCK" 2>/dev/null || true
  fi
fi
if [ ! -d "$LOCK" ] || [ -s "$LOCK/owner" ]; then
  holder=$(cat "$LOCK/owner" 2>/dev/null || echo "unknown")
  echo "another harness is running (started by $holder)."
  echo "the fixtures are killed by pattern and the logs share /tmp paths, so two"
  echo "runs at once produce failures that are about the scheduling, not the code."
  echo "wait for it, or remove $LOCK if you are sure nothing is running."
  exit 2
fi
printf 'pid %s in %s at %s\n' "$$" "$ROOT" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >"$LOCK/owner"
# NOT a `trap ... EXIT` of its own: `cleanup` claims EXIT further down, and a
# second trap on the same signal REPLACES the first rather than adding to it.
# Releasing the lock is therefore folded into `cleanup`. This is not
# hypothetical tidiness — the first version of this lock did use its own trap,
# every run leaked a stale lock, and the next run refused to start.

# ── The dimension this harness did not have ──────────────────────────────────
#
# Every one of the groups below answers *did this break*. None of them answers
# *what can a caller still tell* — D029 §6's priority-zero criterion — and until
# today that question was answered by reading batch reports with a `grep`, which
# is a reading, not a measurement. D031 H1/H2.
#
# Two pieces, and neither of them is a score. `bears_on` lets a group declare
# which transparency it bears on, validated against the ONE place those names
# live; the ledger before the verdict reads what actually ran and prints, per
# transparency, how many groups measured it, how many went red, and what is
# named unmeasured. **The last column is the load-bearing one** — it is what a
# next batch is scoped from — and a transparency nothing declares prints as
# UNMEASURED, never as absence of bad news.
#
# What is deliberately NOT here: a percentage, a completed-of-five count, or any
# figure that could be quoted in prose as progress. `A floor is not a figure`
# and a completion percentage is its worst form — it moves when a group is
# added, and it is wrong the moment a leak is FOUND rather than closed, which is
# what finding a leak is. The verdict line therefore names the unmeasured
# transparencies instead of counting the measured ones.
#
# The five names are NOT written in this file. `spikes/transparency.py` reads
# them out of D029 §6.1, which owns them; a retyped list here would be
# `a classifier is a sentence too` in shell, and would go quiet the day §6.1
# changed. A tag naming something §6.1 does not is a FAILURE naming the group
# and the bad name — the `dk_peer` lesson, where the expected table was checked
# against the peer's own enum before any leg ran, so a typo failed as our table.
TP_DOC="docs/decisions/D029-what-a-complete-orb-would-mean.md"
TP_NAMES=$(python3 spikes/transparency.py --names 2>/dev/null)
tp_load_rc=$?
tp_load_err=0
if [ "$tp_load_rc" -ne 0 ] || [ -z "$TP_NAMES" ]; then tp_load_err=1; fi

TP_GIDX=0            # which group we are inside, 1-based, in file order
TP_GROUP_TITLE=""    # its `hr` title, so a diagnostic can name it
TP_GROUPS=""         # idx \t title
TP_TAGS=""           # transparency \t idx
TP_RED=""            # idx \t how many failures that group added
TP_SKIPS=""          # idx \t the skip's own first line, or empty
tp_fail_at_start=0
tp_skip_text=""

# A group's verdict is a DELTA, not a flag: every group already reports by
# adding to `fail_total`, and asking 81 groups to also set a variable would be
# 81 chances to forget. So the close-out runs when the next `hr` starts and
# reads what the previous group added. No group's own verdict changes, which is
# D031 §2's third refusal — the ledger reads the run, it does not replace it.
tp_close_group() {
  local d
  [ -z "$TP_GROUP_TITLE" ] && return 0
  d=$((fail_total - tp_fail_at_start))
  if [ "$d" -gt 0 ]; then
    TP_RED="${TP_RED}${TP_GIDX}	${d}
"
  fi
  return 0
}

hr() {
  tp_close_group
  TP_GIDX=$((TP_GIDX+1))
  TP_GROUP_TITLE="$1"
  TP_GROUPS="${TP_GROUPS}${TP_GIDX}	$1
"
  tp_fail_at_start=$fail_total
  printf '\n\033[1m%s\033[0m\n' "$1"
}

#   bears_on <name>
# Declared by a group immediately after its `hr`. Declaring nothing is normal
# and most groups do; declaring a name §6.1 does not have is a failure.
bears_on() {
  local name="$1"
  if [ "$tp_load_err" = 1 ]; then
    echo "  FAIL bears_on $name: the transparency names could not be read from"
    echo "       $TP_DOC §6.1, so this group's claim is unvalidated — and an"
    echo "       unvalidated claim is an unmeasured check, which is a failure"
    fail_total=$((fail_total+1))
    return 0
  fi
  if ! grep -qx -- "$name" <<<"$TP_NAMES"; then
    echo "  FAIL group \"$TP_GROUP_TITLE\" declares bears_on \"$name\","
    echo "       which is not one of the transparencies $TP_DOC §6.1 names:"
    echo "       $(tr '\n' ' ' <<<"$TP_NAMES")"
    echo "       fix the tag, or change §6.1 first — the names have one home"
    echo "       and this file is not it"
    fail_total=$((fail_total+1))
    return 0
  fi
  TP_TAGS="${TP_TAGS}${name}	${TP_GIDX}
"
  return 0
}

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }

# Whether something is listening, without needing lsof. The probe used to be
# `lsof -nP -iTCP:2809`, which is absent on a stock CI runner — so the check
# could not tell "nothing is listening" from "I cannot look", and reported the
# first. bash's /dev/tcp needs no package. Two groups need it, so it lives here
# rather than inside whichever one happened to want it first.
port_open() { (exec 3<>/dev/tcp/127.0.0.1/"$1") >/dev/null 2>&1; }

# ── What a diagnostic print owes when the failure is not the shape it expected ─
#
# Measured 2026-08-25: the concurrent-dispatch group printed `FAIL run 5 of 5`
# and then SIX LINES CONTAINING NO INFORMATION, because its extract was
# `grep -A3 "^failures:" | head -6` and cargo's first `failures:` section is
# followed by a blank line and a header. A group whose whole argument is *"one
# green run is not evidence"* produced a red run that was not evidence either.
#
# That group was repaired in place, and the shape was then swept: **94 more
# extracts in this file could print nothing** — 58 of them pattern-filtered
# (`grep FAIL`, `grep -A3 panicked`, `grep -E "left:|right:"`), which is empty
# for every failure that is not the one anticipated, and 36 of them a `tail` of
# a capture that is empty when the producer never printed at all. One of those
# 36 carries a comment recording the day it fired with nothing to read.
#
# The previous sweep counted 73 of these, because it counted the EARLY EXIT ON A
# PIPE. That is the wrong boundary: `grep "no response|closed|error" <<<"$x" |
# sed` has no early exit and prints nothing just as completely, and a `tail` of
# an empty capture prints one blank line. Scoped to the rule instead of to the
# form, the class is 94 — and it does not stop at `FAIL` branches either.
# Twelve more sites print an empty value under an **ok**: `ok   codeset
# negotiated with a second peer:` followed by nothing is the instance this file
# already repaired once, and its own comment at that site says so. Those twelve
# are repaired here too, with their verdicts deliberately unchanged: turning an
# `ok` that measured nothing into a `FAIL` is a different decision from making
# it say so, and only the second one is a diagnostics change.
#
# So the decision of what to say when an extract is empty lives HERE, once,
# rather than being retyped ninety-four times with ninety-four wordings — the
# `pub(crate)` lesson from CLAUDE.md, in shell. Three things are owed:
#   1. when the expected shape is absent, SAY SO and show the tail instead;
#   2. no early-exit form on a pipe (`head`, `grep -q`, `grep -m1`) — the
#      extract is a herestring in and a herestring out, so nothing can SIGPIPE
#      a producer and nothing can hand `pipefail` a status to misread;
#   3. bounded, because an unbounded dump buries the next group.
#
#   diag <what-was-looked-for> <whole-output> <extract> [keep] [tail]
#     $1  what the extract looked for, in words, for the fallback sentence
#     $2  the whole output the extract came from
#     $3  the extract, which is allowed to be empty — that is the point
#     $4  lines of the extract to print   (default 6)
#     $5  lines of tail to print instead  (default 8)
diag() {
  local looked="$1" whole="$2" got="$3" keep="${4:-6}" tailn="${5:-8}"
  if [ -n "$got" ]; then
    sed -n "1,${keep}p" <<<"$got" | sed 's/^/       | /'
  elif [ -n "$whole" ]; then
    echo "       (no $looked in the output — last $tailn line(s) instead:)"
    tail -"$tailn" <<<"$whole" | sed 's/^/       | /'
  else
    echo "       (it printed nothing at all)"
  fi
  return 0
}

# The same rule where the intent is "show me some of it" rather than "find the
# interesting lines". A slice of an empty capture is one blank line, which reads
# as a diagnostic that ran and found nothing worth saying rather than as a
# producer that never said anything — and those are different failures.
#   diag_out <whole-output> [lines] [head|tail]
diag_out() {
  local whole="$1" n="${2:-8}" from="${3:-tail}"
  if [ -z "$whole" ]; then
    echo "       (it printed nothing at all)"
  elif [ "$from" = head ]; then
    sed -n "1,${n}p" <<<"$whole" | sed 's/^/       | /'
  else
    tail -"$n" <<<"$whole" | sed 's/^/       | /'
  fi
  return 0
}

# And the file half: a log a fixture may never have written to. `-s` is the
# whole check, because a log that does not exist and a log that exists empty are
# one absence to a reader and both used to print as silence. This is the shape
# `fixture_died` has had since Phase 0 — it is the only diagnostic in this file
# that always said "the fixture wrote nothing at all" — generalised so the other
# eight log dumps can have it too.
#   diag_log <path> [lines] [head|tail]
diag_log() {
  local p="$1" n="${2:-8}" from="${3:-tail}"
  if [ ! -s "$p" ]; then
    echo "       ($p is empty or absent — the producer wrote nothing at all)"
  elif [ "$from" = head ]; then
    sed -n "1,${n}p" "$p" | sed 's/^/       | /'
  else
    tail -"$n" "$p" | sed 's/^/       | /'
  fi
  return 0
}

# ── How old is a SKIPPED? ────────────────────────────────────────────────────
#
# D010 §2 already makes every skip a counted group naming its fixture, and that
# works. What none of them said is WHEN the claim was last true, so a skip that
# is eleven days old and one that is eleven months old print the same line and
# decay at the same invisible rate. D026 §5's S4.
#
# Every date printed below is COMPUTED, never typed. A date literal in a shell
# string is `A floor is not a figure` in its purest form — right on the day it
# is written, silently wrong every day after, and nothing recompiles a sentence.
# There were two such literals here (the NAT second-host probe's
# "last measured 2026-08-14" and the SSLIOP residue's prose) and they are gone.
#
# Where no date can be computed the line says `date not recorded` rather than
# inventing one. That is the honest answer for a fixture that lives outside the
# tree entirely — omniNames, an OIDC issuer, docker.
days_since() {
  python3 -c 'import sys,datetime
d=datetime.date.fromisoformat(sys.argv[1])
print((datetime.date.today()-d).days)' "$1" 2>/dev/null
}
# The tree's own date for a file. Read as "the last time this fixture changed",
# which is NOT the same as "the last time it was measured" and is never labelled
# as if it were: a probe edited without being re-run moves this date and
# overstates its own freshness. It is a decay clock with a stated limit, and a
# stated limit beats a literal that only ever understates.
git_date() { git -C "$ROOT" log -1 --format=%cs -- "$1" 2>/dev/null; }

#   skip <kind> <datespec> <line> [line...]
#     kind      absent  the fixture is not here and nothing stood in for it
#               replay  a RECORDING of a specific day stood in for a live run
#     datespec  ""              nothing in the tree can date this claim
#               @YYYY-MM-DD     a date read out of a recording's own stamp
#               git:<path>      the tree's date for the fixture that would run it
#     line...   the group's own text, one argument per line, naming its fixture
skip() {
  local kind="$1" spec="$2"; shift 2
  tp_skip_text="$1"   # the group's own first line, for the ledger to cite
  echo "  SKIPPED  $1"; shift
  while [ "$#" -gt 0 ]; do echo "           $1"; shift; done
  skip_age "$kind" "$spec"
}

# The count and the age belong to this file; the TEXT does not always. Nine
# skips are announced by the script being run rather than by the harness — the
# five capture probes, `differential.sh`, `perm_fallback.sh` and the three
# JacORB wide-text scripts each print their own `  SKIPPED` line, and printing
# a second one here would be two spellings of one fact. So those call this
# directly and add only the age.
#   skip_age <kind> <datespec>
skip_age() {
  local kind="$1" spec="$2" when="" whence="" ago=""
  case "$spec" in
    @*)    when="${spec#@}"; whence="last measured" ;;
    git:*) when=$(git_date "${spec#git:}")
           whence="not measured in this run; ${spec#git:} last changed" ;;
  esac
  if [ -n "$when" ]; then
    ago=$(days_since "$when")
    echo "           age: $whence $when${ago:+, $ago day(s) ago}"
  else
    echo "           age: date not recorded"
  fi
  skipped=$((skipped+1))
  [ "$kind" = replay ] && replays=$((replays+1))
  # The ledger's unmeasured column cites the group's own words rather than
  # inventing a second wording for the same absence. Nine skips are announced by
  # the script being run and call `skip_age` directly, so there is no text to
  # cite; the ledger says so rather than printing an empty reason.
  TP_SKIPS="${TP_SKIPS}${TP_GIDX}	${kind}	${tp_skip_text}
"
  tp_skip_text=""
  return 0
}

# ── Kill this run's fixtures, and only this run's ────────────────────────────
# `pkill -f echo_server.py` matches by command line, which is every checkout on
# the machine. The lock above stops two harnesses colliding; this stops a
# harness from killing a fixture a developer started by hand in another tree,
# which the lock cannot see.
#
# Scoped by process group: every fixture is started by this script, so it
# inherits this script's group. When the group cannot be read — a runner that
# reparents children, a `ps` without `pgid` — the fall-back is the old
# behaviour with a printed note, because a harness that silently stops killing
# fixtures leaks them into the next group and fails somewhere unrelated. A
# noisy wide kill beats a quiet leak.
own_pgid=$(ps -o pgid= -p $$ 2>/dev/null | tr -d ' ')
fkill() {
  local pat="$1" pid pgid hit=0 seen=0
  for pid in $(pgrep -f "$pat" 2>/dev/null); do
    seen=1
    pgid=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -n "$own_pgid" ] && [ "$pgid" = "$own_pgid" ]; then
      kill "$pid" 2>/dev/null && hit=1
    fi
  done
  if [ "$seen" = 1 ] && [ "$hit" = 0 ]; then
    echo "  note fixture $pat is running outside this process group; killing it wide" >&2
    pkill -f "$pat" >/dev/null 2>&1 || true
  fi
  return 0
}
need omniidl
need cargo

# Kills the fixture and waits for it to actually be gone. Signalling is
# asynchronous, so returning early lets the next fixture race a dying process.
cleanup() {
  fkill echo_server.py
  fkill evolution_server.py
  for _ in $(seq 1 50); do
    pgrep -f "echo_server.py|evolution_server.py" >/dev/null 2>&1 || return 0
    sleep 0.1
  done
}

# Starts the contract-evolution peer. `$1` is empty for the deployed version or
# --updated for the same service after an additive release.
start_evolution_server() {
  cleanup
  rm -f "$ROOT/spikes/evolution.ior"
  ( cd "$ROOT/spikes" && exec python3 evolution_server.py ${1:+"$1"} \
      >/tmp/orbweaver-evolution.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/evolution.ior" ] && { sleep 0.2; return 0; }
    sleep 0.1
  done
  fixture_died "evolution fixture did not publish an IOR within 10s" \
    /tmp/orbweaver-evolution.log
  return 1
}
release_lock() { rm -rf "$LOCK"; }
trap 'cleanup; release_lock' EXIT

# Waits for the fixture to actually publish an IOR.
#
# The wait must sleep. An earlier version spun without sleeping, which took
# microseconds and therefore did not wait at all; it only looked correct
# because `cargo run` had to compile first and accidentally covered the race.
# Once the build was warm the race surfaced as phantom GIOP timeouts.
# Prints why a fixture did not come up. Discarding its output made "did not
# publish an IOR" the only thing the harness could ever say, which is a
# measurement of the symptom and not of the cause — on a CI runner, where the
# fixture cannot be started by hand, that is the difference between a diagnosis
# and a guess.
fixture_died() {
  echo "  FAIL $1"
  if [ -s "$2" ]; then
    echo "       last output from the fixture:"
    tail -12 "$2" | sed 's/^/       | /'
  else
    echo "       the fixture wrote nothing at all"
  fi
}

start_server() {
  cleanup
  rm -f "$ROOT/spikes/echo.ior"
  ( cd "$ROOT/spikes" && exec python3 echo_server.py "$@" >/tmp/orbweaver-fixture.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/echo.ior" ] && { sleep 0.2; return 0; }  # settle after publish
    sleep 0.1
  done
  fixture_died "fixture did not publish an IOR within 10s" /tmp/orbweaver-fixture.log
  return 1
}

# Starts OUR server. Distinct from start_server, which launches the omniORB
# fixture; conflating the two silently pointed a check at the wrong process.
JH_CHECK=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP_CHECK="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"

start_rust_server() {
  fkill spike-server
  rm -f "$ROOT/spikes/server.ior"
  ( cd "$ROOT" && exec cargo run -q --bin spike-server -- spikes/server.ior 127.0.0.1 0 \
      >/tmp/orbweaver-srv.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; return 0; }
    sleep 0.1
  done
  echo "  FAIL our server did not publish an IOR"
  return 1
}

# ── Formatting ───────────────────────────────────────────────────────────────
# CI has checked this since the first workflow and this file never did, so
# "landed through the harness" did not include formatting — and on 2026-08-25
# that difference cost a red CI on a push whose local harness had said
# `all measured checks green`. Every agent in that day's wave was *required*
# to run `cargo fmt --check`; the coordinator, who wrote the requirement, did
# not run it on the batch he was landing himself.
#
# The rule this file already states about SKIPPED groups applies to gates too:
# a check that lives only in CI is a check this verdict does not cover, and a
# verdict that does not say so reads as coverage.
hr "formatting"
fmt_out=$(cargo fmt --all --check 2>&1); fmt_rc=$?
if [ "$fmt_rc" -eq 0 ]; then
  echo "  ok   cargo fmt --all --check"
else
  echo "  FAIL cargo fmt --all --check (exit $fmt_rc)"
  diag "a 'Diff in' line" "$fmt_out" "$(grep -E "^Diff in " <<<"$fmt_out")" 8
  fail_total=$((fail_total+1))
fi

# ── Unit tests ───────────────────────────────────────────────────────────────
hr "unit tests (CDR + GIOP)"
# Captured, then matched — and the producer's own exit status is read first.
#
# This was `cargo test --workspace --quiet 2>&1 | grep -q "^error"` until
# 2026-08-25, and under this file's own `set -o pipefail` (line 9) that gate
# **could not report a failing test suite**: a pipeline's status is its
# rightmost non-zero exit, so a failing `cargo test` made the pipeline
# non-zero, the `if` took the *else* branch, and the harness printed
# `ok cargo test --workspace` over a red workspace. The FAIL branch required
# `cargo test` to **pass** while printing a line starting with `error`.
# Reproduced before repair:
#   $ bash -c 'set -uo pipefail; f(){ echo "error: x"; return 101; };
#              if f | grep -q "^error"; then echo THEN; else echo ELSE; fi'
#   ELSE
# CLAUDE.md's rule — never pipe into `grep -q` when the producer matters —
# names the SIGPIPE half of this; `pipefail` is a second, independent way the
# same pipeline lies, and it is the half that silenced this gate.
ut_out=$(cargo test --workspace --quiet 2>&1); ut_rc=$?
if [ "$ut_rc" -ne 0 ] || grep -q "^error" <<<"$ut_out"; then
  echo "  FAIL cargo test --workspace (exit $ut_rc)"
  diag "an error line or a FAILED suite" "$ut_out" \
       "$(grep -E "^(error|test result: FAILED|failures:)" <<<"$ut_out")" 12
  fail_total=$((fail_total+1))
else
  echo "  ok   cargo test --workspace"
fi

# ── Lint (runs before the oracle, on purpose) ────────────────────────────────
hr "licence boundary"
# The non-negotiable rule of this project, and until 2026-08-25 **its gate
# could not go red**. `cargo tree --workspace | grep -qiE …` under `pipefail`:
# `grep -q` exits on its first match and SIGPIPEs the producer, whose status
# becomes 141, so the pipeline is non-zero exactly when the forbidden name IS
# present — and the `if` took the else branch and printed `ok`. Reproduced
# before repair with a long producer:
#   $ bash -c 'set -uo pipefail; g(){ seq 1 200000 | sed "s/^/omniorb /"; };
#              if g | grep -qiE "omniorb|jacorb"; then echo THEN; else echo ELSE; fi'
#   ELSE
# A short producer fits the pipe buffer and never sees SIGPIPE, which is why
# this passed every hand-check anyone ever gave it.
tree_out=$(cargo tree --workspace 2>/dev/null); tree_rc=$?
if [ "$tree_rc" -ne 0 ]; then
  echo "  FAIL cargo tree did not run (exit $tree_rc) — the licence boundary was NOT measured"
  fail_total=$((fail_total+1))
elif grep -qiE "omniorb|jacorb" <<<"$tree_out"; then
  echo "  FAIL an ORB fixture has become a dependency"
  sed -n '1,5p' <<<"$(grep -inE "omniorb|jacorb" <<<"$tree_out")" | sed 's/^/    | /'
  fail_total=$((fail_total+1))
else
  echo "  ok   no ORB fixture appears in cargo tree"
fi
# NOTICE promises that --no-default-features drops encoding_rs and the
# BSD-3-Clause obligation with it. That promise is testable, so it is tested.
nodef_out=$(cargo tree -p orbweaver-giop --no-default-features 2>/dev/null); nodef_rc=$?
if [ "$nodef_rc" -ne 0 ]; then
  echo "  FAIL cargo tree --no-default-features did not run (exit $nodef_rc) — NOTICE's promise was NOT measured"
  fail_total=$((fail_total+1))
elif grep -q encoding_rs <<<"$nodef_out"; then
  echo "  FAIL --no-default-features still pulls encoding_rs; NOTICE is wrong"
  fail_total=$((fail_total+1))
else
  echo "  ok   --no-default-features drops encoding_rs, as NOTICE states"
fi
# -D warnings, because this configuration is only built here: a helper that
# became dead once encoding_rs was gone sat un-noticed behind an exit-status
# check that warnings cannot fail.
if RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop --lib --no-default-features --quiet \
     >/dev/null 2>&1; then
  echo "  ok   the attribution-free build still passes its tests, warning-free"
else
  echo "  FAIL the attribution-free build does not build cleanly or does not test"
  fail_total=$((fail_total+1))
fi

hr "union case labels — a peer's bytes, not only our own"
# Our encode and decode agreed with each other in any byte order, so 1200 tests
# stayed green while a long long discriminated union from omniORB could not be
# decoded at all. The recording in the giop test is the regression case; this
# group checks the recording still describes what the live peer writes, because
# a recording nobody re-takes is a claim about the past.
ulc_out=$(python3 spikes/union_label_capture.py 2>&1); ulc_rc=$?
printf '%s\n' "$ulc_out" | sed 's/^  /  /'
if [ "$ulc_rc" -eq 2 ]; then
  skip_age absent git:spikes/union_label_capture.py
elif [ "$ulc_rc" -ne 0 ]; then
  echo "  FAIL the recorded peer bytes no longer match the live peer"
  fail_total=$((fail_total+1))
fi
if cargo test -q -p orbweaver-giop --test union_labels_from_a_peer >/dev/null 2>&1; then
  echo "  ok   the recorded union TypeCodes decode, and re-encode to the peer's bytes"
else
  echo "  FAIL a union TypeCode from a little-endian peer does not round-trip"
  fail_total=$((fail_total+1))
fi
# The default member's label (3a061d8): a bare `default:` used to write NO
# label bytes, so any `any` carrying golden 06's WithDefault went out
# malformed — our own decode, omniORB and JacORB all refused it, and nothing
# was red because every gate ran both ends through one encoder. Now the
# discriminator's width of zeros, ignored on read (§9.3.5.1.4: the value "has
# no semantic significance") — the only form omniORB (writes an unused value,
# ignores it) and JacORB (writes zeros, reads one octet that must be 0) both
# accept in both byte orders. Seventeen omniORB captures retaken live here
# (nine for the label, eight for the member list).
# Negative control: with typecode.rs's change stashed, the registry test
# fails 14 times ("implausible CDR length prefix", both orders) and the peer
# test 3 of 4 (ours 4–8 bytes shorter than the recording).
udc_out=$(python3 spikes/union_default_capture.py 2>&1); udc_rc=$?
printf '%s\n' "$udc_out"
if [ "$udc_rc" -eq 2 ]; then
  skip_age absent git:spikes/union_default_capture.py
elif [ "$udc_rc" -ne 0 ]; then
  echo "  FAIL the recorded default-label bytes no longer match the live peer"; fail_total=$((fail_total+1))
fi
# R18 (40a4729): whatever a third peer writes in the default label slot — 42
# hand-built labels (MAX/MIN/all-ones/colliding/invalid, six discriminator
# kinds, both orders) decode == the zero-label shape and 304 values round-trip
# under them. Negative controls in that commit: reject a colliding label ->
# 3 red; keep the label -> 5 red; "must be zero" -> every recording red.
if cargo test -q -p orbweaver-giop --test union_default_label_from_a_peer >/dev/null 2>&1 \
   && cargo test -q -p orbweaver-registry --test union_default_round_trip >/dev/null 2>&1 \
   && cargo test -q -p orbweaver-dynamic --test union_value_after_a_nonzero_default_label >/dev/null 2>&1; then
  echo "  ok   a union with a default: label slot at the discriminator's width, ignored on read whatever a peer"
  echo "       wrote in it — 42 hand-built labels incl. colliding ones, both orders, TypeCode and value level"
else
  echo "  FAIL a defaulted union TypeCode does not survive the wire"; fail_total=$((fail_total+1))
fi

hr "valuetype and abstract interface TypeCodes — a peer's bytes, not our reading"
# The registry recorded both as TypeCode::ObjRef "so `_is_a` and the catalogue
# keep working", so both emitters emitted a REFERENCE for them and the dynamic
# path marshalled an IOR where a peer sends a value — for six phases, because
# tk_abstract_interface's parameter list is byte-for-byte tk_objref's, and
# nothing here had ever asked omniORB what it writes. Fifteen captures, both
# stream orders; a recording nobody re-takes is a claim about the past.
# Negative controls (74b5662): registry back to ObjRef -> the registry test
# fails 3 of 3 ("Money LE: not equal", both orders); from_u32 forgetting 29
# and 32 -> the giop test fails 2 of 4 ("unknown or unsupported TCKind").
vtc_out=$(python3 spikes/valuetype_capture.py 2>&1); vtc_rc=$?
printf '%s\n' "$vtc_out"
if [ "$vtc_rc" -eq 2 ]; then
  skip_age absent git:spikes/valuetype_capture.py
elif [ "$vtc_rc" -ne 0 ]; then
  echo "  FAIL the recorded valuetype bytes no longer match the live peer"
  fail_total=$((fail_total+1))
fi
if cargo test -q -p orbweaver-giop --test valuetype_typecode_from_a_peer >/dev/null 2>&1 \
   && cargo test -q -p orbweaver-registry --test valuetype_shape_from_a_peer >/dev/null 2>&1; then
  echo "  ok   a valuetype is tk_value and an abstract interface is tk_abstract_interface — decoded, and our derived TypeCode == the peer's, both orders"
else
  echo "  FAIL a valuetype or abstract interface TypeCode does not match the peer's"
  fail_total=$((fail_total+1))
fi
# The member list itself (f8daa21): a branch that is both labelled and
# `default:` is one member per label plus a labelless default member where
# `default:` was written — omniidl's list, member for member, structurally
# equal (==) to the peer's decoded TypeCode over 8 more captures in both
# stream orders (17 recordings retaken by the capture script now). Negative
# control: the folded form fails 6 of 8 comparisons.
if cargo test -q -p orbweaver-registry --test union_shape_from_a_peer >/dev/null 2>&1; then
  echo "  ok   a union's member list is omniidl's: one member per label, the default its own member where written, both orders"
else
  echo "  FAIL the registry's union TypeCode is not structurally equal to the peer's"; fail_total=$((fail_total+1))
fi

hr "a native and a ValueBase — the refusal and the bytes, asked of the peer"
# The same defect one keyword over, and it survived the batch that named it:
# `native X;` was TypeCode::ObjRef, so both emitters emitted a reference and the
# dynamic path put an IOR on the wire for a type that has no wire form in any
# version. Asked of omniORB before choosing a representation, and here the
# measurement is a REFUSAL by all four routes it has: -b dump accepts the
# declaration, -bcxx exits 1 on it, -bpython ignores it and leaves a dangling
# typeMapping entry (KeyError one import later), and the ORB has no
# create_native_tc at all -- createTypeCode((tv_native, ...)) raises INTERNAL.
# So TypeCode::Native has no TcKind and encode refuses it by name. `ValueBase`
# is bytes, not a refusal: tk_value, VM_NONE (not VM_ABSTRACT, which is what a
# reasoned answer gets wrong), tk_null base, zero members.
# Negative controls (22637a8): registry back to ObjRef -> valuebase_shape fails
# with "Envelope BE: not equal"; from_u32 given an arm for 31 -> the giop test
# fails on a peer's 31 being accepted where omniORB itself cannot produce one.
ntc_out=$(python3 spikes/native_capture.py 2>&1); ntc_rc=$?
printf '%s\n' "$ntc_out"
if [ "$ntc_rc" -eq 2 ]; then
  skip_age absent git:spikes/native_capture.py
elif [ "$ntc_rc" -ne 0 ]; then
  echo "  FAIL the recorded native/ValueBase answers no longer match the live peer"
  fail_total=$((fail_total+1))
fi
if cargo test -q -p orbweaver-giop --test native_typecode_from_a_peer >/dev/null 2>&1 \
   && cargo test -q -p orbweaver-registry --test valuebase_shape_from_a_peer >/dev/null 2>&1; then
  echo "  ok   a native has no TypeCode to send and ValueBase is tk_value/VM_NONE — the peer's refusal and the peer's bytes, both orders"
else
  echo "  FAIL a native or a ValueBase does not agree with the peer"
  fail_total=$((fail_total+1))
fi

hr "performance — the dynamic path against the static stub"
# §8 has cited a LAN echo benchmark since v0.2 and there was none. This runs
# for the *shape* of the answer, never for a threshold: the exit code depends
# on whether both paths were measured and agreed on every answer, never on a
# duration. A latency gate fails on the day the machine is busy and teaches
# everyone to re-run it, which is how a gate stops being read.
#
# §11's target is deliberately not enforced here: "≤ 5 ms added and ≤ 3×
# static" names no shape, no payload and no machine, and at ~21µs its two
# clauses disagree by three orders of magnitude. A target nobody can test
# against is not made testable by a script picking a clause.
if cb_out=$(cargo run -q --release -p orbweaver-test --bin call-bench -- --samples 200 2>&1); then
  printf '%s\n' "$cb_out" | grep -E "^  (add|echo_)" | sed 's/^ */  ..   /'
  echo "  ok   both paths measured on four shapes and agreed on every answer"
else
  echo "  FAIL a series was not measured, or the two paths disagreed"
  diag_out "$cb_out" 5
  fail_total=$((fail_total+1))
fi

hr "generated code is linted, not merely compiled"
# `cargo build` accepted what `clippy -D warnings` does not, so a consumer
# building with warnings-as-errors could not compile the real OMG naming
# contract — `to_string`/`to_name`/`to_url` trip wrong_self_convention. Worse,
# an operation named `_default()` (legal IDL; `default` is reserved and the
# spec's escape is the leading underscore, which the mapping drops) emitted
# `Self::default()` and produced E0034: the generated crate did not compile at
# all. Neither was visible to a build-only check, which is why this step exists.
gl_out=$(cargo test -q -p orbweaver-gen --test emitted_current 2>&1)
if grep -q "test result: ok" <<<"$gl_out"; then
  echo "  ok   the blessed emitted corpus still matches, lints included"
else
  echo "  FAIL generated code no longer matches its blessed form"
  diag "a panic" "$gl_out" "$(grep -A3 panicked <<<"$gl_out")" 5
  fail_total=$((fail_total+1))
fi

hr "a generated skeleton answers as the hand-written servant does"
# Two different implementations answer the same interface and the caller's bytes
# are asserted identical. That is D029 §6.1's backend row exactly — *what
# implements it* varies INSIDE this group and the observation does not.
bears_on backend
# 59 scripted steps x 2 byte orders over CosNaming, and every structured reply
# decoded back by orbweaver-giop's own readers — two servants can agree on the
# wrong bytes. This is what forced D009's L2 early: the naming server began
# publishing TAG_CODE_SETS and the generated reference did not, and a byte
# comparison is the only check that could see it.
if cargo test -q -p orbweaver-gen --test naming_shape --test ifr_shape >/dev/null 2>&1; then
  echo "  ok   naming and IFR skeletons match their hand-written servants, both orders"
else
  echo "  FAIL a generated skeleton and its hand-written servant have diverged"
  fail_total=$((fail_total+1))
fi

hr "agent-fuzz — the parsers a tools/call reaches"
# An agent is untrusted in this project's threat model exactly as a peer is
# (R11/R12), and AnyJSON v1.1 put a recursive parser on that boundary —
# `tc_from_json`, which builds a type out of a document's own numbers. Nobody
# added it to a fuzzer, including whoever wrote it. Sibling of the wire-fuzz
# group; neither run's green stands in for the other's.
if af_out=$(cargo run -q --release -p orbweaver-test --bin agent-fuzz -- --cases 50000 2>&1); then
  printf '%s\n' "$af_out" | grep -E "documents:|types:|values:" | sed 's/^ */  ..   /'
  echo "  ok   50k documents over 7 agent-boundary targets: no panic, no unbounded allocation"
else
  echo "  FAIL a document an agent could send panics or buys memory"
  diag "a FAIL or a reproduce-with line" "$af_out" \
       "$(grep -E "FAIL|reproduce with" <<<"$af_out")" 4
  fail_total=$((fail_total+1))
fi
# A zero reach is a green that measured nothing.
# Match the zero-reach wording, not any WARNING: the same binaries also warn
# that a release build cannot observe arithmetic overflow, which is true and is
# not a zero reach. The first version of this check read that note as a missing
# target and turned a correct run red.
case "$af_out" in
  *"were reached; the target"*)
    echo "  FAIL a fuzz target was never reached"; fail_total=$((fail_total+1)) ;;
  *)  echo "  ok   every agent-boundary target was reached" ;;
esac

hr "§5.3 — a breaking change inside an included header reaches the gate"
# corpus/evolution/v{1,2}/ledger.idl are byte-identical; both breaking changes
# live in the common.idl they share. Read as strings, the two revisions are
# indistinguishable, so idl-diff printed "no change" and **exited 0** over a
# retyped struct member and a removed inherited operation. The §5.3 gate was
# waving through the one shape of breaking change a real estate produces.
# Captured then matched, never piped into grep -q.
ev_fail=0
ev_out=$(cargo run -q --bin idl-diff -- \
         corpus/evolution/v1/ledger.idl corpus/evolution/v2/ledger.idl 2>&1); ev_rc=$?
if [ "$ev_rc" -eq 1 ] && grep -q "amount_minor" <<<"$ev_out" \
   && grep -q "restamp" <<<"$ev_out"; then
  echo "  ok   both header-only breaking changes are named and the release is refused"
else
  echo "  FAIL a breaking change in a shared header does not reach the differ (exit $ev_rc)"
  diag_out "$ev_out" 3 head; ev_fail=1
fi
# The negative control, or the check above could pass for the wrong reason.
cargo run -q --bin idl-diff -- \
  corpus/evolution/v1/ledger.idl corpus/evolution/v1/ledger.idl >/dev/null 2>&1
if [ $? -eq 0 ]; then
  echo "  ok   a contract compared with itself is still accepted"
else
  echo "  FAIL idl-diff refuses a contract compared with itself"; ev_fail=1
fi
# And an unresolvable include must be "could not run", never a verdict: a diff
# of two partial graphs says nothing about the contracts it did not read.
ev_orphan=$(mktemp -d "${TMPDIR:-/tmp}/ow-orphan-XXXXXX")
cp corpus/evolution/v1/ledger.idl "$ev_orphan/"
ev2=$(cargo run -q --bin idl-diff -- \
      "$ev_orphan/ledger.idl" corpus/evolution/v2/ledger.idl 2>&1); ev2_rc=$?
if [ "$ev2_rc" -eq 2 ] && grep -q "common.idl" <<<"$ev2"; then
  echo "  ok   an unresolvable include is reported as unmeasured, not as a verdict"
else
  echo "  FAIL a missing header produced a release verdict (exit $ev2_rc)"; ev_fail=1
fi
rm -rf "$ev_orphan"
[ "$ev_fail" -eq 0 ] || fail_total=$((fail_total+1))

hr "DynAny — every corpus type taken apart and put back together"
# §8's discipline applied to the mutation API: a value walked component by
# component and reassembled must produce identical CDR, both byte orders and
# every alignment phase. The sampler that builds the source value does not use
# DynAny — the first version of this oracle did, and it passed with `next()`
# deliberately broken, because a producer and a consumer sharing a defect agree
# about the result. Captured, never piped into grep -q.
if dynany_out=$(RUSTFLAGS="-D warnings" cargo test -p orbweaver-dynamic \
     --test dynany_corpus -- --nocapture --test-threads=1 2>&1); then
  printf '%s\n' "$dynany_out" | grep -E "type\(s\) walked" | sed 's/^ *//;s/^/  ok   /'
  printf '%s\n' "$dynany_out" | grep -E "uncovered:" | sed 's/^ *uncovered:/  note uncovered:/'
else
  echo "  FAIL a corpus type does not survive a DynAny walk"
  diag "a wire difference, a failed walk or an invalid value" "$dynany_out" \
       "$(grep -E "differs on the wire|the walk failed|is invalid" <<<"$dynany_out")" 3
  fail_total=$((fail_total+1))
fi
# An array's length is a number in an agent's document since D008, and the
# reservation was made before the length was checked: 198 bytes reserved 206 GB.
if cargo test -q -p orbweaver-dynamic --test bounded_array >/dev/null 2>&1; then
  echo "  ok   a declared array length is checked against the buffer before it is reserved"
else
  echo "  FAIL an array length from a document can still buy memory"
  fail_total=$((fail_total+1))
fi

hr "peer input — an overflow the release fuzzer cannot see, and a body it cannot buy"
# Two hazards reachable from a peer, both measured before being fixed.
#
# `wire-fuzz` runs --release, where overflow-checks are OFF, so the arithmetic
# class is structurally invisible to it: `60 88 FF FF FF FF FF FF FF FF` panicked
# in debug and *wrapped* in release, returning "GSS token is truncated" — an
# error message that was a lie about what happened, and the quieter behaviour is
# the one that ships. One run with the checks on is the only gate in the tree
# that can see the class at all.
# CARGO_TARGET_DIR is separate on purpose: changing RUSTFLAGS invalidates the
# shared target directory, so without this every later cargo step in the
# harness rebuilds from scratch. The disk is cheaper than the rebuild.
if RUSTFLAGS="-C overflow-checks=on" CARGO_TARGET_DIR=target/overflow-checked \
     cargo run -q --release -p orbweaver-test \
     --bin wire-fuzz -- --cases 20000 >/tmp/orbweaver-oflow.log 2>&1; then
  echo "  ok   20k fuzz cases with overflow checks on: no arithmetic panic"
else
  echo "  FAIL an arithmetic overflow is reachable from peer bytes"
  diag_log /tmp/orbweaver-oflow.log 5
  fail_total=$((fail_total+1))
fi
# And the allocation half: twelve bytes declared 64 MiB and got it, before one
# body byte arrived. `panic_freedom` cannot observe an allocation by
# construction, so the property is asserted as "never asks for more than one
# chunk the peer has not delivered".
if cargo test -q -p orbweaver-giop --release --lib -- \
     a_declared_body_is_committed_only_as_it_arrives \
     a_body_larger_than_one_chunk_still_reads_back_byte_for_byte \
     chunk_boundaries_are_exact >/dev/null 2>&1; then
  echo "  ok   an inbound body is committed as it arrives, not as it is declared"
else
  echo "  FAIL a peer's declared message_size is committed before the bytes arrive"
  fail_total=$((fail_total+1))
fi

hr "wide text follows the connection, not a constant"
# `Cdr::put(&self, e: &mut Encoder)` has no connection to ask, so a stub's
# `wstring` answered with GIOP 1.2's form always — wrong on a 1.1 connection,
# and invisible to every test here, because our own round trip used the same
# constant at both ends. A convention both ends apply cannot be refuted by a
# round trip; that is the union-label lesson, in the static path.
if cargo test -q -p orbweaver-gen --test wide_follows_the_connection >/dev/null 2>&1; then
  echo "  ok   a stub's wstring takes 1.1's form on 1.1, and the encapsulation rule when unattached"
else
  echo "  FAIL a wstring is written from a constant rather than from the connection"
  fail_total=$((fail_total+1))
fi

hr "naming lifetimes — omniORB's client on the three re-examined deferrals"
# `bind_context`/`rebind_context`/`destroy` were deferred; two of the reasons
# turned out to describe the servant rather than constrain it. Driven by the
# peer's client, because ours and our server were written together. --nocapture
# because the verdict line is the fixture signal: the test prints SKIPPED and
# passes when omniORBpy is absent, and a silent green there would be an
# unmeasured check reported as a pass.
np_out=$(cargo test -q -p orbweaver-giop --test naming_lifecycle_from_a_peer -- --nocapture 2>&1)
case "$np_out" in
  *"naming-peer: measured"*)
    echo "  ok   omniORB drove bind_context/rebind_context/destroy against OUR server" ;;
  *"naming-peer: SKIPPED"*)
    skip absent git:crates/orbweaver-giop/tests/naming_lifecycle_from_a_peer.rs \
         "omniORBpy absent — the three deferrals are unmeasured, not passing" ;;
  *)
    echo "  FAIL the peer's view of bind_context/destroy"
    diag "an assertion's left/right" "$np_out" \
         "$(grep -E "^ *(left|right):" <<<"$np_out")" 2
    fail_total=$((fail_total+1)) ;;
esac
# Structural, and cheap: the naming servant must still contain no outbound
# call. That is what a federated bind_context would spend, so it fails here
# rather than in a deadlock six months later.
if cargo test -q -p orbweaver-giop --test naming_no_outbound_call >/dev/null 2>&1; then
  echo "  ok   naming still dials nothing: no lock across an outbound call, structurally"
else
  echo "  FAIL the naming servant now names something that dials a peer"
  fail_total=$((fail_total+1))
fi

hr "F5 lifecycle/property — omniORB's client against OUR tenant service"
# SERVICES-COVERAGE §9's one open direction for golden 23: 16-of-16 had only
# ever been asserted by the client written alongside the server.
f5_fail=0
rm -f /tmp/orbweaver-f5-a.ior /tmp/orbweaver-f5-b.ior /tmp/orbweaver-f5-hold.log
( cd "$ROOT" && exec cargo run -q --bin spike-tenants -- \
    /tmp/orbweaver-f5-a.ior /tmp/orbweaver-f5-b.ior --hold \
    >/tmp/orbweaver-f5-hold.log 2>&1 & )
f5_up=0
for _ in $(seq 1 120); do
  grep -qs HOLDING /tmp/orbweaver-f5-hold.log && { f5_up=1; break; }
  sleep 0.25
done
if [ "$f5_up" -eq 1 ]; then
  f5_out=$(python3 spikes/f5_peer_client.py /tmp/orbweaver-f5-a.ior /tmp/orbweaver-f5-b.ior 2>&1)
  case "$f5_out" in
    *"f5-peer: PASS"*)
      echo "  ok   omniORB called all 16 declared operations of golden 23 on our servant" ;;
    *)
      echo "  FAIL cross-ORB F5"
      diag "a FAIL or BLOCKED line" "$f5_out" "$(grep -E "FAIL|BLOCKED" <<<"$f5_out")" 3
      f5_fail=1 ;;
  esac
else
  echo "  FAIL the holding tenant service never came up"; f5_fail=1
fi
fkill spike-tenants
[ "$f5_fail" -eq 0 ] || fail_total=$((fail_total+1))

hr "cosevent pull model — no peer exists, so this is a repetition check"
# omniEvents is absent and omniORBpy ships no ProxyPullSupplier stubs, so there
# is nothing to check conformance against. What the harness adds over `cargo
# test` is repetition: this is the first servant operation that blocks inside
# `dispatch`, and the failure mode of its central test is a timeout rather than
# a wrong value. One green run of a test like that is not evidence.
pull_fail=0
for pull_profile in "" "--release"; do
  for _ in 1 2 3; do
    pull_out=$(cargo test -q -p orbweaver-giop $pull_profile --test event_pull_model 2>&1)
    if [ $? -ne 0 ] || ! grep -q "test result: ok\." <<<"$pull_out"; then
      pull_fail=$((pull_fail+1))
      diag_out "$pull_out" 6
    fi
  done
done
if [ "$pull_fail" -eq 0 ]; then
  echo "  ok   cosevent pull model green 6/6 across both profiles"
else
  echo "  FAIL cosevent pull model: $pull_fail of 6 runs were not green"
  fail_total=$((fail_total+1))
fi

hr "codeset advertising — a conversion is offered only where one is needed"
# D009 §8 row 4 conditions a non-empty char conversion list on a peer that
# cannot reach UTF-8. This runs the condition rather than remembering it: a
# non-zero exit means such a peer now exists, the empty list is costing it, and
# the row is unblocked. ~3 s, no long-lived fixture, no port.
cpa_out=$(python3 spikes/codeset_peer_probe.py 2>&1); cpa_rc=$?
if [ "$cpa_rc" -eq 1 ]; then
  diag_out "$cpa_out" 3
  echo "  FAIL a peer advertises ISO-8859-1 without UTF-8 — D009 §8 row 4 is unblocked"
  fail_total=$((fail_total+1))
elif [ "$cpa_rc" -eq 2 ]; then
  # This skip was counted and never announced: the probe prints no SKIPPED line
  # of its own, so the group showed two tail lines and bumped the counter, and a
  # reader of the verdict had no group to attach the number to. D010 §2 wants a
  # counted skip to NAME its fixture, so now it does.
  skip absent git:spikes/codeset_peer_probe.py \
       "spikes/codeset_peer_probe.py could not reach a peer configuration (exit 2) —" \
       "D009 §8 row 4's condition is unmeasured, not passing"
  diag_out "$cpa_out" 2
else
  echo "  ok   every peer configuration still reaches UTF-8; the empty conversion list holds"
fi

hr "wide characters — the value's own order, not the message's"
# A UTF-16 wchar or wstring states its byte order in its own octets and ignores
# the stream's. Our writer always emits a mark, so our round trip could never
# produce the unmarked body the reader was wrong about — the read half is the
# one that matters, and only the peer's *reader* can settle it, because its
# writer always marks too. Twelve readings, six answers, no dependence on the
# stream flag.
wcc_out=$(python3 spikes/wide_char_capture.py 2>&1); wcc_rc=$?
printf '%s\n' "$wcc_out" | tail -3
if [ "$wcc_rc" -eq 2 ]; then
  skip_age absent git:spikes/wide_char_capture.py
elif [ "$wcc_rc" -ne 0 ]; then
  echo "  FAIL the recorded wide-character bytes no longer match the live peer"
  fail_total=$((fail_total+1))
else
  echo "  ok   17 recorded wide-character readings still match the live peer"
fi
if cargo test -q -p orbweaver-giop --test wide_chars_from_a_peer >/dev/null 2>&1; then
  echo "  ok   the peer's wchar and wstring bytes read, and a wchar re-encodes to them"
else
  echo "  FAIL wide characters from a peer do not read as the peer reads them"
  fail_total=$((fail_total+1))
fi

hr "encapsulation offsets — the same bytes wherever the body lands"
# position() added the continuing_at prefix after subtracting the origin, so a
# TypeCode encapsulation in a GIOP 1.0/1.1 body aligned from the message's
# offset rather than from its own flag. Unreachable at 1.2, which rounds the
# body start to a multiple of 8; unconditional at 1.0/1.1. It did not round-trip
# against itself either — nothing had ever asked.
if cargo test -q -p orbweaver-giop --test spliced_encapsulations >/dev/null 2>&1; then
  echo "  ok   a TypeCode is the peer's bytes at every offset a body can hand it"
else
  echo "  FAIL an encapsulation's contents change with where the body lands"
  fail_total=$((fail_total+1))
fi

hr "release gate — idl-diff accepts what both oracles accept"
# The check whose absence let the gate refuse a valid contract: nothing had ever
# run idl-diff over the corpus. A contract diffed against itself is "no change"
# by construction, so a non-zero exit here is the gate refusing a file rather
# than finding a breaking change. `gen-naming-subset.idl` — inherited `raises`,
# exactly as the OMG writes it — exited 2 while omniidl and JacORB both
# accepted it, and a gate that cries wolf gets bypassed.
gate_refused=""
for f in corpus/golden/*.idl corpus/services/*.idl corpus/pragma/*.idl; do
  gout=$(cargo run -q --bin idl-diff -- "$f" "$f" 2>&1); grc=$?
  if [ "$grc" -ne 0 ]; then
    # `head -2 | tail -1` is `sed -n 2p` with two early-exit forms in it, and
    # when idl-diff printed nothing the row read `rc=2: ` with a blank after
    # the colon — a refusal that names a file and says nothing about it.
    gline=$(sed -n '2p' <<<"$gout")
    gate_refused="$gate_refused
       $(basename "$f") rc=$grc: ${gline:-(idl-diff printed nothing at all)}"
  fi
done
if [ -z "$gate_refused" ]; then
  echo "  ok   the §5.3 gate issues a verdict on every contract the oracles accept"
else
  echo "  FAIL the gate refused contracts it must be able to diff:$gate_refused"
  fail_total=$((fail_total+1))
fi
# Two negative controls, or the check above passes by never refusing anything.
# The sibling case is the one that matters: a resolver that "fixed" inheritance
# by searching every interface in the unit passes every positive case and fails
# only this.
for nctl in corpus/negative/inherited-scope-leak.idl corpus/negative/n04-unknown-type.idl; do
  if [ ! -f "$nctl" ]; then
    echo "  FAIL $nctl is missing — the control is unmeasured, which is a failure"
    fail_total=$((fail_total+1)); continue
  fi
  cargo run -q --bin idl-diff -- "$nctl" "$nctl" >/dev/null 2>&1
  if [ $? -eq 2 ]; then
    echo "  ok   still exits 2 on $(basename "$nctl")"
  else
    echo "  FAIL $(basename "$nctl") no longer refused; the gate has stopped checking"
    fail_total=$((fail_total+1))
  fi
done

hr "the release profile, run rather than only built"
# Six tests asserted a panic the release build does not produce — the lock
# tripwire counts in both profiles and panics only in debug, deliberately, so a
# live ORB is not killed by its own diagnostic. The tests were asserting the
# debug *reaction* instead of the property, so `cargo test --release` could not
# be run clean, so nobody ran it, so the release-only defect class had no test
# pass that would find it. That is how an overflow that wrapped in release and
# panicked in debug survived every green run until a fuzzer met it.
#
# Measured on this machine, warm: 22 s to build the workspace's release test
# binaries, 49 s to run them. `wire-fuzz` already builds release, so most of
# the first number is paid either way.
if cargo test --workspace --release --no-fail-fast >/tmp/orbweaver-release.log 2>&1; then
  echo "  ok   $(grep -cE '^test result: ok' /tmp/orbweaver-release.log) release suite(s) green"
else
  echo "  FAIL the release profile is not clean; a profile nobody can run is a profile nobody runs"
  diag "a FAILED test or a panic" "$(tail -40 /tmp/orbweaver-release.log 2>/dev/null)" \
       "$(grep -E "^test .*FAILED|panicked at" /tmp/orbweaver-release.log)" 4
  fail_total=$((fail_total+1))
fi

hr "the records keep up with the code"
# A gate for decision *statuses* went in on 2026-08-18. It checks one field and
# does not check whether the documents that describe the code were opened at
# all — and thirty-nine commits later, six of them wire-behaviour changes,
# three COMPONENTS rows had become false: two gap columns naming work that had
# landed, and a row calling a measurement unmeasured. This is the crude half of
# a rule whose precise half no script can hold: it reads no words, it counts
# distance.
if rk_out=$(python3 spikes/records_keep_up.py 2>&1); then
  printf '%s\n' "$rk_out"
else
  printf '%s\n' "$rk_out"
  echo "  FAIL a record that describes this code has not been opened in a while"
  fail_total=$((fail_total+1))
fi

hr "decision status — one source of truth, restated nowhere stale"
# A decision's status lives in docs/decisions/D00N-*.md. Five other documents
# restate it, and restatements drift: the first run of this gate found seven,
# including a planning row that sent a whole planning pass down a branch that
# had already landed, and D003 itself saying APPROVED in English and 제안 in
# Korean four lines apart. Text, no fixtures, so it runs with the licence
# checks rather than behind a peer.
if status_out=$(python3 spikes/decision_status.py 2>&1); then
  printf '  ok   %s\n' "$(printf '%s' "$status_out" | tail -1 | sed 's/^ *//')"
else
  printf '%s\n' "$status_out" | sed 's/^/  /'
  echo "  FAIL a document states a decision status the decision does not have"
  fail_total=$((fail_total+1))
fi

hr "ssliop feature — the D002 dependency promise"
ssl_fail=0
# A default build must carry no cryptography dependency at all.
deft=$(cargo tree -p orbweaver-giop 2>/dev/null)
if grep -qiE "rustls|aws-lc" <<<"$deft"; then
  echo "  FAIL the default build pulls a TLS/crypto crate; NOTICE and D002 are wrong"; ssl_fail=1
else
  echo "  ok   default cargo tree carries no rustls/aws-lc, as NOTICE states"
fi
# And the feature must actually deliver what D002 approved.
feat=$(cargo tree -p orbweaver-giop --features ssliop 2>/dev/null)
if grep -q "rustls" <<<"$feat" && grep -q "aws-lc-rs" <<<"$feat"; then
  echo "  ok   --features ssliop pulls rustls with the aws-lc-rs provider D002 names"
else
  echo "  FAIL --features ssliop does not resolve to rustls + aws-lc-rs"; ssl_fail=1
fi
# In-process TLS tests: certificate verification on, framing pass-through,
# clean refusal of a non-TLS peer. Peer interop (omniORB sslTP) is a future
# batch and is deliberately NOT claimed here.
if RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop --features ssliop --quiet >/dev/null 2>&1; then
  echo "  ok   ssliop build tests green against the in-process rustls peer, warning-free"
else
  echo "  FAIL the ssliop build does not build cleanly or does not test"; ssl_fail=1
fi
# D010 B3: SSLIOP against a peer. This was a SKIPPED group for the life of the
# project, on the premise that the fixture is omniORBpy's `sslTP` (brew's build
# ships none) or JacORB's SSL transport configured. The premise is true and the
# conclusion does not follow: SSLIOP is unmodified GIOP over TLS plus a
# `TAG_SSL_SEC_TRANS` component, so the peer it needs is a socket, not an ORB.
# `spikes/ssliop.sh` is that peer plus the driver — see its header for what it
# does and does not close.
#
# Its exit code is the verdict, and it keeps three answers apart on purpose:
#   0  every case was measured and held
#   3  nothing was measured (no cargo, no python3, no certificates, no driver)
#   1  a case was measured and did not hold
# 3 and 1 must not collapse into each other. An unmeasured check is a failure
# and never a pass, but it is also not a refutation, and a group that printed
# the same line for both would send a reader looking for a wire defect that
# does not exist. So 3 lands as a counted SKIPPED naming its fixture and 1
# lands as a FAIL.
ssliop_out=$(./spikes/ssliop.sh 2>&1); ssliop_rc=$?
ssliop_verdict=$(sed -n 's/^ssliop: //p' <<<"$ssliop_out" | tail -1)
# A herestring, never a pipe: `grep -q` SIGPIPEs its producer and `pipefail`
# turns a failed producer into "no match" (line 9, and CLAUDE.md's rule).
case "$ssliop_rc" in
  0)
    # Exit 0 over zero cases is the green-while-measuring-nothing shape — a
    # script whose body stopped running still reaches `verdict` with fails=0.
    # 21 is a FLOOR, not today's figure: it is what the script's own loops
    # enumerate (6 advertisement cases + 15 transport cases), and adding a
    # case raises it. Nothing here re-states a measurement.
    ssliop_cases=$(sed -n 's/^PASS — \([0-9][0-9]*\) cases.*/\1/p' <<<"$ssliop_verdict")
    if [ -z "$ssliop_cases" ] || [ "$ssliop_cases" -lt 21 ]; then
      echo "  FAIL spikes/ssliop.sh exited 0 over ${ssliop_cases:-no} cases (floor 21) — it measured less than it has"
      ssl_fail=1
    else
      echo "  ok   B3 peer proof: $ssliop_cases cases against spikes/ssliop_peer.py — GIOP over TLS to"
      echo "       another process, the advertisement read out of an IOR our encoder did not write,"
      echo "       both IOR and both component byte orders, and five refusals"
    fi
    ;;
  3)
    skip absent git:spikes/ssliop.sh \
         "spikes/ssliop.sh measured nothing (exit 3): $ssliop_verdict" \
         "its fixture is spikes/ssliop_peer.py, the certificates in spikes/tls/, and a" \
         "build of spike-ssliop; SSLIOP against a peer is unmeasured, not passing (D010 B3)"
    diag "a FAIL line" "$ssliop_out" "$(grep -E '^  FAIL' <<<"$ssliop_out" | tail -4)" 4
    ;;
  *)
    # A script that could not be run at all reaches no verdict, and calling
    # that a refutation would send a reader after a wire defect. Still a
    # failure — an unmeasured check is never a pass — but a different one.
    if [ -z "$ssliop_verdict" ]; then
      echo "  FAIL spikes/ssliop.sh could not be run at all (exit $ssliop_rc) — B3 was NOT measured"
      diag_out "$ssliop_out" 3
    else
      echo "  FAIL spikes/ssliop.sh refuted a B3 claim (exit $ssliop_rc): $ssliop_verdict"
      diag "a FAIL line" "$ssliop_out" "$(grep -E '^  FAIL' <<<"$ssliop_out" | tail -6)" 6
    fi
    ssl_fail=1
    ;;
esac
# The one residue the peer above cannot close, and it is still class B: a
# `TAG_SSL_SEC_TRANS` component produced by omniORB's or JacORB's OWN encoder,
# with the association-option bits and port convention that implementation
# chose. Only they can make that claim, so D010 §2 applies unchanged — a
# counted SKIPPED naming its fixture, never a `note` and never an `ok`, because
# the verdict line counts SKIPPED and does not count prose.
#
# The probe is the interpreter's exit code, NOT a marker grepped from its
# output: the first version printed 'sslTP present' and grepped for it, and the
# ImportError traceback echoes the source line — so the gate matched its own
# probe text and reported the module present where it is not.
if python3 -c "import omniORB.sslTP" >/dev/null 2>&1; then
  echo "  SKIPPED  omniORBpy sslTP IS present here, so the one residue — a TAG_SSL_SEC_TRANS from"
  echo "           THEIR encoder, not ours — could be taken now and is not. Unmeasured (D010 B3)"
else
  echo "  SKIPPED  no omniORBpy sslTP and no JacORB SSL here, so a TAG_SSL_SEC_TRANS produced by"
  echo "           THEIR encoder stays unmeasured, not passing (D010 B3, spikes/tls/PEER-STATUS.md)"
fi
skip_age absent git:spikes/tls/PEER-STATUS.md
[ "$ssl_fail" -eq 0 ] || fail_total=$((fail_total+1))

hr "orbweaver-idl — our parser against the oracle"
# The acceptance criterion is agreement, not taste: omniidl accepts every
# golden file and rejects every negative one, so anywhere we differ we are
# wrong. Semantic negatives are excluded here and belong to the semantic pass.
if cargo test -p orbweaver-idl --quiet >/dev/null 2>&1; then
  echo "  ok   accepts all $(ls corpus/golden/*.idl | wc -l | tr -d ' ') golden files and the 20-file benchmark"
  echo "  ok   rejects the syntactic negatives, including unescaped keywords"
else
  echo "  FAIL our parser disagrees with the oracle"
  idl_re=$(cargo test -p orbweaver-idl 2>&1)
  diag "a 'we do not' or 'accepted them' line" "$idl_re" \
       "$(grep -E "we do not|accepted them" <<<"$idl_re")" 3
  fail_total=$((fail_total+1))
fi

hr "IDL semantics — full agreement with the oracle"
# The interim regex lint (spikes/idl_lint.py) is retired: orbweaver-idl now
# walks a real scope tree, so the identifier rules are expressed once instead
# of re-approximated for each syntactic shape they take — which is how the
# regex missed operation names, and struct scopes before that.
neg_missed=""
for f in corpus/negative/*.idl; do
  if cargo run -q --bin idl-check -- "$f" >/dev/null 2>&1; then
    neg_missed="$neg_missed $(basename "$f")"
  fi
done
if [ -z "$neg_missed" ]; then
  echo "  ok   rejects all $(ls corpus/negative/*.idl | wc -l | tr -d ' ') negatives, syntactic and semantic"
else
  echo "  FAIL the oracle rejects these and we accept them:$neg_missed"
  fail_total=$((fail_total+1))
fi
# stdout only: a build warning on stderr is not an IDL diagnostic.
cargo build -q --bin idl-check 2>/dev/null
clean_out=$(cargo run -q --bin idl-check -- corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl 2>/dev/null)
if [ -z "$clean_out" ]; then
  echo "  ok   accepts every golden, benchmark and fixture file the oracle accepts"
else
  sed -n '1,5p' <<<"$clean_out" | sed 's/^/  FAIL /'
  fail_total=$((fail_total+1))
fi

# ── Differential conformance ─────────────────────────────────────────────────
hr "differential conformance — every front end on every corpus file"
# Was two ad-hoc omniidl loops over golden/ and negative/. They are now one
# script, because CI runs a second oracle (tao_idl) and the interesting result
# there is where the *oracles* disagree with each other — a corpus file that is
# not portable, which agreeing with either one of them cannot reveal.
dout=$(bash spikes/differential.sh 2>&1); drc=$?
printf '%s\n' "$dout"
if [ "$drc" -ne 0 ]; then fail_total=$((fail_total+1)); fi
# An absent oracle is unmeasured, not passing, and the verdict has to say so.
if grep -q "SKIPPED" <<<"$dout"; then skip_age absent git:spikes/differential.sh; fi

# ── Assumption C ─────────────────────────────────────────────────────────────
hr "assumption C — IDL 4 @annotation acceptance in a deployed compiler"
c1=$(omniidl -b dump corpus/annotations/c1-idl4-annotation.idl 2>&1 >/dev/null)
c3=$(omniidl -b dump corpus/annotations/c3-structured-comment.idl 2>&1 >/dev/null)
if [ -n "$c1" ]; then echo "  confirmed  @annotation REJECTED by omniidl (risk R1 is real)"; else echo "  surprise   @annotation accepted — revisit the SIDL plan"; fi
if [ -z "$c3" ]; then echo "  ok         structured-comment fallback compiles"; else echo "  FAIL       fallback does not compile"; fail_total=$((fail_total+1)); fi

# ── Assumption B ─────────────────────────────────────────────────────────────
hr "assumption B — generated IDL compiles"
bp=0; bf=0
for f in corpus/requirements/generated/R*.idl; do
  if [ -z "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then bp=$((bp+1)); else bf=$((bf+1)); echo "  FAIL $(basename "$f")"; fi
done
echo "  $bp/20 compile after self-repair (first pass was 13/20 — see docs/PHASE0.md)"
[ "$bf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Assumption A ─────────────────────────────────────────────────────────────
hr "assumption A — GIOP interop against a stock ORB"
if start_server; then
  # Capture before matching. Piping into `grep -q` closes the pipe on the
  # first match and SIGPIPEs the producer, which shows up as a phantom
  # failure — that bug cost a debugging cycle here already.
  interop=$(cargo run -q --bin spike-interop -- spikes/echo.ior 2>&1)
  printf '%s\n' "$interop" > /tmp/orbweaver-a.log
  if grep -q "assumption A: PASS" <<<"$interop"; then
    echo "  ok   both byte orders interoperated"
  else
    echo "  FAIL see /tmp/orbweaver-a.log"
    diag "a FAIL line" "$interop" "$(grep -E "^  FAIL" <<<"$interop")" 3
    fail_total=$((fail_total+1))
  fi
else
  # A fixture that will not start is an unmeasured assumption, not a pass.
  fail_total=$((fail_total+1))
fi
cleanup

# ── Assumption D ─────────────────────────────────────────────────────────────
hr "assumption D — IOR endpoint publishing"
if start_server; then
  # `... | head -1` SIGPIPEd spike-dump and, under `pipefail`, threw away its
  # exit status — so a spike-dump that could not run at all left `$adv` holding
  # its first error line, which does not contain 127.0.0.1, and the `*)` arm
  # below printed **"confirmed a routable-but-local address is published"** over
  # a producer that published nothing. Capture first, read the producer's own
  # status, then take the first line off a herestring where nothing can exit
  # early on a pipe.
  adv_raw=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1); adv_rc=$?
  adv=$(head -1 <<<"$adv_raw")
  echo "  default publish: $adv"
  if [ "$adv_rc" -ne 0 ]; then
    echo "  FAIL spike-dump could not read the published address (exit $adv_rc) — R7 is UNMEASURED here,"
    echo "       which is a failure and not a 'confirmed'"
    diag_out "$adv_raw" 3
    fail_total=$((fail_total+1))
  else
    case "$adv" in
      *127.0.0.1*) echo "  note  loopback published; a container would publish its pod IP instead" ;;
      *)           echo "  confirmed  a routable-but-local address is published, not loopback (risk R7 is real)" ;;
    esac
  fi
else
  fail_total=$((fail_total+1))
fi
cleanup
# The fixed port sits below every ephemeral range in use here — Linux hands
# out 32768–60999, macOS 49152–65535 — because the harness makes a few thousand
# outbound connections before this line and the kernel may have lent any port
# in that range to one of them. It was 40404, and CI failed to bind it in two
# of ten runs: "Address in use?" from a fixture that had done nothing wrong.
if start_server -ORBendPoint giop:tcp::24404 -ORBendPointPublish giop:tcp:127.0.0.1:24404; then
  rewritten=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1)
  echo "  rewritten publish: $(head -1 <<<"$rewritten")"
  # Was `echo "$rewritten" | grep -q RESPONSE` — the form this file's own header
  # calls out twice: `grep -q` exits on the first match, SIGPIPEs `echo`, and
  # `pipefail` hands the pipeline that 141 so the `if` reads a MATCH as "no
  # match". A herestring has no producer to kill and no pipeline status.
  if grep -q RESPONSE <<<"$rewritten"; then
    echo "  ok   endpoint rewriting works — mitigation for R7 is available"
  else
    echo "  FAIL endpoint rewriting did not produce a reachable reference"
    diag "a no-response, closed or error line" "$rewritten" \
         "$(grep -E "no response|closed|error" <<<"$rewritten")" 6
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi

# ── Reverse interop: a stock ORB calls US ───────────────────────────────────
hr "reverse interop — omniORB client against our server"
if start_rust_server; then
  rev_fail=0
  for v in 1.0 1.1 1.2; do
    if python3 spikes/reverse_client.py spikes/server.ior -ORBmaxGIOPVersion "$v" >/dev/null 2>&1; then
      echo "  ok   omniORB client at GIOP $v -> our server, 5/5"
    else
      echo "  FAIL omniORB client at GIOP $v could not call our server"; rev_fail=1
    fi
  done
  # "We tested three versions" is only true if the peer used three. An ORB that
  # ignored the option would otherwise give three identical passes proving one.
  seen=$(grep -c "first request at GIOP" /tmp/orbweaver-srv.log 2>/dev/null || echo 0)
  if [ "$seen" -eq 3 ]; then
    echo "  ok   server confirms three distinct GIOP versions were received"
  else
    echo "  FAIL server saw $seen distinct versions, not 3 — the option was ignored"
    rev_fail=1
  fi
  [ "$rev_fail" -eq 0 ] || fail_total=$((fail_total+1))
else
  fail_total=$((fail_total+1))
fi
fkill spike-server

# ── Fragmentation ────────────────────────────────────────────────────────────
hr "GIOP fragmentation"
# This comment used to say neither available peer emits fragments. That was an
# assumption nobody had tested with a large enough argument: asked for a 1 MB
# sequence<octet>, omniORB 4.3.4 answers in two pieces, reproducibly, with no
# configuration — measured by spike-mux. JacORB 3.9 still does not. So the
# reassembler has now been fed a real peer's fragments, and the direction below
# (we fragment, they reassemble) is no longer the only independent evidence. The receiver used to be
# covered only by round-trip against our own emitter, which is one shape; it is
# now also driven by hand-built streams from §9.4.9 that a conformant peer may
# legally send and ours never does (`tests/fragment_reception.rs`, run by cargo
# test). That found two reception bugs no peer could have shown us: a stray
# leading Fragment was returned as a message, and a fragment at a different
# GIOP version was accepted as a continuation — in 1.1 the bytes read as a
# request id are body, so a match would have been a coincidence.
fkill spike-server
rm -f "$ROOT/spikes/server.ior"
( cd "$ROOT" && ORBWEAVER_FRAGMENT_THRESHOLD=4096 exec cargo run -q --bin spike-server -- \
    spikes/server.ior 127.0.0.1 0 >/tmp/orbweaver-frag.log 2>&1 & )
frag_up=0
for _ in $(seq 1 100); do
  [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; frag_up=1; break; }
  sleep 0.1
done
if [ "$frag_up" -eq 0 ]; then
  echo "  FAIL fragmenting server did not start"; fail_total=$((fail_total+1))
else
  ffail=0
  out=$(python3 spikes/reverse_client.py spikes/server.ior 2>&1)
  if grep -q "failures: 0" <<<"$out"; then
    echo "  ok   omniORB reassembled our fragments (250 KB at a 4 KB threshold)"
  else
    echo "  FAIL omniORB could not reassemble our fragments"; ffail=1
  fi
  if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
    out=$(cd "$ROOT/spikes/jacorb" && "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Client ../server.ior 2>&1)
    if grep -q "failures: 0" <<<"$out"; then
      echo "  ok   JacORB reassembled our fragments — a second, independent reader"
    else
      echo "  FAIL JacORB could not reassemble our fragments"; ffail=1
    fi
  else
    skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
  fi
  echo "  note our *receiver* has no independent validation: no available peer emits"
  echo "       GIOP fragments, so it is covered by round-trip against our own emitter"
  [ "$ffail" -eq 0 ] || fail_total=$((fail_total+1))
fi
fkill spike-server

# ── Object model ─────────────────────────────────────────────────────────────
hr "object model — references, identity, LOCATION_FORWARD"
# The second half sends a LOCATION_FORWARD and requires the peer to retry
# transparently — the target's address changes under a live caller and the
# caller's result does not.
bears_on location
if start_rust_server; then
  out=$(python3 spikes/object_client.py spikes/server.ior 2>&1)
  if grep -q "failures: 0" <<<"$out"; then
    echo "  ok   _is_a answered from the inheritance graph, no network lookup"
    echo "  ok   an object reference survives as a value and is callable"
  else
    echo "  FAIL object model against omniORB"
    diag "a FAIL line" "$out" "$(grep FAIL <<<"$out")" 3
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi
cleanup

# LOCATION_FORWARD: Phase 1 could follow one and never send one. A peer must
# retry transparently, and the server logs the emission so a call that would
# have succeeded anyway cannot be mistaken for proof.
fwd_fail=0
for peer in omni jacorb; do
  fkill spike-server
  rm -f "$ROOT/spikes/server.ior"
  ( cd "$ROOT" && ORBWEAVER_FORWARD_PING=1 exec cargo run -q --bin spike-server -- \
      spikes/server.ior 127.0.0.1 0 >/tmp/orbweaver-fwd.log 2>&1 & )
  up=0
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; up=1; break; }
    sleep 0.1
  done
  [ "$up" -eq 1 ] || { echo "  FAIL forwarding server did not start"; fwd_fail=1; break; }

  if [ "$peer" = omni ]; then
    got=$(python3 spikes/object_client.py spikes/server.ior 2>&1 | grep -c "get_self() is callable -> 42")
    label="omniORB"
  else
    if [ ! -d "$ROOT/spikes/jacorb/classes" ] || [ ! -x "$JH_CHECK/bin/java" ]; then
      skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"; continue
    fi
    got=$(cd "$ROOT/spikes/jacorb" && "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Client ../server.ior 2>&1 | grep -c "ping() -> 42")
    label="JacORB"
  fi
  sleep 0.3
  emitted=$(grep -c "emitted LOCATION_FORWARD" /tmp/orbweaver-fwd.log 2>/dev/null || echo 0)
  if [ "$got" -ge 1 ] && [ "$emitted" -ge 1 ]; then
    echo "  ok   $label followed a LOCATION_FORWARD we emitted"
  else
    echo "  FAIL $label: call ok=$got, forwards emitted=$emitted"
    fwd_fail=1
  fi
done
fkill spike-server
[ "$fwd_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── LOCATION_FORWARD_PERM: a generated skeleton saying "moved for good" ─────
hr "LOCATION_FORWARD_PERM — status 4 from a generated skeleton, omniORB following it"
# The object has moved for good and a foreign client follows without its caller
# being told: D029 §6.1's location row, measured rather than argued.
bears_on location
# The status byte is the oracle: through every client measured, a temporary
# and a permanent forward produce the same request count at the old reference
# (1), so a count can never go red on its own. The in-test control is the
# temporary servant beside the permanent one reading status 3; the run-red
# control (Forward::reply_status forced to 3 -> "left: 3 right: 4") is in
# 680aa41's message. D010 A1, 2026-08-19.
permout=$(cargo test -q -p orbweaver-gen --test object_identity -- \
            an_object_moved_for_good_answers_with_location_forward_perm \
            omniorb_follows_a_permanent_forward_from_a_generated_skeleton --nocapture 2>&1)
case "$permout" in
  *"test result: ok. 2 passed"*)
    echo "  ok   status 4 at 1.2, 3 below, 3 from the temporary servant — raw off the wire, both byte orders"
    if grep -q "UNMEASURED: omniORB" <<<"$permout"; then
      skip absent git:crates/orbweaver-gen/tests/object_identity.rs \
           "omniORB half — fixture absent"
    else
      echo "  ok   omniORB followed a LOCATION_FORWARD_PERM from a generated skeleton, five calls answered by the new object"
      perm_counts=$(grep "requests at the OLD reference" <<<"$permout")
      if [ -n "$perm_counts" ]; then
        sed -n '1,2p' <<<"$perm_counts" | sed 's/^/       /'
      else
        echo "       (the test printed no request count for the old reference — the ok above"
        echo "        stands on its own assertions, not on a number shown here)"
      fi
    fi ;;
  *) echo "  FAIL LOCATION_FORWARD_PERM"
     diag "a panic or an assertion's left/right" "$permout" \
          "$(grep -E "panicked|left:|right:" <<<"$permout")" 4
     fail_total=$((fail_total+1)) ;;
esac
# The pool half (b77c9fb): Sent::Forward carries Forward, Pool::invoke_tracking
# and Reference::forwarded() report the last hop. Negative controls in that
# commit: interpret forced all-temporary -> red at 1.2 x Permanent, forced
# all-permanent -> red at 1.0 x Temporary.
poolout=$(cargo test -q -p orbweaver-giop --test mux_pool -- \
            the_pool_follows_both_forward_statuses_and_reports_permanent_only_at_1_2 \
            a_real_server_is_heard_as_permanent_only_at_1_2 2>&1)
case "$poolout" in
  *"test result: ok. 2 passed"*)
    echo "  ok   pool: permanent reported only for 1.2 x permanent — 12 scripted cells (both reply orders) + real Server, native" ;;
  *) echo "  FAIL pool forward reporting"
     diag "a panic or an assertion's left/right" "$poolout" \
          "$(grep -E "panicked|left:|right:" <<<"$poolout")" 4
     fail_total=$((fail_total+1)) ;;
esac
# The chain half (adf0867): Pool::attempt accumulates hops into a private
# Chain; Reference::note applies it per hop, so permanent-then-temporary
# re-points ior at the permanent hop and caches the temporary one relative to
# it — the restart returns to the permanent hop instead of through it, and
# reuses the pooled connection, so it costs no dial. Negative controls in that
# commit: note() reverted to last-hop-only -> red at "the permanent hop
# re-pointed the reference"; with the ior asserts muted -> "left: 99 right: 7",
# the original having answered the restart.
chainout=$(cargo test -q -p orbweaver-giop --test forward_chain 2>&1)
case "$chainout" in
  *"test result: ok. 3 passed"*)
    echo "  ok   pool: a permanent->temporary chain restarts at the permanent hop, not the original — 3 shapes x both reply orders, 0 extra dials" ;;
  *) echo "  FAIL forward chain"
     diag "a panic or an assertion's left/right" "$chainout" \
          "$(grep -E "panicked|left:|right:" <<<"$chainout")" 4
     fail_total=$((fail_total+1)) ;;
esac
# cd9f88f: a permanent forward is the object moving, so it is shared by every
# clone of the Reference (Arc<Guarded<Ior>>), while the temporary cache stays
# per handle — §9.6 keeps the original authoritative for a temporary hop, so
# that one is routing state and self-corrects. Negative control in that
# commit: refresh() made a no-op -> a clone taken before the move still reads
# b"old"; with the ior asserts muted, 3 requests at the address the object
# left instead of 1.
#
# The sixth cell (b4a0963) pins the *boundary* of that sharing, which D013
# turns on: three references created independently from one IOR (Pool::reference,
# not Clone) cost 3 requests at the address the object left and 7 at the object,
# both reply orders — one forward per reference, once, because a second
# reference re-points itself on its own first hop. omniORB 4.3.4 charges the
# same 3 of 7 in the identical shape. It pins a cost, not a virtue, and goes
# red on purpose if the identity map D013 declines to build is ever built.
cloneout=$(cargo test -q -p orbweaver-giop --test forward_clone 2>&1)
case "$cloneout" in
  *"test result: ok. 6 passed"*)
    echo "  ok   a permanent forward is seen by every clone of the reference — 3 requests at the old address down to 1, both reply orders; two independent references cost one forward each, once (D013); the temporary cache stays per handle" ;;
  *) echo "  FAIL reference clones and a permanent forward"
     diag "a panic or an assertion's left/right" "$cloneout" \
          "$(grep -E "panicked|left:|right:" <<<"$cloneout")" 4
     fail_total=$((fail_total+1)) ;;
esac
# A caller's version cap across a hop and a restart (adf0867): move_to
# restored byte order, converter, TLS policy and origin but re-negotiated the
# version from the forwarded-to profile, so a caller capped to 1.1 spoke 1.2
# at a 1.2 target — a wire-format change under a caller who cannot see the hop.
# Negative control: `let _ = version_cap;` in move_to -> the target sees 1.2.
capout=$(cargo test -q -p orbweaver-giop --test forward_restart -- \
           a_version_cap_survives_a_forward_and_a_restart 2>&1)
case "$capout" in
  *"test result: ok. 1 passed"*)
    echo "  ok   cap_version survives a forward and a restart — 1.1 read off every request at both peers, both request orders" ;;
  *) echo "  FAIL cap_version across a forward"
     diag "a panic or an assertion's left/right" "$capout" \
          "$(grep -E "panicked|left:|right:" <<<"$capout")" 4
     fail_total=$((fail_total+1)) ;;
esac

hr "LOCATION_FORWARD vs _PERM — fallback-on-failure: the forwarded-to server killed, does the client go back?"
# The hardest half of location transparency: the place the target moved TO dies,
# and the caller must still be able to reach it without knowing either address.
bears_on location
# 680aa41: a request count is 1 under both statuses. The oracle is §9.6:
# temporary -> the client shall restart at the original address; permanent ->
# it may have replaced the reference. Measured 2026-08-19 (af73b2f): omniORB
# 4.3.4 re-asks under temporary and stays on the dead address under
# permanent — asserted. Server-side control in af73b2f: PERM downgraded to 3
# -> omniORB re-asks -> red. Since 3ab23d5 our own clients do the same and are
# asserted alongside: Connection keeps its origin and restarts there when a
# temporary forward's target fails with the request provably unsent
# (CloseConnection, write failure, poisoned at entry — never on unknown
# completion), a permanent forward replaces the origin; Reference caches a
# temporary forward, restarts the same way, re-points on permanent. Ten cells,
# both byte orders. Negative controls (3ab23d5): `let temporary = false` ->
# the temporary arm red; `--only omni --expect-permanent reask` -> red.
pfout=$(./spikes/perm_fallback.sh --expect-temporary reask --expect-permanent stay 2>&1); pfrc=$?
printf '%s\n' "$pfout" | grep -E '^  (ok|FAIL|SKIPPED|\.\.) ' | cut -c1-150
case "$pfrc" in
  0) ;;
  2) skip_age absent git:spikes/perm_fallback.sh ;;
  *) fail_total=$((fail_total+1)) ;;
esac

# ── Registry: does IDL-derived type metadata match the wire? ────────────────
hr "type registry — TypeCode derived from IDL vs the peer's"
# Deriving a TypeCode and encoding it with our own encoder proves only that two
# pieces of our code agree. The question is whether a stock ORB produces the
# same description from the same IDL.
if start_server_omni_echo 2>/dev/null || start_server; then
  rc=$(cargo run -q --bin registry-check -- spikes/echo.ior spikes/echo.idl spike::Ragged 2>/dev/null)
  if grep -q "registry: PASS" <<<"$rc"; then
    echo "  ok   omniORB agrees with the TypeCode we derived for spike::Ragged"
  else
    echo "  FAIL omniORB disagrees with our derived TypeCode"
    diag "a derived/returned line" "$rc" "$(grep -E "derived|returned" <<<"$rc")" 2
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jreg.log 2>&1 & )
  jr=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jr=1; break; }
    sleep 0.1
  done
  if [ "$jr" -eq 1 ]; then
    rc=$(cargo run -q --bin registry-check -- spikes/jacorb.ior spikes/echo.idl spike::Ragged 2>/dev/null)
    if grep -q "registry: PASS" <<<"$rc"; then
      echo "  ok   JacORB agrees too — two independent derivations of one IDL type"
    else
      echo "  FAIL JacORB disagrees with our derived TypeCode"; fail_total=$((fail_total+1))
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; fail_total=$((fail_total+1))
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi

# ── Naming: resolve a target the way a deployment does ───────────────────────
hr "object-reference acquisition — corbaname: through a real naming service"
# The caller holds a NAME and never learns an address at all — the strongest
# form of D029 §6.1's location row, because the property is absent from the
# caller rather than merely unused by it.
bears_on location
fkill omniNames
fkill register_name
sleep 0.5
rm -rf /tmp/orbweaver-names && mkdir -p /tmp/orbweaver-names
# `port_open` is defined with the other helpers at the top of this file. It was
# defined *here*, inside this group, and the ORB group below then used it from
# two hundred lines away — so a group that ran on its own, or a reordering of
# these two, got `port_open: command not found` and reported **"omniNames did
# not start"**, which is a misdiagnosis rather than a failure. Found by running
# the ORB group standalone; nothing in a whole-harness run would have shown it.
if ! command -v omniNames >/dev/null 2>&1; then
  # No date: omniNames is a package on the machine, not a file in this tree,
  # so nothing here can say when it last answered. `date not recorded` is the
  # honest line and an invented one would be worse than none.
  skip absent "" "omniNames is not installed — naming is unmeasured, not passing"
  names_up=-1
else
  ( omniNames -start 2809 -logdir /tmp/orbweaver-names >/tmp/orbweaver-names/out.log 2>&1 & )
  names_up=0
  for _ in $(seq 1 60); do
    port_open 2809 && { names_up=1; break; }
    sleep 0.2
  done
fi
if [ "$names_up" -eq 0 ]; then
  echo "  FAIL omniNames did not start on 2809"
  if [ -s /tmp/orbweaver-names/out.log ]; then
    tail -8 /tmp/orbweaver-names/out.log | sed 's/^/       | /'
  else
    echo "       it wrote nothing at all"
  fi
  fail_total=$((fail_total+1))
elif [ "$names_up" -eq -1 ]; then
  :
else
  ( cd "$ROOT/spikes" && exec python3 register_name.py >/tmp/orbweaver-reg.log 2>&1 & )
  reg_up=0
  for _ in $(seq 1 100); do
    grep -q READY /tmp/orbweaver-reg.log 2>/dev/null && { reg_up=1; break; }
    sleep 0.1
  done
  if [ "$reg_up" -eq 0 ]; then
    echo "  FAIL could not bind a name into the naming service"; fail_total=$((fail_total+1))
  else
    nm=$(cargo run -q --bin spike-naming 2>&1)
    if grep -q "naming: PASS" <<<"$nm"; then
      nm_oks=$(grep "^  ok" <<<"$nm")
      if [ -n "$nm_oks" ]; then
        sed 's/^/  /' <<<"$nm_oks"
      else
        echo "    ok   spike-naming: PASS, but it listed no '  ok' lines to show"
      fi
      # The default in corbaloc::host is GIOP 1.0, so this path only works
      # because of the version negotiation from batch 1. Assert it rather than
      # let a silent upgrade to 1.2 hide a regression.
      if grep -q "GIOP 1.0" <<<"$nm"; then
        echo "  ok   naming service contacted at GIOP 1.0, as corbaloc defaults require"
      else
        echo "  FAIL expected GIOP 1.0 for a corbaloc URL with no version"; fail_total=$((fail_total+1))
      fi
    else
      echo "  FAIL naming resolution"
      diag "a fail or error line" "$nm" "$(grep -iE "fail|error" <<<"$nm")" 3
      fail_total=$((fail_total+1))
    fi
  fi
fi
fkill register_name
fkill omniNames

# ── D019: the ORB object — a table, a URL with no address, three refusals ───
hr "ORB initial references — corbaloc:rir: out of OUR table to a foreign servant"
# Leg A resolves a URL carrying NO ADDRESS AT ALL and a foreign servant in
# another process answers a real call. Location, and the group says so itself:
# "D019 calls this the ORB's whole point."
bears_on location
# `rir` means *resolve initial references*, and CORBA 3.4 §8.5.2 is explicit
# that the mechanism is **local**: *"a simplified, local version of the Naming
# Service."* So handing `corbaloc:rir:NameService` to omniORB's client measures
# **omniORB's** table, however green it comes back, and says nothing about ours.
# The direction that measures ours is the other one, and it is leg A: our `Orb`
# is told where NameService is, resolves a URL carrying **no address at all**
# out of its own table, dials what comes back, and a foreign servant — omniNames
# in a separate process — answers a real call. `spike-rir` had existed and run
# nowhere; D019 calls this the ORB's whole point.
#
# Legs B/C/D are the three states an operator has to be able to tell apart,
# because the fixes differ: register the service, or fix the spelling. Leg E is
# the same three states asked of the peer, which is where the claim
# `orbweaver-console`'s RESOLUTION_NOTE cites came from — it was a date in a doc
# comment with no gate under it until now.
rir_fail=0
fkill omniNames
fkill register_name
sleep 0.5
rm -rf /tmp/orbweaver-rir-names && mkdir -p /tmp/orbweaver-rir-names
if ! command -v omniNames >/dev/null 2>&1; then
  # D010 §2: a counted SKIPPED naming its fixture, never a note and never an ok.
  skip absent "" \
       "omniNames is not installed (fixture: omniNames on 2809, plus" \
       "spikes/register_name.py) — the ORB's initial-references table is" \
       "unmeasured, not passing"
else
  ( omniNames -start 2809 -logdir /tmp/orbweaver-rir-names \
      >/tmp/orbweaver-rir-names/out.log 2>&1 & )
  rir_up=0
  for _ in $(seq 1 60); do
    port_open 2809 && { rir_up=1; break; }
    sleep 0.2
  done
  if [ "$rir_up" -eq 1 ]; then
    ( cd "$ROOT/spikes" && exec python3 register_name.py >/tmp/orbweaver-rir-reg.log 2>&1 & )
    rir_reg=0
    for _ in $(seq 1 100); do
      grep -qs READY /tmp/orbweaver-rir-reg.log && { rir_reg=1; break; }
      sleep 0.1
    done
  else
    rir_reg=0
  fi

  # An unmeasured check is a failure, never a pass — and the fixture is checked
  # here so that every exit 1 below means "the claim was refuted" rather than
  # "nothing was listening". That is the distinction `spikes/ssliop.sh` makes
  # with its exit 3, made on this side because `spike-rir` has only 0 and 1.
  if [ "$rir_up" -ne 1 ]; then
    echo "  FAIL omniNames did not start on 2809 — the ORB table is UNMEASURED, not passing"
    if [ -s /tmp/orbweaver-rir-names/out.log ]; then
      tail -8 /tmp/orbweaver-rir-names/out.log | sed 's/^/       | /'
    else
      echo "       it wrote nothing at all"
    fi
    rir_fail=1
  elif [ "$rir_reg" -ne 1 ]; then
    echo "  FAIL nothing could be bound into the naming service — the ORB table is"
    echo "       UNMEASURED, not passing"
    diag_log /tmp/orbweaver-rir-reg.log 6
    rir_fail=1
  else
    # ── A. our table, a URL with no address, and a foreign servant answering ──
    rir_out=$(cargo run -q -p orbweaver-giop --bin spike-rir 2>&1); rir_rc=$?
    # 9 is a FLOOR on the checks spike-rir counted, not today's figure: a binary
    # whose body stopped early can still exit 0, and `ok` lines are what it has
    # to show for the run.
    rir_oks=$(grep -c '^  ok   ' <<<"$rir_out")
    if [ "$rir_rc" -ne 0 ]; then
      echo "  FAIL our ORB could not bootstrap through its own table (exit $rir_rc) — the"
      echo "       fixture was verified up first, so this is a refuted claim and not an"
      echo "       unmeasured one"
      diag_out "$rir_out" 8
      rir_fail=1
    elif [ "$rir_oks" -lt 9 ]; then
      echo "  FAIL spike-rir exited 0 over $rir_oks checks (floor 9) — it measured less than it has"
      rir_fail=1
    else
      echo "  ok   our Orb resolved corbaloc:rir:NameService out of its OWN table — a URL with"
      echo "       no address — dialled it, and omniNames answered ping() -> 42, $rir_oks checks"
    fi

    # ── B. an empty table refuses; §8.5.2 forbids answering with a nil ──
    emp_out=$(cargo run -q -p orbweaver-giop --bin spike-rir -- --empty-table 2>&1); emp_rc=$?
    emp_say=$(grep '^rir: FAIL' <<<"$emp_out")
    if [ "$emp_rc" -eq 0 ]; then
      echo "  FAIL an ORB with nothing in its table RESOLVED corbaloc:rir:NameService —"
      echo "       §8.5.2 forbids both answering with a nil reference and inventing one"
      rir_fail=1
    elif ! grep -q '"NameService"' <<<"$emp_say"; then
      echo "  FAIL the empty table's refusal did not name the ObjectId it refused, so it"
      echo "       is correct and useless: | ${emp_say:-(no refusal line at all)}"
      rir_fail=1
    else
      echo "  ok   an empty table refuses corbaloc:rir:NameService BY NAME rather than"
      echo "       answering the nil reference §8.5.2 rules out"
    fi

    # ── C/D. reserved-and-unbound vs never-defined, told apart from outside ──
    # Both refusals, so neither can be read off an exit code. They are compared
    # instead — down the SAME code path, differing only in the ObjectId, so the
    # only thing that can make the sentences differ is the distinction itself.
    #
    # And the comparison does NOT retype a substring of the sentence
    # `InvalidName` owns: a classifier built from a hand-copied phrase goes
    # green the day the wording improves, which this project has now measured
    # five times. It blanks every quoted string — the ObjectIds, which the
    # harness itself supplied — and requires what is left to still DIFFER. If
    # the reserved/not-reserved clause were dropped, the two lines would become
    # one sentence and this goes red without knowing a word of it.
    #
    # The first draft of this leg compared `--empty-table` against
    # `--peer corbaloc:rir:NoSuchService`, and its negative control killed it:
    # those are two different code paths inside spike-rir, so their refusals
    # carry different prefixes and differed no matter what — green while
    # measuring nothing, found by the control and not by review.
    resv_out=$(cargo run -q -p orbweaver-giop --bin spike-rir -- \
                 --peer corbaloc:rir:NameService 2>&1); resv_rc=$?
    typo_out=$(cargo run -q -p orbweaver-giop --bin spike-rir -- \
                 --peer corbaloc:rir:NoSuchService 2>&1); typo_rc=$?
    resv_say=$(grep '^rir: FAIL' <<<"$resv_out")
    typo_say=$(grep '^rir: FAIL' <<<"$typo_out")
    resv_blank=$(sed 's/"[^"]*"/"<id>"/g' <<<"$resv_say")
    typo_blank=$(sed 's/"[^"]*"/"<id>"/g' <<<"$typo_say")
    if [ "$resv_rc" -eq 0 ] || [ "$typo_rc" -eq 0 ]; then
      echo "  FAIL an unregistered ObjectId was RESOLVED (reserved exit $resv_rc,"
      echo "       never-defined exit $typo_rc)"
      rir_fail=1
    elif ! grep -q '"NameService"' <<<"$resv_say" \
      || ! grep -q '"NoSuchService"' <<<"$typo_say"; then
      echo "  FAIL a refusal did not name the ObjectId it refused:"
      echo "       | ${resv_say:-(the reserved run printed no refusal line)}"
      echo "       | ${typo_say:-(the never-defined run printed no refusal line)}"
      rir_fail=1
    elif [ "$resv_blank" = "$typo_blank" ]; then
      echo "  FAIL a RESERVED ObjectId with nothing bound and an ObjectId nobody ever"
      echo "       defined got the SAME refusal — a missing registration and a typo need"
      echo "       different fixes, and an operator cannot tell which they have:"
      echo "       | $resv_blank"
      rir_fail=1
    else
      echo "  ok   three states, three answers: a registered id resolves; a RESERVED id with"
      echo "       nothing bound and an id nobody defined are both refused, by name, and"
      echo "       NOT in the same words"
    fi

    # ── E. the same three states asked of the peer, re-taken live ──
    if ! python3 -c "import CORBA, omniORB" >/dev/null 2>&1; then
      skip absent git:spikes/rir_peer.py \
           "omniORBpy absent (fixture: spikes/rir_peer.py needs it) — the peer half" \
           "of the three-state claim is unmeasured, not passing"
    else
      rp_out=$(python3 spikes/rir_peer.py 2>&1); rp_rc=$?
      rp_legs=$(sed -n 's/^rir-peer: every leg answered as expected (\([0-9][0-9]*\) legs)$/\1/p' <<<"$rp_out")
      case "$rp_rc" in
        0)
          if [ -z "$rp_legs" ] || [ "$rp_legs" -lt 9 ]; then
            echo "  FAIL the rir peer exited 0 over ${rp_legs:-no} legs (floor 9) — it measured"
            echo "       less than it has"
            rir_fail=1
          else
            echo "  ok   omniORB makes the same three-way distinction, re-taken live, $rp_legs legs:"
            echo "       registered resolves and reaches a live servant; reserved-and-unbound is"
            echo "       NO_RESOURCES; never-reserved is BAD_PARAM by URL and InvalidName by §8.5.2"
          fi ;;
        3)
          echo "  FAIL the rir peer measured NOTHING (exit 3) — omniORBpy imported but no leg"
          echo "       reached the peer, so no claim was refuted and there is no defect to chase"
          diag_out "$rp_out" 3
          rir_fail=1 ;;
        *)
          echo "  FAIL omniORB no longer answers the three states as recorded (exit $rp_rc)"
          diag_out "$rp_out" 10
          rir_fail=1 ;;
      esac
    fi
  fi
fi
fkill register_name
fkill omniNames
[ "$rir_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Second peer: JacORB, both directions ─────────────────────────────────────
hr "second peer — JacORB client -> our server (independent implementation)"
# PLAN §8 says "one run_checks.sh group per cell". Until 2026-08-19 this was
# one group for JacORB's two directions with one counter, so a green harness
# could not tell "both passed" from "the first failed and the second was never
# reached" — the class the SKIPPED discipline exists to prevent. Two groups
# now, two counters. Negative control (2026-08-19): with the JacORB Server
# jar's Server class made to exit before publishing its IOR, exactly the
# second group went red and this one stayed green.
JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"
jacorb_ready=0
if [ ! -d "$ROOT/spikes/jacorb/classes" ] || [ ! -x "$JH/bin/java" ]; then
  # Not a pass. An absent fixture means the claim is unmeasured, and the
  # summary says so rather than letting silence read as success.
  skip absent git:spikes/jacorb/setup.sh \
       "fixture absent — run spikes/jacorb/setup.sh (needs JDK 21)"
else
  jacorb_ready=1
  jfail=0
  if start_rust_server; then
    out=$(cd "$ROOT/spikes/jacorb" && "$JH/bin/java" -cp "$JCP" Client ../server.ior 2>&1)
    if grep -q "failures: 0" <<<"$out"; then
      echo "  ok   JacORB client -> our server, 5/5"
    else
      echo "  FAIL JacORB client -> our server"
      diag "a FAIL line" "$out" "$(grep FAIL <<<"$out")" 3
      jfail=1
    fi
    # JacORB is big-endian where omniORB was little-endian, so this exercises a
    # decode path the first peer never touched. Worth asserting, not assuming.
    if grep -q "first request at GIOP 1.2 (Big)" /tmp/orbweaver-srv.log 2>/dev/null; then
      echo "  ok   big-endian request path exercised by the second peer"
    else
      echo "  FAIL expected a big-endian request from JacORB"; jfail=1
    fi
  else
    jfail=1
  fi
  fkill spike-server
  [ "$jfail" -eq 0 ] || fail_total=$((fail_total+1))
fi

hr "second peer — our client -> JacORB server"
if [ "$jacorb_ready" -eq 0 ]; then
  skip absent git:spikes/jacorb/setup.sh \
       "fixture absent — run spikes/jacorb/setup.sh (needs JDK 21)"
else
  jfail=0
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH/bin/java" -cp "$JCP" Server ../jacorb.ior >/tmp/orbweaver-jacorb.log 2>&1 & )
  jup=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jup=1; break; }
    sleep 0.1
  done
  if [ "$jup" -eq 1 ]; then
    out=$(cargo run -q --bin spike-interop -- spikes/jacorb.ior 2>&1)
    if grep -q "assumption A: PASS" <<<"$out"; then
      echo "  ok   our client -> JacORB server, 20/20 both byte orders"
      # `grep -m1` on a pipe is the same early-exit hazard as `grep -q`, and it
      # had a second defect on top: when nothing matched, `$cs` was empty and
      # the line below still printed `ok codeset negotiated with a second
      # peer:` followed by nothing — an `ok` asserting a measurement that was
      # not taken. Match on a herestring, take the first line without a pipe,
      # and make the empty case a failure.
      cs=$(head -1 <<<"$(grep "negotiated char codeset" <<<"$out" | sed 's/.*: //')")
      if [ -n "$cs" ]; then
        echo "  ok   codeset negotiated with a second peer: $cs"
      else
        echo "  FAIL our client printed no negotiated char codeset against JacORB, so the"
        echo "       codeset claim is UNMEASURED — not an ok with an empty value after it"
        jfail=1
      fi
    else
      echo "  FAIL our client -> JacORB server"
      diag "a FAIL line" "$out" "$(grep "  FAIL" <<<"$out")" 3
      jfail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; jfail=1
  fi
  fkill "classes Server"
  [ "$jfail" -eq 0 ] || fail_total=$((fail_total+1))
fi

hr "GIOP 1.1 against JacORB — version from the wire, then wide text each way: wstring and the single wchar (D010 B5)"
# spikes/jacorb_giop11.sh: a recording tap republishes the IOR at 1.1 and
# parses every GIOP header it relays; our server's log is the second witness.
# JacORB's giop_minor_version sets the version of the IORs it *creates*; its
# client follows the profile it dials — so the lever is the profile, not the
# property, and the group asserts the version from bytes. Wide text at 1.1 was
# a measured FAIL on landing day: we wrote a byte-order mark JacORB neither
# writes nor strips at 1.1, its user got U+FEFF + text, and its echo of our
# mark came back as data that our reader then stripped — spike-interop's own
# "wstring round-tripped under GIOP 1.1" was green while the peer's user saw
# the wrong value. Fixed the same day (codeset.rs); step 4 of the script
# counts the units on the wire so that masking cannot recur. Negative
# controls: `--expect-minor 2` goes red on the version line; the pre-fix tap
# log fails step 4 in 4 of 4 exchanges.
g11=$(./spikes/jacorb_giop11.sh 2>&1); g11_rc=$?
printf '%s\n' "$g11" | grep -E "^  (ok|FAIL|info|SKIPPED)" | cut -c1-150
if [ "$g11_rc" -eq 2 ]; then
  skip_age absent git:spikes/jacorb_giop11.sh
elif [ "$g11_rc" -ne 0 ]; then
  echo "  FAIL GIOP 1.1 against JacORB — see /tmp/orbweaver-giop11"
  fail_total=$((fail_total+1))
fi
# spikes/jacorb_wchar11.sh: the single wide character (spikes/wide.idl —
# echo.idl has none). A 1.1 wchar has no length indication and nowhere for a
# mark; the only question is the order of its two octets. Measured 2026-08-19
# (382baa9): JacORB reads it in the MESSAGE's order (a little-endian reply with
# the unit in message order reaches its user as sent; big-endian units in the
# same frame reach it swapped, 4/4) and writes it in its message's order;
# U+FEFF is data. The recording in tests/wide_1_1_from_a_peer.rs is re-checked
# against the live octets on every run. Negative control: `--expect-han 5CD5`
# goes red (3 lines).
w11=$(./spikes/jacorb_wchar11.sh 2>&1); w11_rc=$?
printf '%s\n' "$w11" | grep -E "^  (ok|FAIL|info|SKIPPED)" | cut -c1-150
if [ "$w11_rc" -eq 2 ]; then
  skip_age absent git:spikes/jacorb_wchar11.sh
elif [ "$w11_rc" -ne 0 ]; then
  echo "  FAIL 1.1 wchar against JacORB — see /tmp/orbweaver-wchar11"
  fail_total=$((fail_total+1))
fi
# spikes/wide_rust.sh (ff2c742, f77a50c): wide.idl with OUR OWN stack in each
# seat — spike-wide serves and dials IDL:spike/Wide:1.0 through
# Server/Connection; 382baa9's matrix re-run with the real Rust server and
# client at 1.1 AND 1.2, 1.0/1.1/1.2 self-consistency in both orders, JacORB's
# 1.2 wchar reader driven with the recorded forms of tests/wide_1_2_from_a_peer.rs
# (JACORB_READER_1_2, 13 forms both message orders), and the live octets checked
# against wide_1_1_ and wide_1_2_from_a_peer.rs on every run. U+FEFF/U+FFFE as a
# 1.2 wchar cross both ways: ours marked (04 fe ff fe ff), JacORB's bare
# (02 fe ff) read as the unit — the day's fourth wire defect, measured against
# the peer's reader before the writer changed. Negative control:
# `--expect-han 5CD5` -> 12 FAIL lines, rc 1.
wr=$(./spikes/wide_rust.sh 2>&1); wr_rc=$?
printf '%s\n' "$wr" | grep -E "^  (ok|FAIL|info|SKIPPED)" | cut -c1-150
if [ "$wr_rc" -eq 2 ]; then
  skip_age absent git:spikes/wide_rust.sh
elif [ "$wr_rc" -ne 0 ]; then
  echo "  FAIL wide.idl from our Rust stack — see /tmp/orbweaver-wide-rust"
  fail_total=$((fail_total+1))
fi

# ── S4, the validation gate ──────────────────────────────────────────────────
hr "S4 validation gate — diagnostics a generator can act on"
# §5: everything upstream of S4 is allowed to be uncertain because S4 is not.
# §3.3: the self-repair loop is only as good as the messages it feeds on, so
# fix-hint coverage is measured here rather than assumed.
s4_fail=0
if ! cargo run -q --bin sidl-validate -- corpus/golden/*.idl \
     corpus/requirements/generated/*.idl spikes/*.idl >/tmp/orbweaver-s4.log 2>&1; then
  echo "  FAIL the gate rejected IDL both oracles accept"
  diag "an 'error:' line" "$(tail -40 /tmp/orbweaver-s4.log 2>/dev/null)" \
       "$(grep "error:" /tmp/orbweaver-s4.log)" 3
  s4_fail=1
else
  echo "  ok   accepts all $(ls corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl | wc -l | tr -d ' ') valid files"
fi
s4_bad=""
for f in corpus/negative/*.idl; do
  cargo run -q --bin sidl-validate -- "$f" >/dev/null 2>&1 && s4_bad="$s4_bad $(basename "$f")"
done
if [ -z "$s4_bad" ]; then
  echo "  ok   rejects all $(ls corpus/negative/*.idl | wc -l | tr -d ' ') negatives"
else
  echo "  FAIL accepted:$s4_bad"; s4_fail=1
fi
# The measurement §3.3 asks for. Reported as a number, not as a pass: a fix
# hint that cannot be given honestly is better absent than invented.
s4_json=$(cargo run -q --bin sidl-validate -- --json corpus/negative/*.idl 2>/dev/null)
s4_cov=$(printf '%s' "$s4_json" | grep -o '"fix"' | wc -l | tr -d ' ')
s4_tot=$(ls corpus/negative/*.idl | wc -l | tr -d ' ')
echo "  ok   $s4_cov of $s4_tot rejections carry an actionable fix (a missing separator has no unambiguous one)"
[ "$s4_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Contract and property gate ───────────────────────────────────────────────
# --wire v1 (PLAN §4.4, 18c1ef1): the same golden files that pass the default
# form are refused by the strict one — exactly those that reach a deferred
# construct through members, signatures, raises or inheritance. The rule is a
# warning by default because golden 20/21 exist to pin that these constructs
# *parse*; the pipeline's S4 gates for v1, because a contract a model just
# wrote for this ORB is a different caller. Negative control (in that commit):
# WireGate::V1's severity forced to Warning -> this prints "refused: ''".
# --against over a multi-file contract (6c37e68): the command resolves both
# sides — it must, or one missing #include is one diagnostic per name the
# absent file declared — and then handed both `Unit::text` splices back to the
# string entry point, which re-preprocessed each. A spliced header's `#ifndef`
# is not the first directive of the text it sits in, so it read as conditional
# compilation and the §5.3 comparison **never ran** over a guarded multi-file
# contract — the ordinary shape of a released one — while still exiting 1, so
# nothing looked wrong. Exit codes only, no marker greps: the pair is its own
# control, since reverting the fix makes the first exit 1 for the wrong reason
# AND the second exit 1 too.
if cargo run -q --bin sidl-validate -- --against corpus/include/evo-released.idl \
     corpus/include/evo-proposed.idl >/dev/null 2>&1; then
  echo "  FAIL --against accepted a proposal that removes a member from an included header"
  s4_fail=1
elif cargo run -q --bin sidl-validate -- --against corpus/include/evo-released.idl \
       corpus/include/evo-released.idl >/dev/null 2>&1; then
  echo "  ok   --against compares two resolved units: a header's breaking change is refused, a contract against itself is not"
else
  echo "  FAIL --against refused a contract compared against itself"
  s4_fail=1
fi
s4_wire_out=$(cargo run -q --bin sidl-validate -- --wire v1 corpus/golden/*.idl 2>/dev/null)
s4_wire_files=$(printf '%s\n' "$s4_wire_out" | grep 'error: .*\[wire/deferred-type\]' \
  | cut -d: -f1 | sort -u | xargs -n1 basename | tr '\n' ' ')
# Three files until 2026-08-21, when `native` joined the closure as a fourth
# family that is not a deferral: `31-native-type` and `32-valuebase` are refused
# under --wire v1 for two different reasons, and the rule says which.
if [ "$s4_wire_files" = "20-deferred-valuetype.idl 21-deferred-fixed.idl 31-native-type.idl 32-valuebase.idl deferred-reach.idl " ]; then
  echo "  ok   --wire v1 refuses exactly the five golden files that reach a construct the wire cannot carry"
else
  echo "  FAIL --wire v1 refused: '$s4_wire_files' (expected 20, 21, 31, 32, deferred-reach)"
  fail_total=$((fail_total+1))
fi

hr "contract-check — seeded round-trip property plus annotation contract advice"
# Two gates with deliberately different force. A byte-instability in the
# marshalling core is a defect and fails the run; an annotation finding is
# advice about meaning, which no deterministic checker can promote to a verdict
# without inventing a policy the project has not decided. S4 gates syntax and
# semantics; this gates what the annotations claim.
cc_out=$(cargo run -q -p orbweaver-test --bin contract-check -- corpus/golden/*.idl 2>&1)
cc_rc=$?
if [ "$cc_rc" -ne 0 ]; then
  diag "a defect or error line" "$cc_out" "$(grep -i "defect\|error" <<<"$cc_out")" 3
  echo "  FAIL byte instability in the marshalling core"
  fail_total=$((fail_total+1))
else
  cc_line=$(sed -n '1p' <<<"$(tail -2 <<<"$cc_out")")
  echo "  ok   ${cc_line:-(contract-check exited 0 and printed no summary line)}"
fi
# A property case that produced no value ran nothing, and until 2026-08-19 it
# fell through a bare `continue`: golden 15's TreeSeq was `[]` on every valued
# case and None on 22 of 32, while the summary line still said "32 cases".
# It is a `prop/unmeasured` finding now (1b6b4c8). Captured then matched.
if grep -q "prop/unmeasured" <<<"$cc_out"; then
  echo "  FAIL a property case produced no value and therefore ran nothing"
  sed -n '1,3p' <<<"$(grep "prop/unmeasured" <<<"$cc_out")" | sed 's/^/       /'
  fail_total=$((fail_total+1))
else
  echo "  ok   every property case produced a value (no prop/unmeasured)"
fi
# SIDL has a version (40a4729): `SIDL_VERSION = "1"` beside both vocabulary
# copies, pinned equal across crates; a contract may declare
# `//@ sidl_version: N` and an unknown one is a Warning at S3 and S7. Golden
# must be v1 (marker or none); the positive probe runs the checker over a
# scratch copy declaring 2 and requires the finding — unmeasured is not passing.
if grep -q "unknown-sidl-version" <<<"$cc_out"; then
  echo "  FAIL a golden contract declares a SIDL version this checker does not read"
  fail_total=$((fail_total+1))
else
  sv_tmp=$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-sidlv.XXXXXX")
  sed 's|^//@ sidl_version: 1|//@ sidl_version: 2|' corpus/golden/19-realistic-service.idl > "$sv_tmp/v2.idl"
  sv_out=$(cargo run -q -p orbweaver-test --bin contract-check -- "$sv_tmp/v2.idl" 2>&1)
  rm -rf "$sv_tmp"
  if grep -q "unknown-sidl-version" <<<"$sv_out"; then
    echo "  ok   every golden contract is SIDL v1 (marker or none; golden 19 declares it), and a v2 marker is refused to be understood"
  else
    echo "  FAIL the SIDL version check did not fire on a v2 marker (unmeasured is not passing)"
    fail_total=$((fail_total+1))
  fi
fi
# The JSON leg (9fa89ee) is a count, not a finding: a leg that stops running
# prints the same findings, so its floor is pinned — 5248 = 82 mapped golden
# types x 32 cases x 2 byte orders, and every CDR round trip must have crossed.
# Negative controls in that commit: to_json dropping the last struct member ->
# 2712 json/from-json-error; -0.0 flattened -> 70 json/roundtrip-bytes.
cc_json=$(printf '%s\n' "$cc_out" | sed -n 's/.* \([0-9][0-9]*\) of \([0-9][0-9]*\) CDR round trip(s) also taken across AnyJSON.*/\1 \2/p')
set -- $cc_json
if [ -z "${1:-}" ] || [ "$1" -lt 5248 ] || [ "$1" -ne "$2" ]; then
  echo "  FAIL AnyJSON leg: '${cc_json:-absent}' (need every CDR round trip crossed, >= 5248)"
  fail_total=$((fail_total+1))
else
  echo "  ok   $1 of $2 CDR round trips also crossed AnyJSON, byte-equal, both orders"
fi
# §4.4's count over golden (18c1ef1): the declarations the v1 wire cannot
# carry, and how many of them the property sweep cannot even sample. Pinned
# rather than gated to zero — golden 20/21/deferred-reach exist to carry them.
# 19/7 until 2026-08-20, when a valuetype and an abstract interface stopped
# being recorded as object references (74b5662): three valuetypes and a struct
# stopped being sampled *as references*, so the property measures four fewer
# and the closure names one more.
# 20/12 until 2026-08-21, when a `native` and a `ValueBase` stopped being
# recorded as object references too (22637a8): six native declarations and four
# ValueBase ones joined the closure, and the label changed with it -- the set is
# no longer "§4.4" alone, because a native is not deferred, there is nothing to
# defer. The unmeasured half went 12 -> 18 and not 20: a sequence of an
# unsamplable element has one value, the empty one, and that one is measured.
cc_wire=$(printf '%s\n' "$cc_out" | sed -n 's/.* \([0-9][0-9]*\) declaration(s) the wire cannot carry (§4.4 and natives) of which \([0-9][0-9]*\) unmeasured.*/\1 \2/p')
set -- $cc_wire
if [ "${1:-}" = 30 ] && [ "${2:-}" = 18 ]; then
  echo "  ok   30 declaration(s) over golden the wire cannot carry (§4.4 and natives), 18 unmeasured by the property"
else
  echo "  FAIL deferred-wire count over golden: '${cc_wire:-absent}' (pinned 30 of which 18)"
  fail_total=$((fail_total+1))
fi
# Panic freedom. Rust rules out the memory-corruption half of "wire parsing is
# the classic memory-safety hazard" at compile time and rules out nothing about
# panics — a slice index or an unwrap reachable from a peer's bytes ends the
# process just as surely, and `unsafe_code = "forbid"` does not cover it.
# Reported with its reach, because a fuzz that bounces off the header check
# every time is green and worthless and the exit code cannot tell you which.
wf_out=$(cargo run -q --release -p orbweaver-test --bin wire-fuzz -- --cases 20000 2>&1)
if grep -q "wire-fuzz: PASS" <<<"$wf_out"; then
  wf_head=$(sed -n '1p' <<<"$wf_out")
  echo "  ok   ${wf_head#wire-fuzz: }"
  printf '%s' "$wf_out" | sed -n '2,3p' | sed 's/^  /  ok   /'
  # A target that reached nothing is green and worthless, and only a reader of
  # this line can turn the binary's own warning into a failure. Matched on the
  # zero-reach wording rather than on "WARNING:", because the same binary also
  # warns — correctly — that a release build cannot observe arithmetic
  # overflow. The first version of this check read that note as a missing
  # target and turned a correct run red.
  if grep -q "were reached; the target" <<<"$wf_out"; then
    printf '%s' "$wf_out" | grep "were reached; the target" | sed 's/^ */       /'
    echo "  FAIL a fuzz target reached nothing; its green result measures nothing"
    fail_total=$((fail_total+1))
  fi
  # The overflow note is not a failure; it is the scope of the green above it.
  printf '%s' "$wf_out" | grep "overflow-checks" | sed 's/^ */  note /'
else
  diag "a FAIL line" "$wf_out" "$(grep "FAIL" <<<"$wf_out")" 3
  echo "  FAIL a decoder panicked on bytes a peer can send"
  fail_total=$((fail_total+1))
fi

# ── Dynamic invocation: calling with nothing generated ───────────────────────
hr "dynamic invocation — calls built from IDL text alone"
# The whole AI path rests on this: invoke_operation gets a name and a bag of
# values at runtime and has only the registry to work from. Checked against
# peers we did not write, because a dynamic invoker that agrees only with our
# own decoder has not been tested.
dyn_fail=0
if start_server; then
  dv=$(cargo run -q --bin spike-dynamic -- spikes/echo.ior spikes/echo.idl \
       IDL:spike/Echo:1.0 2>&1)
  if grep -q "dynamic invocation: PASS" <<<"$dv"; then
    echo "  ok   omniORB answered 8 dynamically built calls, both byte orders"
    echo "  ok   wrong arguments are refused locally, before anything is sent"
    echo "  ok   a refused call leaves the connection usable"
  else
    echo "  FAIL a dynamically built call did not work against omniORB"
    diag "a FAIL line" "$dv" "$(grep "FAIL" <<<"$dv")" 3
    dyn_fail=1
  fi
else
  dyn_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jdyn.log 2>&1 & )
  jd=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jd=1; break; }
    sleep 0.1
  done
  if [ "$jd" -eq 1 ]; then
    dv=$(cargo run -q --bin spike-dynamic -- spikes/jacorb.ior spikes/echo.idl \
         IDL:spike/Echo:1.0 2>&1)
    if grep -q "dynamic invocation: PASS" <<<"$dv"; then
      echo "  ok   JacORB answered them too — a second, independent decoder"
    else
      echo "  FAIL a dynamically built call did not work against JacORB"
      diag "a FAIL line" "$dv" "$(grep "FAIL" <<<"$dv")" 3
      dyn_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; dyn_fail=1
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi
[ "$dyn_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MCP boundary ─────────────────────────────────────────────────────────
hr "MCP bridge — an agent session with no address in it"
# §4.7: an IOR is a bearer address, so an agent holding one is past the guard,
# past destructive approval and past the audit log. The check is not that the
# calls work — it is that the transcript the agent saw contains no host, port,
# object key or stringified IOR. A leak is a failure even when every call
# succeeded, because that is the shape it would ship in.
mcp_fail=0
if start_server; then
  mv=$(cargo run -q --bin spike-mcp -- spikes/echo.ior spikes/echo.idl \
       IDL:spike/Echo:1.0 2>&1)
  if grep -q "MCP bridge: PASS" <<<"$mv"; then
    echo "  ok   default-deny: an un-allowlisted catalog is invisible"
    echo "  ok   search -> describe -> invoke, entirely in JSON, nothing generated"
    echo "  ok   a returned object reference crosses as a handle and can be passed back"
    echo "  ok   destructive operations need approval; other sessions' handles are worthless"
    printf '%s' "$mv" | grep "JSON message(s) contain no host" | sed 's/^  ok /  ok  /'
  else
    echo "  FAIL the MCP boundary did not hold"
    diag "a FAIL line" "$mv" "$(grep "FAIL" <<<"$mv")" 4
    mcp_fail=1
  fi
else
  mcp_fail=1
fi
cleanup
[ "$mcp_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MCP transport, as a client drives it ─────────────────────────────────
hr "MCP over stdio — a real client session"
# The bridge is exercised in-process elsewhere. This is the part that only
# exists once there is a process: handshake ordering, one JSON object per line,
# and the rule that stdout carries the protocol and nothing else.
stdio_fail=0
if start_server; then
  cargo build -q --bin orbweaver-mcp-server --bin spike-dump 2>/dev/null
  mout=$(python3 spikes/mcp_session.py spikes/echo.ior spikes/echo.idl 2>&1)
  if grep -q "mcp session: PASS" <<<"$mout"; then
    # This grep IS the group's whole green output. Empty means the session
    # passed and the harness showed nothing for it, which reads as a group that
    # ran no checks.
    mcp_oks=$(grep "^  ok" <<<"$mout")
    if [ -n "$mcp_oks" ]; then
      sed -n '1,11p' <<<"$mcp_oks"
    else
      echo "  ok   mcp session: PASS, but it printed no '  ok' lines for this harness to"
      echo "       show — the verdict is the peer's, the detail is missing"
    fi
  else
    echo "  FAIL the stdio transport did not behave"
    diag "a FAIL line" "$mout" "$(grep "FAIL" <<<"$mout")" 4
    stdio_fail=1
  fi
else
  stdio_fail=1
fi
cleanup
[ "$stdio_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Search baseline: stream D ────────────────────────────────────────────────
hr "search baseline — frozen queries against the lexical index"
# §8's benchmark discipline: the query set is versioned and never edited to
# make a run pass. exact/negative/injection are gates; synonym is the measured
# headroom the embedding batch will be judged against, with no pass/fail here.
sb=$(cargo run -q -p orbweaver-mcp --bin search-bench -- \
     corpus/queries/search-v1.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
sb_rc=$?
if [ "$sb_rc" -eq 0 ] && grep -q "search-bench: PASS" <<<"$sb"; then
  printf '%s' "$sb" | grep "search-bench: PASS" | sed 's/^/  ok   /'
else
  echo "  FAIL the frozen search baseline did not hold"
  diag_out "$sb" 4
  fail_total=$((fail_total+1))
fi
# v2 widens the index (attributes, nested ai_desc, compound descriptions). v1
# stays frozen above so the two numbers keep meaning different things.
sb2=$(cargo run -q -p orbweaver-mcp --bin search-bench -- \
      corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
if [ $? -eq 0 ] && grep -q "search-bench: PASS" <<<"$sb2"; then
  printf '%s' "$sb2" | grep "search-bench: PASS" | sed 's/^/  ok   v2 /'
else
  echo "  FAIL the widened search set did not hold"
  diag_out "$sb2" 4
  fail_total=$((fail_total+1))
fi
# D003's arm: embeddings arrive through a process boundary or not at all. With
# no key the vector half is UNMEASURED — never green, and never faked with the
# offline stand-in, which is a plumbing check and cannot close a vocabulary gap.
if [ -n "${VOYAGE_API_KEY:-}" ]; then
  et=/tmp/orbweaver-texts.tsv; vf=/tmp/orbweaver-vectors.txt
  if cargo run -q -p orbweaver-mcp --bin search-bench -- --emit-texts "$et" \
       corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl >/dev/null 2>&1 \
     && vecs=$(cut -f2 "$et" | ./spikes/embed.sh 2>&1); then
    { echo "orbweaver-vectors 1"
      paste <(cut -f1 "$et") <(printf '%s\n' "$vecs" | sed 's/^\[//; s/\]$//')
    } > "$vf"
    sbv=$(cargo run -q -p orbweaver-mcp --bin search-bench -- --vectors "$vf" \
          corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
    if [ $? -eq 0 ]; then
      sbv_pass=$(grep "search-bench: PASS" <<<"$sbv")
      # Reached on exit 0 alone, so unlike the two gates above there is nothing
      # here that has already proved the line exists.
      echo "  ok   vector ${sbv_pass:-(the bench exited 0 and printed no PASS line)}"
    else
      echo "  FAIL vector search regressed a gate"
      diag_out "$sbv" 4
      fail_total=$((fail_total+1))
    fi
  else
    echo "  FAIL embed.sh failed with a key present — that is a broken wrapper, not an absence"
    fail_total=$((fail_total+1))
  fi
else
  skip absent git:spikes/embed.sh \
       "VOYAGE_API_KEY absent — the synonym class, and the injection class against a" \
       "real embedding model (I3), are unmeasured, not passing"
fi

# ── Wire hardening: stream E ─────────────────────────────────────────────────
hr "wire hardening — LocateRequest send, both peers, all three versions"
# Carried forward since Phase 2: the server side has answered locates, but
# nothing here had ever SENT one. Both answers are measured, because a locate
# that can only produce "here" has not been tested against anything.
loc_fail=0
if start_server; then
  lv=$(cargo run -q --bin spike-locate -- spikes/echo.ior 2>&1)
  if grep -q "locate: PASS" <<<"$lv"; then
    echo "  ok   omniORB: OBJECT_HERE for the real key, UNKNOWN for a corrupted one, GIOP 1.0/1.1/1.2"
  else
    echo "  FAIL locate against omniORB"
    diag "a FAIL line" "$lv" "$(grep FAIL <<<"$lv")" 3
    loc_fail=1
  fi
else
  loc_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jloc.log 2>&1 & )
  jl=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jl=1; break; }
    sleep 0.1
  done
  if [ "$jl" -eq 1 ]; then
    lv=$(cargo run -q --bin spike-locate -- spikes/jacorb.ior 2>&1)
    if grep -q "locate: PASS" <<<"$lv"; then
      echo "  ok   JacORB agrees on all six answers — a second, independent locate responder"
    else
      echo "  FAIL locate against JacORB"
      diag "a FAIL line" "$lv" "$(grep FAIL <<<"$lv")" 3
      loc_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; loc_fail=1
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi
[ "$loc_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Wire hardening: stream E — multi-profile failover ────────────────────────
hr "wire hardening — multi-profile failover, dead first profile"
# The reference names several places and the first one is dead; the call
# completes and the caller is never told which endpoint answered.
bears_on location
# Unit tests prove failover against listeners that accept but never speak
# GIOP. This closes the peer half: a synthetic IOR whose first profile is the
# real one with its port forced to 1 must still carry ping() -> 42, and an
# all-dead IOR must report how many endpoints were tried.
fo_fail=0
if start_server; then
  fv=$(cargo run -q --bin spike-failover -- spikes/echo.ior 2>&1)
  if grep -q "failover: PASS" <<<"$fv"; then
    echo "  ok   omniORB: a dead first profile does not cost the call; exhaustion counts endpoints"
  else
    echo "  FAIL failover against omniORB"
    diag "a FAIL line" "$fv" "$(grep FAIL <<<"$fv")" 3
    fo_fail=1
  fi
else
  fo_fail=1   # an unmeasured check is a failure, never a pass
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jfo.log 2>&1 & )
  jf=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jf=1; break; }
    sleep 0.1
  done
  if [ "$jf" -eq 1 ]; then
    fv=$(cargo run -q --bin spike-failover -- spikes/jacorb.ior 2>&1)
    if grep -q "failover: PASS" <<<"$fv"; then
      echo "  ok   JacORB: same behaviour from the second, independent peer"
    else
      echo "  FAIL failover against JacORB"
      diag "a FAIL line" "$fv" "$(grep FAIL <<<"$fv")" 3
      fo_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; fo_fail=1
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi
[ "$fo_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Wire hardening: stream E — CancelRequest is survivable ───────────────────
hr "wire hardening — CancelRequest against both peers"
# §9.4.4 is advisory. Measured: omniORB ignores a 1.2 cancel but CLOSES the
# connection on a 1.0/1.1 one — so the assertion is coherence, not tolerance:
# ignored, or refused with a clean client-side failure and a working fresh
# connection. Desynchronization is the only failure.
can_fail=0
if start_server; then
  cv=$(cargo run -q --bin spike-cancel -- spikes/echo.ior 2>&1)
  if grep -q "cancel: PASS" <<<"$cv"; then
    echo "  ok   omniORB: cancel ignored at 1.2, refused cleanly at 1.0/1.1, never desynchronized"
  else
    echo "  FAIL cancel against omniORB"
    diag "a FAIL line" "$cv" "$(grep FAIL <<<"$cv")" 3
    can_fail=1
  fi
else
  can_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jcan.log 2>&1 & )
  jc=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jc=1; break; }
    sleep 0.1
  done
  if [ "$jc" -eq 1 ]; then
    cv=$(cargo run -q --bin spike-cancel -- spikes/jacorb.ior 2>&1)
    if grep -q "cancel: PASS" <<<"$cv"; then
      echo "  ok   JacORB: coherent too — the second peer's cancel policy measured"
    else
      echo "  FAIL cancel against JacORB"
      diag "a FAIL line" "$cv" "$(grep FAIL <<<"$cv")" 3
      can_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; can_fail=1
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi
[ "$can_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── F6: the first-party CosNaming server ─────────────────────────────────────
hr "naming server — our client, then an independent ORB, against OUR server"
# Every naming claim so far ran our client against omniNames. This is the
# other direction: bind/resolve/unbind/nested contexts served by us, and the
# user-exception bytes confirmed by omniORB's client rather than only our own.
ns_fail=0
NS_IOR=/tmp/orbweaver-names.ior
ns=$(cargo run -q --bin spike-names -- "$NS_IOR" 2>&1)
if grep -q "naming-server: PASS" <<<"$ns"; then
  echo "  ok   our client against our server: bind/resolve/unbind/AlreadyBound/NotFound/nested"
else
  echo "  FAIL naming server self-consistency"
  diag "a FAIL line" "$ns" "$(grep FAIL <<<"$ns")" 3
  ns_fail=1
fi
rm -f "$NS_IOR" /tmp/orbweaver-names-hold.log
( exec cargo run -q --bin spike-names -- "$NS_IOR" --hold >/tmp/orbweaver-names-hold.log 2>&1 & )
ns_up=0
for _ in $(seq 1 60); do
  grep -qs HOLDING /tmp/orbweaver-names-hold.log && { ns_up=1; break; }
  sleep 0.2
done
if [ "$ns_up" -eq 1 ]; then
  oracle=$(python3 -c "import sys; from omniORB import CORBA; import CosNaming; orb = CORBA.ORB_init(sys.argv); nc = orb.string_to_object(open('$NS_IOR').read().strip())._narrow(CosNaming.NamingContextExt); print(orb.object_to_string(nc.resolve_str('spike/Echo')))" 2>&1)
  case "$oracle" in
    IOR:*) echo "  ok   omniORB's client resolved spike/Echo against OUR naming server" ;;
    *) echo "  FAIL cross-ORB resolve: $oracle"; ns_fail=1 ;;
  esac
else
  echo "  FAIL the holding naming server never came up"; ns_fail=1
fi
fkill "spike-names"
[ "$ns_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MoE control plane, one turn on the wire ─────────────────────────────
hr "expert service — registry, policy and residency through GIOP"
# NOT TAGGED `bears_on activation`, deliberately, and this is the finding of
# D031's first ledger. D031 §3 lists "the MoE residency spikes" as a group that
# already measures the activation transparency; read against what this group
# asserts, it does not. It drives residency FROM THE CONTROL PLANE — register,
# heartbeat, prefetch, guarded evict, policy — and a control plane is precisely
# the layer that is ALLOWED to know load state. D029 §6.1's activation row is
# about a caller holding only a reference getting the same answer whether the
# target is resident or evicted, and nothing here asks that question.
#
# Tagging it would have made the ledger print "activation: measured by 1 group,
# 0 red" over a row §6.1 calls "the transparency this project has the most
# machinery for and the least measurement of" — the ledger swallowing a leak,
# which is the one thing it exists not to do. It stays untagged until a group
# calls an evicted target and an equivalent resident one and asserts the caller
# cannot tell (D029 §5 O0).
# F1+F2+F3 joined: register and heartbeat over the wire, run the loading
# policy over the offers it produced, and drive the residency machine with the
# decisions. Measured because the interesting failures are between the parts —
# an offer store that lags the state machine returns an empty decision list
# under memory pressure and nothing fails.
ex=$(cargo run -q --bin spike-experts 2>&1)
if grep -q "expert-service: PASS" <<<"$ex"; then
  echo "  ok   register/heartbeat/oneway prefetch/guarded evict/policy, one control loop"
  # D010 A2 (2026-08-19): a router ordering by latency_p50 refuses to pick when
  # every candidate is unmeasured, and names the unmeasured one when some are.
  # Negative control: with "unknown sorts last" restored, the spike picked
  # expert-math with no measurement at all (agent measurement, in 06ea90e).
  if grep -q "only unmeasured candidates: the router refuses" <<<"$ex"; then
    echo "  ok   ORDER BY latency_p50 over unmeasured experts is refused, not ranked"
  else
    echo "  FAIL the router picked, or did not say it refused, over unmeasured experts"
    fail_total=$((fail_total+1))
  fi
else
  echo "  FAIL expert service"
  diag "a FAIL line" "$ex" "$(grep -i "FAIL" <<<"$ex")" 3
  fail_total=$((fail_total+1))
fi

hr "§5.3 — an approval is a record that replays"
# idl-diff --approve (93c7ea9) writes <proposed>.approvals.tsv (or --approvals):
# one row per blocking finding, bound to both units' SHA-256, with a required
# --approver; a later run reads it and passes covered findings as
# "[approved by …]"; an edited contract invalidates the row; a nameless row
# refuses the store whole (exit 2). approval_replay.rs holds the whole
# sequence, byte-identical apart from the timestamp (SOURCE_DATE_EPOCH pins
# it). Negative control (93c7ea9): the approver column blanked -> exit 2
# "a decision with no approver is not on record". No corpus file may carry an
# approvals store — a committed approval would be a decision nobody made.
ap_out=$(cargo test -q -p orbweaver-registry --test approval_replay 2>&1)
if grep -q "^test result: ok" <<<"$ap_out"; then
  echo "  ok   an approval replays byte-identically, invalidates on an edited byte, and refuses a nameless row"
else
  echo "  FAIL the approval store's replay property"
  diag "a panic" "$ap_out" "$(grep -A3 panicked <<<"$ap_out")" 6
  fail_total=$((fail_total+1))
fi
if ls corpus/evolution/*/*.approvals.tsv corpus/golden/*.approvals.tsv >/dev/null 2>&1; then
  echo "  FAIL a corpus contract carries a committed approvals store"; fail_total=$((fail_total+1))
else
  echo "  ok   no corpus contract carries a committed approval"
fi

hr "§5.3 — moe v1.1 is additive, and the in-place edit is still refused"
# corpus/evolution/moe/v1.0 is the frozen release; golden 22 the served revision;
# v1.1-in-place the negative control (both members added to the released
# struct). Captured then matched, never piped to grep -q. Negative control for
# the group itself: point the first diff at v1.1-in-place and it goes FAIL.
mv_fail=0
mv_out=$(cargo run -q --bin idl-diff -- corpus/evolution/moe/v1.0/moe.idl \
         corpus/golden/22-moe-control-plane.idl 2>&1); mv_rc=$?
if [ "$mv_rc" -eq 0 ] && grep -q "MeasuredCapability" <<<"$mv_out"; then
  echo "  ok   moe v1.0 -> golden 22 is additive (exit 0) and names MeasuredCapability"
else
  echo "  FAIL golden 22 is no longer an additive revision of moe v1.0 (exit $mv_rc)"
  diag_out "$mv_out" 3 head; mv_fail=1
fi
mv_ctl=$(cargo run -q --bin idl-diff -- corpus/evolution/moe/v1.0/moe.idl \
         corpus/evolution/moe/v1.1-in-place/moe.idl 2>&1); mv_ctl_rc=$?
if [ "$mv_ctl_rc" -eq 1 ] && grep -q "latency_p50_ms" <<<"$mv_ctl" \
   && grep -q "specialization" <<<"$mv_ctl"; then
  echo "  ok   the in-place edit is still refused with both members named (exit 1)"
else
  echo "  FAIL the negative control passed the gate (exit $mv_ctl_rc)"; mv_fail=1
fi
# corpus/evolution/union-default (a40317a): the same union with the default
# written first must be "no change"; a case inserted ahead of the default with
# the default retyped must name BOTH — the positional differ named one.
ud_out=$(cargo run -q --bin idl-diff -- corpus/evolution/union-default/v1.0/payload.idl \
         corpus/evolution/union-default/v1.0-default-first/payload.idl 2>&1); ud_rc=$?
if [ "$ud_rc" -eq 0 ] && grep -q "no change" <<<"$ud_out"; then
  echo "  ok   a union's default written first is the same release (exit 0)"
else
  echo "  FAIL member order of a union read as a change (exit $ud_rc)"; mv_fail=1
fi
ud_ctl=$(cargo run -q --bin idl-diff -- corpus/evolution/union-default/v1.0/payload.idl \
         corpus/evolution/union-default/v1.1-retyped-default/payload.idl 2>&1); ud_ctl_rc=$?
if [ "$ud_ctl_rc" -eq 1 ] && grep -q 'default member "text" changed type' <<<"$ud_ctl" \
   && grep -q 'union case(s) added: \["extra"\]' <<<"$ud_ctl"; then
  echo "  ok   the retyped default behind an inserted case is named, not only the case (exit 1)"
else
  echo "  FAIL the retyped default is not named (exit $ud_ctl_rc)"; mv_fail=1
fi
[ "$mv_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The guard's dry-run: a policy preview that costs nothing ────────────────
hr "audit ledger — a gate that sees a secret must not publish one"
# The content seat reads argument *values* (that is what it is for), and it can
# also refuse. Its refusal is free prose written by whatever stage a deployment
# installed, and the audit ledger is the one artifact this crate writes to disk,
# retains and greps. Rendering that prose put the payload there: measured, with
# a PIN in an argument reaching `why=` verbatim. Two checks, because the second
# is the one that catches the next call site.
# Since 2026-08-19 (D010 A3) the static path has the same arm — the guard
# reads a stub's own bytes back through the contract — and a dry run with
# values predicts marshalling from the contract's TypeCodes into a dropped
# buffer. Negative control: `arguments: view.as_ref().ok()` reverted to
# `arguments: None` in guard.rs -> the static arm panics "it saw: <none>" and
# the count drops to 5.
lk=$(cargo test -q -p orbweaver-mcp --lib -- \
     an_argument_a_content_stage_saw_cannot_reach_the_ledger \
     an_argument_a_content_stage_saw_on_the_static_path_cannot_reach_the_ledger \
     the_ledger_keeps_a_typed_reason_whole_and_a_stages_prose_not_at_all \
     a_dry_run_offers_a_content_stage_no_arguments_to_judge \
     a_string_of_eight_given_nine_characters_predicts_marshal_where_it_predicted_allow \
     a_static_dry_run_with_values_predicts_marshalling_and_touches_no_wire 2>&1)
n_lk=$(printf '%s' "$lk" | grep -o '^test result: ok. [0-9]*' | grep -o '[0-9]*$')
if [ "${n_lk:-0}" = "6" ]; then
  echo "  ok   a content stage reads the payload — dynamic and static — the ledger and the trace do not"
  echo "  ok   a dry run with values predicts MARSHAL from the TypeCodes and touches no wire"
else
  # A renamed or deleted test is unmeasured, which is a failure, never a pass.
  echo "  FAIL the content-seat leak property is failing or no longer measured"
  diag "a panic" "$lk" "$(grep -A3 panicked <<<"$lk")" 6
  fail_total=$((fail_total+1))
fi
# The rule is a type, not a grep: `audit_entry` takes an `AuditReason`, and a
# `Denied` cannot become one by `Display`. The grep this replaced caught its own
# explanatory comment and, measured, missed a real violation — green, and
# measuring nothing. What a compiler cannot see is a *third* constructor
# appearing, so that is what is counted here.
ctor_lines=$(grep -n 'AuditReason(std::borrow::Cow::' crates/orbweaver-mcp/src/guard.rs)
ctors=$(grep -c 'AuditReason(std::borrow::Cow::' crates/orbweaver-mcp/src/guard.rs)
if [ "$ctors" = "2" ]; then
  echo "  ok   AuditReason has exactly two constructors; a Denied reaches the ledger through one"
else
  echo "  FAIL AuditReason has $ctors constructor(s), not 2 — a new way into the ledger"
  # Zero is one of the two ways this goes red, and the old dump printed
  # NOTHING for it — a FAIL saying "0 constructor(s)" with a blank space where
  # the evidence goes.
  if [ -n "$ctor_lines" ]; then
    sed 's/^/       | /' <<<"$ctor_lines"
  else
    echo "       (guard.rs matches the constructor pattern nowhere at all, so this is a"
    echo "        rename or a removal rather than a third way into the ledger)"
  fi
  fail_total=$((fail_total+1))
fi

hr "dry-run — the exposure read before it is deployed"
# No --ior and no peer, which is the whole point: this answers before a
# deployment exists. Two properties, both cheap. The report is well-formed and
# carries the summary an operator reads; and every audit line it leaves says
# DRYRUN, so a question can never be counted as a call — a hypothetical in the
# promotion statistics would promote a path nobody ever used.
dr_audit=/tmp/orbweaver-dryrun.audit
dr=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 \
     --as harness --dry-run 2>"$dr_audit")
dr_rc=$?
allowed=$(printf '%s' "$dr" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["summary"]["allow"])' 2>/dev/null)
scoped=$(printf '%s' "$dr" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["summary"]["need_scope"])' 2>/dev/null)
# `grep -c` prints its count AND exits 1 when the count is zero, so a
# `|| echo 0` appends a second line and the comparison below sees "0\n0".
# Count with awk, which has one exit status and one answer.
stray=$(awk '!/^DRYRUN-/ {n++} END {print n+0}' "$dr_audit" 2>/dev/null)
if [ "$dr_rc" -eq 0 ] && [ "$allowed" = "10" ] && [ "$scoped" = "1" ] && [ "$stray" -eq 0 ]; then
  echo "  ok   11 operations previewed with no target dialled: 10 allow, 1 need_scope"
  echo "  ok   every audit line is a DRYRUN line — no question counted as a call"
else
  echo "  FAIL the dry-run preview did not hold (allow=$allowed need_scope=$scoped stray=$stray)"
  fail_total=$((fail_total+1))
fi
# A value-carrying dry run from the CLI (4bb9742): one operation, declared
# values, no target — the document says `marshal` for string<8> given nine.
# Negative control: drop --dry-run-args and the same command says `allow None`.
dv=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl corpus/golden/27-bounds.idl --expose IDL:gc27/Ledger:1.0 --assume-effect read_only \
     --as harness --dry-run=IDL:gc27/Ledger:1.0.keep \
     --dry-run-args '{"key":"123456789","entry":{"label":"ok","payload":"AQID","wide":"ab"}}' 2>/dev/null)
dv_would=$(printf '%s' "$dv" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["would"], d.get("payload"))' 2>/dev/null)
if [ "$dv_would" = "marshal would_not_marshal" ]; then
  echo "  ok   a value-carrying dry run from the CLI predicts MARSHAL for string<8> given nine (no target)"
else
  echo "  FAIL the CLI dry run with values did not predict marshal (got: ${dv_would:-nothing})"; fail_total=$((fail_total+1))
fi
# A held reference from the CLI (ea25fce): heartbeat(in Expert e, …) with
# --dry-run-handle naming an IOR that is parsed and never dialed (TEST-NET-1
# 192.0.2.77:31337, nothing answers there and nothing is asked to) predicts
# `allow marshals`. Negative control: drop --dry-run-handle and the same
# command says `marshal would_not_marshal` ("no reference is held under
# handle \"expert\"") — every answer the CLI could give before it.
dh_ior='IOR:010000001300000049444c3a6d6f652f4578706572743a312e30000001000000000000003c000000010102000b0000003139322e302e322e37370000697a00001b000000766572792d64697374696e63746976652d6f626a6563742d6b65790000000000'
dh_cap='{"id":"x","cost":1,"latency_p99_ms":1,"load":0.5,"state":"RESIDENT","mem_footprint":1,"route_freq":0,"placement_node":"n","contract_version":"1.0"}'
dh=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl corpus/golden/22-moe-control-plane.idl --expose IDL:moe/ExpertRegistry:1.0 \
     --as harness --scope moe.registry.write --dry-run=IDL:moe/ExpertRegistry:1.0.heartbeat \
     --dry-run-handle "expert=$dh_ior" \
     --dry-run-args '{"e":{"_ref":"expert"},"updated_cap":'"$dh_cap"'}' 2>/dev/null)
dh_would=$(printf '%s' "$dh" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["would"], d.get("payload"))' 2>/dev/null)
dn=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl corpus/golden/22-moe-control-plane.idl --expose IDL:moe/ExpertRegistry:1.0 \
     --as harness --scope moe.registry.write --dry-run=IDL:moe/ExpertRegistry:1.0.heartbeat \
     --dry-run-args '{"e":{"_ref":"expert"},"updated_cap":'"$dh_cap"'}' 2>/dev/null)
dn_would=$(printf '%s' "$dn" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["would"], d.get("payload"))' 2>/dev/null)
if [ "$dh_would" = "allow marshals" ] && [ "$dn_would" = "marshal would_not_marshal" ]; then
  echo "  ok   a --dry-run-handle reference resolves from the CLI (allow marshals); without it, marshal would_not_marshal (no target)"
else
  echo "  FAIL the CLI dry run with a held reference did not hold (with: ${dh_would:-nothing}; without: ${dn_would:-nothing})"; fail_total=$((fail_total+1))
fi

# ── Service coverage: every declared operation, over the wire ───────────────
hr "service coverage — what the five servants actually serve"
# Each COMPONENTS row says ✅ and each servant implements a subset, deliberately.
# The wire used to be unable to distinguish a considered BAD_OPERATION from a
# forgotten one, so this group only counted facts and the reasons lived in
# docs/SERVICES-COVERAGE.md. Since 2026-08-18 a decision answers NO_IMPLEMENT
# and BAD_OPERATION means only "this interface does not declare that name", so
# the sweep decides instead of counting: a BAD_OPERATION from an object that
# *claims* the interface is a servant half-serving something it says it is, and
# it fails here. An interface no object claims is reported as its own fact.
# The first version of that check asked `_is_a` with a repository id built from
# the scoped name, which is wrong for every COS interface (#pragma prefix), and
# it passed a deliberately broken servant. It now reads the claim out of the
# rows already measured.
cov=$(./spikes/service_sweep.sh --raw 2>&1)
if grep -q "service-sweep: PASS" <<<"$cov"; then
  cov_total=$(grep '^TOTAL' <<<"$cov")
  echo "  ok   ${cov_total:-(the sweep passed and printed no TOTAL row)}"
  # docs/SERVICES-COVERAGE.md §8 is generated from these same rows, so the
  # document is checked against the wire rather than transcribed from it —
  # the counts it used to carry by hand went stale in four days (D010 A5).
  # Negative control (2026-08-19): one served count edited in the block ->
  # "FAIL docs/SERVICES-COVERAGE.md §8 no longer says what the wire says"
  # with the one-line diff; regenerated -> ok.
  ct_out=$(printf '%s\n' "$cov" | python3 spikes/coverage_tables.py --check 2>&1); ct_rc=$?
  # coverage_tables.py --check decides this group, so silence from it is the
  # one thing that must not print as silence.
  [ -n "$ct_out" ] || ct_out="  (spikes/coverage_tables.py --check printed nothing at all)"
  sed -n '1,12p' <<<"$ct_out"
  [ "$ct_rc" -eq 0 ] || fail_total=$((fail_total+1))
else
  echo "  FAIL service coverage sweep"
  diag "a FAIL/ABSENT/UNMEASURED/BLOCKED row" "$cov" \
       "$(grep -E 'FAIL|ABSENT|UNMEASURED|BLOCKED' <<<"$cov")" 8
  fail_total=$((fail_total+1))
fi

# ── The audit ledger is bounded, and says where it was cut ──────────────────
hr "audit ledger — a survey over the ceiling must name what it dropped"
# Dropping the oldest silently is how an audit log stops being one: a dropped
# hour and a quiet hour read identically, exactly when somebody is reading the
# log to tell them apart. --dry-run needs no IOR, no socket and no handle, so
# this is deterministic and fixture-free; an absent marker is a FAILURE.
al_marker=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice \
  --dry-run --audit-capacity 3 2>&1 >/dev/null | grep -E '^ELIDED ' || true)
al_dropped=$(printf '%s' "$al_marker" | sed -n 's/.*dropped=\([0-9][0-9]*\).*/\1/p')
if [ -n "$al_dropped" ] && [ "$al_dropped" -ge 1 ]; then
  echo "  ok   a ceiling of 3 over 11 decisions elided $al_dropped line(s) and named it in the ledger"
else
  echo "  FAIL the ledger dropped lines without saying so"
  fail_total=$((fail_total+1))
fi

# ── The Python target: a second language is the only test of the mapping ────
# Anything the Rust emitter got right by accident shows up here as something
# Python cannot express, or expresses differently. The seam is a local process
# (D007, PROPOSED) so CPython gains no dependency and the wire stays in Rust.
hr "Python client target — generated Python against the omniORB fixture"
# The only group in this harness that puts a NON-RUST caller of ours on the
# wire: generated Python invokes a C++ target holding only a reference, with no
# Rust stub in the path. It is the weakest of this run's tags and it is worth
# saying why: it measures the CALLER's half of language transparency, and
# D029 §6.1's language row is about the TARGET's — "a Python servant cannot be
# dispatched into". The ledger prints that leak under this row every run, so a
# green count here cannot be read as the row being held.
bears_on language
if start_server; then
  pyout=/tmp/orbweaver-pytarget; rm -rf "$pyout"; mkdir -p "$pyout"
  if cargo run -q --bin gen-python -- --out "$pyout" spikes/echo.idl >/dev/null 2>&1 \
     && cargo build -q --bin orbweaver-py-bridge 2>/dev/null; then
    pyrun=$(python3 crates/orbweaver-gen/python/echo_client.py "$pyout" \
            spikes/echo.idl spikes/echo.ior ./target/debug/orbweaver-py-bridge 2>&1)
    case "$pyrun" in
      *"python target: PASS"*)
        echo "  ok   $(printf '%s' "$pyrun" | grep -c '^  ok') generated call(s) completed \
over the wire, no Rust stub involved" ;;
      *) echo "  FAIL the Python client did not complete its calls"
         diag_out "$pyrun" 12
         fail_total=$((fail_total+1)) ;;
    esac
  else
    echo "  FAIL gen-python or the bridge did not build"
    fail_total=$((fail_total+1))
  fi
  cleanup
else
  echo "  FAIL the omniORB fixture would not start — an unmeasured check is a failure"
  fail_total=$((fail_total+1))
fi

# Generated Python is imported, not string-compared: a target that only ever
# gets diffed is a target nobody has run.
pybatch=/tmp/orbweaver-pybatch; rm -rf "$pybatch"; mkdir -p "$pybatch"
golden=$(ls corpus/golden/*.idl | wc -l | tr -d ' ')
if cargo run -q --bin gen-python -- --out "$pybatch" corpus/golden/*.idl >/dev/null 2>&1; then
  imported=$(cd "$pybatch" && python3 -c '
import importlib, pathlib, sys
sys.path.insert(0, ".")
ok = 0
for d in sorted(p.name for p in pathlib.Path(".").iterdir() if p.is_dir()):
    importlib.import_module(d); ok += 1
print(ok)' 2>/dev/null)
  if [ "${imported:-0}" -ge "$golden" ]; then
    echo "  ok   $imported generated Python package(s) imported, one per golden contract"
  else
    echo "  FAIL only ${imported:-0} of $golden golden contracts produced an importable package"
    fail_total=$((fail_total+1))
  fi
else
  echo "  FAIL gen-python refused the golden corpus"
  fail_total=$((fail_total+1))
fi
# The cross-implementation sweep must keep measuring what it measured. These
# are **floors, not figures**: 170 / 137 over golden (158/132 before golden
# 29's labelled defaults, 2026-08-19; 182/139 as the corpus stands
# 2026-08-24), 70 / 46 over services, 0 divergences (D010 A4 —
# constructed anys and forward-declared references included). A drop is the oracle quietly measuring less, which is how a green
# line stops meaning anything. Negative control: the pre-A4 `_rt.py` reads
# "85 divergence(s)" here (the D008 refusal on every structural `_t`).
py_sweep=$(cargo test -q -p orbweaver-gen --test python_target -- --nocapture 2>&1)
# These two feed the floors below, so they are the early-exit-on-a-pipe class
# that can change a verdict: `sed … | head -1` SIGPIPEs sed and, under
# `pipefail`, gives the substitution sed's status instead of head's. Herestring
# in, herestring out — no pipe anywhere, so nothing can exit early on one.
gl_all=$(sed -n 's/^.*corpus\/golden: .* \([0-9][0-9]*\) value(s) and \([0-9][0-9]*\) call(s) .* \([0-9][0-9]*\) divergence(s)$/\1 \2 \3/p' <<<"$py_sweep")
sv_all=$(sed -n 's/^.*corpus\/services: .* \([0-9][0-9]*\) value(s) and \([0-9][0-9]*\) call(s) .* \([0-9][0-9]*\) divergence(s)$/\1 \2 \3/p' <<<"$py_sweep")
gl=$(head -1 <<<"$gl_all")
sv=$(head -1 <<<"$sv_all")
set -- $gl
if [ "${1:-0}" -ge 170 ] && [ "${2:-0}" -ge 137 ] && [ "${3:-1}" -eq 0 ]; then
  echo "  ok   $1 golden value(s) over $2 call(s) crossed to Python and back, constructed anys included, 0 divergences"
else
  echo "  FAIL python round-trip sweep over golden: ${gl:-did not print its measurement}"; fail_total=$((fail_total+1))
fi
set -- $sv
if [ "${1:-0}" -ge 70 ] && [ "${2:-0}" -ge 46 ] && [ "${3:-1}" -eq 0 ]; then
  echo "  ok   $1 service value(s) over $2 call(s), 0 divergences"
else
  echo "  FAIL python round-trip sweep over services: ${sv:-did not print its measurement}"; fail_total=$((fail_total+1))
fi

# ── corpus/include: the first multi-file cases the corpus has ever had ──────
# Every other corpus file is self-contained, which is exactly why `#include`
# was skipped rather than resolved for six phases and nothing went red. The
# manifest drives the gate, so a case is added by adding a row.
hr "corpus/include — resolution, prefix scope across a file boundary, guards, cycles"
inc=$(cargo test -q -p orbweaver-idl --test include_corpus 2>&1)
if grep -q "^test result: ok" <<<"$inc"; then
  inc_n=$(sed -n '1p' <<<"$(grep -oE '[0-9]+ passed' <<<"$inc")")
  echo "  ok   ${inc_n:-(the suite printed no pass count)} over \
$(awk 'NF && $1 !~ /^#/' corpus/include/cases.tsv | wc -l | tr -d ' ') manifest case(s)"
else
  echo "  FAIL corpus/include"
  diag "a panic" "$inc" "$(grep -A3 panicked <<<"$inc")" 8
  fail_total=$((fail_total+1))
fi

# ── The estate: thirteen legacy contracts through the whole path ────────────
# Consumer-shaped, not a gate — nothing under spikes/estate/ is any stage's
# input, which is what lets it measure the path instead of participating in
# it. Every corpus file is self-contained, so this is the only place a
# multi-file estate, four prefix styles and an unannotated contract are seen
# at once. Takes no lock of its own: private mktemp dir, fixture by PID.
hr "legacy estate — thirteen contracts, one pass, ingestion to agent call"
if [ -x spikes/estate/run.sh ]; then
  if est=$(./spikes/estate/run.sh --tsv 2>&1); then
    printf '%s\n' "$est" | sed 's/^/  /'
    echo "  ok   estate: every stage measured"
  else
    diag_out "$est" 20
    echo "  FAIL estate: see docs/pipeline-runs/2026-08-14-estate.md"
    fail_total=$((fail_total+1))
  fi
else
  echo "  FAIL spikes/estate/run.sh missing — an unmeasured path is a failure"
  fail_total=$((fail_total+1))
fi

# ── D005 option B: a regeneration is diffed against what is registered ──────
hr "registered-contract diff — an undeclared breaking change is refused"
# The half option C cannot cover, and vice versa: the differ reads no
# annotations, so a scope change is compatible by §5.3 and invisible here,
# while a rename that keeps every scope is invisible to C. Neither subsumes
# the other, which is why both landed.
rd=$(cargo test -q -p orbweaver-forge --test registered_diff 2>&1)
if grep -q "^test result: ok" <<<"$rd"; then
  rd_n=$(sed -n '1p' <<<"$(grep -oE '[0-9]+ passed' <<<"$rd")")
  echo "  ok   ${rd_n:-(the suite printed no pass count)} — refuses a breaking
       regeneration, silent on an additive one, and silent when nothing is registered"
else
  echo "  FAIL registered-contract diff"
  diag "a panic" "$rd" "$(grep -A3 panicked <<<"$rd")" 6
  fail_total=$((fail_total+1))
fi

# ── Scope drift is loud before a call (stream C, D005's class) ──────────────
hr "scope drift — a permission name no token can satisfy, reported as an outage"
# The failure D005 measured is silent by construction: an identity provider
# issuing the requirement's literal scope against a contract asking for another
# refuses every legitimate caller, and it reads as a permissions
# misconfiguration rather than a generation defect. So what is checked here is
# that the process refuses to be quiet about it — and, just as important, that
# a deployment which does not configure a mapping cannot tell the feature
# exists.
sd_fail=0
SD=/tmp/orbweaver-scope-drift
rm -rf "$SD" && mkdir -p "$SD"
cat > "$SD/parkinglot.idl" <<'IDL'
module parkinglot {
  interface ParkingControl {
    //@ ai_desc: Raises the entry barrier
    //@ ai_authz: parkinglot.barrier.open
    void open_barrier();
  };
};
IDL
sd_out=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
  --as alice --map-scope 'gate:operate=gate:operate' \
  --token-scope 'gate:operate' --dry-run 2>"$SD/err")
sd_code=$?
sd_err=$(cat "$SD/err" 2>/dev/null)
if [ "$sd_code" -eq 3 ] && grep -q "open_barrier" <<<"$sd_err"; then
  echo "  ok   a scope no issued token can satisfy exits 3 and names the operation that goes dark"
else
  echo "  FAIL a drifted scope was not reported as an outage (exit $sd_code)"
  diag_out "$sd_err" 3 head
  sd_fail=1
fi
if cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
     --as alice --map-scope 'gate:operate=parkinglot.barrier.open' \
     --token-scope 'gate:operate' --dry-run >/dev/null 2>&1; then
  echo "  ok   one line of translation repairs it, with the contract untouched"
else
  echo "  FAIL the mapping did not repair the drift"; sd_fail=1
fi
sd_plain=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
  --as alice --dry-run 2>/dev/null)
case "$sd_plain" in
  *scope_map*) echo "  FAIL an unconfigured deployment can tell the feature exists"; sd_fail=1 ;;
  *) echo "  ok   with no mapping configured, the report is the document it always was" ;;
esac
rm -rf "$SD"
[ "$sd_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── R7: an IOR that is dialable from where the client actually is ───────────
hr "NAT rewriting — the address a container publishes is not the one it bound"
# D029 §6.1's location cell names this one by name — "R7 rewrites an IOR for a
# dialable address". The bound address and the published address differ and the
# caller dials the one it was given.
bears_on location
# assumption D already measured that a server publishes a routable-but-local
# address. Inside a container that address is the namespace's, and a client
# outside cannot dial it. The spike constructs both real failures — refused and
# timed out — because loopback alone cannot show this one.
nat=$(./spikes/nat_rewrite.sh 2>&1)
if grep -q "nat rewriting: PASS" <<<"$nat"; then
  echo "  ok   unrewritten IOR fails to dial, rewritten one completes; key, version and"
  echo "       an undecodable profile all survive untouched"
  if grep -q "unmeasured (skipped): [1-9]" <<<"$nat"; then
    skip absent git:spikes/nat/Dockerfile \
         "the container probe has never run here — no docker, and it is" \
         "counted rather than read as evidence"
  fi
  # The second-host probe (spikes/nat/vm/run.sh) HAS executed — five passes on
  # 2026-08-14 across a multipass VM, transcript in docs/PHASE6.md — but it
  # launches and deletes a VM, so it is not run here; ORBWEAVER_NAT_VM=1 runs
  # it. Named as a SKIPPED with its fixture, so the R7 row can point at a line
  # instead of at "unmeasured" (the plan review of 2026-08-19 found R7 saying
  # a real routing domain was unmeasured when it had been).
  if [ "${ORBWEAVER_NAT_VM:-0}" = "1" ] && command -v multipass >/dev/null 2>&1; then
    natvm=$(./spikes/nat/vm/run.sh 2>&1)
    if grep -q "PASS" <<<"$natvm"; then
      echo "  ok   a real second host: the naive IOR is refused, the rewritten one answers (multipass VM)"
    else
      echo "  FAIL the second-host probe did not hold"
      diag_out "$natvm" 3
      fail_total=$((fail_total+1))
    fi
  else
    # `last measured 2026-08-14` used to be typed here. It was true when it was
    # written and nothing recomputes it, which is the defect CLAUDE.md calls a
    # floor quoted as a figure. The date now comes from the tree. Its stated
    # limit: it is the day the probe last LANDED, which is the day PHASE6
    # records it running — edit run.sh without re-running it and the date moves
    # and overstates freshness. A limit that is written down beats a literal
    # that can only ever understate.
    skip absent git:spikes/nat/vm/run.sh \
         "the second-host probe (multipass VM, spikes/nat/vm/run.sh) is not run here —" \
         "ORBWEAVER_NAT_VM=1 with multipass installed runs it; the transcript of the" \
         "run it stands on is in docs/PHASE6.md"
  fi
else
  echo "  FAIL NAT rewriting"
  diag "a FAIL line" "$nat" "$(grep -i "FAIL" <<<"$nat")" 3
  fail_total=$((fail_total+1))
fi

# ── The whole path, end to end ──────────────────────────────────────────────
hr "end-to-end — requirement → contract → both halves → guarded call"
# Every part is measured somewhere above; this is the only check that runs them
# as one path, which is the claim the project actually makes. S1–S3 are
# replayed from a recorded live run because a committed servant names one
# contract's identifiers — see the run record, and see what re-running the
# model on the same requirement produced.
if [ -x "$ROOT/spikes/end_to_end.sh" ]; then
  e2e=$("$ROOT/spikes/end_to_end.sh" 2>&1)
  case "$e2e" in
    *"end-to-end: PASS"*)
      echo "  ok   8 hops, each measured: $(printf '%s' "$e2e" | grep -c 'PASS ') checks"
      printf '%s' "$e2e" | grep -E '^  \| (hand-written, product|generated by)' \
        | sed 's/^  | /  ok   /'
      # The model stages ran from a dated recording with provenance — a
      # measurement, on the date the log names — but not in this run. The
      # embeddings arm skips when its model is absent; this arm did not, and
      # the plan review of 2026-08-19 found the two rules applied unevenly.
      # Counted SKIPPED, naming the fixture, until E2E_MODEL=1 runs it live.
      if [ "${E2E_MODEL:-0}" != "1" ]; then
        e2e_date=$(awk '$1=="date"{print $2; exit}' "$ROOT/spikes/e2e/recorded/pipeline.log" 2>/dev/null)
        # `replay`, not `absent`: this run DID make the S1–S3 claim, out of a
        # recording of another day. The date is the recording's own stamp, read
        # out of it a line above — the only skip in this file whose date is a
        # measurement rather than a proxy for one.
        skip replay "@${e2e_date}" \
             "S1–S3 not run live here — replayed from a recording; the fixture is" \
             "E2E_MODEL=1 with a producer command and its key (PLAN §8 AI quality, per release)"
      fi ;;
    *) echo "  FAIL the end-to-end path did not hold"
       diag "a FAIL line" "$e2e" "$(grep FAIL <<<"$e2e")" 4
       fail_total=$((fail_total+1)) ;;
  esac
else
  echo "  FAIL spikes/end_to_end.sh missing — an unmeasured path is a failure"
  fail_total=$((fail_total+1))
fi

# ── Repository ids agree with omniidl ───────────────────────────────────────
hr "repository ids — identity, checked against the compiler that owns it"
# `#pragma prefix` makes an id un-derivable by inspection: an id does not say
# how many leading segments are prefix. So the only honest check is to run both
# compilers over the same files and diff, and this group exists because we
# spent months deriving ids from the scope path alone while every legacy IDL
# file carries a prefix — correct locally, wrong against every real peer.
rid_fail=0
rid_work=$(mktemp -d)
cargo run -q --bin repository-ids -- corpus/pragma/*.idl 2>/dev/null \
  | cut -f1,3 | sort > "$rid_work/ours"
for f in corpus/pragma/*.idl; do
  base=$(basename "$f"); out="$rid_work/$base.d"; mkdir -p "$out"
  if ! log=$(omniidl -bpython -C"$out" "$f" 2>&1); then
    echo "  FAIL omniidl rejected $base: ${log%%$'\n'*}"
    [ -n "$log" ] || echo "       (omniidl exited non-zero and printed nothing at all)"
    rid_fail=1; continue
  fi
  grep -rhoE '"IDL:[^"]*"' "$out" 2>/dev/null | tr -d '"' | grep -v '^IDL:omg.org/' \
    | sort -u | sed "s|^|$base	|"
done | sort > "$rid_work/oracle"
if diff -u "$rid_work/oracle" "$rid_work/ours" > "$rid_work/diff" 2>&1; then
  echo "  ok   $(wc -l < "$rid_work/ours" | tr -d ' ') repository id(s) match omniidl, prefixes and all"
else
  echo "  FAIL our repository ids differ from omniidl:"
  head -12 "$rid_work/diff" | sed 's/^/       /'
  rid_fail=1
fi
rm -rf "$rid_work"
[ "$rid_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── D004 + console: the emitter into the reader ─────────────────────────────
hr "observability — real span records through the real console"
# The two halves were built in separate batches against the record table in
# the approved decision, never against each other. That is what fixing the
# shape in the decision buys, and it is worth nothing unless somebody runs one
# into the other, so the harness does — and it does it with no target dialled,
# because --dry-run asks the real chain and reaches no peer.
obs_fail=0
OBS_JSONL=/tmp/orbweaver-obs.jsonl
OBS_HTML=/tmp/orbweaver-obs.html
rm -f "$OBS_JSONL" "$OBS_HTML" "$OBS_JSONL.2"
cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice --session s-harness \
  --trace "$OBS_JSONL" --trace-ts 2026-08-14T09:00:00Z --dry-run >/dev/null 2>&1
if [ ! -s "$OBS_JSONL" ]; then
  echo "  FAIL no span records were emitted"; obs_fail=1
else
  # Replay: same calls, same bytes. The record carries no clock, and this is
  # what makes that claim checkable rather than merely documented.
  cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
    --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice --session s-harness \
    --trace "$OBS_JSONL.2" --trace-ts 2026-08-14T09:00:00Z --dry-run >/dev/null 2>&1
  if cmp -s "$OBS_JSONL" "$OBS_JSONL.2"; then
    echo "  ok   $(wc -l < "$OBS_JSONL" | tr -d ' ') span records, and the trace replays byte-identically"
  else
    echo "  FAIL the trace did not replay byte-identically"; obs_fail=1
  fi
  obs=$(cargo run -q -p orbweaver-console --bin orbweaver-console -- \
        traces "$OBS_JSONL" 2>&1)
  case "$obs" in
    *"0 unclassified, 0 unreadable lines"*)
      echo "  ok   the console read every record the emitter wrote: $(printf '%s' "$obs" | sed -n 2p | sed 's/^ *//')" ;;
    *) echo "  FAIL the console could not read the emitter's records"
       diag_out "$obs" 3 head; obs_fail=1 ;;
  esac
  # The operator's page must not attack the operator: a repository id is chosen
  # by whoever we ingested it from.
  printf '%s\n' '{"ts":"2026-01-01T00:00:00Z","session":"s","caller":"x","target":"IDL:evil/<script>alert(1)</script>:1.0","operation":"go","decision":"dryrun-refuse","stage":"authz.exposure","path":"dynamic","outcome":"-"}' >> "$OBS_JSONL"
  cargo run -q -p orbweaver-console --bin orbweaver-console -- \
    traces "$OBS_JSONL" --html "$OBS_HTML" >/dev/null 2>&1
  page=$(cat "$OBS_HTML" 2>/dev/null)
  case "$page" in
    *"<script"*|*"<img"*) echo "  FAIL markup from a trace field rendered as markup"; obs_fail=1 ;;
    *"&lt;script&gt;"*)    echo "  ok   a hostile repository id renders inert, and is not dropped" ;;
    *) echo "  FAIL the payload was dropped rather than escaped"; obs_fail=1 ;;
  esac
fi
[ "$obs_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Stream E: multiplexing and pooling, against both peers ──────────────────
hr "multiplexing — several requests in flight, replies correlated by id"
# Out-of-order replies are the peer's to volunteer, so they are reported and
# never gated; what is gated is that the run completed. The self-test needs no
# fixture and says so — it scores no out-of-order claim, because our own server
# reads one request per connection.
mx_fail=0
mx=$(cargo run -q --bin spike-mux 2>&1)
if grep -q "mux: PASS" <<<"$mx"; then
  echo "  ok   self-test: pipelining, tombstones, and a refusal below GIOP 1.2"
else
  echo "  FAIL mux self-test"
  diag "a fail line" "$mx" "$(grep -i fail <<<"$mx")" 3
  mx_fail=1
fi
if start_server; then
  mxp=$(cargo run -q --bin spike-mux -- spikes/echo.ior 12 1.2 2>&1)
  if grep -q "mux: PASS" <<<"$mxp"; then
    mx_ooo=$(sed -n '1p' <<<"$(grep -o 'out-of-order [0-9]*' <<<"$mxp")")
    echo "  ok   omniORB at 1.2: ${mx_ooo:+replies }${mx_ooo:-(the spike reported no out-of-order count)}"
    # Legitimately empty — a peer that volunteered no fragments and left nothing
    # unmeasured has nothing to say here — so this one is silent on purpose.
    sed -n '1,2p' <<<"$(grep -E 'FRAGMENTS|UNMEASURED' <<<"$mxp")" | sed 's/^/       /'
  else
    echo "  FAIL multiplexing against omniORB"; mx_fail=1
  fi
else
  mx_fail=1
fi
cleanup
[ "$mx_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Stream E batch 2: concurrent dispatch, run more than once ───────────────
hr "concurrent dispatch — five runs, because one green run is not evidence"
# Every test in these crates is deadline-bounded, so a regression is a failed
# run and never a hung harness. The count is the check: a concurrency change
# that passes once has not been measured.
cd_runs=${ORBWEAVER_CONCURRENCY_RUNS:-5}
cd_failures=0
for cd_run in $(seq 1 "$cd_runs"); do
  # No RUSTFLAGS here on purpose. Setting it changes cargo's fingerprint, so
  # every later group in this file rebuilds the whole graph from scratch — the
  # first version of this group did exactly that and pushed the event-channel
  # fixture past its 12s readiness deadline, which looked like a wire failure
  # and was a build-cache one. The lint gate is CI's job; this group's job is
  # the repeat count.
  cd_out=$(cargo test -q -p orbweaver-giop -p orbweaver-registry -p orbweaver-object 2>&1)
  cd_rc=$?
  if [ "$cd_rc" -ne 0 ] || grep -q "^test result: FAILED" <<<"$cd_out"; then
    echo "  FAIL run $cd_run of $cd_runs (cargo exit $cd_rc)"
    # What this group owes when it goes red is a *diagnosis*, and until
    # 2026-08-25 it could not give one. It printed
    #   printf '%s' "$cd_out" | grep -A3 "^failures:" | head -6
    # and cargo emits two `failures:` sections — a detail block holding the
    # panic and, later, a bare list of names. Three lines after the first
    # match is the blank line and the `---- name stdout ----` header; the
    # panic is further down, and `head -6` cut before reaching it. A CI run
    # therefore reported `FAIL run 5 of 5` followed by six lines containing no
    # information, which is what it did on the first push after this group's
    # own five-run argument was written down. **A group whose case is "one
    # green run is not evidence" produced a red run that was not evidence
    # either.** Name the tests, then print each one's panic line.
    # No pipe: `head`/`grep -q` on a pipe are the two forms this file has been
    # bitten by, so the capture is a variable and the trim is a herestring.
    cd_names=$(grep -E "^test .* \.\.\. FAILED$" <<<"$cd_out" || true)
    cd_why=$(grep -E "panicked at|^assertion .* failed|^  left:|^ right:" <<<"$cd_out" || true)
    [ -n "$cd_names" ] && sed 's/^/       /' <<<"$cd_names"
    [ -n "$cd_why" ] && sed -n '1,20p' <<<"$cd_why" | sed 's/^/       | /'
    if [ -z "$cd_names$cd_why" ]; then
      # Neither shape matched: say so rather than printing nothing, which is
      # how this group spent its first red run saying nothing at all.
      echo "       (no FAILED test line and no panic in the output — last 8 lines:)"
      tail -8 <<<"$cd_out" | sed 's/^/       | /'
    fi
    cd_failures=$((cd_failures+1))
  fi
done
if [ "$cd_failures" -eq 0 ]; then
  echo "  ok   $cd_runs runs of the three servant crates, all green"
  echo "  ok   the negative control is a test: serialized dispatch must NOT overlap,"
  echo "       and it fails on its deadline rather than hanging when it does"
else
  fail_total=$((fail_total+1))
fi

# ── Stream E: concurrent connections ─────────────────────────────────────────
hr "concurrency — many clients at once, and a cap that says no out loud"
# Every service above documented "one client at a time" as a limit its harness
# group had to respect. The overlap is asserted against the server's own
# counter rather than against timing, because a timing-based overlap check
# passes on a fast serial server and is therefore not a check.
cc_fail=0
cy=$(cargo run -q --bin spike-concurrent 2>&1)
if grep -q "concurrency: PASS" <<<"$cy"; then
  echo "  ok   $(printf '%s' "$cy" | grep 'measured overlap' | sed 's/^ *//')"
  echo "  ok   $(printf '%s' "$cy" | grep 'cap behaviour' | sed 's/^ *//') — over the cap gets §9.4.7's goodbye"
else
  echo "  FAIL concurrent serving"
  diag "a FAIL line" "$cy" "$(grep -i "FAIL" <<<"$cy")" 3
  cc_fail=1
fi
[ "$cc_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── F5: tenancy, as an authorization property of the object key ─────────────
hr "tenant service — LifeCycle and Property with the tenant in every key"
# The isolation claims are the substance here, so the spike checks refusals as
# hard as it checks successes: a foreign reference is refused BEFORE the
# existence check, so a refusal cannot be used as an existence oracle. The one
# crossing that is served — base(), the shared model — is counted and audited
# rather than hidden, because the manifest's whole shape is "shared base by
# reference" and a servant that pretended otherwise would be lying about the
# design rather than enforcing it.
tn=$(cargo run -q --bin spike-tenants 2>&1)
if grep -q "tenant-service: PASS" <<<"$tn"; then
  echo "  ok   two tenants, $(printf '%s' "$tn" | grep -c '  ok    ') checks: minting, refusals, retire, policy, per-tenant audit"
else
  echo "  FAIL tenant service"
  diag "a FAIL line" "$tn" "$(grep -i "FAIL" <<<"$tn")" 3
  fail_total=$((fail_total+1))
fi

# ── IFR facade: the registry served as CORBA::Repository ────────────────────
hr "interface repository — our registry read by omniORB's own IR client"
# The claim worth measuring is not that our client agrees with our server; it
# is that a client written against the OMG IR IDL, which we did not write and
# cannot influence, decodes our FullInterfaceDescription and reads the
# enumerators by name. Ordinals that are merely self-consistent would pass a
# self-test and fail here.
ifr_fail=0
IFR_IOR=/tmp/orbweaver-ifr.ior
ifr=$(cargo run -q --bin spike-ifr -- "$IFR_IOR" 2>&1)
if grep -q "ifr-facade: PASS" <<<"$ifr"; then
  echo "  ok   our client against our facade: lookup_id, describe_interface, is_a, refusals"
else
  echo "  FAIL IFR facade self-consistency"
  diag "a FAIL line" "$ifr" "$(grep FAIL <<<"$ifr")" 3
  ifr_fail=1
fi
rm -f "$IFR_IOR" /tmp/orbweaver-ifr-hold.log
# `spikes/dkprobe.idl` rides along on the SAME holding facade rather than
# starting a second one: it adds one definition per DefinitionKind the facade
# can answer, including the three the v1 wire cannot carry, for the def_kind
# group below. Adding it does not move the walk group's expectations — every
# `expect` there is about gc10, and the walk was re-run against this three-file
# facade before the file was added (51 legs, unchanged).
cargo run -q --bin spike-ifr -- "$IFR_IOR" \
  corpus/golden/10-inheritance.idl corpus/golden/19-realistic-service.idl \
  spikes/dkprobe.idl --hold >/tmp/orbweaver-ifr-hold.log 2>&1 &
IFR_PID=$!
ifr_up=0
for _ in $(seq 1 60); do
  grep -qs READY /tmp/orbweaver-ifr-hold.log && { ifr_up=1; break; }
  sleep 0.2
done
if [ "$ifr_up" -eq 1 ]; then
  # Whether omniORBpy's IR stubs are importable is ONE fact, and all three legs
  # below need it. It is decided once, here, by a probe whose verdict is its
  # exit code — never by matching `ImportError` in a leg's output, which is the
  # class D010 §7.2 names: a traceback echoes the source line it failed on, so
  # a gate that greps for its own probe text can match itself. This used to be
  # spelled two ways in this section (a `case` arm here and an exit-code probe
  # before the walk); two spellings of one fact drift, so there is one.
  if python3 -c "import CORBA, omniORB.ir_idl" >/dev/null 2>&1; then
    ir_stubs=1
  else
    ir_stubs=0
  fi

  if [ "$ir_stubs" -eq 0 ]; then
    skip absent "" \
         "omniORBpy IR stubs absent (fixture: omniORBpy's omniORB.ir_idl) — the" \
         "cross-ORB half is unmeasured, not passing"
  else
    # The snippet's verdict is its exit code too, not a line bash matches:
    # every expectation is asserted inside python, so a mismatch is exit 1 and
    # an unexpected exception is exit 1 as well, and neither can be spelled by
    # accident in a traceback.
    ifr_out=$(python3 -c "import sys, CORBA, omniORB.ir_idl
orb = CORBA.ORB_init(sys.argv)
r = orb.string_to_object(open('$IFR_IOR').read().strip())._narrow(CORBA.Repository)
if r is None:
    print('the IOR did not narrow to CORBA::Repository')
    raise SystemExit(1)
d = r.lookup_id('IDL:gc10/Both:1.0')._narrow(CORBA.InterfaceDef).describe_interface()
got = (d.name, [o.name for o in d.operations], [a.name for a in d.attributes])
want = ('Both', ['touch', 'value'], ['id', 'name'])
if got != want:
    print(f'described {got!r}, want {want!r}')
    raise SystemExit(1)
try:
    r.create_module('IDL:x:1.0', 'x', '1.0')
    print('a write was ACCEPTED; the facade is meant to be read-only')
    raise SystemExit(1)
except CORBA.NO_PERMISSION:
    pass" 2>&1); ifr_rc=$?
    if [ "$ifr_rc" -eq 0 ]; then
      echo "  ok   omniORB's IR client decoded our FullInterfaceDescription and was refused a write"
    else
      echo "  FAIL cross-ORB IR client (exit $ifr_rc)"
      diag_out "$ifr_out" 6
      ifr_fail=1
    fi
  fi

  # And the containment walk the one-shot above does not reach: `contents`
  # with its filter, `describe_contents` and `max_returned_objs`, `lookup` vs
  # `lookup_name` with its levels, `defined_in` walked back up to the
  # repository, `get_primitive`/`get_canonical_typecode`, and the write
  # refusal — every leg driven by omniORB's IR client against the SAME holding
  # facade, so no second fixture is started and nothing is killed by pattern.
  #
  # The verdict is the script's exit code (0 every leg answered, 1 a leg
  # raised or answered wrong, 2 the narrow failed), never a marker grepped out
  # of a stream that prints its own expectations — that stream contains the
  # word FAILURES and the words it compares against.
  #
  # Absent stubs are told from a failed walk by `$ir_stubs` above — one probe,
  # one exit code, for all three legs of this group.
  if [ "$ir_stubs" -eq 0 ]; then
    skip absent git:spikes/ifr_walk_peer.py \
         "omniORBpy IR stubs absent (fixture: omniORBpy's omniORB.ir_idl) — the" \
         "containment walk is unmeasured, not passing"
  else
    walk_out=$(python3 spikes/ifr_walk_peer.py "$IFR_IOR" 2>&1); walk_rc=$?
    # 51 is a FLOOR on the legs the script counted, not today's figure: exit 0
    # over no legs at all is the green-while-measuring-nothing shape, and a
    # script whose body stopped running still falls off the end with 0.
    walk_legs=$(sed -n 's/^walk: every leg answered (\([0-9][0-9]*\) legs)$/\1/p' <<<"$walk_out")
    if [ "$walk_rc" -ne 0 ]; then
      echo "  FAIL omniORB's IR client could not walk our repository (exit $walk_rc)"
      diag_out "$walk_out" 10
      ifr_fail=1
    elif [ -z "$walk_legs" ] || [ "$walk_legs" -lt 51 ]; then
      echo "  FAIL the walk exited 0 over ${walk_legs:-no} legs (floor 51) — it measured less than it has"
      ifr_fail=1
    else
      echo "  ok   omniORB's IR client walked the repository, $walk_legs legs: contents and its filter,"
      echo "       describe_contents with max_returned_objs, lookup/lookup_name, defined_in back up"
      echo "       to the repository, get_primitive, and create_module still refused"
    fi
  fi

  # And the one thing neither leg above can refute: the ORDINAL our servant
  # writes for `_get_def_kind`. Every local comparison uses the same enum on
  # both sides and therefore agrees with itself, so a wrong ordinal is
  # invisible to a self-test — and was: before 2026-08-25 a valuetype, an
  # abstract interface and a native all answered `dk_none`, *"no such
  # definition"*, for definitions the registry holds, and nothing was red.
  # `spikes/dkprobe.idl` on the same holding facade is what makes those three
  # reachable at all.
  #
  # Same discipline as the walk: the verdict is the script's exit code, never a
  # marker grepped out of a stream that prints the enumerator names it is
  # comparing against — that stream contains every string a marker match could
  # want. And exit 3 is kept apart from exit 1 the way `spikes/ssliop.sh` keeps
  # them apart: 3 means nothing was measured, 1 means the claim was refuted.
  # Conflating them is how a run whose every leg came back COMM_FAILURE reads
  # as a pass, which is exactly what the un-gated version of this probe did.
  if [ "$ir_stubs" -eq 0 ]; then
    skip absent git:spikes/dk_peer.py \
         "omniORBpy IR stubs absent (fixture: omniORBpy's omniORB.ir_idl) — every" \
         "definition kind is unmeasured, not passing"
  else
    dk_out=$(python3 spikes/dk_peer.py "$IFR_IOR" 2>&1); dk_rc=$?
    # 10 is a FLOOR — nine definitions plus the leg that checks the expected
    # table against omniORB's own DefinitionKind — not today's figure. Adding a
    # definition to dkprobe.idl raises it. Exit 0 over one leg is what an
    # emptied table looks like, and it is why the floor is here at all.
    dk_legs=$(sed -n 's/^def_kind: every leg answered as expected (\([0-9][0-9]*\) legs)$/\1/p' <<<"$dk_out")
    case "$dk_rc" in
      0)
        if [ -z "$dk_legs" ] || [ "$dk_legs" -lt 10 ]; then
          echo "  FAIL the def_kind probe exited 0 over ${dk_legs:-no} legs (floor 10) — it measured"
          echo "       less than it has"
          ifr_fail=1
        else
          echo "  ok   omniORB's IR client named every kind our facade answers, $dk_legs legs:"
          echo "       struct/enum/exception/alias/constant/interface, and the three that used to"
          echo "       come back dk_none — valuetype, abstract interface, native"
        fi ;;
      3)
        # Not a SKIPPED: the optional fixture was already probed and is
        # present, so exit 3 here means OUR facade stopped answering after it
        # wrote READY. Unmeasured is a failure — and saying so in these words
        # keeps a reader from going after a wire defect that is not there.
        echo "  FAIL the def_kind probe measured NOTHING (exit 3) — it never reached the facade,"
        echo "       so no claim was refuted and there is no wire defect to chase; the holding"
        echo "       facade stopped answering after it wrote READY"
        diag_out "$dk_out" 3
        ifr_fail=1 ;;
      *)
        echo "  FAIL omniORB's IR client did not name our definition kinds (exit $dk_rc)"
        diag_out "$dk_out" 12
        ifr_fail=1 ;;
    esac
  fi
else
  echo "  FAIL the holding IFR facade never came up"; ifr_fail=1
fi
kill "$IFR_PID" >/dev/null 2>&1 || true
wait "$IFR_PID" 2>/dev/null || true
[ "$ifr_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Remote IFR ingestion: describing and calling with no IDL file ───────────
hr "remote IFR ingestion — a contract taken off the wire"
# Self-consistency first: our client against our facade proves the walk, the
# refusals and the TypeCode-driven call, and proves nothing about the
# specification. The JacORB leg is what makes it a claim — and it earned its
# place immediately, since JacORB's base_interfaces are Java class names and
# its version field is ":1.0", both of which our client refuses to guess from.
ing=$(cargo run -q --bin spike-ingest 2>&1)
if grep -q "ingest: PASS" <<<"$ing"; then
  echo "  ok   self-consistency: the walk, the refusals, and a call built from"
  echo "       ingested metadata with no .idl file opened"
else
  echo "  FAIL ingestion self-consistency"
  diag "a FAIL line" "$ing" "$(grep -i FAIL <<<"$ing")" 3
  fail_total=$((fail_total+1))
fi

# ── F7: the event channel, both directions ──────────────────────────────────
hr "event channel — our supplier and consumer, then omniORB's consumer"
# The push model served by us. Two things are measured that a self-test alone
# cannot establish: that an ORB we did not write can narrow and attach to the
# channel, and that a consumer which dies mid-stream is disconnected with its
# drops counted. A channel that loses events quietly is worse than one that
# refuses them.
ev_fail=0
EV_IOR=/tmp/orbweaver-events.ior
ev=$(cargo run -q --bin spike-events -- "$EV_IOR" 2>&1)
if grep -q "event-channel: PASS" <<<"$ev"; then
  echo "  ok   our client against our channel: connect both sides, 20 in order, dead consumer disconnected"
  ev_drop=$(printf '%s' "$ev" | grep 'drop report' | sed 's/^ *//')
  echo "  ok   $ev_drop"
  # The split (fa8a4f5) is what makes PLAN-DEFERRED §1's trigger answerable at
  # all: one counter summed five causes, so a clean stop() moved the same
  # number as an overloaded consumer. Echoing the report as an ok and checking
  # nothing is the "prose after an ok reads as coverage" class this file keeps
  # finding. Phase 2 cuts a dead consumer and drives no overflow, so the split
  # must attribute every drop to that cut and none to back-pressure —
  # `on_failure_disconnect` is deliberately NOT pinned to a number: how many
  # of phase 2's six events the dead proxy is still connected for is a
  # scheduling race (3 in every run measured, which is not the same as
  # guaranteed).
  case "$ev_drop" in
    *"overflow=0"*"on_disconnect=0"*"at_stop=0"*)
      case "$ev_drop" in
        *"on_failure_disconnect=0"*)
          echo "  FAIL no drop was attributed to the cut consumer: $ev_drop"; ev_fail=1 ;;
        *) echo "  ok   drops attributed to the cut consumer only — none to back-pressure, none to housekeeping" ;;
      esac ;;
    *) echo "  FAIL the drop split named a cause this phase did not drive: $ev_drop"; ev_fail=1 ;;
  esac
else
  echo "  FAIL event channel self-consistency"
  diag "a FAIL line" "$ev" "$(grep FAIL <<<"$ev")" 3
  ev_fail=1
fi
rm -f "$EV_IOR" /tmp/orbweaver-events-hold.log
cargo run -q --bin spike-events -- "$EV_IOR" --hold >/tmp/orbweaver-events-hold.log 2>&1 &
EV_PID=$!
ev_up=0
for _ in $(seq 1 60); do
  grep -qs HOLDING /tmp/orbweaver-events-hold.log && { ev_up=1; break; }
  sleep 0.2
done
if [ "$ev_up" -eq 1 ]; then
  # Two spellings of one fact used to live here: an `*ImportError*` arm decided
  # whether the fixture existed, and a `*PASS*` arm decided whether it had
  # worked. Both are string matches over a stream that can print either word
  # for the wrong reason — a traceback echoes the source line it failed on, and
  # this consumer's own docstring says the word PASS. Both are now exit codes:
  # the stub probe's, and `event_consumer.py`'s own (0 an event arrived, 1 it
  # did not, non-zero-and-noisy it could not run).
  if ! python3 -c "import CosEventComm, CosEventComm__POA, CosEventChannelAdmin" \
       >/dev/null 2>&1; then
    skip absent git:spikes/event_consumer.py \
         "omniORBpy CosEventComm/CosEventChannelAdmin stubs absent (fixture:" \
         "spikes/event_consumer.py needs them) — the cross-ORB half is" \
         "unmeasured, not passing"
  else
    ev_out=$(python3 spikes/event_consumer.py "$EV_IOR" 2>&1); ev_rc=$?
    if [ "$ev_rc" -eq 0 ]; then
      echo "  ok   omniORB's PushConsumer received events from OUR channel"
    else
      echo "  FAIL cross-ORB consumer (exit $ev_rc)"
      diag_out "$ev_out" 6
      ev_fail=1
    fi
  fi
else
  # Print what the fixture said: on CI (run for 46ccaae, 2026-08-19) this line
  # fired once with nothing to read, right after the self-consistency spike
  # had failed on a race, and did not reproduce on the next run.
  echo "  FAIL the holding event channel never came up within 12s; its log:"
  diag_log /tmp/orbweaver-events-hold.log 5
  ev_fail=1
fi
kill "$EV_PID" >/dev/null 2>&1 || true
wait "$EV_PID" 2>/dev/null || true
# `cargo run` forks; killing cargo does not kill the binary it launched, and a
# leaked channel holds a port and a log for the group below. `fkill` is the
# process-group-scoped killer defined at the top of this file — it will not
# touch a spike-events somebody started by hand in another checkout.
fkill spike-events
[ "$ev_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── F7b: the pull model, with omniORB as the SUPPLIER ────────────────────────
hr "event channel — omniORB is the pull supplier and OUR channel does the asking"
# Our channel holds a PullSupplier reference and cannot tell that an ORB we did
# not write is behind it — the same interface its own PullSupplierServant answers
# in the group above. Backend.
bears_on backend
# `event_consumer.py` above measures the direction where our channel calls an
# ORB we did not write. This measures the other one: our channel is the
# *client*, it dials omniORB's `PullSupplier` and invokes `try_pull` on a
# schedule it owns. A convention both ends apply cannot be refuted by a round
# trip — our own PullSupplierServant and our own channel were written from one
# reading of one chapter, so a wrong reading agrees with itself. omniORB read
# the chapter separately.
#
# Both byte orders, because `--source-endian` is what the channel asks in and a
# supplier replies in the order it was asked in — so this flag, and only this
# flag, is what puts both orders of a pulled event on the wire. The flag is
# read at start-up, so it is one fixture per order rather than one fixture and
# a switch.
#
# The fixture is the built binary, not `cargo run`: `cargo run` forks, so the
# PID this group holds would be cargo's and the channel would outlive the kill.
pull_fail=0
if ! python3 -c "import CosEventComm, CosEventComm__POA, CosEventChannelAdmin" >/dev/null 2>&1; then
  skip absent git:spikes/event_pull_supplier.py \
       "omniORBpy CosEventComm/CosEventChannelAdmin stubs absent (fixture:" \
       "spikes/event_pull_supplier.py needs them) — the pull direction is unmeasured," \
       "not passing"
elif ! cargo build -q --bin spike-events >/dev/null 2>&1; then
  echo "  FAIL spike-events did not build, so the pull direction was NOT measured"
  pull_fail=1
else
  PULL_IOR=/tmp/orbweaver-events-pull.ior
  PULL_LOG=/tmp/orbweaver-events-pull-hold.log
  for pull_e in big little; do
    rm -f "$PULL_IOR" "$PULL_LOG"
    "$ROOT/target/debug/spike-events" "$PULL_IOR" --hold --source-endian "$pull_e" \
      >"$PULL_LOG" 2>&1 &
    PULL_PID=$!
    # A wait loop that sleeps, bounded by a deadline, and that gives up early
    # when what it waits for has died — a loop without the sleep finishes in
    # microseconds and does not wait at all (CLAUDE.md, and it cost a phantom
    # failure here once).
    pull_up=0
    for _ in $(seq 1 100); do
      grep -qs HOLDING "$PULL_LOG" && { pull_up=1; break; }
      kill -0 "$PULL_PID" 2>/dev/null || break
      sleep 0.2
    done
    # The fixture's own account of the order it will ask in. Without this the
    # group would loop twice, print "both byte orders", and be green over ONE
    # order the day `--source-endian` is renamed, dropped or silently ignored —
    # the loop variable would still say big and little. Measured on the fixture
    # that predates the pull half: it prints `listening on 127.0.0.1:PORT` with
    # no endian clause at all, which is exactly the shape this catches.
    pull_said=$(sed -n 's/.*(asking suppliers \([A-Za-z]*\)-endian).*/\1/p' <<<"$(cat "$PULL_LOG" 2>/dev/null)")
    pull_want=$(tr '[:lower:]' '[:upper:]' <<<"${pull_e:0:1}")${pull_e:1}
    if [ "$pull_up" -ne 1 ]; then
      echo "  FAIL the holding channel (--source-endian $pull_e) never came up within 20s; its log:"
      diag_log "$PULL_LOG" 6
      pull_fail=1
    elif [ "$pull_said" != "$pull_want" ]; then
      echo "  FAIL --source-endian $pull_e did not take: the channel says it asks"
      echo "       '${pull_said:-nothing at all}', so this leg would have measured the other order"
      pull_fail=1
    else
      # The verdict is the peer's exit code. It prints PASS, and it also prints
      # the word FAIL on several of its own diagnosis lines, so matching text
      # here would be matching a stream that carries both answers.
      pull_out=$(python3 spikes/event_pull_supplier.py "$PULL_IOR" 2>&1); pull_rc=$?
      if [ "$pull_rc" -eq 0 ]; then
        echo "  ok   --source-endian $pull_e: our channel fetched every event from omniORB's"
        echo "       PullSupplier with try_pull and never the blocking pull, and both of omniORB's"
        echo "       consumer models got them in order"
      else
        echo "  FAIL --source-endian $pull_e: the pull direction (exit $pull_rc)"
        diag_out "$pull_out" 8
        pull_fail=1
      fi
    fi
    # PULL_PID is the channel itself, not a `cargo run` that forked it, so this
    # kill reaches the process holding the port — which is the whole reason the
    # binary is built above and invoked directly.
    kill "$PULL_PID" >/dev/null 2>&1 || true
    wait "$PULL_PID" 2>/dev/null || true
  done
fi
[ "$pull_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Identity: what the peers actually advertise ──────────────────────────────
hr "identity propagation — what a real target says about security"
# §4.8 predicts that many legacy targets have no authentication at all, and that
# where a target cannot enforce a caller identity the bridge is the only
# enforcement point. That is a claim about real deployments, so it is measured
# on real IORs rather than assumed — and the answer belongs in the catalogue.
id_fail=0
if start_server; then
  csi=$(cargo run -q --bin spike-dump -- spikes/echo.ior 2>/dev/null | grep '^csiv2')
  if grep -q "advertises no mechanism list" <<<"$csi"; then
    echo "  ok   omniORB 4.3.4 advertises no CSIv2: the bridge is the only enforcement point"
  else
    echo "  note omniORB advertises: $csi"
  fi
  # PLAN §4.8's per-peer record, through the page an operator reads (1d4404b,
  # 2026-08-19): the console catalog carries each reference's CSIv2 capability
  # beside its interface — the same classification RecordedOnly and the audit
  # line derive from. Negative control: tests/peer_record.rs fabricates an
  # IOR with an identity-asserting mechanism list, both byte orders, and reads
  # enforced-by=target.
  rec=$(cargo run -q -p orbweaver-console --bin orbweaver-console -- \
          catalog spikes/echo.idl --ior spikes/echo.ior --text 2>&1 | grep '^  peer: ')
  case "$rec" in
    *"enforced-by=bridge only"*) echo "  ok   omniORB 4.3.4 on the catalog page: enforced-by=bridge only — the record, not a sentence" ;;
    *"enforced-by=target"*)      echo "  note omniORB advertises identity assertion: $rec" ;;
    *) echo "  FAIL the catalog page carries no peer record for spikes/echo.ior"; id_fail=1 ;;
  esac
  ssl=$(cargo run -q --bin spike-dump -- spikes/echo.ior 2>/dev/null | grep '^ssliop')
  if grep -q "no TAG_SSL_SEC_TRANS" <<<"$ssl"; then
    echo "  ok   and no TAG_SSL_SEC_TRANS either — TLS work (D002) starts from a measured baseline"
  else
    echo "  note omniORB ssliop: $ssl"
  fi
else
  id_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jcsi.log 2>&1 & )
  ji=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; ji=1; break; }
    sleep 0.1
  done
  if [ "$ji" -eq 1 ]; then
    csi=$(cargo run -q --bin spike-dump -- spikes/jacorb.ior 2>/dev/null | grep '^csiv2')
    if grep -q "advertises no mechanism list" <<<"$csi"; then
      echo "  ok   JacORB 3.9 advertises none either — two peers, same answer"
    else
      echo "  note JacORB advertises: $csi"
    fi
    rec=$(cargo run -q -p orbweaver-console --bin orbweaver-console -- \
            catalog spikes/echo.idl --ior spikes/jacorb.ior --text 2>&1 | grep '^  peer: ')
    case "$rec" in
      *"enforced-by=bridge only"*) echo "  ok   JacORB 3.9 on the catalog page: enforced-by=bridge only" ;;
      *"enforced-by=target"*)      echo "  note JacORB advertises identity assertion: $rec" ;;
      *) echo "  FAIL the catalog page carries no peer record for spikes/jacorb.ior"; id_fail=1 ;;
    esac
    nc=$(cargo test -q -p orbweaver-console --test peer_record 2>&1)
    if grep -q '^test result: ok' <<<"$nc"; then
      echo "  ok   negative control: a fabricated mechanism list reads enforced-by=target, both byte orders"
    else
      echo "  FAIL negative control for the peer record"; id_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; id_fail=1
  fi
  fkill "classes Server"
else
  skip absent git:spikes/jacorb/setup.sh "JacORB half — fixture absent"
fi
# D010 B2: identity through a real provider. Until 2026-08-19 this was a
# `note`, which the verdict line does not count; a class-B row lands as a
# SKIPPED group naming its fixture, never as prose. The fixture is two things:
# a peer that advertises a CSIv2 mechanism list (neither installed ORB does —
# measured just above) and an OIDC/JWT issuer to exchange a token against
# (`ORBWEAVER_IDP_URL`). When both are present this line becomes the
# measurement; until then the deliberately-empty verifier stays empty, and a
# verifier wrong in the accepting direction would interoperate perfectly.
if [ -n "${ORBWEAVER_IDP_URL:-}" ] && ! grep -q "advertises no mechanism list" <<<"$csi"; then
  echo "  FAIL an identity provider and a CSIv2 peer are configured and nothing here measures them yet (D010 B2)"
  fail_total=$((fail_total+1))
else
  # No date: both halves of this fixture — a peer that advertises CSIv2 and an
  # OIDC issuer — are outside the tree, so nothing here can date the claim.
  skip absent "" \
       "no peer advertises CSIv2 and no issuer is configured (ORBWEAVER_IDP_URL) — identity" \
       "through a real provider is unmeasured, not passing (D010 B2; CSIv2 encoding is" \
       "unit-tested in both byte orders, PLAN §4.8)"
fi
[ "$id_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Static generation: stream B ──────────────────────────────────────────────
hr "static generation — stubs from the registry, oracle: static equals dynamic"
# The dynamic path is the reference implementation — it is the one verified
# against two independent ORBs — so a generated stub is correct exactly when
# its bytes equal the dynamic bytes for the same values (§8). The generated
# crate is deliberately OUTSIDE the workspace: compiling it proves the stubs
# stand on the published crate surface alone.
gen_fail=0
GEN_OUT=$(mktemp -d)/genout
if cargo run -q --bin gen-corpus -- --out "$GEN_OUT" --workspace "$ROOT" \
     corpus/golden/*.idl corpus/services/*.idl spikes/echo.idl \
     >/tmp/orbweaver-gen.log 2>&1; then
  n_items=$(grep -o 'generated [0-9]* item' /tmp/orbweaver-gen.log | grep -o '[0-9]*')
  echo "  ok   $n_items item(s) generated from the golden corpus plus the fixture"
  grep '^skipped' /tmp/orbweaver-gen.log | sed 's/^/  note /'
  if (cd "$GEN_OUT" && CARGO_TARGET_DIR="$ROOT/target" cargo build -q 2>/tmp/orbweaver-genc.log); then
    echo "  ok   every generated stub compiles outside the workspace"
    # A plain build proves neither of the declarations the emitted modules
    # carry: forbid(unsafe_code) and deny(missing_docs) only bite with
    # -D warnings, and generated code held to a lower standard than
    # hand-written code is generated code nobody will trust.
    if (cd "$GEN_OUT" && CARGO_TARGET_DIR="$ROOT/target" \
          RUSTFLAGS="-D warnings" cargo build -q 2>/tmp/orbweaver-gend.log); then
      echo "  ok   and under -D warnings: no unsafe, no undocumented item"
    else
      echo "  FAIL generated code does not survive its own lint declarations"
      diag_log /tmp/orbweaver-gend.log 5 head
      gen_fail=1
    fi
    # The serving direction. Everything above measures a generated *client*
    # against a stock ORB; this measures a stock ORB's client against a
    # generated *skeleton*, which is the half nothing had ever checked.
    if python3 -c 'import omniORB' >/dev/null 2>&1; then
      skel=$(cargo test -q -p orbweaver-gen --test skeleton_wire -- --nocapture \
             omniorb_python_drives_the_generated_skeleton 2>&1)
      if grep -q "^OK$" <<<"$skel"; then
        echo "  ok   omniORB's python client drove a GENERATED skeleton: narrow, attributes,"
        echo "       a oneway then a twoway on one connection, both user exceptions by class"
      else
        echo "  FAIL omniORB's python client could not drive the generated skeleton"
        diag_out "$skel" 5
        gen_fail=1
      fi
    else
      skip absent git:crates/orbweaver-gen/tests/skeleton_wire.rs \
           "omniORBpy absent — the serving direction is unmeasured, not passing"
    fi
    # A generated servant's system exceptions, read by class by an ORB we did
    # not write. This is where the transposed completion status was caught:
    # every local comparison used the same enum on both sides and agreed with
    # itself, so only a foreign reader could disagree.
    if python3 -c 'import omniORB' >/dev/null 2>&1; then
      flt=$(cargo test -q -p orbweaver-gen --test servant_faults -- --nocapture \
            omniorb_python 2>&1)
      if grep -q "CORBA.NO_PERMISSION" <<<"$flt" \
         && grep -q "COMPLETED_NO" <<<"$flt"; then
        echo "  ok   omniORB caught a servant's system exceptions by class, and read"
        echo "       did_not_run() as COMPLETED_NO — §4.11.4's ordinal, retry-safe"
      else
        echo "  FAIL omniORB did not see the servant's system exceptions as sent"
        diag_out "$flt" 5
        gen_fail=1
      fi
    else
      skip absent git:crates/orbweaver-gen/tests/servant_faults.rs \
           "omniORBpy absent — the servant-fault claims are unmeasured"
    fi
    # §8 in the reading that catches a dropped bound: the two paths must refuse
    # alike. Byte equality only ever samples values both paths accepted, so a
    # bound the generator dropped was invisible to the oracle above — which is
    # how it survived until D006 measured it while arguing about something else.
    bo=$(cargo test -q -p orbweaver-gen --test bounds_oracle 2>&1)
    if grep -q "^test result: ok" <<<"$bo"; then
      n_bo=$(printf '%s' "$bo" | grep -o '^test result: ok. [0-9]*' | grep -o '[0-9]*$')
      echo "  ok   static and dynamic refuse alike: $n_bo bound case(s), both byte orders,"
      echo "       encode and decode, stub and skeleton, argument and reply direction"
    else
      echo "  FAIL a declared bound is enforced by one path and not the other"
      diag "a panic" "$bo" "$(grep -A3 "panicked" <<<"$bo")" 6
      gen_fail=1
    fi
    # D010 A3: the checked-in f_27_bounds stub through Bridge::connect_static
    # with a content stage — the payload seen as AnyJSON, the ledger clean, and
    # an over-bound argument refused by the stub's probe before the guard hears
    # of it (pinned, not moved).
    gs=$(cargo test -q -p orbweaver-gen --test guarded_stub 2>&1)
    if grep -q "^test result: ok" <<<"$gs"; then
      echo "  ok   a real generated stub through the guard: the content seat sees its payload, the ledger does not"
    else
      echo "  FAIL guarded_stub — the static path's content seat or ledger property"
      diag "a panic" "$gs" "$(grep -A3 "panicked" <<<"$gs")" 6
      gen_fail=1
    fi
    # §8's rule in the direction nothing checked: a skeleton's reply bytes
    # against the dynamic path's. No fixture — ours on one end, the reference
    # implementation on the other.
    ora=$(cargo test -q -p orbweaver-gen --test skeleton_oracle -- --nocapture 2>&1)
    if grep -q "FAILED" <<<"$ora"; then
      echo "  FAIL a generated skeleton's replies are not the dynamic path's bytes"
      diag "a 'disagree' line" "$ora" "$(grep -A4 "disagree" <<<"$ora")" 8
      gen_fail=1
    else
      n_cmp=$(printf '%s' "$ora" | grep -o '[0-9]* comparison' | grep -o '[0-9]*' \
              | awk '{s+=$1} END {print s+0}')
      echo "  ok   server-side static equals dynamic: $n_cmp reply comparison(s), both"
      echo "       byte orders, three GIOP versions, two reply origins"
    fi
    if start_server; then
      so=$("$ROOT/target/debug/static-oracle" spikes/echo.ior spikes/echo.idl 2>&1)
      if grep -q "static generation: PASS" <<<"$so"; then
        echo "  ok   static bytes equal dynamic bytes: Ragged, wstring, any, sequence, both orders"
        echo "  ok   the generated stub calls omniORB: 10/10 cases, both byte orders"
        echo "  ok   I1: the same stub through the guard — exposure, ai_authz scope and audit bind it"
        echo "  ok   I1: a refused call never reaches the wire; the audit holds nothing dialable"
        i4=$(grep "I4:" <<<"$so")
        if [ -n "$i4" ]; then
          sed -n '1,5p' <<<"$i4"
        else
          echo "       (static-oracle reported no I4: line — those claims were not shown)"
        fi
      else
        echo "  FAIL static did not equal dynamic"
        diag "a FAIL line" "$so" "$(grep "FAIL" <<<"$so")" 3
        gen_fail=1
      fi
    else
      gen_fail=1
    fi
    cleanup
  else
    echo "  FAIL generated code does not compile"
    diag_log /tmp/orbweaver-genc.log 5 head
    gen_fail=1
  fi
else
  echo "  FAIL generation failed"
  diag_log /tmp/orbweaver-gen.log 3 head
  gen_fail=1
fi
rm -rf "$(dirname "$GEN_OUT")"
[ "$gen_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Contract evolution: is the §5.3 rule table true? ─────────────────────────
hr "contract evolution — §5.3 verdicts against a peer that predates the change"
# The differ's verdicts are predictions about deployed peers. Asserting them
# only against our own tests would prove that two pieces of our code agree, so
# the predicted consequence is produced on the wire by omniORB instead.
ev_fail=0
if start_evolution_server; then
  out=$(cargo run -q --bin spike-evolution -- \
        spikes/evolution_v1.idl spikes/evolution_v2.idl spikes/evolution_v1b.idl \
        spikes/evolution.ior 2>&1)
  if grep -q "contract evolution: PASS" <<<"$out"; then
    echo "  ok   the swapped struct members are flagged BREAKING before release"
    echo "  ok   omniORB answered the swapped call with the WRONG member, no exception"
    echo "  ok   an added operation on an un-updated server gives BAD_OPERATION"
  else
    echo "  FAIL a §5.3 verdict did not match what the wire did"
    diag "a FAIL line" "$out" "$(grep "  FAIL" <<<"$out")" 3
    ev_fail=1
  fi
else
  ev_fail=1
fi
# The other half of "server-first": the additive release must serve both.
if start_evolution_server --updated; then
  out=$(cargo run -q --bin spike-evolution -- --updated spikes/evolution.ior 2>&1)
  if grep -q "contract evolution: PASS" <<<"$out"; then
    echo "  ok   after the additive release, old and new clients are both served"
  else
    echo "  FAIL the additive release did not behave as 'compatible' predicts"
    diag "a FAIL line" "$out" "$(grep "  FAIL" <<<"$out")" 2
    ev_fail=1
  fi
else
  ev_fail=1
fi
cleanup
# The gate is the deliverable, not the report: check that it actually refuses.
if cargo run -q --bin idl-diff -- spikes/evolution_v1.idl spikes/evolution_v2.idl \
     >/dev/null 2>&1; then
  echo "  FAIL idl-diff accepted a change that corrupts data on the wire"
  ev_fail=1
else
  echo "  ok   idl-diff refuses the breaking revision (exit 1)"
fi
if cargo run -q --bin idl-diff -- spikes/evolution_v1.idl spikes/evolution_v1b.idl \
     >/dev/null 2>&1; then
  echo "  ok   idl-diff accepts the additive-only revision"
else
  echo "  FAIL idl-diff refuses a revision that breaks nothing"
  ev_fail=1
fi
[ "$ev_fail" -eq 0 ] || fail_total=$((fail_total+1))

hr "transparency ledger — what a caller can still tell (D029 §6, D031 H2)"
# A READING OF THIS RUN, computed from what ran. It adds no check and removes
# none: every group above keeps its own verdict, `fail_total` and `skipped` keep
# their exact meanings, and the only thing this section can add to `fail_total`
# is its own inability to read the names it needs.
#
# Read the third column. "measured by N group(s)" is the cheap column — it says
# somebody looked. `unmeasured:` is the one a next batch is scoped from.
tp_unmeasured_names=""
tp_measured_names=""
if [ "$tp_load_err" = 1 ]; then
  echo "  FAIL the five transparency names could not be read from $TP_DOC §6.1,"
  echo "       so this run measured regression only and cannot say what a caller"
  echo "       can still tell. That is an unmeasured criterion, not a pass."
  diag_out "$(python3 spikes/transparency.py --names 2>&1)" 6 head
  fail_total=$((fail_total+1))
else
  if [ -z "$TP_TAGS" ]; then
    echo "  NO GROUP IN THIS RUN DECLARED A TRANSPARENCY."
    echo "  All five read as UNMEASURED below. That is not \"nothing is wrong\":"
    echo "  it means this run answered \"did anything regress\" and did not answer"
    echo "  D029 §6's criterion at all."
    echo ""
  fi
  while IFS= read -r tp_name; do
    [ -z "$tp_name" ] && continue
    tp_title=$(python3 spikes/transparency.py --title "$tp_name" 2>/dev/null)
    tp_idxs=$(awk -F'\t' -v n="$tp_name" '$1==n {print $2}' <<<"$TP_TAGS")
    tp_n=0; tp_red_n=0
    for tp_i in $tp_idxs; do tp_n=$((tp_n+1)); done
    printf '  %-11s %s\n' "$tp_name" "— ${tp_title:-$tp_name}"
    if [ "$tp_n" -eq 0 ]; then
      echo "              UNMEASURED — no group in this run declares bears_on $tp_name"
      tp_unmeasured_names="$tp_unmeasured_names $tp_name"
    else
      for tp_i in $tp_idxs; do
        tp_rn=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_RED")
        [ -n "$tp_rn" ] && tp_red_n=$((tp_red_n+1))
      done
      echo "              measured by $tp_n group(s) in this run, $tp_red_n of them red"
      for tp_i in $tp_idxs; do
        tp_gt=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_GROUPS")
        tp_rn=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_RED")
        tp_sk=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_SKIPS")
        # A group that skipped and a group whose sub-probe skipped print the
        # same `SKIPPED` line and this file cannot tell them apart, so it does
        # not guess: `+SKIPPED` says a skip was recorded here and the unmeasured
        # column below says which. Inventing the distinction would be a
        # classifier reading somebody else's sentence.
        if [ -n "$tp_rn" ]; then
          echo "                RED ($tp_rn)  $tp_gt"
        elif [ -n "$tp_sk" ]; then
          echo "                ok +SKIPPED $tp_gt"
        else
          echo "                ok          $tp_gt"
        fi
      done
      tp_measured_names="$tp_measured_names $tp_name"
    fi
    # The unmeasured column. Two sources, both CITED rather than restated: a
    # tagged group that skipped said in its own words why, and §6.1 says where
    # this transparency leaks today. Neither sentence is retyped here.
    for tp_i in $tp_idxs; do
      tp_sk=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_SKIPS")
      tp_sr=$(awk -F'\t' -v i="$tp_i" '$1==i {print $3}' <<<"$TP_SKIPS")
      [ -z "$tp_sk" ] && continue
      tp_gt=$(awk -F'\t' -v i="$tp_i" '$1==i {print $2}' <<<"$TP_GROUPS")
      echo "              unmeasured: $tp_gt"
      if [ -n "$tp_sr" ]; then
        echo "                          SKIPPED ($tp_sk): $tp_sr"
      else
        echo "                          SKIPPED ($tp_sk) — the group printed its own"
        echo "                          SKIPPED line above, with its age"
      fi
    done
    tp_cite=$(python3 spikes/transparency.py --cite "$tp_name" 2>&1); tp_cite_rc=$?
    if [ "$tp_cite_rc" -ne 0 ] || [ -z "$tp_cite" ]; then
      echo "              FAIL D029 §6.1's status for $tp_name could not be read,"
      echo "                   so the load-bearing column is missing for it"
      diag_out "$tp_cite" 4 head
      fail_total=$((fail_total+1))
    else
      echo "              unmeasured, per D029 §6.1 — where it leaks today:"
      sed -e 's/^/                | /' <<<"$tp_cite"
    fi
  done <<<"$TP_NAMES"
  echo ""
  echo "  Where a line above says D029 §6.1, the sentence is READ from that table"
  echo "  at run time, not copied into this harness. No score is printed here and"
  echo "  none should be derived: a shrinking unmeasured list is progress only"
  echo "  when a run closed the leak, and looks identical to nobody looking."
fi

hr "verdict"
if [ "$skipped" -gt 0 ]; then
  echo "  $skipped check group(s) SKIPPED — those claims are unmeasured, not passing"
  # `skipped` still counts every skip, which is the number D010 §2 makes
  # load-bearing and which nothing here changes. What is added is the split: a
  # fixture that is not here and a recording of another day standing in for a
  # live run are different claims, and this line used to print them as one.
  echo "  of those, $replays replayed a recording of another day and $((skipped-replays)) found no fixture at all;"
  echo "  each SKIPPED above carries its own age, or says the date is not recorded"
fi
if [ "$fail_total" -eq 0 ]; then
  echo "  all measured checks green"
else
  echo "  $fail_total check group(s) failed"
fi
# And the other dimension, LAST, so that "all measured checks green" cannot be
# read on its own. Names, not a count: "3 of 5" would be quoted as sixty per
# cent complete by the first person to repeat it, and D031 §2 refuses a score.
# Naming the unmeasured ones cannot be turned into one, and is the useful half.
if [ "$tp_load_err" = 1 ]; then
  echo "  transparency: NOT READ — $TP_DOC §6.1 could not be parsed (see the ledger)"
elif [ -z "$tp_measured_names" ]; then
  echo "  transparency: NONE measured in this run —$tp_unmeasured_names all unmeasured."
  echo "  A green verdict above means nothing regressed; it does not mean anything"
  echo "  was learned about what a caller can still tell (D029 §6)."
else
  echo "  transparency measured this run:$tp_measured_names"
  if [ -n "$tp_unmeasured_names" ]; then
    echo "  transparency UNMEASURED this run:$tp_unmeasured_names"
    echo "  — unmeasured, not passing; the ledger above names what each one waits on"
  fi
fi
exit "$fail_total"
