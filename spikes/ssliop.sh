#!/usr/bin/env bash
# ssliop.sh — SSLIOP against a peer that is a socket, not an ORB. D010 §4 B3.
#
#   ./spikes/ssliop.sh
#
# Why this exists at all
#   `run_checks.sh` has printed `SKIPPED  no SSL peer — omniORBpy has no sslTP
#   here and JacORB SSL is not configured` for the life of the project, and
#   `spikes/tls/PEER-STATUS.md` records the probe that blocked it. The premise
#   is true. The conclusion does not follow, for the same reason it did not
#   for B5: **SSLIOP is not a protocol an ORB implements.** The Security
#   Service's SSLIOP chapter defines unmodified GIOP over TLS plus a
#   `TAG_SSL_SEC_TRANS` component saying where the TLS listener is — no
#   handshake of its own, no negotiation, no framing. What peer proof needs is
#   therefore a peer that speaks IIOP over TLS, and `spikes/ssliop_peer.py` is
#   one: stdlib `ssl`, the certificates that have been in `spikes/tls/` since
#   2026-08-13, and every GIOP and IOR octet built by hand.
#
#   *전제는 참이고 결론은 따라 나오지 않는다. SSLIOP은 TLS 위의 GIOP과 IOR
#   컴포넌트가 전부이므로 필요한 피어는 ORB가 아니라 소켓이다.*
#
# What it does NOT close, honestly
#   A `TAG_SSL_SEC_TRANS` component produced by **omniORB's or JacORB's own**
#   encoder, with the association-option bits and port convention that
#   implementation chose. That is a claim about their encoder and only they can
#   make it. Everything else B3 names is measured here.
#
# The two parts, and why they are separate
#   A. The advertisement, through `spike-dump` — a binary that exists today.
#      The peer publishes a stringified IOR **it built by hand**, so
#      `Ior::parse`, `ssliop::advertised` and `ssliop::ssl_endpoint` read
#      octets this project's encoder did not write. Both IOR byte orders and
#      both component byte orders, independently: an encapsulation restarts
#      alignment and carries its own order octet, so a little-endian component
#      inside a big-endian IOR is a shape a deployment produces and our own
#      encoder never does. Absence and unreadability are checked too, because
#      "present but unreadable" silently treated as absent is a downgrade.
#   B. The transport, through `spike-ssliop` — a binary that **does not exist
#      yet**: `spikes/spike_ssliop.rs` is staged for
#      `crates/orbweaver-giop/src/bin/`, which was held by three other batches
#      the day this landed. Until it moves, part B counts every one of its
#      cases as UNMEASURED and this script exits 3. An unmeasured check is a
#      failure and never a pass; it is exit 3 rather than exit 1 so that a run
#      in which nothing happened is never read as the claim being refuted.
#      `SPIKE_SSLIOP=<path>` overrides, which is how part B was measured
#      before the move.
#
# Both byte orders, per CLAUDE.md. The reply's order is chosen independently
# of the request's — GIOP sets it per message and `Connection` always writes
# its own native order, so a peer that echoed the request would leave one of
# the two orders unmeasured on any one machine.
#
# The exit code is the verdict. Every probe here is an exit status or a
# comparison between two processes' accounts; nothing is decided by grepping a
# marker out of a stream that could echo it, and no output is piped into
# `grep -q` — a herestring, because a pipe lies twice over (SIGPIPE, and
# `pipefail` turning a failed producer into "no match").
#
# No harness lock is taken: every port is ephemeral, no fixed /tmp path is
# written, and nothing is killed by pattern.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

fails=0
cases=0
unmeasured=0

bold() { printf '\n\033[1m%s\033[0m\n' "$1"; }
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }
skip_unmeasured() {
  printf '  FAIL %s\n' "$1"
  fails=$((fails + 1))
  unmeasured=$((unmeasured + 1))
}
note() { printf '  ..   %s\n' "$1"; }

verdict() {
  echo
  if [ "$fails" -eq 0 ]; then
    echo "ssliop: PASS — $cases cases measured, both byte orders, five refusals"
    exit 0
  fi
  echo "ssliop: FAIL — $fails of $cases cases ($unmeasured of them UNMEASURED, which is not a pass)"
  # Nothing measured is told apart from a claim that did not hold: exit 3 only
  # when every failure was an absence of measurement.
  if [ "$unmeasured" -eq "$fails" ]; then exit 3; fi
  exit 1
}

bold "B3 — SSLIOP against a peer that speaks IIOP over TLS"

if ! command -v cargo >/dev/null 2>&1; then
  fail "cargo is not on PATH, so nothing here was measured"
  unmeasured=$((unmeasured + 1))
  verdict
