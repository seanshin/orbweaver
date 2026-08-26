#!/usr/bin/env bash
# Starts every served servant, sweeps every operation its IDL declares, stops
# them again.
#
# HARNESS FIXTURE for `docs/SERVICES-COVERAGE.md`. Run from the repository
# root:
#
#   ./spikes/service_sweep.sh            # human-readable
#   ./spikes/service_sweep.sh --raw      # the TSV the report is built from
#
# Harness rules this script is written to, each of which cost a phantom
# failure once already:
#   - every wait loop sleeps and is bounded by a deadline; none spins
#   - producers are captured to a file and matched afterwards, never piped
#     into `grep -q`, which SIGPIPEs upstream on its first match
#   - every fixture is killed by a PID captured at launch, never by name
#   - a fixture that will not start increments the failure count; an
#     unmeasured check is a failure, never a pass
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-sweep.XXXXXX")"
PIDS=()
FAILURES=0

cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
    done
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && wait "$pid" 2>/dev/null
    done
    rm -rf "$RUN_DIR"
}
trap cleanup EXIT INT TERM

# Waits for $1 to appear, sleeping, bounded by $2 seconds. Returns non-zero on
# the deadline rather than looping forever.
wait_for_file() {
    local path="$1" deadline=$(($(date +%s) + $2))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        [ -s "$path" ] && return 0
        sleep 0.2
    done
    return 1
}

# start <name> <ior-file> <command...>
start() {
    local name="$1" ior="$2"
    shift 2
    local log="$RUN_DIR/$name.log"
    "$@" >"$log" 2>&1 &
    local pid=$!
    PIDS+=("$pid")
    if wait_for_file "$RUN_DIR/$ior" 60; then
        # The IOR file is written before the serve thread starts in some
        # spikes, so give the listener a moment the same way: a sleeping,
        # bounded wait, not an assumption.
        sleep 0.3
        echo "  up    $name"
        return 0
    fi
    echo "  FAIL  $name did not publish $ior — log follows"
    sed 's/^/        /' "$log"
    FAILURES=$((FAILURES + 1))
    return 1
}

echo "building"
cargo build -q --bin spike-names --bin spike-events 2>&1 | sed 's/^/  /'
cargo build -q -p orbweaver-registry --bin spike-ifr 2>&1 | sed 's/^/  /'
cargo build -q -p orbweaver-object --bin spike-experts --bin spike-tenants 2>&1 | sed 's/^/  /'
cargo build -q -p orbweaver-giop --bin spike-trading 2>&1 | sed 's/^/  /'

echo "starting fixtures"
start naming names.ior ./target/debug/spike-names "$RUN_DIR/names.ior" --hold
start events events.ior ./target/debug/spike-events "$RUN_DIR/events.ior" --hold
start ifr ifr.ior ./target/debug/spike-ifr "$RUN_DIR/ifr.ior" --hold
# The trader has been on the wire since D022 T4; what it was waiting for was a
# first-party `CosTrading` contract to derive an operation list from, not a
# servant. `spike-trading` publishes exactly one object — the `Lookup` servant
# at key `TradingService` — and `do_trading()` asserts that identity before it
# sweeps, because a probe aimed elsewhere reports an unserved interface rather
# than a failure.
start trading trading.ior ./target/debug/spike-trading "$RUN_DIR/trading.ior" --hold
# The spikes hold their own servants now, so the separate holder crate is
# redundant — and it was strictly weaker: it published one tenant's factory,
# so the isolation claim could not be measured from outside at all.
start moe-experts moe-registry.ior ./target/debug/spike-experts \
    "$RUN_DIR/moe-registry.ior" "$RUN_DIR/moe-loader.ior" "$RUN_DIR/moe-router.ior" --hold
start moe-tenants moe-factory.ior ./target/debug/spike-tenants \
    "$RUN_DIR/moe-factory.ior" "$RUN_DIR/moe-factory-globex.ior" --hold

if [ "$FAILURES" -ne 0 ]; then
    echo
    echo "service-sweep: FAIL — $FAILURES fixture(s) did not start"
    exit 1
fi

echo "sweeping"
OUT="$RUN_DIR/sweep.tsv"
python3 spikes/service_sweep.py "$RUN_DIR" >"$OUT" 2>&1
SWEEP_STATUS=$?

if [ "${1:-}" = "--raw" ]; then
    cat "$OUT"
else
    grep -v '^ROW	' "$OUT"
    echo
    echo "(re-run with --raw for the per-operation rows)"
fi

exit "$SWEEP_STATUS"
