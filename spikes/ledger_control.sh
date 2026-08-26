#!/usr/bin/env bash
# The transparency ledger's negative controls, runnable without the harness.
#
# `run_checks.sh` takes a machine-wide lock and runs for over half an hour, so
# every batch is told not to run it — and *a prohibition without its replacement
# is an instruction to skip the check* (CLAUDE.md, learned when eight corpus
# files landed with no front end having compared them). This is the replacement
# for the ledger: it exercises the ledger and `bears_on` in about a second, over
# the harness's REAL tag set, without starting a fixture or taking the lock.
#
# How it avoids being a second implementation: it does not contain a copy of the
# ledger. It cuts the ledger, `hr`, `bears_on` and the diagnostic helpers out of
# `run_checks.sh` with `awk` and runs those bytes. If the harness changes, this
# runs the change. The group BODIES are replaced with an `echo`, which is the
# whole point — the ledger is a reading of what ran, and a reading can be
# checked without re-running the thing it reads.
#
# Seven controls, each one a way the ledger could be green while measuring
# nothing:
#   1  nothing tagged            -> every transparency prints UNMEASURED, and
#                                   the verdict says so; NOT "all is well"
#   2  the real tag set          -> the tagged transparencies print a count and
#                                   the untagged ones still print UNMEASURED
#   3  a name §6.1 does not have -> the run FAILS naming the group and the name
#   4  a bears_on removed        -> that transparency's count drops by one and
#                                   the group leaves the list
#   5  a tagged group goes red   -> its transparency shows RED, not swallowed
#   6  a tagged group SKIPPED    -> it appears in the unmeasured column in its
#                                   own words
#   7  §6.1 unreadable           -> the ledger FAILS and says the criterion went
#                                   unmeasured, rather than printing nothing
#
# Exit code is the verdict. It starts no fixture, takes no lock, and writes only
# under a temporary directory.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
RC="$ROOT/spikes/run_checks.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-ledger-control.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
fails=0
checks=0

say()  { printf '\n\033[1m%s\033[0m\n' "$1"; }
# want <label> <haystack> <needle...>: every needle must be present.
want() {
  local label="$1" hay="$2"; shift 2
  local n miss=0
  checks=$((checks+1))
  for n in "$@"; do
    grep -F -q -- "$n" <<<"$hay" || { miss=1; echo "  FAIL missing: $n"; }
  done
  if [ "$miss" = 0 ]; then echo "  ok   $label"; else fails=$((fails+1)); fi
}
# reject <label> <haystack> <needle>: the needle must NOT be present.
reject() {
  local label="$1" hay="$2" n="$3"
  checks=$((checks+1))
  if grep -F -q -- "$n" <<<"$hay"; then
    echo "  FAIL present but should not be: $n"; fails=$((fails+1))
  else
    echo "  ok   $label"
  fi
}
want_rc() {
  local label="$1" got="$2" op="$3" exp="$4"
  checks=$((checks+1))
  if [ "$op" = eq ] && [ "$got" -eq "$exp" ]; then echo "  ok   $label (exit $got)"
  elif [ "$op" = ne ] && [ "$got" -ne "$exp" ]; then echo "  ok   $label (exit $got)"
  else echo "  FAIL $label: exit $got"; fails=$((fails+1)); fi
}

# The harness's own bytes: helpers, then the transparency block, then whatever
# body the caller wants, then the ledger and the verdict.
#   build <body-file> <out> [tree-root]
build() {
  local body="$1" out="$2" tree="${3:-$ROOT}"
  {
    echo '#!/usr/bin/env bash'
    echo 'set -uo pipefail'
    printf 'cd %q\n' "$tree"
    echo 'ROOT=$(pwd)'
    echo 'fail_total=0'; echo 'skipped=0'; echo 'replays=0'
    awk '/^diag\(\) \{/{p=1} /^# ── Kill this run/{p=0} p' "$RC"
    awk '/^# ── The dimension this harness did not have/{p=1} /^need\(\) \{/{p=0} p' "$RC"
    cat "$body"
    awk '/^hr "transparency ledger/{p=1} p' "$RC"
  } >"$out"
  /bin/bash -n "$out" || { echo "  FAIL the lifted driver does not parse"; return 2; }
  return 0
}