fi
# An absent python3 is a FAIL and not a skip: this script's whole subject is
# the peer, so without it nothing was measured.
if ! command -v python3 >/dev/null 2>&1; then
  fail "python3 is not on PATH, so the scripted peer could not run and nothing was measured"
  unmeasured=$((unmeasured + 1))
  verdict
fi
for f in ca.pem wrong-ca.pem server.pem server.key; do
  if [ ! -s "$ROOT/spikes/tls/$f" ]; then
    fail "spikes/tls/$f is missing — run spikes/tls/regen.sh; nothing was measured"
    unmeasured=$((unmeasured + 1))
    verdict
  fi
done

build_out=$(cargo build -q --bin spike-dump 2>&1)
if [ $? -ne 0 ]; then
  fail "spike-dump did not build, so part A measured nothing"
  unmeasured=$((unmeasured + 1))
  printf '%s\n' "$build_out" | sed 's/^/       | /'
  verdict
fi
DUMP="$ROOT/target/debug/spike-dump"

# Part B's driver. Absent by design until `spikes/spike_ssliop.rs` moves into
# the crate; the reason is printed once and every part B case is then counted
# as unmeasured rather than quietly dropped.
DRIVER="${SPIKE_SSLIOP:-}"
driver_why=""
if [ -n "$DRIVER" ]; then
  if [ ! -x "$DRIVER" ]; then
    driver_why="SPIKE_SSLIOP=$DRIVER is not executable"
    DRIVER=""
  fi
else
  drv_out=$(cargo build -q -p orbweaver-giop --features ssliop --bin spike-ssliop 2>&1)
  if [ $? -eq 0 ] && [ -x "$ROOT/target/debug/spike-ssliop" ]; then
    DRIVER="$ROOT/target/debug/spike-ssliop"
  else
    driver_why="spike-ssliop does not exist yet — spikes/spike_ssliop.rs is staged for crates/orbweaver-giop/src/bin/ (see its header); set SPIKE_SSLIOP to a build of it"
    DRIVER=""
  fi
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ssliop.XXXXXX")
PEERS=""
cleanup() {
  for p in $PEERS; do kill "$p" 2>/dev/null; done
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── starting and waiting for the peer ────────────────────────────────────────
# Sets PEER_PID, PEER_PORT, IOR_FILE, PEER_OUT, PEER_ERR on success; returns 1
# and reports the failure itself otherwise.
start_peer() {
  local label="$1"; shift
  IOR_FILE="$WORK/ior.$cases"
  PEER_OUT="$WORK/peer.$cases"
  PEER_ERR="$WORK/peererr.$cases"
  local port_file="$WORK/port.$cases"

  python3 "$ROOT/spikes/ssliop_peer.py" \
    --port-file "$port_file" --ior-file "$IOR_FILE" "$@" \
    >"$PEER_OUT" 2>"$PEER_ERR" &
  PEER_PID=$!
  PEERS="$PEERS $PEER_PID"

  # A wait loop that sleeps, bounded by a deadline, and that gives up early if
  # what it waits for has died. A loop without the sleep does not wait at all —
  # that rule has its own line in CLAUDE.md because it produced a phantom
  # failure here once.
  local waited=0
  while [ ! -s "$port_file" ]; do
    if ! kill -0 "$PEER_PID" 2>/dev/null; then
      skip_unmeasured "$label: UNMEASURED — the peer exited before publishing an address"
      sed 's/^/       | /' "$PEER_ERR"
      return 1
    fi
    if [ "$waited" -ge 300 ]; then
      kill "$PEER_PID" 2>/dev/null; wait "$PEER_PID" 2>/dev/null
      skip_unmeasured "$label: UNMEASURED — the peer never published an address (15s)"
      return 1
    fi
    sleep 0.05
    waited=$((waited + 1))
  done
  PEER_PORT=$(cat "$port_file")
  return 0
}

# One field of the peer's JSON account. Strings come back bare; everything else
# comes back as its JSON literal, so `true`, `false` and `null` compare as
# themselves. A file that will not parse exits non-zero and the caller treats
# that as unmeasured.
peer_field() {
  python3 -c 'import json,sys
v = json.load(open(sys.argv[1])).get(sys.argv[2])
print(v if isinstance(v, str) else json.dumps(v))' "$1" "$2"
}

# One `key=value` line of a driver or dump output held in a variable. A
# herestring, never a pipe: `grep -q`/`head` SIGPIPE the producer, and under
# `pipefail` a failed producer reads as "no match".
field_of() { sed -n "s/^$2=//p" <<<"$1" | tail -1; }

# ── part A: the advertisement, through a binary that exists ──────────────────
# The peer serves plain GIOP at the *profile* port so that `spike-dump`'s own
# exit status is a verdict about a completed round trip, and the `ssliop` lines
# it printed on the way are then compared against what the peer says it
# advertised. Two processes, or the claim is this side agreeing with itself.
dump_case() {
  local label="$1" ior_e="$2" comp_e="$3" advertise="$4" want="$5"
  cases=$((cases + 1))
  start_peer "$label" --transport plain --advertise "$advertise" \
    --ior-endian "$ior_e" --component-endian "$comp_e" --requests 1 --deadline-s 20 || return

  local out status
  out=$("$DUMP" "$IOR_FILE" ping 2>&1)
  status=$?
  if [ "$status" -ne 0 ]; then
    fail "$label: spike-dump exited $status against the peer's own IOR"
    printf '%s\n' "$out" | sed 's/^/       | /'
    kill "$PEER_PID" 2>/dev/null; wait "$PEER_PID" 2>/dev/null
    return
  fi
  wait "$PEER_PID"
  local peer_status=$?
  if [ "$peer_status" -ne 0 ]; then
    fail "$label: the peer's script did not run to the end (exit $peer_status)"
    sed 's/^/       | /' "$PEER_ERR"
    return
  fi

  local listen_port advertised
  listen_port=$(peer_field "$PEER_OUT" listen_port) || {
    skip_unmeasured "$label: UNMEASURED — the peer wrote no account"; return; }
  advertised=$(peer_field "$PEER_OUT" advertised_tls_port)

  # `want` is a template: %P is the port the peer says it listens on. Written
  # this way so the expected text is derived from the peer's account rather
  # than from a number typed twice.
  local expected="${want//%P/$listen_port}"
  local seen
  seen=$(sed -n 's/^ssliop  //p' <<<"$out")
  if [ -z "$seen" ]; then
    skip_unmeasured "$label: UNMEASURED — spike-dump printed no ssliop line at all"
    printf '%s\n' "$out" | sed 's/^/       | /'
    return
  fi
  # A glob, so a case may pin its own head and leave the tail to the crate that
  # owns it: the *reason* an encapsulation would not read is `orbweaver-cdr`'s
  # sentence, and retyping it here would make this script go red the day that
  # crate rewords a diagnostic for a good reason. What is pinned is what this
  # script owns an opinion about — that unreadable is reported as unreadable
  # and never as absent.
  case "$seen" in
    $expected) : ;;
    *)
      fail "$label: the advertisement read back as"
      printf '       | got:  %s\n' "$seen"
      printf '       | want: %s\n' "$expected"
      return
      ;;
  esac
  if [ "$advertise" = "ssl-only" ] || [ "$advertise" = "same-port" ]; then
    if [ "$advertised" != "$listen_port" ]; then
      fail "$label: the peer advertised $advertised and listens on $listen_port"
      return
    fi
  fi
  pass "$label"
}

