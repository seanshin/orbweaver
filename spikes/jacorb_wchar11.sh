#!/usr/bin/env bash
# The single wide character at GIOP 1.1, both directions, against JacORB.
#
# D010 B5, second half. spikes/jacorb_giop11.sh measured the 1.1 wstring
# (commits 74b0f15, 2a052cb) and left the 1.1 wchar unmeasured because
# spikes/echo.idl has no wchar operation — so this fixture serves and dials
# spikes/wide.idl instead, with a hand-built GIOP 1.1 peer on our side
# (spikes/jacorb_wchar11.py) that records every octet and can choose the byte
# order of what it writes. omniORBpy cannot unmarshal its own 1.1 wchar, so
# JacORB 3.9 is the only peer that can be asked.
#
# A 1.1 wchar under UTF-16 is two octets with no length indication, so it has
# nowhere to carry a mark; the only question is which order the two octets are
# in. Our writer and reader (codeset.rs, put_wchar/get_wchar at 1.1) use the
# MESSAGE's order. JacORB writes big-endian messages only, so what its reader
# does with a little-endian message can only be seen if somebody sends it one
# — the Python server here does, and the control run sends big-endian units in
# a little-endian message so that "message order" is measured against its
# alternative rather than inferred.
#
# What is measured, in order:
#   1. JacORB client -> our server, replies big-endian: the four units come
#      back to JacORB's user as sent, and JacORB's own request octets are
#      printed (provenance for tests/wide_1_1_from_a_peer.rs).
#   2. the same, replies LITTLE-endian with the unit in the message's order:
#      JacORB's user still gets every unit as sent.
#   3. control: replies little-endian with BIG-endian units: JacORB's user
#      must get every unit swapped (U+0077 -> U+7700 ...). If it did not,
#      step 2 would have proved nothing about the order.
#   4. our client -> a JacORB server advertising IIOP 1.1: our 1.1 requests
#      in both byte orders, unit in the message's order, every unit echoed
#      back as sent; the control (big-endian units in a little-endian
#      request) must come back swapped; and three behaviours recorded, not
#      gated — a lone surrogate, a surrogate pair offered as one wchar, and
#      U+FEFF as the first character of a wstring.
#   5. the live octets against the recording in
#      crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs, whose tests pin
#      our codec to them.
#
# Versions and byte orders are asserted from the message headers as the Python
# side parsed them, never from what a fixture was told; the Java client
# reports what its user received, and that value is compared to what was sent.
#
# Exit 0 when every gated line is ok, 1 when any is FAIL, 2 when the fixture is
# absent — SKIPPED is unmeasured, never passing. `--expect-han HEX` is the
# negative control: the same run asserting that U+D55C comes back as another
# unit must go red.
#
# Harness rules: every wait loop sleeps and is bounded; every producer is
# captured to a file and matched afterwards, never piped into grep -q; every
# fixture is killed by the PID captured at launch.
#
# *GIOP 1.1의 와이드 문자 하나를 JacORB와 양방향으로 잰다. 순서는 메시지의
# 순서라는 것을, 반대 규약을 보내 뒤집히는 것으로 확인한다.*
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

EXPECT_HAN=D55C
case "${1:-}" in
  --expect-han) EXPECT_HAN="${2:-D55C}" ;;
  "") ;;
  *) echo "usage: $0 [--expect-han HEX]"; exit 2 ;;
esac

JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"
JDIR="$ROOT/spikes/jacorb"
PY="$ROOT/spikes/jacorb_wchar11.py"
RS="$ROOT/crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs"

# The units, as hex arguments so no file's encoding takes part: 'w' (whose
# swap, U+7700, is a different valid character), '한' U+D55C, U+FEFF as DATA
# (a mark, if a 1.1 wchar could carry one, which it cannot), and a lone
# surrogate — the nearest a Java char can get to a character above the BMP.
UNITS="0077 D55C FEFF D83D"

if [ ! -f "$JDIR/classes/WideClient.class" ] || [ ! -f "$JDIR/classes/WideServer.class" ] \
   || [ ! -x "$JH/bin/java" ]; then
  echo "  SKIPPED  JacORB Wide fixture absent — run spikes/jacorb/setup.sh (needs JDK 21); 1.1 wchar against JacORB is unmeasured, not passing"
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "  SKIPPED  python3 absent — our hand-built 1.1 peer cannot run; 1.1 wchar against JacORB is unmeasured, not passing"
  exit 2
