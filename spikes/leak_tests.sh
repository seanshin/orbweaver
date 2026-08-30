#!/usr/bin/env bash
# D029 §5 O0 — one leak test per transparency, and a counted SKIPPED for every
# transparency that does not have one yet.
#
# WHAT THIS IS FOR. D031's ledger reads the harness run and reports, per
# transparency, how many groups measured it. Its author wrote the limit into
# `run_checks.sh` in the same commit: *"nothing here CHANGES a hidden property
# under a live caller — every group this ledger counts was written for another
# reason and is being re-read."* This script is the other kind. Every `ok` below
# comes from a test that held a live caller while the property changed
# underneath it; every `SKIPPED` names the specific thing its test waits on.
#
# WHY IT IS A SCRIPT AND NOT FIVE HARNESS GROUPS. It began as one because the
# batch that wrote it did not own `run_checks.sh`. **The wiring has since
# landed** — the harness has one group per transparency, each calling
# `leak_leg <name>`, which reads the `--raw` rows below — so the SKIPPED half is
# gated now and this paragraph's old "until then" is gone. One consequence is
# live: `leak_leg` FAILS a MEASURED row whose group still carries the static
# `tp_measures_nothing` declaration it was given while its leg was a skip, so a
# leg that starts measuring cannot be swallowed by a stale declaration.
#
# **The activation instance of that was paid and this file kept billing for it.**
# Until 2026-08-27 the paragraph here, the verdict below, and
# `docs/COMPONENTS.md` all said one line was still owed in `run_checks.sh`, and
# all three cited **line 4318** — for a group that had moved to 4792 and whose
# declaration had already been deleted, with a comment in its place saying why.
# The line number is the tell: a debt that names a location nobody re-checked is
# a debt nobody re-checked. A record that outlives its fact is the defect this
# project measures, and it is cheaper to find when the record carries a number.
#
# THE FIVE NAMES ARE NOT WRITTEN HERE. `spikes/transparency.py` reads them from
# D029 §6.1, which owns them. A name that arrives without a handler is a
# FAILURE, not a skip: a transparency renamed or added in §6.1 must not fall
# out of this instrument silently. That is the `dk_peer` lesson — check the
# expected table against the owner before any leg runs.
#
# Exit code is the verdict. Takes no lock, starts no fixture beyond the
# in-process servers the Rust tests bind on 127.0.0.1:0.
#
# Usage: ./spikes/leak_tests.sh [--raw]
#   --raw   one TSV row per transparency: name<TAB>verdict<TAB>detail

set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
RAW=0
[ "${1:-}" = "--raw" ] && RAW=1

TESTS=crates/orbweaver-test/tests/what_a_caller_can_tell.rs
fails=0
skipped=0
measured=0

emit() { [ "$RAW" = 1 ] && printf '%s\t%s\t%s\n' "$1" "$2" "$3"; }
hdr()  { [ "$RAW" = 1 ] || printf '\n\033[1m%s — %s\033[0m\n' "$1" "$2"; }
say()  { [ "$RAW" = 1 ] || echo "$1"; }

NAMES=$(python3 spikes/transparency.py --names 2>&1)
if [ $? -ne 0 ]; then
  echo "FAIL the five transparency names could not be read from their owner, so this"
  echo "     run cannot say which one each test bears on. That is an unmeasured"
  echo "     criterion, not a pass."
  sed 's/^/     /' <<<"$NAMES"
  exit 2
fi

# run_tests <label> <test-name>...
# Green only when the named tests all pass AND cargo itself ran.
run_tests() {
  run_tests_in orbweaver-test what_a_caller_can_tell "$@"
}

