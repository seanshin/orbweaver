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
# WHY IT IS A SCRIPT AND NOT FIVE HARNESS GROUPS. It would be five harness
# groups if this batch owned `run_checks.sh`, and it does not — that file is
# held. The wiring is one group per transparency and is written out at the
# bottom of this file, ready to paste. Until then the Rust half is still gated:
# `cargo test --workspace` is the harness's first group and it runs these tests.
# The SKIPPED half is NOT gated and is not counted by the harness's verdict,
# which is this script's own honest limit and is printed in its verdict.
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
  local label="$1"; shift
  local out rc
  out=$(cargo test -q -p orbweaver-test --test what_a_caller_can_tell -- "$@" 2>&1); rc=$?
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
  grep -E '^(thread |---- )|THE CALLER' <<<"$out" | head -8 | cut -c1-160 | sed 's/^/       /'
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
    emit "$name" SKIPPED "a POA-level activation path that reloads an evicted target"
    skip "no test evicts a target under a live caller, because an evicted" \
      "target cannot answer yet." \
      "waits on: a POA-level activation path — an evicted object being" \
      "reloaded on demand so that evict-then-invoke ANSWERS. Until then a" \
      "test asserting the caller cannot tell would be asserting the leak:" \
      "dialling an OFFLOADED expert answers OBJECT_NOT_EXIST, and" \
      "expert_service.rs records that as intended, the caller's cue to" \
      "prefetch. NOT skipped for want of machinery — residency.rs has" \
      "register/forget/pin/unpin/status and Offer::residency is live in the" \
      "store Router::select reads. Those are driven FROM THE CONTROL PLANE," \
      "which is a layer allowed to know load state, and a test written from" \
      "there would measure the property from a layer permitted to see it." \
      "D031's first ledger declined to tag those spikes for that reason and" \
      "that decline is the standard this line keeps."
    ;;

  lifecycle)
    emit "$name" SKIPPED "a redirect emitted for a NAME rather than for an object"
    skip "no test removes a target under a live caller and finds the caller" \
      "unable to tell, because today it can tell immediately." \
      "waits on: a second endpoint and a redirect for a NAME. O1 landed, so" \
      "removal has an implementation and a test — Orb::shutdown, measured" \
      "from a peer's own socket — but the TRANSPARENCY of the removal did" \
      "not move: a caller of a removed server has nowhere else for its" \
      "request to go. LOCATION_FORWARD is served for objects and nothing" \
      "emits it for a name, and nothing re-publishes, so a moved target" \
      "leaves its name pointing at a dead address. Note what IS measured" \
      "under location above: a target moving under a live caller is" \
      "invisible. Moving is the half of this row that works; removing is not."
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
echo "  This script's own limit: the SKIPPED count above is not counted by"
echo "  run_checks.sh, because wiring these in means editing that file and this"
echo "  batch does not own it. The groups to paste, one per transparency:"
echo ""
echo "      hr \"leak test — a move under a live caller (D029 §5 O0)\""
echo "      bears_on location"
echo "      ...  ./spikes/leak_tests.sh --raw, one group per row"
echo ""
echo "  Until then the two measured legs ARE gated — cargo test --workspace is"
echo "  the harness's first group and runs them — and the three SKIPPED are not."
exit $(( fails > 0 ? 1 : 0 ))
