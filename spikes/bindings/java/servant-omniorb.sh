#!/usr/bin/env bash
# Cell: servant × omniorb. omniORB's own Python client drives a **Java** servant
# behind our ORB.
#
# ── Why this cell exists before `servant × self` ────────────────────────────
#
# `spikes/bindings/java.manifest` refused the cheap one first and said why: *"a
# self cell that existed while the foreign ones did not would report a seam we
# had never run against anybody else."* That ordering is honoured — this is a
# foreign peer, and `servant × self` follows it rather than preceding it.
#
# ── It reports `observed`, and reported `claimed` until 2026-09-01 ──────────
#
# It used to say: *"no tap sits between them, so no byte of any request is
# inspected; the exchange is little-endian because omniORB writes its host's
# native order — a sound inference and still not a reading."* That was honest
# and it was a gap the suite printed every run as
# `servant × little (claimed, never read)`.
#
# The tap is peer-agnostic — its own header says the version and codeset choice
# come from the ORBs and the log is what they did — and it was already sitting
# in front of JacORB one cell over. There was no reason left for the two cells
# to be different kinds of evidence, so this one reads too: the order comes off
# §15.4.1's flag byte of omniORB's own **requests**, because in the servant
# direction the peer is the caller and the requests are its writing.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# `cargo test` exits 0 when the test returns early on an absent fixture, so the
# exit status alone would report an unmeasured cell as a pass — *an unmeasured
# check is a failure, never a pass*. The test prints `UNMEASURED:` in that case
# and this turns it into exit 2, which the suite counts as SKIPPED.
#
# *외래 피어가 자바 서번트를 몬다. 매니페스트가 정한 순서를 지킨다 — 싼 self 칸을
# 먼저 두면 아무에게도 돌려보지 않은 seam을 보고하게 된다. 판독이 아니라 추론이므로
# `claimed`으로 적는다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

out=$(cargo test -q -p orbweaver-gen --test java_servant_wire \
        -- --exact omniorb_calls_a_java_servant --nocapture 2>&1); rc=$?

# The producer's status first.
if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out" | grep -E "panicked at|assertion|did not see|left:|right:|error" | head -8
  exit 1
fi

if grep -q "UNMEASURED" <<<"$out"; then
  grep "UNMEASURED" <<<"$out" | sed 's/^/SKIPPED  /'
  exit 2
fi

# The readings the test printed. Absent readings are a failure and not a quiet
# pass: the calls can complete while nothing is read off the wire, which is the
# distinction this cell reported on the wrong side of until 2026-09-01.
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
printf 'note\tomniORB narrowed to spike::Echo and called a Java object; nothing about the servant'"'"'s language reached it\n'
printf 'note\tthe servant arrives as a Dispatch in a server this test binds, not as a second endpoint — a caller sent elsewhere would have been MOVED, which is a different row of D029 §6.1\n'
exit 0
