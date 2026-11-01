"""Replay each decision point on one house model through Ollama's OpenAI-compatible endpoint,
as the node's local relay sends a team turn's call (whole, thinking left on).

    BENCH_URL=http://192.168.1.10:11434/v1 python3 -I run.py <bench dir> <model>
        -> <bench dir>/out-<model>.jsonl (appends; resumes)
"""
import json
import os
import sys
import time
import urllib.request

BENCH, MODEL = sys.argv[1], sys.argv[2]
URL = os.environ["BENCH_URL"].rstrip("/") + "/chat/completions"
out_path = f"{BENCH}/out-{MODEL.replace(':', '_')}.jsonl"

done = set()
try:
    for line in open(out_path):
        done.add(json.loads(line)["id"])
except FileNotFoundError:
    pass

for line in open(f"{BENCH}/points.jsonl"):
    p = json.loads(line)
    if p["id"] in done:
        continue
    body = dict(p["request"], model=MODEL, stream=False)
    req = urllib.request.Request(URL, data=json.dumps(body).encode(), headers={"content-type": "application/json"})
    started = time.time()
    try:
        with urllib.request.urlopen(req, timeout=900) as r:
            reply = json.load(r)
        error = None
    except Exception as e:  # noqa: BLE001 - a failed call is a result too
        reply, error = None, repr(e)[:300]
    took = round(time.time() - started, 1)
    msg = (reply or {}).get("choices", [{}])[0].get("message", {}) if reply else {}
    rec = {
        "id": p["id"], "model": MODEL, "seconds": took, "error": error,
        "finish": (reply or {}).get("choices", [{}])[0].get("finish_reason") if reply else None,
        "usage": (reply or {}).get("usage"),
        "content": msg.get("content"),
        "reasoning": msg.get("reasoning") or msg.get("reasoning_content"),
        "tool_calls": msg.get("tool_calls"),
    }
    with open(out_path, "a") as f:
        f.write(json.dumps(rec) + "\n")
    calls = [c["function"]["name"] for c in (rec["tool_calls"] or [])]
    print(p["id"], MODEL, took, "s", rec["finish"], calls or (rec["content"] or "")[:60], error or "", flush=True)
