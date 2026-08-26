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
# live and is printed in the verdict: `leak_leg` FAILS a MEASURED row whose
# group still carries the static `tp_measures_nothing` declaration it was given
# while its leg was a skip. The activation leg started measuring on 2026-08-26
# and its declaration is still there, so the harness is red until one line is
# deleted; see the verdict for which.
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
    emit "$name" SKIPPED "a Python servant mountable in a server the test owns"
    skip "no test changes the servant's LANGUAGE under a live caller." \
      "waits on: a Python servant that can be mounted as a \`Dispatch\` in a" \
      "server the caller's test owns. \`PyServant\` IS such a \`Dispatch\` and a" \
      "bilingual dispatcher holding it beside a Rust servant is a few lines —" \
      "that is NOT what this waits on. What it waits on is a real Python" \
      "process reachable from one: the only route today is" \
      "\`orbweaver-py-bridge --serve\`, which BINDS ITS OWN LISTENER, so the" \
      "Python servant arrives as an endpoint rather than as a servant and a" \
      "swap becomes a move. The alternative — this script speaking the seam's" \
      "JSON protocol to a python3 child of its own — is refused: that protocol" \
      "has one home (py_bridge's \`Parent\` and _rt's \`Bridge\`) and a second" \
      "implementation of it here is the very drift CLAUDE.md's one-home rule" \
      "forbids. The change is therefore in orbweaver-gen, which another batch" \
      "holds this wave: a serve path that hands back a \`Dispatch\` instead of" \
      "binding. Measured 2026-08-26." \
      "what exists instead: orbweaver-gen's python_servant.rs compares a" \
      "Python and a Rust servant over SEPARATE runs — 19 calls x 3 GIOP" \
      "versions x 2 orders, byte-identical. That measures that two servants" \
      "agree, which is not the same claim as a caller being unable to tell" \
      "them apart, because no caller was there when the language changed."
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
    emit "$name" SKIPPED "decision X: the reference Orb::server hands out is not indirect"
    skip "no test removes a target under a live caller and finds the caller" \
      "unable to tell, because today it can tell immediately." \
      "THE BLOCKER CHANGED on 2026-08-26 and this leg did not. It no longer" \
      "waits on a redirect emitted for a NAME: that is built and measured," \
      "crates/orbweaver-giop/tests/forward_for_a_name.rs — a servant holding" \
      "names and no objects, 7 tests, both byte orders, 3 negative controls." \
      "It waits on decision X, D029 6.1's lifecycle subsection: that the" \
      "reference Orb::server hands out is INDIRECT, its profile carrying a" \
      "name-resolving endpoint and a name rather than the servant's own" \
      "address. Until then a client holds the backend's own address, and no" \
      "redirect can reach it once that address is dead — a forward is a" \
      "reply and a reply needs a listener, so it can never be emitted by the" \
      "party that went away. O1 landed, so removal has an implementation and" \
      "a test — Orb::shutdown, measured from a peer's own socket — but the" \
      "TRANSPARENCY of the removal did not move. Note what IS measured under" \
      "location above: a target moving under a live caller is invisible." \
      "Moving is the half of this row that works; removing is not."
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
echo "  ONE EDIT IS OWED IN run_checks.sh AND THIS SCRIPT CANNOT MAKE IT."
echo "  The activation leg went from SKIPPED to MEASURED on 2026-08-26. Each"
echo "  leg's group carries a static \`tp_measures_nothing\` declaration while"
echo "  its leg is a skip, and leak_leg FAILS a MEASURED row whose group still"
echo "  declares it — deliberately, so a leg that starts measuring cannot be"
echo "  swallowed by a stale declaration. So run_checks.sh will report"
echo ""
echo "      FAIL this group declares tp_measures_nothing and the leak test for"
echo "           activation MEASURED (...) — the declaration is now understating"
echo "           the run; delete it so the ledger can count this leg"
echo ""
echo "  The fix is that message: delete the bare \`tp_measures_nothing\` line"
echo "  between \`bears_on activation\` and \`leak_leg activation\` (line 4318"
echo "  when this was written). The batch that closed the activation leak did"
echo "  not own run_checks.sh and left the red rather than leaving a SKIPPED"
echo "  that named a blocker it had removed."
exit $(( fails > 0 ? 1 : 0 ))
