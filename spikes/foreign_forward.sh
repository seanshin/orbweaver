#!/usr/bin/env bash
# foreign_forward.sh — a FOREIGN ORB forwards our client, and our client lands
# somewhere else and completes the call there.
#
# D029 §6.1's location row is the best-measured of the five and it was measured
# in one direction only. This ORB sends `LOCATION_FORWARD` and `LOCATE_FORWARD`
# in both byte orders across GIOP 1.0/1.1/1.2, and it follows a forward — but
# every forward it had ever followed was one it had written itself. CLAUDE.md
# names that shape and says what it is worth:
#
#   A convention both ends apply cannot be refuted by a round trip, and a
#   convention one end applies on read can hide the other end's defect on
#   write; twelve wire changes in v0.5.0 were found this way and none by a test
#   we could have written from the specification alone.
#
# So the peer here is omniORB, made to forward by ITS OWN mechanism — a POA with
# USE_SERVANT_MANAGER + NON_RETAIN whose `ServantLocator.preinvoke` raises
# `PortableServer.ForwardRequest` — pointing at a SECOND omniORB process at a
# different ephemeral port. Nothing in this repository encodes the reply.
#
#   ./spikes/foreign_forward.sh [--break no-forward|forward-to-self] [--keep]
#
# Two halves, and they are deliberately not the same measurement:
#
#   1. spikes/foreign_forward_capture.py — imports no ORB, builds its own GIOP
#      requests, and reads the reply out of the octets. Six probes: three GIOP
#      versions x two byte orders. This is the provenance half: it says what
#      omniORB actually put on the wire, with the byte order taken OFF the flag
#      byte in both places it appears (the reply message's, and the forwarded
#      IIOP profile's own encapsulation flag, which is independent of it).
#   2. crates/orbweaver-giop/tests/foreign_forward.rs — OUR client, dialling the
#      same forwarder, which must follow the forward and complete the call at
#      the destination. Seven cases.
#
# The first without the second would say a foreign ORB forwards and never that
# we can follow it. The second without the first would say a call completed and
# never that a foreign ORB was what redirected it. Neither alone buys the half
# that was missing.
#
# Exit: 0 every check green; 1 any check failed or could not be measured;
#       2 the fixture is absent (a counted SKIPPED naming it, never an ok).
#
# NEGATIVE CONTROLS (D010 §7.2). `--break` removes the thing being measured and
# leaves everything else alone; the run must go RED, with the failure counter
# moving, not merely print a different line:
#
#   --break no-forward       the forwarder serves in place at the same address
#                            and emits no forward at all
#   --break forward-to-self  a well-formed LOCATION_FORWARD naming the address
#                            it was sent to — a forward that is not a move,
#                            which is the case "a forward came back" cannot see
#
# No harness lock is taken. Every port is ephemeral, every fixture is killed by
# PID, and nothing is written to a fixed /tmp path, so a concurrent
# run_checks.sh cannot collide with this and it cannot collide with one — the
# same argument spikes/perm_fallback.sh makes for itself.
#
# TEST FIXTURE ONLY. omniORB is LGPL/GPL, is never imported, linked or shipped,
# and is reached here only as a separate process over TCP (clause (a) of the
# licensing boundary).
#
# *외부 ORB가 우리 클라이언트를 다른 주소로 넘기고, 우리가 그곳에서 호출을 끝낸다.
# 우리가 만들어 우리가 따라간 포워드는 왕복으로 반박될 수 없다 — 빠진 절반을 산다.*
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

break_it=""
keep=""
while [ $# -gt 0 ]; do
  case "$1" in
    --break) break_it="$2"; shift 2 ;;
    --keep) keep=1; shift ;;
    -h|--help) sed -n '2,60p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$break_it" in
  ""|no-forward|forward-to-self) ;;
  *) echo "--break must be no-forward or forward-to-self, got '$break_it'" >&2; exit 2 ;;
esac

fails=0
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }

# ── the fixture, and its absence ────────────────────────────────────────────
#
# D010 §2: a fixture that is not here is a counted SKIPPED naming it, never a
# note and never an ok. Exit 2 is that signal to the caller; the harness group
# turns it into `skip absent`.
command -v python3 >/dev/null 2>&1 || {
  echo "  SKIPPED  python3 is not installed — the foreign-forward leg is"
  echo "           unmeasured, not passing"
  exit 2
}
if ! python3 -c 'import omniORB, PortableServer' >/dev/null 2>&1; then
  echo "  SKIPPED  omniORB's Python bindings are not importable (fixture:"
  echo "           spikes/foreign_forward_peer.py, which needs omniORBpy for"
  echo "           the ServantLocator that raises ForwardRequest) — whether a"
  echo "           FOREIGN ORB can forward this client is unmeasured, not passing"
  exit 2
fi
command -v cargo >/dev/null 2>&1 || {
  echo "  SKIPPED  cargo is not installed — our client cannot be built, so the"
  echo "           following half is unmeasured, not passing"
  exit 2
}

