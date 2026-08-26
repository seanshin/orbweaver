#!/usr/bin/env bash
# Cell: client × omniorb. Generated **Java** invokes a C++ target holding only a
# reference, with no Rust stub and no `org.omg.CORBA` anywhere in the path.
#
# ── What differs from the Python cell of the same name ──────────────────────
#
# `spikes/bindings/python/client-omniorb.sh` reports `claimed`, and says why:
# *"omniORB writes its host's native order; no tap sits between the generated
# Python and the fixture, so no flag byte is read in this direction by any
# cell."* This one puts the tap there. The order is then **little because the
# flag byte says so**, not because the host is little-endian — which is the whole
# distinction the suite turns on, and it costs one process.
#
# That is not a criticism of the Python cell: its first leg is the migration's
# oracle and had to be run unchanged. It is what a second language could add
# without touching the first.
#
# ── Exit ────────────────────────────────────────────────────────────────────
#
#   0  the calls completed and the order was read off §15.4.1's flag byte
#   1  a fixture that is present would not start, or a reading is missing
#   2  the JDK, omniORB's Python bindings or python3 are absent
#
# *같은 이름의 Python 칸은 `claimed`를 보고하며 그 이유를 적는다 — 탭이 없기
# 때문이다. 이 칸은 탭을 둔다. 그러면 순서는 호스트가 아니라 플래그 바이트가 말한다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
D=/tmp/orbweaver-java-omniorb

[ -x "$JH/bin/java" ] && [ -x "$JH/bin/javac" ] || {
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME. Unmeasured, not passing."
  exit 2
}
command -v python3 >/dev/null 2>&1 || { echo "SKIPPED  python3 absent"; exit 2; }
if ! python3 -c 'import omniORB' >/dev/null 2>&1; then
  echo "SKIPPED  omniORB's Python bindings are not importable, so there is no foreign"
  echo "SKIPPED  target for generated Java to call; unmeasured, not passing"
  exit 2
fi

rm -rf "$D"; mkdir -p "$D"
PIDS=()
cleanup() {
  pkill -f echo_server.py >/dev/null 2>&1
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" >/dev/null 2>&1
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] && wait "$pid" 2>/dev/null
  done
}
trap cleanup EXIT INT TERM
pkill -f echo_server.py >/dev/null 2>&1
rm -f "$ROOT/spikes/echo.ior"

# The wait loop SLEEPS. `for i in $(seq 1 500); do [ -f f ] && break; done`
# finishes in microseconds and does not wait at all — the harness rule that cost
# this project its first phantom failure.
( cd "$ROOT/spikes" && exec python3 echo_server.py >"$D/fixture.log" 2>&1 ) &
PIDS+=("$!")
started=0
for _ in $(seq 1 100); do
  if [ -s "$ROOT/spikes/echo.ior" ]; then sleep 0.2; started=1; break; fi
  sleep 0.1
done
if [ "$started" != 1 ]; then
  echo "FAIL	the omniORB fixture is installed but published no IOR within 10s —"
  echo "FAIL	a fixture that will not start is a failure, not a skip"
  tail -5 "$D/fixture.log" 2>/dev/null
  exit 1
fi

python3 spikes/jacorb_giop11_tap.py --ior "$ROOT/spikes/echo.ior" --out "$D/tapped.ior" \
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
  echo "FAIL	javac refused what the emitter wrote"
  head -8 "$D/javac.log"
  exit 1
fi

run_out=$("$JH/bin/java" -Dfile.encoding=UTF-8 -cp "$D/classes" EchoClient \
          spikes/echo.idl "$D/tapped.ior" "$ROOT/target/debug/orbweaver-py-bridge" 2>&1)
run_rc=$?
if [ "$run_rc" -ne 0 ] || ! grep -q "java target: PASS" <<<"$run_out"; then
  echo "FAIL	the Java client did not complete its calls against omniORB (exit $run_rc)"
  tail -12 <<<"$run_out"
  exit 1
fi
calls=$(grep -c '^  ok' <<<"$run_out")

replies=$(grep "S->C GIOP" "$D/tap.log" | grep " Reply ")
if [ -z "$replies" ]; then
  echo "FAIL	the calls completed but the tap recorded no reply, so the byte order"
  echo "FAIL	was NOT read off the wire. An absent reading cannot count as covered."
  exit 1
fi
seen=""
while IFS= read -r line; do
  v=$(sed -n 's/.*GIOP \([0-9]\.[0-9]\).*/\1/p' <<<"$line")
  # The flag byte, as the tap wrote it: `... Reply size=16 BE id=1 status=0`.
  # A `case` and not a `sed`: the tap puts the request id and the operation
  # AFTER the order, so an end-anchored match found nothing — and the obvious
  # repair, `sed -n 's/.*\(BE\|LE\).*/\1/p'`, is a GNU extension that BSD sed
  # on this machine reads as a literal `\|`. Both spellings turned a cell that
  # had reached the wire into one that looked unable to parse it.
  case "$line" in
    *" BE "*|*" BE") order=big ;;
    *" LE "*|*" LE") order=little ;;
    *) echo "FAIL	a tap line names no byte order: $line"; exit 1 ;;
  esac
  [ -n "$v" ] || { echo "FAIL	a tap line names no GIOP version: $line"; exit 1; }
  key="$v/$order"
  case " $seen " in *" $key "*) continue ;; esac
  seen="$seen $key"
  printf 'observed\tgiop=%s\torder=%s\n' "$v" "$order"
done <<<"$replies"

printf 'note\t%s generated call(s) completed over the wire against omniORB, no Rust stub and no org.omg.CORBA in the path\n' "$calls"
printf 'note\tthe order above was read off the reply flag byte; the Python cell of the same name reports it as claimed because no tap sits in that path\n'
exit 0
