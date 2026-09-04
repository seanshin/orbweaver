#!/usr/bin/env bash
# A peer that advertises a CSIv2 mechanism list, measured by the harness's own
# reader (D010 B2, PLAN-FIRST-COMPLETION §G lane D). A fixture driver, not a
# gate: run standalone, its exit code is the verdict for its own two legs.
#
# What it measures, and with what oracle: `spike-dump`'s `^csiv2` lines are
# exactly what the harness's identity group greps over `spikes/jacorb.ior`, so
# this driver uses the same reader rather than a second decoder that could
# drift from it. Two legs, because indistinguishability is evidence only
# beside a demonstration that distinguishing is possible:
#
#   control      the stock Server (no SAS policy) must read
#                "the target advertises no mechanism list" — the reader can say no;
#   measurement  CsiServer must read a mechanism list — the reader says yes to
#                an IOR that actually carries TAG_CSI_SEC_MECH_LIST (tag 33).
#
# `--swap` is the negative control for the measurement assertion: it runs the
# SAME assertion (lifted, not restated — one function, both callers) against
# the control peer's IOR, so it must exit 1 printing the FAIL. If --swap ever
# exits 0, the measurement assertion has stopped being able to go red.
#
# THE BOUND, stated where the work lands: this fixture buys the exchange path a
# measurement against an INDEPENDENT peer's advertisement — JacORB 3.9's, an
# implementation we did not write. It says nothing about a verifier's ACCEPTING
# direction: a verifier wrong in the accepting direction interoperates
# perfectly with every honest peer, which is stream C's recorded reason
# (crates/orbweaver-mcp/src/token.rs module docs, D002's rule) for leaving the
# verifier a trait this project does not implement. Do not read a green here as
# evidence about token verification.
#
# *이 픽스처가 사는 것은 독립 피어(JacORB)의 광고에 대한 측정이지, 검증기의 수용
# 방향이 아니다. 수용 방향으로 틀린 검증기는 모든 정직한 피어와 완벽히 상호운용
# 된다 — 그래서 검증기는 이 프로젝트가 구현하지 않는 trait로 남아 있다.*
#
# Exit codes: 0 verdict held · 1 a leg failed · 3 unmeasured (fixture or JDK
# absent — unmeasured is stated, never a pass).
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(pwd)
. "$ROOT/spikes/lib/accepting.sh"

SWAP=0
[ "${1:-}" = --swap ] && SWAP=1

JAVA_HOME_21=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"

if [ ! -x "$JAVA_HOME_21/bin/java" ]; then
  echo "UNMEASURED: no JDK 21 at $JAVA_HOME_21 (brew install openjdk@21)"; exit 3
fi
if [ ! -f "$ROOT/spikes/jacorb/classes/CsiServer.class" ]; then
  echo "UNMEASURED: spikes/jacorb/classes/CsiServer.class absent — run spikes/jacorb/setup.sh"; exit 3
fi

WORK=$(mktemp -d)
PID=""
cleanup() {
  # A JVM spawns no children here, so killing the pid IS reaping the tree.
  if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

# start_peer <MainClass> <ior-path> <log-path>; sets PID, waits until accepting.
start_peer() {
  rm -f "$2"
  ( cd "$ROOT/spikes/jacorb" && exec "$JAVA_HOME_21/bin/java" -cp "$JCP" "$1" "$2" ) \
    >"$3" 2>&1 &
  PID=$!
  if ! wait_accepting "$2" --deadline 30 --ready "$3" "^READY$"; then
    echo "  FAIL $1 did not come up; its log:"
    cat "$3"
    return 1
  fi
}

stop_peer() {
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  PID=""
}

# The harness's reader, verbatim: spike-dump's ^csiv2 lines. Producer status is
# read before the match — an IOR the reader could not decode is unmeasured,
# never "no".
read_csi() {
  local out
  if ! out=$(cargo run -q --bin spike-dump -- "$1" 2>/dev/null); then
    echo "FAIL spike-dump could not read $1"
    return 1
  fi
  grep '^csiv2' <<<"$out"
}

# The measurement assertion — ONE function, called by the measurement leg and
# lifted unchanged by --swap, so the control cannot restate it and drift.
# assert_mech_list <ior> <who>: 0 iff the IOR carries a readable mechanism list.
assert_mech_list() {
  local csi
  if ! csi=$(read_csi "$1"); then
    echo "  FAIL $2: the reader produced no csiv2 verdict"
    return 1
  fi
  if grep -q "advertises no mechanism list" <<<"$csi"; then
    echo "  FAIL $2 advertises no mechanism list"
    return 1
  fi
  if ! grep -q "mechanism(s)" <<<"$csi"; then
    echo "  FAIL $2: the reader printed neither verdict: $csi"
    return 1
  fi
  echo "  ok   $2 advertises:"
  sed 's/^/       /' <<<"$csi"
}

fail=0

# ── control: the stock peer advertises nothing, and the reader says so ──────
if start_peer Server "$WORK/plain.ior" "$WORK/plain.log"; then
  if plain_csi=$(read_csi "$WORK/plain.ior") \
     && grep -q "advertises no mechanism list" <<<"$plain_csi"; then
    echo "  ok   control: stock JacORB Server — $plain_csi"
  else
    echo "  FAIL control: stock Server was expected to advertise nothing; got: ${plain_csi:-<nothing>}"
    fail=1
  fi

  if [ "$SWAP" -eq 1 ]; then
    # Negative control: the measurement assertion over the peer that
    # advertises nothing. Expected outcome: the FAIL line above this exit,
    # and exit 1. An exit 0 here means the assertion cannot go red.
    if assert_mech_list "$WORK/plain.ior" "control peer under --swap"; then
      exit 0
    fi
    exit 1
  fi
else
  fail=1
fi
stop_peer

# ── measurement: CsiServer's IOR carries TAG_CSI_SEC_MECH_LIST ───────────────
if start_peer CsiServer "$WORK/csi.ior" "$WORK/csi.log"; then
  if ! assert_mech_list "$WORK/csi.ior" "CsiServer (JacORB 3.9, SAS/GSSUP)"; then
    echo "       its log:"
    sed 's/^/       /' "$WORK/csi.log"
    fail=1
  fi
else
  fail=1
fi
stop_peer

if [ "$fail" -eq 0 ]; then
  echo "  ok   two peers, two answers, one reader — the advertisement is measured, not assumed"
fi
exit "$fail"
