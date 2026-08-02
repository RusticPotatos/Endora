#!/usr/bin/env bash
# Smoke test: does the butler actually operate the kitchen light via the Home
# Assistant MCP tools? Hits the live node's chat endpoint and checks that a real HA
# *action* tool ran (turn on/off / set) — not just a state read, and not the degraded
# "couldn't reach my model" reply.
#
# Usage:
#   scripts/smoke-kitchen-light.sh ["turn off the kitchen light"]
#   ENDORA_URL=https://host:8787 scripts/smoke-kitchen-light.sh
#
# Exit 0 = PASS (an HA action tool ran); 1 = no action ran; 2 = degraded reply.
set -uo pipefail

BASE="${ENDORA_URL:-https://192.168.1.10:8787}"
MSG="${1:-turn off the kitchen light}"

payload=$(MSG="$MSG" python3 -c 'import json,os; print(json.dumps({"message": os.environ["MSG"]}))')
resp=$(curl -sk -m 240 -X POST "$BASE/v1/chat" -H 'content-type: application/json' -d "$payload" 2>/dev/null)

reply=$(printf '%s' "$resp" | python3 -c 'import sys,json; d=json.load(sys.stdin); print((d.get("reply") or {}).get("text",""))' 2>/dev/null)
activity=$(printf '%s' "$resp" | python3 -c 'import sys,json; d=json.load(sys.stdin); print("\n".join(d.get("activity",[])))' 2>/dev/null)

echo "URL     : $BASE"
echo "MESSAGE : $MSG"
echo "REPLY   : $reply"
echo "ACTIVITY:"
printf '%s\n' "$activity" | sed 's/^/  - /'
echo "---"

if printf '%s' "$activity" | grep -qiE "Used the home-assistant\.(HassTurnOn|HassTurnOff|HassLightSet)"; then
  echo "SMOKE: PASS — an HA action tool ran"
  exit 0
fi
if printf '%s' "$reply" | grep -qi "couldn't reach my language model"; then
  echo "SMOKE: FAIL — degraded reply (model cold-load / timeout)"
  exit 2
fi
echo "SMOKE: FAIL — no HA action tool ran (model read state or answered without acting)"
exit 1
