#!/usr/bin/env bash
# Project check harness: every Phase 0 feasibility assumption plus the
# Phase 1 wire and licence checks. Exit code is the verdict.
#
# Named run_checks.sh until Phase 1 outgrew it.
#
# The omniORB fixture is LGPL/GPL and is used only as a wire peer and a
# conformance oracle. Nothing here links it into Orbweaver. See docs/PLAN.md §10.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
fail_total=0
skipped=0

hr() { printf '\n\033[1m%s\033[0m\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }
need omniidl
need cargo

# Kills the fixture and waits for it to actually be gone. Signalling is
# asynchronous, so returning early lets the next fixture race a dying process.
cleanup() {
  pkill -f echo_server.py >/dev/null 2>&1 || true
  for _ in $(seq 1 50); do
    pgrep -f echo_server.py >/dev/null 2>&1 || return 0
    sleep 0.1
  done
}
trap cleanup EXIT

# Waits for the fixture to actually publish an IOR.
#
# The wait must sleep. An earlier version spun without sleeping, which took
# microseconds and therefore did not wait at all; it only looked correct
# because `cargo run` had to compile first and accidentally covered the race.
# Once the build was warm the race surfaced as phantom GIOP timeouts.
start_server() {
  cleanup
  rm -f "$ROOT/spikes/echo.ior"
  ( cd "$ROOT/spikes" && exec python3 echo_server.py "$@" >/dev/null 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/echo.ior" ] && { sleep 0.2; return 0; }  # settle after publish
    sleep 0.1
  done
  echo "  FAIL fixture did not publish an IOR within 10s"
  return 1
}

# Starts OUR server. Distinct from start_server, which launches the omniORB
# fixture; conflating the two silently pointed a check at the wrong process.
JH_CHECK=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP_CHECK="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"

start_rust_server() {
  pkill -f spike-server >/dev/null 2>&1 || true
  rm -f "$ROOT/spikes/server.ior"
  ( cd "$ROOT" && exec cargo run -q --bin spike-server -- spikes/server.ior 127.0.0.1 0 \
      >/tmp/orbweaver-srv.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; return 0; }
    sleep 0.1
  done
  echo "  FAIL our server did not publish an IOR"
  return 1
}

# ── Unit tests ───────────────────────────────────────────────────────────────
hr "unit tests (CDR + GIOP)"
if cargo test --workspace --quiet 2>&1 | grep -q "^error"; then
  echo "  FAIL cargo test"; fail_total=$((fail_total+1))
else
  echo "  ok   cargo test --workspace"
fi

# ── Lint (runs before the oracle, on purpose) ────────────────────────────────
hr "licence boundary"
if cargo tree --workspace 2>/dev/null | grep -qiE "omniorb|jacorb"; then
  echo "  FAIL an ORB fixture has become a dependency"; fail_total=$((fail_total+1))
else
  echo "  ok   no ORB fixture appears in cargo tree"
fi
# NOTICE promises that --no-default-features drops encoding_rs and the
# BSD-3-Clause obligation with it. That promise is testable, so it is tested.
if cargo tree -p orbweaver-giop --no-default-features 2>/dev/null | grep -q encoding_rs; then
  echo "  FAIL --no-default-features still pulls encoding_rs; NOTICE is wrong"
  fail_total=$((fail_total+1))
else
  echo "  ok   --no-default-features drops encoding_rs, as NOTICE states"
fi
if cargo test -p orbweaver-giop --lib --no-default-features --quiet >/dev/null 2>&1; then
  echo "  ok   the attribution-free build still passes its tests"
else
  echo "  FAIL the attribution-free build does not build or test"; fail_total=$((fail_total+1))
fi

hr "orbweaver-idl — our parser against the oracle"
# The acceptance criterion is agreement, not taste: omniidl accepts every
# golden file and rejects every negative one, so anywhere we differ we are
# wrong. Semantic negatives are excluded here and belong to the semantic pass.
if cargo test -p orbweaver-idl --quiet >/dev/null 2>&1; then
  echo "  ok   accepts all $(ls corpus/golden/*.idl | wc -l | tr -d ' ') golden files and the 20-file benchmark"
  echo "  ok   rejects the syntactic negatives, including unescaped keywords"
else
  echo "  FAIL our parser disagrees with the oracle"
  cargo test -p orbweaver-idl 2>&1 | grep -E "we do not|accepted them" | head -3 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

hr "IDL semantics — full agreement with the oracle"
# The interim regex lint (spikes/idl_lint.py) is retired: orbweaver-idl now
# walks a real scope tree, so the identifier rules are expressed once instead
# of re-approximated for each syntactic shape they take — which is how the
# regex missed operation names, and struct scopes before that.
neg_missed=""
for f in corpus/negative/*.idl; do
  if cargo run -q --bin idl-check -- "$f" >/dev/null 2>&1; then
    neg_missed="$neg_missed $(basename "$f")"
  fi
done
if [ -z "$neg_missed" ]; then
  echo "  ok   rejects all $(ls corpus/negative/*.idl | wc -l | tr -d ' ') negatives, syntactic and semantic"
else
  echo "  FAIL the oracle rejects these and we accept them:$neg_missed"
  fail_total=$((fail_total+1))
fi
# stdout only: a build warning on stderr is not an IDL diagnostic.
cargo build -q --bin idl-check 2>/dev/null
clean_out=$(cargo run -q --bin idl-check -- corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl 2>/dev/null)
if [ -z "$clean_out" ]; then
  echo "  ok   accepts every golden, benchmark and fixture file the oracle accepts"
else
  echo "$clean_out" | head -5 | sed 's/^/  FAIL /'
  fail_total=$((fail_total+1))
fi

# ── Golden corpus ────────────────────────────────────────────────────────────
hr "golden IDL corpus (must all compile)"
gp=0; gf=0
for f in corpus/golden/*.idl; do
  if [ -z "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then gp=$((gp+1)); else gf=$((gf+1)); echo "  FAIL $(basename "$f")"; fi
done
echo "  $gp pass / $gf fail"
[ "$gf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Negative corpus ──────────────────────────────────────────────────────────
hr "negative IDL corpus (must all be rejected)"
np=0; nf=0
for f in corpus/negative/*.idl; do
  if [ -n "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then np=$((np+1)); else nf=$((nf+1)); echo "  FAIL $(basename "$f") compiled but should not"; fi
done
echo "  $np correctly rejected / $nf wrongly accepted"
[ "$nf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Assumption C ─────────────────────────────────────────────────────────────
hr "assumption C — IDL 4 @annotation acceptance in a deployed compiler"
c1=$(omniidl -b dump corpus/annotations/c1-idl4-annotation.idl 2>&1 >/dev/null)
c3=$(omniidl -b dump corpus/annotations/c3-structured-comment.idl 2>&1 >/dev/null)
if [ -n "$c1" ]; then echo "  confirmed  @annotation REJECTED by omniidl (risk R1 is real)"; else echo "  surprise   @annotation accepted — revisit the SIDL plan"; fi
if [ -z "$c3" ]; then echo "  ok         structured-comment fallback compiles"; else echo "  FAIL       fallback does not compile"; fail_total=$((fail_total+1)); fi

# ── Assumption B ─────────────────────────────────────────────────────────────
hr "assumption B — generated IDL compiles"
bp=0; bf=0
for f in corpus/requirements/generated/R*.idl; do
  if [ -z "$(omniidl -b dump "$f" 2>&1 >/dev/null)" ]; then bp=$((bp+1)); else bf=$((bf+1)); echo "  FAIL $(basename "$f")"; fi
done
echo "  $bp/20 compile after self-repair (first pass was 13/20 — see docs/PHASE0.md)"
[ "$bf" -eq 0 ] || fail_total=$((fail_total+1))

# ── Assumption A ─────────────────────────────────────────────────────────────
hr "assumption A — GIOP interop against a stock ORB"
if start_server; then
  # Capture before matching. Piping into `grep -q` closes the pipe on the
  # first match and SIGPIPEs the producer, which shows up as a phantom
  # failure — that bug cost a debugging cycle here already.
  interop=$(cargo run -q --bin spike-interop -- spikes/echo.ior 2>&1)
  printf '%s\n' "$interop" > /tmp/orbweaver-a.log
  if printf '%s' "$interop" | grep -q "assumption A: PASS"; then
    echo "  ok   both byte orders interoperated"
  else
    echo "  FAIL see /tmp/orbweaver-a.log"
    printf '%s' "$interop" | grep -E "^  FAIL" | head -3 | sed 's/^/     /'
    fail_total=$((fail_total+1))
  fi
else
  # A fixture that will not start is an unmeasured assumption, not a pass.
  fail_total=$((fail_total+1))
fi
cleanup

# ── Assumption D ─────────────────────────────────────────────────────────────
hr "assumption D — IOR endpoint publishing"
if start_server; then
  adv=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1 | head -1)
  echo "  default publish: $adv"
  case "$adv" in
    *127.0.0.1*) echo "  note  loopback published; a container would publish its pod IP instead" ;;
    *)           echo "  confirmed  a routable-but-local address is published, not loopback (risk R7 is real)" ;;
  esac
else
  fail_total=$((fail_total+1))
fi
cleanup
if start_server -ORBendPoint giop:tcp::40404 -ORBendPointPublish giop:tcp:127.0.0.1:40404; then
  rewritten=$(cargo run -q --bin spike-dump -- spikes/echo.ior ping little 1 2>&1)
  echo "  rewritten publish: $(echo "$rewritten" | head -1)"
  if echo "$rewritten" | grep -q RESPONSE; then
    echo "  ok   endpoint rewriting works — mitigation for R7 is available"
  else
    echo "  FAIL endpoint rewriting did not produce a reachable reference"
    echo "$rewritten" | grep -E "no response|closed|error" | sed 's/^/       /'
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi

# ── Reverse interop: a stock ORB calls US ───────────────────────────────────
hr "reverse interop — omniORB client against our server"
if start_rust_server; then
  rev_fail=0
  for v in 1.0 1.1 1.2; do
    if python3 spikes/reverse_client.py spikes/server.ior -ORBmaxGIOPVersion "$v" >/dev/null 2>&1; then
      echo "  ok   omniORB client at GIOP $v -> our server, 5/5"
    else
      echo "  FAIL omniORB client at GIOP $v could not call our server"; rev_fail=1
    fi
  done
  # "We tested three versions" is only true if the peer used three. An ORB that
  # ignored the option would otherwise give three identical passes proving one.
  seen=$(grep -c "first request at GIOP" /tmp/orbweaver-srv.log 2>/dev/null || echo 0)
  if [ "$seen" -eq 3 ]; then
    echo "  ok   server confirms three distinct GIOP versions were received"
  else
    echo "  FAIL server saw $seen distinct versions, not 3 — the option was ignored"
    rev_fail=1
  fi
  [ "$rev_fail" -eq 0 ] || fail_total=$((fail_total+1))
else
  fail_total=$((fail_total+1))
fi
pkill -f spike-server >/dev/null 2>&1 || true

# ── Fragmentation ────────────────────────────────────────────────────────────
hr "GIOP fragmentation"
# Neither available peer emits GIOP fragments: omniORB's giopMaxMsgSize is a
# hard cap that raises MARSHAL rather than a split threshold, and JacORB 3.9
# has no GIOP fragmentation property at all. So the independent evidence runs
# in one direction only — we fragment, they reassemble — and the receiver is
# covered by round-trip against our own (peer-validated) emitter. Stated here
# rather than implied by a green line.
pkill -f spike-server >/dev/null 2>&1 || true
rm -f "$ROOT/spikes/server.ior"
( cd "$ROOT" && ORBWEAVER_FRAGMENT_THRESHOLD=4096 exec cargo run -q --bin spike-server -- \
    spikes/server.ior 127.0.0.1 0 >/tmp/orbweaver-frag.log 2>&1 & )
frag_up=0
for _ in $(seq 1 100); do
  [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; frag_up=1; break; }
  sleep 0.1
done
if [ "$frag_up" -eq 0 ]; then
  echo "  FAIL fragmenting server did not start"; fail_total=$((fail_total+1))
else
  ffail=0
  out=$(python3 spikes/reverse_client.py spikes/server.ior 2>&1)
  if printf '%s' "$out" | grep -q "failures: 0"; then
    echo "  ok   omniORB reassembled our fragments (250 KB at a 4 KB threshold)"
  else
    echo "  FAIL omniORB could not reassemble our fragments"; ffail=1
  fi
  if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
    out=$(cd "$ROOT/spikes/jacorb" && "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Client ../server.ior 2>&1)
    if printf '%s' "$out" | grep -q "failures: 0"; then
      echo "  ok   JacORB reassembled our fragments — a second, independent reader"
    else
      echo "  FAIL JacORB could not reassemble our fragments"; ffail=1
    fi
  else
    echo "  SKIPPED  JacORB half — fixture absent"
    skipped=$((skipped+1))
  fi
  echo "  note our *receiver* has no independent validation: no available peer emits"
  echo "       GIOP fragments, so it is covered by round-trip against our own emitter"
  [ "$ffail" -eq 0 ] || fail_total=$((fail_total+1))
fi
pkill -f spike-server >/dev/null 2>&1 || true

# ── Registry: does IDL-derived type metadata match the wire? ────────────────
hr "type registry — TypeCode derived from IDL vs the peer's"
# Deriving a TypeCode and encoding it with our own encoder proves only that two
# pieces of our code agree. The question is whether a stock ORB produces the
# same description from the same IDL.
if start_server_omni_echo 2>/dev/null || start_server; then
  rc=$(cargo run -q --bin registry-check -- spikes/echo.ior spikes/echo.idl spike::Ragged 2>/dev/null)
  if printf '%s' "$rc" | grep -q "registry: PASS"; then
    echo "  ok   omniORB agrees with the TypeCode we derived for spike::Ragged"
  else
    echo "  FAIL omniORB disagrees with our derived TypeCode"
    printf '%s' "$rc" | grep -E "derived|returned" | head -2 | sed 's/^/       /'
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  pkill -f "classes Server" >/dev/null 2>&1 || true
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jreg.log 2>&1 & )
  jr=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jr=1; break; }
    sleep 0.1
  done
  if [ "$jr" -eq 1 ]; then
    rc=$(cargo run -q --bin registry-check -- spikes/jacorb.ior spikes/echo.idl spike::Ragged 2>/dev/null)
    if printf '%s' "$rc" | grep -q "registry: PASS"; then
      echo "  ok   JacORB agrees too — two independent derivations of one IDL type"
    else
      echo "  FAIL JacORB disagrees with our derived TypeCode"; fail_total=$((fail_total+1))
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; fail_total=$((fail_total+1))
  fi
  pkill -f "classes Server" >/dev/null 2>&1 || true
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi

# ── Naming: resolve a target the way a deployment does ───────────────────────
hr "object-reference acquisition — corbaname: through a real naming service"
pkill -f omniNames >/dev/null 2>&1 || true
pkill -f register_name >/dev/null 2>&1 || true
sleep 0.5
rm -rf /tmp/orbweaver-names && mkdir -p /tmp/orbweaver-names
( omniNames -start 2809 -logdir /tmp/orbweaver-names >/tmp/orbweaver-names/out.log 2>&1 & )
names_up=0
for _ in $(seq 1 60); do
  lsof -nP -iTCP:2809 >/dev/null 2>&1 && { names_up=1; break; }
  sleep 0.2
done
if [ "$names_up" -eq 0 ]; then
  echo "  FAIL omniNames did not start on 2809"; fail_total=$((fail_total+1))
else
  ( cd "$ROOT/spikes" && exec python3 register_name.py >/tmp/orbweaver-reg.log 2>&1 & )
  reg_up=0
  for _ in $(seq 1 100); do
    grep -q READY /tmp/orbweaver-reg.log 2>/dev/null && { reg_up=1; break; }
    sleep 0.1
  done
  if [ "$reg_up" -eq 0 ]; then
    echo "  FAIL could not bind a name into the naming service"; fail_total=$((fail_total+1))
  else
    nm=$(cargo run -q --bin spike-naming 2>&1)
    if printf '%s' "$nm" | grep -q "naming: PASS"; then
      printf '%s\n' "$nm" | grep "^  ok" | sed 's/^/  /'
      # The default in corbaloc::host is GIOP 1.0, so this path only works
      # because of the version negotiation from batch 1. Assert it rather than
      # let a silent upgrade to 1.2 hide a regression.
      if printf '%s' "$nm" | grep -q "GIOP 1.0"; then
        echo "  ok   naming service contacted at GIOP 1.0, as corbaloc defaults require"
      else
        echo "  FAIL expected GIOP 1.0 for a corbaloc URL with no version"; fail_total=$((fail_total+1))
      fi
    else
      echo "  FAIL naming resolution"; printf '%s' "$nm" | grep -iE "fail|error" | head -3 | sed 's/^/       /'
      fail_total=$((fail_total+1))
    fi
  fi
fi
pkill -f register_name >/dev/null 2>&1 || true
pkill -f omniNames >/dev/null 2>&1 || true

# ── Second peer: JacORB, both directions ─────────────────────────────────────
hr "second peer — JacORB (independent implementation)"
JH=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"
if [ ! -d "$ROOT/spikes/jacorb/classes" ] || [ ! -x "$JH/bin/java" ]; then
  # Not a pass. An absent fixture means the claim is unmeasured, and the
  # summary says so rather than letting silence read as success.
  echo "  SKIPPED  fixture absent — run spikes/jacorb/setup.sh (needs JDK 21)"
  skipped=$((skipped+1))
else
  jfail=0
  # JacORB client -> our Rust server.
  if start_rust_server; then
    out=$(cd "$ROOT/spikes/jacorb" && "$JH/bin/java" -cp "$JCP" Client ../server.ior 2>&1)
    if printf '%s' "$out" | grep -q "failures: 0"; then
      echo "  ok   JacORB client -> our server, 5/5"
    else
      echo "  FAIL JacORB client -> our server"; printf '%s' "$out" | grep FAIL | head -3 | sed 's/^/       /'
      jfail=1
    fi
    # JacORB is big-endian where omniORB was little-endian, so this exercises a
    # decode path the first peer never touched. Worth asserting, not assuming.
    if grep -q "first request at GIOP 1.2 (Big)" /tmp/orbweaver-srv.log 2>/dev/null; then
      echo "  ok   big-endian request path exercised by the second peer"
    else
      echo "  FAIL expected a big-endian request from JacORB"; jfail=1
    fi
  else
    jfail=1
  fi
  pkill -f spike-server >/dev/null 2>&1 || true

  # Our Rust client -> JacORB server.
  pkill -f "classes Server" >/dev/null 2>&1 || true
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH/bin/java" -cp "$JCP" Server ../jacorb.ior >/tmp/orbweaver-jacorb.log 2>&1 & )
  jup=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jup=1; break; }
    sleep 0.1
  done
  if [ "$jup" -eq 1 ]; then
    out=$(cargo run -q --bin spike-interop -- spikes/jacorb.ior 2>&1)
    if printf '%s' "$out" | grep -q "assumption A: PASS"; then
      echo "  ok   our client -> JacORB server, 20/20 both byte orders"
      cs=$(printf '%s' "$out" | grep -m1 "negotiated char codeset" | sed 's/.*: //')
      echo "  ok   codeset negotiated with a second peer: $cs"
    else
      echo "  FAIL our client -> JacORB server"; printf '%s' "$out" | grep "  FAIL" | head -3 | sed 's/^/       /'
      jfail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; jfail=1
  fi
  pkill -f "classes Server" >/dev/null 2>&1 || true
  [ "$jfail" -eq 0 ] || fail_total=$((fail_total+1))
fi

hr "verdict"
if [ "$skipped" -gt 0 ]; then
  echo "  $skipped check group(s) SKIPPED — those claims are unmeasured, not passing"
fi
if [ "$fail_total" -eq 0 ]; then
  echo "  all measured checks green"
else
  echo "  $fail_total check group(s) failed"
fi
exit "$fail_total"
