#!/usr/bin/env bash
# spikes/wide.idl with OUR stack in each seat — the half 382baa9 could not sit.
#
# D010 B5, third part. spikes/jacorb_wchar11.sh measured the GIOP 1.1 wchar
# against JacORB in both directions with a hand-built Python peer on our side,
# because spike-server and spike-interop know only spikes/echo.idl; the Rust
# codec was held to the exchanged octets by tests alone. This script puts the
# live Rust server and client (spike-wide, crates/orbweaver-object/src/bin/
# spike_wide.rs — Server + Dispatch, Connection + WideCodec) on the wire for
# the same contract and re-measures the same matrix, plus the self-consistency
# arm at every version. The octets are recorded by the tap
# (spikes/jacorb_giop11_tap.py), which also republishes our IIOP 1.2 IOR at
# the version each arm asks for — the same mechanism jacorb_giop11.sh uses, so
# no version flag was added to the server: the version on the wire is always
# something the tap's headers can be asked about. The tap describes wstring
# bodies only; a wchar body shows as "not a wstring" and its octets are read
# from the full dump that follows.
#
# What is measured, in order:
#   A. JacORB client -> our Rust server, profile republished at 1.1: JacORB
#      speaks 1.1 big-endian; our server's log says what OUR reader decoded
#      for each unit; JacORB's user gets 'w', '한' and U+FEFF back as sent;
#      the lone surrogate is refused by our reader (MARSHAL at JacORB's user)
#      — recorded behaviour, gated so that a change is visible. Little-endian
#      replies cannot be elicited here: our server answers in the request's
#      order and JacORB requests big-endian only — the real server's LE reply
#      is measured in arm C and its octets checked against OUR_REPLY_HAN_LE,
#      and JacORB's reading of that form is what jacorb_wchar11.sh step 2
#      measured with the hand-built peer.
#   B. our Rust client -> a JacORB server advertising IIOP 1.1: our 1.1
#      requests in both byte orders, unit in the message's order, every unit
#      and both 2a052cb texts echoed as sent; JacORB's replies at 1.1 BE.
#   C. our Rust client -> our Rust server at 1.0, 1.1 and 1.2, both orders:
#      1.0 refuses wchar on both sides (our codec before the wire, our server
#      with MARSHAL OMG minor 6 for two raw octets); 1.1 and 1.2 round-trip
#      the units and the texts, replies in the request's order. U+FEFF as a
#      1.2 wchar is measured separately and recorded, not gated: it does not
#      cross (see the DEFECT line).
#   R. the live octets against crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs:
#      JacORB's whole echo_wchar(U+D55C) request as our real server received
#      it, our real server's replies to it in both orders, our own 1.1 request
#      (octet for octet JacORB's), and JacORB's reply to our request.
#
# Versions and byte orders are asserted from the tap's parsed headers and from
# our server's own log, never from what a fixture was told. Values are compared
# decoded — the Java client reports what its user received, our client and
# server print the code point they decoded — and only then are the whole
# messages compared to the recording.
#
# Exit 0 when every gated line is ok, 1 when any is FAIL, 2 when the fixture is
# absent — SKIPPED is unmeasured, never passing. `--expect-han HEX` is the
# negative control: the same run asserting that U+D55C is decoded as another
# unit must go red on the Rust arms.
#
# Harness rules: every wait loop sleeps and is bounded; every producer is
# captured to a file and matched afterwards, never piped into grep -q; every
# fixture is killed by the PID captured at launch.
#
# *382baa9가 앉히지 못한 자리 — 우리 Rust 서버와 클라이언트 — 에 wide.idl을
# 올린다. 값은 디코드해서 비교하고, 그 다음에야 옥텟을 기록과 대조한다.*
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
TAP="$ROOT/spikes/jacorb_giop11_tap.py"
PY="$ROOT/spikes/jacorb_wchar11.py"
RS="$ROOT/crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs"
BIN="$ROOT/target/debug/spike-wide"

# The units of jacorb_wchar11.sh, as hex so no file's encoding takes part:
# 'w' (its swap U+7700 is another valid character), '한', U+FEFF as DATA, and
# a lone surrogate — which JacORB's char can carry and our `char` cannot.
UNITS="0077 D55C FEFF D83D"
# The two texts 2a052cb measured: twelve BMP units, and one that needs a
# surrogate pair — two UTF-16 units, therefore two 1.1 "wide characters".
TEXT_BMP="wide 함정 전투체계"
TEXT_ASTRAL="pair 😀 end"