work=$(mktemp -d "${TMPDIR:-/tmp}/fwdfgn-XXXXXX")
pids=()
cleanup() {
  for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null; done
  for p in "${pids[@]:-}"; do [ -n "$p" ] && wait "$p" 2>/dev/null; done
  if [ -n "$keep" ]; then echo "kept: $work"; else rm -rf "$work"; fi
}
trap cleanup EXIT

# Waits — SLEEPING — for a peer's `READY <host> <port>` line, up to $2 seconds.
# CLAUDE.md's first harness rule: a wait loop that does not sleep does not
# wait, and the protocol looks broken when the harness was.
#
# The READY line is printed by the peer only after it has itself connected to
# the endpoint it published, so a client that starts when this returns is not
# racing the listener — the macOS-loopback accept miss (~5% measured) is waited
# out on the peer's side rather than papered over with a settle sleep here.
wait_ready() {
  local log="$1" secs="${2:-25}" i line
  for _ in $(seq 1 $((secs * 10))); do
    if [ -s "$log" ]; then
      line=$(grep '^READY ' "$log" 2>/dev/null | head -1)
      [ -n "$line" ] && { echo "$line"; return 0; }
    fi
    sleep 0.1
  done
  return 1
}

start_peer() {
  # start_peer <name> <log> <args...>  ->  echoes "<host> <port>"
  local name="$1" log="$2"; shift 2
  ( exec python3 "$ROOT/spikes/foreign_forward_peer.py" "$@" >"$log" 2>&1 & echo $! >"$log.pid" )
  sleep 0.1
  local pid; pid=$(cat "$log.pid" 2>/dev/null)
  [ -n "$pid" ] && pids+=("$pid")
  local ready
  if ! ready=$(wait_ready "$log"); then
    fail "the $name peer did not become ready within 25s — the leg is unmeasured, which is a failure"
    sed 's/^/       /' "$log" | tail -8
    return 1
  fi
  # shellcheck disable=SC2086
  set -- $ready
  echo "$2 $3"
}

printf '\n\033[1mforeign forward — omniORB redirects our client to another address\033[0m\n'
[ -n "$break_it" ] && printf '  (negative control: --break %s)\n' "$break_it"

dest_ior="$work/dest.ior"
fwd_ior="$work/fwd.ior"

if ! dest_at=$(start_peer destination "$work/dest.log" \
      --role dest --out-ior "$dest_ior" --tag dest); then
  echo "  $fails check(s) failed"; exit 1
