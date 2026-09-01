#!/usr/bin/env bash
# Cell: client × c. A generated **Java** client calls the C peer's server role.
#
# ── What it buys, and the bound the axis file set ───────────────────────────
#
# `spikes/bindings/AXES` decided this when the peer landed, refusing to answer it
# by declaring:
#
#     `independent` refutes coding errors and does NOT satisfy clause 6.
#
# **This row's own prose said the opposite** — that such a cell would be *"the
# strongest form of clause 6 this grid can offer"* — and that was corrected on
# 2026-09-01, when the sibling `servant × c` cell landed. `binding_suite.sh`
# implements the axis file correctly and always did: clause 2 and clause 6 both
# require `observed` from a **foreign** peer. So this cell prints no `observed`
# line and closes no clause; what it refutes is a coding error on our side, since
# the peer shares no code with `crates/`.
#
# ── Why the peer serves its own contract ────────────────────────────────────
#
# `spikes/c_peer.c` answers `IDL:orbweaver/CPeerEcho:1.0` with its type id
# compiled in, and `spikes/cpeer.idl` is that fact where a compiler can read it.
# A fixture taking its identity from a flag would let a caller assert whatever it
# expected, and `_is_a` would be unfalsifiable here.
#
# ── The licence boundary ────────────────────────────────────────────────────
#
# No `org.omg.CORBA` is on this classpath and none can be: JDK 11 removed it
# (JEP 320) and the only one on this machine is JacORB's jar, which this cell
# does not name. The generated Java speaks AnyJSON to `orbweaver-py-bridge`,
# which speaks GIOP, and the peer at the other end links nothing but libSystem.
#
# *생성된 자바 클라이언트가 ORB를 링크하지 않는 C 프로그램을 호출한다. **이 행의
# 산문이 반대로 적고 있었고** 2026-09-01에 정정되었다 — `independent`는 코딩 오류를
# 반증하지 clause 6을 충족하지 않는다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
[ -x "$JH/bin/javac" ] && [ -x "$JH/bin/java" ] || {
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME. Unmeasured, not passing."
  exit 2
}
[ -x "$ROOT/target/c_peer" ] || {
  echo "SKIPPED  the C peer is not built (target/c_peer) — run spikes/build_c_peer.sh."
  echo "SKIPPED  Unmeasured, not passing."
  exit 2
}

D=/tmp/orbweaver-java-cpeer
rm -rf "$D"; mkdir -p "$D"
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" >/dev/null 2>&1; done
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && wait "$pid" 2>/dev/null; done
}
trap cleanup EXIT INT TERM

if ! cargo run -q --bin gen-java -- --out "$D/src" --package cpeer spikes/cpeer.idl \
     >"$D/gen.log" 2>&1 || ! cargo build -q --bin orbweaver-py-bridge 2>>"$D/gen.log"; then
  echo "FAIL	gen-java or the bridge did not build"
  tail -5 "$D/gen.log"
  exit 1
fi
find "$D/src" -name '*.java' >"$D/sources"
echo "$ROOT/spikes/bindings/java/CPeerAdd.java" >>"$D/sources"
if ! "$JH/bin/javac" -nowarn -encoding UTF-8 -d "$D/classes" @"$D/sources" \
     >"$D/javac.log" 2>&1; then
  echo "FAIL	javac refused what the emitter wrote"
  head -8 "$D/javac.log"
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

out=$("$JH/bin/java" -cp "$D/classes" CPeerAdd spikes/cpeer.idl "$D/c.ior" \
        "$ROOT/target/debug/orbweaver-py-bridge" 2>&1); rc=$?
if [ "$rc" -ne 0 ] || ! grep -q "java cpeer: PASS" <<<"$out"; then
  echo "FAIL	the generated Java client did not complete its call (exit $rc)"
  tail -8 <<<"$out"
  exit 1
fi

printf 'note\ta generated Java client called a C program that links no ORB, over GIOP; no org.omg.CORBA on the classpath\n'
printf 'note\t`independent` refutes coding errors and closes no clause — spikes/bindings/AXES, which decided that when the peer landed\n'
exit 0
