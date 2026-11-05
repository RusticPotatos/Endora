"""Turn Bo's stored transcripts back into the chat requests the house model was sent, one per
decision point (the conversation up to a call the model made), for a replay on two models.

    python3 -I build.py <bench dir>   -> <bench dir>/points.jsonl
"""
import json
import random
import re
import sys

BENCH = sys.argv[1]
MOST_A_HOUSE_REPLY = 8192
# qwen3.5:9b served the house from 30 Oct 10:04 UTC; qwen3:8b before.
NINE_B_FROM_MS = int(__import__("datetime").datetime(2026, 10, 30, 10, 4, tzinfo=__import__("datetime").UTC).timestamp() * 1000)

# --- the tool schemas the turn was offered -----------------------------------------------------
schemas = {}
hq = json.load(open(f"{BENCH}/hq.json"))
for t in hq["result"]["tools"]:
    schemas[t["name"]] = t
for t in json.load(open(f"{BENCH}/worker.json")):
    schemas[t["name"]] = t
for t in json.load(open(f"{BENCH}/node.json"))["tools"]:
    schemas.setdefault(t["name"], t)


def function(name):
    t = schemas.get(name)
    if t is None:
        return None
    return {"type": "function", "function": {
        "name": name,
        "description": t.get("description", ""),
        "parameters": t.get("inputSchema") or t.get("input_schema") or {"type": "object", "properties": {}},
    }}


# --- parsing a transcript ---------------------------------------------------------------------
HEAD = re.compile(r"^--- (tools|tools: held back|system|user|assistant|call: [a-z_]+|result) ---$", re.M)


def sections(body):
    marks = list(HEAD.finditer(body))
    for i, m in enumerate(marks):
        end = marks[i + 1].start() if i + 1 < len(marks) else len(body)
        yield m.group(1), body[m.end():end].strip("\n")


def parse(body):
    """The offered tools, the held-back ones, and the messages in order."""
    offered, held, messages = [], [], []
    pending = None
    for kind, text in sections(body):
        if kind == "tools":
            offered = [n for n in text.split("\n") if n]
        elif kind == "tools: held back":
            held = [n for n in text.split("\n") if n]
        elif kind == "system":
            messages.append({"role": "system", "content": text})
        elif kind == "user":
            messages.append({"role": "user", "content": text})
        elif kind == "assistant":
            messages.append({"role": "assistant", "content": text})
        elif kind.startswith("call: "):
            name = kind[len("call: "):]
            last = messages[-1] if messages and messages[-1]["role"] == "assistant" else None
            if last is None:
                last = {"role": "assistant", "content": ""}
                messages.append(last)
            cid = f"call_{len(messages)}_{len(last.get('tool_calls', []))}"
            last.setdefault("tool_calls", []).append(
                {"id": cid, "type": "function", "function": {"name": name, "arguments": text}})
            pending = (cid, name)
        elif kind == "result" and pending:
            messages.append({"role": "tool", "tool_call_id": pending[0], "content": text})
            pending = None
    return offered, held, messages


def added_by(result_text):
    """Names find_tools said it added."""
    if not result_text.startswith("Added for your next step"):
        return []
    return re.findall(r"^([a-z_]+): ", result_text, re.M)


def points_of(run):
    offered, held, messages = parse(run["body"])
    if not offered:
        # Transcripts before the tool list was kept (31 Oct): round 3's opening set, and every
        # tool the turn itself called, so each model is offered what that turn could use.
        offered = ["my_turn", "claim", "post", "set_status", "ask_owner", "shell", "find_tools"]
        used = re.findall(r"^--- call: ([a-z_]+) ---$", run["body"], re.M)
        offered += [n for n in dict.fromkeys(used) if n not in offered]
    # A decision point: the messages before each assistant message that made a call, and the
    # tools offered at that moment (the opening's, plus what find_tools brought in so far).
    tools_now = list(offered)
    found = []
    for i, m in enumerate(messages):
        if m["role"] == "tool":
            prev = messages[i - 1]
            if prev.get("tool_calls") and prev["tool_calls"][-1]["function"]["name"] == "find_tools":
                tools_now += [n for n in added_by(m["content"]) if n not in tools_now]
        if m["role"] == "assistant" and m.get("tool_calls"):
            actual = m["tool_calls"][0]["function"]
            fns = [f for f in (function(n) for n in tools_now) if f]
            found.append({
                "run": run["run"], "task": run["task"], "step": sum(1 for x in messages[:i] if x["role"] == "assistant"),
                "era": "9b" if run["at"] >= NINE_B_FROM_MS else "8b",
                "offered": tools_now[:], "held": held,
                "missing_schemas": [n for n in tools_now if n not in schemas],
                "actual": actual,
                "request": {"max_tokens": MOST_A_HOUSE_REPLY, "messages": messages[:i], "tools": fns},
            })
    return found


runs = json.load(open(f"{BENCH}/transcripts.json"))
# Work turns on tasks only, with a system prompt and tools (not chess or downtime).
pool = []
for r in runs:
    if r["task"] is None:
        continue
    for p in points_of(r):
        if p["request"]["messages"] and p["request"]["messages"][0]["role"] == "system" and p["request"]["tools"] and not p["missing_schemas"]:
            pool.append(p)

random.seed(20261101)
chosen = []
for era in ("9b", "8b"):
    mine = [p for p in pool if p["era"] == era]
    # At most two points from one run, opening and one later, spread over tasks.
    by_run = {}
    for p in mine:
        by_run.setdefault(p["run"], []).append(p)
    runs_ = list(by_run)
    random.shuffle(runs_)
    picked = []
    tasks_seen = {}
    for run in runs_:
        ps = by_run[run]
        task = ps[0]["task"]
        if tasks_seen.get(task, 0) >= 2:
            continue
        tasks_seen[task] = tasks_seen.get(task, 0) + 1
        picked.append(ps[0])
        if len(ps) > 2:
            picked.append(random.choice(ps[1:]))
        if len(picked) >= 15:
            break
    chosen += picked[:15]

with open(f"{BENCH}/points.jsonl", "w") as f:
    for i, p in enumerate(chosen):
        p["id"] = i
        f.write(json.dumps(p) + "\n")
print(len(pool), "points in the pool;", len(chosen), "chosen")
for p in chosen:
    size = len(json.dumps(p["request"]["messages"]))
    print(p["id"], p["era"], p["run"], p["task"], "step", p["step"], "tools", len(p["offered"]),
          "missing", p["missing_schemas"], "actual", p["actual"]["name"], size, "bytes")