# run_tests_in <pkg> <test-target> <label> <test-name>...
# The same check against a leak test that lives in another crate. The
# activation leg's test is in `orbweaver-object`, because the property it
# changes under the caller — a POA's activation set — is that crate's, and a
# test in `orbweaver-test` would have to reach through a published surface
# that does not exist rather than evict the thing directly.
run_tests_in() {
  local pkg="$1"; shift
  local target="$1"; shift
  local label="$1"; shift
  local out rc
  out=$(cargo test -q -p "$pkg" --test "$target" -- "$@" 2>&1); rc=$?
  local line
  line=$(grep -E '^test result:' <<<"$out" | head -1)
  if [ "$rc" -eq 0 ] && [ -n "$line" ]; then
    say "  ok   $label"
    say "       $line"
    return 0
  fi
  say "  FAIL $label"
  # The producer's own exit status first: a cargo that could not run at all is
  # an unmeasured check, which is a failure and never a pass.
  say "       cargo exited $rc${line:+, $line}"
  # The head is the point: a red that does not say what the caller could tell
  # is a diagnosis nobody can act on. `THE CALLER` is matched anywhere on the
  # line because an `assert_eq!` message arrives after `assertion ... failed:`.
  # `--raw`'s contract is one TSV row per transparency and nothing else. This
  # extract used to be written to stdout unconditionally, so the TSV was well
  # formed **exactly when nothing was wrong** — a consumer counting rows read
  # eight on the one run where a leak had been found. It goes through `say`
  # like every other line now.
  #
  # No `head` on the pipe either: an early-exit form SIGPIPEs the `grep` that
  # feeds it, which under `pipefail` is the shape this project has been bitten
  # by twice. Capture, then trim with a herestring.
  if [ "$RAW" != 1 ]; then
    local why
    why=$(grep -E '^(thread |---- )|THE CALLER' <<<"$out" || true)
    [ -n "$why" ] && sed -n '1,8p' <<<"$why" | cut -c1-160 | sed 's/^/       /'
  fi
  return 1
}

# skip <what-it-waits-on-lines...>
skip() {
  skipped=$((skipped+1))
  say "  SKIPPED  $1"; shift
  local l
  for l in "$@"; do say "           $l"; done
}

