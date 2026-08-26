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
    awk '/^hr "transparency ledger — what a caller can still tell/{p=1} p' "$RC"
  } >"$out"
  /bin/bash -n "$out" || { echo "  FAIL the lifted driver does not parse"; return 2; }
  # A driver that would run THIS SCRIPT is unbounded recursion, and it hangs
  # rather than failing — the one diagnostic nobody can read. Measured
  # 2026-08-26: the anchors below were title PREFIXES, a new harness group was
  # titled `transparency ledger — its own negative controls`, and the lift
  # swallowed it. The anchors are now full titles; this is the check that says so
  # if a future title collides anyway.
  # Comment lines are stripped first: this file's own name appears in a comment
  # inside the block being lifted, and a guard that fired on a comment would be
  # a guard nobody could keep. No early-exit form on either grep, so neither can
  # SIGPIPE the other and `pipefail` has no status to misread.
  local reenter
  reenter=$(grep -vE '^[[:space:]]*#' "$out" | grep -n -F -- 'ledger_control.sh')
  if [ -n "$reenter" ]; then
    echo "  FAIL the lifted driver would re-enter this script — a group title has"
    echo "       collided with one of the awk anchors above, and running it would"
    echo "       recurse without bound"
    sed 's/^/       /' <<<"$reenter"
    return 2
  fi
  return 0
}

# The harness's real groups and real tags, bodies replaced by an `ok`.
#
# `tp_measures_nothing` is lifted alongside `bears_on` and for the same reason:
# it is a DECLARATION about the group, not part of its body, and a group that
# declares a transparency while declaring it measured nothing must read the same
# way here as it does in a real run. That is why the harness writes it at column
# 0 beside `bears_on` rather than from inside the leg that learns the blocker —
# a declaration only a body could make would be invisible to every control below,
# and control 2 would then read `activation` as measured on the strength of a
# group whose own output says nobody has looked yet.
awk '/^hr "verdict"/{exit}
     /^hr "transparency ledger — what a caller can still tell/{exit}
     /^hr "/{print; print "echo \"  ok   (body not run in this control)\""; next}
     /^bears_on /{print}
     /^tp_measures_nothing$/ || /^tp_measures_nothing /{print}' "$RC" >"$WORK/body_all.sh"

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
# The property under test: *a group that declares a transparency and measures
# none of it must not flip the row to measured.* It is a property of the LEDGER
# and of nothing else, and until 2026-08-26 both this control and control 8
# demonstrated it by typing the name `activation`.
#
# That pin outlived its fact twice in one day. First activation went from
# UNDECLARED to declared-and-measuring-nothing when D029 §5 O0's leak tests
# landed as groups, and the assertion was edited to match. Then the activation
# leg started actually measuring, `tp_measures_nothing` came off it — and this
# control went RED while control 8 went VACUOUSLY GREEN. That is the pair's
# whole failure mode in one move: control 8 exists to prove this control is not
# tuned-until-quiet, and it proved it by asserting a flip that no longer had
# anything to flip. One broke loudly, the other in silence, and only the loud
# one would ever have been looked at.
#
# And the deeper reason it kept moving is that the property is about a GROUP —
# *this group measured none of what it declared* — while the only place the
# ledger makes it VISIBLE is a row, and a row reads UNMEASURED only when every
# group declaring that transparency is marked. `activation` satisfied that by
# arithmetic accident: it had exactly one declaring group. Today no transparency
# does — `language` and `lifecycle` still carry the two markers, and both are
# also declared by groups that measure them for real — so there is no live
# transparency left to point at, and pointing at one was never the right move.
#
# So the subject is SYNTHESISED. One group, one tag, one marker, no dependence
# on what the harness happens to be waiting on this week. `activation` is used
# as its tag purely because §6.1 has to know the name; nothing about the real
# activation groups is asserted, which is the whole difference.
want "the verdict names the measured half over the real tag set" "$o2" \
     "transparency measured this run:"
want_rc "a green run stays green" "$rc" eq 0
loc_before=$(grep -c '^                ok  ' <<<"$(sed -n '/^  location /,/^  backend /p' <<<"$o2")")

