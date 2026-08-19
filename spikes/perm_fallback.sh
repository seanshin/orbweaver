#!/usr/bin/env bash
# perm_fallback.sh — the oracle that tells LOCATION_FORWARD from
# LOCATION_FORWARD_PERM: what a client does when the address it was forwarded
# to stops answering.
#
# 680aa41 made status 4 reachable and measured that a request *count* at the
# old address is 1 under either status, for our client and for omniORB's, so
# a count cannot be the oracle. What can is fallback-on-failure. CORBA Part 2
# §9.6 (Object Location; the text read is formal/2012-11-14, unchanged in 3.4):
#
#   "Once a connection based on location-forwarding information is closed, a
#    client can attempt to reuse the forwarding information it has, but, if
#    that fails, it shall restart the location process using the original
#    address specified in the initial object reference."
#
#   "... the usage of LOCATION_FORWARD_PERM ... behaves like the usage of
#    LOCATION_FORWARD ..., but when used by the server it also provides an
#    indication to the client that it may replace the old IOR with the new
#    IOR. ... both the old IOR and the new IOR are valid, but the new IOR is
#    preferred for future use."
#
# So after a temporary forward the client SHALL go back to the original when
# the new address fails; after a permanent one it MAY have replaced the
# original and so may not. A client that goes back under temporary and stays
# under permanent has distinguished the two; one that goes back under both is
# within the spec and has not.
#
#   ./spikes/perm_fallback.sh [--expect-temporary reask|stay|report]
#                             [--expect-permanent reask|stay|report]
#                             [--only omni|ours]
#
# Defaults: temporary is asserted "reask" (the spec's shall); permanent is
# "report" — ok when the peer stays (the two are distinguished), a note when
# it re-asks (spec-permitted, not distinguished). "stay" asserts the peer
# does not re-ask; "reask" on the permanent run is the negative control and
# must go red against a peer that stays. Measured 2026-08-19: omniORB 4.3.4
# re-asks under temporary and stays under permanent, so a harness may assert
# `--expect-permanent stay` and have a server that downgrades status 4 to 3
# go red through the peer's behaviour (that control was run: it does).
#
# The expectations apply to every client, ours included: our cells (from the
# Rust test, both byte orders) are judged with the same --expect-temporary and
# --expect-permanent as the peer's. Measured 2026-08-19, after Connection
# learned to keep its origin and Reference to cache a forward: both re-ask
# under temporary and stay under permanent — the same cells as omniORB's.
# Before that, Connection was Err under both and Reference re-asked under
# both, and those cells were printed as measured and not judged.
#
# What it does, per status:
#   1. Starts two spike-servers at two ephemeral ports: the target (ping
#      answers 2) and the original, which forwards ping() to the target's
#      published IOR file — LOCATION_FORWARD or _PERM — for as long as that
#      file exists, and serves ping() itself (answering 1) once it is gone.
#   2. Runs omniORB's Python client (spikes/perm_fallback_client.py, a separate
#      process, never linked): ping() once, which is forwarded and answered 2
#      by the target; then it pauses.
#   3. Kills the target BY PID, removes its IOR file, and lets the client go.
#   4. The client pings twice more. The original's log says how many requests
#      reached it after the death; the client's output says what it got.
#   Then runs crates/orbweaver-gen/tests/forward_fallback.rs — our two clients
#   (Connection, Reference/pool) in the same shape, both byte orders — and
#   judges its `cell` lines against the same expectations.
#
# Exit: 0 every cell as expected; 1 any cell failed or unmeasurable; 2 no
# failure but a fixture was absent (omniORB half SKIPPED, never ok).
#
# No harness lock is taken: every port is ephemeral, every fixture is killed
# by PID, and no fixed /tmp path is written, so a concurrent run_checks.sh
# cannot collide with this and it cannot collide with one.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

