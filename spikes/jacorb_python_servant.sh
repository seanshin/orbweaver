#!/usr/bin/env bash
# A Python servant behind our ORB, called by JacORB — the byte order omniORB
# cannot reach.
#
# D030 §3: *"A language is a target when its generated code is measured against
# a peer that is not us, in both byte orders, and its refusals say the same
# sentences ours do."* The servant seam landed with the first half and reported
# the second honestly: omniORB emits its native order, and `orbweaver-giop`'s
# server replies in the *request's* order, so on a little-endian host every
# byte of that exchange is little-endian. Java's ORBs write big-endian, so
# JacORB is the peer that reaches the other half.
#
# What is measured, and in which order:
#
#   1. bytes    — every request JacORB wrote has its §15.4.1 flag bit read by a
#                 recording relay inside the test. "Java is big-endian" is a
#                 belief until that byte says so, and the assertion is over
#                 what was read, never over what the peer was told.
#   2. identity — the same driver run answered by a Python servant and by a
#                 Rust servant, replies compared **byte for byte**. That is the
#                 seam's own claim (python_servant.rs) made from the other
#                 endianness, over a socket, with a foreign peer choosing the
#                 order. Both encoders are ours, which is the exception
#                 CLAUDE.md names to "compare decoded values, never raw
#                 buffers"; the peer's own bytes are read and never compared.
#   3. transcript — JacORB's client gets the same answers omniORB's does, and
#                 nothing about the servant's language reaches it.
#
# Both IIOP 1.2 (JacORB's default) and 1.1 (reached the way
# spikes/jacorb_giop11.sh reaches it — by republishing the profile, because a
# peer's outbound version follows the profile it dialled).
#
# Exit 0 when every gated line is ok, 1 when any is FAIL, 2 when the fixture is
# absent — SKIPPED is unmeasured, never passing. A fixture that is present and
# will not *start* is a FAIL, not a skip.
#
# Negative controls, either of which must make this script exit 1 against an
# unchanged wire:
#
#   ./spikes/jacorb_python_servant.sh --expect-order little
#       the order assertion is told to expect the order the peer does not write
#   ./spikes/jacorb_python_servant.sh --perturb
#       the Rust servant answers one `sequence_no` the Python one would not,
#       and the byte comparison must name the reply and print both hex strings
#
# The harness group this script is meant to be run by, recommended rather than
# written: `spikes/run_checks.sh` is held by another batch, and CLAUDE.md's rule
# is not to edit a script while it may be running. Drop this beside the two
# existing JacORB groups, in their shape:
#
#   hr "JacORB -> a Python servant behind our ORB — the byte order omniORB cannot reach (D030 §3)"
#   # spikes/jacorb_python_servant.sh: the servant seam's own reported gap.
#   # omniORB emits its native order and our server replies in the request's
#   # order, so that leg is little-endian in both directions on this host;
#   # JacORB writes big-endian and the assertion is over §15.4.1's flag bit of
#   # the requests it actually wrote, at IIOP 1.2 and 1.1. The same driver run
#   # is then answered by a Python servant and a Rust one and the replies are
#   # compared byte for byte — the exception CLAUDE.md names, because both
#   # encoders are ours. Negative controls: `--expect-order little` goes red on
#   # the order line (left: ["big"] right: ["little"]); `--perturb` makes the
#   # Rust servant answer one sequence_no the Python one would not and goes red
#   # on the byte comparison, naming reply 2 of 11 with both hex strings.
#   jps=$(./spikes/jacorb_python_servant.sh 2>&1); jps_rc=$?
#   printf '%s\n' "$jps" | grep -E "^  (ok|FAIL|info|SKIPPED)" | cut -c1-150
#   if [ "$jps_rc" -eq 2 ]; then
#     skip_age absent git:spikes/jacorb_python_servant.sh
#   elif [ "$jps_rc" -ne 0 ]; then
#     echo "  FAIL JacORB -> a Python servant — see /tmp/orbweaver-jacorb-servant"
#     fail_total=$((fail_total+1))
#   fi
#
# Harness rules: the producer's exit status is read before anything it printed;
# nothing is piped into `grep -q` (output is captured and matched with a
# herestring); JacORB is a fixture and never a dependency, which is asserted
# here rather than assumed.
#
# *우리 ORB 뒤의 Python 서번트를 JacORB가 호출한다. 바이트 순서는 피어의 언어가
# 아니라 요청 헤더의 플래그 바이트에서 읽는다. 같은 호출에 대한 Python 서번트와
# Rust 서번트의 응답은 바이트까지 같아야 한다.*
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

EXPECT_ORDER=big
PERTURB=
while [ $# -gt 0 ]; do
  case "$1" in
    --expect-order) EXPECT_ORDER="${2:-big}"; shift 2 ;;
    --perturb) PERTURB=1; shift ;;
    *) echo "usage: $0 [--expect-order big|little] [--perturb]"; exit 2 ;;
  esac
done

JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JDIR="$ROOT/spikes/jacorb"
D="${JACORB_SERVANT_DIR:-/tmp/orbweaver-jacorb-servant}"
FAILS=0

