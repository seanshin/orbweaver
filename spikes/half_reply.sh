#!/usr/bin/env bash
# half_reply.sh — a peer that closes the connection between two writes of one
# reply, and what each caller multiplexed on that connection is told.
#
#   ./spikes/half_reply.sh
#
# Why this exists at all
#   `docs/decisions/D010` §4 B5 files this shape as class B — buildable, its
#   oracle absent — on the ground that "neither installed ORB will shut down
#   inside the window between two fragments on command". That is true of
#   omniORB and of JacORB, and it is the wrong conclusion: the peer this needs
#   is not an ORB. `spikes/half_reply_peer.py` is a socket that writes GIOP by
#   hand, in another language, in another process, out of the published wire
#   specification rather than out of the encoder under test. A socket needs
#   nothing that is missing on this machine, which is what distinguishes B5's
#   second half from every other class-B row.
#
# What is checked, per case
#   1. the caller whose reply had already begun hears `InterruptedMidReassembly`
#      naming *its own* request id, and is NOT told the call may be re-sent —
#      the peer demonstrably processed it, whatever §13.5.1 says about requests
#      without replies;
#   2. the other caller on the same connection hears `ConnectionClosed` and IS
#      told it may re-send, because §13.5.1 does describe a request that got
#      nothing back;
#   3. the id the client says was cut is the id the **peer** says it cut. Two
#      processes, separately, or the claim is the client agreeing with itself;
#   4. and with a §9.4.8 `MessageError` in place of the goodbye, neither caller
#      is freed — the negative arm, without which (2) could be "the other
#      caller is always free".
#
# Both byte orders, per CLAUDE.md, and the reply's order is chosen
# independently of the request's: GIOP sets it per message, and a peer that
# echoed the request would leave one of the two orders unmeasured on any one
# machine. Both cut positions, both fragment counts, and a window that is a
# knob rather than a race.
#
# The exit code is the verdict. Every probe here is an exit status; nothing is
# decided by grepping a marker out of a stream that could echo it.
#
# The in-process version of the same measurement is
# `crates/orbweaver-giop/tests/two_writes_of_one_reply.rs`, and it is the more
# thorough one — sixteen scripted peers with no external process at all. This
# script is what makes the bytes independent of our encoders.
#
# No harness lock is taken: every port is ephemeral, no fixed /tmp path is
# written, and nothing is killed by pattern, so a concurrent run_checks.sh
# cannot collide with this and it cannot collide with one.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

fails=0
cases=0
unmeasured=0

bold() { printf '\n\033[1m%s\033[0m\n' "$1"; }
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }
note() { printf '  ..   %s\n' "$1"; }

bold "B5 — a peer that closes between two writes of one reply"

if ! command -v cargo >/dev/null 2>&1; then
  fail "cargo is not on PATH, so nothing here was measured"
  echo; echo "half_reply: FAIL — 0 cases measured"; exit 1
fi
# An absent python3 is a FAIL and not a skip: this script's entire subject is
# the peer, so without it nothing was measured, and an unmeasured check is a
# failure.
if ! command -v python3 >/dev/null 2>&1; then
  fail "python3 is not on PATH, so the scripted peer could not run and nothing was measured"
  echo; echo "half_reply: FAIL — 0 cases measured"; exit 1
fi

if ! cargo build -q --bin spike-half-reply 2>&1; then
  fail "the driver did not build, so nothing was measured"
  echo; echo "half_reply: FAIL — 0 cases measured"; exit 1
