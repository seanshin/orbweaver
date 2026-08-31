# wait_accepting — wait until a fixture's published endpoint ACCEPTS, not until
# its IOR file exists.
#
# `. spikes/lib/accepting.sh` and then:
#   wait_accepting <ior-path> [--ready <logfile> <regex>] [--deadline <seconds>]
#
# **A published IOR is not an accepting listener.** The shape this replaces is
# `[ -s x.ior ] && { sleep 0.5; }` — a fixed guess after a side effect — which
# CLAUDE.md names twice and which `81cc546` repaired on 2026-08-29 for one
# JacORB group after `ping(): io: Resource temporarily unavailable (os error
# 35)`. **That repair was scoped to the group that went red.** Swept 2026-08-31:
# eighteen sites had the shape and seventeen still did, including six against
# the same JacORB peer with the same 0.5s guess. *A sweep is scoped to a rule; a
# sweep that names a file will sweep that file.* This file is the rule's home so
# there is one of it.
#
# THE PROBE IS A TCP CONNECT AND NOTHING ELSE. `spike-dump --address` decodes the
# endpoint and stops — it does not dial — because a probe that dials is a caller,
# and CLAUDE.md records what that cost when it was tried on a fixture whose
# traffic is compared against recorded octets: 0 failures to 10 with a GIOP
# call, and still 6 with a bare connect. Nothing in this helper's callers is
# taped or counts connections (checked 2026-08-31: no recorder sits in
# `echo_server.py`'s or the JacORB fixtures' path), which is why a connect is
# allowed here and why that is stated rather than assumed.
#
# NOT FOR THE NAT FIXTURES, and the reason is the rule rather than an oversight:
# `spikes/nat/run.sh` and `spikes/nat/vm/run.sh` exist to measure an address that
# is deliberately NOT dialable from where the harness runs. An accept-probe from
# here would score those fixtures' whole purpose as a failure. They wait on the
# IOR and sleep between tries, which is correct for what they measure.
#
# *발행된 IOR은 accept하는 리스너가 아니다. 2026-08-29의 수리는 빨개진 그룹에만
# 범위가 맞춰져 있었다 — 열여덟 곳 중 열일곱이 그대로였고, 그중 여섯은 같은 피어에
# 같은 0.5초 추측이었다. **스윕의 범위는 규칙이지 파일이 아니다.** 탐침은 TCP
# 연결뿐이며, `--address`는 해독만 하고 걸지 않는다. NAT 픽스처는 제외한다 — 그
# 주소가 여기서 닿지 않는 것이 바로 그 픽스처가 재는 것이기 때문이다.*

wait_accepting() {
  local ior="$1"; shift
  local ready_log="" ready_pat="" deadline=30
  while [ $# -gt 0 ]; do
    case "$1" in
      --ready)    ready_log="$2"; ready_pat="$3"; shift 3 ;;
      --deadline) deadline="$2"; shift 2 ;;
      *) shift ;;
    esac
  done

  local root end addr
  root="${ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
  end=$(( $(date +%s) + deadline ))

  while [ "$(date +%s)" -lt "$end" ]; do
    # 1. the fixture has published something
    [ -s "$ior" ] || { sleep 0.1; continue; }
    # 2. its own readiness line, where it prints one — strictly later than the
    #    file for every fixture that does, which is why it is asked second
    if [ -n "$ready_pat" ]; then
      grep -E "$ready_pat" "$ready_log" >/dev/null 2>&1 || { sleep 0.1; continue; }
    fi
    # 3. the only one of the three about what the caller is about to do
    # `--address` prints a bare `host:port`; the older full dump prints
    # `endpoint host:port  object_key …`. Accept either, so a caller that was
    # parsing the long form keeps working and neither spelling becomes a second
    # place that knows how an endpoint is written.
    addr=$( (cd "$root" && cargo run -q --bin spike-dump -- --address "$ior") 2>/dev/null \
            | sed -n 's/^endpoint //; s/^\([0-9A-Za-z._:-]*:[0-9][0-9]*\).*/\1/p' | head -1)
    if [ -n "$addr" ] && (exec 3<>"/dev/tcp/${addr%:*}/${addr##*:}") 2>/dev/null; then
      exec 3<&- 2>/dev/null || true
      return 0
    fi
    sleep 0.1
  done
  return 1
}
