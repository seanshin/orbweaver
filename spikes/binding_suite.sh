#!/usr/bin/env bash
# The acceptance suite for a language binding. ONE suite, parameterised by
# language — D032 §4, item B3, and D033 §3.1.
#
#   ./spikes/binding_suite.sh --language python
#   ./spikes/binding_suite.sh --list                    # what languages exist
#   ./spikes/binding_suite.sh --language python --grid  # the grid, run nothing
#
# ── The rule this file is ────────────────────────────────────────────────────
#
# D032 §4: *"A language binding is accepted by passing a suite, not by being
# written. The suite is one suite, parameterised by language — never a copy."*
# The reason is measured rather than stylistic: a per-language copy of an
# instrument drifts exactly the way a per-language copy of a SENTENCE does, and
# this project has measured that four times in four shapes. Three targets means
# three instruments unless this exists.
#
# So: **there is no language name in this file.** Not in a `case`, not in a
# variable, not in a comment that a reader could mistake for configuration. A
# language is `spikes/bindings/<name>.manifest` plus whatever that manifest
# names, and the axis values are `spikes/bindings/AXES`. If a language ever
# needs a special case here, that special case is a **finding about the seam**
# and gets reported as one — never a `case` arm.
#
# ── The six clauses are not six checks ───────────────────────────────────────
#
# The derivation is in `spikes/bindings/AXES` and is not restated here. In one
# line: clauses 3/4/5 are language-scoped checks, clause 1 is one measurement
# ranged over a (direction × peer) grid, and clauses 2 and 6 are **coverage
# requirements over that grid** rather than checks of their own. A suite with a
# "both byte orders" line would print `ok` for Python today off a loopback that
# has no peer in it and, in the client direction, no wire either.
#
# ── What this prints, and what it refuses to print ───────────────────────────
#
# No score. No percentage. No "N of M languages", and no "N of M cells". `A
# floor is not a figure` and a completion percentage is its worst form: it moves
# when a cell is added and it is *wrong the moment a gap is found* rather than
# closed, which is what finding a gap is. The verdict names what is unmeasured.
#
# A cell nobody ran is a counted SKIPPED naming what it waits on — never absent
# and never `ok` (D010 §2). The waiting sentence is DERIVED from the grid rather
# than typed per cell, because a hand-typed reason per cell would be one more
# sentence with no home; a manifest may add a real one with `waits`.
#
# ── An order that was read, and an order that was assumed ────────────────────
#
# A cell reports `observed` for a byte order it read out of GIOP §15.4.1's flag
# byte of what the peer actually wrote, and `claimed` for one it asserted from
# the peer's language or its host. **Only `observed` counts toward clause 2.**
# That distinction is not pedantry: it is the difference between the JacORB
# servant leg, which reads the flag bit on every request, and the omniORB legs,
# which are little-endian because the host is and have never had a byte read.
#
# ── Exit ─────────────────────────────────────────────────────────────────────
#
#   0  every cell that ran is ok, and every allowed gap is named
#   1  a cell went red, or a manifest is malformed, or an axis name is bad
#   2  the language has no manifest, or usage
#
# A SKIPPED cell does not make this exit 1: it is unmeasured, which the verdict
# says out loud. What DOES exit 1 is a cell that was supposed to run and could
# not, because `an unmeasured check is a failure, never a pass` applies to a
# fixture the manifest claimed was there.
#
# ── Harness rules honoured here ──────────────────────────────────────────────
#
# Every producer's exit status is read before anything it printed. Nothing is
# piped into `grep -q` — output is captured and matched with a herestring —
# because such a pipeline lies in two independent ways (SIGPIPE, and `pipefail`
# turning a failed producer into "no match"). No wait loop without a sleep.
#
# *하나의 스위트, 언어로 매개변수화. **이 파일에는 언어 이름이 없다.** 절 2와 6은
# 검사가 아니라 격자에 대한 커버리지 요구이며, 바이트 순서는 §15.4.1 플래그 바이트에서
# 읽은 것만 센다 — 피어의 언어에서 가정한 것은 `claimed`로 따로 적힌다. 점수도,
# 백분율도 없다. 판정은 재지 못한 것을 이름으로 부른다.*
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

