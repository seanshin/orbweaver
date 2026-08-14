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

# ── One harness at a time, machine-wide ──────────────────────────────────────
# The fixtures are killed by pattern (`pkill -f echo_server.py`) and the logs
# live at fixed /tmp paths, so two harnesses running at once destroy each
# other's fixtures and report failures that are about the scheduling, not the
# code. That has now happened twice — once in a worktree agent's run and once
# in the main tree, both times producing "Connection refused" against a peer
# that had been alive a moment earlier, and both times costing a diagnosis.
#
# Two fixes, because they cover different attackers. The lock stops a second
# harness; `fkill` below stops this harness from killing a fixture somebody
# started by hand in another checkout, which the lock cannot see. Neither
# touches the shared /tmp log paths, which are threaded through 46 places and
# are only a hazard for two concurrent harnesses — the case the lock removes.
#
# Refuse rather than queue: a harness that silently waits looks identical to a
# harness that hung, and the person who started the second one wants to know
# the first is running.
LOCK=/tmp/orbweaver-harness.lock
if ! mkdir "$LOCK" 2>/dev/null; then
  holder=$(cat "$LOCK/owner" 2>/dev/null || echo "unknown")
  # A holder that is gone is a crashed run, not a running one. Taking the lock
  # over is safe and refusing would make one killed harness wedge the machine
  # until somebody read this file — the failure mode of every lock that only
  # ever waits.
  holder_pid=$(printf '%s' "$holder" | awk '{print $2}')
  if [ -n "$holder_pid" ] && ! ps -p "$holder_pid" >/dev/null 2>&1; then
    echo "note: taking over a stale lock from a run that is no longer alive ($holder)"
    rm -rf "$LOCK"
    mkdir "$LOCK" 2>/dev/null || true
  fi
fi
if [ ! -d "$LOCK" ] || [ -s "$LOCK/owner" ]; then
  holder=$(cat "$LOCK/owner" 2>/dev/null || echo "unknown")
  echo "another harness is running (started by $holder)."
  echo "the fixtures are killed by pattern and the logs share /tmp paths, so two"
  echo "runs at once produce failures that are about the scheduling, not the code."
  echo "wait for it, or remove $LOCK if you are sure nothing is running."
  exit 2
fi
printf 'pid %s in %s at %s\n' "$$" "$ROOT" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >"$LOCK/owner"
# NOT a `trap ... EXIT` of its own: `cleanup` claims EXIT further down, and a
# second trap on the same signal REPLACES the first rather than adding to it.
# Releasing the lock is therefore folded into `cleanup`. This is not
# hypothetical tidiness — the first version of this lock did use its own trap,
# every run leaked a stale lock, and the next run refused to start.

hr() { printf '\n\033[1m%s\033[0m\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }

# ── Kill this run's fixtures, and only this run's ────────────────────────────
# `pkill -f echo_server.py` matches by command line, which is every checkout on
# the machine. The lock above stops two harnesses colliding; this stops a
# harness from killing a fixture a developer started by hand in another tree,
# which the lock cannot see.
#
# Scoped by process group: every fixture is started by this script, so it
# inherits this script's group. When the group cannot be read — a runner that
# reparents children, a `ps` without `pgid` — the fall-back is the old
# behaviour with a printed note, because a harness that silently stops killing
# fixtures leaks them into the next group and fails somewhere unrelated. A
# noisy wide kill beats a quiet leak.
own_pgid=$(ps -o pgid= -p $$ 2>/dev/null | tr -d ' ')
fkill() {
  local pat="$1" pid pgid hit=0 seen=0
  for pid in $(pgrep -f "$pat" 2>/dev/null); do
    seen=1
    pgid=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -n "$own_pgid" ] && [ "$pgid" = "$own_pgid" ]; then
      kill "$pid" 2>/dev/null && hit=1
    fi
  done
  if [ "$seen" = 1 ] && [ "$hit" = 0 ]; then
    echo "  note fixture $pat is running outside this process group; killing it wide" >&2
    pkill -f "$pat" >/dev/null 2>&1 || true
  fi
  return 0
}
need omniidl
need cargo

# Kills the fixture and waits for it to actually be gone. Signalling is
# asynchronous, so returning early lets the next fixture race a dying process.
cleanup() {
  fkill echo_server.py
  fkill evolution_server.py
  for _ in $(seq 1 50); do
    pgrep -f "echo_server.py|evolution_server.py" >/dev/null 2>&1 || return 0
    sleep 0.1
  done
}

