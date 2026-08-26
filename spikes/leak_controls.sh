#!/usr/bin/env bash
# The negative controls for D029 §5 O0's leak tests, runnable without the harness.
#
# `run_checks.sh` takes a machine-wide lock and runs for over half an hour, so
# every batch is told not to start it — and *a prohibition without its
# replacement is an instruction to skip the check* (CLAUDE.md). This is the
# replacement for `crates/orbweaver-test/tests/what_a_caller_can_tell.rs`.
#
# WHY IT EXISTS. A leak test that cannot be made red is a group that measures
# nothing, which is the class this project has found nine times. The leaks are
# therefore built into the test file and switched with `ORBWEAVER_LEAK_CONTROL`,
# and this script is the thing that proves each switch still works. Without it,
# "the controls were run once, by the agent that wrote them" is a sentence in a
# commit message and not a property of the tree.
#
# WHAT IT ASSERTS, per control:
#   1  the run goes red                     — exit code, never a marker in text
#   2  exactly the expected tests go red    — a control that reddens everything
#                                             is not controlling what it names
#   3  the output names what the caller could tell, in a sentence THE TEST FILE
#      OWNS — the heads are grepped out of the source, never retyped here.
#      *A classifier is a sentence too*: a head typed in this file would drift
#      from the test's wording the day somebody improves it, and this script
#      would go green over the drift.
#   4  with no control set, the same tests are green
#
# The expected test names are checked against the source BEFORE any leg runs —
# the `dk_peer` lesson, so a renamed test fails as *our* table rather than
# looking like a control that stopped working.
#
# Exit code is the verdict. It starts no fixture, takes no lock, and writes
# nothing outside the target directory cargo already owns.

set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
SRC="$ROOT/crates/orbweaver-test/tests/what_a_caller_can_tell.rs"
TESTBIN=(cargo test -q -p orbweaver-test --test what_a_caller_can_tell)

fails=0
checks=0

say() { printf '\n\033[1m%s\033[0m\n' "$1"; }
ok()   { checks=$((checks+1)); echo "  ok   $1"; }
bad()  { checks=$((checks+1)); fails=$((fails+1)); echo "  FAIL $1"; }

[ -r "$SRC" ] || { echo "FAIL the leak tests are not where this script expects them: $SRC"; exit 2; }

# ── The heads the test file owns, read from it ──────────────────────────────
# Every sentence a leak test uses to say what the caller could tell. Read, not
# retyped. If the file stops carrying any, that is a failure here rather than a
# quiet weakening of assertion 3 below.
HEADS=$(grep -o 'THE CALLER[^:]*:' "$SRC" | sort -u)
if [ -z "$HEADS" ]; then
  echo "FAIL $SRC carries no \"THE CALLER ...:\" sentence, so this script has"
  echo "     nothing to hold the output against and assertion 3 would pass vacuously."
  exit 2
fi

# ── The expected table, checked against the owner before any leg runs ───────
# control : the tests that must go red, space separated
CONTROLS=(
  "no_forward:a_move_under_a_live_caller_is_invisible limits_survive_a_move"
  "address:a_move_under_a_live_caller_is_invisible"
  "backend:backend_swapped_under_a_live_caller"
)
say "the expected table names tests this file actually has"
for row in "${CONTROLS[@]}"; do
  for t in ${row#*:}; do
    if grep -q "fn $t()" "$SRC"; then
      ok "$t is a test in the source"
    else
      bad "$t is in this script's expected table and is NOT a test in $SRC — a rename, not a broken control"
    fi
  done
done
if [ "$fails" -ne 0 ]; then
  echo ""
  echo "expected table does not match the source; not running the controls."
  exit 1
fi

# run_control <name> <expected-red...>
run_control() {
  local name="$1"; shift
  local expected=("$@")
  local out rc
  out=$(ORBWEAVER_LEAK_CONTROL="$name" "${TESTBIN[@]}" 2>&1); rc=$?

  # 1 — red, by exit code. Probes use exit codes, not markers.
  if [ "$rc" -ne 0 ]; then
    ok "$name: the run went red (exit $rc)"
  else
    bad "$name: the run stayed GREEN — this leak is not being detected, so the"
    echo "       test it belongs to measures nothing"
    return
  fi

  # 2 — exactly the expected tests, no more and no fewer.
  local reported
  reported=$(grep -E '^    [a-z_]+$' <<<"$out" | tr -d ' ' | sort -u)
  local want
  want=$(printf '%s\n' "${expected[@]}" | sort -u)
  if [ "$reported" = "$want" ]; then
    ok "$name: exactly the expected test(s) went red — $(tr '\n' ' ' <<<"$want")"
  else
    bad "$name: the wrong set of tests went red"
    echo "       expected: $(tr '\n' ' ' <<<"$want")"
    echo "       got:      $(tr '\n' ' ' <<<"$reported")"
  fi

  # 3 — the output says what the caller could tell, in the file's own words.
  local named=0 head
  while IFS= read -r head; do
    [ -z "$head" ] && continue
    if grep -F -q -- "$head" <<<"$out"; then named=1; break; fi
  done <<<"$HEADS"
  if [ "$named" -eq 1 ]; then
    ok "$name: the failure names what the caller could tell, in the test's own sentence"
  else
    bad "$name: the run went red without naming what the caller could tell —"
    echo "       a red that does not say what leaked is a diagnosis nobody can act on"
  fi
}

say "each leak, put back, must be seen"
for row in "${CONTROLS[@]}"; do
  # shellcheck disable=SC2086
  run_control "${row%%:*}" ${row#*:}
done

# ── 4 — and the same tests, with nothing put back, are green ────────────────
say "with no leak put back, the same tests are green"
green_out=$(ORBWEAVER_LEAK_CONTROL=none "${TESTBIN[@]}" 2>&1); green_rc=$?
if [ "$green_rc" -eq 0 ]; then
  ok "the unmodified tests pass ($(grep -E '^test result:' <<<"$green_out" | head -1))"
else
  bad "the unmodified tests do not pass, so nothing above is evidence about a leak"
  grep -E '^(test result|thread|---- )' <<<"$green_out" | head -10 | sed 's/^/       /'
fi

# A control this script does not run, named rather than left looking covered.
say "not covered by this script"
cat <<'NOTE'
  The fourth control for `limits_survive_a_move` is in orbweaver-giop, which the
  batch that wrote these tests does not own, so it cannot be an
  ORBWEAVER_LEAK_CONTROL arm. It is a temporary edit — deleting
  `self.set_orb_limits(limits);` from `Connection::move_to` — and what it
  printed is recorded in that test's own rustdoc and in 6e7249a's message.
  This script therefore proves three of the four controls, not four.
NOTE

say "verdict"
echo "  $checks check(s), $fails failure(s)"
if [ "$fails" -eq 0 ]; then
  echo "  every leak this file names is put back and seen; the tests are green without it."
else
  echo "  a control that cannot make its test red is a test that measures nothing."
fi
exit $(( fails > 0 ? 1 : 0 ))
