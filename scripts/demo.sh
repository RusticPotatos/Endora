#!/usr/bin/env bash
#
# Endora end-to-end demo: drives the entire learning loop through the CLI
# against a running node, so you can see the whole thing in one go.
#
# Environment:
#   ENDORA_URL  node base URL   (default: http://127.0.0.1:8787)
#   ENDORA      path to the CLI (default: `endora` on PATH)
#
# Usage:
#   make demo                       # spins up a throwaway node and runs this
#   ./scripts/demo.sh               # against a node you started with `endora-node`
#
set -euo pipefail

ENDORA="${ENDORA:-endora}"
export ENDORA_URL="${ENDORA_URL:-http://127.0.0.1:8787}"

if ! curl -fsS "$ENDORA_URL/health" >/dev/null 2>&1; then
    echo "No Endora node at $ENDORA_URL." >&2
    echo "Start one with 'endora-node' (or run 'make demo')." >&2
    exit 1
fi

# Extract a top-level JSON string field from stdin (jq if available, else python3).
jget() {
    if command -v jq >/dev/null 2>&1; then
        jq -r ".$1"
    else
        python3 -c "import json,sys;print(json.load(sys.stdin)['$1'])"
    fi
}

# Run a CLI command and print it with its output.
run() {
    echo
    echo "\$ endora $*"
    "$ENDORA" "$@"
}

# Like run(), but return the created entity's id on stdout (command + output go
# to stderr so they stay visible without polluting the captured id).
capture() {
    echo >&2
    echo "\$ endora $*" >&2
    local out
    out="$("$ENDORA" "$@")"
    echo "$out" >&2
    printf '%s' "$out" | jget id
}

echo "=== Endora demo — the full learning loop ==="
echo "(node: $ENDORA_URL)"

did=$(capture direction create "Live intentionally")
gid=$(capture goal create "$did" "Run a 5k")
aid=$(capture assumption create "$gid" "Mornings are freest")
eid=$(capture experiment propose "$aid" "Try morning runs for two weeks")
run experiment start "$eid"
oid=$(capture observation record "$eid" "ran 3k before work, felt great")
run experiment conclude "$eid"
rid=$(capture reflection create "$gid" "mornings really do work for me" "$oid")
cid=$(capture process-change propose "$rid" "Default my runs to the morning")

echo
echo "--- the deterministic policy boundary (models propose, policy authorizes) ---"
run process-change decide "$cid" act_within_policy   # not approved yet -> requires human approval
run process-change approve "$cid"                    # a human approves
run process-change decide "$cid" act_within_policy   # -> permit
run process-change decide "$cid" observe             # observe-level actor -> deny

echo
echo "--- accountability + memory rights ---"
run audit                                            # every decision was recorded
run export                                           # your entire dataset, exportable as JSON

echo
echo "=== done — that whole tree now lives in your local SQLite database ==="
echo "Save it:   endora export > backup.json"
echo "Wipe it:   endora purge confirm"
