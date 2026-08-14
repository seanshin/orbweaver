#!/usr/bin/env bash
# Container probe for risk R7: a client in another routing domain.
#
# ***THIS SCRIPT HAS NEVER BEEN RUN.***
#
# It was written on a machine with no Docker (`docker: command not found`), so
# nothing below is evidence of anything. `spikes/nat_rewrite.sh` counts it as a
# SKIP, never a pass, and the first person to run it should expect to fix it
# rather than to confirm it. What it is for is stated so that fixing it is
# cheap: the single thing this probe adds over the host-only measurement is a
# client that genuinely cannot route to the servant's own address.
#
#   ./spikes/nat/preflight.sh    # whether this machine can run it, and what is
#                                # missing if not — measured, not asserted
#   ./spikes/nat/run.sh
#
# Its sibling `spikes/nat/k8s/` is the same assertion against a Deployment
# behind a Service, dialed from outside the cluster. That one also translates
# the *port* (a NodePort is not the port the servant bound), which this one
# does not; it is equally unrun.
#
# Expected outcome, which is the assertion:
#   naive      the servant publishes its container address; the client's dial
#              FAILS (it hangs to the connect timeout, then errors)
#   published  the servant publishes host.docker.internal:5555; the client's
#              dial SUCCEEDS and ping() returns 42
#
# A run where both succeed has not demonstrated the fix — it has demonstrated
# that the two networks are not isolated, which is a broken probe, not a pass.
set -uo pipefail
cd "$(dirname "$0")"

fails=0
DC="docker compose -f compose.yaml"

cleanup() { $DC down -v --remove-orphans >/dev/null 2>&1; }
trap cleanup EXIT

reset_shared() {
  rm -rf shared
  mkdir -p shared
}

# Deadline-bounded and sleeping. A loop without a sleep does not wait, which is
# the harness rule that cost this project a debugging cycle in Phase 0.
wait_for_ior() {
  for _ in $(seq 1 120); do
    [ -s shared/server.ior ] && return 0
    sleep 0.5
  done
  return 1
}

run_case() {
  local mode="$1" map="$2" want="$3" label="$4"
  reset_shared
  NAT_MODE="$mode" ORBWEAVER_PUBLISH_MAP="$map" $DC up -d --build server || return 1
  if ! wait_for_ior; then
    # An unmeasured check is a failure, never a pass.
    echo "  FAIL $label: the servant never published a reference"
    $DC logs server | tail -20 | sed 's/^/       /'
    $DC down -v >/dev/null 2>&1
    return 1
  fi
  echo "  ..   $label published: $(head -c 40 shared/server.ior)..."
  NAT_MODE=call $DC run --rm client
  local got=$?
  $DC down -v >/dev/null 2>&1
  if [ "$want" = pass ] && [ "$got" -eq 0 ]; then
    echo "  ok   $label: the client's call completed"
    return 0
  fi
  if [ "$want" = fail ] && [ "$got" -ne 0 ]; then
    echo "  ok   $label: the client could not dial it, as R7 predicts"
    return 0
  fi
  echo "  FAIL $label: wanted the dial to $want, it did not"
  return 1
}

echo "R7 container probe — UNRUN as written; treat a green run with suspicion"

run_case naive "" fail "naive publish (container's own address)" || fails=$((fails + 1))
run_case published '*=host.docker.internal' pass "published (rewritten at publish time)" ||
  fails=$((fails + 1))

echo
if [ "$fails" -eq 0 ]; then
  echo "container probe: PASS"
else
  echo "container probe: FAIL — $fails case(s)"
fi
[ "$fails" -eq 0 ] || exit 1