# ── part B: the transport, through the driver ────────────────────────────────
# $1 label  $2 transport  $3 advertise  $4 ca  $5 expect  $6 reply endian
# $7 ior endian  $8 component endian  $9 peer deadline
b_case() {
  local label="$1" transport="$2" advertise="$3" ca="$4" expect="$5"
  local reply_e="$6" ior_e="$7" comp_e="$8" deadline="$9"
  cases=$((cases + 1))
  if [ -z "$DRIVER" ]; then
    skip_unmeasured "$label: UNMEASURED — $driver_why"
    return
  fi
  start_peer "$label" --transport "$transport" --advertise "$advertise" \
    --ior-endian "$ior_e" --component-endian "$comp_e" \
    --reply-endian "$reply_e" --requests 1 --deadline-s "$deadline" || return

  # `--expect-reply-endian` is passed unconditionally: the driver consults it
  # only where a reply exists, and an unset array under `set -u` is an error on
  # the bash macOS ships.
  local out status
  out=$("$DRIVER" --ior "$IOR_FILE" --ca "$ROOT/spikes/tls/$ca" \
    --expect "$expect" --a 7 --b 35 --expect-reply-endian "$reply_e" 2>&1)
  status=$?

  # Exit 3 is the driver saying it never got as far as a measurement, so it has
  # no account to be right or wrong about. Still a failure, and counted apart
  # from a refutation. Checked before the peer is waited on, because a peer
  # nobody reached is sitting in `accept` and waiting for it costs its deadline.
  if [ "$status" -eq 3 ]; then
    skip_unmeasured "$label: UNMEASURED — the driver measured nothing"
    printf '%s\n' "$out" | sed 's/^/       | /'
    kill "$PEER_PID" 2>/dev/null; wait "$PEER_PID" 2>/dev/null
    return
  fi

  wait "$PEER_PID"
  local peer_status=$?
  if [ "$status" -ne 0 ]; then
    fail "$label: the client's account is wrong (exit $status)"
    printf '%s\n' "$out" | sed 's/^/       | /'
    return
  fi
  if [ "$peer_status" -ne 0 ]; then
    fail "$label: the peer's script did not run to the end (exit $peer_status)"
    sed 's/^/       | /' "$PEER_ERR"
    return
  fi

  local accepted handshake listen_port hello key_ok
  accepted=$(peer_field "$PEER_OUT" accepted) || {
    skip_unmeasured "$label: UNMEASURED — the peer wrote no account"; return; }
  handshake=$(peer_field "$PEER_OUT" handshake)
  listen_port=$(peer_field "$PEER_OUT" listen_port)
  hello=$(peer_field "$PEER_OUT" client_hello)
  key_ok=$(peer_field "$PEER_OUT" object_key_matched)

  # The cross-checks the client cannot do for itself, one per direction.
  case "$expect" in
    ok)
      if [ "$accepted" != "true" ] || [ "$handshake" != "ok" ]; then
        fail "$label: the peer's account of the handshake is '$handshake' (accepted=$accepted)"
        return
      fi
      if [ "$key_ok" != "true" ]; then
        fail "$label: the caller used an object key the peer did not publish"
        return
      fi
      local dialed
      dialed=$(field_of "$out" tls_endpoint)
      if [ "$dialed" != "127.0.0.1:$listen_port" ]; then
        fail "$label: ssl_endpoint answered '$dialed' and the peer listens on $listen_port"
        return
      fi
      ;;
    refused)
      case "$transport" in
        tls)
          # The refusal must be the certificate's, and the peer must have seen
          # a handshake begin and fail. A refusal for want of an advertisement
          # would be green over nothing.
          if [ "$accepted" != "true" ]; then
            fail "$label: the peer saw no connection at all, so nothing was refused"
            return
          fi
          case "$handshake" in
            failed:*) : ;;
            *) fail "$label: the peer's handshake account is '$handshake', not a failure"; return ;;
          esac
          local why
          why=$(field_of "$out" refusal)
          case "$why" in
            *ertificate*) : ;;
            *) fail "$label: the refusal does not name the certificate: $why"; return ;;
          esac
          ;;
        plain)
          # The far end's positive evidence that the client attempted TLS and
          # did not quietly downgrade: a ClientHello arrived in cleartext.
          if [ "$advertise" = "ssl-only" ]; then
            if [ "$hello" != "true" ]; then
              fail "$label: the peer did not see a TLS ClientHello (accepted=$accepted)"
              return
            fi
          elif [ "$accepted" != "true" ]; then
            : # `elsewhere`: nobody connected, which is the point — see below.
          else
            fail "$label: the client fell back to the cleartext listener"
            return
          fi
          ;;
      esac
      ;;
    no-tls-endpoint)
      if [ "$accepted" != "false" ]; then
        fail "$label: a live cleartext listener was dialed after no usable advertisement"
        return
      fi
      ;;
  esac
  pass "$label"
}

