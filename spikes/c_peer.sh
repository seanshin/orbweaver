#!/usr/bin/env bash
# The C peer against our ORB: D033 §6 item 3.4, measured rather than described.
#
# `spikes/c_peer.c` is a C program that speaks GIOP over a socket — not a C ORB,
# and not a binding to anyone's. This runner starts our own servers, drives the
# peer at them, and decides. The peer reports what it saw and never judges; the
# judging is here, because the runner is what knows what was asked for.
#
# ── What it measures ────────────────────────────────────────────────────────
#
#   * four calls (`ping`, `add` twice, `echo_string`) at GIOP 1.0, 1.1 and 1.2,
#     in BOTH byte orders — 24 cells, every one comparing DECODED VALUES. Never
#     raw buffers: CDR padding content is undefined by the specification, and a
#     hand-written encoder will not match ours octet for octet nor should it.
#   * five refusals, in both orders — BAD_OPERATION, MARSHAL, OBJECT_NOT_EXIST,
#     and MessageError for each of a bad magic and an unsupported version. A
#     peer that only shows the happy path proves less than one that shows our
#     server refusing something by name.
#   * one control in which the C peer is the SERVER and a second instance dials
#     it with the OPPOSITE order. It measures the peer's own encoder and
#     **nothing about our ORB**, and is labelled that way everywhere it appears.
#
# ── The order is read, never assumed ────────────────────────────────────────
#
# `spikes/bindings/AXES`: *a cell reports the order it READ out of GIOP
# §15.4.1's flag byte of what the peer actually wrote*, and a cell that asserts
# an order from the peer's language reports it as `claimed`, counted separately
# and never as met. Every assertion below requires `order_source == "observed"`,
# so a case that stopped reading the flag byte and started believing a default
# goes red rather than quiet. That requirement IS the gate for clause 2 here.
#
# ── Exit ────────────────────────────────────────────────────────────────────
#
#   0  every case held
#   1  a case was refuted, OR a fixture that is present would not start — which
#      CLAUDE.md counts as a failure and never as a skip
#   2  no C compiler on this machine: unmeasured, and unmeasured is not passing
#
# ── Modes ───────────────────────────────────────────────────────────────────
#
#   (none)              the human report
#   --cell              print only the suite's `observed`/`note` vocabulary,
#                       for a `cell` row in a language manifest
#   --negative-control  invert five assertions and require every one of them to
#                       go RED. D010 §7.2: a group lands with the command that
#                       was run to make it red. This is that command, kept in
#                       the file rather than only in a commit message, because a
#                       control that is not runnable is a claim about the past.
#
# *피어는 관찰만 보고하고 판정은 러너가 한다. 값은 디코드해서 비교한다 — 원시 버퍼가
# 아니라. 바이트 순서는 §15.4.1의 플래그 바이트에서 읽으며, 읽지 않은 값은 red다.*

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
cd "$root" || exit 1

mode="report"
case "${1:-}" in
    --cell) mode="cell" ;;
    --negative-control) mode="negative" ;;
    "") ;;
    *) echo "usage: $0 [--cell|--negative-control]" >&2; exit 1 ;;
esac

held=0
fails=0
notes=()
observations=()

