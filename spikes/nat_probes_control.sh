#!/usr/bin/env bash
# Negative control for `spikes/lib/nat_probes.sh`, the parser the harness's NAT
# group reads `nat_rewrite.sh` through.
#
# It SYNTHESISES transcripts rather than pointing at today's — a control pinned
# to a live subject stops being a control when the subject moves — and it runs
# the shipped parser's own bytes by sourcing the file. It does not restate the
# parser.
#
# Five shapes, five different readings. The harness's previous code produced ONE
# sentence for all five ("the container probe has never run here — no docker"),
# which is what this control exists to make impossible again.
#
# *합성한 트랜스크립트 다섯 모양에 다섯 가지 다른 판독. 이전 하네스 코드는 다섯
# 전부에 문장 하나를 냈다.*
set -u
LIB="${1:-$(dirname "$0")/lib/nat_probes.sh}"
. "$LIB"

fails=0
say() { echo "  $1"; [ "${1#ok}" = "$1" ] && fails=$((fails+1)); }

# The shapes are built from the parent script's real vocabulary — headings and
# judgement lines as `nat_rewrite.sh` prints them — with sub-script noise
# inserted where the real transcript has it (headings, its own ok/FAIL lines,
# a `verdict` of its own). A parser that survives the noise here survives it
# in the harness.
H_VM="$(printf '\033[1mvm probe — a client on a real second host\033[0m')"
H_CT="$(printf '\033[1mcontainer probe — a client in another routing domain\033[0m')"
H_K8="$(printf '\033[1mcluster probe — a client outside the cluster, dialing a Service\033[0m')"
H_VD="$(printf '\033[1mverdict\033[0m')"
NOISE="$(printf '\033[1mR7 across a real routing boundary — a second host, not a simulation\033[0m
  ..   servant runs on the host; client runs in r7client
         FAIL all 1 endpoint(s) failed; last: io: Connection refused
  ok   naive: the client could not dial it, as R7 predicts
%s
  failures: 0' "$H_VD")"

VM_OK="  ok   the vm probe ran: the loopback reference did not dial, the mapped one did"
VM_SK="  skip no multipass instance is running; ORBWEAVER_KEEP=1 spikes/nat/vm/run.sh launches one"
CT_OK="  ok   the container probe ran: naive publish failed, rewritten publish worked"
CT_SK="  skip Docker is not available here; spikes/nat/ runs where docker is (first ran on CI 2026-09-04)"
CT_FL="  FAIL the container probe ran and did not demonstrate the fix"
K8_SK="  skip no cluster answered here; spikes/nat/k8s/ runs where a cluster is (first ran on CI 2026-09-04)"
# The parent's OTHER skip sentence (2026-09-04): the probe was reached and its
# own script exited 2 — could not run — which judge_probe speaks as a skip
# rather than as "ran and did not demonstrate". A refusal-to-run must read as
# skipped, never as failed and never as ran.
K8_S2="  skip the cluster probe could not run (its own exit 2 — a precondition failed inside it; its lines above say which)"

shape() { # shape <vm line> <container line> <cluster line>
  printf '%s\n%s\n%s\n\n%s\n%s\n\n%s\n%s\n\n%s\n  failures: 0\n' \
    "$H_VM" "$NOISE" "$1" "$H_CT" "$2" "$H_K8" "$3" "$H_VD"
}

expect() { # expect <label> <transcript> <expected tab-separated probe/state pairs, one per line>
  local label="$1" got want
  got=$(nat_probe_lines "$2" | cut -f1,2)
  want="$3"
  if [ "$got" = "$want" ]; then say "ok   $label"
  else say "FAIL $label — got:"; sed 's/^/         /' <<<"$got"; echo "       wanted:"; sed 's/^/         /' <<<"$want"; fi
}

# 1 — this machine: vm ran, container and cluster skipped
expect "vm ran, container+cluster skipped (this machine)" \
  "$(shape "$VM_OK" "$CT_SK" "$K8_SK")" \
  "$(printf 'vm\tran\ncontainer\tskipped\ncluster\tskipped')"

# 2 — CI as the plan expects it: container ran, vm and cluster skipped
expect "container ran, vm+cluster skipped (CI)" \
  "$(shape "$VM_SK" "$CT_OK" "$K8_SK")" \
  "$(printf 'vm\tskipped\ncontainer\tran\ncluster\tskipped')"

# 3 — nothing ran
expect "all three skipped" \
  "$(shape "$VM_SK" "$CT_SK" "$K8_SK")" \
  "$(printf 'vm\tskipped\ncontainer\tskipped\ncluster\tskipped')"

# 4 — the container probe ran and FAILED: must read as failed, never as skipped
expect "container probe failed is failed, not skipped" \
  "$(shape "$VM_SK" "$CT_FL" "$K8_SK")" \
  "$(printf 'vm\tskipped\ncontainer\tfailed\ncluster\tskipped')"

# 5 — the sub-script's own ok/FAIL/verdict must not be read as the parent's
#     judgement: a vm section with noise but NO parent judgement line yields
#     nothing for vm, rather than an `ok` lifted from the sub-script.
expect "sub-script noise is not a judgement" \
  "$(shape "" "$CT_SK" "$K8_SK")" \
  "$(printf 'container\tskipped\ncluster\tskipped')"

# 6 — a probe REACHED whose own script exited 2 (could not run): the parent
#     speaks judge_probe's skip sentence, and it must read as skipped — the
#     shape whose absence let an exit-2 print as a run that refuted the fix.
expect "a probe that could not run (exit 2) is skipped, not failed" \
  "$(shape "$VM_SK" "$CT_OK" "$K8_S2")" \
  "$(printf 'vm\tskipped\ncontainer\tran\ncluster\tskipped')"

# 7 — the control's own control: a parser that emits nothing for everything
#     would pass no shape above but 5. Assert shape 1 is non-empty explicitly,
#     so an emptied parser cannot slide through on the strength of shape 5.
n=$(nat_probe_lines "$(shape "$VM_OK" "$CT_SK" "$K8_SK")" | wc -l | tr -d ' ')
# `if`, not `a && b || c`: with that chain a `say` whose test returns non-zero
# falls through to the `||` arm and reports the failure it just did not have.
if [ "$n" -eq 3 ]; then say "ok   the parser emits three lines for three probes (not zero)"
else say "FAIL the parser emitted $n line(s) for three probes"; fi

echo
[ "$fails" -eq 0 ] && { echo "nat_probes control: 7 of 7"; exit 0; }
echo "nat_probes control: $fails FAILED"; exit 1
