#!/usr/bin/env bash
# Cell: servant × c. A first-party C peer that links no ORB calls a **Python**
# servant behind our ORB.
#
# The Java sibling landed first and this shares its bound, which
# `spikes/bindings/AXES` decided when the peer landed and refused to answer by
# declaring:
#
#     `independent` refutes coding errors and does NOT satisfy clause 6.
#
# The peer shares **no code** with `crates/` — an error on our side is not
# mirrored on the other, which is real evidence and more than `self` can offer —
# and shares the same reading of the same specification by the same process, and
# *a convention both ends apply cannot be refuted by a round trip.* So this cell
# closes no clause, prints no `observed` line, and says why rather than leaving
# a reader to infer it. `binding_suite.sh` implements that correctly already:
# clause 2 and clause 6 both require `observed` from a **foreign** peer.
#
# Worth having: every other peer that has driven a Python servant here is an ORB.
# This one is a program that speaks GIOP and nothing else, so the servant's
# answer crosses a stack with no ORB assumptions in it.
#
# The servant arrives as a `Dispatch` in a server the test binds — not through
# `orbweaver-py-bridge --serve`, which binds its own listener and would make the
# servant an *endpoint*. A caller sent to a different address has been moved, and
# location and language are different rows of D029 §6.1.
#
# *자바 형제가 먼저 착지했고 한계도 같다 — `independent`는 코딩 오류를 반증하지
# clause 6을 충족하지 않는다. 그래도 값어치가 있다: 파이썬 서번트를 몰아 본 다른
# 피어는 전부 ORB다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

[ -x "$ROOT/target/c_peer" ] || {
  echo "SKIPPED  the C peer is not built (target/c_peer) — run spikes/build_c_peer.sh."
  echo "SKIPPED  Unmeasured, not passing."
  exit 2
}

out=$(cargo test -q -p orbweaver-gen --test a_python_servant_this_process_owns \
        -- --exact a_c_peer_calls_a_python_servant --nocapture 2>&1); rc=$?

if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out" | grep -E "panicked at|assertion|did not read|left:|right:" | head -8
  exit 1
fi

if grep -q "UNMEASURED" <<<"$out"; then
  grep "UNMEASURED" <<<"$out" | sed 's/^/SKIPPED  /'
  exit 2
fi

printf 'note\ta C peer that links no ORB called a Python servant and decoded its answer\n'
printf 'note\t`independent` refutes coding errors and closes no clause — spikes/bindings/AXES, which decided that when the peer landed\n'
exit 0
