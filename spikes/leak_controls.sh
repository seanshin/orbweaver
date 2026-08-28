#!/usr/bin/env bash
# The negative controls for D029 §5 O0's leak tests, runnable without the harness.
#
# `run_checks.sh` takes a machine-wide lock and runs for over half an hour, so
# every batch is told not to start it — and *a prohibition without its
# replacement is an instruction to skip the check* (CLAUDE.md). This is the
# replacement for every leak test whose leak is switched by
# `ORBWEAVER_LEAK_CONTROL`, which is now two files rather than one.
#
# THE SECOND SUBJECT ARRIVED THE WAY THIS SCRIPT PREDICTS. On 2026-08-28
# `crates/orbweaver-giop/tests/what_a_caller_can_tell_about_a_removal.rs`
# landed with a working `removal_isolation` arm, and `leak_tests.sh` printed
#
#   say "       control: ORBWEAVER_LEAK_CONTROL=removal_isolation ... Run
#                        2026-08-27: the fresh dial of B answered Gone where
#                        it must answer Reply(22)"
#
# which is a control run once, by hand, on a day, and recorded in prose — the
# exact sentence eleven lines below says is not a property of the tree. It was
# found by the sweep for documents that cite an executable nothing runs, one
# hour after that sweep's own gate landed, in a leg written the same day.
# A script whose subject is a single `SRC` grows this hole every time a leak
# test is added somewhere else, so the subject is a table.
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
# crate | test target | control:tests-that-must-go-red[ control:...]
#
# The source path is COMPUTED from the first two fields rather than typed a
# third time: a file this table cannot find is a failure below, never a leg
# that quietly does not run.
SUBJECTS=(
  "orbweaver-test|what_a_caller_can_tell|no_forward:a_move_under_a_live_caller_is_invisible limits_survive_a_move|address:a_move_under_a_live_caller_is_invisible|backend:backend_swapped_under_a_live_caller"
  "orbweaver-giop|what_a_caller_can_tell_about_a_removal|removal_isolation:removing_one_target_is_invisible_to_a_caller_of_another"
)

fails=0
checks=0

say() { printf '\n\033[1m%s\033[0m\n' "$1"; }
ok()   { checks=$((checks+1)); echo "  ok   $1"; }
bad()  { checks=$((checks+1)); fails=$((fails+1)); echo "  FAIL $1"; }

# ── The heads the test file owns, read from it ──────────────────────────────
# Every sentence a leak test uses to say what the caller could tell. Read, not
# retyped — *a classifier is a sentence too*. If a file stops carrying any,
# that is a failure here rather than a quiet weakening of assertion 3 below.
read_heads() {
  local src="$1" heads
  heads=$(grep -o 'THE CALLER[^:]*:' "$src" | sort -u)
  if [ -z "$heads" ]; then
    echo "FAIL $src carries no \"THE CALLER ...:\" sentence, so this script has" >&2
    echo "     nothing to hold its output against and assertion 3 would pass vacuously." >&2
    return 1
  fi
  printf '%s\n' "$heads"
}

# ── The expected table, checked against the owner before any leg runs ───────
#
# NOT every leak test's control lives here. This script drives the ones whose
# leaks are switched by `ORBWEAVER_LEAK_CONTROL`, because they are servant
# behaviour a test cannot re-enter. The activation leg's control is **inside
# its own test file** —
# `crates/orbweaver-object/tests/what_a_caller_can_tell_about_load.rs`, in
# `the_refusing_miss_policies_are_the_leak` — because its leak is a `MissPolicy`
# variant, so putting the leak back is passing a different enum to the same
# fixture and needs no switch, no environment variable and no second process.
# `cargo test` runs it in both directions. Absence from this table is therefore
# not absence of a control; the next reader is told here so they do not have to
# infer it from a table that does not mention activation.
say "the expected table names tests these files actually have"
for subject in "${SUBJECTS[@]}"; do
  IFS='|' read -r s_crate s_target s_rest <<<"$subject"
  s_src="$ROOT/crates/$s_crate/tests/$s_target.rs"
  if [ ! -r "$s_src" ]; then
    bad "$s_crate/$s_target: no such test file ($s_src) — a moved or renamed"
    echo "       subject, not a control that stopped working"
    continue
  fi
  IFS='|' read -r -a s_rows <<<"$s_rest"
  for row in "${s_rows[@]}"; do
    for t in ${row#*:}; do
      if grep -q "fn $t()" "$s_src"; then
        ok "$t is a test in $s_crate/$s_target"
      else
        bad "$t is in this script's expected table and is NOT a test in $s_src — a rename, not a broken control"
      fi
    done
  done
done
if [ "$fails" -ne 0 ]; then
  echo ""
  echo "expected table does not match the source; not running the controls."
  exit 1
fi

# run_control <crate> <target> <heads> <name> <expected-red...>
run_control() {
  local crate="$1" target="$2" heads="$3" name="$4"; shift 4
  local expected=("$@")
  local out rc
  out=$(ORBWEAVER_LEAK_CONTROL="$name" cargo test -q -p "$crate" --test "$target" 2>&1); rc=$?

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
  done <<<"$heads"
  if [ "$named" -eq 1 ]; then
    ok "$name: the failure names what the caller could tell, in the test's own sentence"
  else
    bad "$name: the run went red without naming what the caller could tell —"
    echo "       a red that does not say what leaked is a diagnosis nobody can act on"
  fi
}

for subject in "${SUBJECTS[@]}"; do
  IFS='|' read -r s_crate s_target s_rest <<<"$subject"
  s_src="$ROOT/crates/$s_crate/tests/$s_target.rs"
  s_heads=$(read_heads "$s_src") || { fails=$((fails+1)); checks=$((checks+1)); continue; }
  IFS='|' read -r -a s_rows <<<"$s_rest"

  say "$s_crate/$s_target — each leak, put back, must be seen"
  for row in "${s_rows[@]}"; do
    # shellcheck disable=SC2086
    run_control "$s_crate" "$s_target" "$s_heads" "${row%%:*}" ${row#*:}
  done

  # ── 4 — and the same tests, with nothing put back, are green ──────────────
  green_out=$(ORBWEAVER_LEAK_CONTROL=none cargo test -q -p "$s_crate" --test "$s_target" 2>&1); green_rc=$?
  if [ "$green_rc" -eq 0 ]; then
    ok "with no leak put back the same tests are green ($(grep -E '^test result:' <<<"$green_out" | head -1))"
  else
    bad "$s_crate/$s_target does not pass unmodified, so nothing above is evidence about a leak"
    grep -E '^(test result|thread|---- )' <<<"$green_out" | head -10 | sed 's/^/       /'
  fi
done

# A control this script does not run, named rather than left looking covered.
say "not covered by this script"
cat <<'NOTE'
  The fourth control for `limits_survive_a_move` is in orbweaver-giop, which the
  batch that wrote these tests does not own, so it cannot be an
  ORBWEAVER_LEAK_CONTROL arm. It is a temporary edit — deleting
  `self.set_orb_limits(limits);` from `Connection::move_to` — and what it
  printed is recorded in that test's own rustdoc and in 6e7249a's message.
  This script therefore proves four of the five controls it names, not five.
NOTE

say "verdict"
echo "  $checks check(s), $fails failure(s)"
if [ "$fails" -eq 0 ]; then
  echo "  every leak this file names is put back and seen; the tests are green without it."
else
  echo "  a control that cannot make its test red is a test that measures nothing."
fi
exit $(( fails > 0 ? 1 : 0 ))
