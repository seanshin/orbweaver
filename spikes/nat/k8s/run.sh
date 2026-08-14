#!/usr/bin/env bash
# Cluster probe for risk R7: a client outside the cluster, dialing in.
#
# ***THIS SCRIPT HAS NEVER BEEN RUN.***
#
# It was written on a machine with no cluster and no way to make one — no
# container engine of any kind, and `multipass launch` refused before it even
# booted: "Available disk (974639104 bytes) below minimum for this image
# (3758096384 bytes)". So nothing below is evidence of anything.
# `spikes/nat_rewrite.sh` counts it as a SKIP, never a pass. Run
# `spikes/nat/preflight.sh` first; it says whether this machine can run this at
# all, and what is missing if not.
#
#   ORBWEAVER_NODE_ADDR=192.168.64.7 ./spikes/nat/k8s/run.sh
#
# ── What has to be true before it can work ───────────────────────────────────
#
#   1. `kubectl` reaches a cluster.
#   2. The image exists **in the cluster**, side-loaded rather than pushed:
#
#        docker build -t orbweaver/spike-nat:probe -f spikes/nat/Dockerfile .
#        kind load docker-image orbweaver/spike-nat:probe     # kind
#        minikube image load orbweaver/spike-nat:probe        # minikube
#
#   3. `$ORBWEAVER_NODE_ADDR:30555` is reachable **from this shell**. This is
#      the one step that is environment-specific and the one most likely to be
#      what breaks first:
#        - kind on Linux: the node's InternalIP works as-is.
#        - kind on macOS: the node is a container inside a VM, so its
#          InternalIP is not reachable from the host. Create the cluster with
#          an extraPortMapping for 30555 and pass ORBWEAVER_NODE_ADDR=127.0.0.1.
#        - minikube: `minikube ip` on the VM drivers; with the docker driver,
#          `minikube service --url orbweaver-nat -n <ns>` and take the host and
#          port from that.
#      If the address is wrong, BOTH cases fail to dial — which this script
#      reports as a failure, because a probe whose control case does not work
#      has measured nothing.
#
# ── The assertion ────────────────────────────────────────────────────────────
#
#   naive      the servant publishes its **pod IP**. The out-of-cluster client
#              cannot route to it: the dial FAILS.
#   published  the servant publishes **the Service**, host and port both
#              translated (5555 inside, 30555 outside), through
#              ORBWEAVER_PUBLISH_MAP. The dial SUCCEEDS and ping() returns 42.
#
# A run where both succeed has not demonstrated the fix. It means the client
# was not outside the cluster's routing domain — see the note in
# manifests.yaml about pod-to-pod networking being flat. That is a broken
# probe, not a pass, and this script fails it as one.
set -uo pipefail
cd "$(dirname "$0")"
HERE=$(pwd)
ROOT=$(cd "$HERE/../../.." && pwd)

NS=${ORBWEAVER_NS:-orbweaver-r7}
NODE_PORT=30555
IMAGE=orbweaver/spike-nat:probe

fails=0
WORK=$(mktemp -d -t orbweaver-r7-k8s) || exit 2

cleanup() {
  kubectl delete namespace "$NS" --ignore-not-found --wait=false >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1" >&2; exit 2; }; }
need kubectl
need cargo

k() { kubectl -n "$NS" "$@"; }

# ── The address the client will actually dial ────────────────────────────────
node_addr() {
  if [ -n "${ORBWEAVER_NODE_ADDR:-}" ]; then
    printf '%s' "$ORBWEAVER_NODE_ADDR"
    return
  fi
  # Captured, then read. The InternalIP is the right answer on a cluster whose
  # nodes are on a network this shell shares, and the wrong one on every
  # macOS-hosted cluster — hence the override above.
  kubectl get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}' 2>/dev/null
}

# Deadline-bounded and sleeping. A loop with no sleep finishes in microseconds
# and does not wait at all — the harness rule that cost this project a
# debugging cycle in Phase 0.
wait_for_ior() { # wait_for_ior <destination-file>
  local dest="$1"
  local i
  for i in $(seq 1 120); do
    if k exec deploy/orbweaver-nat -- cat /shared/server.ior >"$dest" 2>/dev/null &&
      [ -s "$dest" ]; then
      return 0
    fi
    sleep 1
  done
  return 1
}

