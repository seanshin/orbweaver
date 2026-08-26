#!/usr/bin/env bash
# Routing-domain probe for risk R7, across a REAL second host.
#
# ***THIS SCRIPT HAS EXECUTED*** — unlike everything else under spikes/nat/.
# First run 2026-08-14 on macOS 26 (Darwin 25.6.0) against multipass 1.16.3+mac
# with the qemu driver, Ubuntu 24.04. Its transcript is quoted in
# docs/PHASE6.md; the result there is the one this script produced, not a
# prediction of what it would produce.
#
#   ./spikes/nat/vm/run.sh              # launch a VM, probe, delete it
#   ORBWEAVER_KEEP=1 ./spikes/nat/vm/run.sh   # leave the VM running
#
# ── Why a VM and not a container ─────────────────────────────────────────────
#
# The same reason `spikes/nat/run.sh` and `spikes/nat/k8s/run.sh` exist and
# have never run: this project's machine has no container engine, and on macOS
# every engine that could be installed is a Linux VM underneath anyway. So the
# probe skips the middle layer. What R7 needs is not a container; it is a
# client whose **routing domain differs from the servant's**, and a second host
# on a bridged network is that, without qualification.
#
# ── The experiment, and why it is shaped this way ────────────────────────────
#
# The servant binds `0.0.0.0:PORT` on the **host** and is therefore genuinely
# reachable from the VM at the bridge address, in BOTH cases. Only the address
# written into the reference changes:
#
#   naive      publishes 127.0.0.1:PORT — the address an ORB believes it has
#              when it is behind any kind of boundary. In the VM's routing
#              domain that names the VM itself. The dial must FAIL.
#   published  publishes <bridge>:PORT through ORBWEAVER_PUBLISH_MAP.
#              The dial must SUCCEED and ping() must return 42.
#
# Holding the servant reachable in both cases is the point. It isolates the
# variable: the naive case does not fail because the server is missing, it
# fails because the *reference* names an address that means something else
# where the client is standing. That is R7, with nothing else in the way.
#
# A run where both cases succeed has not demonstrated the fix — it means the
# client was not in another routing domain, which is a broken probe rather than
# a pass, and this script fails it as one.
set -uo pipefail
cd "$(dirname "$0")/../../.."
ROOT=$(pwd)

VM=${ORBWEAVER_VM:-r7client}
PORT=${ORBWEAVER_PORT:-15555}
IMAGE=${ORBWEAVER_VM_IMAGE:-24.04}
KEY_TEXT="nat-servant"
TYPE_ID="IDL:spike/Echo:1.0"

fails=0
WORK=$(mktemp -d -t orbweaver-r7-vm) || exit 2
SERVE_PID=""

bold() { printf '\n\033[1m%s\033[0m\n' "$1"; }
pass() { printf '  ok   %s\n' "$1"; }
fail() {
  printf '  FAIL %s\n' "$1"
  fails=$((fails + 1))
}
note() { printf '  ..   %s\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || {
  echo "missing tool: $1" >&2
  exit 2
}; }

stop_servant() {
  # By captured PID, never by pattern: a pattern kill in this project's harness
  # has taken out the wrong process before.
  [ -n "$SERVE_PID" ] && kill -TERM "$SERVE_PID" 2>/dev/null
  [ -n "$SERVE_PID" ] && wait "$SERVE_PID" 2>/dev/null
  SERVE_PID=""
}

