#!/usr/bin/env bash
# What can be checked about the R7 container probe on a machine with no docker.
#
# `spikes/nat/run.sh` needs an engine and this machine has none, so the probe
# itself is unmeasured here — `spikes/nat_rewrite.sh` counts that as a skip.
# What does NOT need an engine is whether the files the probe is made of still
# say what they were repaired to say (docs/PLAN-NAT-PROBE.md §3, lane B):
#
#   1. the committed `.dockerignore` at the repository root is exactly what
#      run.sh's derivation produces from `.gitignore` — the function is LIFTED
#      out of run.sh by its marker lines and run here, never restated, so
#      there is one derivation and this asks whether its output was committed;
#   2. every path the Dockerfile `COPY`s is in the context that ignore file
#      leaves: tracked by git, or re-included by name in the derivation —
#      the Dockerfile and the derivation each name `target/debug/spike-nat`,
#      and this is what keeps those two namings from drifting apart;
#   3. the Dockerfile compiles nothing: no line matches `cargo|rust:`;
#   4. run.sh parses (`bash -n`);
#   5. compose.yaml's `networks:` block is byte-identical to the one at HEAD.
#      The probe's claim is about routing and that block IS the routing: two
#      networks the daemon isolates, so the servant's own address is genuinely
#      unreachable from the client. A base-image or compose change that
#      altered it would turn the `naive` case green for the wrong reason.
#      `docker compose config` cannot run here; this is the check that can.
#
# Exit 0 when all five hold, 1 when any does not. Its verdict is about the
# files; it says nothing about whether the probe runs, which only a host with
# docker can say (the header of run.sh says what that host should expect).
#
# Negative controls, run before this landed and transcribed in its commit:
# a `cargo` line added to the Dockerfile → 1; a line appended to the committed
# `.dockerignore` → 1; the `begin` marker removed from run.sh → 1, because a
# lift that lifts nothing must refuse rather than compare nothing to nothing.
set -uo pipefail
cd "$(dirname "$0")"
NAT=$(pwd)
ROOT=$(cd ../.. && pwd)

fails=0
ok() { printf '  ok   %s\n' "$1"; }
fail() {
  printf '  FAIL %s\n' "$1"
  fails=$((fails + 1))
}

# An explicit template: `mktemp -t PREFIX` on GNU returns nothing.
work=$(mktemp -d "${TMPDIR:-/tmp}/orbweaver-nat-local.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT

# ── 1. the committed .dockerignore is the derivation's output ────────────────
# Lifted, not restated: the bytes between the two markers in run.sh are the
# function. An awk whose anchors have moved prints nothing, and `eval` of
# nothing succeeds — so the lift is checked before it is trusted (an empty
# control script exits 0 too; that is the lesson behind this line).
lifted=$(awk '/^# ── derive_dockerignore begin/{p=1} /^# ── derive_dockerignore end/{p=0} p' "$NAT/run.sh")
if [ -z "$lifted" ]; then
  fail "the derive_dockerignore markers in run.sh lifted nothing — this check cannot derive, so it cannot compare"
else
  eval "$lifted"
  if ! declare -F derive_dockerignore >/dev/null; then
    fail "the lifted text from run.sh does not define derive_dockerignore"
  elif ! derive_dockerignore "$ROOT/.gitignore" >"$work/derived" 2>"$work/derive.err"; then
    fail "derive_dockerignore failed over .gitignore: $(cat "$work/derive.err")"
  elif [ ! -s "$work/derived" ]; then
    fail "derive_dockerignore produced an empty file over .gitignore"
  elif [ ! -f "$ROOT/.dockerignore" ]; then
    fail "no .dockerignore at the repository root; run.sh derives one — commit its output"
  elif ! cmp -s "$work/derived" "$ROOT/.dockerignore"; then
    fail ".dockerignore differs from what run.sh derives from .gitignore (derived < > committed):"
    diff "$work/derived" "$ROOT/.dockerignore" | sed 's/^/         /'
  else
    ok ".dockerignore is the derivation's output ($(grep -c . "$work/derived") lines, $(grep -c '^!' "$work/derived") negations)"
  fi
fi

# ── 2. what the Dockerfile copies is in the context ──────────────────────────
# `COPY src… dst`: every src but the last word. A source the ignore file drops
# is a build that fails at COPY on the runner and nowhere else; a source it
# re-includes by name is the one exception the derivation carries, and this is
# where the Dockerfile's spelling and the derivation's are held together.
copy_missing=""
copy_seen=0
while IFS= read -r src; do
  copy_seen=$((copy_seen + 1))
  if git -C "$ROOT" ls-files --error-unmatch -- "$src" >/dev/null 2>&1; then
    continue
  fi
  if [ -s "$work/derived" ] && grep -qxF -- "!$src" "$work/derived"; then
    continue
  fi
  copy_missing="$copy_missing $src"
done < <(awk '$1 == "COPY" { for (i = 2; i < NF; i++) if ($i !~ /^--/) print $i }' "$NAT/Dockerfile")
if [ "$copy_seen" -eq 0 ]; then
  fail "the Dockerfile has no COPY line — it must copy spike-nat in, since it compiles nothing"
elif [ -n "$copy_missing" ]; then
  fail "the Dockerfile copies a path that is neither tracked nor re-included by the derivation:$copy_missing"
else
  ok "every COPY source ($copy_seen) is tracked or re-included by name in the derivation"
fi

# ── 3. the Dockerfile compiles nothing ───────────────────────────────────────
# Captured, then read; the producer's status is read first. `grep` without
# `-q` reads its whole input and is safe in a capture.
toolchain=$(grep -nE 'cargo|rust:' "$NAT/Dockerfile")
rc=$?
if [ "$rc" -eq 0 ]; then
  fail "the Dockerfile names a toolchain — the binary comes from the host, not from a build in the image:"
  printf '%s\n' "$toolchain" | sed 's/^/         /'
elif [ "$rc" -eq 1 ]; then
  ok "the Dockerfile has no line matching cargo|rust:"
else
  fail "could not read the Dockerfile (grep exit $rc)"
fi

# ── 4. run.sh parses ─────────────────────────────────────────────────────────
if syn=$(bash -n "$NAT/run.sh" 2>&1); then
  ok "run.sh parses"
else
  fail "run.sh does not parse: $syn"
fi

# ── 5. the networks block is the one at HEAD ─────────────────────────────────
# From `networks:` at column 0 to the next top-level key or the end of file.
networks_of() { awk '/^networks:/{p=1} p && !/^networks:/ && /^[^[:space:]#]/{p=0} p' "$1"; }
networks_of "$NAT/compose.yaml" >"$work/net.now"
if ! git -C "$ROOT" show HEAD:spikes/nat/compose.yaml >"$work/compose.head" 2>"$work/head.err"; then
  fail "could not read spikes/nat/compose.yaml at HEAD: $(cat "$work/head.err")"
else
  networks_of "$work/compose.head" >"$work/net.head"
  if [ ! -s "$work/net.now" ]; then
    fail "compose.yaml has no networks: block — the probe's routing claim is that block"
  elif ! cmp -s "$work/net.now" "$work/net.head"; then
    fail "compose.yaml's networks: block differs from HEAD's (HEAD < > now) — the routing changed:"
    diff "$work/net.head" "$work/net.now" | sed 's/^/         /'
  else
    ok "compose.yaml's networks: block is byte-identical to HEAD's ($(grep -c . "$work/net.now") lines)"
  fi
fi

echo
if [ "$fails" -eq 0 ]; then
  echo "nat local checks: PASS"
  exit 0
fi
echo "nat local checks: FAIL — $fails"
exit 1
