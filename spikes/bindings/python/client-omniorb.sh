#!/usr/bin/env bash
# Cell: client × omniorb. Generated Python invokes a C++ target holding only a
# reference, with no Rust stub anywhere in the path.
#
# ── The migration's oracle ──────────────────────────────────────────────────
#
# The three commands below are the harness group `Python client target`'s first
# leg, **unchanged**: the same `gen-python` invocation, the same
# `orbweaver-py-bridge` build, the same `echo_client.py` arguments, the same
# `python target: PASS` verdict string, the same `grep -c '^  ok'` count. B3
# requires today's group to produce byte-identical results as an instance of the
# suite, and the way to guarantee that is not to re-derive the leg but to run it.
# What this file adds is the fixture's lifetime and the observation vocabulary.
#
# ── Why the order is `claimed` ──────────────────────────────────────────────
#
# Same reason as `servant-omniorb.sh`, one direction over: nothing here reads a
# flag byte. There is a sharper point in this direction, though, and the suite
# prints it — the client direction has NO cell that reads an order off the wire
# at all, because the only other client cell is `python_target.rs`, which never
# opens a socket. D030 §3.1's client column, mechanically.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# Started and stopped here so the cell runs standalone. The wait loop SLEEPS —
# a `for i in $(seq 1 500); do [ -f f ] && break; done` finishes in microseconds
# and does not wait at all, which is the harness rule that cost this project its
# first phantom failure. Absent omniORB is exit 2 (unmeasured), a fixture that
# is present and will not start is exit 1 (a failure), which is the distinction
# D010 §2 turns on.
#
# *하네스 그룹의 첫 다리를 그대로 실행한다 — 이관의 오라클은 다시 유도하는 것이
# 아니라 같은 것을 돌리는 것이다. 대기 루프는 반드시 잠든다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

command -v python3 >/dev/null 2>&1 || { echo "SKIPPED  python3 absent"; exit 2; }
if ! python3 -c 'import omniORB' >/dev/null 2>&1; then
  echo "SKIPPED  omniORB's Python bindings are not importable, so there is no foreign"
  echo "SKIPPED  target for generated Python to call; unmeasured, not passing"
  exit 2
fi

fixture_down() { pkill -f echo_server.py >/dev/null 2>&1 || true; }
trap fixture_down EXIT
fixture_down
rm -f "$ROOT/spikes/echo.ior"
( cd "$ROOT/spikes" && exec python3 echo_server.py >/tmp/orbweaver-binding-fixture.log 2>&1 & )

# A published IOR is not an accepting listener; the rule and its probe live in
# spikes/lib/accepting.sh. This cell had its own copy of the fixed-guess wait,
# which is why the sweep that fixed the harness in 81cc546 never reached it.
. "$ROOT/spikes/lib/accepting.sh"
started=0
wait_accepting "$ROOT/spikes/echo.ior" --deadline 15 && started=1
if [ "$started" != 1 ]; then
  echo "FAIL	the omniORB fixture is installed but published no IOR within 10s —"
  echo "FAIL	a fixture that will not start is a failure, not a skip"
  tail -5 /tmp/orbweaver-binding-fixture.log 2>/dev/null
  exit 1
fi

pyout=/tmp/orbweaver-binding-pytarget; rm -rf "$pyout"; mkdir -p "$pyout"
if ! cargo run -q --bin gen-python -- --out "$pyout" spikes/echo.idl >/dev/null 2>&1 \
   || ! cargo build -q --bin orbweaver-py-bridge 2>/dev/null; then
  echo "FAIL	gen-python or the bridge did not build"
  exit 1
fi

# ── one pass per GIOP version, each through a recording tap ─────────────────
#
# **1.2 is what the fixture publishes; 1.1 and 1.0 are reached by republishing
# the profile**, because a peer's outbound version follows the profile it
# dialled. Without the passes the suite reads `client: … neither[1.0 1.1]`, and
# a version nobody read is the same kind of not-a-measurement as an order nobody
# read. A version this pair cannot carry is a RESULT: only the 1.2 pass must
# pass, and the others say what happened.
. "$ROOT/spikes/lib/tap_orders.sh"
D=/tmp/orbweaver-binding-pyclient; rm -rf "$D"; mkdir -p "$D"
TAPS=()
tap_down() { for t in "${TAPS[@]:-}"; do [ -n "$t" ] && kill "$t" >/dev/null 2>&1; done; fixture_down; }
trap tap_down EXIT