fi
DRIVER="$ROOT/target/debug/spike-half-reply"
[ -x "$DRIVER" ] || { fail "no $DRIVER after a successful build"; exit 1; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/half-reply.XXXXXX")
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# ── one case ─────────────────────────────────────────────────────────────────
# $1 endian  $2 control  $3 cut index  $4 continuations  $5 window ms
one_case() {
  local endian="$1" control="$2" cut="$3" conts="$4" window="$5"
  local label="$endian reply, cut #$cut, $conts continuation(s), ${window}ms window, $control"
  cases=$((cases + 1))

  local port_file="$WORK/port.$cases" peer_out="$WORK/peer.$cases" peer_err="$WORK/peererr.$cases"
  python3 "$ROOT/spikes/half_reply_peer.py" \
    --port-file "$port_file" \
    --requests 2 --cut "$cut" \
    --reply-endian "$endian" \
    --continuations "$conts" \
    --window-ms "$window" \
    --control "$control" \
    --deadline-s 30 >"$peer_out" 2>"$peer_err" &
  local peer_pid=$!

  # A wait loop that sleeps, bounded by a deadline, and that gives up early if
  # the thing it is waiting for has died. A loop without the sleep does not
  # wait at all — that rule has its own line in CLAUDE.md because it produced a
  # phantom failure here once.
  local waited=0
  while [ ! -s "$port_file" ]; do
    if ! kill -0 "$peer_pid" 2>/dev/null; then
      fail "$label: the peer exited before publishing a port"
      sed 's/^/       | /' "$peer_err"
      return
    fi
    if [ "$waited" -ge 300 ]; then
      kill "$peer_pid" 2>/dev/null; wait "$peer_pid" 2>/dev/null
      fail "$label: the peer never published a port (15s)"
      return
    fi
    sleep 0.05
    waited=$((waited + 1))
  done
  local port
  port=$(cat "$port_file")

  local out status
  out=$("$DRIVER" --addr "127.0.0.1:$port" --cut "$cut" --control "$control" 2>&1)
  status=$?

  # Exit 3 is the driver saying it never reached the peer, so it has no account
  # of the interruption to be right or wrong about. Still a failure — an
  # unmeasured check is never a pass — but it must not be reported as the claim
  # having been refuted, which would point a false diagnosis at the code under
  # test. Seen once in ~450 cases here as a `Connection refused` against a peer
  # that had bound, listened and published its port; not diagnosed. Checked
  # before the peer is waited for, because a peer nobody connected to is still
  # sitting in `accept` and waiting for it would cost its whole deadline.
  if [ "$status" -eq 3 ]; then
    unmeasured=$((unmeasured + 1))
    fail "$label: UNMEASURED — the client never reached the peer"
    printf '%s\n' "$out" | sed 's/^/       | /'
    kill "$peer_pid" 2>/dev/null
    wait "$peer_pid" 2>/dev/null
    return
  fi

  wait "$peer_pid"
  local peer_status=$?

  if [ "$status" -ne 0 ]; then
    fail "$label: the client's account is wrong (exit $status)"
    printf '%s\n' "$out" | sed 's/^/       | /'
    return
  fi
  if [ "$peer_status" -ne 0 ]; then
    fail "$label: the peer's script did not run to the end (exit $peer_status)"
    sed 's/^/       | /' "$peer_err"
    return
  fi

  # The cross-check the client cannot do for itself. Captured into variables
  # and then matched — never piped into `grep -q`, which exits on first match
  # and SIGPIPEs whatever was producing the stream.
  local client_id peer_id
  client_id=$(printf '%s\n' "$out" | sed -n 's/.*cut_id=\([0-9][0-9]*\).*/\1/p' | head -1)
  peer_id=$(sed -n 's/.*"cut_id"[^0-9]*\([0-9][0-9]*\).*/\1/p' "$peer_out" | head -1)
  if [ -z "$client_id" ] || [ -z "$peer_id" ]; then
    fail "$label: one of the two processes did not name the call that was cut"
    printf '%s\n' "$out" | sed 's/^/       | /'
    sed 's/^/       | /' "$peer_out"
    return
  fi
  if [ "$client_id" != "$peer_id" ]; then
    fail "$label: the peer cut request $peer_id and the client was told about $client_id"
    return
  fi

  pass "$label (request $peer_id)"
}

note "the peer is spikes/half_reply_peer.py — stdlib only, no ORB, bytes built from §9.4"

# The window alternates across the matrix rather than doubling it: what
# matters is that a zero window and a real one each appear against every other
# axis, not that every pair of them is enumerated.
n=0
for endian in big little; do
  for control in close error; do
    for cut in 0 1; do
      for conts in 0 1; do
        if [ $(( n % 2 )) -eq 0 ]; then window=0; else window=80; fi
        n=$((n + 1))
        one_case "$endian" "$control" "$cut" "$conts" "$window"
      done
    done
  done
done

echo
# The verdict names what was measured and what was not, separately. Collapsing
# them is how a run in which nothing happened comes to read as a run in which
# something held.
if [ "$fails" -eq 0 ]; then
  echo "half_reply: PASS — $cases cases measured, both byte orders, both control messages"
  exit 0
fi
echo "half_reply: FAIL — $fails of $cases cases ($unmeasured of them UNMEASURED, which is not a pass)"
exit 1