fi
dest_host=${dest_at% *}; dest_port=${dest_at#* }

fwd_args=(--role forward --out-ior "$fwd_ior" --target-ior "$dest_ior" --tag fwd)
[ -n "$break_it" ] && fwd_args+=(--break "$break_it")
if ! fwd_at=$(start_peer forwarder "$work/fwd.log" "${fwd_args[@]}"); then
  echo "  $fails check(s) failed"; exit 1
fi
fwd_host=${fwd_at% *}; fwd_port=${fwd_at#* }

if [ "$fwd_port" = "$dest_port" ]; then
  fail "both peers published port $fwd_port; this leg measures a move to a DIFFERENT address and cannot"
  echo "  $fails check(s) failed"; exit 1
fi
echo "  ..   forwarder $fwd_host:$fwd_port  destination $dest_host:$dest_port  (foreign, two processes)"

# ── half 1: what omniORB actually put on the wire ───────────────────────────
for minor in 0 1 2; do
  for order in little big; do
    cap=$(python3 "$ROOT/spikes/foreign_forward_capture.py" \
            --ior "$fwd_ior" --minor "$minor" --order "$order" \
            --expect-port "$dest_port" 2>&1)
    cap_rc=$?
    # The producer's own exit status is read FIRST. A probe that could not run
    # at all is an unmeasured check, which is a failure and never a pass —
    # and it is never inferred from whether some string is in its output.
    if [ "$cap_rc" -eq 0 ]; then
      # Both orders are reported as OBSERVED, off the flag byte, beside what we
      # chose to send. This project has a measured instance of a probe that
      # reported an order it had assumed.
      robs=$(grep '^reply_order=' <<<"$cap" | head -1); robs=${robs#reply_order=}
      pobs=$(grep '^profile_order=' <<<"$cap" | head -1)
      pobs=${pobs#profile_order=}; pobs=${pobs%% *}
      pass "GIOP 1.$minor sent=$order -> LOCATION_FORWARD (status 3) to :$dest_port; reply order observed=$robs, forwarded profile order observed=$pobs"
    elif [ "$cap_rc" -eq 3 ]; then
      fail "GIOP 1.$minor sent=$order: nothing could be measured — $(grep '^reason=' <<<"$cap" | head -1)"
    else
      fail "GIOP 1.$minor sent=$order: $(grep '^reason=' <<<"$cap" | head -1)"
      sed 's/^/       /' <<<"$cap" | head -12
    fi
  done
done

# ── the recording, re-taken ─────────────────────────────────────────────────
#
# crates/orbweaver-giop/tests/foreign_forward_bytes.rs holds three replies
# omniORB wrote on a named day, and is the gate that fires where omniORB is not
# installed. A recording nobody re-takes is a claim about the past, so this
# regenerates them from the live fixture and compares the decoded values.
#
# Skipped under --break for the obvious reason: a control that removes the
# forward makes the re-take fail for a reason that is not drift, and counting
# it would inflate the control's own count with a finding it did not make.
if [ -z "$break_it" ]; then
  rec=$(python3 "$ROOT/spikes/foreign_forward_capture.py" \
          --ior "$fwd_ior" --check-recording 2>&1)
  rec_rc=$?
  sed 's/^/  /' <<<"$(grep -E '^  (ok|FAIL|SKIPPED)' <<<"$rec")"
  if [ "$rec_rc" -eq 2 ]; then
    fail "the recording could not be re-taken: $(grep '^reason=' <<<"$rec" | head -1)"
  elif [ "$rec_rc" -ne 0 ]; then
    fail "the recorded omniORB replies no longer describe the live peer"
  fi
fi

# ── half 2: our client, following it ────────────────────────────────────────
#
# `--ignored` because these cases need the two live foreign processes; the test
# file panics on a missing variable rather than skipping, so a fixture that is
# not here cannot make that file look green under a bare `cargo test`.
rust=$(OW_FOREIGN_FORWARD_IOR="$fwd_ior" \
       OW_FOREIGN_FORWARD_DEST_HOST="$dest_host" \
       OW_FOREIGN_FORWARD_DEST_PORT="$dest_port" \
       cargo test -q -p orbweaver-giop --test foreign_forward -- \
         --ignored --nocapture --test-threads=1 2>&1)
rust_rc=$?

# What the leg covers, pinned as EQUALITIES and not as floors.
#
# `cargo test` exits 0 for a run that executed nothing — a renamed case, a stale
# filter, an `#[ignore]` that stopped being lifted all print `0 passed` and
# succeed. Reading only the exit status would make this half green on precisely
# the day it stopped measuring, which is the class CLAUDE.md has five instances
# of. So the count is asserted, and asserted as `-ne` rather than `-lt`: a floor
# proves nothing about the figure, and here the figure IS the coverage claim —
# three GIOP versions x two byte orders, plus the re-dial case.
EXPECT_TESTS=7
EXPECT_ORDER_CELLS=6

# Not anchored at ^: libtest writes its per-test progress dot on the same line,
# so six of the eight cells arrive as `.cell giop=...`. The first draft of this
# line anchored the match, counted 2 of 8, and still printed `ok` — the verdict
# was sound and the number beside it was not.
cells=$(grep 'cell ' <<<"$rust" | sed 's/^\.*//')
order_cells=$(grep -c 'cell giop=' <<<"$rust")
passed=$(grep -oE 'test result: ok\. [0-9]+ passed' <<<"$rust" | grep -oE '[0-9]+' | head -1)
passed=${passed:-0}

if [ "$rust_rc" -ne 0 ]; then
  fail "our client did not follow omniORB's forward to a completed call at :$dest_port"
  # `-A1` on the panic line, because libtest puts the assertion's own words on
  # the line AFTER `panicked at <file>:<line>`. The first draft matched only
  # the `panicked` lines and printed seven file:line references with not one
  # word of why — a diagnostic that names where and never what, which is the
  # thing a reader is going to need at 2am.
  sed 's/^/       /' <<<"$(grep -A1 -E 'panicked at' <<<"$rust" | head -14)"
  sed 's/^/       /' <<<"$(grep -E 'test result:' <<<"$rust" | head -2)"
elif [ "$passed" -ne "$EXPECT_TESTS" ] || [ "$order_cells" -ne "$EXPECT_ORDER_CELLS" ]; then
  fail "the client half exited 0 having measured $passed/$EXPECT_TESTS case(s) and $order_cells/$EXPECT_ORDER_CELLS order cell(s) — an unmeasured check is a failure, never a pass"
  sed 's/^/       /' <<<"$(grep -E 'test result:|filtered out' <<<"$rust" | head -4)"
else
  pass "our client followed the foreign forward and completed the call at :$dest_port — $passed case(s), $order_cells order cell(s)"
  sed 's/^/       /' <<<"$cells"
fi

# The forwarder's own log is PRINTED, never counted. What a peer believes it
# answered is not what the client received; the verdict above comes from the
# client and from the octets, and this is here so a reader can see the two
# accounts side by side (D034 §5.1).
fwd_says=$(grep -c 'preinvoke' "$work/fwd.log" 2>/dev/null)
echo "  ..   the forwarder's own log says it forwarded $fwd_says time(s) — printed, not counted"

printf '\n'
if [ "$fails" -eq 0 ]; then
  echo "  all measured checks green"
  exit 0
fi
echo "  $fails check(s) failed"
exit 1