calls=0
readings=""
for minor in "" 1 0; do
  label=${minor:-2}
  log="$D/tap-$label.log"
  out_ior="$D/tapped-$label.ior"
  tap_out="$D/tap-$label.out"
  # Two invocations, not an array: macOS ships bash 3.2, where `"${arr[@]}"` on
  # an EMPTY array is an unbound variable under `set -u` — the defect that made
  # the Java cell's 1.2 pass die before its tap ever forked.
  if [ -n "$minor" ]; then
    python3 spikes/jacorb_giop11_tap.py --ior "$ROOT/spikes/echo.ior" --out "$out_ior" \
            --log "$log" --op echo_string --minor "$minor" >"$tap_out" 2>&1 &
  else
    python3 spikes/jacorb_giop11_tap.py --ior "$ROOT/spikes/echo.ior" --out "$out_ior" \
            --log "$log" --op echo_string >"$tap_out" 2>&1 &
  fi
  TAPS+=("$!")
  tapped=0
  for _ in $(seq 1 150); do
    if [ -s "$out_ior" ] && grep -q "^READY" "$tap_out" 2>/dev/null; then tapped=1; break; fi
    sleep 0.1
  done
  if [ "$tapped" != 1 ]; then
    echo "FAIL	the recording tap did not come up at IIOP 1.$label, so no flag byte could be read"
    tail -5 "$tap_out" 2>/dev/null
    exit 1
  fi

  pyrun=$(python3 crates/orbweaver-gen/python/echo_client.py "$pyout" \
          spikes/echo.idl "$out_ior" ./target/debug/orbweaver-py-bridge 2>&1)
  case "$pyrun" in
    *"python target: PASS"*)
      [ "$label" = 2 ] && calls=$(grep -c '^  ok' <<<"$pyrun")
      readings="$readings$(read_reply_orders "$log")
"
      ;;
    *)
      if [ "$label" = 2 ]; then
        echo "FAIL	the Python client did not complete its calls"
        printf '%s\n' "$pyrun" | tail -12
        exit 1
      fi
      # The EXCEPTION line, not the `raise` that produced it. The first draft
      # matched `Error` and caught the source line out of the traceback, so the
      # note carried a fragment of `_rt.py` where the reason should have been —
      # the truncated-read class, in the sentence that explains a result.
      why_all=$(grep -E "^[A-Za-z._]+(Error|Exception): |^  FAIL" <<<"$pyrun")
      why=$(head -1 <<<"$why_all")
      # **Whose refusal it is matters and the note says which.** At 1.1 and 1.0
      # against this peer the answer comes back as a SystemException from
      # omniORB — its own vendor minor code — which is the peer declining, not
      # our stack failing. The Java client cell's 1.0 is the other kind: our own
      # runtime refusing per §9.3.1.6 because the driver carries wide text. Both
      # leave the version unread and they are not the same finding.
      printf 'note\tIIOP 1.%s: the calls did not complete, so that version stays unread — a result, not a failure (%s)\n' "$label" "${why:-no message; see the pass output}"
      ;;
  esac
done

# ── what the peer wrote, read off §15.4.1's flag byte ───────────────────────
# The REPLIES: in the client direction the peer is the one answering, so its
# order is the order of what it wrote. This cell reported `claimed` here until
# 2026-09-02 and said why — no tap sat between the generated Python and the
# fixture. One does now.
sort -u <<<"$(grep -E "^observed" <<<"$readings")"
if ! grep -q "^observed" <<<"$readings"; then
  echo "FAIL	no order was read off the wire in any pass, so this cell measured nothing"
  exit 1
fi
printf 'note\t%s generated call(s) completed over the wire at 1.2, no Rust stub in the path\n' "$calls"
exit 0
