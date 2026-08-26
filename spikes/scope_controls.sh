#!/usr/bin/env bash
# The negative controls for the two scope widenings landed 2026-08-26.
#
# `decision_status.py` used to glob markdown only, and `bilingual_drift.py`
# used to list six documents. Both were narrower than the facts they pin, and
# both went green over a real drift for days — the class CLAUDE.md names as
# *"a pin whose scope is narrower than its fact's is a pin that will go green
# over the drift."*
#
# Widening a checker's scope is also the classic way to make it unusable, and
# `bilingual_drift.py`'s own history is the warning: its first design flagged
# 34 sections of which 4 were real, and the threshold that quietened it removed
# exactly those 4. So neither widening is allowed to rest on being quiet at
# HEAD. This script runs each new rule against **the tree that held the
# defect**, and then confirms the quiet. It is the negative control, kept as a
# script rather than as a paragraph in a commit message, because a control that
# cannot be re-run is a claim.
#
# It is a control, not a gate: it is not in `run_checks.sh` and its exit code
# says whether the *controls* held, not whether the tree is clean.
#
# Usage:  ./spikes/scope_controls.sh
#
# 두 검사의 범위 확대에 대한 부정 대조군. 결함이 있던 트리에 대해 먼저 돌리고,
# 그 다음에 조용함을 확인한다 — 조용함만으로는 증거가 되지 않기 때문이다.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
fail=0
hr() { printf '\n\033[1m── %s\033[0m\n' "$1"; }
ok() { printf '  \033[32mok\033[0m %s\n' "$1"; }
bad() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; fail=$((fail + 1)); }

# The commit that repaired three of the four D007 restatements. Its parent is
# the tree in which all four were live, which is the control we want: today's
# rules, yesterday's defect. Recorded as a hash because the point of a control
# is that it does not move when the branch does.
REPAIR=b7836b525026573a80d20bb0d8fc07f3a9567432

# ── Control 1: decision_status.py over the tree that held the four ──────────
# `git archive` extracts a rev without touching the working tree, so this can
# run while anything else is in flight. `--root` points today's script at it.
hr "decision_status.py — the four D007 restatements, at ${REPAIR:0:7}^"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git archive "$REPAIR^" | tar -x -C "$tmp"
# The gate exits 1 on findings, which is the expected result here.
before=$(python3 spikes/decision_status.py --root "$tmp" || true)
printf '%s\n' "$before" | sed 's/^/  │ /'

# Four files, each named once. Capture first, then match with a herestring —
# never a pipe into `grep -q`, which SIGPIPEs its producer and, under
# `set -o pipefail`, reads a failed producer as "no match" (CLAUDE.md).
for f in crates/orbweaver-gen/Cargo.toml \
         crates/orbweaver-gen/src/python.rs \
         crates/orbweaver-gen/src/bin/py_bridge.rs \
         spikes/run_checks.sh; do
    if grep -q "DRIFT $f:.* D007 is APPROVED" <<<"$before"; then
        ok "found the restatement in $f"
    else
        bad "did NOT find the restatement in $f — the widening does not catch what it was built for"
    fi
done

# And the noise it makes on that tree, which is the number that decides whether
# a widened check is usable at all.
n_before=$(grep -c "^  DRIFT " <<<"$before" || true)
if [ "$n_before" -eq 4 ]; then
    ok "4 findings on that tree, all four of them the drift (0 false positives)"
else
    bad "$n_before findings on that tree, expected exactly the 4 — read them above"
fi

# ── Control 2: and it is quiet where it should be ──────────────────────────
# Only after the tree that had the defect. Quiet first is how a check gets
# tuned into measuring nothing.
hr "decision_status.py — the same rules at HEAD"
now=$(python3 spikes/decision_status.py || true)
printf '%s\n' "$now" | sed 's/^/  │ /'
n_gen=$(grep -c "DRIFT crates/orbweaver-gen" <<<"$now" || true)
if [ "$n_gen" -eq 0 ]; then
    ok "the three repaired files are quiet"
else
    bad "$n_gen finding(s) still in crates/orbweaver-gen"
fi
if grep -q "DRIFT spikes/run_checks.sh:.* D007 is APPROVED" <<<"$now"; then
    ok "run_checks.sh's restatement is still reported — it is another batch's \
footprint and was recommended, not edited"
else
    ok "run_checks.sh's restatement is gone — the recommendation was taken"