fi

D="${JACORB_WCHAR11_DIR:-/tmp/orbweaver-wchar11}"
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

wait_for_file() { # sleeping, deadline-bounded
  local path="$1" deadline=$(($(date +%s) + $2))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    [ -s "$path" ] && return 0
    sleep 0.1
  done
  return 1
}

stop() { [ -n "${1:-}" ] || return 0; kill "$1" 2>/dev/null; wait "$1" 2>/dev/null; }

# start_py_server <tag> <reply-order> <unit-order> → PY_PID; IOR at $D/<tag>.ior, log at $D/<tag>.srv.log
start_py_server() {
  local tag="$1"
  python3 "$PY" server --out "$D/$tag.ior" --log "$D/$tag.srv.log" --reply-order "$2" --unit-order "$3" \
    >"$D/$tag.srv.out" 2>&1 &
  PY_PID=$!; PIDS+=("$PY_PID")
  wait_for_file "$D/$tag.ior" 15 || { fail "$tag: our server did not publish an IOR"; sed 's/^/       /' "$D/$tag.srv.out"; return 1; }
  sleep 0.2
}

# run_wide_client <tag> <ior> → rc; output at $D/<tag>.client.log
run_wide_client() {
  local tag="$1" ior="$2"
  # shellcheck disable=SC2086 — the units are separate arguments on purpose
  ( cd "$JDIR" && "$JH/bin/java" -cp "$JCP" WideClient "$ior" $UNITS ) >"$D/$tag.client.log" 2>&1
}

# What the Python server saw: versions and orders of every request, e.g. "1.1 BE".
requests_seen() { # <srv log>
  grep -o 'C->S GIOP [0-9]\.[0-9] Request [BL]E' "$1" 2>/dev/null | sed 's/.*GIOP //; s/ Request//' | sort -u | tr '\n' ',' | sed 's/,$//'
}
# JacORB's request octets for one unit, from the server log.
peer_wrote() { # <srv log> <unit hex, upper>
  sed -n "s/^    request body: wchar 1\.1 body=\([0-9a-f]*\) .*-> U+$2\$/\1/p" "$1" | head -1
}

# ── 1. JacORB client -> our server, big-endian replies ─────────────────────
echo "JacORB client -> our server (GIOP 1.1)"
start_py_server a be message || exit 1
run_wide_client a "$D/a.ior"; a_rc=$?
stop "$PY_PID"
a_seen=$(requests_seen "$D/a.srv.log")
a_ok=$(grep -c "^  ok   echo_wchar" "$D/a.client.log")
a_han=$(grep -c "echo_wchar\[U+D55C\] -> U+$EXPECT_HAN\$" "$D/a.client.log")
if [ "$a_rc" -eq 0 ] && [ "$a_seen" = "1.1 BE" ] && [ "$a_ok" -eq 4 ] && [ "$a_han" -eq 1 ]; then
  ok "JacORB dials our IIOP 1.1 IOR at GIOP $a_seen and its user gets all four units back from our big-endian replies (U+D55C -> U+$EXPECT_HAN)"
else
  fail "big-endian replies: client rc=$a_rc, requests seen '${a_seen:-none}', $a_ok/4 ok, U+D55C -> U+$EXPECT_HAN seen $a_han time(s)"
  grep -E "FAIL|ok" "$D/a.client.log" | head -4 | sed 's/^/       /'
fi
for u in $UNITS; do
  w=$(peer_wrote "$D/a.srv.log" "$u")
  echo "       JacORB wrote U+$u as: ${w:-not recorded}"
done
a_ctx=$(grep -o 'codesets([^)]*)' "$D/a.srv.log" | head -1)
[ -n "$a_ctx" ] && echo "       negotiated: $a_ctx"

# ── 2. the same, little-endian replies, unit in the message's order ────────
start_py_server b le message || exit 1
run_wide_client b "$D/b.ior"; b_rc=$?
stop "$PY_PID"
b_ok=$(grep -c "^  ok   echo_wchar" "$D/b.client.log")
b_le=$(grep -c "S->C GIOP 1.1 Reply LE" "$D/b.srv.log")
if [ "$b_rc" -eq 0 ] && [ "$b_ok" -eq 4 ] && [ "$b_le" -eq 4 ]; then
  ok "little-endian replies, unit in the message's order: JacORB's user gets all four units as sent ($b_le LE replies)"
