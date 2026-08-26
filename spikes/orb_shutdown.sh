#!/usr/bin/env bash
# What a peer mid-call sees when the ORB stops the server under it (D029 §5 O1).
#
# Two processes and one claim. `spike-orb-shutdown` binds through `Orb::server`,
# serves with `|| false` as the caller's own predicate, holds its servant on the
# first call, and calls `Orb::shutdown` the moment the servant reports it was
# entered. `spikes/orb_shutdown_peer.py` is the peer: stdlib only, no ORB
# imported, two pipelined requests assembled by hand from §9.4.
#
# **The measurement is the peer's exit code.** The fixture's own exit code is a
# second, weaker check about our own counters, and it is reported separately so
# the two cannot be confused — a fixture that says it stopped a server proves
# nothing about what a client received.
#
# Both byte orders, because an assertion that only ever ran little-endian is an
# assertion about this machine.
#
# This is **not yet a `run_checks.sh` group**: the gate for this claim is
# `crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`, which runs in
# `cargo test --workspace`. What this adds is provenance — a peer that applies
# none of our conventions. Wiring it into the harness is left undone and named
# in D034 §9.
#
# Exit: 0 all cases held · 1 refuted · 3 nothing measured.
#
# *측정은 피어의 종료 코드다. 픽스처가 서버를 멈췄다고 말하는 것은 클라이언트가
# 무엇을 받았는지에 대해 아무것도 증명하지 않는다.*

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail=0
unmeasured=0
held=0

echo "building the fixture"
if ! cargo build -q --manifest-path "$root/Cargo.toml" -p orbweaver-giop --bin spike-orb-shutdown; then
    echo "UNMEASURED  the fixture would not build"
    exit 3
fi
fixture="$root/target/debug/spike-orb-shutdown"

for endian in big little; do
    port_file="$work/port.$endian"
    rm -f "$port_file"

    "$fixture" --port-file "$port_file" >"$work/fixture.$endian.json" 2>"$work/fixture.$endian.err" &
    fixture_pid=$!

    # A wait loop that sleeps. `for i in $(seq 1 500); do [ -f f ] && break; done`
    # finishes in microseconds and does not wait at all — the harness rule this
    # project paid for once already.
    port=""
    deadline=$((SECONDS + 20))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if [ -s "$port_file" ]; then
            port="$(cat "$port_file")"
            break
        fi
        if ! kill -0 "$fixture_pid" 2>/dev/null; then
            break
        fi
        sleep 0.05
    done

    if [ -z "$port" ]; then
        echo "UNMEASURED  $endian: the fixture never published a port"
        cat "$work/fixture.$endian.err" >&2 || true
        kill "$fixture_pid" 2>/dev/null || true
        wait "$fixture_pid" 2>/dev/null || true
        unmeasured=$((unmeasured + 1))
        continue
    fi

    set +e
    peer_out="$(python3 "$here/orb_shutdown_peer.py" --port "$port" --endian "$endian" 2>&1)"
    peer_status=$?
    set -e

    wait "$fixture_pid"
    fixture_status=$?
    fixture_out="$(cat "$work/fixture.$endian.json")"

    echo "  peer     $endian: exit $peer_status  $peer_out"
    echo "  fixture  $endian: exit $fixture_status  $fixture_out"

    case "$peer_status" in
        0) held=$((held + 1)) ;;
        1)
            echo "REFUTED     $endian: the peer did not see what D034 §3 says it must"
            fail=$((fail + 1))
            ;;
        *)
            echo "UNMEASURED  $endian: the peer could not measure"
            unmeasured=$((unmeasured + 1))
            ;;
    esac

    # Reported, never allowed to vouch for the peer. A fixture that miscounted
    # its own servers is worth knowing about and is not evidence either way
    # about what the client received.
    if [ "$fixture_status" -ne 0 ]; then
        echo "note        $endian: the fixture's own check failed (exit $fixture_status)"
        fail=$((fail + 1))
    fi
done

echo
echo "held $held · refuted-or-broken $fail · unmeasured $unmeasured"
if [ "$unmeasured" -gt 0 ]; then
    exit 3
fi
if [ "$fail" -gt 0 ]; then
    exit 1
fi
exit 0