say "control 2b — one group declares a transparency and measures none of it"
SYNTH_TITLE="ledger_control's own group, which measures nothing"
# TWO groups, and the second one is load-bearing. With only the marked group the
# run measures nothing at all, so the verdict takes its `NONE measured in this
# run` branch — which is control 1's subject, not this one, and would have let
# this control assert something control 1 already proves while the half it
# actually cares about never printed. A companion group that measures a
# DIFFERENT transparency for real is what makes the verdict print both halves,
# which is the only place `activation` can be seen sitting in the unmeasured one.
{
  printf 'hr "%s"\n' "$SYNTH_TITLE"
  echo 'bears_on activation'
  echo 'tp_measures_nothing'
  echo 'echo "  ok   (body not run in this control)"'
  echo 'hr "ledger_control'"'"'s own group, which measures something"'
  echo 'bears_on location'
  echo 'echo "  ok   (body not run in this control)"'
} >"$WORK/body_synth.sh"
build "$WORK/body_synth.sh" "$WORK/d2b.sh" || fails=$((fails+1))
o2b=$(bash "$WORK/d2b.sh" 2>&1); rc=$?
want "the row its only group declared still reads UNMEASURED" "$o2b" \
     "UNMEASURED — 1 group(s) declare bears_on activation and not one"
syn_sec=$(awk '$0 ~ "^  activation +— " {p=1; print; next}
               p && /^  [a-z]+ +— / {p=0} p' <<<"$o2b")
reject "and its row does not claim a measurement" "$syn_sec" "measured by"
# The blocker TEXT is learned by a group's body, which this control replaces with
# an `echo`, so what is asserted is that the group reaches the load-bearing
# column at all and says it measured nothing — in its own title, not in a
# wording this file keeps a second copy of.
want "it reaches the load-bearing column in the group's own words" "$syn_sec" \
     "$SYNTH_TITLE" "measured nothing"
want "the verdict's unmeasured half names it" \
     "$(grep 'transparency UNMEASURED this run:' <<<"$o2b")" "activation"
want_rc "declaring and measuring nothing does not fail the run — it reports" "$rc" eq 0

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
# This assertion used to read `measured by 1 group(s) in this run, 1 of them
# red`, and it went FALSE on 2026-08-26 when the language row grew a second
# declaring group — the acceptance suite. Nothing noticed, because this script
# was not a harness group; it is one now. **A floor is not a figure and neither
# is a group count**: what this control is for is that a red tagged group is not
# swallowed by its transparency's row, so it asserts the red half and the named
# group, and says nothing about how many groups happen to declare `language`.
want "the transparency shows red rather than the ledger swallowing it" "$o" \
     "in this run, 1 of them red" \
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

# ── 8. the no-measure declaration is what holds the line ────────────────────
say "control 8 — a group declares a transparency and measures none of it"
# Control 2b asserts that a transparency whose only group measured nothing reads
# UNMEASURED. That assertion is only worth something if the row would read
# differently without the declaration — otherwise it is a check tuned until it
# is quiet, tested only against a tree with no defect in it. So: strip the
# `tp_measures_nothing` and require the row to FLIP to measured. The flip is the
# defect the declaration exists to prevent, and this is the run that shows it.
#
# **On 2b's synthetic body, and that is the repair.** This control spent
# 2026-08-26 stripping markers out of the REAL body and then asserting a flip on
# `activation`, which by then had no marker to strip — so every one of its
# assertions was already true before the strip, and it passed green without
# exercising anything at all. It was the vacuum this whole file exists to hunt,
# sitting inside the file. A control that strips a marker must assert on a body
# it knows had one.
grep -v '^tp_measures_nothing' "$WORK/body_synth.sh" >"$WORK/body_nomark.sh"
checks=$((checks+1))
if cmp -s "$WORK/body_synth.sh" "$WORK/body_nomark.sh"; then
  echo "  FAIL the strip removed nothing, so this control exercises nothing —"
  echo "       which is exactly how it passed on 2026-08-26"
  fails=$((fails+1))
else
  echo "  ok   the strip actually removed a declaration to strip"
fi
build "$WORK/body_nomark.sh" "$WORK/d8.sh" || fails=$((fails+1))
o=$(bash "$WORK/d8.sh" 2>&1); rc=$?
syn8=$(awk '$0 ~ "^  activation +— " {p=1; print; next}
            p && /^  [a-z]+ +— / {p=0} p' <<<"$o")
want "without the declaration the row flips to measured — the leak swallowed" \
     "$syn8" "measured by 1 group(s)"
reject "the UNMEASURED reading is gone without it" "$syn8" "UNMEASURED"
want "the verdict now claims it was measured" \
     "$(grep 'transparency measured this run:' <<<"$o")" "activation"
want_rc "swallowing a leak does not fail the run — which is exactly why the
       declaration has to be in the file rather than in a reviewer" "$rc" eq 0

say "ledger controls"
echo "  $checks assertion group(s), $fails failed"
if [ "$fails" -eq 0 ]; then
  echo "  every way this ledger could be green while measuring nothing was tried"
fi
exit "$fails"