run_case() { # run_case <mode> <publish-map> <pass|fail> <label>
  local mode="$1" map="$2" want="$3" label="$4"
  local ior="$WORK/$mode.ior"

  # env from a configMapKeyRef is read once, at container start: rewriting the
  # ConfigMap does NOT reconfigure a running pod. The rollout restart is the
  # part that is easy to leave out and impossible to notice, because the probe
  # would then measure the previous case twice.
  kubectl create configmap orbweaver-nat -n "$NS" \
    --from-literal=nat-mode="$mode" \
    --from-literal=publish-map="$map" \
    --dry-run=client -o yaml | kubectl apply -f - >/dev/null || return 1
  k rollout restart deployment/orbweaver-nat >/dev/null 2>&1
  if ! k rollout status deployment/orbweaver-nat --timeout=180s >/dev/null 2>&1; then
    echo "  FAIL $label: the servant never became ready"
    k logs deploy/orbweaver-nat --tail=20 2>&1 | sed 's/^/       /'
    return 1
  fi
  if ! wait_for_ior "$ior"; then
    # An unmeasured check is a failure, never a pass.
    echo "  FAIL $label: the servant never published a reference"
    k logs deploy/orbweaver-nat --tail=20 2>&1 | sed 's/^/       /'
    return 1
  fi

  # What the servant says it published, so a later failure is attributable.
  local published
  published=$(k logs deploy/orbweaver-nat --tail=20 2>&1 | sed -n 's/^published //p')
  echo "  ..   $label: servant reports published ${published:-<not logged>}"

  local out status
  out=$(cd "$ROOT" && cargo run -q --bin spike-nat -- call "$ior" 2>&1)
  status=$?
  printf '%s\n' "$out" | sed 's/^/       /'

  if [ "$want" = pass ] && [ "$status" -eq 0 ]; then
    echo "  ok   $label: the out-of-cluster client's call completed"
    return 0
  fi
  if [ "$want" = fail ] && [ "$status" -ne 0 ]; then
    echo "  ok   $label: the out-of-cluster client could not dial it, as R7 predicts"
    return 0
  fi
  echo "  FAIL $label: wanted the dial to $want, it did not"
  return 1
}

echo "R7 cluster probe — UNRUN as written; treat a green run with suspicion"

NODE=$(node_addr)
if [ -z "$NODE" ]; then
  echo "  FAIL could not determine a node address; set ORBWEAVER_NODE_ADDR" >&2
  exit 1
fi
echo "  ..   namespace $NS, client dials $NODE:$NODE_PORT from outside the cluster"
if [ -z "${ORBWEAVER_NODE_ADDR:-}" ]; then
  echo "  ..   (that is the node's InternalIP, which is NOT reachable from a"
  echo "        macOS host running kind or minikube — set ORBWEAVER_NODE_ADDR)"
fi

kubectl create namespace "$NS" --dry-run=client -o yaml | kubectl apply -f - >/dev/null || exit 1
kubectl apply -n "$NS" -f "$HERE/manifests.yaml" >/dev/null || exit 1

# The image is side-loaded, so a cluster that has never seen it fails with
# ImagePullBackOff and the rollout wait above reports it — which is the right
# failure, but the message is worth pre-empting.
echo "  ..   image $IMAGE must already be in the cluster (see this file's header)"

run_case naive "" fail \
  "naive publish (the pod's own IP)" || fails=$((fails + 1))
run_case published "0.0.0.0:5555=$NODE:$NODE_PORT" pass \
  "published (Service address and NodePort, host AND port translated)" ||
  fails=$((fails + 1))

echo
if [ "$fails" -eq 0 ]; then
  echo "cluster probe: PASS"
else
  echo "cluster probe: FAIL — $fails case(s)"
fi
[ "$fails" -eq 0 ] || exit 1