# The harness's real groups and real tags, bodies replaced by an `ok`.
awk '/^hr "verdict"/{exit}
     /^hr "transparency ledger/{exit}
     /^hr "/{print; print "echo \"  ok   (body not run in this control)\""; next}
     /^bears_on /{print}' "$RC" >"$WORK/body_all.sh"

TAGS=$(grep -c '^bears_on ' "$WORK/body_all.sh")
echo "lifted $(grep -c '^hr ' "$WORK/body_all.sh") group(s) and $TAGS tag(s) out of spikes/run_checks.sh"

# ── 1. nothing tagged ────────────────────────────────────────────────────────
say "control 1 — nothing declares a transparency"
printf 'hr "a group that measures something and declares nothing"\necho "  ok"\n' \
  >"$WORK/body_empty.sh"
build "$WORK/body_empty.sh" "$WORK/d1.sh" || fails=$((fails+1))
o=$(bash "$WORK/d1.sh" 2>&1); rc=$?
want "the ledger says nothing declared, in capitals" "$o" \
     "NO GROUP IN THIS RUN DECLARED A TRANSPARENCY."
want "every transparency reads UNMEASURED" "$o" \
     "UNMEASURED — no group in this run declares bears_on location" \
     "UNMEASURED — no group in this run declares bears_on lifecycle"
want "the verdict refuses to read as a pass" "$o" \
     "transparency: NONE measured in this run" \
     "it does not mean anything"
reject "no transparency is claimed as measured" "$o" "transparency measured this run:"
want_rc "an empty ledger does not fail the run — it reports" "$rc" eq 0

# ── 2. the real tag set ──────────────────────────────────────────────────────
say "control 2 — the harness's own tags"
build "$WORK/body_all.sh" "$WORK/d2.sh" || fails=$((fails+1))
o2=$(bash "$WORK/d2.sh" 2>&1); rc=$?
want "declared transparencies print a count and their groups" "$o2" \
     "group(s) in this run, 0 of them red" \
     "ok          NAT rewriting"
want "§6.1 is cited for every transparency, measured or not" "$o2" \
     "unmeasured, per D029 §6.1 — where it leaks today:"
want "an undeclared transparency still reads UNMEASURED" "$o2" \
     "UNMEASURED — no group in this run declares bears_on activation"
want "the verdict names both halves" "$o2" \
     "transparency measured this run:" "transparency UNMEASURED this run:"
want_rc "a green run stays green" "$rc" eq 0
loc_before=$(grep -c '^                ok  ' <<<"$(sed -n '/^  location /,/^  backend /p' <<<"$o2")")

# ── 3. a name §6.1 does not have ─────────────────────────────────────────────
say "control 3 — a tag naming something D029 §6.1 does not"
cp "$WORK/body_all.sh" "$WORK/body_bad.sh"
printf 'hr "a control group that mistypes a transparency"\nbears_on lifecyle\necho "  ok"\n' \
  >>"$WORK/body_bad.sh"
build "$WORK/body_bad.sh" "$WORK/d3.sh" || fails=$((fails+1))
o=$(bash "$WORK/d3.sh" 2>&1); rc=$?
want "the run names the group and the bad name" "$o" \
     'a control group that mistypes a transparency" declares bears_on "lifecyle"' \
     "the names have one home"
want_rc "a bad tag fails the run" "$rc" ne 0

# ── 4. a bears_on removed ────────────────────────────────────────────────────
say "control 4 — a group stops declaring what it bears on"
awk '/^hr "NAT rewriting/{n=1} n==1 && /^bears_on location$/{n=2; next} {print}' \
  "$WORK/body_all.sh" >"$WORK/body_untag.sh"