expect_temporary=reask
expect_permanent=report
only=""
while [ $# -gt 0 ]; do
  case "$1" in
    --expect-temporary) expect_temporary="$2"; shift 2 ;;
    --expect-permanent) expect_permanent="$2"; shift 2 ;;
    --only) only="$2"; shift 2 ;;
    -h|--help) sed -n '2,60p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
for v in "$expect_temporary" "$expect_permanent"; do
  case "$v" in reask|stay|report) ;; *) echo "expectation must be reask|stay|report, got '$v'" >&2; exit 2 ;; esac
done

fails=0
skipped=0
notes=0
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }
skip() { printf '  SKIPPED  %s\n' "$1"; skipped=$((skipped + 1)); }
note() { printf '  ..   %s\n' "$1"; notes=$((notes + 1)); }
hr()   { printf '\n\033[1m%s\033[0m\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1" >&2; exit 2; }; }
need cargo
need python3

# Every fixture this script starts, killed by PID on the way out.
pids=()
work=$(mktemp -d "${TMPDIR:-/tmp}/permfb-XXXXXX")
cleanup() {
  for p in "${pids[@]:-}"; do
    [ -n "$p" ] && kill "$p" 2>/dev/null
  done
  for p in "${pids[@]:-}"; do
    [ -n "$p" ] && wait "$p" 2>/dev/null
  done
  # PERMFB_KEEP=1 keeps the servers' logs and the client's output for a look
  # (with ORBtraceLevel=25 in the environment, omniORB says in its own words
  # what it did with each forward).
  if [ -n "${PERMFB_KEEP:-}" ]; then echo "kept: $work"; else rm -rf "$work"; fi
}
trap cleanup EXIT

# Waits — sleeping — until a file is non-empty, up to $2 seconds. 0 if it
# appeared, 1 if not. (CLAUDE.md: wait loops must sleep.)
wait_file() {
  local f="$1" deadline_s="${2:-20}" i=0 n
  n=$(( deadline_s * 20 ))
  while [ "$i" -lt "$n" ]; do
    [ -s "$f" ] && return 0
    sleep 0.05
    i=$((i + 1))
  done
  return 1
}

# Waits — sleeping — until PID $1 has exited, up to $2 seconds.
wait_pid_gone() {
  local p="$1" deadline_s="${2:-30}" i=0 n
  n=$(( deadline_s * 20 ))
  while [ "$i" -lt "$n" ]; do
    kill -0 "$p" 2>/dev/null || return 0
    sleep 0.05
    i=$((i + 1))
  done
  return 1
}

# Judges one cell against an expectation. $1 label, $2 status, $3 re-asked
# yes|no, $4 the measured detail, $5 the expectation.
judge() {
  local label="$1" st="$2" reasked="$3" detail="$4" want="$5"
  case "$want" in
    reask)
      if [ "$reasked" = yes ]; then pass "$label  $st: re-asked the original after the target died — $detail"
      else fail "$label  $st: did NOT re-ask the original (expected to) — $detail"; fi ;;
    stay)
      if [ "$reasked" = no ]; then pass "$label  $st: stayed on the dead address, did not re-ask (distinguished from temporary) — $detail"
      else fail "$label  $st: re-asked the original (expected to stay) — $detail"; fi ;;
    report)
      if [ "$reasked" = no ]; then pass "$label  $st: stayed on the dead address, did not re-ask (distinguished from temporary) — $detail"
      else note "$label  $st: re-asked the original — spec-permitted ('may replace'), NOT distinguished from temporary — $detail"; fi ;;
  esac
}

