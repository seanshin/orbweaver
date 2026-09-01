#!/usr/bin/env bash
# Cell: servant × c. A first-party C peer that links no ORB calls a **Java**
# servant behind our ORB.
#
# ── What this row was waiting on, and what removed it ───────────────────────
#
# `java.manifest` said it plainly: *"as `servant/omniorb`: the Java half of the
# seam is what is missing, not the peer. A C peer calling a Java servant needs
# both this row's peer and that row's seam."* The peer has existed since
# 2026-08-26; the seam's Java half landed 2026-09-01, and the row's own sentence
# is what says this cell became possible rather than a judgement made here.
#
# ── What it buys, and what it does not ──────────────────────────────────────
#
# `spikes/bindings/AXES` decided this when the peer landed and refused to answer
# it by declaring:
#
#     `independent` refutes coding errors and does NOT satisfy clause 6.
#
# The peer shares **no code** with `crates/`, so an error on our side is not
# mirrored on the other — real evidence, and more than `self` can offer. It
# shares the same reading of the same specification by the same process, and *a
# convention both ends apply cannot be refuted by a round trip.* So this cell
# closes no clause. `binding_suite.sh` already implements that correctly: its
# clause 2 and clause 6 checks both require `observed` from a **foreign** peer.
#
# Worth having anyway: every other peer that has driven this servant is an ORB.
# This one is a program that speaks GIOP and nothing else, so the servant's
# answer is exercised through a stack with no ORB assumptions in it.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# `cargo test` exits 0 when a test returns early on an absent fixture, so the
# exit status alone would report an unmeasured cell as a pass. The test prints
# `UNMEASURED:` and this turns it into exit 2, which the suite counts as SKIPPED.
#
# *이 행이 기다리던 것은 피어가 아니라 seam의 자바 절반이었고, 그것이 착지했다.
# 사는 것은 AXES가 정해 두었다 — `independent`는 코딩 오류를 반증하지 clause 6을
# 충족하지 않는다. 그래도 값어치가 있다: 이 서번트를 몰아 본 다른 피어는 전부
# ORB이고, 이것은 GIOP만 말하는 프로그램이다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

[ -x "$ROOT/target/c_peer" ] || {
  echo "SKIPPED  the C peer is not built (target/c_peer) — run spikes/build_c_peer.sh."
  echo "SKIPPED  Unmeasured, not passing."
  exit 2
}

out=$(cargo test -q -p orbweaver-gen --test java_servant_wire \
        -- --exact a_c_peer_calls_a_java_servant --nocapture 2>&1); rc=$?

if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out" | grep -E "panicked at|assertion|did not read|left:|right:" | head -8
  exit 1
fi

if grep -q "UNMEASURED" <<<"$out"; then
  grep "UNMEASURED" <<<"$out" | sed 's/^/SKIPPED  /'
  exit 2
fi

# No `observed` line: this peer is `independent`, and the suite counts an order
# toward clause 2 only when a FOREIGN peer wrote it. Reporting one here would be
# claiming coverage the axis file says this peer cannot give.
printf 'note\ta C peer that links no ORB called a Java servant and decoded its answer\n'
printf 'note\t`independent` refutes coding errors and closes no clause — spikes/bindings/AXES, which decided that when the peer landed\n'
exit 0
