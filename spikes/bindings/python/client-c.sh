#!/usr/bin/env bash
# Cell: client × c. A generated **Python** client calls the C peer's server role.
#
# ── What it buys, and the bound the axis file set ───────────────────────────
#
# `spikes/bindings/AXES` decided this when the peer landed, refusing to answer it
# by declaring:
#
#     `independent` refutes coding errors and does NOT satisfy clause 6.
#
# The peer shares **no code** with `crates/` — an error on our side is not
# mirrored on the other, which is real evidence and more than `self` can offer —
# and shares the same reading of the same specification by the same process,
# which *a convention both ends apply cannot be refuted by a round trip* settles.
# So this prints no `observed` line and closes no clause, and says so rather than
# leaving a reader to work it out. `binding_suite.sh` already implements that:
# clause 2 and clause 6 both require `observed` from a **foreign** peer.
#
# ── Why the peer serves its own contract ────────────────────────────────────
#
# `spikes/c_peer.c` answers `IDL:orbweaver/CPeerEcho:1.0` with its type id
# compiled in, so `spikes/cpeer.idl` is that fact written where a compiler can
# read it. A fixture that took its identity from a flag would let a caller assert
# whatever it expected, and `_is_a` would be unfalsifiable here — a contract is
# what a target claims to be.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# The peer's server role exits on its own deadline, so this waits for the port
# FILE and then dials; a port file that never appears is a counted SKIPPED
# naming the peer, never a pass.
#
# *생성된 파이썬 클라이언트가 ORB를 링크하지 않는 C 프로그램을 호출한다. 사는 것은
# AXES가 정해 두었다 — 코딩 오류를 반증하지 clause 6을 충족하지 않는다. 피어는 자기
# 계약을 서비스하고, 그 타입 id는 컴파일되어 있다: 플래그로 정체를 받는 픽스처는
# `_is_a`를 반증 불가능하게 만든다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

[ -x "$ROOT/target/c_peer" ] || {
  echo "SKIPPED  the C peer is not built (target/c_peer) — run spikes/build_c_peer.sh."
  echo "SKIPPED  Unmeasured, not passing."
  exit 2
}

D=/tmp/orbweaver-python-cpeer
rm -rf "$D"; mkdir -p "$D"
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" >/dev/null 2>&1; done
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && wait "$pid" 2>/dev/null; done
}
trap cleanup EXIT INT TERM

if ! cargo run -q --bin gen-python -- --out "$D/site" spikes/cpeer.idl >"$D/gen.log" 2>&1 \
   || ! cargo build -q --bin orbweaver-py-bridge 2>>"$D/gen.log"; then
  echo "FAIL	gen-python or the bridge did not build"
  tail -5 "$D/gen.log"
  exit 1
fi

"$ROOT/target/c_peer" --role server --ior-file "$D/c.ior" --port-file "$D/c.port" \
    --deadline-s 30 >"$D/server.json" 2>"$D/server.err" &
PIDS+=("$!")
up=0
for _ in $(seq 1 150); do
  [ -s "$D/c.port" ] && { up=1; break; }
  sleep 0.1
done
if [ "$up" != 1 ]; then
  echo "SKIPPED  the C peer's server role published no port; nothing was measured."
  tail -3 "$D/server.err" 2>/dev/null
  exit 2
fi

out=$(python3 "$ROOT/spikes/bindings/python/CPeerAdd.py" "$D/site" spikes/cpeer.idl \
        "$D/c.ior" "$ROOT/target/debug/orbweaver-py-bridge" 2>&1); rc=$?
if [ "$rc" -ne 0 ] || ! grep -q "python cpeer: PASS" <<<"$out"; then
  echo "FAIL	the generated Python client did not complete its call (exit $rc)"
  tail -8 <<<"$out"
  exit 1
fi

printf 'note\ta generated Python client called a C program that links no ORB, over GIOP\n'
printf 'note\t`independent` refutes coding errors and closes no clause — spikes/bindings/AXES, which decided that when the peer landed\n'
exit 0
