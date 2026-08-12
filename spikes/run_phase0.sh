#!/usr/bin/env bash
# Phase 0 harness: runs every feasibility assumption and prints a verdict.
#
# The omniORB fixture is LGPL/GPL and is used only as a wire peer and a
# conformance oracle. Nothing here links it into Orbweaver. See docs/PLAN.md §10.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
fail_total=0

hr() { printf '\n\033[1m%s\033[0m\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }
need omniidl
need cargo

# Kills the fixture and waits for it to actually be gone. Signalling is
# asynchronous, so returning early lets the next fixture race a dying process.
cleanup() {
  pkill -f echo_server.py >/dev/null 2>&1 || true
  for _ in $(seq 1 50); do
    pgrep -f echo_server.py >/dev/null 2>&1 || return 0
    sleep 0.1
  done
}
trap cleanup EXIT

# Waits for the fixture to actually publish an IOR.
#
# The wait must sleep. An earlier version spun without sleeping, which took
# microseconds and therefore did not wait at all; it only looked correct
# because `cargo run` had to compile first and accidentally covered the race.
# Once the build was warm the race surfaced as phantom GIOP timeouts.
start_server() {
  cleanup
  rm -f "$ROOT/spikes/echo.ior"
  ( cd "$ROOT/spikes" && exec python3 echo_server.py "$@" >/dev/null 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/echo.ior" ] && { sleep 0.2; return 0; }  # settle after publish
    sleep 0.1
  done
  echo "  FAIL fixture did not publish an IOR within 10s"
  return 1
}

# ── Unit tests ───────────────────────────────────────────────────────────────
hr "unit tests (CDR + GIOP)"
if cargo test --workspace --quiet 2>&1 | grep -q "^error"; then
  echo "  FAIL cargo test"; fail_total=$((fail_total+1))
else
  echo "  ok   cargo test --workspace"
fi

# ── Lint (runs before the oracle, on purpose) ────────────────────────────────
hr "licence boundary"
if cargo tree --workspace 2>/dev/null | grep -qiE "omniorb|jacorb"; then
  echo "  FAIL an ORB fixture has become a dependency"; fail_total=$((fail_total+1))
else
  echo "  ok   no ORB fixture appears in cargo tree"
fi
# NOTICE promises that --no-default-features drops encoding_rs and the
# BSD-3-Clause obligation with it. That promise is testable, so it is tested.
if cargo tree -p orbweaver-giop --no-default-features 2>/dev/null | grep -q encoding_rs; then
  echo "  FAIL --no-default-features still pulls encoding_rs; NOTICE is wrong"
  fail_total=$((fail_total+1))
else
  echo "  ok   --no-default-features drops encoding_rs, as NOTICE states"
fi
if cargo test -p orbweaver-giop --lib --no-default-features --quiet >/dev/null 2>&1; then
  echo "  ok   the attribution-free build still passes its tests"
else
  echo "  FAIL the attribution-free build does not build or test"; fail_total=$((fail_total+1))
fi

hr "IDL lint — case-insensitive identifier clashes"
lint_out=$(python3 spikes/idl_lint.py corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl 2>&1)
if [ -z "$lint_out" ]; then
  echo "  ok   no clashes in files that are expected to compile"
else
  echo "$lint_out" | sed 's/^/  /'
  fail_total=$((fail_total+1))
fi
# The lint must keep catching what it was written for.
lint_neg=0
for f in corpus/negative/n02-identifier-clash.idl corpus/negative/n03-scope-clash.idl corpus/negative/n09-struct-scope-clash.idl; do
  [ -n "$(python3 spikes/idl_lint.py "$f" 2>&1)" ] && lint_neg=$((lint_neg+1))
done
if [ "$lint_neg" -eq 3 ]; then
  echo "  ok   still catches all 3 pinned clash cases"
else
  echo "  FAIL lint caught only $lint_neg/3 pinned clash cases — it has regressed"
  fail_total=$((fail_total+1))
fi

# ── Golden corpus ────────────────────────────────────────────────────────────
hr "golden IDL corpus (must all compile)"
gp=0; gf=0
for f in corpus/golden/*.idl; do
  if [ -z "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then gp=$((gp+1)); else gf=$((gf+1)); echo "  FAIL $(basename "$f")"; fi
done
echo "  $gp pass / $gf fail"
[ "$gf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Negative corpus ──────────────────────────────────────────────────────────
hr "negative IDL corpus (must all be rejected)"
np=0; nf=0
for f in corpus/negative/*.idl; do
  if [ -n "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then np=$((np+1)); else nf=$((nf+1)); echo "  FAIL $(basename "$f") compiled but should not"; fi
done
echo "  $np correctly rejected / $nf wrongly accepted"
[ "$nf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Assumption C ─────────────────────────────────────────────────────────────
hr "assumption C — IDL 4 @annotation acceptance in a deployed compiler"
c1=$(omniidl -b dump corpus/annotations/c1-idl4-annotation.idl 2>&1 >/dev/null)
c3=$(omniidl -b dump corpus/annotations/c3-structured-comment.idl 2>&1 >/dev/null)
if [ -n "$c1" ]; then echo "  confirmed  @annotation REJECTED by omniidl (risk R1 is real)"; else echo "  surprise   @annotation accepted — revisit the SIDL plan"; fi
if [ -z "$c3" ]; then echo "  ok         structured-comment fallback compiles"; else echo "  FAIL       fallback does not compile"; fail_total=$((fail_total+1)); fi

# ── Assumption B ─────────────────────────────────────────────────────────────
hr "assumption B — generated IDL compiles"
bp=0; bf=0
for f in corpus/requirements/generated/R*.idl; do
  if [ -z "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then bp=$((bp+1)); else bf=$((bf+1)); echo "  FAIL $(basename "$f")"; fi
done
echo "  $bp/20 compile after self-repair (first pass was 13/20 — see docs/PHASE0.md)"
[ "$bf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Assumption A ─────────────────────────────────────────────────────────────
hr "assumption A — GIOP interop against a stock ORB"
if start_server; then
  # Capture before matching. Piping into `grep -q` closes the pipe on the
  # first match and SIGPIPEs the producer, which shows up as a phantom
  # failure — that bug cost a debugging cycle here already.
  interop=$(cargo run -q --bin spike-interop -- spikes/echo.ior 2>&1)
  printf '%s\n' "$interop" > /tmp/orbweaver-a.log
  if printf '%s' "$interop" | grep -q "assumption A: PASS"; then
    echo "  ok   both byte orders interoperated"
  else
    echo "  FAIL see /tmp/orbweaver-a.log"
    printf '%s' "$interop" | grep -E "^  FAIL" | head -3 | sed 's/^/     /'
    fail_total=$((fail_total+1))
  fi
else
  # A fixture that will not start is an unmeasured assumption, not a pass.
  fail_total=$((fail_total+1))
fi
cleanup

# ── Assumption D ─────────────────────────────────────────────────────────────
hr "assumption D — IOR endpoint publishing"
if start_server; then
  adv=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1 | head -1)
  echo "  default publish: $adv"
  case "$adv" in
    *127.0.0.1*) echo "  note  loopback published; a container would publish its pod IP instead" ;;
    *)           echo "  confirmed  a routable-but-local address is published, not loopback (risk R7 is real)" ;;
  esac
else
  fail_total=$((fail_total+1))
fi
cleanup
if start_server -ORBendPoint giop:tcp::40404 -ORBendPointPublish giop:tcp:127.0.0.1:40404; then
  rewritten=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1)
  echo "  rewritten publish: $(echo "$rewritten" | head -1)"
  if echo "$rewritten" | grep -q RESPONSE; then
    echo "  ok   endpoint rewriting works — mitigation for R7 is available"
  else
    echo "  FAIL endpoint rewriting did not produce a reachable reference"
    echo "$rewritten" | grep -E "no response|closed|error" | sed 's/^/       /'
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi

hr "verdict"
if [ "$fail_total" -eq 0 ]; then
  echo "  Phase 0: all checks green"
else
  echo "  Phase 0: $fail_total check group(s) failed"
fi
exit "$fail_total"
