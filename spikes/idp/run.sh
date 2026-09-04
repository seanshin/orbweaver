#!/usr/bin/env bash
# Driver for the orbweaver-idp fixture (spikes/idp/idp.py). A fixture driver,
# not a gate: `selftest` runs the whole loop and its exit code is the verdict;
# `start`/`stop` are for a caller (the harness's D010 B2 group, an interactive
# session) that owns a longer lifecycle.
#
#   run.sh selftest [--misissue]   start on an ephemeral port, run
#                                  spikes/idp/selftest.py against it, stop.
#                                  With --misissue the signature check MUST
#                                  fail — the negative control; a green
#                                  selftest there verifies nothing.
#   run.sh start [statefile]       start, print ISSUER url, leave it running.
#                                  The caller that starts owes the stop.
#   run.sh stop  [statefile]       stop what start recorded.
#
# State lives in spikes/idp/.run/ (gitignored), never under /tmp/orbweaver* —
# that prefix belongs to the harness's own fixtures and its cleanup sweeps.
# The port is ephemeral (idp.py binds port 0 and prints what it got); this
# fixture never squats on a well-known port.
#
# THE BOUND, restated at the door because this is the file a harness would
# call: this issuer measures the exchange path against a real token from a
# real endpoint. It cannot refute a verifier's ACCEPTING direction — a
# verifier wrong that way interoperates perfectly with every honest token
# here — which is why the tree's `Verifier` stays an unimplemented trait
# (crates/orbweaver-mcp/src/token.rs, D002). `--misissue` is the closest an
# issuer can come: it signs with a key the JWKS does not publish, and any
# consumer that still accepts the token is not verifying.
#
# Exit codes: 0 verdict held · 1 failed · 2 usage · 3 unmeasured.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$(pwd)
STATE_DEFAULT="$HERE/.run/state.env"

command -v python3 >/dev/null 2>&1 || { echo "UNMEASURED: no python3"; exit 3; }

start_idp() { # $1 statefile, remaining args passed to idp.py
  local state="$1"; shift
  mkdir -p "$(dirname "$state")"
  local log="${state%.env}.log"
  : >"$log"
  python3 "$HERE/idp.py" "$@" >"$log" 2>&1 &
  local pid=$!
  # Wait for READY — printed strictly after the socket listens — with a
  # sleeping, deadline-bounded loop (grep reads a file: no pipe to lie).
  local end=$(( $(date +%s) + 15 ))
  while [ "$(date +%s)" -lt "$end" ]; do
    if grep -q "^READY$" "$log" 2>/dev/null; then
      local url
      url=$(grep '^ISSUER ' "$log" | head -1 | cut -d' ' -f2)
      printf 'PID=%s\nURL=%s\nLOG=%s\n' "$pid" "$url" "$log" >"$state"
      echo "$url"
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "FAIL idp.py exited before READY; its log:" >&2
      cat "$log" >&2
      return 1
    fi
    sleep 0.1
  done
  echo "FAIL idp.py did not print READY within 15s" >&2
  kill "$pid" 2>/dev/null || true
  return 1
}

stop_idp() { # $1 statefile
  local state="$1"
  [ -f "$state" ] || { echo "nothing recorded at $state"; return 0; }
  local pid
  pid=$(grep '^PID=' "$state" | cut -d= -f2)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    # idp.py spawns nothing, so the pid IS the tree; TERM exits it cleanly.
    kill "$pid" 2>/dev/null || true
    local end=$(( $(date +%s) + 5 ))
    while kill -0 "$pid" 2>/dev/null && [ "$(date +%s)" -lt "$end" ]; do
      sleep 0.1
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$state"
}

case "${1:-selftest}" in
  start)
    start_idp "${2:-$STATE_DEFAULT}"
    ;;
  stop)
    stop_idp "${2:-$STATE_DEFAULT}"
    ;;
  selftest)
    # A plain string, not an array: macOS ships bash 3.2, where expanding an
    # empty array under `set -u` is an error. Unquoted on purpose — empty
    # vanishes, non-empty is one flag with no spaces in it.
    MIS=""
    [ "${2:-}" = --misissue ] && MIS=--misissue
    STATE=$(mktemp -d)/state.env
    trap 'stop_idp "$STATE"; rm -rf "$(dirname "$STATE")"' EXIT INT TERM
    URL=$(start_idp "$STATE" $MIS)
    echo "  ok   issuer up at $URL (ephemeral port)"
    rc=0
    python3 "$HERE/selftest.py" "$URL" || rc=$?
    exit "$rc"
    ;;
  *)
    echo "usage: run.sh [selftest [--misissue] | start [statefile] | stop [statefile]]" >&2
    exit 2
    ;;
esac
