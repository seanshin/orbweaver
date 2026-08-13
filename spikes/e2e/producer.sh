#!/bin/sh
# producer.sh — the one command `forge-pipeline` invokes for S1, S2 and S3.
#
#   $1 = input file      (requirement text for S1, brief for S2, IDL for S3)
#   $2 = repair prompt   (optional; rounds 2+ only, appended verbatim per §3.3)
#
# The stage's constraints arrive in the environment as FORGE_PROMPT, a file the
# pipeline writes straight out of the crate, so the prompt a measurement used is
# versioned with the checker that graded it. FORGE_STAGE says which stage this
# is; nothing here needs to know, which is the point of one wrapper for three
# stages. Standard output is the artifact and nothing else.
#
# The 2026-08-13 split-pipeline run left this file in a scratch directory and
# recorded "the recommended committed form is in the Harness section"; this is
# that form, committed. Its first attempt failed 20/20 with `producer-error`
# because an apostrophe inside a `${VAR:?...}` default made the script a syntax
# error — hence no `:?` expansions below, and an explicit check instead.
set -eu

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <input-file> [<repair-prompt-file>]" >&2
    exit 2
fi
if [ -z "${FORGE_PROMPT:-}" ] || [ ! -f "$FORGE_PROMPT" ]; then
    echo "FORGE_PROMPT must name the stage prompt file" >&2
    exit 2
fi

PROMPT=$(cat "$FORGE_PROMPT")
INPUT=$(cat "$1")

if [ $# -eq 2 ]; then
    REPAIR=$(cat "$2")
else
    REPAIR=""
fi

# Capture, then post-process. Never pipe the producer straight into a consumer
# that may exit early (CLAUDE.md harness rules) — a non-zero exit here is
# recorded by the pipeline as `producer-error`, which is a different fact from
# "the model produced something invalid".
OUTPUT=$(claude -p "$PROMPT

$INPUT

$REPAIR")

# Strip markdown fences if the model added them despite the instruction.
printf '%s\n' "$OUTPUT" | sed '/^[[:space:]]*```/d'
