#!/usr/bin/env bash
# Cell: client × jacorb. Generated **Java** invokes a JacORB target holding only
# a reference, and the byte order is READ OFF §15.4.1's flag byte of what JacORB
# actually wrote.
#
# ── Why this cell exists / 이 칸의 이유 ──────────────────────────────────────
#
# D030 §3.1's table has two columns and they do not agree: Python's servant
# direction meets all three of D030 §3's clauses and **its client direction does
# not** — `python_target.rs` walks both orders through a loopback with no peer in
# it, and the live client leg's peer is omniORB, which writes its host's native
# order. `spikes/bindings/python.manifest` states that gap as a `waits` row:
#
#   "Nothing drives generated Python at a JacORB server, and that is precisely
#    the cell that would give the client direction a big-endian reading off a
#    foreign peer's flag byte."
#
# This is that cell, one language over. JacORB is the only peer in the grid that
# writes big-endian, `spikes/jacorb/Server.java` is a JacORB **server** for
# `spike::Echo`, and the tap in front of it reads every reply's flag byte. So the
# client direction gets what the servant direction already had: an order that was
# read rather than believed.
#
# ── The licence boundary / 라이선스 경계 ────────────────────────────────────
#
# JacORB is **LGPL and a fixture, never a dependency**. It is started as a
# separate process and read from over TCP; nothing under `crates/` links it, and
# the generated Java does not import `org.omg.CORBA` at all — it cannot, because
# JDK 11 removed one (JEP 320) and the only one on this machine is JacORB's own
# jar. This cell asserts that with `cargo tree --workspace`, **reading the
# producer's exit status before anything it printed**, because a `cargo tree`
# that could not run is an unmeasured check and an unmeasured check is a failure.
#
# ── Exit ────────────────────────────────────────────────────────────────────
#
#   0  the calls completed and at least one order was read off the wire
#   1  a fixture that is present would not start, or a reading is missing
#   2  the JDK, the JacORB jars or python3 are absent — unmeasured, not passing
#
# *생성된 Java가 JacORB 서버를 호출하고, 바이트 순서는 피어가 실제로 쓴 플래그
# 바이트에서 읽는다. JacORB는 별개 프로세스로 도는 픽스처이며 의존성이 아니다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
JDIR="$ROOT/spikes/jacorb"
D=/tmp/orbweaver-java-jacorb
JARS="$JDIR/lib/jacorb.jar:$JDIR/lib/jacorb-omgapi.jar:$JDIR/lib/jboss-rmi-api.jar:$JDIR/lib/slf4j-api-1.7.36.jar"
JCP="$JARS:$JDIR/classes"

# ── fixtures, absent or present ─────────────────────────────────────────────
[ -x "$JH/bin/java" ] && [ -x "$JH/bin/javac" ] || {
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME. Unmeasured, not passing."
  exit 2
}
command -v python3 >/dev/null 2>&1 || {
  echo "SKIPPED  python3 absent, so the recording tap cannot run and no flag byte"
  echo "SKIPPED  could be read. Unmeasured, not passing."
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
#
# The names are NOT written here. This was the **third** copy of one rule —
# after `spikes/run_checks.sh` and `.github/workflows/ci.yml` — and it was
# found on 2026-08-27 by the sweep that repaired the other two, only because
# that sweep was scoped to the rule rather than to the files that had the
# incident. All three carried `omniorb|jacorb`; two of them had already drifted
# apart on whether TAO was in it. `spikes/licence_boundary.sh` owns the pattern
# and the producer-status discipline now.
# Resolved from `$0`, not from the caller's CWD: this script has no `cd` of its
# own and is run by hand, so a bare relative path would find the gate only when
# the caller happened to stand in the right directory — and a gate that cannot
# be found is an unmeasured check, which is a failure and never a pass.
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
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" >/dev/null 2>&1
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && wait "$pid" 2>/dev/null
  done
}
trap cleanup EXIT INT TERM

# ── the JacORB server ───────────────────────────────────────────────────────
( cd "$JDIR" && exec "$JH/bin/java" -cp "$JCP" Server "$D/j.ior" >"$D/server.log" 2>&1 ) &
PIDS+=("$!")
# See spikes/lib/accepting.sh. JacORB's own READY line is printed after
# the_POAManager().activate() and after the file, so it is strictly later than
# what this used to wait for — and the TCP connect is later still.
. "$ROOT/spikes/lib/accepting.sh"
started=0
wait_accepting "$D/j.ior" --deadline 30 --ready "$D/server.log" "^READY$" && started=1
if [ "$started" != 1 ]; then
  echo "FAIL	the JacORB fixture is installed but published no IOR within 15s —"
  echo "FAIL	a fixture that will not start is a failure, not a skip"
  tail -5 "$D/server.log" 2>/dev/null
  exit 1