say() { [ "$mode" = "cell" ] || printf '%s\n' "$*"; }
pass() { held=$((held + 1)); [ "$mode" = "cell" ] || printf '  ok    %s\n' "$1"; }
fail() { fails=$((fails + 1)); printf '  FAIL  %s\n' "$1" >&2; }
note() { notes+=("$1"); [ "$mode" = "cell" ] || printf '  note  %s\n' "$1"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/cpeer-XXXXXX")"
pids=()
cleanup() {
    for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null; done
    for p in "${pids[@]:-}"; do [ -n "$p" ] && wait "$p" 2>/dev/null; done
    if [ -n "${C_PEER_KEEP:-}" ]; then echo "kept: $work" >&2; else rm -rf "$work"; fi
}
trap cleanup EXIT INT TERM

# No harness lock is taken: every port is ephemeral, every fixture is killed by
# PID, and no fixed path under /tmp is written, so a concurrent run_checks.sh
# cannot collide with this and it cannot collide with one.

# ── the peer, built ─────────────────────────────────────────────────────────
#
# The build script's exit status is read FIRST and its two red outcomes are kept
# apart: 2 is "no compiler here" (unmeasured) and 1 is "a compiler is here and
# our source is broken" (a failure). Collapsing them would let a machine without
# `cc` report the same thing as a peer that does not compile.
build_out="$("$here/build_c_peer.sh" 2>&1)"
build_status=$?
if [ "$build_status" -eq 2 ]; then
    printf '%s\n' "$build_out" >&2
    echo "SKIPPED  the C peer is unmeasured on this machine: no C compiler" >&2
    exit 2
fi
if [ "$build_status" -ne 0 ]; then
    printf '%s\n' "$build_out" >&2
    fail "spikes/c_peer.c did not compile, and a compiler is present"
    exit 1
fi
peer="$(tail -1 <<<"$build_out")"
if [ ! -x "$peer" ]; then
    fail "the build reported success but $peer is not executable"
    exit 1
fi
say "peer: $peer"
say "$(head -1 <<<"$build_out")"

# ── our servers ─────────────────────────────────────────────────────────────
if ! cargo build -q -p orbweaver-object --bin spike-server 2>"$work/b1.err" ||
   ! cargo build -q -p orbweaver-giop --bin spike-names 2>"$work/b2.err"; then
    cat "$work/b1.err" "$work/b2.err" >&2
    fail "our own fixtures would not build"
    exit 1
fi

# A wait loop that SLEEPS. `for i in $(seq 1 500); do [ -f f ] && break; done`
# finishes in microseconds and does not wait at all — the harness rule this
# project paid a phantom failure for once already.
wait_file() {
    local f="$1" deadline_s="${2:-30}" i=0 n
    n=$((deadline_s * 20))
    while [ "$i" -lt "$n" ]; do
        [ -s "$f" ] && return 0
        sleep 0.05
        i=$((i + 1))
    done
    return 1
}

./target/debug/spike-server "$work/echo.ior" 127.0.0.1 0 >"$work/echo.log" 2>&1 &
pids+=("$!")
./target/debug/spike-names "$work/names.ior" --hold >"$work/names.log" 2>&1 &
pids+=("$!")

for pair in "echo.ior:spike-server" "names.ior:spike-names"; do
    f="${pair%%:*}"
    who="${pair##*:}"
    if ! wait_file "$work/$f" 60; then
        # A fixture that is present and will not start is a FAILURE, not a skip.
        fail "$who did not publish an IOR within 60s — log follows"
        sed 's/^/        /' "$work/${f%.ior}.log" >&2
        exit 1
    fi
done
# The IOR file is written before the accept loop is entered, so a moment here
# is the difference between measuring the protocol and measuring the race.
sleep 0.4

echo_ior="$work/echo.ior"
names_ior="$work/names.ior"

# ── the judge ───────────────────────────────────────────────────────────────
#
# One python3 process per case, given the peer's JSON on stdin and the
# expectation as arguments. Everything is compared as a DECODED VALUE; the only
# octets this file ever compares are the object key it asked the peer to send.
# The judge lives in a file rather than a heredoc because it needs BOTH its
# arguments and the peer's JSON on stdin, and a function cannot take two stdin
# redirections — the first draft of this file tried and the second one silently
# won.
judge_py="$work/judge.py"
cat >"$judge_py" <<'PY'
import json, sys

want = {}
for arg in sys.argv[1:]:
    k, _, v = arg.partition("=")
    want[k] = v

raw = sys.stdin.read()
try:
    d = json.loads(raw)
except Exception as exc:
    print("the peer did not print one JSON object: %s" % exc)
    sys.exit(1)

if not d.get("connected", False) and want.get("connected") != "false":
    print("the peer did not connect: %s" % d.get("dial_error", "no reason given"))
    sys.exit(1)

ex = d.get("exchanges") or []
if not ex:
    print("the peer completed no exchange")
    sys.exit(1)
e = ex[int(want.get("exchange", "0"))]
r = e.get("reply")
if r is None:
    if want.get("reply") == "none":
        sys.exit(0)
    print("no reply: peer_closed=%s %s" % (e.get("peer_closed"), e.get("why", "")))
    sys.exit(1)

bad = []
def eq(key, got, expected):
    if str(got) != str(expected):
        bad.append("%s: got %r, wanted %r" % (key, got, expected))

# The order clause, and it is not optional. A case whose order was not READ off
# §15.4.1's flag byte is refused here even if every value in it is right.
if "order" in want:
    eq("order", r.get("order"), want["order"])
    if r.get("order_source") != "observed":
        bad.append("order_source: %r, but only an order READ off the flag byte counts"
                   % r.get("order_source"))
    if "flag_byte" not in r:
        bad.append("the peer reported no flag byte, so the order was not read")
    else:
        bit = r["flag_byte"] & 1
        expect_bit = 1 if want["order"] == "little" else 0
        if bit != expect_bit:
            bad.append("flag byte %d has bit 0 = %d, which is not %s"
                       % (r["flag_byte"], bit, want["order"]))

for key, field in (("type", "message_type_name"), ("giop", "giop"),
                   ("status", "reply_status_name"), ("exception", "exception_id"),
                   ("long", "result_long"), ("string", "result_string"),
                   ("completed", "completion_status_name")):
    if key in want:
        eq(key, r.get(field), want[key])

if r.get("truncated"):
    bad.append("the reply was truncated: %s" % r.get("why"))
if r.get("decode_error"):
    bad.append("the peer could not decode the body: %s" % r["decode_error"])
if r.get("more_fragments"):
    bad.append("the reply set MORE_FRAGMENTS, which this fixture never provokes")

if bad:
    print("; ".join(bad))
    sys.exit(1)
sys.exit(0)
PY

judge() {
    local json="$1"; shift
    python3 "$judge_py" "$@" <<<"$json"
}

# Runs the peer and judges one case. The peer's OWN exit status is read before
# anything is matched: a producer that could not run at all is an unmeasured
# check, which is a failure and never a pass.
case_run() {
    local label="$1" ior="$2"; shift 2
    local expects=() args=()
    local seen_sep=0
    for a in "$@"; do
        if [ "$a" = "--" ]; then seen_sep=1; continue; fi
        if [ "$seen_sep" = 1 ]; then expects+=("$a"); else args+=("$a"); fi
    done

    local out status why jstatus
    out="$("$peer" --role client --ior-file "$ior" --deadline-s 15 \
           ${args[@]+"${args[@]}"} 2>&1)"
    status=$?
    if [ "$status" -ne 0 ]; then
        fail "$label: the peer exited $status without running to the end: $out"
        return 1
    fi

    why="$(judge "$out" ${expects[@]+"${expects[@]}"})"
    jstatus=$?
    if [ "$jstatus" -ne 0 ]; then
        fail "$label: $why"
        [ -n "${C_PEER_VERBOSE:-}" ] && printf '        %s\n' "$out" >&2
        return 1
    fi
    pass "$label"
    return 0
}

# Records what the peer READ off the wire, for the suite's vocabulary.
observe() {
    observations+=("$1	$2")
}

# ── the negative control ────────────────────────────────────────────────────
#
# Five assertions inverted, each of which MUST go red. A control that comes back
# green is the finding: it means the case it mirrors was measuring nothing.
if [ "$mode" = "negative" ]; then
    echo "negative control — every line below must be REFUTED"
    ncfail=0
    nc() {
        local label="$1" ior="$2"; shift 2
        local expects=() args=() seen=0
        for a in "$@"; do
            if [ "$a" = "--" ]; then seen=1; continue; fi
            if [ "$seen" = 1 ]; then expects+=("$a"); else args+=("$a"); fi
        done
        local out status why jstatus
        out="$("$peer" --role client --ior-file "$ior" --deadline-s 15 \
               ${args[@]+"${args[@]}"} 2>&1)"
        status=$?
        if [ "$status" -ne 0 ]; then
            printf '  refuted  %s (the peer exited %d)\n' "$label" "$status"
            return 0
        fi
        why="$(judge "$out" ${expects[@]+"${expects[@]}"})"
        jstatus=$?
        if [ "$jstatus" -ne 0 ]; then
            printf '  refuted  %s — %s\n' "$label" "$why"
            return 0
        fi
        printf '  GREEN    %s — this control did not go red, so the case it mirrors measures nothing\n' "$label" >&2
        ncfail=$((ncfail + 1))
        return 1
    }

    nc "ping must NOT answer 43" "$echo_ior" \
        --op ping --expect long -- order=little status=NO_EXCEPTION long=43
    nc "a known operation must NOT be refused as BAD_OPERATION" "$echo_ior" \
        --op ping --expect long -- order=little status=SYSTEM_EXCEPTION \
        exception=IDL:omg.org/CORBA/BAD_OPERATION:1.0
    nc "a big-endian request must NOT come back little" "$echo_ior" \
        --op ping --expect long --request-endian big -- order=little status=NO_EXCEPTION
    nc "add(1000,234) must NOT be 1235" "$echo_ior" \
        --op add --arg-long 1000 --arg-long 234 --expect long -- order=little long=1235
    nc "a bogus key at spike-names must NOT be served" "$names_ior" \
        --op resolve --expect void --object-key-hex 6e6f742d68657265 -- \
        order=little status=NO_EXCEPTION

    # The sixth control is about the ORDER CLAUSE itself rather than a value:
    # the judge must refuse a case whose order was not read off the flag byte.
    # Fed a hand-made object with order_source `claimed`, it has to go red, or
    # every `observed` above is decoration.
    forged='{"connected":true,"exchanges":[{"reply":{"message_type_name":"Reply","order":"little","order_source":"claimed","flag_byte":1,"reply_status_name":"NO_EXCEPTION","result_long":42}}]}'
    why="$(judge "$forged" order=little status=NO_EXCEPTION long=42)"
    if [ $? -ne 0 ]; then
        printf '  refuted  a `claimed` order must not satisfy the order clause — %s\n' "$why"
    else
        printf '  GREEN    the judge accepted a `claimed` order; clause 2 is not being measured\n' >&2
        ncfail=$((ncfail + 1))
    fi

    echo
    if [ "$ncfail" -gt 0 ]; then
        echo "negative control: $ncfail of 6 came back GREEN — this runner is not measuring what it says" >&2
        exit 1
    fi
    echo "negative control: all 6 refuted"
    exit 0
fi

# ── clause 1: every call, both orders, three versions ───────────────────────
say ""
say "calls — 4 operations x 2 byte orders x 3 GIOP versions, decoded values only"
for giop in 1.0 1.1 1.2; do
    for order in little big; do
        case_run "ping at GIOP $giop, $order" "$echo_ior" \
            --op ping --expect long --giop "$giop" --request-endian "$order" \
            -- "order=$order" "giop=$giop" type=Reply status=NO_EXCEPTION long=42 &&
            observe "$giop" "$order"

        # 1000 + 234, deliberately NOT a pair summing to 42: `ping` answers 42,
        # so an `add` case whose arguments summed to 42 would pass while the
        # server ignored them entirely. The first draft of this file used 7 and
        # 35 and could not have told the difference.
        case_run "add(1000,234) at GIOP $giop, $order" "$echo_ior" \
            --op add --arg-long 1000 --arg-long 234 --expect long \
            --giop "$giop" --request-endian "$order" \
            -- "order=$order" "giop=$giop" type=Reply status=NO_EXCEPTION long=1234

        # Negatives, because a sign that survives one order and not the other is
        # exactly the defect a single-endian test cannot see.
        case_run "add(-5,-7) at GIOP $giop, $order" "$echo_ior" \
            --op add --arg-long -5 --arg-long -7 --expect long \
            --giop "$giop" --request-endian "$order" \
            -- "order=$order" "giop=$giop" type=Reply status=NO_EXCEPTION long=-12

        case_run "echo_string at GIOP $giop, $order" "$echo_ior" \
            --op echo_string --arg-string "a hand-written C peer" --expect string \
            --giop "$giop" --request-endian "$order" \
            -- "order=$order" "giop=$giop" type=Reply status=NO_EXCEPTION \
            "string=a hand-written C peer"
    done
done

# ── the refusals ────────────────────────────────────────────────────────────
say ""
say "refusals — what our server says no to, by name, in both orders"
for order in little big; do
    case_run "BAD_OPERATION for an operation we do not serve ($order)" "$echo_ior" \
        --op no_such_operation_at_all --expect void --request-endian "$order" \
        -- "order=$order" type=Reply status=SYSTEM_EXCEPTION \
        exception=IDL:omg.org/CORBA/BAD_OPERATION:1.0 completed=COMPLETED_NO

    # `add` declared and no arguments sent: the body runs out where the first
    # `long` should be, which is the shape a MARSHAL exists for.
    case_run "MARSHAL for a request body that runs out ($order)" "$echo_ior" \
        --op add --expect long --request-endian "$order" \
        -- "order=$order" type=Reply status=SYSTEM_EXCEPTION \
        exception=IDL:omg.org/CORBA/MARSHAL:1.0 completed=COMPLETED_NO

    # Against `spike-names` and not `spike-server`, and the reason is a measured
    # one rather than a preference: `Dispatch::knows` DEFAULTS to accepting every
    # object key — "right for a single-servant process", says its own doc — and
    # `spike-server` does not override it, so it serves `ping` on a key nobody
    # ever activated. `spike-names` overrides `knows` with a real comparison.
    # The peer found this by asking; it is documented behaviour, not a defect,
    # and it is why this refusal needs the second fixture.
    case_run "OBJECT_NOT_EXIST for a key nobody activated ($order)" "$names_ior" \
        --op resolve --expect void --object-key-hex 6e6f742d68657265 \
        --request-endian "$order" \
        -- "order=$order" type=Reply status=SYSTEM_EXCEPTION \
        exception=IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0 completed=COMPLETED_NO

    # §15.4.2: a MessageError answers a message whose magic is not `GIOP` or
    # whose version we do not support. Both are refusals at the framing layer,
    # BELOW the point where a reply status exists, which is why they are checked
    # by message type and not by exception id.
    case_run "MessageError for a magic that is not GIOP ($order)" "$echo_ior" \
        --op ping --expect long --magic GARB --request-endian "$order" \
        -- type=MessageError

    case_run "MessageError for GIOP 1.9, which we do not support ($order)" "$echo_ior" \
        --op ping --expect long --giop 1.9 --request-endian "$order" \
        -- type=MessageError
done

# ── what the peer saw that nobody asked it to look for ──────────────────────
#
# Recorded rather than asserted. A `note` is not a gate and never counts as one;
# these are observations a foreign-shaped peer made that no test in this tree
# was written to make, and they belong in the record either way.
me_out="$("$peer" --role client --ior-file "$echo_ior" --op ping --expect long \
          --magic GARB --request-endian big --deadline-s 15 2>&1)"
me_status=$?
if [ "$me_status" -eq 0 ]; then
    me_order="$(python3 -c '
import json, sys
d = json.loads(sys.stdin.read())
ex = (d.get("exchanges") or [{}])[0]
r = ex.get("reply") or {}
print(r.get("order", "?"))' <<<"$me_out")"
    if [ "$me_order" = "little" ]; then
        note "our server MIRRORS the caller's order on a Reply and does NOT on a"
        note "MessageError: a big-endian request with a bad magic came back with"
        note "flag bit 0 SET (little). §15.4.2 does not require a mirror there and"
        note "the caller reads the flag byte either way, so this is an asymmetry"
        note "worth recording, not a defect. Found by asking, not by review."
    fi
fi

# ── the control in which the peer is the server ─────────────────────────────
#
# Both ends are this file's own C, so it can satisfy NO clause of D030 §3 and
# measures NOTHING about our ORB. It is here because the server role would
# otherwise ship untested, and it is labelled a control every place it appears
# so that it can never be counted as a measurement of ours.
say ""
say "control — the peer as server, dialed by a second instance in the OPPOSITE order"
"$peer" --role server --ior-file "$work/c.ior" --port-file "$work/c.port" \
    --reply-endian big --deadline-s 20 >"$work/cserver.json" 2>"$work/cserver.err" &
cpid=$!
pids+=("$cpid")
if ! wait_file "$work/c.port" 20; then
    fail "the C peer's own server role did not publish a port"
else
    sleep 0.2
    if case_run "the peer's server role answers a little-endian caller in big" \
        "$work/c.ior" --op add --arg-long 1000 --arg-long 234 --expect long \
        --request-endian little \
        -- order=big type=Reply status=NO_EXCEPTION long=1234; then
        note "that case is a CONTROL: both ends are spikes/c_peer.c, so it satisfies"
        note "no clause of D030 §3 and says nothing about our ORB. It exists so the"
        note "server role does not ship unexecuted."
    fi
    wait "$cpid" 2>/dev/null
    if grep -q '"request_order_source":"observed"' "$work/cserver.json" 2>/dev/null; then
        pass "the peer's server role read its caller's order off the flag byte too"
    else
        fail "the peer's server role did not report reading a caller order"
    fi
fi

# ── the verdict ─────────────────────────────────────────────────────────────
if [ "$mode" = "cell" ]; then
    # Only `observed` counts toward the suite's clause 2, and every line here was
    # read off §15.4.1's flag byte by the peer that received it.
    if [ "${#observations[@]}" -gt 0 ]; then
        printf '%s\n' "${observations[@]}" | sort -u | while IFS=$'\t' read -r v o; do
            printf 'observed\tgiop=%s\torder=%s\n' "$v" "$o"
        done
    fi
    printf 'note\t%d case(s) held, %d refuted, against a hand-written C peer that links no ORB\n' \
        "$held" "$fails"
    for n in "${notes[@]:-}"; do
        [ -n "$n" ] && printf 'note\t%s\n' "$n"
    done
    [ "$fails" -gt 0 ] && exit 1
    exit 0
fi

say ""
say "held $held · refuted $fails"
if [ "$fails" -gt 0 ]; then
    exit 1
fi
exit 0