# ── The peer half: omniORB 4.3.4 against two spike-servers ───────────────────
run_omni() {
  hr "fallback-on-failure — omniORB's client, two spike-servers, the forwarded-to one killed"
  local importable
  importable=$(python3 -c "import omniORB; print(omniORB.__version__)" 2>/dev/null)
  if [ -z "$importable" ] || ! command -v omniidl >/dev/null 2>&1; then
    skip "omniORB temporary — fixture absent (python3 -c 'import omniORB' failed or no omniidl)"
    skip "omniORB permanent — fixture absent"
    return
  fi
  local label="omniORB $importable"

  # Built once, run by path, so the PID we hold is the server's own.
  if ! cargo build -q -p orbweaver-object --bin spike-server 2>"$work/build.err"; then
    fail "spike-server did not build: $(head -3 "$work/build.err")"
    return
  fi
  local target_dir bin
  target_dir=$(cargo metadata --format-version 1 --no-deps 2>/dev/null |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
  bin="$target_dir/debug/spike-server"
  [ -x "$bin" ] || { fail "spike-server not at $bin"; return; }

  for st in temporary permanent; do
    local d="$work/$st"
    mkdir -p "$d"
    local tior="$d/target.ior" oior="$d/original.ior" tlog="$d/target.log" olog="$d/original.log"
    local ready="$d/ready" go="$d/go" cout="$d/client.out" cerr="$d/client.err"

    ORBWEAVER_PING_ANSWER=2 "$bin" "$tior" 127.0.0.1 0 >"$tlog" 2>&1 &
    local tpid=$!
    pids+=("$tpid")
    if ! wait_file "$tior" 20; then
      fail "$label  $st: the target server did not publish an IOR ($(tail -2 "$tlog" | tr '\n' ' '))"
      continue
    fi
    ORBWEAVER_FORWARD_TO="$tior" ORBWEAVER_FORWARD_STATUS="$st" ORBWEAVER_PING_ANSWER=1 \
      "$bin" "$oior" 127.0.0.1 0 >"$olog" 2>&1 &
    local opid=$!
    pids+=("$opid")
    if ! wait_file "$oior" 20; then
      fail "$label  $st: the original server did not publish an IOR ($(tail -2 "$olog" | tr '\n' ' '))"
      kill "$tpid" 2>/dev/null; wait "$tpid" 2>/dev/null
      continue
    fi
    # The IOR file is written before the accept loop starts; give it a beat
    # (the same 0.3 s run_checks.sh gives spike-server).
    sleep 0.3

    python3 spikes/perm_fallback_client.py "$oior" "$ready" "$go" >"$cout" 2>"$cerr" &
    local cpid=$!
    pids+=("$cpid")
    if ! wait_file "$ready" 20; then
      fail "$label  $st: the client never made its first call — $(cat "$cout" "$cerr" | tr '\n' ' ' | cut -c1-300)"
      kill "$cpid" "$opid" "$tpid" 2>/dev/null; wait "$cpid" "$opid" "$tpid" 2>/dev/null
      continue
    fi
    local first
    first=$(sed -n 's/^call 1 -> //p' "$cout")
    local fwd_before tgt_before
    fwd_before=$(grep -c "forwarded ping()" "$olog")
    tgt_before=$(grep -c "served ping()" "$tlog")

    # The forwarded-to server dies, by PID; its IOR file goes with it, so the
    # original now serves ping() itself. Only then may the client go on.
    kill "$tpid" 2>/dev/null
    wait "$tpid" 2>/dev/null
    rm -f "$tior"
    touch "$go"
    if ! wait_pid_gone "$cpid" 40; then
      fail "$label  $st: the client did not finish after the target died — $(cat "$cout" | tr '\n' ' ')"
      kill "$cpid" "$opid" 2>/dev/null; wait "$cpid" "$opid" 2>/dev/null
      continue
    fi
    wait "$cpid"; local crc=$?
    # A moment for the original's log to be flushed after the last request.
    sleep 0.2
    local fwd_total served_here second third
    fwd_total=$(grep -c "forwarded ping()" "$olog")
    served_here=$(grep -c "served ping()" "$olog")
    second=$(sed -n 's/^call 2 -> //p' "$cout")
    third=$(sed -n 's/^call 3 -> //p' "$cout")
    kill "$opid" 2>/dev/null; wait "$opid" 2>/dev/null

    if [ "$crc" -ne 0 ] || [ "$first" != "2" ] || [ "$fwd_before" -lt 1 ] || [ "$tgt_before" -lt 1 ]; then
      fail "$label  $st: the forward was not followed (client rc=$crc, call 1 -> '${first:-none}', forwarded=$fwd_before, at target=$tgt_before) — $(tr '\n' ' ' <"$cerr" | cut -c1-300)"
      continue
    fi
    local after reasked
    after=$(( served_here + fwd_total - fwd_before ))
    if [ "$after" -ge 1 ]; then reasked=yes; else reasked=no; fi
    judge "$label" "$st" "$reasked" \
      "requests at the original: $fwd_before before (forwarded), $after after ($served_here served there); at the target: $tgt_before; calls 1->$first 2->${second:-?} 3->${third:-?}" \
      "$( [ "$st" = temporary ] && echo "$expect_temporary" || echo "$expect_permanent" )"
  done
}

# ── Our half: crates/orbweaver-gen/tests/forward_fallback.rs ─────────────────
run_ours() {
  hr "fallback-on-failure — our clients (Connection, Reference), both byte orders"
  local out
  out=$(cargo test -q -p orbweaver-gen --test forward_fallback -- --nocapture 2>&1)
  # `grep -o`, not `^cell`: the test harness prints its progress dot on the
  # same line as whatever the test printed next, so one cell in eight arrived
  # as `.cell ...` and a `^cell` match dropped it (found on the first run).
  # Sorted so the two tests' interleaved output reads as a matrix — client,
  # then temporary before permanent, then byte order.
  local cells
  cells=$(printf '%s\n' "$out" | grep -o 'cell client=.*' | sort -s -t' ' -k2,2 -k3,3r -k4,4)
  case "$out" in
    *"test result: ok."*)
      if [ -z "$cells" ]; then
        fail "forward_fallback passed but printed no cells — nothing measured"
        return
      fi
      # Every cell of ours is judged against the same expectations as the
      # peer's: §9.6's shall applies to us too, and the permanent arm is
      # whatever the caller asked of the peer (report by default, stay for
      # a harness that has measured its peer to stay — ours stays).
      # A here-string, not a pipe: the counters judge() bumps must survive
      # the loop, and a pipe would put it in a subshell.
      while IFS= read -r line; do
        local client st endian reasked after second
        client=$(printf '%s' "$line" | sed -n 's/.*client=\([^ ]*\).*/\1/p')
        st=$(printf '%s' "$line" | sed -n 's/.*status=\([^ ]*\).*/\1/p')
        endian=$(printf '%s' "$line" | sed -n 's/.*endian=\([^ ]*\).*/\1/p')
        reasked=$(printf '%s' "$line" | sed -n 's/.*reasked=\([^ ]*\).*/\1/p')
        after=$(printf '%s' "$line" | sed -n 's/.*requests_at_original_after_death=\([^ ]*\).*/\1/p')
        second=$(printf '%s' "$line" | sed -n 's/.*second_call=\(.*\)$/\1/p')
        judge "ours $client ($endian)" "$st" "$reasked" "requests at original after death=$after, next call=$second" \
          "$( [ "$st" = temporary ] && echo "$expect_temporary" || echo "$expect_permanent" )"
      done <<< "$cells"
      ;;
    *)
      fail "forward_fallback: the test went red"
      printf '%s\n' "$out" | grep -E "panicked|left:|right:|error" | head -6 | sed 's/^/       /'
      ;;
  esac
}

case "$only" in
  omni) run_omni ;;
  ours) run_ours ;;
  "") run_omni; run_ours ;;
  *) echo "--only takes omni or ours" >&2; exit 2 ;;
esac

echo
echo "perm_fallback: failures=$fails skipped=$skipped notes=$notes"
if [ "$fails" -gt 0 ]; then exit 1; fi
if [ "$skipped" -gt 0 ]; then exit 2; fi
exit 0
