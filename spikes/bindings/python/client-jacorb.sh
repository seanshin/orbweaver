#!/usr/bin/env bash
# Cell: client × jacorb. Generated **Python** invokes a JacORB target holding
# only a reference, and the byte order is READ OFF §15.4.1's flag byte of what
# JacORB actually wrote.
#
# ── Why this cell exists / 이 칸의 이유 ──────────────────────────────────────
#
# `spikes/bindings/python.manifest` has stated this gap as a `waits` row since
# the suite was written, and stated it precisely:
#
#   "a JacORB SERVER that generated Python dials. Every JacORB leg in this tree
#    has JacORB as the CLIENT … or drives a JacORB server from a RUST client.
#    Nothing drives generated Python at a JacORB server, and that is precisely
#    the cell that would give the client direction a big-endian reading off a
#    foreign peer's flag byte."
#
# D030 §3.1 records the same asymmetry: Python's **servant** direction meets all
# three of D030 §3's clauses and its **client** direction does not — the
# both-orders test is a loopback with no peer in it, and the live client leg's
# peer is omniORB, which writes its host's native order. JacORB is the only peer
# in this grid that writes big-endian.
#
# This is that cell. The Java one for the same peer landed first
# (`spikes/bindings/java/client-jacorb.sh`) and is the model; what differs is the
# four lines that generate and drive Python instead of Java. The reading itself
# is not restated — `spikes/lib/tap_orders.sh` owns it, lifted there when this
# cell needed the identical parse, along with the two bugs that reading has
# already shipped.
#
# ── The licence boundary / 라이선스 경계 ────────────────────────────────────
#
# JacORB is **LGPL and a fixture, never a dependency**: a separate process read
# from over TCP, with nothing under `crates/` linking it. Asserted here through
# `spikes/licence_boundary.sh`, which owns the pattern so that this is not a
# fourth copy of one rule — the third copy was found in 2026-08-27's sweep only
# because that sweep was scoped to the rule and not to the files that had had
# the incident.
#
# ── Exit ────────────────────────────────────────────────────────────────────
#
#   0  the calls completed and at least one order was read off the wire
#   1  a fixture that is present would not start, or a reading is missing
#   2  the JDK, the JacORB jars or python3 are absent — unmeasured, not passing
#
# *생성된 Python이 JacORB 서버를 호출하고, 바이트 순서는 피어가 실제로 쓴 플래그
# 바이트에서 읽는다. 매니페스트가 이 칸을 "클라이언트 방향에 빅엔디언 판독을 줄
# 바로 그 칸"이라고 적어 둔 지 오래다. 읽기 자체는 다시 적지 않는다 —
# `spikes/lib/tap_orders.sh`가 소유한다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
JDIR="$ROOT/spikes/jacorb"
D=/tmp/orbweaver-python-jacorb
JCP="$JDIR/lib/jacorb.jar:$JDIR/lib/jacorb-omgapi.jar:$JDIR/lib/jboss-rmi-api.jar:$JDIR/lib/slf4j-api-1.7.36.jar:$JDIR/classes"

# ── fixtures, absent or present ─────────────────────────────────────────────
[ -x "$JH/bin/java" ] || {
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME. Unmeasured, not passing."
  exit 2
}
command -v python3 >/dev/null 2>&1 || {
  echo "SKIPPED  python3 absent, so neither the client nor the recording tap can"
  echo "SKIPPED  run and no flag byte could be read. Unmeasured, not passing."
  exit 2
}
for jar in "$JDIR/lib/jacorb.jar" "$JDIR/lib/jacorb-omgapi.jar" \
           "$JDIR/lib/jboss-rmi-api.jar" "$JDIR/lib/slf4j-api-1.7.36.jar"; do
  [ -s "$jar" ] || {
    echo "SKIPPED  JacORB fixture absent ($(basename "$jar")) — run spikes/jacorb/setup.sh"
    exit 2
  }
