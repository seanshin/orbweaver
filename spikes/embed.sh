#!/bin/sh
# embed.sh — the embedding command the vector cache is built from (D003 part A).
#
#   stdin   one text per line, UTF-8. A line is one document; the text may not
#           be empty and may not contain a newline (that is what "per line"
#           means). Tabs are permitted but the vector cache format forbids them
#           in keys, so callers normally strip them first.
#   stdout  one JSON array of floats per line — `[0.0123,-0.4567,...]` — in the
#           SAME ORDER as the input lines, one output line per input line.
#   exit    0 only if every input line produced a vector. Non-zero means the
#           call itself failed (missing key, HTTP error, short response); the
#           caller records that as unmeasured, never as "the model returned
#           nothing". A partial batch is a failure, not a partial success.
#
# Environment:
#   VOYAGE_API_KEY   required. Absent is a *supported* state: this script exits
#                    non-zero with a clear message and the batch takes its
#                    documented offline path instead of pretending to measure.
#   EMBED_MODEL      default voyage-4-nano — the small/cheap member of the
#                    family Anthropic's own embeddings page points at, and the
#                    one whose weights licence the vendor states outright.
#   EMBED_ENDPOINT   default https://api.voyageai.com/v1/embeddings
#   EMBED_INPUT_TYPE optional: `document` when embedding the catalog, `query`
#                    when embedding a search query. Unset sends neither.
#   EMBED_BATCH      default 64 texts per HTTP request.
#
# Why a script and not a crate: D003 (APPROVED 2026-08-14) adopts embeddings as
# an EXTERNAL COMMAND, in the `gen_claude.sh` mold, precisely so that no model
# and no inference library ever enters `cargo tree`. **This project never links
# an embedding library.** The model is a separate process/service whose text
# output we read — the same boundary category as `omniidl` as a conformance
# oracle and omniORB as a wire peer. Vectors coming back over HTTPS are
# outputs, not incorporated code or data tables; nothing licensed enters the
# repository. See docs/decisions/D003-embeddings-and-catalog-storage.md.
#
# POSIX sh on purpose: fixture-side plumbing, same rules as the rest of
# spikes/. Requires `curl` and `python3` on PATH — python3 is used only as a
# JSON codec (it is already a project tool: spikes/idl_lint.py), never as a
# model. Capture-then-process throughout: never pipe a producer whose failure
# matters straight into a consumer that may exit early (CLAUDE.md harness
# rules).
set -eu

usage() {
    echo "usage: $0 < texts.txt > vectors.jsonl" >&2
    echo "  one text per line in, one JSON array of floats per line out" >&2
}

