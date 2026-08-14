#!/usr/bin/env bash
# nat_rewrite.sh — risk R7: an IOR that names an address the client cannot be
# served at, and the rewrite that repairs it.
#
# Phase 0 assumption D measured the hazard on a stock ORB ("a routable-but-local
# address is published, not loopback"). This measures the mitigation on ours,
# and it measures it by *dialing*: a unit test cannot tell a correct rewrite
# from a plausible one, because nothing in it ever opens a socket.
#
#   ./spikes/nat_rewrite.sh
#
# What it does
#   1. Finds a real address on this machine that is NOT the one the servant
#      will bind. That address is what an ORB would publish if it believed it
#      were there — assumption D's exact mistake.
#   2. Runs `spike-nat prove`, which requires the unrewritten reference to fail
#      to dial and the rewritten one to complete a call.
#   3. Runs the container probe under spikes/nat/ if Docker is present.
#
# What it is honest about
#   There is no NAT on this host. The claimed address is unusable because the
#   servant is not listening on it, where in a container it would be the
#   namespace boundary. The container probe is the check that would close that
#   gap; where Docker is absent it is a COUNTED SKIP, never a pass.
#
# No harness lock is taken: nothing here is killed by pattern, every port is
# ephemeral, and no fixed /tmp path is written, so a concurrent run_checks.sh
# cannot collide with this and it cannot collide with one.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

fails=0
skipped=0

bold() { printf '\n\033[1m%s\033[0m\n' "$1"; }
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }
skip() { printf '  skip %s\n' "$1"; skipped=$((skipped + 1)); }
note() { printf '  ..   %s\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }

need cargo

# ── A real address on this machine that is not loopback ──────────────────────
# Preferred over an invented one because a *real* local address gives an
# immediate refusal rather than a timeout, so the failure is attributable to
# "nothing is serving there" and not to "the packet went nowhere". Both are
# exercised when both are available.
second_address() {
  if command -v ip >/dev/null 2>&1; then
    ip -4 -o addr show scope global 2>/dev/null |
      awk '{split($4,a,"/"); print a[1]}' | grep -v '^127\.' | head -1
  elif command -v ifconfig >/dev/null 2>&1; then
    ifconfig 2>/dev/null | awk '/inet /{print $2}' | grep -v '^127\.' | head -1
  fi
}

# A container-style address on no network this host has. It reproduces the
# other half of assumption D — the client hangs rather than being refused —
# and it is the same prefix that table used.
UNROUTABLE=10.244.3.17

bold "R7 — IOR endpoint rewriting, measured by dialing"

claimed=""
lan=$(second_address)
if [ -n "$lan" ]; then
  claimed="$lan"
  note "claimed address 1: $lan (a real address on this machine, nothing serving there)"
else
  skip "no non-loopback address on this machine: the refused-immediately case is unmeasured"
fi
claimed="$claimed $UNROUTABLE"
note "claimed address 2: $UNROUTABLE (no route from here; the client hangs, as in assumption D)"

# shellcheck disable=SC2086 — the address list is deliberately word-split.
out=$(cd "$ROOT" && cargo run -q --bin spike-nat -- prove 127.0.0.1 $claimed 2>&1)
status=$?
printf '%s\n' "$out" | sed 's/^/  | /'
# Captured, then matched: piping into `grep -q` SIGPIPEs the producer, which
# has produced a phantom pass in this project before.
if [ "$status" -eq 0 ] && printf '%s' "$out" | grep -q "nat rewriting: PASS"; then
  pass "unrewritten references did not dial; rewritten ones completed a call"
else
  fail "see the transcript above"
fi

# ── The container probe ──────────────────────────────────────────────────────
# The one thing this host cannot show: a client in another routing domain. The
# probe under spikes/nat/ puts the servant and the client on separate Docker
# networks, so the servant's own address is genuinely unreachable from the
# client and only the published one works.
bold "container probe — a client in another routing domain"
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  if (cd "$ROOT/spikes/nat" && ./run.sh); then
    pass "the container probe ran: naive publish failed, rewritten publish worked"
  else
    fail "the container probe ran and did not demonstrate the fix"
  fi
else
  skip "Docker is not available here; spikes/nat/ is written and UNRUN"
  note "it has never executed anywhere — do not read it as evidence"
fi

bold "verdict"
echo "  failures: $fails   unmeasured (skipped): $skipped"
if [ "$fails" -eq 0 ] && [ "$skipped" -eq 0 ]; then
  echo "  nat rewriting: PASS"
elif [ "$fails" -eq 0 ]; then
  echo "  nat rewriting: PASS with $skipped unmeasured check(s) — see above"
else
  echo "  nat rewriting: FAIL"
fi
[ "$fails" -eq 0 ] || exit 1
