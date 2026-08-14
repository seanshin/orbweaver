#!/usr/bin/env bash
# run.sh — a legacy estate through the whole path, in one pass.
#
# Everything Orbweaver has measured so far runs over contracts we wrote, one at
# a time, against fixtures we start. This drives a **set**: thirteen IDL files
# with the texture of code somebody maintained for fifteen years — a shared
# common.idl everyone includes, `#pragma prefix` on some files and not others,
# an interface with eleven operations, a file that only compiles because of a
# typedef three files away, and two contracts that would embarrass their author.
#
#   ./spikes/estate/run.sh          human-readable, one section per stage
#   ./spikes/estate/run.sh --tsv    one `stage<TAB>metric<TAB>value` row per fact
#
# The estate is `spikes/estate/idl/`. It is **not** in `corpus/`, on purpose:
# the corpus is a gate's fixture set and this is a consumer. Nothing here is
# allowed to become a gate's input.
#
# Exit status: 0 when every stage was measured and every assertion held, 1 when
# an assertion failed, 2 when the run could not happen at all. A stage that
# cannot be measured counts as a FAILURE and never as a skip.
#
# Harness rules this script is written to, each of which cost a phantom failure
# in this project already:
#   - every wait loop sleeps and is bounded by a deadline; none spins
#   - producers are captured to a file and matched afterwards, never piped into
#     `grep -q`, which SIGPIPEs upstream on its first match
#   - the one fixture is killed by a PID captured at launch, never by name
#   - no machine-wide lock is taken and none is needed: everything lives in a
#     private mktemp directory and the fixture is killed by its own PID, so a
#     concurrent `run_checks.sh` cannot collide with this and it cannot collide
#     with one
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 2
ROOT=$(pwd)
ESTATE="$ROOT/spikes/estate"
IDLDIR="$ESTATE/idl"

TSV=0
[ "${1:-}" = "--tsv" ] && TSV=1

fails=0
declare -a ROWS=()