BDIR="$ROOT/spikes/bindings"
AXES="$BDIR/AXES"

LANGUAGE=""
GRID_ONLY=0
LIST=0
RAW=0
ONLY=""

while [ $# -gt 0 ]; do
  case "$1" in
    --language) LANGUAGE="${2:-}"; shift 2 ;;
    --list)     LIST=1; shift ;;
    --raw)      RAW=1; shift ;;    # with --list: just the names, one per line
    --grid)     GRID_ONLY=1; shift ;;
    --only)     ONLY="${2:-}"; shift 2 ;;   # <direction>/<peer>, for one cell
    -h|--help)  sed -n '2,60p' "$0"; exit 0 ;;
    *) echo "usage: $0 --language <name> [--grid] [--only <direction>/<peer>]"
       echo "       $0 --list"
       exit 2 ;;
  esac
done

FAILS=0
SKIPS=0
ok()   { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
info() { echo "  info $*"; }
skipped() { echo "  SKIPPED  $*"; SKIPS=$((SKIPS+1)); }

# ── the axes, read from their one home ───────────────────────────────────────
# Read once and KEEP WHAT IT SAID. A second read to produce a diagnostic can
# succeed where the first failed, and then the failure has no explanation in the
# transcript.
if [ ! -r "$AXES" ]; then
  echo "  FAIL $AXES is unreadable, so the suite does not know its own axes."
  echo "       Every cell below would be an unvalidated claim, and an unvalidated"
  echo "       claim is an unmeasured check, which is a failure and never a pass."
  exit 1
fi
axis_values() { awk -F'\t' -v a="$1" '$1==a && $0 !~ /^#/ {print $2}' "$AXES"; }
axis_prop()   { awk -F'\t' -v a="$1" -v v="$2" '$1==a && $2==v && $0 !~ /^#/ {print $3}' "$AXES"; }

DIRECTIONS=$(axis_values direction)
PEERS=$(axis_values peer)
ORDERS=$(axis_values order)
GIOPS=$(axis_values giop)
if [ -z "$DIRECTIONS" ] || [ -z "$PEERS" ] || [ -z "$ORDERS" ]; then
  fail "$AXES names no direction, peer or order — the grid would be empty and an empty grid prints as green"
  exit 1
fi

# ── --list ───────────────────────────────────────────────────────────────────
if [ "$LIST" = 1 ]; then
  # `--raw`: names only, for a caller that enumerates. A caller that globbed
  # `spikes/bindings/*.manifest` itself would be a second place that knows the
  # layout, and the day the layout changed one of the two would go quiet.
  if [ "$RAW" = 1 ]; then
    found=0
    for m in "$BDIR"/*.manifest; do
      [ -e "$m" ] || continue
      basename "$m" .manifest
      found=1
    done
    # An empty enumeration is not a quiet success: a harness looping over
    # nothing prints no failures, which reads exactly like passing.
    [ "$found" = 1 ] || { echo "binding_suite: no manifest in $BDIR" >&2; exit 1; }
    exit 0
  fi
  echo "languages with a manifest in $BDIR:"
  found=0
  for m in "$BDIR"/*.manifest; do
    [ -e "$m" ] || continue
    b=$(basename "$m" .manifest)
    echo "  $b"
    found=1
  done
  [ "$found" = 0 ] && echo "  (none)"
  echo
  echo "axes (from $AXES):"
  echo "  direction: $(tr '\n' ' ' <<<"$DIRECTIONS")"
  echo "  peer:      $(tr '\n' ' ' <<<"$PEERS")"
  echo "  order:     $(tr '\n' ' ' <<<"$ORDERS")"
  echo "  giop:      $(tr '\n' ' ' <<<"$GIOPS")"
  exit 0
fi

[ -n "$LANGUAGE" ] || { echo "usage: $0 --language <name> | --list"; exit 2; }

MANIFEST="$BDIR/$LANGUAGE.manifest"
if [ ! -r "$MANIFEST" ]; then
  # Not a failure: a language with no manifest is a language nobody has claimed
  # is a target, which is a different thing from one that failed. Exit 2 so a
  # harness group counts it as a skip rather than red.
  echo "  SKIPPED  no manifest at $MANIFEST, so \"$LANGUAGE\" is not a binding this suite can run."
  echo "           To commission one, a language supplies: a manifest naming a runner per"
  echo "           (direction, peer) cell it claims, a \`waits\` line for each it does not,"
  echo "           and a command for each of clauses 3, 4 and 5. See spikes/bindings/AXES"
  echo "           for what the cells are and $BDIR for an example."
  exit 2
fi

mf() { awk -F'\t' -v k="$1" '$1==k && $0 !~ /^#/' "$MANIFEST"; }

echo "acceptance suite: $LANGUAGE"
echo "  manifest: $MANIFEST"
echo "  axes:     $AXES"
echo

# ── the manifest's own names are validated before anything runs ──────────────
# The `bears_on` lesson, one axis over: a tag naming something the owning
# document does not have is a FAILURE naming the group and the bad name. A
# manifest row for a peer that does not exist would otherwise define a cell the
# grid never visits, so the row would sit there measuring nothing and reading as
# coverage.
manifest_names_ok=1
while IFS=$'\t' read -r kind d p rest; do
  [ -n "${kind:-}" ] || continue
  if ! grep -qx -- "${d:-}" <<<"$DIRECTIONS"; then
    fail "$MANIFEST: \"$kind\" row names direction \"${d:-}\", which $AXES does not have: $(tr '\n' ' ' <<<"$DIRECTIONS")"
    manifest_names_ok=0
  fi
  if ! grep -qx -- "${p:-}" <<<"$PEERS"; then
    fail "$MANIFEST: \"$kind\" row names peer \"${p:-}\", which $AXES does not have: $(tr '\n' ' ' <<<"$PEERS")"
    manifest_names_ok=0
  fi
done <<<"$(mf cell; mf waits)"
[ "$manifest_names_ok" = 1 ] || { echo; echo "$LANGUAGE: FAIL — the manifest names an axis value that does not exist"; exit 1; }

# ── the grid ─────────────────────────────────────────────────────────────────
# OBS collects `<direction>\t<kind>\t<peerprop>\t<giop>\t<order>` for every
# observation any cell reported. The coverage verdict is computed from it and
# from nothing else — in particular not from which cells exist, because a cell
# existing is not a measurement.
OBS=""
CELLS_RUN=0

run_cell() {
  local d="$1" p="$2" cmd="$3" prop="$4"
  local out rc
  out=$(eval "$cmd" 2>&1); rc=$?
  CELLS_RUN=$((CELLS_RUN+1))

  # The producer's status FIRST. Everything after is about what it printed, and
  # what it printed is only evidence if it ran.
  case "$rc" in
    0) ;;
    2) skipped "$d/$p — the cell reported its fixture absent; unmeasured, not passing"
       while IFS= read -r l; do
         case "$l" in *SKIPPED*|*UNMEASURED*) echo "           ${l#*SKIPPED}" ;; esac
       done <<<"$out"
       echo "           runner: $cmd"
       return 0 ;;
    *) fail "$d/$p exited $rc"
       # A bounded extract, and if it matched nothing say so rather than print
       # blank lines: six lines containing no information is what this project
       # already paid for once.
       # What a diagnostic print owes when the failure is not the shape it
       # expected: never blank lines. An empty producer and a producer whose
       # output did not match are different facts and are said differently.
       ex=$(grep -E "FAIL|panicked at|assertion|left:|right:|Error|error:" <<<"$out" | head -8)
       if [ -n "$ex" ]; then sed 's/^/       /' <<<"$ex"
       elif [ -z "${out//[[:space:]]/}" ]; then
         echo "       the cell printed NOTHING at all, so its exit status is the only evidence"
         echo "       there is. Its runner was: $cmd"
       else
         echo "       (nothing in the cell's output matched the extract; last lines follow)"
         tail -4 <<<"$out" | sed 's/^/       /'
       fi
       return 1 ;;
  esac

  # Observations. `observed` was read off the wire; `claimed` was not.
  local nobs=0 ncl=0
  while IFS=$'\t' read -r kind f1 f2 f3; do
    case "$kind" in
      observed|claimed)
        local g="${f1#giop=}" o="${f2#order=}"
        if ! grep -qx -- "$o" <<<"$ORDERS"; then
          fail "$d/$p reported order \"$o\", which $AXES does not have"
          return 1
        fi
        if ! grep -qx -- "$g" <<<"$GIOPS"; then
          fail "$d/$p reported GIOP \"$g\", which $AXES does not have"
          return 1
        fi
        OBS="${OBS}${d}	${kind}	${prop}	${g}	${o}
"
        [ "$kind" = observed ] && nobs=$((nobs+1)) || ncl=$((ncl+1))
        [ "$kind" = claimed ] && info "$d/$p claims giop=$g order=$o but did not read it off the wire: ${f3:-no reason given}"
        ;;
      note) info "$d/$p: $f1" ;;
    esac
  done <<<"$out"

  if [ "$nobs" -gt 0 ]; then
    ok "$d/$p — $nobs order/version reading(s) off the wire, peer is $prop"
  elif [ "$ncl" -gt 0 ]; then
    ok "$d/$p — ran; $ncl claimed reading(s), none read off the wire"
  else
    ok "$d/$p — ran; it reports no wire observation at all"
  fi
  return 0
}

for d in $DIRECTIONS; do
  for p in $PEERS; do
    [ -z "$ONLY" ] || [ "$ONLY" = "$d/$p" ] || continue
    prop=$(axis_prop peer "$p")
    cmd=$(mf cell | awk -F'\t' -v d="$d" -v p="$p" '$2==d && $3==p {print $4}' | head -1)
    if [ "$GRID_ONLY" = 1 ]; then
      if [ -n "$cmd" ]; then echo "  cell     $d/$p ($prop) -> $cmd"
      else echo "  no cell  $d/$p ($prop)"; fi
      continue
    fi
    if [ -n "$cmd" ]; then
      run_cell "$d" "$p" "$cmd" "$prop"
    else
      # Derived, not typed. The manifest may add a real reason with `waits`.
      why=$(mf waits | awk -F'\t' -v d="$d" -v p="$p" '$2==d && $3==p {print $4}' | head -1)
      skipped "$d/$p — no runner supplied for this cell (peer is $prop)"
      [ -n "$why" ] && echo "           waits on: $why"
      echo "           to close it, $LANGUAGE's manifest gains: cell<TAB>$d<TAB>$p<TAB><command>"
    fi
  done
done

if [ "$GRID_ONLY" = 1 ]; then
  echo
  echo "(--grid: nothing was run, so nothing is claimed)"
  exit 0
fi

# ── the language-scoped clauses ──────────────────────────────────────────────
# D032 §4's 3, 4 and 5: no peer and no wire in any of them, so no grid.
echo
while IFS=$'\t' read -r _ name cmd; do
  [ -n "${name:-}" ] || continue
  out=$(eval "$cmd" 2>&1); rc=$?
  case "$rc" in
    0) n=$(grep -c '^note' <<<"$out")
       [ "$n" -gt 0 ] && ok "clause \"$name\" — met ($n note(s))" || ok "clause \"$name\" — met"
       while IFS= read -r l; do case "$l" in note*) echo "         ${l#note	}" ;; esac; done <<<"$out" ;;
    2) skipped "clause \"$name\" — the check reported its fixture absent; unmeasured, not passing"
       echo "           runner: $cmd" ;;
    *) fail "clause \"$name\" exited $rc"
       ex=$(grep -E "FAIL|panicked at|assertion|error" <<<"$out" | head -8)
       if [ -n "$ex" ]; then sed 's/^/       /' <<<"$ex"
       else tail -4 <<<"$out" | sed 's/^/       /'; fi ;;
  esac
done <<<"$(mf clause)"

# ── coverage: clauses 2 and 6, computed over what the cells reported ─────────
# Not over which cells exist. A cell existing is not a measurement, and the
# whole reason these are coverage requirements rather than checks is that a
# check would go green off the wrong cell.
echo
echo "  coverage — clause 2 (both byte orders) and clause 6 (a foreign peer), per direction:"
foreign_seen() {   # direction order -> 0 when some FOREIGN peer OBSERVED that order
  grep -q "^$1	observed	foreign	[^	]*	$2$" <<<"$OBS"
}
any_seen() {       # direction order kind -> 0 when any peer of any kind reported it
  awk -F'\t' -v d="$1" -v o="$2" -v k="$3" '$1==d && $2==k && $5==o {found=1} END{exit !found}' <<<"$OBS"
}
UNMET=""
for d in $DIRECTIONS; do
  had_foreign=$(awk -F'\t' -v d="$d" '$1==d && $2=="observed" && $3=="foreign" {f=1} END{print f+0}' <<<"$OBS")
  for o in $ORDERS; do
    if foreign_seen "$d" "$o"; then
      echo "    ok        $d × $o — read off the wire from a foreign peer"
    elif any_seen "$d" "$o" claimed; then
      echo "    UNMEASURED $d × $o — a foreign peer exercised it, but the order was never read"
      echo "                off §15.4.1's flag byte; it is claimed from the peer's host or language"
      UNMET="${UNMET}$d × $o (claimed, never read)
"
    elif any_seen "$d" "$o" observed; then
      echo "    UNMEASURED $d × $o — observed, but only with a peer that is ours; clause 6 unmet"
      UNMET="${UNMET}$d × $o (only against ourselves)
"
    else
      echo "    UNMEASURED $d × $o — no cell reported this order at all"
      UNMET="${UNMET}$d × $o (nothing reported it)
"
    fi
  done
  [ "$had_foreign" = 1 ] \
    && echo "    ok        $d — clause 6: a foreign peer was one end of a reading" \
    || { echo "    UNMEASURED $d — clause 6: no foreign peer reading in this direction"
         UNMET="${UNMET}$d clause 6 (no foreign peer read)
"; }
done

echo
echo "  GIOP versions reached, per direction — read off the wire only, for the same"
echo "  reason clause 2 counts only what was read: a version inferred from a peer's"
echo "  default is the same kind of not-a-measurement as an order inferred from its host."
for d in $DIRECTIONS; do
  seen=$(awk -F'\t' -v d="$d" '$1==d && $2=="observed" {print $4}' <<<"$OBS" | sort -u | tr '\n' ' ')
  said=$(awk -F'\t' -v d="$d" '$1==d && $2=="claimed" {print $4}' <<<"$OBS" | sort -u | tr '\n' ' ')
  miss=""
  for g in $GIOPS; do
    grep -q "^$d	observed	[a-z]*	$g	" <<<"$OBS" || miss="$miss $g"
  done
  echo "    $d: read[${seen% }] claimed-only[${said% }] neither[${miss# }]"
  [ -n "$miss" ] && UNMET="${UNMET}$d GIOP${miss} (no cell read it off the wire)
"
done

# ── verdict ──────────────────────────────────────────────────────────────────
# It names what is unmeasured. It does not count what is measured, and there is
# no number here that prose could quote as progress.
echo
echo "$LANGUAGE: cells run $CELLS_RUN, skipped $SKIPS, red $FAILS"
if [ -n "$UNMET" ]; then
  echo "$LANGUAGE: UNMEASURED —"
  sed '/^$/d;s/^/  - /' <<<"$UNMET"
fi
if [ "$FAILS" -eq 0 ]; then
  echo "binding suite ($LANGUAGE): PASS over what ran; the lines above are what it did not measure"
  exit 0
fi
echo "binding suite ($LANGUAGE): FAIL — $FAILS red"
exit 1
