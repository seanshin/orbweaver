#!/usr/bin/env bash
# The one home for "which names must never appear in `cargo tree`".
#
# ── Why this is a file and not two greps ────────────────────────────────────
#
# This check existed twice — once in `spikes/run_checks.sh` and once in
# `.github/workflows/ci.yml` — and on 2026-08-27 the two had drifted to
# different patterns: CI matched `omniorb|jacorb|\btao\b` and the harness
# matched `omniorb|jacorb`. **The harness copy was missing the term that was
# about to matter**: D035 was approved that day with a TAO fixture as its
# second step, and the harness runs on the machine where that fixture is
# actually built, so it would have stayed green over a TAO dependency.
#
# Neither copy was wrong when written; they drifted because a rule restated in
# two places drifts on the next change, silently, as this project's own
# "where a fact lives" rule says. `spikes/build_c_peer.sh` holds a third check
# of the same boundary, by header-include shape rather than by crate name,
# which is why the gap survived being looked at more than once.
#
# So the pattern lives here, once, and both callers invoke this script. The
# divergence is not detectable any more; it is unrepresentable.
#
# ── What the boundary is ────────────────────────────────────────────────────
#
# CLAUDE.md, non-negotiable: omniORB, ACE/TAO and JacORB are LGPL/GPL/DOC and
# are **fixtures, never dependencies**. `NOTICE`'s "Not dependencies" section
# names the same three. Anything under `crates/` is original work written
# against the OMG specification, and `cargo tree` must stay free of them.
#
# ── Exit codes, and why there are three ─────────────────────────────────────
#
#   0  measured, and clean
#   1  measured, and a fixture name is present — the boundary is violated
#   3  NOT measured: `cargo tree` could not run
#
# 3 is separate from 1 on purpose. Until 2026-08-26 CI's copy could not tell
# them apart: a `cargo tree` that failed printed nothing, `grep` found nothing
# and exited 1, the `if` was false, and the step reported "cargo tree is free
# of ORB fixtures" — **the one rule this project calls non-negotiable had a
# gate reporting a boundary it had never measured.** An unmeasured check is a
# failure, never a pass.
set -uo pipefail
cd "$(dirname "$0")/.."

# `\btao\b` and `\bace\b`, not the bare letters: unanchored they match any
# crate whose name merely contains them, and a false red kills a gate as surely
# as a true one nobody reads. The word boundaries are checked by --self-test
# below rather than asserted here.
FIXTURE_CRATES='omniorb|jacorb|\btao\b|\bace\b'

case "${1:-}" in
  --pattern)
    printf '%s\n' "$FIXTURE_CRATES"
    exit 0
    ;;
  --self-test)
    # SYNTHESISE THE SUBJECT. A pattern that matches nothing over a clean tree
    # is indistinguishable from a pattern that cannot match, so before this
    # script's silence is allowed to mean anything it is shown catching one of
    # each name — and refusing a set of near-misses that a bare `tao`/`ace`
    # would have swallowed. The near-miss half is the load-bearing one: it is
    # what makes widening the pattern safe to do again.
    st_fail=0
    for s in "├── omniorb v4.3.4" "└── jacorb v3.9.0" "├── tao v0.16.0" "│   └── ace v8.0.7"; do
      grep -qiE "$FIXTURE_CRATES" <<<"$s" || { echo "self-test: MISSED a fixture name: $s"; st_fail=1; }
    done
    for s in "├── tokio v1.44.0" "├── trace-context v0.1" "├── staot v1.0" \
             "├── interface v0.2" "├── spacetime v3.1" "├── facet v0.9" "├── palace v0.1"; do
      grep -qiE "$FIXTURE_CRATES" <<<"$s" && { echo "self-test: FALSE POSITIVE on: $s"; st_fail=1; }
    done
    if [ "$st_fail" -ne 0 ]; then
      echo "self-test FAILED — this script's silence over a real tree means nothing"
      exit 2
    fi
    echo "self-test ok: 4 fixture names caught, 7 near-misses refused"
    exit 0
    ;;
  "") ;;
  *)
    echo "usage: licence_boundary.sh [--pattern | --self-test]" >&2
    exit 2
    ;;
esac

# Capture, read the producer's own status, then match with a herestring.
# `cargo tree | grep -q` is the form CLAUDE.md documents as lying in two
# independent ways — `grep -q` SIGPIPEs the producer, and under `pipefail` a
# failed producer reads as "no match". The harness swept 76 of these on
# 2026-08-25 and `ci.yml` was not in the sweep, because the sweep was scoped to
# a file when the rule is about a shape.
tree_out=$(cargo tree --workspace 2>&1); tree_rc=$?
if [ "$tree_rc" -ne 0 ]; then
  echo "UNMEASURED cargo tree --workspace did not run (exit $tree_rc)"
  tail -5 <<<"$tree_out"
  exit 3
fi

hits=$(grep -inE "$FIXTURE_CRATES" <<<"$tree_out" || true)
if [ -n "$hits" ]; then
  echo "VIOLATION an ORB fixture has become a dependency"
  head -5 <<<"$hits"
  exit 1
fi

# No "ok" prefix: the exit code is the verdict and each caller adds its own
# label, so printing one here produced `  ok   ok 72 line(s) read` in the
# harness. The text is the detail, not the judgement.
echo "$(grep -c . <<<"$tree_out") line(s) read, none of omniORB / ACE / TAO / JacORB present"
exit 0