build "$WORK/body_untag.sh" "$WORK/d4.sh" || fails=$((fails+1))
o=$(bash "$WORK/d4.sh" 2>&1); rc=$?
loc_after=$(grep -c '^                ok  ' <<<"$(sed -n '/^  location /,/^  backend /p' <<<"$o")")
checks=$((checks+1))
if [ "$loc_after" -eq $((loc_before - 1)) ]; then
  echo "  ok   location's group list drops by exactly one ($loc_before -> $loc_after)"
else
  echo "  FAIL location's group list went $loc_before -> $loc_after, expected one fewer"
  fails=$((fails+1))
fi
reject "the untagged group leaves the list" "$(sed -n '/^  location /,/^  backend /p' <<<"$o")" \
       "NAT rewriting"
want_rc "removing a tag does not fail the run — it changes the reading" "$rc" eq 0

# ── 5. a tagged group goes red ───────────────────────────────────────────────
say "control 5 — a group that measures a transparency fails"
awk '/^hr "Python client target/{print; print "fail_total=$((fail_total+1)); echo \"  FAIL simulated\""; next} {print}' \
  "$WORK/body_all.sh" >"$WORK/body_red.sh"
build "$WORK/body_red.sh" "$WORK/d5.sh" || fails=$((fails+1))
o=$(bash "$WORK/d5.sh" 2>&1); rc=$?
want "the transparency shows red rather than the ledger swallowing it" "$o" \
     "measured by 1 group(s) in this run, 1 of them red" \
     "RED (1)  Python client target"
want "the group keeps its own verdict" "$o" "1 check group(s) failed"
want_rc "a red tagged group fails the run" "$rc" ne 0

# ── 6. a tagged group SKIPPED ────────────────────────────────────────────────
say "control 6 — a group that measures a transparency is skipped"
awk '/^hr "object-reference acquisition/{print; print "skip absent \"\" \"omniNames is not installed — naming is unmeasured, not passing\""; next} {print}' \
  "$WORK/body_all.sh" >"$WORK/body_skip.sh"
build "$WORK/body_skip.sh" "$WORK/d6.sh" || fails=$((fails+1))
o=$(bash "$WORK/d6.sh" 2>&1); rc=$?
want "the unmeasured column names the group, in the group's own words" "$o" \
     "unmeasured: object-reference acquisition" \
     "SKIPPED (absent): omniNames is not installed"
want "the skip still counts, exactly as D010 §2 requires" "$o" \
     "check group(s) SKIPPED — those claims are unmeasured, not passing"
want_rc "a skip does not fail the run" "$rc" eq 0

# ── 7. §6.1 unreadable ───────────────────────────────────────────────────────
say "control 7 — D029 §6.1 changes shape under the run"
# A whole small tree rather than a back door: transparency.py resolves the
# decision relative to its OWN location, so a copy beside a mangled copy of the
# decision is enough. Giving the reader an environment override would have made
# "one home" configurable, which is the property being defended.
mkdir -p "$WORK/tree/spikes" "$WORK/tree/docs/decisions"
cp "$ROOT/spikes/transparency.py" "$WORK/tree/spikes/"
grep -v '^| \*\*Backend\*\*' \
  "$ROOT/docs/decisions/D029-what-a-complete-orb-would-mean.md" \
  >"$WORK/tree/docs/decisions/D029-what-a-complete-orb-would-mean.md"
build "$WORK/body_all.sh" "$WORK/d7.sh" "$WORK/tree" || fails=$((fails+1))
o=$(bash "$WORK/d7.sh" 2>&1); rc=$?
want "the ledger refuses rather than printing four transparencies" "$o" \
     "the five transparency names could not be read from" \
     "That is an unmeasured criterion, not a pass." \
     "holds 4 transparency row(s), not the five"
want "every tag becomes an unvalidated claim, which is a failure" "$o" \
     "so this group's claim is unvalidated"
want "the verdict says the criterion was not read" "$o" \
     "transparency: NOT READ"
want_rc "an unreadable §6.1 fails the run" "$rc" ne 0

say "ledger controls"
echo "  $checks assertion group(s), $fails failed"
if [ "$fails" -eq 0 ]; then
  echo "  every way this ledger could be green while measuring nothing was tried"
fi
exit "$fails"
