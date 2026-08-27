#!/usr/bin/env bash
# preflight.sh — what this machine can and cannot run of the R7 probes.
#
# ***THIS SCRIPT REPORTS; IT PROVES NOTHING.***
#
# The R7 rewriting itself is measured on real sockets by `spikes/nat_rewrite.sh`
# and that part runs anywhere. The part that needs more than one host is a
# client in **another routing domain**, and there are three ways to get one:
#
#   spikes/nat/vm/run.sh    a multipass VM — a real second host. **This one has
#                           run**, on 2026-08-14; see docs/PHASE6.md.
#   spikes/nat/run.sh       two isolated Docker networks. Never executed.
#   spikes/nat/k8s/run.sh   a Deployment behind a Service. Never executed.
#
# Which of them a machine can run is an environment fact rather than an
# opinion, so this script measures the fact instead of asserting it, and says
# what the smallest missing piece is when the answer is none of them.
#
#   ./spikes/nat/preflight.sh          # human report
#   ./spikes/nat/preflight.sh --quiet  # exit code only
#
# Exit 0  at least one routing-domain probe can run here (and which one).
# Exit 1  none can; every line above says why, and the last line says what
#         would be enough to change that.
#
# Why this exists as its own file: "docker: command not found" is a true but
# useless diagnosis. It does not distinguish an engine that is absent from one
# that is installed and not running, nor from a machine where the engine could
# never be installed because the disk is full — which is this machine, and
# which took a run of `multipass launch` to find out.
set -uo pipefail

QUIET=0
[ "${1:-}" = "--quiet" ] && QUIET=1

