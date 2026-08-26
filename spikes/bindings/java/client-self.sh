#!/usr/bin/env bash
# Cell: client × self. The cross-implementation sweep — every value and every
# call the golden corpus declares, through generated Java and back, held to the
# Rust mapping in both byte orders.
#
# ── Why this is a cell and not the verdict ──────────────────────────────────
#
# Both ends are ours, so this cell can never satisfy D032 §4 clause 6, and
# `spikes/bindings/AXES` says so where the axis value is defined: *"`self` is a
# legitimate value and a legitimate measurement; it just cannot satisfy clause 6,
# and writing it down as a peer is what stops a loopback run from being quietly
# counted as one."* It reports **no wire observation at all**, which is honest:
# `_Rt.Loopback` has no socket in it.
#
# ── Why a wrapper rather than a bare `cargo test` ───────────────────────────
#
# The Python cell of this kind is a bare `cargo test` line in the manifest,
# because CPython is present wherever this project runs. A JDK is not, and the
# difference has to land on the right side of D010 §2: **absent is a counted
# SKIPPED naming its fixture, present-but-unmeasured is a failure.** A Rust test
# cannot exit 2, so the split lives here — the test prints `UNMEASURED` and
# returns when it finds no JDK, and this wrapper decides which of the two that
# is by looking for the JDK itself.
#
#   0  the sweep ran and every value and call agreed
#   1  it disagreed, or a JDK is present and the test still measured nothing
#   2  no JDK — unmeasured, not passing
#
# *양쪽 끝이 모두 우리 것이므로 이 칸은 절 6을 충족할 수 없다. JDK가 없으면 SKIPPED,
# 있는데도 재지 못했다면 실패 — Rust 테스트는 종료 코드 2를 낼 수 없으므로 그 구분이
# 여기에 산다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

JH="${ORBWEAVER_JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}"
if [ ! -x "$JH/bin/java" ] || [ ! -x "$JH/bin/javac" ]; then
  echo "SKIPPED  no JDK at $JH — set ORBWEAVER_JAVA_HOME, or install the same JDK 21"
  echo "SKIPPED  spikes/jacorb/setup.sh names. Unmeasured, not passing."
  exit 2
fi

out=$(ORBWEAVER_JAVA_HOME="$JH" cargo test -q -p orbweaver-gen --test java_target -- \
      --nocapture 2>&1); rc=$?
if [ "$rc" -ne 0 ]; then
  echo "FAIL	the Java target's oracle went red (exit $rc)"
  grep -E "disagree|panicked|assertion|javac refused" <<<"$out" | head -8
  exit 1
fi
# A JDK is here, so `UNMEASURED` means the test could not find what this script
# just found — which is a defect in the discovery, not a reason to skip.
if grep -q "UNMEASURED" <<<"$out"; then
  echo "FAIL	a JDK is present at $JH and the oracle still measured nothing:"
  grep "UNMEASURED" <<<"$out" | head -3
  exit 1
fi

while IFS= read -r line; do
  printf 'note\t%s\n' "${line#java target: }"
done <<<"$(grep "^java target:" <<<"$out")"
printf 'note\tno wire and no peer: _Rt.Loopback is a value-level round trip, which is why this cell reports no order\n'
exit 0
