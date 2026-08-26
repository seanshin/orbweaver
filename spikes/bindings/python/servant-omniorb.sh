#!/usr/bin/env bash
# Cell: servant × omniorb. omniORB's own Python client drives the Python servant
# behind our ORB.
#
# ── Why this cell reports `claimed` and not `observed` ──────────────────────
#
# `omniorb_calls_a_python_servant` dials the servant's IOR directly. There is no
# tap between them, so no byte of any request is ever inspected: the exchange is
# little-endian because omniORB writes its host's native order and our server
# replies in the request's order, and on a little-endian host that is what
# happens. **That is a sound inference and it is still not a reading.** The
# JacORB cell one row over reads the flag bit; this one does not, and the suite
# is told so rather than left to assume the two are the same kind of evidence.
#
# The honest consequence, which the coverage verdict prints every run: the
# servant direction's LITTLE-endian half is exercised by a foreign peer but has
# never been read off §15.4.1's flag byte. Closing it is a tap in
# `python_servant_wire.rs`, of the shape `jacorb_calls_a_python_servant` already
# has — deliberately not written by this batch, because that test landed hours
# earlier and changing it would put the migration's byte-identity oracle and the
# improvement in one commit where neither could be read.
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

printf 'claimed\tgiop=1.2\torder=little\tomniORB writes its host'"'"'s native order and our server replies in the request'"'"'s, so this exchange is little-endian on a little-endian host — inferred, with no tap between the peers and no flag byte read\n'
printf 'note\tomniORB drove the Python servant and nothing about the servant'"'"'s language reached it\n'
exit 0