say() { [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"; }
ok() { say "  ok   $*"; }
no() { say "  --   $*"; }

# ── A deadline, without GNU `timeout` ────────────────────────────────────────
# macOS ships no `timeout`, and `docker info` against a dead daemon is exactly
# the call that hangs. The loop sleeps and is deadline-bounded (the harness
# rule), and the child is killed by its captured PID rather than by pattern.
bounded() { # bounded <seconds> <cmd...>
  local limit="$1"
  shift
  local out
  # `-t PREFIX` without X's is a BSD extension; GNU fails with `too few X's`
  # and leaves this empty, so every path built from it lands at the filesystem
  # root. Swept 2026-08-27 across every tracked *.sh.
  out=$(mktemp "${TMPDIR:-/tmp}/orbweaver-preflight.XXXXXX") || return 125
  "$@" >"$out" 2>&1 &
  local pid=$! ticks=0
  local max=$((limit * 5))
  while kill -0 "$pid" 2>/dev/null; do
    sleep 0.2
    ticks=$((ticks + 1))
    if [ "$ticks" -ge "$max" ]; then
      kill -TERM "$pid" 2>/dev/null
      sleep 0.5
      kill -KILL "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null
      rm -f "$out"
      return 124
    fi
  done
  wait "$pid"
  local rc=$?
  cat "$out"
  rm -f "$out"
  return "$rc"
}

runnable=0
engine=""

say
say "container engines — spikes/nat/run.sh needs one that is running"
for e in docker podman nerdctl finch container; do
  if ! command -v "$e" >/dev/null 2>&1; then
    no "$e: not installed"
    continue
  fi
  # Apple's `container` names the same question differently.
  if [ "$e" = container ]; then
    bounded 20 "$e" system status >/dev/null 2>&1
  else
    bounded 20 "$e" info >/dev/null 2>&1
  fi
  if [ $? -eq 0 ]; then
    ok "$e: installed and responding"
    [ -z "$engine" ] && engine="$e"
    runnable=1
  else
    no "$e: installed, but its daemon did not answer within 20s"
  fi
done

say
say "clusters — spikes/nat/k8s/run.sh needs kubectl and a reachable cluster"
if command -v kubectl >/dev/null 2>&1; then
  # Captured first, then read — and read by expansion rather than by `head`,
  # which would close the pipe on the process feeding it.
  ci=$(bounded 20 kubectl cluster-info 2>&1)
  if [ $? -eq 0 ]; then
    ok "kubectl: a cluster answered (${ci%%$'\n'*})"
    runnable=1
  else
    no "kubectl: installed, no cluster answered within 20s"
  fi
else
  no "kubectl: not installed"
fi
for k in kind minikube k3d; do
  if command -v "$k" >/dev/null 2>&1; then
    ok "$k: installed (can create the cluster kubectl would then reach)"
  else
    no "$k: not installed"
  fi
done

# ── The layer under all of them, which is also a probe in its own right ──────
# On Linux the engine is the boundary. On macOS every engine above is a Linux
# VM in a trench coat — so a hypervisor is both the prerequisite for the other
# two probes AND, on its own, a second host, which is all R7 ever needed.
# `spikes/nat/vm/run.sh` skips the middle layer and uses it directly.
say
say "hypervisors — spikes/nat/vm/run.sh needs one (and IS the probe that ran)"
case "$(uname -s)" in
  Darwin)
    no "network namespaces: macOS has none, so no unprivileged in-host boundary"
    for v in multipass colima lima krunvm tart; do
      if command -v "$v" >/dev/null 2>&1; then
        ok "$v: installed"
      else
        no "$v: not installed"
      fi
    done
    if command -v multipass >/dev/null 2>&1; then
      # An instance that already exists needs no image and no free disk.
      #
      # Herestring, and the producer's status read first by the `&&`. Piping
      # into `grep -q` lies **quietly** here: `grep -q` stops at the first
      # `Running` and SIGPIPEs `multipass list` (141), which `pipefail` — set
      # on line 31 — makes the pipeline's status, and the `if` reads that as
      # "no instance is running". A preflight whose whole job is to say which
      # probes could run would report the one that *can* run as unavailable.
      if insts=$(bounded 20 multipass list --format csv 2>/dev/null) &&
        grep -q Running <<<"$insts"; then
        ok "multipass: an instance is already running — spikes/nat/vm/run.sh can run now"
        runnable=1
      fi
    fi
    # Free space on the volume a VM image would land on. Multipass refuses an
    # image whose declared minimum disk exceeds what is free, before download.
    # Reported in MiB, not rounded to GiB: the interesting readings here are
    # under a gigabyte and "0GiB" hides the difference between "tight" and
    # "there is nothing left".
    avail_kb=$(df -k / | awk 'NR==2 {print $4}')
    avail_mb=$((avail_kb / 1024))
    if [ "${avail_kb:-0}" -lt 4194304 ]; then
      no "free disk: ${avail_mb}MiB — under the ~4096MiB a minimal VM image needs"
    else
      ok "free disk: ${avail_mb}MiB — enough to launch a VM"
      command -v multipass >/dev/null 2>&1 && runnable=1
    fi
    ;;
  Linux)
    if command -v unshare >/dev/null 2>&1; then
      ok "unshare: present — network namespaces exist on this kernel"
    else
      no "unshare: not installed"
    fi
    ;;
  *) no "unrecognised kernel $(uname -s)" ;;
esac

say
if [ "$runnable" -eq 1 ]; then
  say "verdict: a routing-domain probe can run here${engine:+ (engine: $engine)}."
  say "         spikes/nat/vm/run.sh is the one that has actually executed."
  say "         The container and cluster probes have never executed anywhere —"
  say "         expect to fix them, not to confirm them, and treat a first green"
  say "         run with suspicion."
  exit 0
fi
say "verdict: no routing-domain probe can run here."
say "         The smallest unblocking step, in order of cost:"
say "         1. free enough disk for a Linux VM image (~4GiB), then"
say "            \`spikes/nat/vm/run.sh\`, which launches one itself. No"
say "            container engine and no guest network are needed: the client"
say "            is cross-compiled here and copied in."
say "         2. or point KUBECONFIG at any cluster you can already reach and"
say "            run spikes/nat/k8s/run.sh against it."
say "         Until then R7 across a real routing boundary stays UNMEASURED"
say "         on THIS machine, and a skip is counted, never passed."
exit 1