if [ ! -f "$JDIR/classes/WideClient.class" ] || [ ! -f "$JDIR/classes/WideServer.class" ] \
   || [ ! -x "$JH/bin/java" ]; then
  echo "  SKIPPED  JacORB Wide fixture absent — run spikes/jacorb/setup.sh (needs JDK 21); wide.idl from our stack against JacORB is unmeasured, not passing"
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "  SKIPPED  python3 absent — the recording tap cannot run; wide.idl from our stack is unmeasured, not passing"
  exit 2
fi

D="${WIDE_RUST_DIR:-/tmp/orbweaver-wide-rust}"
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

# start_rust_server <tag> → RUST_PID; IOR at $D/<tag>.server.ior, log at $D/<tag>.rust.log
start_rust_server() {
  local tag="$1"
  "$BIN" serve "$D/$tag.server.ior" >"$D/$tag.rust.log" 2>&1 &
  RUST_PID=$!; PIDS+=("$RUST_PID")
  wait_for_file "$D/$tag.server.ior" 15 || { fail "$tag: our server did not publish an IOR"; sed 's/^/       /' "$D/$tag.rust.log" | tail -3; return 1; }
  sleep 0.2
}

# start_tap <tag> <ior-in> [--minor N] → TAP_PID; tapped IOR at $D/<tag>.tapped.ior, log at $D/<tag>.tap.log
start_tap() {
  local tag="$1" ior="$2"; shift 2
  python3 "$TAP" --ior "$ior" --out "$D/$tag.tapped.ior" --log "$D/$tag.tap.log" --op echo_wchar "$@" >"$D/$tag.tap.out" 2>&1 &
  TAP_PID=$!; PIDS+=("$TAP_PID")
  wait_for_file "$D/$tag.tapped.ior" 15 || { fail "$tag: the tap did not publish an IOR"; sed 's/^/       /' "$D/$tag.tap.out"; return 1; }
  sleep 0.2
}

# run_wide_client <tag> <ior> → rc; output at $D/<tag>.client.log
run_wide_client() {
  local tag="$1" ior="$2"
  # shellcheck disable=SC2086 — the units are separate arguments on purpose
  ( cd "$JDIR" && "$JH/bin/java" -cp "$JCP" WideClient "$ior" $UNITS ) >"$D/$tag.client.log" 2>&1
}

# run_our_client <tag> <ior> <be|le> [units...] → rc; output at $D/<tag>.out
run_our_client() {
  local tag="$1" ior="$2" order="$3"; shift 3
  "$BIN" call "$ior" "$order" --text "$TEXT_BMP" --text "$TEXT_ASTRAL" "$@" >"$D/$tag.out" 2>&1
}

