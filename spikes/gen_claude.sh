#!/bin/sh
# gen_claude.sh — the generator command forge-pipeline invokes (docs/PLAN.md §5.1).
#
# **A producer, not a gate**, and nothing in `run_checks.sh` or `ci.yml`
# invokes it — deliberately. It makes a model call, so it is neither
# deterministic nor free, and a gate that calls a model is a gate whose red
# means "the weather changed". `forge-pipeline` runs it when a producer
# command is configured, which is what the harness's S1-S3 leg reports as a
# counted SKIPPED naming its fixture (`E2E_MODEL=1` with a producer command
# and its key). That SKIPPED is the honest state and this line is the reason
# for it, written where `spikes/cited_and_run.py` looks — it had not been
# written anywhere, and D003 has cited this script since it landed.
#
# *게이트가 아니라 생산자다. 모델을 호출하므로 결정적이지도 공짜도 아니고,
# 모델을 부르는 게이트의 빨강은 "날씨가 바뀌었다"는 뜻이다. 하네스의
# S1-S3 다리가 계수된 SKIPPED로 보고하는 것이 정직한 상태다.*
#
#   $1 = requirement file (one natural-language requirement, UTF-8)
#   $2 = repair prompt file (optional; rounds 2+ only — the S4 repair_prompt(),
#        appended verbatim per §3.3: the diagnostics ARE the feedback channel)
#
# Prints IDL text — and nothing else — on stdout. A non-zero exit means the
# model call itself failed; forge-pipeline records that as `generator-error`,
# distinct from "the model wrote invalid IDL".
#
# POSIX sh on purpose: this is fixture-side plumbing, same rules as the rest
# of spikes/. Requires the `claude` CLI on PATH, authenticated for -p mode.
set -eu

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <requirement-file> [<repair-prompt-file>]" >&2
    exit 2
fi

REQUIREMENT=$(cat "$1")

# The prompt constraints mirror CLAUDE.md's "IDL rules the compiler enforces".
# The case-clash reminder is quoted because it is the dominant generation
# failure Phase 0 measured; note in any report that Phase 0's prompt did NOT
# carry this reminder, so first-pass rates are not directly comparable.
PROMPT="You are writing OMG IDL 4.2 for a CORBA system. Output the complete
IDL file text and NOTHING else: no markdown fences, no commentary, no
explanation before or after.

Rules this project's compiler enforces:
- Identifier clashes are case-insensitive. A member, parameter or operation
  may not share a name with a type or an enclosing scope, ignoring case.
  'Position position', 'Value value', 'module inventory { interface
  Inventory }' and 'struct Version { unsigned long version; }' are all
  illegal. This is natural naming in every other language, which is exactly
  why it is the dominant generation failure.
- Use structured comments for AI metadata: '//@ ai_desc: <one sentence>'
  and '//@ ai_effect: read_only|idempotent|destructive' above each
  operation. Do NOT use IDL 4 @annotation syntax — deployed compilers
  reject it.
- TypeCode, if used, must be qualified as ::CORBA::TypeCode.
- Do not use valuetype, abstract interfaces, or fixed — this wire does not
  support them.

Requirement (may be in Korean):
$REQUIREMENT"

# Rounds 2+: append the S4 repair prompt verbatim (§3.3 — the diagnostics are
# returned to the model unedited, grouped by cause).
if [ $# -eq 2 ]; then
    PROMPT="$PROMPT

$(cat "$2")"
fi

# Capture, then post-process — never pipe the producer straight into a
# consumer that may exit early (CLAUDE.md harness rules).
OUTPUT=$(claude -p "$PROMPT")

# Strip markdown fences if the model added them despite the instruction.
printf '%s\n' "$OUTPUT" | sed '/^[[:space:]]*```/d'