fi

# ── one pass per GIOP version ───────────────────────────────────────────────
#
# **The loop is `spikes/lib/giop_versions.sh`, not this file** (since
# 2026-09-03). It used to live here, and the Python cell of the same name had
# none — so Java read `1.1 1.2` and Python read `1.2`, and the suite's `neither`
# column carried that as though it were a fact about Python. It was a fact about
# which cell had been written second. AXES: *one suite, parameterised by
# language, never a copy.*
#
# What is language-specific is the callback below and nothing else.
. "$ROOT/spikes/lib/tap_orders.sh"
. "$ROOT/spikes/lib/giop_versions.sh"

# The client is built once; only the profile it dials changes.
if ! cargo run -q --bin gen-java -- --out "$D/src" --package echo spikes/echo.idl \
     >"$D/gen.log" 2>&1 || ! cargo build -q --bin orbweaver-py-bridge 2>>"$D/gen.log"; then
  echo "FAIL	gen-java or the bridge did not build"
  tail -5 "$D/gen.log"
  exit 1
fi
find "$D/src" -name '*.java' >"$D/sources"
echo "$ROOT/spikes/bindings/java/EchoClient.java" >>"$D/sources"
if ! "$JH/bin/javac" -nowarn -encoding UTF-8 -d "$D/classes" @"$D/sources" \
     >"$D/javac.log" 2>&1; then
  echo "FAIL	javac refused the generated client"
  tail -12 "$D/javac.log"
  exit 1
fi

# The one language-specific line. `$1` is the version label, `$2` the tapped
# IOR; the run's own output is kept so the 1.2 pass can be counted.
drive_java() {
  local out
  out=$("$JH/bin/java" -Dfile.encoding=UTF-8 -cp "$D/classes" EchoClient \
        spikes/echo.idl "$2" "$ROOT/target/debug/orbweaver-py-bridge" \
        $([ "$1" = 0 ] && echo --no-wide) 2>&1)
  local rc=$?
  printf '%s\n' "$out" >"$D/run-$1.out"
  printf '%s\n' "$out"
  [ "$rc" -eq 0 ] && grep -q "java target: PASS" <<<"$out"
}

passes=$(run_each_giop_version "$D/j.ior" "$D" echo_string drive_java) || {
  printf '%s\n' "$passes"
  exit 1
}
readings=""
calls=0
while IFS=$'\t' read -r kind a b; do
  case "$kind" in
    RAN)
      readings="$readings$(read_reply_orders "$b")
"
      [ "$a" = 2 ] && calls=$(grep -c '^  ok' "$D/run-2.out")
      ;;
    note) printf 'note\t%s\n' "$a" ;;
  esac
done <<<"$passes"

# ── what the peer wrote, read off §15.4.1's flag byte ───────────────────────
# The REPLIES: in the client direction the peer is the one answering, so its
# order is the order of what it wrote. `spikes/lib/tap_orders.sh` owns the
# reading and the two parsing bugs it has already shipped.
# Herestrings, not pipes. `printf … | grep -q` is named in CLAUDE.md as having
# the same defect as the form it looks like a repair for: `grep -q` exits on the
# first match and SIGPIPEs the producer, and under `pipefail` that becomes "no
# match". The harness's own gate caught both of these on the run that landed
# them.
sort -u <<<"$(grep -E "^observed" <<<"$readings")"
if ! grep -q "^observed" <<<"$readings"; then
  echo "FAIL	no order was read off the wire in any pass, so this cell measured nothing"
  exit 1
fi
note_request_orders "$D/tap-2.log"

printf 'note\tIIOP 1.0 was driven WITHOUT the wide case, and the reading is about version and order rather than about every operation: with it, JacORB answers MARSHAL — the PEER declining a wstring at 1.0, which is a different finding from our own stack refusing, and both were measured 2026-09-03\n'
printf 'note\t%s generated call(s) completed over the wire against JacORB, no Rust stub and no org.omg.CORBA in the path\n' "$calls"
printf 'note\tD030 §3.1 records the client direction as "not established"; this cell is a reading off a foreign peer in that direction\n'
exit 0