# Versions and orders the tap saw from the client side, e.g. "1.1 BE".
requests_seen() { # <tap log>
  grep -o 'C->S GIOP [0-9]\.[0-9] Request size=[0-9]* [BL]E' "$1" 2>/dev/null | sed 's/.*GIOP //; s/ Request size=[0-9]*//' | sort -u | tr '\n' ',' | sed 's/,$//'
}
# The whole message under the first header line beginning with <pattern>.
whole() { # <log> <header line prefix> → hex
  awk -v pat="$2" '
    found && /^    [0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / { line = substr($0, 11, 47); gsub(/ /, "", line); hex = hex line; next }
    found && /^    / { next }
    found { exit }
    index($0, pat) == 1 { found = 1; hex = "" }
    END { print hex }
  ' "$1"
}
# The first message whose header contains <pattern> and whose octets end with
# <suffix>; prints "<id> <hex>".
whole_ending() { # <log> <header substring> <hex suffix>
  awk -v pat="$2" -v suf="$3" '
    function flush() {
      if (found && substr(hex, length(hex) - length(suf) + 1) == suf) { print id, hex; exit }
      found = 0
    }
    /^    [0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / { if (found) { line = substr($0, 11, 47); gsub(/ /, "", line); hex = hex line }; next }
    /^    / { next }
    { flush() }
    $0 ~ pat { found = 1; hex = ""; match($0, / id=[0-9]+/); id = substr($0, RSTART + 4, RLENGTH - 4) }
    END { flush() }
  ' "$1"
}
# A 1.1 reply with its request id blanked, so a reply recorded under one
# client's numbering can be compared to another client's.
sans_id_11_reply() { printf '%s' "${1:0:32}........${1:40}"; }

rec_check() { # <const name> <live hex> [what]
  local want; want=$(python3 "$PY" recorded --rs "$RS" --name "$1")
  if [ "$want" = "$2" ] && [ -n "$2" ]; then
    ok "$1${3:+ ($3)} = $2, as recorded"
  else
    fail "$1${3:+ ($3)}: live '$2', recorded '$want' — the recording no longer describes the wire, or the wire changed"
  fi
}
rec_check_sans_id() { # <const name> <live hex> <what>
  local want; want=$(python3 "$PY" recorded --rs "$RS" --name "$1")
  if [ -n "$2" ] && [ "$(sans_id_11_reply "$want")" = "$(sans_id_11_reply "$2")" ]; then
    ok "$1 ($3) = $2, as recorded apart from the request id (recorded ${want:32:8}, live ${2:32:8} — the two clients' numbering)"
  else
    fail "$1 ($3): live '$2', recorded '$want' (request id disregarded) — the recording no longer describes the peer, or the peer changed"
  fi
}

echo "building"
if ! cargo build -q --bin spike-wide 2>"$D/build.log"; then
  fail "spike-wide did not build"; sed 's/^/       /' "$D/build.log" | head -5
  exit 1
fi

# ── A. JacORB client -> our Rust server, profile republished at 1.1 ─────────
echo "JacORB client -> our Rust server (GIOP 1.1)"
start_rust_server a && start_tap a "$D/a.server.ior" --minor 1 || exit 1
run_wide_client a "$D/a.tapped.ior"; a_rc=$?
stop "$TAP_PID"; stop "$RUST_PID"
a_seen=$(requests_seen "$D/a.tap.log")
a_first=$(grep -c "^first request at GIOP 1.1 (Big)$" "$D/a.rust.log")
a_dec_w=$(grep -c "^served echo_wchar #[0-9]* at GIOP 1.1 (Big) UTF-16 (0x00010109): decoded U+0077$" "$D/a.rust.log")
a_dec_han=$(grep -c "^served echo_wchar #[0-9]* at GIOP 1.1 (Big) UTF-16 (0x00010109): decoded U+$EXPECT_HAN$" "$D/a.rust.log")
a_dec_feff=$(grep -c "^served echo_wchar #[0-9]* at GIOP 1.1 (Big) UTF-16 (0x00010109): decoded U+FEFF$" "$D/a.rust.log")
a_ok=$(grep -c "^  ok   echo_wchar" "$D/a.client.log")
a_han=$(grep -c "echo_wchar\[U+D55C\] -> U+$EXPECT_HAN\$" "$D/a.client.log")
if [ "$a_seen" = "1.1 BE" ] && [ "$a_first" -eq 1 ] && [ "$a_dec_w" -eq 1 ] && [ "$a_dec_han" -eq 1 ] && [ "$a_dec_feff" -eq 1 ]; then
  ok "JacORB dials our IOR republished at IIOP 1.1 and speaks GIOP $a_seen (tap headers, our server's log agree); our reader decoded U+0077, U+$EXPECT_HAN, U+FEFF from its octets"
else
  fail "JacORB -> our server at 1.1: tap saw '${a_seen:-nothing}', server first-request line x$a_first, decoded w/han/feff = $a_dec_w/$a_dec_han/$a_dec_feff (han expected U+$EXPECT_HAN)"
  grep -E "^served|^refused|^first" "$D/a.rust.log" | head -5 | sed 's/^/       /'
fi
if [ "$a_ok" -eq 3 ] && [ "$a_han" -eq 1 ]; then
  ok "JacORB's user gets 'w', '한' and U+FEFF back from our real server's big-endian 1.1 replies (U+D55C -> U+$EXPECT_HAN); U+FEFF is data at 1.1 on both sides"
else
  fail "JacORB's user: $a_ok/3 units back as sent, U+D55C -> U+$EXPECT_HAN seen $a_han time(s) (client rc=$a_rc)"
  grep -E "FAIL|ok" "$D/a.client.log" | head -4 | sed 's/^/       /'
fi
# Behaviour, gated because a change must be visible: the lone surrogate JacORB
# can ask for is not a character to our reader.
a_lone_java=$(grep -c "^  FAIL echo_wchar\[U+D83D\] raised org.omg.CORBA.MARSHAL" "$D/a.client.log")
a_lone_rust=$(grep -c "^refused echo_wchar #[0-9]* at GIOP 1.1 (Big): received bytes are not valid UTF-16" "$D/a.rust.log")
if [ "$a_rc" -eq 1 ] && [ "$a_lone_java" -eq 1 ] && [ "$a_lone_rust" -eq 1 ]; then
  ok "recorded: JacORB's lone surrogate d8 3d is refused by our real reader — MARSHAL to JacORB's user (the hand-built peer of jacorb_wchar11.sh passed it through as octets; the Rust reader does not)"
else
  fail "lone surrogate: expected our reader to refuse it with MARSHAL (java rc 1); got rc=$a_rc, java MARSHAL line x$a_lone_java, server refusal x$a_lone_rust"
fi
info "little-endian replies from our real server to JacORB are unmeasurable: it answers in the request's order and JacORB requests big-endian only — the real server's LE reply octets are taken in arm C and checked against OUR_REPLY_HAN_LE below"
a_ctx=$(grep -o 'codesets([^)]*)' "$D/a.tap.log" | head -1)
[ -n "$a_ctx" ] && echo "       negotiated: $a_ctx"

# ── B. our Rust client -> a JacORB server advertising IIOP 1.1 ──────────────
echo "our Rust client -> JacORB server (GIOP 1.1)"
( cd "$JDIR" && exec "$JH/bin/java" -cp "$JCP" -Djacorb.giop_minor_version=1 -DOAIAddr=127.0.0.1 \
    WideServer "$D/j.ior" ) >"$D/j.log" 2>&1 &
JSRV_PID=$!; PIDS+=("$JSRV_PID")
if ! wait_for_file "$D/j.ior" 30; then
  fail "JacORB WideServer did not publish an IOR"; sed 's/^/       /' "$D/j.log" | tail -5
  exit 1
fi
sleep 0.5
start_tap b "$D/j.ior" || exit 1
# shellcheck disable=SC2086
run_our_client b.be "$D/b.tapped.ior" be $UNITS; b_be_rc=$?
# shellcheck disable=SC2086
run_our_client b.le "$D/b.tapped.ior" le $UNITS; b_le_rc=$?
stop "$TAP_PID"; stop "$JSRV_PID"
b_prof=$(sed -n 's/^  endpoint .*(IIOP \([0-9]\.[0-9]\)).*/\1/p' "$D/b.be.out" | head -1)
for order in be le; do
  eval "rc=\$b_${order}_rc"
  ORD=$(echo "$order" | tr a-z A-Z)
  n_ok=$(grep -c "^  ok   " "$D/b.$order.out")
  n_han=$(grep -c "^  ok   echo_wchar\[U+D55C\] -> U+$EXPECT_HAN " "$D/b.$order.out")
  n_txt=$(grep -c "^  ok   echo_wstring\[.*\] -> the same" "$D/b.$order.out")
  n_req=$(grep -c "C->S GIOP 1.1 Request size=[0-9]* $ORD" "$D/b.tap.log")
  n_rep=$(grep -c "S->C GIOP 1.1 Reply size=[0-9]* BE" "$D/b.tap.log")
  if [ "$rc" -eq 0 ] && [ "$n_ok" -eq 5 ] && [ "$n_han" -eq 1 ] && [ "$n_txt" -eq 2 ] && [ "$n_req" -ge 5 ] && [ "$b_prof" = "1.1" ]; then
    ok "our $ORD 1.1 requests to a JacORB server (profile IIOP $b_prof): 'w', '한', U+FEFF and both texts echoed as sent (U+D55C -> U+$EXPECT_HAN); $n_req $ORD requests on the wire at 1.1, JacORB replied BE"
  else
    fail "our $ORD 1.1 requests: rc=$rc, $n_ok/5 ok, U+D55C -> U+$EXPECT_HAN seen $n_han time(s), texts $n_txt/2, $n_req $ORD requests at 1.1 (profile '${b_prof:-?}'), $n_rep BE replies"
    grep -E "FAIL|ok" "$D/b.$order.out" | head -5 | sed 's/^/       /'
  fi
done
info "our client cannot ask for a lone surrogate (not a char) or send U+1F600 as one wchar (two units): both refusals are ours, before the wire — jacorb_wchar11.sh's hand-built client is what carries those to JacORB"

# ── C. our Rust client -> our Rust server at 1.0 / 1.1 / 1.2, both orders ───
echo "our Rust client -> our Rust server (GIOP 1.0, 1.1, 1.2)"
start_rust_server c || exit 1
for minor in 0 1 2; do
  start_tap "c$minor" "$D/c.server.ior" --minor "$minor" || exit 1
  for order in be le; do
    ORD=$(echo "$order" | tr a-z A-Z)
    case $minor in
      2) run_our_client "c$minor.$order" "$D/c$minor.tapped.ior" "$order" 0077 D55C D83D; rc=$?
         # U+FEFF at 1.2, on its own so its outcome is recorded and not counted.
         "$BIN" call "$D/c$minor.tapped.ior" "$order" FEFF >"$D/c$minor.$order.feff.out" 2>&1 ;;
      # shellcheck disable=SC2086
      *) run_our_client "c$minor.$order" "$D/c$minor.tapped.ior" "$order" $UNITS; rc=$? ;;
    esac
    n_req=$(grep -c "C->S GIOP 1.$minor Request size=[0-9]* $ORD" "$D/c$minor.tap.log")
    n_rep=$(grep -c "S->C GIOP 1.$minor Reply size=[0-9]* $ORD" "$D/c$minor.tap.log")
    case $minor in
      0)
        n_ref_c=$(grep -c "sent as two raw octets under GIOP 1.0 -> the server refused it: MARSHAL (OMG minor 6)" "$D/c0.$order.out")
        n_ref_s=$(grep -c "^refused echo_wchar #[0-9]* at GIOP 1.0 ($([ "$order" = be ] && echo Big || echo Little)): wchar is not legal" "$D/c.rust.log")
        n_txt=$(grep -c "wstring is illegal under GIOP 1.0; our codec refuses" "$D/c0.$order.out")
        if [ "$rc" -eq 0 ] && [ "$n_ref_c" -eq 3 ] && [ "$n_ref_s" -eq 3 ] && [ "$n_txt" -eq 2 ] && [ "$n_req" -ge 3 ] && [ "$n_rep" -ge 3 ]; then
          ok "1.0 $ORD: wchar refused on both sides — our codec will not write it, and two raw octets sent anyway get MARSHAL (OMG minor 6) from our server, 3/3; wstring refused before the wire, 2/2; $n_req requests at GIOP 1.0 $ORD"
        else
          fail "1.0 $ORD: rc=$rc, client-side MARSHAL lines $n_ref_c/3, server refusals $n_ref_s/3, wstring refusals $n_txt/2, wire 1.0 $ORD requests $n_req replies $n_rep"
          grep -E "FAIL|ok|info" "$D/c0.$order.out" | head -4 | sed 's/^/       /'
        fi ;;
      1|2)
        want_units=$([ "$minor" -eq 1 ] && echo 3 || echo 2)
        n_units=$(grep -c "^  ok   echo_wchar\[U+[0-9A-F]*\] -> U+[0-9A-F]*  (reply id=[0-9]* GIOP 1.$minor" "$D/c$minor.$order.out")
        n_han=$(grep -c "^  ok   echo_wchar\[U+D55C\] -> U+$EXPECT_HAN  (reply id=[0-9]* GIOP 1.$minor" "$D/c$minor.$order.out")
        n_txt=$(grep -c "^  ok   echo_wstring\[.*\] -> the same .*GIOP 1.$minor" "$D/c$minor.$order.out")
        s_han=$(grep -c "^served echo_wchar #[0-9]* at GIOP 1.$minor ($([ "$order" = be ] && echo Big || echo Little)) UTF-16 (0x00010109): decoded U+$EXPECT_HAN$" "$D/c.rust.log")
        if [ "$rc" -eq 0 ] && [ "$n_units" -eq "$want_units" ] && [ "$n_han" -eq 1 ] && [ "$n_txt" -eq 2 ] && [ "$s_han" -eq 1 ] && [ "$n_req" -ge 4 ] && [ "$n_rep" -ge 4 ]; then
          ok "1.$minor $ORD: $n_units/$want_units units and both texts round-trip our stack; our server decoded U+$EXPECT_HAN and our client read it back as U+$EXPECT_HAN; $n_req requests and $n_rep replies at GIOP 1.$minor $ORD (replies follow the request's order)"
        else
          fail "1.$minor $ORD: rc=$rc, units $n_units/$want_units, U+D55C -> U+$EXPECT_HAN client x$n_han server x$s_han, texts $n_txt/2, wire 1.$minor $ORD requests $n_req replies $n_rep"
          grep -E "FAIL|ok" "$D/c$minor.$order.out" | head -4 | sed 's/^/       /'
        fi ;;
    esac
  done
  stop "$TAP_PID"
