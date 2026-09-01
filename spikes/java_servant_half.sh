#!/usr/bin/env bash
# The Java serving half of the seam, executed.
#
# ── What this is, and what it is not ────────────────────────────────────────
#
# `COMPONENTS.md` recorded the gap precisely: what a Java servant owed was *"an
# `Answerer` over the bridge's pipes and a `_Rt.Host`/`dispatchCall` in
# `java_rt.java` — the two things `python_rt.py` has and `java_rt.java` does not
# — and **not** anything in the seam's definition."* That was right, and this is
# the second of those two: `_Rt.dispatchCall`, `_Rt.Servant`, `_Rt.Op` and the
# generated `<Name>Servant` base, driven over a real contract.
#
# **It is NOT the `servant × self`, `servant × omniorb` or `servant × jacorb`
# cell**, and those stay counted `SKIPPED`. The spawner they were waiting on
# landed the same day — `SeamChild::java`, and
# `tests/a_java_servant_this_process_owns.rs` mounts a Java servant behind a
# `Dispatch` this process owns — so what a CELL still needs is narrower than it
# was: a runner script in the acceptance grid, which is what the suite counts.
# Claiming a cell on the strength of either would be the *green because nothing
# happened* shape this repository keeps finding, one row up.
#
# ── Why it can be measured with no bridge at all ────────────────────────────
#
# `dispatchCall` is a pure function of a servant and a parsed call document, for
# the same reason `python_rt.dispatch_call` is one. That is what lets every
# branch — argument conversion, the method call, reply shaping, and both
# refusals — be executed with no process, no socket and no peer in sight. The
# design decision and the measurability are the same decision.
#
# ── Exit ────────────────────────────────────────────────────────────────────
#
#   0  every case answered the document it must
#   1  a case answered something else, or the emitter's Java did not compile
#   2  no JDK — unmeasured, not passing
#
# *`COMPONENTS.md`가 적어 둔 두 가지 중 두 번째 — `dispatchCall`과 생성된
# `<Name>Servant` — 를 실제 계약 위에서 실행한다. **칸이 아니다**: 칸에는 `java`를
# seam 자식으로 띄우는 러스트 쪽이 필요하고 그것은 아직 없다. 다리가 놓일 절반이
# 동작한다는 것이지, 다리를 건넜다는 것이 아니다. 브리지 없이 잴 수 있는 이유는
# `dispatchCall`이 순수 함수이기 때문이고, 그 설계와 이 측정 가능성은 같은 결정이다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
[ -x "$JH/bin/javac" ] && [ -x "$JH/bin/java" ] || {
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME. Unmeasured, not passing."
  exit 2
}

D=/tmp/orbweaver-java-servant-half
rm -rf "$D"; mkdir -p "$D"

if ! cargo run -q --bin gen-java -- --out "$D/src" --package echo spikes/echo.idl \
     >"$D/gen.log" 2>&1; then
  echo "FAIL	gen-java did not run"
  tail -5 "$D/gen.log"
  exit 1
fi
find "$D/src" -name '*.java' >"$D/srcs"
echo "$ROOT/spikes/bindings/java/EchoServantProbe.java" >>"$D/srcs"
if ! "$JH/bin/javac" -nowarn -encoding UTF-8 -d "$D/classes" @"$D/srcs" >"$D/javac.log" 2>&1; then
  echo "FAIL	javac refused the servant bases the emitter wrote"
  head -10 "$D/javac.log"
  exit 1
fi

out=$("$JH/bin/java" -cp "$D/classes" EchoServantProbe 2>"$D/run.err")
rc=$?
if [ "$rc" -ne 0 ]; then
  echo "FAIL	the probe did not run (exit $rc)"
  head -6 "$D/run.err"
  exit 1
fi

# The documents each case must answer. Compared whole, not by substring: a
# reply that carried the right value under the wrong key would pass a `grep`.
expect() {
  local name="$1" want="$2" got
  got=$(grep "^$name	" <<<"$out" | cut -f2-)
  if [ -z "$got" ]; then
    echo "FAIL	the probe printed no line for $name — every case must answer"
    return 1
  fi
  if [ "$got" != "$want" ]; then
    echo "FAIL	$name answered the wrong document"
    echo "     	want: $want"
    echo "     	got:  $got"
    return 1
  fi
  echo "  ok   $name: $got"
  return 0
}

fails=0
expect implemented        '{"ok":{"returns":42,"outputs":{}}}' || fails=$((fails+1))
expect string             '{"ok":{"returns":"java:hello","outputs":{}}}' || fails=$((fails+1))
expect not-implemented    '{"system_exception":{"id":"IDL:omg.org/CORBA/NO_IMPLEMENT:1.0","minor":0,"completed":1}}' || fails=$((fails+1))
expect no-such-operation  '{"system_exception":{"id":"IDL:omg.org/CORBA/BAD_OPERATION:1.0","minor":0,"completed":1}}' || fails=$((fails+1))

echo "  note the three Java servant CELLS stay SKIPPED. The spawner they were waiting"
echo "       on now exists (SeamChild::java) and the route is measured by"
echo "       tests/a_java_servant_this_process_owns.rs; what a CELL additionally needs"
echo "       is a runner script in the acceptance grid, and those are not written"
if [ "$fails" -ne 0 ]; then
  echo "java servant half: $fails case(s) answered the wrong document"
  exit 1
fi
echo "java servant half: PASS — 4 cases, no bridge and no socket in the path"
exit 0
