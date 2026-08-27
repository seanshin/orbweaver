#!/usr/bin/env bash
# Reclaim what finished work is still holding / 끝난 작업이 붙들고 있는 것을 회수한다
#
# Parallel agent waves leave a worktree per batch. Landing merges the branch and
# nothing removes the checkout, so they accumulate: **70 mounted, 61 of them
# already merged** when this was written (2026-08-27), and a recorded incident
# of 45 worktrees holding 37 GB for a day before that. The cost is not only
# disk — a `grep -r` over the repository reads them, so a tree-wide scan reports
# other branches' defects as this tree's. That happened to this project's own
# early-exit gate on the day it was written, which is why the gate now uses
# `git ls-files` and why this script exists beside it.
#
# SAFETY. A worktree is removed only when ALL of these hold:
#   - it is under .claude/worktrees/ (never a hand-made worktree elsewhere)
#   - its branch is an ancestor of main — every commit on it is already landed
#   - it is not the current worktree, and not main's
#   - `git worktree remove` reports it clean, unless --force is given
#
# `--incremental` additionally drops `target/*/incremental`, which is a rebuild
# cache and not a build product. Opt-in: see the comment at that block.
# A branch that is NOT merged is left alone and printed. Losing an unlanded
# batch to a cleanup script is the one failure this must never have.
#
# 병합이 끝난 워크트리만, 그것도 main의 조상인 것만 제거한다. 착지하지 않은
# 브랜치는 손대지 않고 출력만 한다.
set -uo pipefail

ROOT=$(git rev-parse --show-toplevel) || exit 2
cd "$ROOT" || exit 2

DRY=1
FORCE=""
INCR=0
for a in "$@"; do
  case "$a" in
    --apply) DRY=0 ;;
    --force) FORCE="--force" ;;
    --incremental) INCR=1 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown argument: $a" >&2; exit 2 ;;
  esac
done

held_before=$(du -sk .claude/worktrees 2>/dev/null | cut -f1)
held_before=${held_before:-0}

echo "== what this repository is holding =="
du -sh target .claude/worktrees .git 2>/dev/null | sed 's/^/  /'
df -h . | tail -1 | sed 's/^/  /'
echo

# `git for-each-ref`, not `git branch --list`: a branch checked out in another
# worktree is printed with a `+` prefix, and stripping that with `tr` has
# silently mangled names here before.
# `mapfile` is bash 4; this project runs on macOS, which ships 3.2. A plain
# while-read over a here-string keeps the loop in the parent shell, which a
# pipeline would not — the counters below have to survive it.
rows=$(git worktree list --porcelain | awk '
  /^worktree /{w=$2} /^branch /{print w "\t" $2}')

removed=0 kept=0 skipped=0
while IFS=$'\t' read -r wt br; do
  [ -n "$wt" ] || continue
  short=${br#refs/heads/}

  [ "$wt" = "$ROOT" ] && continue
  case "$wt" in "$ROOT"/.claude/worktrees/*) ;; *)
    echo "  skip     $short — not under .claude/worktrees/"; skipped=$((skipped+1)); continue ;;
  esac
  if ! git merge-base --is-ancestor "$br" main 2>/dev/null; then
    echo "  KEEP     $short — not merged into main; its work would be lost"
    kept=$((kept+1)); continue
  fi

  if [ "$DRY" -eq 1 ]; then
    echo "  would rm $short"
    removed=$((removed+1))
    continue
  fi
  # shellcheck disable=SC2086 — $FORCE is a single optional flag
  if rm_out=$(git worktree remove $FORCE "$wt" 2>&1); then
    if git branch -d "$short" >/dev/null 2>&1 || git branch -D "$short" >/dev/null 2>&1; then :; fi
    echo "  removed  $short"
    removed=$((removed+1))
  else
    echo "  FAILED   $short — $rm_out"
    echo "           (dirty? re-run with --force if the work is genuinely landed)"
    kept=$((kept+1))
  fi
done <<EOF
$rows
EOF

# `target/*/incremental` is a developer's rebuild cache. It is regenerated on
# demand, nothing reads it across a clean checkout, and it is the single largest
# disposable thing here: **14 G of a 35 G `target`, measured 2026-08-27**. It is
# opt-in rather than automatic because dropping it costs the NEXT local build
# some time, and this script's default should never make someone's next command
# slower without being asked.
if [ "$INCR" -eq 1 ]; then
  incr_kb=$(du -sk target/*/incremental 2>/dev/null | awk '{s+=$1} END{print s+0}')
  if [ "${incr_kb:-0}" -eq 0 ]; then
    echo "  incremental: nothing to drop"
  elif [ "$DRY" -eq 1 ]; then
    echo "  would drop $((incr_kb / 1024)) MB of target/*/incremental"
  else
    rm -rf target/*/incremental
    echo "  dropped    $((incr_kb / 1024)) MB of target/*/incremental"
  fi
fi

git worktree prune
echo
if [ "$DRY" -eq 1 ]; then
  echo "== dry run: $removed reclaimable, $kept kept, $skipped skipped =="
  echo "   nothing was removed. Re-run with --apply."
else
  held_after=$(du -sk .claude/worktrees 2>/dev/null | cut -f1); held_after=${held_after:-0}
  freed=$(( (held_before - held_after) / 1024 ))
  echo "== removed $removed, kept $kept, skipped $skipped; freed ${freed} MB =="
  df -h . | tail -1 | sed 's/^/  /'
fi
[ "$kept" -gt 0 ] && echo "   KEEP lines above are unlanded work — land or delete them deliberately."
exit 0