bold() { [ "$TSV" = 1 ] || printf '\n\033[1m%s\033[0m\n' "$1"; }
say()  { [ "$TSV" = 1 ] || printf '  %s\n' "$1"; }
pass() { [ "$TSV" = 1 ] || printf '  ok   %s\n' "$1"; }
fail() { [ "$TSV" = 1 ] || printf '  FAIL %s\n' "$1"; fails=$((fails + 1)); }
note() { [ "$TSV" = 1 ] || printf '  ..   %s\n' "$1"; }
row()  { ROWS+=("$1	$2	$3"); }
# stdout for humans only: --tsv must emit rows and nothing else, or the thing
# reading it has to parse prose out of its input.
quiet() { if [ "$TSV" = 1 ]; then cat >/dev/null; else cat; fi; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1"; exit 2; }; }

need cargo
need python3

WORK=$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-estate.XXXXXX")
SERVANT_PID=""

# Kill the fixture by the PID captured at launch, and wait for it to be gone:
# signalling is asynchronous, so returning early would let the next thing race
# a dying process. Nothing is matched by pattern, so no other checkout's
# fixture can be caught by this.
cleanup() {
    if [ -n "$SERVANT_PID" ] && kill -0 "$SERVANT_PID" 2>/dev/null; then
        { kill "$SERVANT_PID"; wait "$SERVANT_PID"; } 2>/dev/null
        deadline=$((SECONDS + 5))
        while kill -0 "$SERVANT_PID" 2>/dev/null && [ $SECONDS -lt $deadline ]; do
            sleep 0.1
        done
    fi
    [ "${ESTATE_KEEP:-0}" = "1" ] || rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

[ "$TSV" = 1 ] || echo "workspace: $WORK"

# ── Build, before any stage is judged ────────────────────────────────────────
bold "building the binaries the path runs on"
if ! cargo build -q --bin sidl-validate --bin forge-pipeline --bin gen-corpus \
        --bin repository-ids --bin orbweaver-mcp-server 2>"$WORK/build.log"; then
    tail -20 "$WORK/build.log"
    echo "estate: FAIL — the workspace binaries do not build"
    exit 2
fi
say "sidl-validate, forge-pipeline, gen-corpus, repository-ids, orbweaver-mcp-server"

FILES=("$IDLDIR"/*.idl)
N=${#FILES[@]}
row estate files "$N"

# ── Stage 1: the estate as it is stored — one file at a time ─────────────────
bold "stage 1 — S4 over each file on its own, the way the estate is stored"
s1_ok=0
for f in "${FILES[@]}"; do
    if "$ROOT/target/debug/sidl-validate" "$f" >"$WORK/s1.$(basename "$f").log" 2>&1; then
        s1_ok=$((s1_ok + 1))
    fi
done
say "first-pass: $s1_ok/$N files accepted standing alone"
row s1-per-file accepted "$s1_ok"
row s1-per-file total "$N"
# This is a measurement of the front end, not of the estate: `orbweaver-idl`
# skips `#include` rather than resolving it, so any file naming a type declared
# elsewhere cannot be validated alone. Recorded as a number rather than as a
# failure, because it is the finding.
if [ "$s1_ok" -lt "$N" ]; then
    note "$((N - s1_ok)) file(s) reference a type declared in another file"
    note "the front end skips #include (crates/orbweaver-idl/src/lex.rs) — see the run record"
fi

# ── Stage 2: the conformance oracle, over each file on its own ───────────────
bold "stage 2 — omniidl over each file on its own"
if command -v omniidl >/dev/null 2>&1; then
    s2_ok=0
    for f in "${FILES[@]}"; do
        if (cd "$IDLDIR" && omniidl -b dump "$(basename "$f")" >/dev/null 2>"$WORK/s2.err"); then
            s2_ok=$((s2_ok + 1))
        else
            note "omniidl rejected $(basename "$f"): $(head -1 "$WORK/s2.err")"
        fi
    done
    say "first-pass: $s2_ok/$N files accepted by the oracle"
    row s2-oracle accepted "$s2_ok"
    row s2-oracle total "$N"
    if [ "$s2_ok" -eq "$N" ]; then
        pass "the estate is valid IDL by a compiler that is not ours"
    else
        fail "the oracle rejected $((N - s2_ok)) file(s) — the estate itself is wrong"
    fi
else
    fail "omniidl absent — the conformance oracle is an unmeasured check, not a pass"
    row s2-oracle accepted UNMEASURED
fi

# ── Stage 3: the amalgamation, which is the only multi-file path we have ─────
bold "stage 3 — one translation unit, in dependency order"
python3 "$ESTATE/amalgamate.py" "$IDLDIR" >"$WORK/ESTATE.sidl.idl" 2>"$WORK/amal.err"
if [ ! -s "$WORK/ESTATE.sidl.idl" ]; then
    fail "amalgamate.py produced nothing: $(head -3 "$WORK/amal.err")"
    echo "estate: FAIL — nothing downstream can be measured"
    exit 1
fi
python3 "$ESTATE/amalgamate.py" "$IDLDIR" --naive >"$WORK/ESTATE-naive.idl" 2>/dev/null
LINES=$(grep -c '' "$WORK/ESTATE.sidl.idl")
say "spliced $N files into $LINES lines"
row s3-splice lines "$LINES"
VAL=$("$ROOT/target/debug/sidl-validate" "$WORK/ESTATE.sidl.idl" 2>&1)
if [ $? -eq 0 ]; then
    pass "S4 accepts the whole estate as one unit: $(printf '%s' "$VAL" | tail -1)"
else
    fail "S4 rejected the amalgamation: $(printf '%s' "$VAL" | tail -3)"
fi
ADVICE=$(printf '%s' "$VAL" | tail -1 | grep -oE '[0-9]+ non-blocking' | cut -d' ' -f1)
row s3-splice advice "${ADVICE:-0}"

# The name is load-bearing and it should not be. `forge-pipeline --only s4`
# discovers `<id>.sidl.idl` and nothing else, so a legacy contract that no
# annotation stage has ever seen has to be named as if one had.
note "the file is named .sidl.idl because that is the only name S4 discovers"

# ── Stage 4: identity, against the compiler that owns it ─────────────────────
bold "stage 4 — repository ids, ours against omniidl"
if command -v omniidl >/dev/null 2>&1; then
    IDW="$WORK/ids"; mkdir -p "$IDW"
    "$ROOT/target/debug/repository-ids" "$WORK/ESTATE.sidl.idl" 2>/dev/null \
        | cut -f3 | sort -u >"$IDW/ours"
    "$ROOT/target/debug/repository-ids" "$WORK/ESTATE-naive.idl" 2>/dev/null \
        | cut -f3 | sort -u >"$IDW/naive"
    for f in "${FILES[@]}"; do
        b=$(basename "$f"); mkdir -p "$IDW/o-$b.d"
        omniidl -bpython -C"$IDW/o-$b.d" "$f" >/dev/null 2>&1
    done
    grep -rhoE '"IDL:[^"]*"' "$IDW"/o-*.d 2>/dev/null | tr -d '"' \
        | grep -v '^IDL:omg.org/' | sort -u >"$IDW/oracle"

    # A constant's id is not emitted by omniidl's python back end, so this
    # extraction cannot see one. Excluded from both sides by name rather than
    # silently: an unmeasured row must not be counted as agreement.
    grep -v '/MAX_BAYS:' "$IDW/ours" >"$IDW/ours.cmp"
    grep -v '/MAX_BAYS:' "$IDW/naive" >"$IDW/naive.cmp"
    ORACLE_N=$(grep -c '' "$IDW/oracle")
    say "$ORACLE_N ids from the oracle, over $N files compiled separately"
    row s4-identity oracle-ids "$ORACLE_N"

    if diff -q "$IDW/oracle" "$IDW/ours.cmp" >/dev/null 2>&1; then
        pass "every id of the spliced estate matches omniidl, prefixes and all"
        row s4-identity spliced-agrees yes
    else
        fail "the spliced estate disagrees with omniidl:"
        diff -u "$IDW/oracle" "$IDW/ours.cmp" | head -12 | sed 's/^/       /' | quiet
        row s4-identity spliced-agrees no
    fi

    # The drift, measured rather than described. A file-scope `#pragma prefix`
    # runs to the end of its FILE; after a naive splice there is one file, so
    # every unprefixed file silently inherits its predecessor's prefix.
    DRIFT=$(diff "$IDW/oracle" "$IDW/naive.cmp" 2>/dev/null | grep -c '^>')
    row s4-identity naive-drift "$DRIFT"
    if [ "$DRIFT" -gt 0 ]; then
        pass "the naive splice drifts $DRIFT id(s) — measured, and why amalgamate.py resets the prefix"
        diff "$IDW/oracle" "$IDW/naive.cmp" | grep '^>' | head -6 | sed 's/^/       /' | quiet
    else
        fail "the naive splice drifted nothing — the reset is now untested and may be dead"
    fi
else
    fail "omniidl absent — identity is an unmeasured check, not a pass"
    row s4-identity spliced-agrees UNMEASURED
fi

# ── Stage 5: S4 gate + S5 registration, exposure off ─────────────────────────
bold "stage 5 — S4 gate and S5 registration, every interface exposed=no"
mkdir -p "$WORK/out"
cp "$WORK/ESTATE.sidl.idl" "$WORK/out/ESTATE.sidl.idl"
PIPE=$("$ROOT/target/debug/forge-pipeline" --out "$WORK/out" --only s4 --register 2>&1)
PIPE_RC=$?
printf '%s\n' "$PIPE" | sed 's/^/  | /' | quiet
if [ $PIPE_RC -eq 0 ]; then
    pass "S4 gated the estate and S5 registered it"
else
    fail "the pipeline exited $PIPE_RC"
fi
WS="$WORK/out/exposure.todo.tsv"
if [ -s "$WS" ]; then
    EXPOSABLE=$(grep -c '^IDL:' "$WS")
    NOTEXPOSED=$(grep '^IDL:' "$WS" | grep -c 'exposed=no')
    row s5-register exposable "$EXPOSABLE"
    if [ "$EXPOSABLE" -eq "$NOTEXPOSED" ] && [ "$EXPOSABLE" -gt 0 ]; then
        pass "$EXPOSABLE exposable interface(s), every one exposed=no"
    else
        fail "the worksheet exposes something by default"
    fi
else
    fail "S5 left no exposure worksheet"
fi
# The pipeline's own attribution line, which the filename convention makes lie.
if printf '%s' "$PIPE" | grep -q 'gated 1 annotated file'; then
    note "the pipeline reports the estate as an ANNOTATED file — it carries no annotations"
    note "the only name --only s4 discovers is .sidl.idl, so the honesty line reports the name"
fi

# ── Stage 6: both generated halves ───────────────────────────────────────────
bold "stage 6 — client stubs and server skeletons for the whole estate"
# The same bytes under a second name. `gen-corpus` derives its module name from
# the file stem, so generating from `ESTATE.sidl.idl` would emit `f_ESTATE_sidl`
# — a module name carrying a pipeline-stage suffix that the contract has nothing
# to do with. The rename is the driver's, and it is the same class of finding as
# stage 5's: two tools disagree about what a legacy contract's filename means.
mkdir -p "$WORK/gen"
cp "$WORK/ESTATE.sidl.idl" "$WORK/gen/ESTATE.idl"
GEN=$("$ROOT/target/debug/gen-corpus" --out "$WORK/genout" --workspace "$ROOT" \
      "$WORK/gen/ESTATE.idl" 2>&1)
GEN_RC=$?
printf '%s\n' "$GEN" | sed 's/^/  | /' | quiet
EMITTED_FILE="$WORK/genout/src/f_ESTATE.rs"
SKIPS=$(printf '%s' "$GEN" | grep -c '^skipped ')
row s6-generate skipped "$SKIPS"
if [ $GEN_RC -eq 0 ] && [ -s "$EMITTED_FILE" ]; then
    GENLINES=$(grep -c '' "$EMITTED_FILE")
    row s6-generate lines "$GENLINES"
    pass "generated $GENLINES lines, $SKIPS skip(s)"
else
    fail "gen-corpus exited $GEN_RC and left no $EMITTED_FILE"
fi
# Both halves, for every interface the worksheet listed.
HALVES_OK=0
HALVES_N=0
while read -r id; do
    [ -n "$id" ] || continue
    iface=${id##*/}; iface=${iface%%:*}
    HALVES_N=$((HALVES_N + 1))
    if grep -q "pub trait ${iface}Servant" "$EMITTED_FILE" 2>/dev/null \
       && grep -q "pub struct ${iface}Client" "$EMITTED_FILE" 2>/dev/null; then
        HALVES_OK=$((HALVES_OK + 1))
    else
        fail "no client stub or no server skeleton for $iface"
    fi
done < <(grep '^IDL:' "$WS" 2>/dev/null | cut -f1)
row s6-generate both-halves "$HALVES_OK/$HALVES_N"
[ "$HALVES_OK" -eq "$HALVES_N" ] && [ "$HALVES_N" -gt 0 ] \
    && pass "$HALVES_OK/$HALVES_N registered interfaces have BOTH halves generated"

# The hand-written servant compiles into the generated crate, which lives
# outside the workspace: compiling it proves the stubs stand on the published
# crate surface alone. gen-corpus emits no servant on purpose — a generator
# that also wrote the program driving it would be grading itself.
cp "$ESTATE/servant.rs" "$WORK/genout/src/estate_servant.rs"
cat >>"$WORK/genout/Cargo.toml" <<EOF

# Appended by spikes/estate/run.sh.
[dependencies.orbweaver-object]
path = "$ROOT/crates/orbweaver-object"

[[bin]]
name = "estate-servant"
path = "src/estate_servant.rs"
EOF
if (cd "$WORK/genout" && cargo build -q --bin estate-servant 2>"$WORK/genbuild.log"); then
    pass "a hand-written servant compiles against the generated trait"
else
    tail -25 "$WORK/genbuild.log" | sed 's/^/  | /' | quiet
    fail "the servant does not compile against the generated trait"
fi

# ── Stage 7: the dry run, before anything is dialled ─────────────────────────
# Deliberately here in wall-clock order: --dry-run reads no IOR and opens no
# socket, and the point of the instrument is that an operator can sign an
# exposure off before the thing it protects is running. At this instant nothing
# is listening.
bold "stage 7 — the dry run an operator would actually read"
if [ -n "$SERVANT_PID" ]; then
    fail "something was already serving; the dry run is no longer a prediction"
fi
MCP="$ROOT/target/debug/orbweaver-mcp-server"
EXPOSE_ALL=()
while read -r id; do
    [ -n "$id" ] && EXPOSE_ALL+=(--expose "$id")
done < <(grep '^IDL:' "$WS" 2>/dev/null | cut -f1)
"$MCP" --idl "$WORK/out/ESTATE.sidl.idl" "${EXPOSE_ALL[@]}" --as ops-agent \
       --dry-run >"$WORK/dry-all.json" 2>"$WORK/dry-all.err"
if [ -s "$WORK/dry-all.json" ]; then
    SUM=$(python3 - "$WORK/dry-all.json" <<'PY'
import json, sys
from collections import Counter
r = json.load(open(sys.argv[1]))
ops = [o for i in r["interfaces"] for o in i["operations"]]
c = Counter(o["would"] for o in ops)
print(f"interfaces={len(r['interfaces'])} operations={len(ops)} "
      + " ".join(f"{k}={v}" for k, v in sorted(c.items())))
PY
)
    say "$SUM"
    ALLOWED=$(printf '%s' "$SUM" | grep -oE 'allow=[0-9]+' | cut -d= -f2)
    TOTALOPS=$(printf '%s' "$SUM" | grep -oE 'operations=[0-9]+' | cut -d= -f2)
    BYTES=$(wc -c <"$WORK/dry-all.json" | tr -d ' ')
    AUDIT=$(grep -c '^DRYRUN-' "$WORK/dry-all.err")
    row s7-dryrun operations "$TOTALOPS"
    row s7-dryrun allowed "$ALLOWED"
    row s7-dryrun bytes "$BYTES"
    row s7-dryrun audit-lines "$AUDIT"
    row s7-dryrun expose-flags "${#EXPOSE_ALL[@]}"
    say "the operator wrote ${#EXPOSE_ALL[@]} arguments; the report is $BYTES bytes and $AUDIT audit lines"
    if [ "$ALLOWED" = "$TOTALOPS" ]; then
        pass "measured: $ALLOWED of $TOTALOPS operations would be ALLOWED for a caller holding no scopes"
        note "the estate carries no ai_effect and no ai_authz, so approval and scope have nothing to key on"
        note "this is the finding, not a failure of the gate — see the run record"
        DESTRUCTIVE=$(python3 - "$WORK/dry-all.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
watch = {"SHUTDOWN", "purge", "_set_enabled", "void_invoice", "flush_queue", "cancel"}
hits = [f"{i['id'].split('/')[-1].split(':')[0]}.{o['operation']}"
        for i in r["interfaces"] for o in i["operations"]
        if o["operation"] in watch and o["would"] == "allow"]
print(", ".join(sorted(hits)))
PY
)
        note "allowed, among them: $DESTRUCTIVE"
    else
        note "some operations were not allowed; the estate may have gained annotations"
    fi
    if [ "$AUDIT" -eq "$TOTALOPS" ]; then
        pass "every question is audited under its own DRYRUN- token, one line per operation"
    else
        fail "the dry run audited $AUDIT of $TOTALOPS questions"
    fi
else
    fail "--dry-run produced no report"
fi

# ── Stage 8: one object of the estate, served over real TCP ──────────────────
bold "stage 8 — one interface of the estate, served on our POA"
IOR="$WORK/tracker.ior"
rm -f "$IOR"
"$WORK/genout/target/debug/estate-servant" "$IOR" >"$WORK/servant.out" 2>"$WORK/servant.log" &
SERVANT_PID=$!
# The wait sleeps and is deadline-bounded. A loop without a sleep finishes in
# microseconds and does not wait at all.
deadline=$((SECONDS + 25))
while [ ! -s "$IOR" ] && [ $SECONDS -lt $deadline ]; do
    kill -0 "$SERVANT_PID" 2>/dev/null || break
    sleep 0.2
done
if [ -s "$IOR" ]; then
    pass "the servant published an IOR: $(sed 's/^\(IOR:.\{24\}\).*/\1…/' "$IOR")"
    note "$(cat "$WORK/servant.log")"
    row s8-serve published yes
else
    fail "the servant did not publish an IOR within 25s"
    sed 's/^/  | /' "$WORK/servant.log" | quiet
    row s8-serve published no
fi

# ── Stage 9: an agent-shaped caller, twice, with two exposures ───────────────
bold "stage 9 — an agent reaches the estate through the MCP bridge"
if [ -s "$IOR" ]; then
    AGENT=$(python3 "$ESTATE/agent.py" "$IOR" "$WORK/out/ESTATE.sidl.idl" 2>&1)
    AGENT_RC=$?
    printf '%s\n' "$AGENT" | sed 's/^/  | /' | quiet
    row s9-agent rc "$AGENT_RC"
    if [ $AGENT_RC -eq 0 ]; then
        pass "both sessions completed: search -> describe -> invoke, through the guard"
    else
        fail "the agent path failed"
    fi
    case "$AGENT" in
        *"refused BY THE 2003 SERVANT"*)
            pass "session A: a destructive legacy operation was stopped by the contract, not the guard" ;;
        *) fail "session A did not show what stopped the destructive call" ;;
    esac
    case "$AGENT" in
        *"refused by the GUARD this time"*)
            pass "session B: the same operation is refused before anything is dialled" ;;
        *) fail "session B did not show the guard refusing" ;;
    esac
    # Not a failure — a finding, and one only an estate with pervasive
    # inheritance makes visible. Recorded as a row either way so a future run
    # that closes it is a visible change rather than a silence.
    case "$AGENT" in
        *"INHERITANCE GAP"*)
            note "describe_interface omits inherited operations the guard counts and the servant serves"
            row s9-agent describe-lists-inherited no ;;
        *) row s9-agent describe-lists-inherited yes ;;
    esac
else
    fail "no IOR to reach: the agent stage is unmeasured, which is a failure"
fi

# ── Verdict ─────────────────────────────────────────────────────────────────
if [ "$TSV" = 1 ]; then
    printf '%s\n' "${ROWS[@]}"
    exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
fi

bold "measured"
printf '%s\n' "${ROWS[@]}" | sed 's/^/  /'
echo
if [ "$fails" -ne 0 ]; then
    echo "estate: FAIL — $fails check(s)"
    exit 1
fi
echo "estate: PASS — every stage measured"
echo "  The pass is not the result. docs/pipeline-runs/2026-08-14-estate.md is."
exit 0