fi

# ── Control 2b: the markdown half was not weakened by the widening ─────────
# The widening changed shared code — `passages` now strips comment markers,
# because `//!` ends in `!` and was read as a sentence end. That function is
# what reads markdown too, so "the source scan works" is not enough: the ten
# stale markdown claims that made this gate exist must still be found, by the
# new script, on the tree that held them. Run the pre-widening script over the
# same tree and require the new one to find everything it did.
hr "decision_status.py — the markdown findings this gate was built on"
BEFORE_GATE=861b74e   # "harness: a decision's status is checked, not restated"
OLD_SCRIPT=aea8939    # the commit before the widening
tmd=$(mktemp -d)
git archive "$BEFORE_GATE^" | tar -x -C "$tmd"
# Line numbers are normalised out of the comparison, because `pinpoint` moved
# them on purpose — a finding reported at the paragraph's first line now names
# the line the status word is on, which is a different string for the same
# claim. Compare the claim; print the moves. The producers' own exit status is
# 1 (they found drift), which is the expected result and why each is `|| true` —
# and each is captured whole before being matched, never piped into `grep -q`.
new_raw=$(python3 spikes/decision_status.py --root "$tmd" || true)
git show "$OLD_SCRIPT:spikes/decision_status.py" > "$tmd/spikes/decision_status.py"
old_raw=$(python3 "$tmd/spikes/decision_status.py" || true)
rm -rf "$tmd"
claims() { grep "^  DRIFT " <<<"$1" | sed 's/:[0-9][0-9]*  */  /' | sort; }
new_md=$(claims "$new_raw")
old_md=$(claims "$old_raw")
n_old=$(grep -c . <<<"$old_md" || true)
n_new=$(grep -c . <<<"$new_md" || true)
lost=$(comm -23 <(printf '%s\n' "$old_md") <(printf '%s\n' "$new_md") || true)
printf '  │ pre-widening script: %s finding(s); widened script: %s\n' "$n_old" "$n_new"
if [ -z "${lost//[[:space:]]/}" ]; then
    ok "the widened script finds every claim the pre-widening one found ($n_old of $n_old)"
else
    bad "the widening LOST findings the old script had:"
    printf '%s\n' "$lost" | sed 's/^/      /'
fi
if [ "$n_old" -ge 10 ]; then
    ok "$n_old on that tree — the sweep that motivated this gate is still visible"
else
    bad "only $n_old findings on the pre-gate tree; the control is measuring the wrong commit"
fi

# ── Control 3: bilingual_drift.py over decisions whose halves moved apart ──
# Three sections in `docs/decisions/` have halves last edited 5+ days apart,
# every one of them the English re-measured and the Korean not touched since —
# the exact shape the report exists to name, in a directory the six-file DOCS
# list could not see. Named individually rather than counted, because "at least
# one finding" is satisfied by a finding that is not the one we widened for.
hr "bilingual_drift.py — decision sections whose halves moved apart"
drift=$(python3 spikes/bilingual_drift.py --days 3 || true)
printf '%s\n' "$drift" | sed 's/^/  │ /'
for want in "D010-what-remains-and-what-cannot-be-measured-here.md § 8. Recommended order" \
            "D006-plane-rule-tensor.md § What was verified" \
            "D006-plane-rule-tensor.md § 2. Why nothing catches it today"; do
    if grep -qF "$want" <<<"$drift"; then
        ok "reported: $want"
    else
        bad "not reported: $want — either it was repaired (check, then update \
this control) or the widening lost it"
    fi
done

# ── Control 4: and the halves that landed together are not reported ────────
# D032 and D033 landed 2026-08-26 with both halves in one commit (c616be9).
# A check that flagged them would be flagging the correct case, which is how a
# report becomes something people learn to skip.
hr "bilingual_drift.py — documents whose halves landed in one commit"
for d in D032 D033; do
    if grep -q "$d" <<<"$drift"; then
        bad "$d is reported, and both its halves landed in one commit"
    else
        ok "$d is not reported"
    fi
done

hr "verdict"
if [ "$fail" -eq 0 ]; then
    printf '  \033[32mall controls held\033[0m\n'
else
    printf '  \033[31m%d control(s) failed\033[0m\n' "$fail"
fi
exit "$fail"