done
[ -s "$JDIR/classes/Server.class" ] || {
  echo "SKIPPED  JacORB's spike::Echo server is not compiled — run spikes/jacorb/setup.sh"
  exit 2
}

# ── the licence gate ────────────────────────────────────────────────────────
lb_out=$("$(dirname "$0")/../../licence_boundary.sh" 2>&1); lb_rc=$?
if [ "$lb_rc" -eq 1 ]; then
  echo "FAIL	a fixture has become a dependency — cargo tree names it:"
  head -4 <<<"$lb_out"
  exit 1
elif [ "$lb_rc" -ne 0 ]; then
  echo "FAIL	the licence boundary is UNMEASURED (exit $lb_rc)"
  head -4 <<<"$lb_out"
  exit 1
fi

rm -rf "$D"; mkdir -p "$D"
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" >/dev/null 2>&1; done
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && wait "$pid" 2>/dev/null; done
}
trap cleanup EXIT INT TERM

# ── the JacORB server ───────────────────────────────────────────────────────
( cd "$JDIR" && exec "$JH/bin/java" -cp "$JCP" Server "$D/j.ior" >"$D/server.log" 2>&1 ) &
PIDS+=("$!")
. "$ROOT/spikes/lib/accepting.sh"
started=0
wait_accepting "$D/j.ior" --deadline 30 --ready "$D/server.log" "^READY$" && started=1
if [ "$started" != 1 ]; then
  echo "FAIL	the JacORB fixture is installed but never accepted —"
  echo "FAIL	a fixture that will not start is a failure, not a skip"
  tail -5 "$D/server.log" 2>/dev/null
  exit 1
fi

# ── the tap, so the order is read rather than assumed ───────────────────────
python3 spikes/jacorb_giop11_tap.py --ior "$D/j.ior" --out "$D/tapped.ior" \
        --log "$D/tap.log" --op echo_string >"$D/tap.out" 2>&1 &
PIDS+=("$!")
tapped=0
for _ in $(seq 1 150); do
  if [ -s "$D/tapped.ior" ] && grep -q "^READY" "$D/tap.out" 2>/dev/null; then
    tapped=1; break
  fi
  sleep 0.1
done
if [ "$tapped" != 1 ]; then
  echo "FAIL	the recording tap did not come up, so no flag byte could be read"
  tail -5 "$D/tap.out" 2>/dev/null
  exit 1
fi

# ── the generated Python client ─────────────────────────────────────────────
if ! cargo run -q --bin gen-python -- --out "$D/site" spikes/echo.idl >"$D/gen.log" 2>&1 \
   || ! cargo build -q --bin orbweaver-py-bridge 2>>"$D/gen.log"; then
  echo "FAIL	gen-python or the bridge did not build"
  tail -5 "$D/gen.log"
  exit 1
fi

# The same driver the omniORB cell uses, pointed at the TAPPED ior — so what
# differs between the two client cells is the peer and nothing else.
run_out=$(python3 crates/orbweaver-gen/python/echo_client.py "$D/site" \
          spikes/echo.idl "$D/tapped.ior" "$ROOT/target/debug/orbweaver-py-bridge" 2>&1)
run_rc=$?
case "$run_out" in
  *"python target: PASS"*) ;;
  *) echo "FAIL	the Python client did not complete its calls against JacORB (exit $run_rc)"
     tail -12 <<<"$run_out"
     exit 1 ;;
esac
calls=$(grep -c '^  ok' <<<"$run_out")

# ── what the peer wrote, read off §15.4.1's flag byte ───────────────────────
. "$ROOT/spikes/lib/tap_orders.sh"
read_reply_orders "$D/tap.log" || exit 1
note_request_orders "$D/tap.log"

printf 'note\t%s generated call(s) completed over the wire against JacORB, no Rust stub in the path\n' "$calls"
printf 'note\tD030 §3.1 records the Python client direction as not established off a foreign peer; this cell is that reading\n'
exit 0
