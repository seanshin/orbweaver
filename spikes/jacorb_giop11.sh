#!/usr/bin/env bash
# JacORB at GIOP 1.1, both directions, and then one wide-string call each way.
#
# D010 B5. Every JacORB group in run_checks.sh runs at JacORB's default of 1.2,
# so before this script nothing in the tree had measured what JacORB does at
# 1.1 — the version whose wide-character rule is the contentious one (count in
# wide characters plus a terminator, no per-character length, order and mark
# per the negotiated TCS-W). omniORBpy cannot unmarshal its own 1.1 wchar, so
# JacORB is the only 1.1 wide-text peer this host has, and this is the fixture
# that reaches it.
#
# What is measured, in order:
#   1. control  — a 1.2 IOR, nothing asked: JacORB speaks 1.2 to us and 1.1
#                 is NOT seen. If it were, the version check below could not
#                 discriminate and the run fails here.
#   2. info     — -Djacorb.giop_minor_version=1 with the same 1.2 IOR: which
#                 version the wire carried is printed, not gated. (Measured
#                 2026-08-19: 1.2. JacORB 3.9's property sets the version of
#                 the IORs it *creates*; its outbound version follows the
#                 profile it dials.)
#   3. fixture  — the same IOR republished by the tap at IIOP 1.1, every
#                 component (TAG_CODE_SETS included) copied byte for byte:
#                 the wire must carry GIOP 1.1, seen by our server's own
#                 "first request at GIOP 1.1" line AND by the tap's headers.
#                 Then echo_wstring at 1.1, JacORB client → our server,
#                 compared as decoded code points on the Java side; the bytes
#                 both sides wrote are printed from the tap.
#   4. reverse  — a JacORB server started with the property: its IOR must
#                 advertise IIOP 1.1 (control: without the property it says
#                 1.2); spike-interop then runs its whole case list at 1.1
#                 through the tap, and JacORB's echo_wstring reply is checked
#                 against what we sent — we must write no mark at 1.1, and
#                 the reply must be exactly as long as the request.
#                 spike-interop's own verdict cannot see a mark echoed as a
#                 character, because our reader strips a leading U+FEFF, which
#                 is exactly the case this line exists to catch (it did, on
#                 2026-08-19: 4/4 exchanges, fixed in codeset.rs the same day).
#
# Wire versions are asserted from bytes (the tap parses every header) and from
# the Rust server's log, never from what a fixture was *told*.
#
# Exit 0 when every gated line is ok, 1 when any is FAIL, 2 when the fixture is
# absent — SKIPPED is unmeasured, never passing. `--expect-minor 2` is the
# negative control for step 3: the same assertions, expecting 1.2 where the
# tap republished 1.1, must go red.
#
# Harness rules: every wait loop sleeps and is bounded; every producer is
# captured to a file and matched afterwards, never piped into grep -q; every
# fixture is killed by the PID captured at launch.
#
# *JacORB를 GIOP 1.1로 양방향 구동하는 픽스처. 버전은 피어에게 지시한 값이 아니라
# 와이어의 바이트와 우리 서버의 로그로 단언한다. 그 다음 와이드 문자열 한 번씩.*
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

EXPECT_MINOR=1
case "${1:-}" in
  --expect-minor) EXPECT_MINOR="${2:-1}" ;;
  "") ;;
  *) echo "usage: $0 [--expect-minor N]"; exit 2 ;;
esac

JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"
JDIR="$ROOT/spikes/jacorb"
TAP="$ROOT/spikes/jacorb_giop11_tap.py"

# The two texts, as arguments so the octets are the measurement and no file's
# encoding takes part: twelve BMP units, and one that needs a surrogate pair,
# which is two UTF-16 units and therefore two 1.1 "wide characters".
TEXT_BMP="wide 함정 전투체계"
TEXT_ASTRAL="pair 😀 end"

if [ ! -f "$JDIR/classes/Client11.class" ] || [ ! -f "$JDIR/classes/Server.class" ] \
   || [ ! -x "$JH/bin/java" ]; then
  echo "  SKIPPED  JacORB fixture absent — run spikes/jacorb/setup.sh (needs JDK 21); GIOP 1.1 against JacORB is unmeasured, not passing"
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "  SKIPPED  python3 absent — the recording tap cannot run; GIOP 1.1 against JacORB is unmeasured, not passing"
  exit 2
