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
#   4. Runs the cluster probe under spikes/nat/k8s/ if a cluster answers.
#
# What it is honest about
#   There is no NAT on this host. The claimed address is unusable because the
#   servant is not listening on it, where in a container it would be the
#   namespace boundary. The two routing-domain probes are the checks that would
#   close that gap; where their prerequisites are absent each is a COUNTED
#   SKIP, never a pass. Neither has ever executed anywhere. When one is skipped
#   this script prints the *measured* reason from `spikes/nat/preflight.sh`
#   rather than the useless one ("docker: command not found"), because that
#   message cannot tell an engine that is missing from an engine that is
#   installed and dead from a machine where no engine could be installed.
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

# ── The cluster manifest's map string, checked without a cluster ─────────────
# `spikes/nat/k8s/` has never run, and most of it cannot be checked here. One
# part can: the `publish-map` the ConfigMap carries is a string the real
# `EndpointMap` has to accept, and it is the part a reviewer is least able to
# check by eye — it translates a *port* as well as a host, which the compose
# probe never does. So the string is read out of the manifest itself, rather
# than retyped here, and run through the actual publish path.
#
# What this shows: the manifest's configuration is well-formed and applied as
# intended. What it does NOT show: that anything is reachable at the published
# address. Nothing is listening on the NodePort here and no dial is attempted.
bold "the cluster manifest's publish map — checked here, without a cluster"
manifest="$ROOT/spikes/nat/k8s/manifests.yaml"
raw=$(sed -n 's/^  publish-map: "\(.*\)"$/\1/p' "$manifest")
if [ -z "$raw" ]; then
  fail "no publish-map key found in $manifest — the check and the file disagree"
else
  # Only the placeholder host is substituted. The bind side and the translated
  # port are the manifest's own, because those are what is being checked.
  map=${raw/REPLACE-WITH-A-NODE-ADDRESS/127.0.0.1}
  want=${map#*=}
  note "manifest carries $raw"
  note "checking $map → expecting a reference naming $want"
  # Build first and run the binary directly: `cargo run` would put cargo
  # between us and the servant, and a captured PID must be the process we
  # actually need to kill.
  bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/spike-nat"
  if ! cargo build -q --bin spike-nat 2>/dev/null; then
    fail "could not build spike-nat"
  else
    tmp=$(mktemp -d -t orbweaver-r7-map)
    ORBWEAVER_PUBLISH_MAP="$map" "$bin" serve 0.0.0.0:5555 "$tmp/k8s.ior" \
      >"$tmp/serve.log" 2>&1 &
    serve_pid=$!
    published=""
    # Sleeping and deadline-bounded, and it also stops early if the servant
    # died — waiting the full deadline on a process that is gone is how a
    # timeout gets blamed for a bind error.
    for _ in $(seq 1 60); do
      published=$(sed -n 's/^published \([^ ]*\) .*/\1/p' "$tmp/serve.log")
      [ -n "$published" ] && break
      kill -0 "$serve_pid" 2>/dev/null || break
      sleep 0.25
    done
    kill -TERM "$serve_pid" 2>/dev/null
    wait "$serve_pid" 2>/dev/null
    log=$(cat "$tmp/serve.log" 2>/dev/null)
    if [ "$published" = "$want" ]; then
      pass "the manifest's map is accepted and publishes $published (host and port both translated)"
      note "unmeasured here: whether anything answers there. That needs the cluster."
    elif printf '%s' "$log" | grep -q "in use"; then
      # Unmeasured, so counted — never quietly passed.
      skip "port 5555 is busy on this machine, so the manifest's map was not exercised"
    else
      fail "the manifest's map published ${published:-<nothing>}, wanted $want"
      printf '%s\n' "$log" | sed 's/^/  | /'
    fi
    rm -rf "$tmp"
  fi
fi

# ── The routing-domain probes ────────────────────────────────────────────────
# The one thing a single host cannot show: a client in another routing domain.
# Three probes would show it. One of them has run.
#
#   spikes/nat/vm/    a multipass VM — a real second host. **HAS RUN**
#                     (2026-08-14); see docs/PHASE6.md for the transcript.
#   spikes/nat/       two isolated Docker networks. Never executed.
#   spikes/nat/k8s/   a Deployment behind a Service, dialed from outside the
#                     cluster — which additionally translates the *port*,
#                     since a NodePort is not the port the servant bound.
#                     Never executed.
#
# The VM probe is run here only when an instance is ALREADY running: launching
# one takes minutes and downloads an image, which is not a thing a check
# should do behind its caller's back. Where there is none it is a counted skip
# naming the command that would do it.
bold "vm probe — a client on a real second host"
if command -v multipass >/dev/null 2>&1 &&
  vms=$(multipass list --format csv 2>/dev/null) &&
  printf '%s' "$vms" | grep -q Running; then
  if (cd "$ROOT" && ORBWEAVER_KEEP=1 ./spikes/nat/vm/run.sh); then
    pass "the vm probe ran: the loopback reference did not dial, the mapped one did"
  else
    fail "the vm probe ran and did not demonstrate the fix"
  fi
else
  skip "no multipass instance is running; ORBWEAVER_KEEP=1 spikes/nat/vm/run.sh launches one"
  note "this probe HAS executed before — unlike the two below — see docs/PHASE6.md"
fi

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

bold "cluster probe — a client outside the cluster, dialing a Service"
if command -v kubectl >/dev/null 2>&1 && kubectl cluster-info >/dev/null 2>&1; then
  if (cd "$ROOT/spikes/nat/k8s" && ./run.sh); then
    pass "the cluster probe ran: the pod IP did not dial, the Service address did"
  else
    fail "the cluster probe ran and did not demonstrate the fix"
  fi
else
  skip "no cluster answered here; spikes/nat/k8s/ is written and UNRUN"
  note "it has never executed anywhere — do not read it as evidence"
fi

# Why the reason is measured rather than asserted: see this file's header.
if [ "$skipped" -gt 0 ]; then
  bold "why — measured, not assumed (spikes/nat/preflight.sh)"
  # Captured, then printed. Piping a live producer into a consumer that can
  # exit early is how this project has manufactured a phantom pass before.
  pre=$("$ROOT/spikes/nat/preflight.sh" 2>&1)
  printf '%s\n' "$pre" | sed 's/^/  | /'
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
