#!/usr/bin/env bash
# Reads `spikes/nat_rewrite.sh`'s output and says, per probe, what happened.
#
# ── Why this exists ──────────────────────────────────────────────────────────
#
# `run_checks.sh`'s NAT group used to read the script's *skip count* and print
# one fixed sentence for any non-zero value: "the container probe has never run
# here — no docker". The script has FIVE distinct skips (no second address, port
# 5555 busy, no multipass, no docker, no cluster) and prints a `pass` line for
# each probe that ran. Reading the count meant the harness could not say which
# probe ran and which did not — and on CI, where docker is present and multipass
# is not, it named the wrong probe. Measured 2026-09-03, `docs/PLAN-NAT-PROBE.md`.
#
# Every caller — the harness and the control — sources THIS file, so the parser
# has one home. `spikes/nat_probes_control.sh` feeds it synthesised transcripts
# and is what makes its silence over a real one mean anything.
#
# ── The contract ─────────────────────────────────────────────────────────────
#
#   nat_probe_lines <transcript>      one line per probe, tab-separated:
#                                     <probe>\t<ran|skipped|failed>\t<the script's own line>
#
# Probes are `vm`, `container`, `cluster`. The script's line is carried verbatim
# so the harness quotes the script rather than describing it.
#
# *스크립트의 skip **개수**를 읽고 문장 하나를 찍던 것을, 탐침마다 무엇이 일어났는지
# 스크립트의 **줄**에서 읽는다. 파서의 집은 여기 하나이고 하네스와 대조군이 같은
# 바이트를 쓴다.*

nat_probe_lines() {
  local transcript="$1"
  # `awk` reads its whole input and never exits early; the transcript arrives as
  # a file or a herestring, never through a pipe with a `-q` on its end.
  awk '
    function emit(probe, state, line) { printf "%s\t%s\t%s\n", probe, state, line }
    # The script marks each probe with a bold heading; what follows until the
    # NEXT PROBE heading belongs to it, and nothing else closes a section. Two
    # drafts of this parser got that wrong and each lost the vm probe: the
    # first let any bold line close a section, the second let `verdict` — and
    # `nat/vm/run.sh` prints a `verdict` heading of its own BEFORE the parent
    # script prints its one-line judgement of the probe. The parent'"'"'s
    # judgement lines are the only ones matched below, so a sub-script'"'"'s
    # own `ok`/`FAIL` cannot be mistaken for them whatever heading it sits under.
    /vm probe — a client on a real second host/        { cur = "vm";        next }
    /container probe — a client in another routing/    { cur = "container"; next }
    /cluster probe — a client outside/                 { cur = "cluster";   next }
    cur == "" { next }
    # Only the SCRIPT'"'"'s own verdict for the probe, not the sub-script'"'"'s
    # case lines: `nat_rewrite.sh` says "the vm probe ran: …" in one sentence,
    # and that sentence is what the harness quotes.
    /^  ok   the (vm|container|cluster) probe ran/   { emit(cur, "ran",     substr($0, 8)) }
    /^  skip /                                        { emit(cur, "skipped", substr($0, 8)) }
    /^  FAIL the (vm|container|cluster) probe ran/   { emit(cur, "failed",  substr($0, 8)) }
  ' <<<"$transcript"
}

# The lines of one probe's section, verbatim — what the harness quotes when a
# probe fails, so the reason travels with the verdict.
#
#   nat_probe_transcript <transcript> <vm|container|cluster>
nat_probe_transcript() {
  local transcript="$1" want="$2"
  awk -v want="$want" '
    /vm probe — a client on a real second host/        { cur = "vm";        next }
    /container probe — a client in another routing/    { cur = "container"; next }
    /cluster probe — a client outside/                 { cur = "cluster";   next }
    cur == want { print }
  ' <<<"$transcript"
}
