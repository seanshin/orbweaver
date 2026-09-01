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
# ── Why it reports `claimed` and not `observed` ─────────────────────────────
#
# `omniorb_calls_a_java_servant` dials the servant's IOR directly. No tap sits
# between them, so no byte of any request is inspected: the exchange is
# little-endian because omniORB writes its host's native order and our server
# replies in the request's, and on a little-endian host that is what happens.
# **A sound inference is still not a reading.** The same sentence the Python
# `servant × omniorb` cell carries, for the same reason, and the suite is told
# rather than left to treat the two kinds of evidence as one.
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

printf 'claimed\tgiop=1.2\torder=little\tomniORB writes its host'"'"'s native order and our server replies in the request'"'"'s, so this exchange is little-endian on a little-endian host — inferred, with no tap between the peers and no flag byte read\n'
printf 'note\tomniORB narrowed to spike::Echo and called a Java object; nothing about the servant'"'"'s language reached it\n'
printf 'note\tthe servant arrives as a Dispatch in a server this test binds, not as a second endpoint — a caller sent elsewhere would have been MOVED, which is a different row of D029 §6.1\n'
exit 0