cleanup() {
  stop_servant
  if [ "${ORBWEAVER_KEEP:-0}" != "1" ] && [ "${LAUNCHED:-0}" = "1" ]; then
    multipass delete --purge "$VM" >/dev/null 2>&1
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

need multipass
need cargo

hex_of() { printf '%s' "$1" | od -An -tx1 | tr -d ' \n'; }

bold "R7 across a real routing boundary — a second host, not a simulation"

# ── The VM ───────────────────────────────────────────────────────────────────
LAUNCHED=0
state=$(multipass info "$VM" --format csv 2>/dev/null | awk -F, 'NR==2 {print $2}')
if [ -z "$state" ]; then
  note "launching $VM ($IMAGE); this downloads an image the first time"
  # Multipass refuses an image whose minimum disk exceeds free space, before
  # downloading. spikes/nat/preflight.sh reports that number.
  if ! multipass launch --name "$VM" --cpus 2 --memory 2G --disk 8G "$IMAGE"; then
    fail "could not launch a VM; run spikes/nat/preflight.sh for the reason"
    exit 1
  fi
  LAUNCHED=1
elif [ "$state" != "Running" ]; then
  multipass start "$VM" >/dev/null 2>&1
fi

# ── The client's routing domain, established rather than assumed ─────────────
GUEST=$(multipass info "$VM" --format csv 2>/dev/null | awk -F, 'NR==2 {print $3}')
BRIDGE=$(multipass exec "$VM" -- ip route 2>/dev/null | awk '/^default/ {print $3; exit}')
if [ -z "$GUEST" ] || [ -z "$BRIDGE" ]; then
  fail "could not read the VM's address or its route to the host"
  exit 1
fi
note "servant runs on the host; client runs in $VM ($GUEST)"
note "the host is $BRIDGE from inside the VM — a different routing domain"

# ── The client binary ────────────────────────────────────────────────────────
# Cross-compiled here and copied in, rather than built inside the VM. The
# first attempt did build it inside the VM and got as far as discovering that
# this host's VPN eats the guest's NAT: the VM reaches the host at the bridge
# address (0% loss) and reaches nothing beyond it, so `apt` and `rustup` both
# hang. Cross-compiling removes the guest's need for a network entirely, which
# is a better probe anyway — the only traffic left is the traffic being
# measured.
#
# musl and `rust-lld` between them mean no C toolchain is needed on either
# side: a statically linked guest binary out of a macOS host, with nothing
# installed but a rustup target.
GUEST_ARCH=$(multipass exec "$VM" -- uname -m 2>/dev/null | tr -d '\r\n')
case "$GUEST_ARCH" in
  aarch64 | arm64) TRIPLE=aarch64-unknown-linux-musl ;;
  x86_64) TRIPLE=x86_64-unknown-linux-musl ;;
  *)
    fail "unrecognised guest architecture ${GUEST_ARCH:-<unknown>}"
    exit 1
    ;;
esac
note "guest is $GUEST_ARCH; cross-compiling the client for $TRIPLE"
rustup target add "$TRIPLE" >/dev/null 2>&1
LLD_DIR="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
LINKER_VAR="CARGO_TARGET_$(printf '%s' "$TRIPLE" | tr 'a-z-' 'A-Z_')_LINKER"
if ! PATH="$LLD_DIR:$PATH" env "$LINKER_VAR=rust-lld" \
  cargo build -q -p orbweaver-giop --bin spike-nat --target "$TRIPLE"; then
  fail "could not cross-compile the client for $TRIPLE"
  exit 1
fi
GUEST_BIN=/home/ubuntu/spike-nat
multipass transfer "${CARGO_TARGET_DIR:-$ROOT/target}/$TRIPLE/debug/spike-nat" \
  "$VM:$GUEST_BIN" >/dev/null 2>&1 || {
  fail "could not copy the client into the VM"
  exit 1
}
multipass exec "$VM" -- chmod +x "$GUEST_BIN"

cargo build -q --bin spike-nat || exit 1
HOST_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/spike-nat"