# Starts the contract-evolution peer. `$1` is empty for the deployed version or
# --updated for the same service after an additive release.
start_evolution_server() {
  cleanup
  rm -f "$ROOT/spikes/evolution.ior"
  ( cd "$ROOT/spikes" && exec python3 evolution_server.py ${1:+"$1"} \
      >/tmp/orbweaver-evolution.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/evolution.ior" ] && { sleep 0.2; return 0; }
    sleep 0.1
  done
  fixture_died "evolution fixture did not publish an IOR within 10s" \
    /tmp/orbweaver-evolution.log
  return 1
}
release_lock() { rm -rf "$LOCK"; }
trap 'cleanup; release_lock' EXIT

# Waits for the fixture to actually publish an IOR.
#
# The wait must sleep. An earlier version spun without sleeping, which took
# microseconds and therefore did not wait at all; it only looked correct
# because `cargo run` had to compile first and accidentally covered the race.
# Once the build was warm the race surfaced as phantom GIOP timeouts.
# Prints why a fixture did not come up. Discarding its output made "did not
# publish an IOR" the only thing the harness could ever say, which is a
# measurement of the symptom and not of the cause — on a CI runner, where the
# fixture cannot be started by hand, that is the difference between a diagnosis
# and a guess.
fixture_died() {
  echo "  FAIL $1"
  if [ -s "$2" ]; then
    echo "       last output from the fixture:"
    tail -12 "$2" | sed 's/^/       | /'
  else
    echo "       the fixture wrote nothing at all"
  fi
}

start_server() {
  cleanup
  rm -f "$ROOT/spikes/echo.ior"
  ( cd "$ROOT/spikes" && exec python3 echo_server.py "$@" >/tmp/orbweaver-fixture.log 2>&1 & )
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/echo.ior" ] && { sleep 0.2; return 0; }  # settle after publish
    sleep 0.1
  done
  fixture_died "fixture did not publish an IOR within 10s" /tmp/orbweaver-fixture.log
  return 1
}

# Starts OUR server. Distinct from start_server, which launches the omniORB
# fixture; conflating the two silently pointed a check at the wrong process.
JH_CHECK=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
JCP_CHECK="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar:classes"

start_rust_server() {
  fkill spike-server
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
# -D warnings, because this configuration is only built here: a helper that
# became dead once encoding_rs was gone sat un-noticed behind an exit-status
# check that warnings cannot fail.
if RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop --lib --no-default-features --quiet \
     >/dev/null 2>&1; then
  echo "  ok   the attribution-free build still passes its tests, warning-free"
else
  echo "  FAIL the attribution-free build does not build cleanly or does not test"
  fail_total=$((fail_total+1))
fi

hr "ssliop feature — the D002 dependency promise"
ssl_fail=0
# A default build must carry no cryptography dependency at all.
deft=$(cargo tree -p orbweaver-giop 2>/dev/null)
if printf '%s' "$deft" | grep -qiE "rustls|aws-lc"; then
  echo "  FAIL the default build pulls a TLS/crypto crate; NOTICE and D002 are wrong"; ssl_fail=1
else
  echo "  ok   default cargo tree carries no rustls/aws-lc, as NOTICE states"
fi
# And the feature must actually deliver what D002 approved.
feat=$(cargo tree -p orbweaver-giop --features ssliop 2>/dev/null)
if printf '%s' "$feat" | grep -q "rustls" && printf '%s' "$feat" | grep -q "aws-lc-rs"; then
  echo "  ok   --features ssliop pulls rustls with the aws-lc-rs provider D002 names"
else
  echo "  FAIL --features ssliop does not resolve to rustls + aws-lc-rs"; ssl_fail=1
fi
# In-process TLS tests: certificate verification on, framing pass-through,
# clean refusal of a non-TLS peer. Peer interop (omniORB sslTP) is a future
# batch and is deliberately NOT claimed here.
if RUSTFLAGS="-D warnings" cargo test -p orbweaver-giop --features ssliop --quiet >/dev/null 2>&1; then
  echo "  ok   ssliop build tests green against the in-process rustls peer, warning-free"
else
  echo "  FAIL the ssliop build does not build cleanly or does not test"; ssl_fail=1
fi
[ "$ssl_fail" -eq 0 ] || fail_total=$((fail_total+1))

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

# ── Differential conformance ─────────────────────────────────────────────────
hr "differential conformance — every front end on every corpus file"
# Was two ad-hoc omniidl loops over golden/ and negative/. They are now one
# script, because CI runs a second oracle (tao_idl) and the interesting result
# there is where the *oracles* disagree with each other — a corpus file that is
# not portable, which agreeing with either one of them cannot reveal.
dout=$(bash spikes/differential.sh 2>&1); drc=$?
printf '%s\n' "$dout"
if [ "$drc" -ne 0 ]; then fail_total=$((fail_total+1)); fi
# An absent oracle is unmeasured, not passing, and the verdict has to say so.
if printf '%s' "$dout" | grep -q "SKIPPED"; then skipped=$((skipped+1)); fi

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
fkill spike-server

# ── Fragmentation ────────────────────────────────────────────────────────────
hr "GIOP fragmentation"
# This comment used to say neither available peer emits fragments. That was an
# assumption nobody had tested with a large enough argument: asked for a 1 MB
# sequence<octet>, omniORB 4.3.4 answers in two pieces, reproducibly, with no
# configuration — measured by spike-mux. JacORB 3.9 still does not. So the
# reassembler has now been fed a real peer's fragments, and the direction below
# (we fragment, they reassemble) is no longer the only independent evidence. The receiver used to be
# covered only by round-trip against our own emitter, which is one shape; it is
# now also driven by hand-built streams from §9.4.9 that a conformant peer may
# legally send and ours never does (`tests/fragment_reception.rs`, run by cargo
# test). That found two reception bugs no peer could have shown us: a stray
# leading Fragment was returned as a message, and a fragment at a different
# GIOP version was accepted as a continuation — in 1.1 the bytes read as a
# request id are body, so a match would have been a coincidence.
fkill spike-server
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
fkill spike-server

# ── Object model ─────────────────────────────────────────────────────────────
hr "object model — references, identity, LOCATION_FORWARD"
if start_rust_server; then
  out=$(python3 spikes/object_client.py spikes/server.ior 2>&1)
  if printf '%s' "$out" | grep -q "failures: 0"; then
    echo "  ok   _is_a answered from the inheritance graph, no network lookup"
    echo "  ok   an object reference survives as a value and is callable"
  else
    echo "  FAIL object model against omniORB"
    printf '%s' "$out" | grep FAIL | head -3 | sed 's/^/       /'
    fail_total=$((fail_total+1))
  fi
else
  fail_total=$((fail_total+1))
fi
cleanup

# LOCATION_FORWARD: Phase 1 could follow one and never send one. A peer must
# retry transparently, and the server logs the emission so a call that would
# have succeeded anyway cannot be mistaken for proof.
fwd_fail=0
for peer in omni jacorb; do
  fkill spike-server
  rm -f "$ROOT/spikes/server.ior"
  ( cd "$ROOT" && ORBWEAVER_FORWARD_PING=1 exec cargo run -q --bin spike-server -- \
      spikes/server.ior 127.0.0.1 0 >/tmp/orbweaver-fwd.log 2>&1 & )
  up=0
  for _ in $(seq 1 100); do
    [ -s "$ROOT/spikes/server.ior" ] && { sleep 0.3; up=1; break; }
    sleep 0.1
  done
  [ "$up" -eq 1 ] || { echo "  FAIL forwarding server did not start"; fwd_fail=1; break; }

  if [ "$peer" = omni ]; then
    got=$(python3 spikes/object_client.py spikes/server.ior 2>&1 | grep -c "get_self() is callable -> 42")
    label="omniORB"
  else
    if [ ! -d "$ROOT/spikes/jacorb/classes" ] || [ ! -x "$JH_CHECK/bin/java" ]; then
      echo "  SKIPPED  JacORB half — fixture absent"; skipped=$((skipped+1)); continue
    fi
    got=$(cd "$ROOT/spikes/jacorb" && "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Client ../server.ior 2>&1 | grep -c "ping() -> 42")
    label="JacORB"
  fi
  sleep 0.3
  emitted=$(grep -c "emitted LOCATION_FORWARD" /tmp/orbweaver-fwd.log 2>/dev/null || echo 0)
  if [ "$got" -ge 1 ] && [ "$emitted" -ge 1 ]; then
    echo "  ok   $label followed a LOCATION_FORWARD we emitted"
  else
    echo "  FAIL $label: call ok=$got, forwards emitted=$emitted"
    fwd_fail=1
  fi
done
fkill spike-server
[ "$fwd_fail" -eq 0 ] || fail_total=$((fail_total+1))

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
  fkill "classes Server"
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
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi

# ── Naming: resolve a target the way a deployment does ───────────────────────
hr "object-reference acquisition — corbaname: through a real naming service"
fkill omniNames
fkill register_name
sleep 0.5
rm -rf /tmp/orbweaver-names && mkdir -p /tmp/orbweaver-names
# Whether something is listening, without needing lsof. The probe used to be
# `lsof -nP -iTCP:2809`, which is absent on a stock CI runner — so the check
# could not tell "nothing is listening" from "I cannot look", and reported the
# first. bash's /dev/tcp needs no package.
port_open() { (exec 3<>/dev/tcp/127.0.0.1/"$1") >/dev/null 2>&1; }
if ! command -v omniNames >/dev/null 2>&1; then
  echo "  SKIPPED  omniNames is not installed — naming is unmeasured, not passing"
  skipped=$((skipped+1))
  names_up=-1
else
  ( omniNames -start 2809 -logdir /tmp/orbweaver-names >/tmp/orbweaver-names/out.log 2>&1 & )
  names_up=0
  for _ in $(seq 1 60); do
    port_open 2809 && { names_up=1; break; }
    sleep 0.2
  done
fi
if [ "$names_up" -eq 0 ]; then
  echo "  FAIL omniNames did not start on 2809"
  if [ -s /tmp/orbweaver-names/out.log ]; then
    tail -8 /tmp/orbweaver-names/out.log | sed 's/^/       | /'
  else
    echo "       it wrote nothing at all"
  fi
  fail_total=$((fail_total+1))
elif [ "$names_up" -eq -1 ]; then
  :
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
fkill register_name
fkill omniNames

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
  fkill spike-server

  # Our Rust client -> JacORB server.
  fkill "classes Server"
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
  fkill "classes Server"
  [ "$jfail" -eq 0 ] || fail_total=$((fail_total+1))
fi

# ── S4, the validation gate ──────────────────────────────────────────────────
hr "S4 validation gate — diagnostics a generator can act on"
# §5: everything upstream of S4 is allowed to be uncertain because S4 is not.
# §3.3: the self-repair loop is only as good as the messages it feeds on, so
# fix-hint coverage is measured here rather than assumed.
s4_fail=0
if ! cargo run -q --bin sidl-validate -- corpus/golden/*.idl \
     corpus/requirements/generated/*.idl spikes/*.idl >/tmp/orbweaver-s4.log 2>&1; then
  echo "  FAIL the gate rejected IDL both oracles accept"
  grep "error:" /tmp/orbweaver-s4.log | head -3 | sed 's/^/       /'
  s4_fail=1
else
  echo "  ok   accepts all $(ls corpus/golden/*.idl corpus/requirements/generated/*.idl spikes/*.idl | wc -l | tr -d ' ') valid files"
fi
s4_bad=""
for f in corpus/negative/*.idl; do
  cargo run -q --bin sidl-validate -- "$f" >/dev/null 2>&1 && s4_bad="$s4_bad $(basename "$f")"
done
if [ -z "$s4_bad" ]; then
  echo "  ok   rejects all $(ls corpus/negative/*.idl | wc -l | tr -d ' ') negatives"
else
  echo "  FAIL accepted:$s4_bad"; s4_fail=1
fi
# The measurement §3.3 asks for. Reported as a number, not as a pass: a fix
# hint that cannot be given honestly is better absent than invented.
s4_json=$(cargo run -q --bin sidl-validate -- --json corpus/negative/*.idl 2>/dev/null)
s4_cov=$(printf '%s' "$s4_json" | grep -o '"fix"' | wc -l | tr -d ' ')
s4_tot=$(ls corpus/negative/*.idl | wc -l | tr -d ' ')
echo "  ok   $s4_cov of $s4_tot rejections carry an actionable fix (a missing separator has no unambiguous one)"
[ "$s4_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Contract and property gate ───────────────────────────────────────────────
hr "contract-check — seeded round-trip property plus annotation contract advice"
# Two gates with deliberately different force. A byte-instability in the
# marshalling core is a defect and fails the run; an annotation finding is
# advice about meaning, which no deterministic checker can promote to a verdict
# without inventing a policy the project has not decided. S4 gates syntax and
# semantics; this gates what the annotations claim.
cc_out=$(cargo run -q -p orbweaver-test --bin contract-check -- corpus/golden/*.idl 2>&1)
cc_rc=$?
if [ "$cc_rc" -ne 0 ]; then
  printf '%s\n' "$cc_out" | grep -i "defect\|error" | head -3 | sed 's/^/       /'
  echo "  FAIL byte instability in the marshalling core"
  fail_total=$((fail_total+1))
else
  echo "  ok   $(printf '%s\n' "$cc_out" | tail -2 | head -1)"
fi
# Panic freedom. Rust rules out the memory-corruption half of "wire parsing is
# the classic memory-safety hazard" at compile time and rules out nothing about
# panics — a slice index or an unwrap reachable from a peer's bytes ends the
# process just as surely, and `unsafe_code = "forbid"` does not cover it.
# Reported with its reach, because a fuzz that bounces off the header check
# every time is green and worthless and the exit code cannot tell you which.
wf_out=$(cargo run -q --release -p orbweaver-test --bin wire-fuzz -- --cases 20000 2>&1)
if printf '%s' "$wf_out" | grep -q "wire-fuzz: PASS"; then
  echo "  ok   $(printf '%s' "$wf_out" | head -1 | sed 's/^wire-fuzz: //')"
  printf '%s' "$wf_out" | sed -n '2,3p' | sed 's/^  /  ok   /'
  # A target that reached nothing is green and worthless, and only a reader of
  # this line can turn the binary's own warning into a failure.
  if printf '%s' "$wf_out" | grep -q "WARNING:"; then
    printf '%s' "$wf_out" | grep "WARNING:" | sed 's/^ */       /'
    echo "  FAIL a fuzz target reached nothing; its green result measures nothing"
    fail_total=$((fail_total+1))
  fi
else
  printf '%s' "$wf_out" | grep "FAIL" | head -3 | sed 's/^/       /'
  echo "  FAIL a decoder panicked on bytes a peer can send"
  fail_total=$((fail_total+1))
fi

# ── Dynamic invocation: calling with nothing generated ───────────────────────
hr "dynamic invocation — calls built from IDL text alone"
# The whole AI path rests on this: invoke_operation gets a name and a bag of
# values at runtime and has only the registry to work from. Checked against
# peers we did not write, because a dynamic invoker that agrees only with our
# own decoder has not been tested.
dyn_fail=0
if start_server; then
  dv=$(cargo run -q --bin spike-dynamic -- spikes/echo.ior spikes/echo.idl \
       IDL:spike/Echo:1.0 2>&1)
  if printf '%s' "$dv" | grep -q "dynamic invocation: PASS"; then
    echo "  ok   omniORB answered 8 dynamically built calls, both byte orders"
    echo "  ok   wrong arguments are refused locally, before anything is sent"
    echo "  ok   a refused call leaves the connection usable"
  else
    echo "  FAIL a dynamically built call did not work against omniORB"
    printf '%s' "$dv" | grep "FAIL" | head -3 | sed 's/^/       /'
    dyn_fail=1
  fi
else
  dyn_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jdyn.log 2>&1 & )
  jd=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jd=1; break; }
    sleep 0.1
  done
  if [ "$jd" -eq 1 ]; then
    dv=$(cargo run -q --bin spike-dynamic -- spikes/jacorb.ior spikes/echo.idl \
         IDL:spike/Echo:1.0 2>&1)
    if printf '%s' "$dv" | grep -q "dynamic invocation: PASS"; then
      echo "  ok   JacORB answered them too — a second, independent decoder"
    else
      echo "  FAIL a dynamically built call did not work against JacORB"
      printf '%s' "$dv" | grep "FAIL" | head -3 | sed 's/^/       /'
      dyn_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; dyn_fail=1
  fi
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi
[ "$dyn_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MCP boundary ─────────────────────────────────────────────────────────
hr "MCP bridge — an agent session with no address in it"
# §4.7: an IOR is a bearer address, so an agent holding one is past the guard,
# past destructive approval and past the audit log. The check is not that the
# calls work — it is that the transcript the agent saw contains no host, port,
# object key or stringified IOR. A leak is a failure even when every call
# succeeded, because that is the shape it would ship in.
mcp_fail=0
if start_server; then
  mv=$(cargo run -q --bin spike-mcp -- spikes/echo.ior spikes/echo.idl \
       IDL:spike/Echo:1.0 2>&1)
  if printf '%s' "$mv" | grep -q "MCP bridge: PASS"; then
    echo "  ok   default-deny: an un-allowlisted catalog is invisible"
    echo "  ok   search -> describe -> invoke, entirely in JSON, nothing generated"
    echo "  ok   a returned object reference crosses as a handle and can be passed back"
    echo "  ok   destructive operations need approval; other sessions' handles are worthless"
    printf '%s' "$mv" | grep "JSON message(s) contain no host" | sed 's/^  ok /  ok  /'
  else
    echo "  FAIL the MCP boundary did not hold"
    printf '%s' "$mv" | grep "FAIL" | head -4 | sed 's/^/       /'
    mcp_fail=1
  fi
else
  mcp_fail=1
fi
cleanup
[ "$mcp_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MCP transport, as a client drives it ─────────────────────────────────
hr "MCP over stdio — a real client session"
# The bridge is exercised in-process elsewhere. This is the part that only
# exists once there is a process: handshake ordering, one JSON object per line,
# and the rule that stdout carries the protocol and nothing else.
stdio_fail=0
if start_server; then
  cargo build -q --bin orbweaver-mcp-server --bin spike-dump 2>/dev/null
  mout=$(python3 spikes/mcp_session.py spikes/echo.ior spikes/echo.idl 2>&1)
  if printf '%s' "$mout" | grep -q "mcp session: PASS"; then
    printf '%s\n' "$mout" | grep "^  ok" | head -11
  else
    echo "  FAIL the stdio transport did not behave"
    printf '%s' "$mout" | grep "FAIL" | head -4 | sed 's/^/       /'
    stdio_fail=1
  fi
else
  stdio_fail=1
fi
cleanup
[ "$stdio_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Search baseline: stream D ────────────────────────────────────────────────
hr "search baseline — frozen queries against the lexical index"
# §8's benchmark discipline: the query set is versioned and never edited to
# make a run pass. exact/negative/injection are gates; synonym is the measured
# headroom the embedding batch will be judged against, with no pass/fail here.
sb=$(cargo run -q -p orbweaver-mcp --bin search-bench -- \
     corpus/queries/search-v1.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
sb_rc=$?
if [ "$sb_rc" -eq 0 ] && printf '%s' "$sb" | grep -q "search-bench: PASS"; then
  printf '%s' "$sb" | grep "search-bench: PASS" | sed 's/^/  ok   /'
else
  echo "  FAIL the frozen search baseline did not hold"
  printf '%s' "$sb" | tail -4 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi
# v2 widens the index (attributes, nested ai_desc, compound descriptions). v1
# stays frozen above so the two numbers keep meaning different things.
sb2=$(cargo run -q -p orbweaver-mcp --bin search-bench -- \
      corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
if [ $? -eq 0 ] && printf '%s' "$sb2" | grep -q "search-bench: PASS"; then
  printf '%s' "$sb2" | grep "search-bench: PASS" | sed 's/^/  ok   v2 /'
else
  echo "  FAIL the widened search set did not hold"
  printf '%s' "$sb2" | tail -4 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi
# D003's arm: embeddings arrive through a process boundary or not at all. With
# no key the vector half is UNMEASURED — never green, and never faked with the
# offline stand-in, which is a plumbing check and cannot close a vocabulary gap.
if [ -n "${VOYAGE_API_KEY:-}" ]; then
  et=/tmp/orbweaver-texts.tsv; vf=/tmp/orbweaver-vectors.txt
  if cargo run -q -p orbweaver-mcp --bin search-bench -- --emit-texts "$et" \
       corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl >/dev/null 2>&1 \
     && vecs=$(cut -f2 "$et" | ./spikes/embed.sh 2>&1); then
    { echo "orbweaver-vectors 1"
      paste <(cut -f1 "$et") <(printf '%s\n' "$vecs" | sed 's/^\[//; s/\]$//')
    } > "$vf"
    sbv=$(cargo run -q -p orbweaver-mcp --bin search-bench -- --vectors "$vf" \
          corpus/queries/search-v2.tsv corpus/golden/*.idl spikes/echo.idl 2>&1)
    if [ $? -eq 0 ]; then
      printf '%s' "$sbv" | grep "search-bench: PASS" | sed 's/^/  ok   vector /'
    else
      echo "  FAIL vector search regressed a gate"
      printf '%s' "$sbv" | tail -4 | sed 's/^/       /'
      fail_total=$((fail_total+1))
    fi
  else
    echo "  FAIL embed.sh failed with a key present — that is a broken wrapper, not an absence"
    fail_total=$((fail_total+1))
  fi
else
  echo "  SKIPPED  VOYAGE_API_KEY absent — the synonym class is unmeasured, not passing"
  skipped=$((skipped+1))
fi

# ── Wire hardening: stream E ─────────────────────────────────────────────────
hr "wire hardening — LocateRequest send, both peers, all three versions"
# Carried forward since Phase 2: the server side has answered locates, but
# nothing here had ever SENT one. Both answers are measured, because a locate
# that can only produce "here" has not been tested against anything.
loc_fail=0
if start_server; then
  lv=$(cargo run -q --bin spike-locate -- spikes/echo.ior 2>&1)
  if printf '%s' "$lv" | grep -q "locate: PASS"; then
    echo "  ok   omniORB: OBJECT_HERE for the real key, UNKNOWN for a corrupted one, GIOP 1.0/1.1/1.2"
  else
    echo "  FAIL locate against omniORB"
    printf '%s' "$lv" | grep FAIL | head -3 | sed 's/^/       /'
    loc_fail=1
  fi
else
  loc_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jloc.log 2>&1 & )
  jl=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jl=1; break; }
    sleep 0.1
  done
  if [ "$jl" -eq 1 ]; then
    lv=$(cargo run -q --bin spike-locate -- spikes/jacorb.ior 2>&1)
    if printf '%s' "$lv" | grep -q "locate: PASS"; then
      echo "  ok   JacORB agrees on all six answers — a second, independent locate responder"
    else
      echo "  FAIL locate against JacORB"
      printf '%s' "$lv" | grep FAIL | head -3 | sed 's/^/       /'
      loc_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; loc_fail=1
  fi
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi
[ "$loc_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Wire hardening: stream E — multi-profile failover ────────────────────────
hr "wire hardening — multi-profile failover, dead first profile"
# Unit tests prove failover against listeners that accept but never speak
# GIOP. This closes the peer half: a synthetic IOR whose first profile is the
# real one with its port forced to 1 must still carry ping() -> 42, and an
# all-dead IOR must report how many endpoints were tried.
fo_fail=0
if start_server; then
  fv=$(cargo run -q --bin spike-failover -- spikes/echo.ior 2>&1)
  if printf '%s' "$fv" | grep -q "failover: PASS"; then
    echo "  ok   omniORB: a dead first profile does not cost the call; exhaustion counts endpoints"
  else
    echo "  FAIL failover against omniORB"
    printf '%s' "$fv" | grep FAIL | head -3 | sed 's/^/       /'
    fo_fail=1
  fi
else
  fo_fail=1   # an unmeasured check is a failure, never a pass
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jfo.log 2>&1 & )
  jf=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jf=1; break; }
    sleep 0.1
  done
  if [ "$jf" -eq 1 ]; then
    fv=$(cargo run -q --bin spike-failover -- spikes/jacorb.ior 2>&1)
    if printf '%s' "$fv" | grep -q "failover: PASS"; then
      echo "  ok   JacORB: same behaviour from the second, independent peer"
    else
      echo "  FAIL failover against JacORB"
      printf '%s' "$fv" | grep FAIL | head -3 | sed 's/^/       /'
      fo_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; fo_fail=1
  fi
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi
[ "$fo_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Wire hardening: stream E — CancelRequest is survivable ───────────────────
hr "wire hardening — CancelRequest against both peers"
# §9.4.4 is advisory. Measured: omniORB ignores a 1.2 cancel but CLOSES the
# connection on a 1.0/1.1 one — so the assertion is coherence, not tolerance:
# ignored, or refused with a clean client-side failure and a working fresh
# connection. Desynchronization is the only failure.
can_fail=0
if start_server; then
  cv=$(cargo run -q --bin spike-cancel -- spikes/echo.ior 2>&1)
  if printf '%s' "$cv" | grep -q "cancel: PASS"; then
    echo "  ok   omniORB: cancel ignored at 1.2, refused cleanly at 1.0/1.1, never desynchronized"
  else
    echo "  FAIL cancel against omniORB"
    printf '%s' "$cv" | grep FAIL | head -3 | sed 's/^/       /'
    can_fail=1
  fi
else
  can_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jcan.log 2>&1 & )
  jc=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; jc=1; break; }
    sleep 0.1
  done
  if [ "$jc" -eq 1 ]; then
    cv=$(cargo run -q --bin spike-cancel -- spikes/jacorb.ior 2>&1)
    if printf '%s' "$cv" | grep -q "cancel: PASS"; then
      echo "  ok   JacORB: coherent too — the second peer's cancel policy measured"
    else
      echo "  FAIL cancel against JacORB"
      printf '%s' "$cv" | grep FAIL | head -3 | sed 's/^/       /'
      can_fail=1
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; can_fail=1
  fi
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi
[ "$can_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── F6: the first-party CosNaming server ─────────────────────────────────────
hr "naming server — our client, then an independent ORB, against OUR server"
# Every naming claim so far ran our client against omniNames. This is the
# other direction: bind/resolve/unbind/nested contexts served by us, and the
# user-exception bytes confirmed by omniORB's client rather than only our own.
ns_fail=0
NS_IOR=/tmp/orbweaver-names.ior
ns=$(cargo run -q --bin spike-names -- "$NS_IOR" 2>&1)
if printf '%s' "$ns" | grep -q "naming-server: PASS"; then
  echo "  ok   our client against our server: bind/resolve/unbind/AlreadyBound/NotFound/nested"
else
  echo "  FAIL naming server self-consistency"
  printf '%s' "$ns" | grep FAIL | head -3 | sed 's/^/       /'
  ns_fail=1
fi
rm -f "$NS_IOR" /tmp/orbweaver-names-hold.log
( exec cargo run -q --bin spike-names -- "$NS_IOR" --hold >/tmp/orbweaver-names-hold.log 2>&1 & )
ns_up=0
for _ in $(seq 1 60); do
  grep -qs HOLDING /tmp/orbweaver-names-hold.log && { ns_up=1; break; }
  sleep 0.2
done
if [ "$ns_up" -eq 1 ]; then
  oracle=$(python3 -c "import sys; from omniORB import CORBA; import CosNaming; orb = CORBA.ORB_init(sys.argv); nc = orb.string_to_object(open('$NS_IOR').read().strip())._narrow(CosNaming.NamingContextExt); print(orb.object_to_string(nc.resolve_str('spike/Echo')))" 2>&1)
  case "$oracle" in
    IOR:*) echo "  ok   omniORB's client resolved spike/Echo against OUR naming server" ;;
    *) echo "  FAIL cross-ORB resolve: $oracle"; ns_fail=1 ;;
  esac
else
  echo "  FAIL the holding naming server never came up"; ns_fail=1
fi
fkill "spike-names"
[ "$ns_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── The MoE control plane, one turn on the wire ─────────────────────────────
hr "expert service — registry, policy and residency through GIOP"
# F1+F2+F3 joined: register and heartbeat over the wire, run the loading
# policy over the offers it produced, and drive the residency machine with the
# decisions. Measured because the interesting failures are between the parts —
# an offer store that lags the state machine returns an empty decision list
# under memory pressure and nothing fails.
ex=$(cargo run -q --bin spike-experts 2>&1)
if printf '%s' "$ex" | grep -q "expert-service: PASS"; then
  echo "  ok   register/heartbeat/oneway prefetch/guarded evict/policy, one control loop"
else
  echo "  FAIL expert service"
  printf '%s' "$ex" | grep -i "FAIL" | head -3 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── The guard's dry-run: a policy preview that costs nothing ────────────────
hr "dry-run — the exposure read before it is deployed"
# No --ior and no peer, which is the whole point: this answers before a
# deployment exists. Two properties, both cheap. The report is well-formed and
# carries the summary an operator reads; and every audit line it leaves says
# DRYRUN, so a question can never be counted as a call — a hypothetical in the
# promotion statistics would promote a path nobody ever used.
dr_audit=/tmp/orbweaver-dryrun.audit
dr=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 \
     --as harness --dry-run 2>"$dr_audit")
dr_rc=$?
allowed=$(printf '%s' "$dr" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["summary"]["allow"])' 2>/dev/null)
scoped=$(printf '%s' "$dr" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["summary"]["need_scope"])' 2>/dev/null)
# `grep -c` prints its count AND exits 1 when the count is zero, so a
# `|| echo 0` appends a second line and the comparison below sees "0\n0".
# Count with awk, which has one exit status and one answer.
stray=$(awk '!/^DRYRUN-/ {n++} END {print n+0}' "$dr_audit" 2>/dev/null)
if [ "$dr_rc" -eq 0 ] && [ "$allowed" = "10" ] && [ "$scoped" = "1" ] && [ "$stray" -eq 0 ]; then
  echo "  ok   11 operations previewed with no target dialled: 10 allow, 1 need_scope"
  echo "  ok   every audit line is a DRYRUN line — no question counted as a call"
else
  echo "  FAIL the dry-run preview did not hold (allow=$allowed need_scope=$scoped stray=$stray)"
  fail_total=$((fail_total+1))
fi

# ── Service coverage: every declared operation, over the wire ───────────────
hr "service coverage — what the five servants actually serve"
# Each COMPONENTS row says ✅ and each servant implements a subset, deliberately.
# The wire cannot distinguish a considered BAD_OPERATION from a forgotten one,
# so this counts the facts and docs/SERVICES-COVERAGE.md carries the reasons; a
# servant that stops dispatching an operation, or starts dispatching one the
# plan says is refused, moves a count here and nowhere else.
cov=$(./spikes/service_sweep.sh 2>&1)
if printf '%s' "$cov" | grep -q "service-sweep: PASS"; then
  printf '%s' "$cov" | grep '^TOTAL' | sed 's/^/  ok   /'
else
  echo "  FAIL service coverage sweep"
  printf '%s' "$cov" | grep -E 'FAIL|UNMEASURED|BLOCKED' | head -5 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── The audit ledger is bounded, and says where it was cut ──────────────────
hr "audit ledger — a survey over the ceiling must name what it dropped"
# Dropping the oldest silently is how an audit log stops being one: a dropped
# hour and a quiet hour read identically, exactly when somebody is reading the
# log to tell them apart. --dry-run needs no IOR, no socket and no handle, so
# this is deterministic and fixture-free; an absent marker is a FAILURE.
al_marker=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice \
  --dry-run --audit-capacity 3 2>&1 >/dev/null | grep -E '^ELIDED ' || true)
al_dropped=$(printf '%s' "$al_marker" | sed -n 's/.*dropped=\([0-9][0-9]*\).*/\1/p')
if [ -n "$al_dropped" ] && [ "$al_dropped" -ge 1 ]; then
  echo "  ok   a ceiling of 3 over 11 decisions elided $al_dropped line(s) and named it in the ledger"
else
  echo "  FAIL the ledger dropped lines without saying so"
  fail_total=$((fail_total+1))
fi

# ── The Python target: a second language is the only test of the mapping ────
# Anything the Rust emitter got right by accident shows up here as something
# Python cannot express, or expresses differently. The seam is a local process
# (D007, PROPOSED) so CPython gains no dependency and the wire stays in Rust.
hr "Python client target — generated Python against the omniORB fixture"
if start_server; then
  pyout=/tmp/orbweaver-pytarget; rm -rf "$pyout"; mkdir -p "$pyout"
  if cargo run -q --bin gen-python -- --out "$pyout" spikes/echo.idl >/dev/null 2>&1 \
     && cargo build -q --bin orbweaver-py-bridge 2>/dev/null; then
    pyrun=$(python3 crates/orbweaver-gen/python/echo_client.py "$pyout" \
            spikes/echo.idl spikes/echo.ior ./target/debug/orbweaver-py-bridge 2>&1)
    case "$pyrun" in
      *"python target: PASS"*)
        echo "  ok   $(printf '%s' "$pyrun" | grep -c '^  ok') generated call(s) completed \
over the wire, no Rust stub involved" ;;
      *) echo "  FAIL the Python client did not complete its calls"
         printf '%s' "$pyrun" | tail -12 | sed 's/^/       /'
         fail_total=$((fail_total+1)) ;;
    esac
  else
    echo "  FAIL gen-python or the bridge did not build"
    fail_total=$((fail_total+1))
  fi
  cleanup
else
  echo "  FAIL the omniORB fixture would not start — an unmeasured check is a failure"
  fail_total=$((fail_total+1))
fi

# Generated Python is imported, not string-compared: a target that only ever
# gets diffed is a target nobody has run.
pybatch=/tmp/orbweaver-pybatch; rm -rf "$pybatch"; mkdir -p "$pybatch"
golden=$(ls corpus/golden/*.idl | wc -l | tr -d ' ')
if cargo run -q --bin gen-python -- --out "$pybatch" corpus/golden/*.idl >/dev/null 2>&1; then
  imported=$(cd "$pybatch" && python3 -c '
import importlib, pathlib, sys
sys.path.insert(0, ".")
ok = 0
for d in sorted(p.name for p in pathlib.Path(".").iterdir() if p.is_dir()):
    importlib.import_module(d); ok += 1
print(ok)' 2>/dev/null)
  if [ "${imported:-0}" -ge "$golden" ]; then
    echo "  ok   $imported generated Python package(s) imported, one per golden contract"
  else
    echo "  FAIL only ${imported:-0} of $golden golden contracts produced an importable package"
    fail_total=$((fail_total+1))
  fi
else
  echo "  FAIL gen-python refused the golden corpus"
  fail_total=$((fail_total+1))
fi

# ── corpus/include: the first multi-file cases the corpus has ever had ──────
# Every other corpus file is self-contained, which is exactly why `#include`
# was skipped rather than resolved for six phases and nothing went red. The
# manifest drives the gate, so a case is added by adding a row.
hr "corpus/include — resolution, prefix scope across a file boundary, guards, cycles"
inc=$(cargo test -q -p orbweaver-idl --test include_corpus 2>&1)
if printf '%s' "$inc" | grep -q "^test result: ok"; then
  echo "  ok   $(printf '%s' "$inc" | grep -oE '[0-9]+ passed' | head -1) over \
$(awk 'NF && $1 !~ /^#/' corpus/include/cases.tsv | wc -l | tr -d ' ') manifest case(s)"
else
  echo "  FAIL corpus/include"
  printf '%s' "$inc" | grep -A3 panicked | head -8 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── The estate: thirteen legacy contracts through the whole path ────────────
# Consumer-shaped, not a gate — nothing under spikes/estate/ is any stage's
# input, which is what lets it measure the path instead of participating in
# it. Every corpus file is self-contained, so this is the only place a
# multi-file estate, four prefix styles and an unannotated contract are seen
# at once. Takes no lock of its own: private mktemp dir, fixture by PID.
hr "legacy estate — thirteen contracts, one pass, ingestion to agent call"
if [ -x spikes/estate/run.sh ]; then
  if est=$(./spikes/estate/run.sh --tsv 2>&1); then
    printf '%s\n' "$est" | sed 's/^/  /'
    echo "  ok   estate: every stage measured"
  else
    printf '%s\n' "$est" | tail -20 | sed 's/^/  /'
    echo "  FAIL estate: see docs/pipeline-runs/2026-08-14-estate.md"
    fail_total=$((fail_total+1))
  fi
else
  echo "  FAIL spikes/estate/run.sh missing — an unmeasured path is a failure"
  fail_total=$((fail_total+1))
fi

# ── D005 option B: a regeneration is diffed against what is registered ──────
hr "registered-contract diff — an undeclared breaking change is refused"
# The half option C cannot cover, and vice versa: the differ reads no
# annotations, so a scope change is compatible by §5.3 and invisible here,
# while a rename that keeps every scope is invisible to C. Neither subsumes
# the other, which is why both landed.
rd=$(cargo test -q -p orbweaver-forge --test registered_diff 2>&1)
if printf '%s' "$rd" | grep -q "^test result: ok"; then
  echo "  ok   $(printf '%s' "$rd" | grep -oE '[0-9]+ passed' | head -1) — refuses a breaking
       regeneration, silent on an additive one, and silent when nothing is registered"
else
  echo "  FAIL registered-contract diff"
  printf '%s' "$rd" | grep -A3 panicked | head -6 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── Scope drift is loud before a call (stream C, D005's class) ──────────────
hr "scope drift — a permission name no token can satisfy, reported as an outage"
# The failure D005 measured is silent by construction: an identity provider
# issuing the requirement's literal scope against a contract asking for another
# refuses every legitimate caller, and it reads as a permissions
# misconfiguration rather than a generation defect. So what is checked here is
# that the process refuses to be quiet about it — and, just as important, that
# a deployment which does not configure a mapping cannot tell the feature
# exists.
sd_fail=0
SD=/tmp/orbweaver-scope-drift
rm -rf "$SD" && mkdir -p "$SD"
cat > "$SD/parkinglot.idl" <<'IDL'
module parkinglot {
  interface ParkingControl {
    //@ ai_desc: Raises the entry barrier
    //@ ai_authz: parkinglot.barrier.open
    void open_barrier();
  };
};
IDL
sd_out=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
  --as alice --map-scope 'gate:operate=gate:operate' \
  --token-scope 'gate:operate' --dry-run 2>"$SD/err")
sd_code=$?
sd_err=$(cat "$SD/err" 2>/dev/null)
if [ "$sd_code" -eq 3 ] && printf '%s' "$sd_err" | grep -q "open_barrier"; then
  echo "  ok   a scope no issued token can satisfy exits 3 and names the operation that goes dark"
else
  echo "  FAIL a drifted scope was not reported as an outage (exit $sd_code)"
  printf '%s' "$sd_err" | head -3 | sed 's/^/       /'
  sd_fail=1
fi
if cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
     --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
     --as alice --map-scope 'gate:operate=parkinglot.barrier.open' \
     --token-scope 'gate:operate' --dry-run >/dev/null 2>&1; then
  echo "  ok   one line of translation repairs it, with the contract untouched"
else
  echo "  FAIL the mapping did not repair the drift"; sd_fail=1
fi
sd_plain=$(cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl "$SD/parkinglot.idl" --expose IDL:parkinglot/ParkingControl:1.0 \
  --as alice --dry-run 2>/dev/null)
case "$sd_plain" in
  *scope_map*) echo "  FAIL an unconfigured deployment can tell the feature exists"; sd_fail=1 ;;
  *) echo "  ok   with no mapping configured, the report is the document it always was" ;;
esac
rm -rf "$SD"
[ "$sd_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── R7: an IOR that is dialable from where the client actually is ───────────
hr "NAT rewriting — the address a container publishes is not the one it bound"
# assumption D already measured that a server publishes a routable-but-local
# address. Inside a container that address is the namespace's, and a client
# outside cannot dial it. The spike constructs both real failures — refused and
# timed out — because loopback alone cannot show this one.
nat=$(./spikes/nat_rewrite.sh 2>&1)
if printf '%s' "$nat" | grep -q "nat rewriting: PASS"; then
  echo "  ok   unrewritten IOR fails to dial, rewritten one completes; key, version and"
  echo "       an undecodable profile all survive untouched"
  if printf '%s' "$nat" | grep -q "unmeasured (skipped): [1-9]"; then
    echo "  SKIPPED  the container probe has never run here — no docker, and it is"
    echo "           counted rather than read as evidence"
    skipped=$((skipped+1))
  fi
else
  echo "  FAIL NAT rewriting"
  printf '%s' "$nat" | grep -i "FAIL" | head -3 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── The whole path, end to end ──────────────────────────────────────────────
hr "end-to-end — requirement → contract → both halves → guarded call"
# Every part is measured somewhere above; this is the only check that runs them
# as one path, which is the claim the project actually makes. S1–S3 are
# replayed from a recorded live run because a committed servant names one
# contract's identifiers — see the run record, and see what re-running the
# model on the same requirement produced.
if [ -x "$ROOT/spikes/end_to_end.sh" ]; then
  e2e=$("$ROOT/spikes/end_to_end.sh" 2>&1)
  case "$e2e" in
    *"end-to-end: PASS"*)
      echo "  ok   8 hops, each measured: $(printf '%s' "$e2e" | grep -c 'PASS ') checks"
      printf '%s' "$e2e" | grep -E '^  \| (hand-written, product|generated by)' \
        | sed 's/^  | /  ok   /' ;;
    *) echo "  FAIL the end-to-end path did not hold"
       printf '%s\n' "$e2e" | grep FAIL | head -4 | sed 's/^/       /'
       fail_total=$((fail_total+1)) ;;
  esac
else
  echo "  FAIL spikes/end_to_end.sh missing — an unmeasured path is a failure"
  fail_total=$((fail_total+1))
fi

# ── Repository ids agree with omniidl ───────────────────────────────────────
hr "repository ids — identity, checked against the compiler that owns it"
# `#pragma prefix` makes an id un-derivable by inspection: an id does not say
# how many leading segments are prefix. So the only honest check is to run both
# compilers over the same files and diff, and this group exists because we
# spent months deriving ids from the scope path alone while every legacy IDL
# file carries a prefix — correct locally, wrong against every real peer.
rid_fail=0
rid_work=$(mktemp -d)
cargo run -q --bin repository-ids -- corpus/pragma/*.idl 2>/dev/null \
  | cut -f1,3 | sort > "$rid_work/ours"
for f in corpus/pragma/*.idl; do
  base=$(basename "$f"); out="$rid_work/$base.d"; mkdir -p "$out"
  if ! log=$(omniidl -bpython -C"$out" "$f" 2>&1); then
    echo "  FAIL omniidl rejected $base: $(printf '%s' "$log" | head -1)"; rid_fail=1; continue
  fi
  grep -rhoE '"IDL:[^"]*"' "$out" 2>/dev/null | tr -d '"' | grep -v '^IDL:omg.org/' \
    | sort -u | sed "s|^|$base	|"
done | sort > "$rid_work/oracle"
if diff -u "$rid_work/oracle" "$rid_work/ours" > "$rid_work/diff" 2>&1; then
  echo "  ok   $(wc -l < "$rid_work/ours" | tr -d ' ') repository id(s) match omniidl, prefixes and all"
else
  echo "  FAIL our repository ids differ from omniidl:"
  head -12 "$rid_work/diff" | sed 's/^/       /'
  rid_fail=1
fi
rm -rf "$rid_work"
[ "$rid_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── D004 + console: the emitter into the reader ─────────────────────────────
hr "observability — real span records through the real console"
# The two halves were built in separate batches against the record table in
# the approved decision, never against each other. That is what fixing the
# shape in the decision buys, and it is worth nothing unless somebody runs one
# into the other, so the harness does — and it does it with no target dialled,
# because --dry-run asks the real chain and reaches no peer.
obs_fail=0
OBS_JSONL=/tmp/orbweaver-obs.jsonl
OBS_HTML=/tmp/orbweaver-obs.html
rm -f "$OBS_JSONL" "$OBS_HTML" "$OBS_JSONL.2"
cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
  --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice --session s-harness \
  --trace "$OBS_JSONL" --trace-ts 2026-08-14T09:00:00Z --dry-run >/dev/null 2>&1
if [ ! -s "$OBS_JSONL" ]; then
  echo "  FAIL no span records were emitted"; obs_fail=1
else
  # Replay: same calls, same bytes. The record carries no clock, and this is
  # what makes that claim checkable rather than merely documented.
  cargo run -q -p orbweaver-mcp --bin orbweaver-mcp-server -- \
    --idl spikes/echo.idl --expose IDL:spike/Echo:1.0 --as alice --session s-harness \
    --trace "$OBS_JSONL.2" --trace-ts 2026-08-14T09:00:00Z --dry-run >/dev/null 2>&1
  if cmp -s "$OBS_JSONL" "$OBS_JSONL.2"; then
    echo "  ok   $(wc -l < "$OBS_JSONL" | tr -d ' ') span records, and the trace replays byte-identically"
  else
    echo "  FAIL the trace did not replay byte-identically"; obs_fail=1
  fi
  obs=$(cargo run -q -p orbweaver-console --bin orbweaver-console -- \
        traces "$OBS_JSONL" 2>&1)
  case "$obs" in
    *"0 unclassified, 0 unreadable lines"*)
      echo "  ok   the console read every record the emitter wrote: $(printf '%s' "$obs" | sed -n 2p | sed 's/^ *//')" ;;
    *) echo "  FAIL the console could not read the emitter's records"
       printf '%s' "$obs" | head -3 | sed 's/^/       /'; obs_fail=1 ;;
  esac
  # The operator's page must not attack the operator: a repository id is chosen
  # by whoever we ingested it from.
  printf '%s\n' '{"ts":"2026-01-01T00:00:00Z","session":"s","caller":"x","target":"IDL:evil/<script>alert(1)</script>:1.0","operation":"go","decision":"dryrun-refuse","stage":"authz.exposure","path":"dynamic","outcome":"-"}' >> "$OBS_JSONL"
  cargo run -q -p orbweaver-console --bin orbweaver-console -- \
    traces "$OBS_JSONL" --html "$OBS_HTML" >/dev/null 2>&1
  page=$(cat "$OBS_HTML" 2>/dev/null)
  case "$page" in
    *"<script"*|*"<img"*) echo "  FAIL markup from a trace field rendered as markup"; obs_fail=1 ;;
    *"&lt;script&gt;"*)    echo "  ok   a hostile repository id renders inert, and is not dropped" ;;
    *) echo "  FAIL the payload was dropped rather than escaped"; obs_fail=1 ;;
  esac
fi
[ "$obs_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Stream E: multiplexing and pooling, against both peers ──────────────────
hr "multiplexing — several requests in flight, replies correlated by id"
# Out-of-order replies are the peer's to volunteer, so they are reported and
# never gated; what is gated is that the run completed. The self-test needs no
# fixture and says so — it scores no out-of-order claim, because our own server
# reads one request per connection.
mx_fail=0
mx=$(cargo run -q --bin spike-mux 2>&1)
if printf '%s' "$mx" | grep -q "mux: PASS"; then
  echo "  ok   self-test: pipelining, tombstones, and a refusal below GIOP 1.2"
else
  echo "  FAIL mux self-test"; printf '%s' "$mx" | grep -i fail | head -3 | sed 's/^/       /'
  mx_fail=1
fi
if start_server; then
  mxp=$(cargo run -q --bin spike-mux -- spikes/echo.ior 12 1.2 2>&1)
  if printf '%s' "$mxp" | grep -q "mux: PASS"; then
    echo "  ok   omniORB at 1.2: $(printf '%s' "$mxp" | grep -o 'out-of-order [0-9]*' | head -1 | sed 's/^/replies /')"
    printf '%s' "$mxp" | grep -E 'FRAGMENTS|UNMEASURED' | head -2 | sed 's/^/       /'
  else
    echo "  FAIL multiplexing against omniORB"; mx_fail=1
  fi
else
  mx_fail=1
fi
cleanup
[ "$mx_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Stream E batch 2: concurrent dispatch, run more than once ───────────────
hr "concurrent dispatch — five runs, because one green run is not evidence"
# Every test in these crates is deadline-bounded, so a regression is a failed
# run and never a hung harness. The count is the check: a concurrency change
# that passes once has not been measured.
cd_runs=${ORBWEAVER_CONCURRENCY_RUNS:-5}
cd_failures=0
for cd_run in $(seq 1 "$cd_runs"); do
  # No RUSTFLAGS here on purpose. Setting it changes cargo's fingerprint, so
  # every later group in this file rebuilds the whole graph from scratch — the
  # first version of this group did exactly that and pushed the event-channel
  # fixture past its 12s readiness deadline, which looked like a wire failure
  # and was a build-cache one. The lint gate is CI's job; this group's job is
  # the repeat count.
  cd_out=$(cargo test -q -p orbweaver-giop -p orbweaver-registry -p orbweaver-object 2>&1)
  if printf '%s' "$cd_out" | grep -q "^test result: FAILED"; then
    echo "  FAIL run $cd_run of $cd_runs"
    printf '%s' "$cd_out" | grep -A3 "^failures:" | head -6 | sed 's/^/       /'
    cd_failures=$((cd_failures+1))
  fi
done
if [ "$cd_failures" -eq 0 ]; then
  echo "  ok   $cd_runs runs of the three servant crates, all green"
  echo "  ok   the negative control is a test: serialized dispatch must NOT overlap,"
  echo "       and it fails on its deadline rather than hanging when it does"
else
  fail_total=$((fail_total+1))
fi

# ── Stream E: concurrent connections ─────────────────────────────────────────
hr "concurrency — many clients at once, and a cap that says no out loud"
# Every service above documented "one client at a time" as a limit its harness
# group had to respect. The overlap is asserted against the server's own
# counter rather than against timing, because a timing-based overlap check
# passes on a fast serial server and is therefore not a check.
cc_fail=0
cy=$(cargo run -q --bin spike-concurrent 2>&1)
if printf '%s' "$cy" | grep -q "concurrency: PASS"; then
  echo "  ok   $(printf '%s' "$cy" | grep 'measured overlap' | sed 's/^ *//')"
  echo "  ok   $(printf '%s' "$cy" | grep 'cap behaviour' | sed 's/^ *//') — over the cap gets §9.4.7's goodbye"
else
  echo "  FAIL concurrent serving"
  printf '%s' "$cy" | grep -i "FAIL" | head -3 | sed 's/^/       /'
  cc_fail=1
fi
[ "$cc_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── F5: tenancy, as an authorization property of the object key ─────────────
hr "tenant service — LifeCycle and Property with the tenant in every key"
# The isolation claims are the substance here, so the spike checks refusals as
# hard as it checks successes: a foreign reference is refused BEFORE the
# existence check, so a refusal cannot be used as an existence oracle. The one
# crossing that is served — base(), the shared model — is counted and audited
# rather than hidden, because the manifest's whole shape is "shared base by
# reference" and a servant that pretended otherwise would be lying about the
# design rather than enforcing it.
tn=$(cargo run -q --bin spike-tenants 2>&1)
if printf '%s' "$tn" | grep -q "tenant-service: PASS"; then
  echo "  ok   two tenants, $(printf '%s' "$tn" | grep -c '  ok    ') checks: minting, refusals, retire, policy, per-tenant audit"
else
  echo "  FAIL tenant service"
  printf '%s' "$tn" | grep -i "FAIL" | head -3 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── IFR facade: the registry served as CORBA::Repository ────────────────────
hr "interface repository — our registry read by omniORB's own IR client"
# The claim worth measuring is not that our client agrees with our server; it
# is that a client written against the OMG IR IDL, which we did not write and
# cannot influence, decodes our FullInterfaceDescription and reads the
# enumerators by name. Ordinals that are merely self-consistent would pass a
# self-test and fail here.
ifr_fail=0
IFR_IOR=/tmp/orbweaver-ifr.ior
ifr=$(cargo run -q --bin spike-ifr -- "$IFR_IOR" 2>&1)
if printf '%s' "$ifr" | grep -q "ifr-facade: PASS"; then
  echo "  ok   our client against our facade: lookup_id, describe_interface, is_a, refusals"
else
  echo "  FAIL IFR facade self-consistency"
  printf '%s' "$ifr" | grep FAIL | head -3 | sed 's/^/       /'
  ifr_fail=1
fi
rm -f "$IFR_IOR" /tmp/orbweaver-ifr-hold.log
cargo run -q --bin spike-ifr -- "$IFR_IOR" --hold >/tmp/orbweaver-ifr-hold.log 2>&1 &
IFR_PID=$!
ifr_up=0
for _ in $(seq 1 60); do
  grep -qs READY /tmp/orbweaver-ifr-hold.log && { ifr_up=1; break; }
  sleep 0.2
done
if [ "$ifr_up" -eq 1 ]; then
  ifr_out=$(python3 -c "import sys, CORBA, omniORB.ir_idl
orb = CORBA.ORB_init(sys.argv)
r = orb.string_to_object(open('$IFR_IOR').read().strip())._narrow(CORBA.Repository)
d = r.lookup_id('IDL:gc10/Both:1.0')._narrow(CORBA.InterfaceDef).describe_interface()
print(d.name, [o.name for o in d.operations], [a.name for a in d.attributes])
try:
    r.create_module('IDL:x:1.0', 'x', '1.0')
    print('WRITE ACCEPTED')
except CORBA.NO_PERMISSION:
    print('refused')" 2>&1)
  case "$ifr_out" in
    "Both ['touch', 'value'] ['id', 'name']"*refused*)
      echo "  ok   omniORB's IR client decoded our FullInterfaceDescription and was refused a write" ;;
    *ImportError*|*ModuleNotFoundError*)
      echo "  SKIPPED  omniORBpy IR stubs absent — the cross-ORB half is unmeasured, not passing"
      skipped=$((skipped+1)) ;;
    *) echo "  FAIL cross-ORB IR client: $(printf '%s' "$ifr_out" | tr '\n' ' ')"; ifr_fail=1 ;;
  esac
else
  echo "  FAIL the holding IFR facade never came up"; ifr_fail=1
fi
kill "$IFR_PID" >/dev/null 2>&1 || true
wait "$IFR_PID" 2>/dev/null || true
[ "$ifr_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Remote IFR ingestion: describing and calling with no IDL file ───────────
hr "remote IFR ingestion — a contract taken off the wire"
# Self-consistency first: our client against our facade proves the walk, the
# refusals and the TypeCode-driven call, and proves nothing about the
# specification. The JacORB leg is what makes it a claim — and it earned its
# place immediately, since JacORB's base_interfaces are Java class names and
# its version field is ":1.0", both of which our client refuses to guess from.
ing=$(cargo run -q --bin spike-ingest 2>&1)
if printf '%s' "$ing" | grep -q "ingest: PASS"; then
  echo "  ok   self-consistency: the walk, the refusals, and a call built from"
  echo "       ingested metadata with no .idl file opened"
else
  echo "  FAIL ingestion self-consistency"
  printf '%s' "$ing" | grep -i FAIL | head -3 | sed 's/^/       /'
  fail_total=$((fail_total+1))
fi

# ── F7: the event channel, both directions ──────────────────────────────────
hr "event channel — our supplier and consumer, then omniORB's consumer"
# The push model served by us. Two things are measured that a self-test alone
# cannot establish: that an ORB we did not write can narrow and attach to the
# channel, and that a consumer which dies mid-stream is disconnected with its
# drops counted. A channel that loses events quietly is worse than one that
# refuses them.
ev_fail=0
EV_IOR=/tmp/orbweaver-events.ior
ev=$(cargo run -q --bin spike-events -- "$EV_IOR" 2>&1)
if printf '%s' "$ev" | grep -q "event-channel: PASS"; then
  echo "  ok   our client against our channel: connect both sides, 20 in order, dead consumer disconnected"
  echo "  ok   $(printf '%s' "$ev" | grep 'drop report' | sed 's/^ *//')"
else
  echo "  FAIL event channel self-consistency"
  printf '%s' "$ev" | grep FAIL | head -3 | sed 's/^/       /'
  ev_fail=1
fi
rm -f "$EV_IOR" /tmp/orbweaver-events-hold.log
cargo run -q --bin spike-events -- "$EV_IOR" --hold >/tmp/orbweaver-events-hold.log 2>&1 &
EV_PID=$!
ev_up=0
for _ in $(seq 1 60); do
  grep -qs HOLDING /tmp/orbweaver-events-hold.log && { ev_up=1; break; }
  sleep 0.2
done
if [ "$ev_up" -eq 1 ]; then
  ev_out=$(python3 spikes/event_consumer.py "$EV_IOR" 2>&1)
  case "$ev_out" in
    *PASS*) echo "  ok   omniORB's PushConsumer received events from OUR channel" ;;
    *ModuleNotFoundError*|*ImportError*)
      echo "  SKIPPED  omniORBpy CosEventComm stubs absent — the cross-ORB half is unmeasured, not passing"
      skipped=$((skipped+1)) ;;
    *) echo "  FAIL cross-ORB consumer: $(printf '%s' "$ev_out" | tail -2 | tr '\n' ' ')"; ev_fail=1 ;;
  esac
else
  echo "  FAIL the holding event channel never came up"; ev_fail=1
fi
kill "$EV_PID" >/dev/null 2>&1 || true
wait "$EV_PID" 2>/dev/null || true
[ "$ev_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Identity: what the peers actually advertise ──────────────────────────────
hr "identity propagation — what a real target says about security"
# §4.8 predicts that many legacy targets have no authentication at all, and that
# where a target cannot enforce a caller identity the bridge is the only
# enforcement point. That is a claim about real deployments, so it is measured
# on real IORs rather than assumed — and the answer belongs in the catalogue.
id_fail=0
if start_server; then
  csi=$(cargo run -q --bin spike-dump -- spikes/echo.ior 2>/dev/null | grep '^csiv2')
  if printf '%s' "$csi" | grep -q "advertises no mechanism list"; then
    echo "  ok   omniORB 4.3.4 advertises no CSIv2: the bridge is the only enforcement point"
  else
    echo "  note omniORB advertises: $csi"
  fi
  ssl=$(cargo run -q --bin spike-dump -- spikes/echo.ior 2>/dev/null | grep '^ssliop')
  if printf '%s' "$ssl" | grep -q "no TAG_SSL_SEC_TRANS"; then
    echo "  ok   and no TAG_SSL_SEC_TRANS either — TLS work (D002) starts from a measured baseline"
  else
    echo "  note omniORB ssliop: $ssl"
  fi
else
  id_fail=1
fi
cleanup
if [ -d "$ROOT/spikes/jacorb/classes" ] && [ -x "$JH_CHECK/bin/java" ]; then
  fkill "classes Server"
  rm -f "$ROOT/spikes/jacorb.ior"
  ( cd "$ROOT/spikes/jacorb" && exec "$JH_CHECK/bin/java" -cp "$JCP_CHECK" Server ../jacorb.ior \
      >/tmp/orbweaver-jcsi.log 2>&1 & )
  ji=0
  for _ in $(seq 1 150); do
    [ -s "$ROOT/spikes/jacorb.ior" ] && { sleep 0.5; ji=1; break; }
    sleep 0.1
  done
  if [ "$ji" -eq 1 ]; then
    csi=$(cargo run -q --bin spike-dump -- spikes/jacorb.ior 2>/dev/null | grep '^csiv2')
    if printf '%s' "$csi" | grep -q "advertises no mechanism list"; then
      echo "  ok   JacORB 3.9 advertises none either — two peers, same answer"
    else
      echo "  note JacORB advertises: $csi"
    fi
  else
    echo "  FAIL JacORB server did not publish an IOR"; id_fail=1
  fi
  fkill "classes Server"
else
  echo "  SKIPPED  JacORB half — fixture absent"
  skipped=$((skipped+1))
fi
echo "  note CSIv2 encoding is unit-tested in both byte orders; no peer here enforces it,"
echo "       so interop remains a per-peer claim and is unmeasured (docs/PLAN.md §4.8)"
[ "$id_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Static generation: stream B ──────────────────────────────────────────────
hr "static generation — stubs from the registry, oracle: static equals dynamic"
# The dynamic path is the reference implementation — it is the one verified
# against two independent ORBs — so a generated stub is correct exactly when
# its bytes equal the dynamic bytes for the same values (§8). The generated
# crate is deliberately OUTSIDE the workspace: compiling it proves the stubs
# stand on the published crate surface alone.
gen_fail=0
GEN_OUT=$(mktemp -d)/genout
if cargo run -q --bin gen-corpus -- --out "$GEN_OUT" --workspace "$ROOT" \
     corpus/golden/*.idl corpus/services/*.idl spikes/echo.idl \
     >/tmp/orbweaver-gen.log 2>&1; then
  n_items=$(grep -o 'generated [0-9]* item' /tmp/orbweaver-gen.log | grep -o '[0-9]*')
  echo "  ok   $n_items item(s) generated from the golden corpus plus the fixture"
  grep '^skipped' /tmp/orbweaver-gen.log | sed 's/^/  note /'
  if (cd "$GEN_OUT" && CARGO_TARGET_DIR="$ROOT/target" cargo build -q 2>/tmp/orbweaver-genc.log); then
    echo "  ok   every generated stub compiles outside the workspace"
    # A plain build proves neither of the declarations the emitted modules
    # carry: forbid(unsafe_code) and deny(missing_docs) only bite with
    # -D warnings, and generated code held to a lower standard than
    # hand-written code is generated code nobody will trust.
    if (cd "$GEN_OUT" && CARGO_TARGET_DIR="$ROOT/target" \
          RUSTFLAGS="-D warnings" cargo build -q 2>/tmp/orbweaver-gend.log); then
      echo "  ok   and under -D warnings: no unsafe, no undocumented item"
    else
      echo "  FAIL generated code does not survive its own lint declarations"
      head -5 /tmp/orbweaver-gend.log | sed 's/^/       /'
      gen_fail=1
    fi
    # The serving direction. Everything above measures a generated *client*
    # against a stock ORB; this measures a stock ORB's client against a
    # generated *skeleton*, which is the half nothing had ever checked.
    if python3 -c 'import omniORB' >/dev/null 2>&1; then
      skel=$(cargo test -q -p orbweaver-gen --test skeleton_wire -- --nocapture \
             omniorb_python_drives_the_generated_skeleton 2>&1)
      if printf '%s' "$skel" | grep -q "^OK$"; then
        echo "  ok   omniORB's python client drove a GENERATED skeleton: narrow, attributes,"
        echo "       a oneway then a twoway on one connection, both user exceptions by class"
      else
        echo "  FAIL omniORB's python client could not drive the generated skeleton"
        printf '%s' "$skel" | tail -5 | sed 's/^/       /'
        gen_fail=1
      fi
    else
      echo "  SKIPPED  omniORBpy absent — the serving direction is unmeasured, not passing"
      skipped=$((skipped+1))
    fi
    # A generated servant's system exceptions, read by class by an ORB we did
    # not write. This is where the transposed completion status was caught:
    # every local comparison used the same enum on both sides and agreed with
    # itself, so only a foreign reader could disagree.
    if python3 -c 'import omniORB' >/dev/null 2>&1; then
      flt=$(cargo test -q -p orbweaver-gen --test servant_faults -- --nocapture \
            omniorb_python 2>&1)
      if printf '%s' "$flt" | grep -q "CORBA.NO_PERMISSION" \
         && printf '%s' "$flt" | grep -q "COMPLETED_NO"; then
        echo "  ok   omniORB caught a servant's system exceptions by class, and read"
        echo "       did_not_run() as COMPLETED_NO — §4.11.4's ordinal, retry-safe"
      else
        echo "  FAIL omniORB did not see the servant's system exceptions as sent"
        printf '%s\n' "$flt" | tail -5 | sed 's/^/       /'
        gen_fail=1
      fi
    else
      echo "  SKIPPED  omniORBpy absent — the servant-fault claims are unmeasured"
      skipped=$((skipped+1))
    fi
    # §8 in the reading that catches a dropped bound: the two paths must refuse
    # alike. Byte equality only ever samples values both paths accepted, so a
    # bound the generator dropped was invisible to the oracle above — which is
    # how it survived until D006 measured it while arguing about something else.
    bo=$(cargo test -q -p orbweaver-gen --test bounds_oracle 2>&1)
    if printf '%s' "$bo" | grep -q "^test result: ok"; then
      n_bo=$(printf '%s' "$bo" | grep -o '^test result: ok. [0-9]*' | grep -o '[0-9]*$')
      echo "  ok   static and dynamic refuse alike: $n_bo bound case(s), both byte orders,"
      echo "       encode and decode, stub and skeleton, argument and reply direction"
    else
      echo "  FAIL a declared bound is enforced by one path and not the other"
      printf '%s\n' "$bo" | grep -A3 "panicked" | head -6 | sed 's/^/       /'
      gen_fail=1
    fi
    # §8's rule in the direction nothing checked: a skeleton's reply bytes
    # against the dynamic path's. No fixture — ours on one end, the reference
    # implementation on the other.
    ora=$(cargo test -q -p orbweaver-gen --test skeleton_oracle -- --nocapture 2>&1)
    if printf '%s' "$ora" | grep -q "FAILED"; then
      echo "  FAIL a generated skeleton's replies are not the dynamic path's bytes"
      printf '%s\n' "$ora" | grep -A4 "disagree" | head -8 | sed 's/^/       /'
      gen_fail=1
    else
      n_cmp=$(printf '%s' "$ora" | grep -o '[0-9]* comparison' | grep -o '[0-9]*' \
              | awk '{s+=$1} END {print s+0}')
      echo "  ok   server-side static equals dynamic: $n_cmp reply comparison(s), both"
      echo "       byte orders, three GIOP versions, two reply origins"
    fi
    if start_server; then
      so=$("$ROOT/target/debug/static-oracle" spikes/echo.ior spikes/echo.idl 2>&1)
      if printf '%s' "$so" | grep -q "static generation: PASS"; then
        echo "  ok   static bytes equal dynamic bytes: Ragged, wstring, any, sequence, both orders"
        echo "  ok   the generated stub calls omniORB: 10/10 cases, both byte orders"
        echo "  ok   I1: the same stub through the guard — exposure, ai_authz scope and audit bind it"
        echo "  ok   I1: a refused call never reaches the wire; the audit holds nothing dialable"
        printf '%s' "$so" | grep "I4:" | sed 's/^  ok   /  ok   /' | head -3
      else
        echo "  FAIL static did not equal dynamic"
        printf '%s' "$so" | grep "FAIL" | head -3 | sed 's/^/       /'
        gen_fail=1
      fi
    else
      gen_fail=1
    fi
    cleanup
  else
    echo "  FAIL generated code does not compile"
    head -5 /tmp/orbweaver-genc.log | sed 's/^/       /'
    gen_fail=1
  fi
else
  echo "  FAIL generation failed"; head -3 /tmp/orbweaver-gen.log | sed 's/^/       /'
  gen_fail=1
fi
rm -rf "$(dirname "$GEN_OUT")"
[ "$gen_fail" -eq 0 ] || fail_total=$((fail_total+1))

# ── Contract evolution: is the §5.3 rule table true? ─────────────────────────
hr "contract evolution — §5.3 verdicts against a peer that predates the change"
# The differ's verdicts are predictions about deployed peers. Asserting them
# only against our own tests would prove that two pieces of our code agree, so
# the predicted consequence is produced on the wire by omniORB instead.
ev_fail=0
if start_evolution_server; then
  out=$(cargo run -q --bin spike-evolution -- \
        spikes/evolution_v1.idl spikes/evolution_v2.idl spikes/evolution_v1b.idl \
        spikes/evolution.ior 2>&1)
  if printf '%s' "$out" | grep -q "contract evolution: PASS"; then
    echo "  ok   the swapped struct members are flagged BREAKING before release"
    echo "  ok   omniORB answered the swapped call with the WRONG member, no exception"
    echo "  ok   an added operation on an un-updated server gives BAD_OPERATION"
  else
    echo "  FAIL a §5.3 verdict did not match what the wire did"
    printf '%s' "$out" | grep "  FAIL" | head -3 | sed 's/^/       /'
    ev_fail=1
  fi
else
  ev_fail=1
fi
# The other half of "server-first": the additive release must serve both.
if start_evolution_server --updated; then
  out=$(cargo run -q --bin spike-evolution -- --updated spikes/evolution.ior 2>&1)
  if printf '%s' "$out" | grep -q "contract evolution: PASS"; then
    echo "  ok   after the additive release, old and new clients are both served"
  else
    echo "  FAIL the additive release did not behave as 'compatible' predicts"
    printf '%s' "$out" | grep "  FAIL" | head -2 | sed 's/^/       /'
    ev_fail=1
  fi
else
  ev_fail=1
fi
cleanup
# The gate is the deliverable, not the report: check that it actually refuses.
if cargo run -q --bin idl-diff -- spikes/evolution_v1.idl spikes/evolution_v2.idl \
     >/dev/null 2>&1; then
  echo "  FAIL idl-diff accepted a change that corrupts data on the wire"
  ev_fail=1
else
  echo "  ok   idl-diff refuses the breaking revision (exit 1)"
fi
if cargo run -q --bin idl-diff -- spikes/evolution_v1.idl spikes/evolution_v1b.idl \
     >/dev/null 2>&1; then
  echo "  ok   idl-diff accepts the additive-only revision"
else
  echo "  FAIL idl-diff refuses a revision that breaks nothing"
  ev_fail=1
fi
[ "$ev_fail" -eq 0 ] || fail_total=$((fail_total+1))

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