if [ $# -ne 0 ]; then
    usage
    exit 2
fi

ENDPOINT=${EMBED_ENDPOINT:-https://api.voyageai.com/v1/embeddings}
MODEL=${EMBED_MODEL:-voyage-4-nano}
BATCH=${EMBED_BATCH:-64}

if [ -z "${VOYAGE_API_KEY:-}" ]; then
    echo "$0: VOYAGE_API_KEY is not set." >&2
    echo "$0: this is a supported state — the embedding group is UNMEASURED," >&2
    echo "$0: never green. See docs/decisions/D003 and search-bench --vectors." >&2
    exit 3
fi

for tool in curl python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$0: $tool is required and is not on PATH" >&2
        exit 4
    fi
done

WORK=$(mktemp -d) || exit 5
trap 'rm -rf "$WORK"' EXIT INT TERM

cat > "$WORK/input.txt"

# Refuse an empty document before the API does: the vendor rejects empty
# strings, and a caller who sent one deserves to be told which line.
if ! python3 - "$WORK/input.txt" <<'PY'
import sys
path = sys.argv[1]
with open(path, encoding="utf-8") as fh:
    lines = fh.read().split("\n")
if lines and lines[-1] == "":
    lines.pop()
if not lines:
    sys.stderr.write("embed.sh: stdin was empty; nothing to embed\n")
    sys.exit(1)
for n, line in enumerate(lines, 1):
    if not line.strip():
        sys.stderr.write(f"embed.sh: line {n}: empty text, which no model can embed\n")
        sys.exit(1)
PY
then
    exit 6
fi

TOTAL=$(python3 -c 'import sys;print(sum(1 for _ in open(sys.argv[1],encoding="utf-8")))' \
        "$WORK/input.txt")

: > "$WORK/out.jsonl"
OFFSET=0
while [ "$OFFSET" -lt "$TOTAL" ]; do
    # Build the request body with a real JSON encoder. Hand-rolled shell
    # quoting is how instruction-shaped catalog text escapes its quoting, and
    # this repository has a whole risk (R11) about exactly that.
    python3 - "$WORK/input.txt" "$OFFSET" "$BATCH" "$MODEL" "${EMBED_INPUT_TYPE:-}" \
        > "$WORK/body.json" <<'PY'
import json, sys
path, offset, batch, model, input_type = sys.argv[1:6]
with open(path, encoding="utf-8") as fh:
    lines = fh.read().split("\n")
if lines and lines[-1] == "":
    lines.pop()
chunk = lines[int(offset):int(offset) + int(batch)]
body = {"input": chunk, "model": model}
if input_type:
    body["input_type"] = input_type
json.dump(body, sys.stdout)
PY

    # Capture status and body separately; a 200 with an error document and a
    # 429 must not look the same to the caller.
    STATUS=$(curl -sS -o "$WORK/resp.json" -w '%{http_code}' \
                  -X POST "$ENDPOINT" \
                  -H "Authorization: Bearer $VOYAGE_API_KEY" \
                  -H 'Content-Type: application/json' \
                  --data-binary @"$WORK/body.json") || {
        echo "$0: curl failed against $ENDPOINT" >&2
        exit 7
    }
    if [ "$STATUS" != "200" ]; then
        echo "$0: $ENDPOINT returned HTTP $STATUS" >&2
        head -c 400 "$WORK/resp.json" >&2
        echo >&2
        exit 8
    fi

    # Decode with the same encoder, honouring the vendor's `index` field rather
    # than trusting response order.
    if ! python3 - "$WORK/resp.json" "$OFFSET" "$BATCH" "$TOTAL" >> "$WORK/out.jsonl" <<'PY'
import json, sys
path, offset, batch, total = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
with open(path, encoding="utf-8") as fh:
    doc = json.load(fh)
data = doc.get("data")
if not isinstance(data, list):
    sys.stderr.write(f"embed.sh: response has no data array: {str(doc)[:200]}\n")
    sys.exit(1)
want = min(batch, total - offset)
if len(data) != want:
    sys.stderr.write(f"embed.sh: asked for {want} vectors, got {len(data)}\n")
    sys.exit(1)
rows = [None] * want
for item in data:
    idx = item.get("index")
    vec = item.get("embedding")
    if not isinstance(idx, int) or not 0 <= idx < want or not isinstance(vec, list):
        sys.stderr.write(f"embed.sh: malformed embedding entry: {str(item)[:200]}\n")
        sys.exit(1)
    if rows[idx] is not None:
        sys.stderr.write(f"embed.sh: duplicate index {idx} in response\n")
        sys.exit(1)
    rows[idx] = vec
for vec in rows:
    if vec is None:
        sys.stderr.write("embed.sh: response skipped an index\n")
        sys.exit(1)
    sys.stdout.write("[" + ",".join(repr(float(x)) for x in vec) + "]\n")
PY
    then
        exit 9
    fi

    OFFSET=$((OFFSET + BATCH))
done

# One last count check: same order is only meaningful alongside same length.
PRODUCED=$(python3 -c 'import sys;print(sum(1 for _ in open(sys.argv[1],encoding="utf-8")))' \
           "$WORK/out.jsonl")
if [ "$PRODUCED" != "$TOTAL" ]; then
    echo "$0: produced $PRODUCED vectors for $TOTAL texts" >&2
    exit 10
fi

cat "$WORK/out.jsonl"