for name in $NAMES; do
  title=$(python3 spikes/transparency.py --title "$name" 2>/dev/null)
  tell=$(python3 spikes/transparency.py --tell "$name" 2>/dev/null)
  hdr "$name" "the caller must not be able to tell $tell"

  case "$name" in

  location)
    if run_tests "a move under a live caller changed nothing the caller observed" \
         a_move_under_a_live_caller_is_invisible limits_survive_a_move; then
      measured=$((measured+1)); emit "$name" MEASURED "a move under a live caller"
    else
      fails=$((fails+1)); emit "$name" RED "a move under a live caller"
    fi
    say "       unmeasured: one process and loopback — the caller is our own"
    say "                   Connection, so a leak only omniORB's or JacORB's client"
    say "                   would see is invisible here. $TESTS names the rest."
    ;;

  backend)
    if run_tests "the implementation behind one reference was replaced mid-session" \
         backend_swapped_under_a_live_caller; then
      measured=$((measured+1)); emit "$name" MEASURED "a servant swap under a live caller"
    else
      fails=$((fails+1)); emit "$name" RED "a servant swap under a live caller"
    fi
    say "       unmeasured: two implementations, not N. A third that agreed with"
    say "                   neither is not measured by any number of runs."
    ;;

  language)
    # This leg was a counted SKIPPED from the day it was written until
    # 2026-08-30, and it named its own blocker rather than leaving it to be
    # guessed at: the only route to a Python servant was
    # `orbweaver-py-bridge --serve`, **which binds its own listener**, so the
    # Python side arrived as an ENDPOINT and a language swap became an address
    # swap. A caller made to dial elsewhere has been *moved*, which is the
    # location row — a test built that way would have measured the wrong row
    # and been green while it did.
    #
    # `orbweaver_gen::pychild::PythonChild` closed that: python3 as a child of
    # the test's own process, wrapped by `seam::ForeignServant` into a plain
    # `Dispatch`. Both implementations now sit behind ONE server, ONE reference
    # and ONE open connection.
    if run_tests_in orbweaver-gen what_a_caller_can_tell_about_a_language \
         "a language swapped under a live caller: one reference, one connection" \
         a_language_swapped_under_a_live_caller_is_invisible \
         the_two_languages_are_two_implementations_and_not_one; then
      measured=$((measured+1)); emit "$name" MEASURED "Rust and Python behind one reference"
    else
      fails=$((fails+1)); emit "$name" RED "Rust and Python behind one reference"
    fi
    say "       control:    ORBWEAVER_LEAK_CONTROL=language — the Python half"
    say "                   answers a different number, which is what a caller"
    say "                   would see if the language behind a reference were"
    say "                   observable. RUN BY spikes/leak_controls.sh, not by"
    say "                   this sentence."
    say "       unmeasured: two languages, not N — and one OPERATION. A pair"
    say "                   that agreed on \`count\` and diverged on a \`wstring\`"
    say "                   is not measured by any number of runs of this."
    say "                   \`python_servant.rs\` is the wide comparison (19"
    say "                   calls x 3 versions x 2 orders) and has NO live"
    say "                   caller; this has the live caller and the narrow"
    say "                   pair. Neither is the other."
    ;;

  activation)
    # The blocker this leg named until 2026-08-26 — "a POA-level activation
    # path, an evicted object reloaded on demand so that evict-then-invoke
    # ANSWERS" — is `MissPolicy::Activate`, and it landed. The leg's other
    # sentence, that the leak is `Router::select` and that a test written from
    # the control plane would measure it from a layer permitted to see it, was
    # answered rather than worked around: `select` is a contract, the leak is
    # in what the REFERENCE does across an eviction, and the caller below
    # holds nothing but one.
    if run_tests_in orbweaver-object what_a_caller_can_tell_about_load \
         "an eviction under a live caller changed nothing the caller observed" \
         a_caller_cannot_tell_an_evicted_expert_from_a_resident_one \
         the_second_call_was_served_by_a_demand_load \
         an_unregistered_id_is_still_unknown_under_every_policy; then
      measured=$((measured+1)); emit "$name" MEASURED "an eviction under a live caller"
    else
      fails=$((fails+1)); emit "$name" RED "an eviction under a live caller"
    fi
    say "       control:    in the tree, not in a commit message —"
    say "                   the_refusing_miss_policies_are_the_leak runs the same"
    say "                   scenario under MissPolicy::Refuse and ::RefuseAndPrefetch"
    say "                   and REQUIRES it to fail naming OBJECT_NOT_EXIST, so a"
    say "                   green leg is evidence about a leak rather than about a"
    say "                   switch that has stopped working."
    say "       unmeasured: TIME. A demand-loaded call is slower than a resident"
    say "                   one and a caller with a clock can tell. In this"
    say "                   repository a load is two map writes, so the latency"
    say "                   MissPolicy's refusal is about is real in a deployment"
    say "                   and absent here. This leg compares BYTES."
    say "       unmeasured: the miss policy a DEPLOYMENT chooses. Nothing in the"
    say "                   tree mounts ExpertLocator on a served POA, so which"
    say "                   variant production would run is undecided, and a"
    say "                   deployment on either refusing variant leaks."
    ;;

  lifecycle)
    # The blocker this leg named until 2026-08-27 was **decision X**, and X was
    # answered: D035 was approved with the owner's answer to D029's required
    # question — *displacement is not closure* — so the row is no longer waiting
    # on a decision that could not reach zero. What landed is D035 §5's option
    # B: the bootstrap leak is recorded as an irreducible floor of a single-node
    # deployment, and everything ABOVE that floor is measured.
    #
    # Two things the first draft of that test got wrong, kept here because they
    # are what makes the leg's claim the one it is:
    #
    #   * "a caller cannot tell WHICH target was removed" is empty — the caller
    #     chose which reference to dial. The real property is ISOLATION:
    #     removing one target must be invisible to a caller of another.
    #   * a removed target still answered on an already-open connection. That
    #     is not a bug, it is D034's graceful shutdown at request granularity.
    #     The floor is therefore observed by DIALLING AGAIN, which is what a
    #     caller holding only a reference does.
    if run_tests_in orbweaver-giop what_a_caller_can_tell_about_a_removal \
         "a removal under a live caller: the floor named, and isolation measured above it" \
         removing_one_target_is_invisible_to_a_caller_of_another \
         a_caller_of_a_removed_target_can_tell_it_is_gone_and_that_is_the_floor \
         the_two_targets_could_be_told_apart_while_they_were_alive; then
      measured=$((measured+1)); emit "$name" MEASURED "removal isolation, over a named bootstrap floor"
    else
      fails=$((fails+1)); emit "$name" RED "removal isolation, over a named bootstrap floor"
    fi
    say "       floor:      a caller of a removed target CAN tell it is gone, and"
    say "                   nothing in one process changes that — it must be given"
    say "                   one address to send a first packet to. D035 §4 calls"
    say "                   this displacement rather than closure and the owner"
    say "                   approved NAMING it. The leg asserts the floor rather"
    say "                   than prose, so a change that made it stop being true"
    say "                   cannot pass unnoticed."
    say "       control:    ORBWEAVER_LEAK_CONTROL=removal_isolation — A's removal"
    say "                   takes B down with it, the shape a shared pool or a"
    say "                   process-wide stop produces. The HELD connection stays"
    say "                   green under that leak; only the redial catches it,"
    say "                   which is why the leg has both."
    say "                   RUN BY spikes/leak_controls.sh, not by this sentence."
    say "                   It said \"Run 2026-08-27: ...\" here for a day, which is"
    say "                   a control run once by hand and recorded in prose — the"
    say "                   thing leak_controls.sh exists to stop being how a"
    say "                   control is kept."
    say "       unmeasured: a target removed by being KILLED rather than stopped."
    say "                   Orb::shutdown says GIOP 9.4.10's goodbye; a killed"
    say "                   process leaves a reset, and a caller can tell those"
    say "                   apart. That is a second floor, named rather than"
    say "                   measured, because this repository stops ORBs and does"
    say "                   not kill them."
    ;;

  *)
    # A name §6.1 grew or renamed. Louder than a skip on purpose.
    fails=$((fails+1))
    emit "$name" NOHANDLER "no leg in leak_tests.sh"
    say "  FAIL \"$name\" is one of the transparencies its owner names and this"
    say "       script has no leg for it — neither a test nor a reason. A"
    say "       transparency must not fall out of this instrument silently."
    ;;
  esac
done

if [ "$RAW" = 1 ]; then
  exit $(( fails > 0 ? 1 : 0 ))
fi

printf '\n\033[1mverdict\033[0m\n'
echo "  $measured transparency(ies) measured by a test that changed the property"
echo "  under a live caller; $fails red; $skipped SKIPPED."
echo ""
echo "  The SKIPPED are the column a next batch is scoped from. They are claims"
echo "  that are UNMEASURED, not claims that passed."
echo ""
echo "  The wiring this footer used to say was missing LANDED: run_checks.sh has"
echo "  one group per transparency and leak_leg reads the rows above, so the"
echo "  SKIPPED are counted by the harness verdict."
echo ""
echo "  A leg that starts measuring while its group still declares"
echo "  \`tp_measures_nothing\` makes run_checks.sh FAIL that row deliberately,"
echo "  so the ledger cannot swallow a measurement. The activation instance of"
echo "  that was settled on 2026-08-27 — the declaration is gone and a comment"
echo "  stands where it was. This script billed for it until that day, citing a"
echo "  line number the group had long since moved away from."
exit $(( fails > 0 ? 1 : 0 ))
