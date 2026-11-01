"""Mechanical scores for each model's replies: did it act, with a tool it was offered, with
arguments that fit, without calling Endora's tools from the shell (D14).

    python3 -I score.py <bench dir> <model> [<model> ...]
"""
import json
import re
import statistics
import sys

BENCH, MODELS = sys.argv[1], sys.argv[2:]
points = {json.loads(l)["id"]: json.loads(l) for l in open(f"{BENCH}/points.jsonl")}
ENDORA = {"my_turn", "claim", "post", "set_status", "create_task", "ask_owner", "check_back", "recheck",
          "read_document", "read_doc", "chess", "read_world", "ask_record", "add_to_map", "homelab_status",
          "container_logs", "homelab_map", "ansible_nas_doc", "repo_checkout", "repo_submit", "image_tags",
          "find_tools"}


def schema_of(point, name):
    for t in point["request"]["tools"]:
        if t["function"]["name"] == name:
            return t["function"]["parameters"]
    return None


def judge(point, rec):
    calls = rec["tool_calls"] or []
    r = {"error": bool(rec["error"]), "cut": rec["finish"] == "length", "acted": bool(calls),
         "offered": None, "args_ok": None, "shell_endora": False, "same_as_bo": False}
    if not calls:
        return r
    c = calls[0]["function"]
    name = c["name"]
    r["offered"] = name in point["offered"]
    r["same_as_bo"] = name == point["actual"]["name"]
    try:
        args = json.loads(c["arguments"]) if isinstance(c["arguments"], str) else c["arguments"]
        schema = schema_of(point, name) or {}
        need = schema.get("required", [])
        props = schema.get("properties", {})
        r["args_ok"] = isinstance(args, dict) and all(k in args for k in need) and \
            all(k in props for k in args) if props else isinstance(args, dict)
    except (ValueError, TypeError):
        args, r["args_ok"] = None, False
    if name == "shell" and isinstance(args, dict):
        first = re.split(r"[\s;&|]+", (args.get("command") or "").strip().replace("cd /work &&", "").strip())
        r["shell_endora"] = any(w in ENDORA for w in first[:3])
    return r


rows = {}
for model in MODELS:
    path = f"{BENCH}/out-{model.replace(':', '_')}.jsonl"
    recs = [json.loads(l) for l in open(path)]
    rows[model] = [(points[x["id"]], x, judge(points[x["id"]], x)) for x in recs]

print(f"{'':28}" + "".join(f"{m:>16}" for m in MODELS))


def line(label, fn):
    print(f"{label:28}" + "".join(f"{fn(rows[m]):>16}" for m in MODELS))


def frac(key, era=None, of=None):
    def f(rs):
        rs = [r for r in rs if era is None or r[0]["era"] == era]
        base = [r for r in rs if of is None or r[2][of]]
        hit = sum(1 for r in base if r[2][key])
        return f"{hit}/{len(base)}"
    return f


def med(fn):
    def f(rs):
        vals = [v for v in (fn(r) for r in rs) if v is not None]
        return f"{statistics.median(vals):.0f}" if vals else "-"
    return f


line("replies", lambda rs: str(len(rs)))
line("errors", frac("error"))
line("cut at max_tokens", frac("cut"))
line("made a tool call", frac("acted"))
line("  on 9B-era points", frac("acted", "9b"))
line("  on 8B-era points", frac("acted", "8b"))
line("tool was offered", frac("offered", of="acted"))
line("arguments fit schema", frac("args_ok", of="acted"))
line("Endora tool via shell", frac("shell_endora", of="acted"))
line("same tool Bo chose", frac("same_as_bo", of="acted"))
line("median seconds", med(lambda r: r[1]["seconds"]))
line("median output tokens", med(lambda r: (r[1]["usage"] or {}).get("completion_tokens")))
line("median thinking chars", med(lambda r: len(r[1]["reasoning"] or "")))
line("median prompt tokens", med(lambda r: (r[1]["usage"] or {}).get("prompt_tokens")))
