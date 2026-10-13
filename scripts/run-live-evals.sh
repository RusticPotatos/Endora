#!/usr/bin/env bash
#
# Runs the live model evaluations and keeps a dated record of what they measured, so a model or
# prompt change shows up as a number that moved instead of a feeling.
#
# Needs a model endpoint: ENDORA_MODEL_URL (OpenAI-compatible, e.g. your Ollama) and
# ENDORA_MODEL. Nothing here touches the deployed node or any personal data; the cases use the
# placeholder places and teams from AGENTS.md.
#
# Usage:
#   ENDORA_MODEL_URL=... ENDORA_MODEL=qwen2.5:7b ./scripts/run-live-evals.sh
#   make evals                      # same, reading both from local.mk
#
# The record goes to target/evals/<date>.txt; the headline lines are also printed. To run it on a
# schedule, call it from cron or a systemd timer on the machine that can reach the model.
set -uo pipefail

: "${ENDORA_MODEL_URL:?set ENDORA_MODEL_URL}"
: "${ENDORA_MODEL:?set ENDORA_MODEL}"
export ENDORA_EVAL_MODELS="${ENDORA_EVAL_MODELS:-$ENDORA_MODEL}"

out_dir="target/evals"
mkdir -p "$out_dir"
record="$out_dir/$(date +%Y-%m-%d).txt"
{
  echo "# live evaluations $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# model: $ENDORA_MODEL   build: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$record"

status=0
for suite in router_eval tool_choice_eval butler_eval; do
  echo "== $suite" | tee -a "$record"
  if ! cargo test -p endora-infrastructure --test "$suite" -- --ignored --nocapture --test-threads=1 \
      2>&1 | tee -a "$record" | grep -E "^(word list|[a-z0-9.:_-]+: exact|  SUMMARY|test result)"; then
    status=1
  fi
done
echo "record: $record"
exit $status
