#!/usr/bin/env bash
# One pass per GIOP version, for a cell that drives a JacORB peer.
#
# ── Why this is a library and not a paragraph in two cells ───────────────────
#
# `spikes/bindings/AXES` states the rule the acceptance suite is built on:
# *the suite is one suite, parameterised by language — never a copy.* On
# 2026-09-02 the two `client-jacorb.sh` cells disagreed about GIOP coverage and
# the reason was not a language difference: **Java's cell had this loop and
# Python's had none.** So Java read `1.1 1.2` and Python read `1.2`, and the
# suite's `neither` column carried the difference as though it were a fact about
# Python.
#
# It is not. The only language-specific part of a version pass is **which driver
# is run**, which is why that is the callback and everything else is here.
#
# ── What it does ────────────────────────────────────────────────────────────
#
# 1.2 is what JacORB publishes; 1.1 and 1.0 are reached by **republishing the
# profile**, because a peer's outbound version follows the profile it dialled —
# the same mechanism `spikes/jacorb_giop11.sh` uses.
#
# **A version the peer will not speak is a RESULT, not a failure.** Only the 1.2
# pass is required; the others report what happened, and the suite's `neither`
# column is where an unread version lands. A cell that treated an unread version
# as a failure would be asserting a peer's capability, which is not ours to
# assert.
#
# ── The contract ────────────────────────────────────────────────────────────
#
#   run_each_giop_version <base_ior> <workdir> <op> <callback>
#
# `callback` is invoked once per version as
#
#   <callback> <label> <tapped_ior> <tap_log>
#
# with `label` one of 2, 1, 0. It returns 0 when the pass completed, non-zero
# when it did not; on a non-zero from the **1.2** pass this function fails the
# cell, and on a non-zero from 1.1 or 1.0 it emits a `note` row carrying the
# callback's own last word — the reason travels with the result.
#
# The callback's stdout is captured, so it may print freely; what it prints is
# used as the reason when it fails.
#
# *한 버전당 한 번의 패스. 이것이 라이브러리인 이유는 버전 패스에서 언어에 따라
# 다른 부분이 **어떤 드라이버를 돌리는가** 하나뿐이기 때문이다 — 나머지가 셀마다
# 복사되어 있었고, 그 결과 자바는 `1.1 1.2`를 읽고 파이썬은 `1.2`만 읽으면서 그
# 차이가 파이썬에 대한 사실인 것처럼 `neither` 열에 실려 있었다. 아니다.
# **피어가 말하지 않는 버전은 실패가 아니라 결과다.***

# Set by the caller before sourcing, or defaulted here.
: "${ROOT:?run_each_giop_version needs ROOT}"

run_each_giop_version() {
  local base_ior="$1" dir="$2" op="$3" callback="$4"
  local minor label log out_ior tap_out tap_pid tapped run_out run_rc why

  for minor in "" 1 0; do
    label=${minor:-2}
    log="$dir/tap-$label.log"
    out_ior="$dir/tapped-$label.ior"
    tap_out="$dir/tap-$label.out"

    # **Two invocations and not an array.** macOS ships bash 3.2, where
    # `"${arr[@]}"` on an EMPTY array is an unbound variable under `set -u` — so
    # the 1.2 pass, the one with no `--minor`, died before the tap ever forked
    # and the cell reported "the recording tap did not come up", which was true
    # and pointed at the wrong thing. Kept as two calls on purpose.
    if [ -n "$minor" ]; then
      python3 "$ROOT/spikes/jacorb_giop11_tap.py" --ior "$base_ior" --out "$out_ior" \
              --log "$log" --op "$op" --minor "$minor" >"$tap_out" 2>&1 &
    else
      python3 "$ROOT/spikes/jacorb_giop11_tap.py" --ior "$base_ior" --out "$out_ior" \
              --log "$log" --op "$op" >"$tap_out" 2>&1 &
    fi
    tap_pid=$!
    tapped=0
    for _ in $(seq 1 150); do
      if [ -s "$out_ior" ] && grep -q "^READY" "$tap_out" 2>/dev/null; then tapped=1; break; fi
      sleep 0.1
    done
    if [ "$tapped" != 1 ]; then
      echo "FAIL	the recording tap did not come up at IIOP 1.$label, so no flag byte could be read"
      tail -5 "$tap_out" 2>/dev/null
      kill "$tap_pid" >/dev/null 2>&1
      return 1
    fi

    run_out=$("$callback" "$label" "$out_ior" "$log" 2>&1)
    run_rc=$?
    kill "$tap_pid" >/dev/null 2>&1

    if [ "$run_rc" -ne 0 ]; then
      if [ "$label" = 2 ]; then
        echo "FAIL	the driver did not complete its calls against JacORB at 1.2 (exit $run_rc)"
        tail -12 <<<"$run_out"
        return 1
      fi
      # **The EXCEPTION line, not the `raise` that produced it.** This pattern
      # is lifted from `spikes/bindings/python/client-omniorb.sh`, which already
      # carries the correction and the reason: a first draft there matched
      # `Error` and caught the source line out of a traceback, so the note
      # carried a fragment of `_rt.py` where the reason should have been.
      #
      # The first version of THIS library restated the Java cell's looser
      # pattern instead of lifting that one — and on the Python cell's first run
      # it produced exactly the defect already written down one file over:
      # `raise TransportError(reply["error"].get(...))`. A shared function that
      # restates one caller's assumption is a copy with a library's manners.
      #
      # Captured, then the first line taken off a HERESTRING — never
      # `grep … | head -1`, which is the early-exit consumer this repository has
      # a gate for.
      why=$(grep -E "^[A-Za-z._]+(Error|Exception): |^  FAIL" <<<"$run_out")
      why=$(head -1 <<<"$why")
      why=$(sed 's/[[:space:]]\{1,\}/ /g' <<<"$why")
      printf 'note\tIIOP 1.%s: the calls did not complete, so that version stays unread — a result, not a failure (%s)\n' \
             "$label" "${why:-no message}"
      continue
    fi
    # The pass completed: its tap log is the caller's to read orders from.
    printf 'RAN\t%s\t%s\n' "$label" "$log"
  done
  return 0
}
