#!/usr/bin/env bash
# Cell: servant × omniorb. omniORB's own Python client drives the Python servant
# behind our ORB.
#
# ── It reports `observed`, and reported `claimed` until 2026-09-02 ──────────
#
# It used to say: *"there is no tap between them, so no byte of any request is
# ever inspected; the exchange is little-endian because omniORB writes its host's
# native order — a sound inference and still not a reading."* It also named its
# own repair — *"closing it is a tap in `python_servant_wire.rs`, of the shape
# `jacorb_calls_a_python_servant` already has"* — and deferred it with a reason
# that was good on the day: that test had landed hours earlier and changing it
# would have put a byte-identity oracle and an improvement in one commit where
# neither could be read.
#
# **The reason expired and the deferral outlived it.** The relay has been in that
# file since; the tap is now in front of omniORB too, and the order comes off
# §15.4.1's flag byte of the peer's own **requests** — in the servant direction
# the peer is the caller, so the requests are its writing.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# `cargo test` exits 0 when the test returns early on an absent fixture, so the
# exit status alone would report an unmeasured cell as a pass — `an unmeasured
# check is a failure, never a pass`. The test prints `UNMEASURED:` in that case
# and this script turns it into exit 2, which the suite counts as SKIPPED.
#
# *이 칸은 `observed`가 아니라 `claimed`을 보고한다 — 순서는 맞지만 아무도 읽지
# 않았기 때문이다. 건전한 추론이어도 판독은 아니다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

out=$(cargo test -q -p orbweaver-gen --test python_servant_wire \
        -- --exact omniorb_calls_a_python_servant --nocapture 2>&1); rc=$?

# The producer's status first.
if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out" | grep -E "panicked at|assertion|left:|right:|error" | head -8
  exit 1
fi

if grep -q "UNMEASURED" <<<"$out"; then
  grep "UNMEASURED" <<<"$out" | sed 's/^/SKIPPED  /'
  exit 2
fi

# The readings the test printed. An absent reading is a failure and not a quiet
# pass: the calls can complete while nothing is read off the wire, which is the
# side of that distinction this cell reported on until 2026-09-02.
wire=$(grep "read off the wire at" <<<"$out")
if [ -z "$wire" ]; then
  echo "FAIL	the calls completed and no order was read off the wire, so this cell"
  echo "FAIL	measured nothing it could not have claimed"
  exit 1
fi
while IFS= read -r line; do
  v=$(sed -n 's/.*read off the wire at \([0-9.]*\).*/\1/p' <<<"$line")
  o=$(sed -n 's/.*order=\([a-z]*\).*/\1/p' <<<"$line")
  [ -n "$v" ] && [ -n "$o" ] || { echo "FAIL	a reading names no version or order: $line"; exit 1; }
  printf 'observed\tgiop=%s\torder=%s\n' "$v" "$o"
done <<<"$wire"
printf 'note\tomniORB drove the Python servant and nothing about the servant'"'"'s language reached it\n'
exit 0