note "the peer is spikes/ssliop_peer.py — stdlib ssl, no ORB, every GIOP and IOR octet by hand"

bold "A. the advertisement, read out of the peer's own IOR (spike-dump)"
SUP="supports=0x0026 requires=0x0006"
for ior_e in big little; do
  for comp_e in big little; do
    dump_case "IOR $ior_e, component $comp_e" "$ior_e" "$comp_e" same-port \
      "$SUP port=0
TLS endpoint would be 127.0.0.1:%P"
  done
done
dump_case "no component at all" little big none "no TAG_SSL_SEC_TRANS"
dump_case "present but unreadable" little big unreadable \
  "TAG_SSL_SEC_TRANS present but unreadable: *"

bold "B. the transport: GIOP over TLS to another process (spike-ssliop)"
[ -z "$DRIVER" ] && note "$driver_why"
for ior_e in big little; do
  for comp_e in big little; do
    for reply_e in big little; do
      b_case "TLS call — IOR $ior_e, component $comp_e, reply $reply_e" \
        tls ssl-only ca.pem ok "$reply_e" "$ior_e" "$comp_e" 20
    done
  done
done
for reply_e in big little; do
  b_case "TLS call at the profile's own port (component port 0), reply $reply_e" \
    tls same-port ca.pem ok "$reply_e" little big 20
done

bold "   and the refusals, which are what a security-shaped claim owes"
b_case "a certificate our CA did not sign is refused, by name" \
  tls ssl-only wrong-ca.pem refused big little big 20
b_case "a plaintext peer at the advertised SSL port is refused" \
  plain ssl-only ca.pem refused big little big 20
b_case "an advertisement pointing elsewhere does not fall back to the cleartext port" \
  plain elsewhere ca.pem refused big little big 4
b_case "no advertisement is not a licence to dial cleartext" \
  plain none ca.pem no-tls-endpoint big little big 4
b_case "an unreadable advertisement is not an absent one" \
  plain unreadable ca.pem no-tls-endpoint big little big 4

verdict
