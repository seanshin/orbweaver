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

started=0
for _ in $(seq 1 100); do
  if [ -s "$ROOT/spikes/echo.ior" ]; then sleep 0.2; started=1; break; fi
  sleep 0.1
done
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

pyrun=$(python3 crates/orbweaver-gen/python/echo_client.py "$pyout" \
        spikes/echo.idl spikes/echo.ior ./target/debug/orbweaver-py-bridge 2>&1)

case "$pyrun" in
  *"python target: PASS"*) ;;
  *) echo "FAIL	the Python client did not complete its calls"
     printf '%s\n' "$pyrun" | tail -12
     exit 1 ;;
esac

calls=$(grep -c '^  ok' <<<"$pyrun")
printf 'claimed\tgiop=1.2\torder=little\tomniORB writes its host'"'"'s native order; no tap sits between the generated Python and the fixture, so no flag byte is read in this direction by any cell\n'
printf 'note\t%s generated call(s) completed over the wire, no Rust stub in the path\n' "$calls"
exit 0