ok()   { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
info() { echo "  info $*"; }

# ── the fixture, before anything is claimed about it ────────────────────────
[ -x "$JH/bin/java" ] && [ -x "$JH/bin/javac" ] || {
  echo "  SKIPPED  JDK 21 absent at $JH — a big-endian peer is unmeasured, not passing (brew install openjdk@21)"
  exit 2
}
for jar in jacorb.jar jacorb-omgapi.jar jacorb-idl-compiler.jar jboss-rmi-api.jar slf4j-api-1.7.36.jar; do
  [ -s "$JDIR/lib/$jar" ] || {
    echo "  SKIPPED  JacORB fixture absent ($jar) — run spikes/jacorb/setup.sh --jars-only; a big-endian peer is unmeasured, not passing"
    exit 2
  }
done
[ -s "$JDIR/GaugeDriver.java" ] || { fail "spikes/jacorb/GaugeDriver.java is missing; the driver is part of the tree, not of the download"; exit 1; }
command -v python3 >/dev/null 2>&1 || {
  echo "  SKIPPED  python3 absent — there is no Python servant to call; unmeasured, not passing"
  exit 2
}

rm -rf "$D"; mkdir -p "$D"

# ── the licence boundary, which this group is entitled to weaken ────────────
# JacORB is run as a separate process and read from; nothing links it. The
# check reads `cargo tree`'s own exit status first, because a producer that
# could not run is an unmeasured check and never a pass — and matches with a
# herestring, because `grep -q` in a pipeline SIGPIPEs the producer and
# `pipefail` then reads a *found* forbidden dependency as "no match".
tree_out=$(cargo tree --workspace 2>&1); tree_rc=$?
if [ "$tree_rc" -ne 0 ]; then
  fail "cargo tree --workspace did not run (exit $tree_rc), so the licence boundary is unmeasured"
  head -3 <<<"$tree_out" | sed 's/^/       /'
elif grep -qiE "jacorb|omniorb|jboss-rmi" <<<"$tree_out"; then
  fail "a fixture reached the dependency graph — JacORB is LGPL and must never be linked"
  grep -iE "jacorb|omniorb|jboss-rmi" <<<"$tree_out" | head -3 | sed 's/^/       /'
else
  ok "cargo tree --workspace names no fixture: JacORB stays a separate process we read"
fi

# ── the measurement ─────────────────────────────────────────────────────────
echo "JacORB client -> our ORB -> a Python servant"
[ -n "$PERTURB" ] && info "--perturb: the Rust servant will answer one sequence_no the Python one would not"
[ "$EXPECT_ORDER" != big ] && info "--expect-order $EXPECT_ORDER: the wire is unchanged; this is the control"

# `env`, not a bare assignment prefix: a `${VAR:+NAME=1}` expansion is not an
# assignment word — bash decides that at parse time — so it would become the
# command name instead of an environment entry.
env ORBWEAVER_JACORB_EXPECT_ORDER="$EXPECT_ORDER" ${PERTURB:+ORBWEAVER_JACORB_PERTURB=1} \
  cargo test -q -p orbweaver-gen --test python_servant_wire \
    -- --exact jacorb_calls_a_python_servant --nocapture >"$D/test.log" 2>&1
test_rc=$?
out=$(cat "$D/test.log")

# The producer's status first. Everything below is about what it printed, and
# what it printed is only evidence if it ran.
if [ "$test_rc" -ne 0 ]; then
  fail "jacorb_calls_a_python_servant exited $test_rc"
  grep -E "panicked at|assertion|left:|right:|did not report|UNMEASURED" <<<"$out" | head -8 | sed 's/^/       /'
else
  # An absent fixture inside the test is a FAIL here: this script has already
  # established that the JDK, the jars, the driver and python3 are all present,
  # so UNMEASURED at this point means something would not start.
  if grep -q "UNMEASURED" <<<"$out"; then
    fail "the test reported UNMEASURED although every fixture this script checked is present — something would not start"
    grep "UNMEASURED" <<<"$out" | head -2 | sed 's/^/       /'
  fi
fi

# What was read off the wire, printed on every run so the bytes travel with the
# verdict rather than being quoted from a commit message later.
wire=$(grep "^read off the wire" <<<"$out")
same=$(grep "^byte-identical" <<<"$out")
sed 's/^/       /' <<<"$wire"
sed 's/^/       /' <<<"$same"

versions_seen=$(grep -c "^read off the wire" <<<"$wire")
identities=$(grep -c "^byte-identical" <<<"$same")
if [ "$test_rc" -eq 0 ] && [ "$versions_seen" -eq 2 ] && [ "$identities" -eq 2 ]; then
  ok "the peer's flag byte read at two IIOP versions, and both servants' replies byte-identical at each"
elif [ "$test_rc" -eq 0 ]; then
  fail "expected two versions measured and two byte-identity results, got $versions_seen and $identities"
fi

echo
echo "logs: $D/test.log"
if [ "$FAILS" -eq 0 ]; then
  echo "jacorb -> python servant: PASS"
  exit 0
fi
echo "jacorb -> python servant: FAIL — $FAILS line(s)"
exit 1