else
  fail "little-endian replies, unit little-endian: client rc=$b_rc, $b_ok/4 ok, $b_le LE replies"
  grep -E "FAIL" "$D/b.client.log" | head -4 | sed 's/^/       /'
fi

# ── 3. control: little-endian replies, big-endian units ────────────────────
start_py_server c le big || exit 1
run_wide_client c "$D/c.ior"; c_rc=$?
stop "$PY_PID"
c_swapped=0
for pair in "0077 7700" "D55C 5CD5" "FEFF FFFE" "D83D 3DD8"; do
  set -- $pair
  n=$(grep -c "FAIL echo_wchar\[U+$1\] -> U+$2\$" "$D/c.client.log")
  c_swapped=$((c_swapped + n))
done
if [ "$c_rc" -eq 1 ] && [ "$c_swapped" -eq 4 ]; then
  ok "control: big-endian units in little-endian replies reach JacORB's user swapped, 4/4 — JacORB reads a 1.1 wchar in the MESSAGE's order, which is what we write"
else
  fail "control: expected all four units swapped at JacORB's user (rc 1); got rc=$c_rc, $c_swapped/4 swapped"
  grep -E "FAIL|ok" "$D/c.client.log" | head -4 | sed 's/^/       /'
fi

# ── 4. our client -> a JacORB server advertising IIOP 1.1 ──────────────────
echo "our client -> JacORB server (GIOP 1.1)"
( cd "$JDIR" && exec "$JH/bin/java" -cp "$JCP" -Djacorb.giop_minor_version=1 -DOAIAddr=127.0.0.1 \
    WideServer "$D/j.ior" ) >"$D/j.log" 2>&1 &
JSRV_PID=$!; PIDS+=("$JSRV_PID")
if ! wait_for_file "$D/j.ior" 30; then
  fail "JacORB WideServer did not publish an IOR"; sed 's/^/       /' "$D/j.log" | tail -5
  exit 1
fi
sleep 0.5
CASES="0077 D55C FEFF D83D pair wstring-feff"
# shellcheck disable=SC2086
python3 "$PY" client --ior "$D/j.ior" --log "$D/d.be.log" --order be $CASES >"$D/d.be.out" 2>&1; d_be_rc=$?
# shellcheck disable=SC2086
python3 "$PY" client --ior "$D/j.ior" --log "$D/d.le.log" --order le $CASES >"$D/d.le.out" 2>&1; d_le_rc=$?
# shellcheck disable=SC2086
python3 "$PY" client --ior "$D/j.ior" --log "$D/d.ctl.log" --order le --unit-order big $UNITS >"$D/d.ctl.out" 2>&1; d_ctl_rc=$?
stop "$JSRV_PID"
d_prof=$(sed -n 's/^  info profile IIOP \([0-9]\.[0-9]\).*/\1/p' "$D/d.be.out" | head -1)
for order in be le; do
  eval "rc=\$d_${order}_rc"
  n_ok=$(grep -c "^  ok   " "$D/d.$order.out")
  n_han=$(grep -c "^  ok   D55C: .*-> U+$EXPECT_HAN\$" "$D/d.$order.out")
  n_11=$(grep -c "reply GIOP 1.1 BE" "$D/d.$order.out")
  if [ "$rc" -eq 0 ] && [ "$n_ok" -eq 4 ] && [ "$n_han" -eq 1 ] && [ "$n_11" -ge 4 ]; then
    ok "our $(echo "$order" | tr a-z A-Z) 1.1 requests to a JacORB server (profile IIOP ${d_prof:-?}): all four units echoed as sent, replies at GIOP 1.1 (U+D55C -> U+$EXPECT_HAN)"
  else
    fail "our $(echo "$order" | tr a-z A-Z) 1.1 requests: rc=$rc, $n_ok/4 ok, U+D55C -> U+$EXPECT_HAN seen $n_han time(s), $n_11 replies at 1.1 BE"
    grep -E "FAIL|ok" "$D/d.$order.out" | head -4 | sed 's/^/       /'
  fi