fi

D="${JACORB_GIOP11_DIR:-/tmp/orbweaver-giop11}"
rm -rf "$D"; mkdir -p "$D"
PIDS=()
FAILS=0

cleanup() {
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" 2>/dev/null; done
  for pid in "${PIDS[@]:-}"; do [ -n "$pid" ] && wait "$pid" 2>/dev/null; done
}
trap cleanup EXIT INT TERM

ok()   { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
info() { echo "  info $*"; }

# Sleeping, deadline-bounded wait for a non-empty file.
wait_for_file() {
  local path="$1" deadline=$(($(date +%s) + $2))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    [ -s "$path" ] && return 0
    sleep 0.1
  done
  return 1
}

stop() { # stop <pid>
  [ -n "${1:-}" ] || return 0
  kill "$1" 2>/dev/null; wait "$1" 2>/dev/null
}

# start_rust_server <tag> → RUST_PID; IOR at $D/<tag>.server.ior, log at $D/<tag>.rust.log
start_rust_server() {
  local tag="$1"
  ./target/debug/spike-server "$D/$tag.server.ior" >"$D/$tag.rust.log" 2>&1 &
  RUST_PID=$!; PIDS+=("$RUST_PID")
  wait_for_file "$D/$tag.server.ior" 15 || { fail "$tag: our server did not publish an IOR"; return 1; }
  sleep 0.2
}

# start_tap <tag> <ior-in> [--minor N] → TAP_PID; tapped IOR at $D/<tag>.tapped.ior, log at $D/<tag>.tap.log
start_tap() {
  local tag="$1" ior="$2"; shift 2
  python3 "$TAP" --ior "$ior" --out "$D/$tag.tapped.ior" --log "$D/$tag.tap.log" "$@" >"$D/$tag.tap.out" 2>&1 &
  TAP_PID=$!; PIDS+=("$TAP_PID")
  wait_for_file "$D/$tag.tapped.ior" 15 || { fail "$tag: the tap did not publish an IOR"; sed 's/^/       /' "$D/$tag.tap.out"; return 1; }
  sleep 0.2
}

# run_client11 <tag> <ior> [java -D options...] → rc; output at $D/<tag>.client.log
run_client11() {
  local tag="$1" ior="$2"; shift 2
  ( cd "$JDIR" && JAVA_TOOL_OPTIONS=-Dfile.encoding=UTF-8 \
      "$JH/bin/java" -cp "$JCP" "$@" Client11 "$ior" "$TEXT_BMP" "$TEXT_ASTRAL" ) \
      >"$D/$tag.client.log" 2>&1
}

# start_jacorb_server <tag> [java -D options...] → JSRV_PID; IOR at $D/<tag>.jacorb.ior
start_jacorb_server() {
  local tag="$1"; shift
  ( cd "$JDIR" && exec "$JH/bin/java" -cp "$JCP" "$@" Server "$D/$tag.jacorb.ior" ) >"$D/$tag.jacorb.log" 2>&1 &
  JSRV_PID=$!; PIDS+=("$JSRV_PID")
  wait_for_file "$D/$tag.jacorb.ior" 30 || { fail "$tag: JacORB server did not publish an IOR"; return 1; }
  sleep 0.5
}

# Versions the tap saw from the client side, e.g. "1.1 1.2".
wire_versions() { # <tap log>
  grep -o 'C->S GIOP [0-9]\.[0-9]' "$1" 2>/dev/null | sed 's/.*GIOP //' | sort -u | tr '\n' ' ' | sed 's/ $//'
}

echo "building"
if ! cargo build -q --bin spike-server --bin spike-interop 2>"$D/build.log"; then
  fail "spike-server / spike-interop did not build"; sed 's/^/       /' "$D/build.log" | head -5
  exit 1
fi

# ── 1. control: nothing asked, a 1.2 profile ────────────────────────────────
echo "JacORB client -> our server"
start_rust_server c && start_tap c "$D/c.server.ior" || exit 1
run_client11 c "$D/c.tapped.ior"; c_rc=$?
stop "$TAP_PID"; stop "$RUST_PID"
c_seen=$(wire_versions "$D/c.tap.log")
c_srv=$(grep -c "first request at GIOP 1.2" "$D/c.rust.log")
if [ "$c_rc" -eq 0 ] && [ "$c_seen" = "1.2" ] && [ "$c_srv" -eq 1 ]; then
  ok "control: profile 1.2, nothing asked -> wire 1.2 only (tap headers and our server agree)"
else
  fail "control: expected 1.2 only; tap saw '${c_seen:-nothing}', server log: $(grep 'first request' "$D/c.rust.log" | tr '\n' ';'), client rc=$c_rc"
  grep FAIL "$D/c.client.log" | head -3 | sed 's/^/       /'
fi

# ── 2. info: the property alone, against a 1.2 profile ──────────────────────
start_rust_server p && start_tap p "$D/p.server.ior" || exit 1
run_client11 p "$D/p.tapped.ior" -Djacorb.giop_minor_version=1; p_rc=$?
stop "$TAP_PID"; stop "$RUST_PID"
p_seen=$(wire_versions "$D/p.tap.log")
info "-Djacorb.giop_minor_version=1 with a 1.2 profile -> wire ${p_seen:-nothing} (client rc=$p_rc; the property is not what lowers JacORB's outbound version)"

# ── 3. the fixture: the profile republished at 1.1 ──────────────────────────
start_rust_server f && start_tap f "$D/f.server.ior" --minor 1 || exit 1
run_client11 f "$D/f.tapped.ior"; f_rc=$?
stop "$TAP_PID"; stop "$RUST_PID"
f_seen=$(wire_versions "$D/f.tap.log")
f_srv=$(grep -c "first request at GIOP 1.$EXPECT_MINOR (Big)" "$D/f.rust.log")
f_narrow=$(grep -cE "^  ok   (ping\(\)|echo_string)" "$D/f.client.log")
if [ "$f_seen" = "1.$EXPECT_MINOR" ] && [ "$f_srv" -eq 1 ] && [ "$f_narrow" -eq 2 ]; then
  ok "profile republished at 1.1 -> JacORB speaks GIOP 1.$EXPECT_MINOR to us: tap headers '$f_seen', server log agrees, ping/echo_string decoded"
else
  fail "profile republished at 1.1: expected wire 1.$EXPECT_MINOR; tap saw '${f_seen:-nothing}', server log: $(grep 'first request' "$D/f.rust.log" | tr '\n' ';'), narrow calls ok=$f_narrow"
fi
# The one more call: wide text at 1.1, JacORB writing and reading.
f_wide_ok=$(grep -c "^  ok   echo_wstring" "$D/f.client.log")
f_wide_bad=$(grep "^  FAIL echo_wstring" "$D/f.client.log")
if [ "$f_wide_ok" -eq 2 ] && [ -z "$f_wide_bad" ]; then
  ok "echo_wstring at 1.1, JacORB client -> our server: both texts back as the same code points"
else
  fail "echo_wstring at 1.1, JacORB client -> our server: JacORB's user did not get the text back ($f_wide_ok/2 ok)"
  printf '%s\n' "$f_wide_bad" | head -2 | cut -c1-160 | sed 's/^/       /'
fi
# Provenance: what each side wrote, from the tap. Printed on every run so the
# bytes travel with the verdict.
sed -n 's/^    request body: /       JacORB wrote:  /p; s/^    reply body: /       we wrote:      /p' "$D/f.tap.log" | head -2
f_ctx=$(grep -o 'codesets([^)]*)' "$D/f.tap.log" | head -1)
[ -n "$f_ctx" ] && echo "       negotiated: $f_ctx"

# ── 4. reverse: our client -> a JacORB server whose IOR says 1.1 ────────────
echo "our client -> JacORB server"
start_jacorb_server r0 || exit 1
stop "$JSRV_PID"
# The tap's first log line names the profile version it was given, so it is
# also the reader for the IOR a JacORB server writes without the property.
start_tap r0 "$D/r0.jacorb.ior" || exit 1
stop "$TAP_PID"
r0_ver=$(sed -n 's/.*original profile: IIOP \([0-9]\.[0-9]\).*/\1/p' "$D/r0.tap.log" | head -1)
start_jacorb_server r -Djacorb.giop_minor_version=1 || exit 1
start_tap r "$D/r.jacorb.ior" || exit 1
r_ver=$(sed -n 's/.*original profile: IIOP \([0-9]\.[0-9]\).*/\1/p' "$D/r.tap.log" | head -1)
if [ "$r0_ver" = "1.2" ] && [ "$r_ver" = "1.1" ]; then
  ok "JacORB server advertises IIOP $r_ver with -Djacorb.giop_minor_version=1 (control: $r0_ver without it)"
else
  fail "JacORB server IOR: expected 1.2 without the property and 1.1 with it, got '$r0_ver' and '$r_ver'"
fi
./target/debug/spike-interop "$D/r.tapped.ior" >"$D/r.interop.log" 2>&1; r_rc=$?
stop "$TAP_PID"; stop "$JSRV_PID"
r_seen=$(wire_versions "$D/r.tap.log")
r_pass=$(grep -c "assumption A: PASS" "$D/r.interop.log")
r_w11=$(grep -c "wstring round-tripped under GIOP 1.1" "$D/r.interop.log")
if [ "$r_rc" -eq 0 ] && [ "$r_pass" -eq 1 ] && [ "$r_seen" = "1.1" ]; then
  ok "our client -> JacORB server at GIOP 1.1: spike-interop passes, both byte orders, every request on the wire at 1.1 ($r_w11 wstring lines at 1.1)"
else
  fail "our client -> JacORB server at 1.1: rc=$r_rc, wire '${r_seen:-nothing}'"
  grep "  FAIL" "$D/r.interop.log" | head -3 | sed 's/^/       /'
fi
# What spike-interop's green cannot see: JacORB's reply, in wide characters,
# against our request. Until 2026-08-19 we wrote a byte-order mark at 1.1 as
# at 1.2; JacORB read it as text, echoed it as the first unit, and our reader
# stripped that U+FEFF as a mark — so the round trip was green in 4/4
# exchanges while JacORB's own user saw U+FEFF + text. Only the wire tells:
# our request must carry no mark, and the reply must be exactly as long as
# the request. (A reply one unit shorter would mean we marked again and the
# peer stripped it; a reply as long as a marked request is the original bug.)
r_counts=$(awk '
  function id_of(s,  m) { match(s, / id=[0-9]+/); return substr(s, RSTART+4, RLENGTH-4) }
  function count_of(s) { match(s, /count=[0-9]+/); return substr(s, RSTART+6, RLENGTH-6) }
  function marked(s) { match(s, /body=[0-9a-f]+/); b = substr(s, RSTART+5, 4); return (b == "feff" || b == "fffe") ? "marked" : "unmarked" }
  /^\[[0-9]+\] C->S GIOP 1\.1 Request .*op=echo_wstring/ { pending = $1 " " id_of($0); next }
  pending != "" && /request body: wstring 1\.1 count=/ { req[pending] = count_of($0) " " marked($0); pending = "" }
  /^\[[0-9]+\] S->C GIOP 1\.1 Reply .*for=echo_wstring/ { rpending = $1 " " id_of($0); next }
  rpending != "" && /reply body: wstring 1\.1 count=/ { print req[rpending], count_of($0); rpending = "" }
' "$D/r.tap.log")
r_pairs=$(printf '%s\n' "$r_counts" | grep -c '[0-9]')
r_bad=$(printf '%s\n' "$r_counts" | awk 'NF == 3 && ($2 != "unmarked" || $3 != $1) { n++ } END { print n + 0 }')
if [ "$r_pairs" -ge 1 ] && [ "$r_bad" -eq 0 ]; then
  ok "our 1.1 wstring requests carry no mark and JacORB's replies are exactly as long ($r_pairs exchange(s), both byte orders): no U+FEFF reached either user"
elif [ "$r_pairs" -ge 1 ]; then
  fail "1.1 wstring: $r_bad of $r_pairs exchange(s) either carried a mark from us or came back a different length (a marked request echoed at equal length is U+FEFF in JacORB's user's text, hidden by our reader)"
  printf '%s\n' "$r_counts" | head -2 | sed 's/^/       request count, marked?, reply count: /'
else
  fail "no echo_wstring exchange at 1.1 was recorded by the tap"
fi
sed -n 's/^    request body: /       we wrote:      /p; s/^    reply body: /       JacORB wrote:  /p' "$D/r.tap.log" | head -2

echo
echo "logs: $D"
if [ "$FAILS" -eq 0 ]; then
  echo "jacorb giop 1.1: PASS"
  exit 0
fi
echo "jacorb giop 1.1: FAIL — $FAILS line(s)"
exit 1
