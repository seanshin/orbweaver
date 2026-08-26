#!/usr/bin/env bash
# E3's peer half: omniORB's client finds our event channel by NAME.
#
# `channel_found_by_name.rs` measures the Location claim with our client at both
# ends. A convention both ends apply cannot be refuted by a round trip, so this
# runs the same claim with a client we did not write: omniORB resolves the name
# out of our naming server, narrows to CosEventChannelAdmin::EventChannel and
# receives an event over a reference whose address it was never told.
#
# Not wired into run_checks.sh: that file is held by another batch as this
# lands. Wiring it in is one `hr` group and is named as undone in the report.
#
# Exit code is the verdict.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-by-name.XXXXXX")"
NS_IOR="$RUN_DIR/channel-names.ior"
LOG="$RUN_DIR/fixture.log"
PID=""

cleanup() {
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
    rm -rf "$RUN_DIR"
}
trap cleanup EXIT

fail() { echo "event-by-name: FAIL — $1"; exit 1; }

# The stubs the peer needs. An unmeasured check is a failure, never a pass, so
# a missing stub is SKIPPED with its reason and a non-zero code, not an ok.
if ! python3 -c "import CosNaming, CosEventChannelAdmin, CosEventComm, CosEventComm__POA" 2>/dev/null; then
    echo "event-by-name: SKIPPED — omniORB CosNaming/CosEvent stubs not importable"
    echo "  fixture: spikes/event_by_name_client.py (brew install omniorb)"
    exit 2
fi

echo "building"
cargo build -q -p orbweaver-giop --bin spike-channel-by-name

echo "starting the fixture"
# The built binary and not `cargo run`: cargo forks, and the PID we captured
# would be cargo's rather than the fixture's.
#
# If this ever hangs with a zero-byte log and no IOR, sample the pid before
# blaming the fixture. A freshly linked binary backgrounded immediately after
# its own build can sit in `_dyld_start` — before `main`, so no Rust code of
# ours has run and no timeout of ours can end it. It cost a diagnosis here:
# the symptom is identical to a wedged server, and the wrong reading of it was
# "the macOS accept race", which is a real hazard this file also guards against
# and was not what was happening. `sample <pid>` says which in one run. A
# foreground run of the binary first clears it.
# *갓 링크된 바이너리는 `main` 이전, dyld에서 멈출 수 있다. 서번트를 탓하기 전에
# 샘플을 뜬다.*
./target/debug/spike-channel-by-name "$NS_IOR" --hold >"$LOG" 2>&1 &
PID=$!

# A sleeping, deadline-bounded wait. A `for` loop with no sleep finishes in
# microseconds and does not wait at all — the assumption A failure.
deadline=$(( $(date +%s) + 30 ))
while [ ! -f "$NS_IOR" ]; do
    if ! kill -0 "$PID" 2>/dev/null; then
        cat "$LOG"
        fail "the fixture exited before publishing"
    fi
    [ "$(date +%s)" -ge "$deadline" ] && { cat "$LOG"; fail "the fixture never wrote its IOR"; }
    sleep 0.2
done

# Capture, then match with a herestring. Never `printf … | grep -q`: grep -q
# exits on first match and SIGPIPEs the producer, and under pipefail an `if`
# reads that as "no match".
fixture_out="$(cat "$LOG")"
if ! grep -q "READY" <<<"$fixture_out"; then
    # READY may not have been flushed yet even though the IOR exists.
    sleep 1
    fixture_out="$(cat "$LOG")"
    grep -q "READY" <<<"$fixture_out" || { echo "$fixture_out"; fail "the fixture never said READY"; }
fi
echo "$fixture_out" | sed 's/^/  /'

echo "running omniORB's client"
set +e
peer_out="$(python3 spikes/event_by_name_client.py "$NS_IOR" alerts 2>&1)"
peer_status=$?
set -e
echo "$peer_out" | sed 's/^/  /'

# Read the producer's own exit status first: a client that could not run at all
# is an unmeasured check, which is a failure and never a pass.
[ "$peer_status" -eq 0 ] || fail "omniORB's client exited $peer_status"
grep -q "^PASS$" <<<"$peer_out" || fail "omniORB's client did not print PASS"

# The channel's address was never written down, decided by the fixture because
# only the fixture can decode an IOR.
#
# This check used to live here as `grep -q "$ch_port" "$NS_IOR"` and was the
# green-while-measuring-nothing class exactly: the file is `IOR:` followed by
# hex and the port is two CDR bytes inside it, so a search for the decimal port
# could never match and the `ok` line was printed unconditionally. Its negative
# control — pointing it at the naming port, which *is* in that file — did not go
# red, which is how it was found. The check now round-trips the written IOR
# through `string_to_object` and compares decoded ports.
grep -q "the peer's only file advertises" <<<"$fixture_out" \
    || fail "the fixture did not report on what its published file advertises"

echo
echo "event-by-name: PASS"