done
d_swapped=0
for pair in "0077 7700" "D55C 5CD5" "FEFF FFFE" "D83D 3DD8"; do
  set -- $pair
  n=$(grep -c "^  FAIL $1: .*-> U+$2\$" "$D/d.ctl.out")
  d_swapped=$((d_swapped + n))
done
if [ "$d_ctl_rc" -eq 1 ] && [ "$d_swapped" -eq 4 ]; then
  ok "control: big-endian units in our little-endian requests come back from JacORB swapped, 4/4 — same rule in this direction"
else
  fail "control: expected all four units back swapped; rc=$d_ctl_rc, $d_swapped/4"
  grep -E "FAIL|ok" "$D/d.ctl.out" | head -4 | sed 's/^/       /'
fi
# Behaviour, recorded rather than gated: what each side does with what a
# wchar cannot carry.
sed -n 's/^  info pair: /       pair (JacORB, our BE request): /p' "$D/d.be.out"
sed -n 's/^  info wstring-feff: /       wstring FEFF as data (JacORB): /p' "$D/d.be.out"
info "a lone surrogate U+D83D crosses both ways as two octets (JacORB's char is a UTF-16 unit); our reader refuses it as not a character (tests/wide_1_1_from_a_peer.rs), our writer refuses U+1F600 as two units"
sed -n 's|^  ok   D55C: |       U+D55C, we wrote / JacORB wrote: |p' "$D/d.be.out" "$D/d.le.out"

# ── 5. the live octets against the recording ───────────────────────────────
echo "recording"
rec_check() { # <const name> <live hex> [what]
  local want; want=$(python3 "$PY" recorded --rs "$RS" --name "$1")
  if [ "$want" = "$2" ] && [ -n "$2" ]; then
    ok "$1${3:+ ($3)} = $2, as recorded"
  else
    fail "$1${3:+ ($3)}: live '$2', recorded '$want' — the recording no longer describes the peer, or the peer changed"
  fi
}
rec_check JACORB_WCHAR_W    "$(peer_wrote "$D/a.srv.log" 0077)"
rec_check JACORB_WCHAR_HAN  "$(peer_wrote "$D/a.srv.log" D55C)"
rec_check JACORB_WCHAR_FEFF "$(peer_wrote "$D/a.srv.log" FEFF)"
rec_check JACORB_WCHAR_LONE_SURROGATE "$(peer_wrote "$D/a.srv.log" D83D)"
# The whole request JacORB's client wrote for U+D55C (its second request, so
# no service context is in it), and the whole reply its server wrote for our
# big-endian U+D55C request; both from the hexdumps.
whole() { # <log> <header line pattern> → hex of the dump that follows it
  awk -v pat="$2" '
    found && /^    [0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / { line = substr($0, 11, 47); gsub(/ /, "", line); hex = hex line; next }
    found && !/^    / { exit }
    index($0, pat) == 1 { found = 1; hex = "" }
    END { print hex }
  ' "$1"
}
rec_check JACORB_REQUEST_HAN "$(whole "$D/a.srv.log" "[1] C->S GIOP 1.1 Request BE id=2 op=echo_wchar")"
rec_check JACORB_REPLY_HAN   "$(whole "$D/d.be.log" "S->C GIOP 1.1 Reply BE id=4 status=0")" "to our BE request"
rec_check JACORB_REPLY_HAN   "$(whole "$D/d.le.log" "S->C GIOP 1.1 Reply BE id=4 status=0")" "to our LE request"
# And what our side wrote that JacORB's user read as U+D55C, in each order.
rec_check OUR_REPLY_HAN_BE "$(whole "$D/a.srv.log" "[1] S->C GIOP 1.1 Reply BE id=2 for=echo_wchar")"
rec_check OUR_REPLY_HAN_LE "$(whole "$D/b.srv.log" "[1] S->C GIOP 1.1 Reply LE id=2 for=echo_wchar")"

echo
echo "logs: $D"
if [ "$FAILS" -eq 0 ]; then
  echo "jacorb wchar 1.1: PASS"
  exit 0
fi
echo "jacorb wchar 1.1: FAIL — $FAILS line(s)"
exit 1
