#!/usr/bin/env bash
# Cell: servant × jacorb. JacORB's own client drives a **Java** servant behind
# our ORB, and the byte order is READ OFF §15.4.1's flag byte of what JacORB
# wrote.
#
# ── What this cell closes ───────────────────────────────────────────────────
#
# It is the one the Java servant direction was waiting on. `servant × omniorb`
# reports `claimed`: no tap sits between the peers and the little-endian order is
# inferred from the host. Here a recording tap sits in front of our server, so
# D032 §4's clause 6 — *a foreign peer was one end of a reading* — is met in the
# servant direction, which it was not for Java until now.
#
# **In the servant direction the peer's writing is in the REQUESTS.** We are the
# one answering, so reading the replies here would be reading our own order and
# reporting it as a foreign peer's — the strongest wrong claim this suite can
# make, and exactly what `claimed` exists to keep separate.
# `spikes/lib/tap_orders.sh` keeps the two readings in two functions rather than
# one with a flag, because the mistake being prevented is picking the wrong one.
#
# ── The fixture ─────────────────────────────────────────────────────────────
#
# `cargo test` exits 0 when a test returns early on an absent fixture, so the
# exit status alone would report an unmeasured cell as a pass — *an unmeasured
# check is a failure, never a pass*. The test prints `UNMEASURED:` and this
# turns it into exit 2, which the suite counts as SKIPPED.
#
# *자바 서번트 방향이 기다리던 칸이다. 서번트 방향에서 피어의 쓰기는 **요청**에
# 있다 — 여기서 답신을 읽으면 우리 순서를 외래 피어의 판독으로 보고하게 된다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

out=$(cargo test -q -p orbweaver-gen --test java_servant_wire \
        -- --exact jacorb_calls_a_java_servant --nocapture 2>&1); rc=$?

# The producer's status first: a test that could not run is not a clean cell.
if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out" | grep -E "panicked at|assertion|did not complete|FAIL|left:|right:" | head -8
  exit 1
fi

if grep -q "UNMEASURED" <<<"$out"; then
  grep "UNMEASURED" <<<"$out" | sed 's/^/SKIPPED  /'
  exit 2
fi

# The readings the test printed. Absent readings are a failure and not a quiet
# pass: the calls can complete while nothing is read off the wire, which is the
# whole distinction this cell exists to make.
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

grep "^note " <<<"$out" | sed 's/^note /note\t/' | head -2
printf 'note\tthe servant arrives as a Dispatch in a server the test binds, not as a second endpoint — a caller sent elsewhere would have been MOVED, a different row of D029 §6.1\n'
exit 0