done
# What the 1.2 arm cannot carry, recorded and not gated. Both writers here
# (ours and JacORB 3.9's, measured 2026-08-19 with the same binary) write
# U+FEFF as `02 fe ff`, and both readers take a leading fe ff for the mark
# §9.3.1.6 says to remove — JacORB hands its user U+0000, our reader refuses
# the empty remainder with MARSHAL. codeset.rs is outside this batch; the
# defect is named in tests/wide_1_1_from_a_peer.rs so a fix must revise it.
for order in be le; do
  ORD=$(echo "$order" | tr a-z A-Z)
  feff_line=$(grep -E "^  (ok|FAIL) +echo_wchar\[U\+FEFF\]" "$D/c2.$order.feff.out" | head -1 | sed 's/^  //')
  feff_octets=$(whole_ending "$D/c2.tap.log" "C->S GIOP 1.2 Request size=[0-9]* $ORD" "02feff" | cut -d' ' -f2)
  info "DEFECT recorded, not gated — U+FEFF as a 1.2 wchar, $ORD: we wrote ${feff_octets: -6}, and ${feff_line:-no line}"
done
stop "$RUST_PID"

# ── R. the live octets against the recording ────────────────────────────────
echo "recording"
# JacORB's whole echo_wchar(U+D55C) request as our real server received it,
# and our real server's big-endian reply to it.
set -- $(whole_ending "$D/a.tap.log" "C->S GIOP 1.1 Request" "d55c")
a_id="${1:-}"; a_req="${2:-}"
rec_check JACORB_REQUEST_HAN "$a_req" "JacORB's request, received by our real server"
rec_check OUR_REPLY_HAN_BE "$(whole "$D/a.tap.log" "[1] S->C GIOP 1.1 Reply size=14 BE id=${a_id:-?} ")" "our real server's reply to JacORB"
# Our own 1.1 big-endian request for U+D55C is octet for octet the one JacORB
# wrote — same key, same layout, same id — and our real server's replies to
# our own client in each order are the two forms JacORB's user read as U+D55C.
set -- $(whole_ending "$D/c1.tap.log" "C->S GIOP 1.1 Request size=[0-9]* BE" "d55c")
c_id="${1:-}"; c_req="${2:-}"
rec_check JACORB_REQUEST_HAN "$c_req" "our own client's BE request, id ${c_id:-?}"
rec_check OUR_REPLY_HAN_BE "$(whole "$D/c1.tap.log" "[1] S->C GIOP 1.1 Reply size=14 BE id=${c_id:-?} ")" "our real server to our BE client"
set -- $(whole_ending "$D/c1.tap.log" "C->S GIOP 1.1 Request size=[0-9]* LE" "5cd5")
c_lid="${1:-}"
rec_check OUR_REPLY_HAN_LE "$(whole "$D/c1.tap.log" "[2] S->C GIOP 1.1 Reply size=14 LE id=${c_lid:-?} ")" "our real server to our LE client"
# JacORB's reply to our real client's U+D55C request, in each order.
set -- $(whole_ending "$D/b.tap.log" "C->S GIOP 1.1 Request size=[0-9]* BE" "d55c")
b_id="${1:-}"
rec_check_sans_id JACORB_REPLY_HAN "$(whole "$D/b.tap.log" "[1] S->C GIOP 1.1 Reply size=14 BE id=${b_id:-?} ")" "to our real BE request"
set -- $(whole_ending "$D/b.tap.log" "C->S GIOP 1.1 Request size=[0-9]* LE" "5cd5")
b_lid="${1:-}"
rec_check_sans_id JACORB_REPLY_HAN "$(whole "$D/b.tap.log" "[2] S->C GIOP 1.1 Reply size=14 BE id=${b_lid:-?} ")" "to our real LE request"

echo
echo "logs: $D"
if [ "$FAILS" -eq 0 ]; then
  echo "wide rust: PASS"
  exit 0
fi
echo "wide rust: FAIL — $FAILS line(s)"
exit 1