# ── One case ─────────────────────────────────────────────────────────────────
# The servant is bound wide and reachable from the VM in both cases. Only the
# published address differs, so the dial's outcome is attributable to the
# reference and to nothing else.
run_case() { # run_case <label> <published-host> <pass|fail> <ior-name>
  local label="$1" pub="$2" want="$3" name="$4"
  local ior="$WORK/$name.ior"
  local log="$WORK/$name.log"

  ORBWEAVER_PUBLISH_MAP="0.0.0.0:$PORT=$pub:$PORT" \
    "$HOST_BIN" serve "0.0.0.0:$PORT" "$ior" >"$log" 2>&1 &
  SERVE_PID=$!

  # Sleeping and deadline-bounded, and it gives up early if the servant died —
  # waiting out the full deadline on a dead process blames a timeout for a
  # bind error.
  local i published=""
  for i in $(seq 1 80); do
    [ -s "$ior" ] && break
    kill -0 "$SERVE_PID" 2>/dev/null || break
    sleep 0.25
  done
  if [ ! -s "$ior" ]; then
    # An unmeasured check is a failure, never a pass.
    fail "$label: the servant never published a reference"
    sed 's/^/       /' "$log"
    stop_servant
    return 1
  fi
  published=$(sed -n 's/^published \([^ ]*\) .*/\1/p' "$log")
  note "$label: servant bound 0.0.0.0:$PORT, published $published"

  multipass transfer "$ior" "$VM:/home/ubuntu/$name.ior" >/dev/null 2>&1 || {
    fail "$label: could not hand the reference to the client"
    stop_servant
    return 1
  }
  # Captured, then matched. Never piped into a consumer that can exit early.
  local out status
  out=$(multipass exec "$VM" -- "$GUEST_BIN" call "/home/ubuntu/$name.ior" 2>&1)
  status=$?
  printf '%s\n' "$out" | sed 's/^/       /'
  stop_servant

  if [ "$want" = pass ] && [ "$status" -eq 0 ]; then
    pass "$label: the call completed from the other routing domain"
    return 0
  fi
  if [ "$want" = fail ] && [ "$status" -ne 0 ]; then
    pass "$label: the client could not dial it, as R7 predicts"
    return 0
  fi
  fail "$label: wanted the dial to $want, it did not"
  [ "$want" = fail ] &&
    note "both cases reachable means the client was NOT in another routing domain"
  return 1
}

bold "the reference an ORB publishes when it believes it is at loopback"
run_case "naive" "127.0.0.1" fail naive

bold "the same servant, published through an endpoint map"
run_case "published" "$BRIDGE" pass published

# ── What the rewrite did not touch ───────────────────────────────────────────
# Partial by construction: this compares the two references the two runs
# produced. Full field preservation — profile count, IIOP version, an
# undecodable profile's bytes — is measured by `spike-nat prove` on the host
# and by the unit tests in `nat.rs`, not here.
bold "identity across the two references"
key_hex=$(hex_of "$KEY_TEXT")
tid_hex=$(hex_of "$TYPE_ID")
both_ok=1
# Herestrings, and the read's own status first — but for a reason worth writing
# down, because the obvious one is **wrong** and was measured wrong here on
# 2026-08-27. `grep -q` cannot decide before it has a *complete line*, so the
# early exit that SIGPIPEs the producer only happens once a whole matching line
# has arrived. A stringified IOR is one unbroken line: `grep` is obliged to read
# it to the end, the `printf` is never killed, and at 1 MB of single line the
# pipeline still answered `status=0`. So these two were **not** lying, and the
# hazard is governed by where the first complete matching line ends rather than
# by how much output there is. They are herestrings anyway — the form is not a
# judgement call about today's payload — and the live defect this loop actually
# had is the one below it: an unreadable .ior made `body` empty, both greps
# missed, and an unmeasured check was reported as "the rewrite dropped the key".
# That is a different observation and is now counted as itself.
for name in naive published; do
  if ! body=$(tr 'A-Z' 'a-z' <"$WORK/$name.ior" 2>/dev/null); then
    note "could not read $WORK/$name.ior — the identity check is unmeasured for $name"
    both_ok=0
    continue
  fi
  grep -q "$key_hex" <<<"$body" || both_ok=0
  grep -q "$tid_hex" <<<"$body" || both_ok=0
done
if [ "$both_ok" -eq 1 ]; then
  pass "object key \"$KEY_TEXT\" and type id \"$TYPE_ID\" appear verbatim in both"
  note "profile count, IIOP version and undecodable profiles: see spike-nat prove"
else
  fail "a reference lost its object key or type id"
fi

bold "verdict"
echo "  failures: $fails"
if [ "$fails" -eq 0 ]; then
  echo "  vm routing-domain probe: PASS"
else
  echo "  vm routing-domain probe: FAIL"
fi
[ "${ORBWEAVER_KEEP:-0}" = "1" ] &&
  echo "  note: $VM left running; 'multipass delete --purge $VM' removes it"
[ "$fails" -eq 0 ] || exit 1
