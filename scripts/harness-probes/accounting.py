#!/usr/bin/env python3
"""Accounting for the harness comparison: raw traces -> attempt records -> metrics.

Stdlib only. Attempt record format: SCHEMA.md (this module adds the optional keys `meta`,
`actual_charge_usd`, `terminal_at`, and per-usage `cache_write_1h`/`status`).

Rules (the researcher's):
  * uncached input, cache reads, cache writes and output are metered separately, summed once;
  * every model call counts (continuations, retries, subagents); a call repeated over several
    trace lines, or over a resumed session's file, counts once (by message id);
  * unknown usage is null with known=false, never 0; totals say `complete: false`;
  * cash: one frozen tariff (tariff.json) for the normalized figure; the actual charge is
    reported separately and only where a trace states it;
  * time runs dispatched_at -> ended_at; agent-active = union of model/tool intervals,
    external waiting = union of wait intervals; contaminated when waiting > threshold * elapsed;
  * handoffs, fresh sessions and continuations are counted apart;
  * tool calls: total, failed, refused, wasted (see WASTED_RULE).
"""
import argparse
import json
import re
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_TARIFF = HERE / "tariff.json"
DEFAULT_WAIT_THRESHOLD = 0.10

WASTED_RULE = (
    "A tool call is wasted only when the trace says so explicitly: its result is an error "
    "(is_error true, which includes a refusal), or it was refused (the permission system or "
    "the node's policy refused it), or it started after the attempt's terminal outcome "
    "(terminal_at, default ended_at). A non-zero exit code alone is failed, not wasted: a "
    "failing test run is information. Intent is never guessed."
)

# Phrases Claude Code writes in a tool_result when the tool was not allowed to run.
CC_REFUSAL = re.compile(
    r"(user doesn't want to proceed|tool use was rejected|permission to use .{0,80} has been denied"
    r"|was blocked by (?:a )?hook|blocked by hook|requires approval)", re.I)
EXIT_CODE = re.compile(r"(?im)^\s*exit(?:ed)?(?: with)?(?: code| status)?\s*[:= ]\s*(-?\d+)\b")

FIELDS = ("input_uncached", "cache_read", "cache_write", "output")


# ----------------------------------------------------------------------------- helpers

def iso_to_epoch(s):
    if not s:
        return None
    try:
        return datetime.fromisoformat(str(s).replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def is_int(v):
    return isinstance(v, int) and not isinstance(v, bool)


def make_usage(call_id, agent, model, uncached, read, write, output,
               write_1h=None, retry=False, status=None, ttl_known=True):
    vals = [uncached, read, write, output]
    known = all(is_int(v) for v in vals)
    u = {"call_id": call_id, "agent": agent, "model": model,
         "input_uncached": uncached if is_int(uncached) else None,
         "cache_read": read if is_int(read) else None,
         "cache_write": write if is_int(write) else None,
         "output": output if is_int(output) else None,
         "known": known, "retry": bool(retry), "ttl_known": bool(ttl_known)}
    if is_int(write_1h):
        u["cache_write_1h"] = write_1h
    if status is not None:
        u["status"] = status
    return u


def usage_from_api(usage):
    """(uncached, read, write, output, write_1h) from an Anthropic usage dict; None if absent."""
    if not isinstance(usage, dict):
        return (None,) * 5
    write = usage.get("cache_creation_input_tokens")
    cc = usage.get("cache_creation")
    w1h = None
    if isinstance(cc, dict):
        w5, w1 = cc.get("ephemeral_5m_input_tokens"), cc.get("ephemeral_1h_input_tokens")
        if is_int(w1):
            w1h = w1
        if write is None and is_int(w5) and is_int(w1):
            write = w5 + w1
    return (usage.get("input_tokens"), usage.get("cache_read_input_tokens"), write,
            usage.get("output_tokens"), w1h)


def merge_usage(a, b):
    """Field-wise max: lines of one split message repeat usage, a streaming one may grow."""
    if not isinstance(a, dict):
        return b
    if not isinstance(b, dict):
        return a
    out = dict(a)
    for k, v in b.items():
        if is_int(v) and is_int(out.get(k)):
            out[k] = max(out[k], v)
        elif k not in out or out[k] is None:
            out[k] = v
    return out


def merge_intervals(intervals):
    ivs = sorted((s, e) for s, e in intervals if s is not None and e is not None and e >= s)
    out = []
    for s, e in ivs:
        if out and s <= out[-1][1]:
            out[-1][1] = max(out[-1][1], e)
        else:
            out.append([s, e])
    return out


def union_length(intervals, lo=None, hi=None):
    total = 0.0
    for s, e in merge_intervals(intervals):
        if lo is not None:
            s = max(s, lo)
        if hi is not None:
            e = min(e, hi)
        if e > s:
            total += e - s
    return total


def union_length_less(intervals, minus, lo=None, hi=None):
    """The length of the union of `intervals`, less the parts the union of `minus` covers."""
    cut = merge_intervals(minus)
    total = 0.0
    for s, e in merge_intervals(intervals):
        if lo is not None:
            s = max(s, lo)
        if hi is not None:
            e = min(e, hi)
        if e <= s:
            continue
        total += e - s
        for cs, ce in cut:
            total -= max(0.0, min(e, ce) - max(s, cs))
    return total


def read_jsonl(path):
    """(records, bad_line_count). A truncated or garbled line is counted, never fatal."""
    out, bad = [], 0
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if isinstance(rec, dict):
                out.append(rec)
            else:
                bad += 1
    return out, bad


def result_text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(b.get("text", "") for b in content if isinstance(b, dict))
    return "" if content is None else str(content)


def tool_fields(name, inp):
    inp = inp if isinstance(inp, dict) else {}
    command = inp.get("command") if isinstance(inp.get("command"), str) else None
    path = None
    for k in ("file_path", "path", "notebook_path"):
        if isinstance(inp.get(k), str):
            path = inp[k]
            break
    return command, path


# ----------------------------------------------------------------------------- Claude Code arm

class _Msg:
    def __init__(self, mid, agent):
        self.id, self.agent = mid, agent
        self.model = None
        self.usage = None
        self.first_ts = self.last_ts = None
        self.parent = None
        self.stop = None
        self.error = False
        self.text = []
        self.from_stream = False


def _terminal_from_result(res):
    sub = res.get("subtype", "")
    if res.get("is_error") is False and sub == "success":
        return "done"
    if sub == "error_max_turns":
        return "timeout"
    return "failed"


def parse_cc(sessions, subagents_dir=None, stream_json=None, family="", seed=0):
    sources = []  # (path, kind, agent)
    for p in sessions:
        sources.append((Path(p), "session", None))
    if subagents_dir:
        for p in sorted(Path(subagents_dir).rglob("*.jsonl")):
            stem = p.stem
            sources.append((p, "subagent", stem[len("agent-"):] if stem.startswith("agent-") else stem))
    loaded = []
    bad_lines = 0
    for path, kind, aid in sources:
        recs, bad = read_jsonl(path)
        bad_lines += bad
        loaded.append((path, kind, aid, recs))
    stream_recs = []
    if stream_json:
        stream_recs, bad = read_jsonl(stream_json)
        bad_lines += bad

    uuid_ts = {}
    for _, _, _, recs in loaded:
        for r in recs:
            if r.get("uuid") and r.get("timestamp"):
                uuid_ts[r["uuid"]] = iso_to_epoch(r["timestamp"])

    msgs = {}            # message id -> _Msg (insertion order = first sight)
    tool_uses = {}       # tool_use id -> dict
    tool_results = {}    # tool_use id -> dict
    seen_keys = set()
    session_files = []   # (path, continuation?)
    compactions = set()
    all_ts = []
    prompt_ts = []
    subagent_first = {}
    last_main_text = (None, None)  # (ts, text)
    last_stop = None

    def agent_of(rec, default):
        if default:
            return "subagent:" + (rec.get("agentId") or default)
        if rec.get("isSidechain") and rec.get("agentId"):
            return "subagent:" + rec["agentId"]
        return "main"

    def feed(rec, default_agent, ts):
        nonlocal last_main_text, last_stop
        typ = rec.get("type")
        agent = agent_of(rec, default_agent)
        if agent != "main":
            subagent_first.setdefault(agent, ts)
        if typ == "system" and rec.get("subtype") == "compact_boundary":
            compactions.add(rec.get("uuid") or id(rec))
            return
        if rec.get("isCompactSummary"):
            compactions.add(rec.get("uuid") or id(rec))
            return
        message = rec.get("message")
        if not isinstance(message, dict):
            return
        if typ == "assistant":
            mid = message.get("id") or rec.get("uuid")
            m = msgs.get(mid)
            if m is None:
                m = msgs[mid] = _Msg(mid, agent)
                m.parent = rec.get("parentUuid")
                m.first_ts = ts
            m.last_ts = ts if ts is not None else m.last_ts
            m.model = m.model or message.get("model")
            m.usage = merge_usage(m.usage, message.get("usage"))
            m.stop = message.get("stop_reason") or m.stop
            if rec.get("isApiErrorMessage") is True:
                m.error = True
            content = message.get("content")
            for b in content if isinstance(content, list) else []:
                if not isinstance(b, dict):
                    continue
                if b.get("type") == "tool_use" and b.get("id") and b["id"] not in tool_uses:
                    tool_uses[b["id"]] = {"name": b.get("name"), "input": b.get("input"),
                                          "ts": ts, "agent": agent, "msg": mid}
                if b.get("type") == "text" and isinstance(b.get("text"), str):
                    m.text.append(b["text"])
            return
        if typ == "user":
            content = message.get("content")
            if isinstance(content, list):
                for b in content:
                    if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id"):
                        tool_results.setdefault(b["tool_use_id"], {
                            "ts": ts, "is_error": b.get("is_error"),
                            "text": result_text(b.get("content")),
                            "meta": rec.get("toolUseResult")})
                return
            if agent == "main" and not rec.get("isMeta") and ts is not None:
                prompt_ts.append(ts)

    for path, kind, aid, recs in loaded:
        if kind == "session":
            keys = {("a", (r.get("message") or {}).get("id")) for r in recs
                    if r.get("type") == "assistant" and (r.get("message") or {}).get("id")}
            keys |= {("u", r["uuid"]) for r in recs if r.get("type") == "user" and r.get("uuid")}
            continuation = bool(session_files) and bool(keys & seen_keys)
            session_files.append((str(path), continuation))
            seen_keys |= keys
        for r in recs:
            ts = iso_to_epoch(r.get("timestamp"))
            if ts is not None:
                all_ts.append(ts)
            feed(r, aid, ts)

    # The headless stream fills in what the session files lack, and is reconciled below.
    stream_result = None
    results_seen = 0
    for r in stream_recs:
        if r.get("type") == "result":
            stream_result = r
            results_seen += 1
            continue
        default = None
        if r.get("parent_tool_use_id"):
            default = str(r["parent_tool_use_id"])
        before = set(msgs)
        feed(r, default, None)
        for mid in set(msgs) - before:
            msgs[mid].from_stream = True

    # ---- usage records and model_call events
    usage_list, events = [], []
    for mid, m in msgs.items():
        synthetic = m.model == "<synthetic>"
        if m.error and synthetic:
            parts = (None,) * 5
        else:
            parts = usage_from_api(m.usage)
        model = None if synthetic else m.model
        usage_list.append(make_usage(mid, m.agent, model, *parts[:4], write_1h=parts[4],
                                     retry=m.error, status="api_error" if m.error else None))
        t_start = uuid_ts.get(m.parent, m.first_ts)
        if t_start is None:
            t_start = m.first_ts
        ev = {"kind": "model_call", "t_start": t_start, "t_end": m.last_ts,
              "agent": m.agent, "tool": None, "command": None, "path": None, "exit": None,
              "is_error": m.error, "refused": False, "wait_kind": None, "call_id": mid}
        events.append(ev)

    # ---- tool_call events (use <-> result pairing)
    for tid, tu in tool_uses.items():
        res = tool_results.get(tid)
        command, path = tool_fields(tu["name"], tu["input"])
        exit_code, is_error, refused, t_end = None, None, False, None
        if res:
            t_end = res["ts"]
            is_error = bool(res["is_error"]) if res["is_error"] is not None else False
            meta = res["meta"] if isinstance(res["meta"], dict) else {}
            for k in ("exitCode", "exit_code"):
                if is_int(meta.get(k)):
                    exit_code = meta[k]
            if exit_code is None:
                mt = re.match(r"\s*Exit code (-?\d+)", res["text"])
                if mt:
                    exit_code = int(mt.group(1))
            refused = bool(is_error and CC_REFUSAL.search(res["text"]))
            # Claude Code reports a non-zero exit as an error result ("Exit code N"); a Bash
            # result without the error flag is an exit of 0.
            if exit_code is None and tu["name"] == "Bash" and res["is_error"] is False:
                exit_code = 0
        events.append({"kind": "tool_call", "t_start": tu["ts"], "t_end": t_end,
                       "agent": tu["agent"], "tool": tu["name"], "command": command,
                       "path": path, "exit": exit_code, "is_error": is_error,
                       "refused": refused, "wait_kind": None, "call_id": tid,
                       "output": kept_output(res["text"]) if res else None})
        if tu["name"] == "AskUserQuestion":
            events.append({"kind": "question", "t_start": tu["ts"], "t_end": tu["ts"],
                           "agent": tu["agent"]})

    # ---- sessions and handoffs
    for i, (path, cont) in enumerate(session_files):
        events.append({"kind": "session_start", "t_start": None, "t_end": None,
                       "agent": "main", "continuation": cont, "source": Path(path).name})
    for agent, ts in subagent_first.items():
        events.append({"kind": "handoff", "t_start": ts, "t_end": ts, "agent": agent,
                       "source": "subagent"})
    events.sort(key=lambda e: (e.get("t_start") is None, e.get("t_start") or 0))

    # ---- report, times, terminal
    final_report, last_ts_main, last_stop = "", None, None
    for m in msgs.values():
        if m.agent == "main" and not m.error and m.text:
            if last_ts_main is None or (m.last_ts or 0) >= last_ts_main or m.from_stream:
                final_report, last_ts_main, last_stop = "\n".join(m.text), m.last_ts or 0, m.stop
    if stream_result and isinstance(stream_result.get("result"), str):
        final_report = stream_result["result"]
    dispatched = min(prompt_ts) if prompt_ts else (min(all_ts) if all_ts else None)
    ended = max(all_ts) if all_ts else None
    if stream_result:
        terminal = _terminal_from_result(stream_result)
    elif last_stop == "end_turn":
        terminal = "done"
    elif msgs and list(msgs.values())[-1].error:
        terminal = "failed"
    else:
        terminal = "timeout"

    meta = {"parse_errors": bad_lines, "compactions": len(compactions),
            "session_files": [{"file": Path(p).name, "continuation": c} for p, c in session_files],
            "subagents": sorted(subagent_first)}
    attempt = {"arm": "claude-code", "family": family, "seed": seed, "final_report": final_report,
               "dispatched_at": dispatched, "ended_at": ended, "terminal": terminal,
               "events": events, "usage": usage_list, "meta": meta}
    if stream_result:
        if is_number(stream_result.get("total_cost_usd")):
            attempt["actual_charge_usd"] = stream_result["total_cost_usd"]
        meta["stream_result"] = {k: stream_result.get(k) for k in
                                 ("subtype", "is_error", "duration_ms", "duration_api_ms",
                                  "num_turns", "total_cost_usd")}
        meta["stream_result"]["result_events"] = results_seen
        meta["reconciliation"] = reconcile_stream(usage_list, stream_result.get("usage"))
        helpers = helper_calls(usage_list, stream_result.get("modelUsage"))
        usage_list.extend(helpers)
        meta["helper_calls_from_model_usage"] = len(helpers)
    return attempt


def helper_calls(usage_list, model_usage):
    """Model calls Claude Code made but never wrote to its transcript (a helper on a small
    model): what its result's `modelUsage` counts beyond the summed messages, per model, as one
    usage record each, attributed `helper`. Cache figures beyond the messages' go with them."""
    if not isinstance(model_usage, dict):
        return []
    keys = {"input_uncached": "inputTokens", "cache_read": "cacheReadInputTokens",
            "cache_write": "cacheCreationInputTokens", "output": "outputTokens"}
    out = []
    for model, seen in model_usage.items():
        if not isinstance(seen, dict):
            continue
        summed = {f: sum(u[f] for u in usage_list if u.get("model") == model and is_int(u.get(f)))
                  for f in keys}
        extra = {f: (seen.get(k) - summed[f]) if is_int(seen.get(k)) else None
                 for f, k in keys.items()}
        if not any(is_int(v) and v > 0 for v in extra.values()):
            continue
        extra = {f: max(0, v) if is_int(v) else v for f, v in extra.items()}
        out.append(make_usage(f"helper:{model}", "helper", model, extra["input_uncached"],
                              extra["cache_read"], extra["cache_write"], extra["output"],
                              status="from_model_usage"))
    return out


def is_number(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def reconcile_stream(usage_list, result_usage):
    """Per-message usage summed against the stream's result event. Mismatch is reported."""
    if not isinstance(result_usage, dict):
        return {"checked": False, "match": None, "reason": "result event carries no usage"}
    summed = {f: 0 for f in FIELDS}
    complete = True
    for u in usage_list:
        for f in FIELDS:
            if is_int(u[f]):
                summed[f] += u[f]
            else:
                complete = False
    pairs = {"input_uncached": "input_tokens", "cache_read": "cache_read_input_tokens",
             "cache_write": "cache_creation_input_tokens", "output": "output_tokens"}
    fields, ok = {}, True
    for f, key in pairs.items():
        if not is_int(result_usage.get(key)):
            continue
        diff = summed[f] - result_usage[key]
        fields[f] = {"summed": summed[f], "result": result_usage[key], "diff": diff}
        ok = ok and diff == 0
    return {"checked": True, "match": ok, "summed_complete": complete, "fields": fields}


# ----------------------------------------------------------------------------- Endora arm

def model_for_brain(brain):
    b = (brain or "").lower()
    for key, model in (("opus", "claude-opus-5-5"), ("sonnet", "claude-sonnet-5-5"),
                       ("haiku", "claude-haiku-5-5")):
        if key in b:
            return model
    if b in ("local", "mac"):
        return "local"
    return None


def _cols(con, table):
    return [r[1] for r in con.execute(f"PRAGMA table_info({table})")]


def _select(con, table, wanted, where="", args=()):
    """Rows as dicts, columns missing from this store's table come back as None."""
    have = _cols(con, table)
    if not have:
        return []
    sel = ", ".join(c if c in have else f"NULL AS {c}" for c in wanted)
    return [dict(r) for r in con.execute(f"SELECT {sel} FROM {table} {where}", args)]


def _ms(v):
    return None if v is None else v / 1000.0


def _split_kinds(body):
    return [kind for kind, _ in _split_transcript(body or "")]


def _split_transcript(body):
    """[(kind, text)] from the worker's `--- kind ---` divided text."""
    parts, kind, buf = [], None, []
    for line in (body or "").splitlines(keepends=True):
        mt = re.match(r"^--- (.+) ---\n?$", line)
        if mt:
            if kind is not None:
                parts.append((kind, "".join(buf)))
            kind, buf = mt.group(1), []
        elif kind is not None:
            buf.append(line)
    if kind is not None:
        parts.append((kind, "".join(buf)))
    return parts


# Waits that are the harness's own doing, not the world's: never contamination.
HARNESS_WAITS = ("queue", "retry_backoff")


def _relay_ok(status):
    """A relay log's call went through: no status, a word for success, or an HTTP 2xx."""
    if status in (None, "ok", "success"):
        return True
    return is_int(status) and 200 <= status < 300


def transcript_assistant_text(body):
    """What the model said in a transcript, its assistant blocks in order."""
    return "\n".join(text.strip() for kind, text in _split_transcript(body)
                     if kind == "assistant" and text.strip())


def transcript_tool_calls(body, agent):
    """Tool-call events from a transcript. Calls and results pair in order."""
    out = []
    for kind, text in _split_transcript(body):
        if kind.startswith("tried again"):
            # HQ's divider before a later attempt of the turn: the calls the killed attempt
            # left unanswered stay unanswered, and the new attempt's results are its own.
            for item in out:
                if item["result"] is None:
                    item["result"] = False
            continue
        if kind.startswith("call: "):
            out.append({"call": (kind[len("call: "):], text), "result": None})
        elif kind == "result" or kind.startswith("result:"):
            # Each result answers the earliest call still unanswered.
            for item in out:
                if item["result"] is None:
                    item["result"] = (kind, text)
                    break
    events = []
    for item in out:
        name, argtext = item["call"]
        try:
            args = json.loads(argtext.strip() or "{}")
        except json.JSONDecodeError:
            args = {}
        command, path = tool_fields(name, args)
        if command is None and isinstance(args, dict) and isinstance(args.get("cmd"), str):
            command = args["cmd"]
        exit_code, refused, is_error, output = None, False, None, None
        if item["result"] is False:
            is_error = True  # cut off: the attempt ended before the call answered
        if item["result"]:
            rkind, rtext = item["result"]
            refused = rkind == "result: refused"
            is_error = refused
            output = kept_output(rtext)
            mt = EXIT_CODE.search(rtext)
            if mt:
                exit_code = int(mt.group(1))
        events.append({"kind": "tool_call", "t_start": None, "t_end": None, "agent": agent,
                       "tool": name, "command": command, "path": path, "exit": exit_code,
                       "is_error": is_error, "refused": refused, "wait_kind": None,
                       "output": output})
    return events


# How much of a tool's output an attempt record keeps: enough for a test run's summary.
OUTPUT_KEPT = 8000


def kept_output(text):
    """A tool result's text for the record: whole when short, else its head and tail."""
    text = text if isinstance(text, str) else str(text or "")
    if len(text) <= OUTPUT_KEPT:
        return text
    half = OUTPUT_KEPT // 2
    return text[:half] + f"\n[… {len(text) - OUTPUT_KEPT} chars not kept …]\n" + text[-half:]


TIMELINE_KINDS = ("task.created", "wake.queued", "run.started", "task.claimed",
                  "receipt.recorded", "branch.submitted", "task.changed", "run.ended")


def hq_timeline(db, task, tree, run_ids):
    """[(label, epoch seconds)] from HQ's event log, for the task's tree and its runs, in order;
    empty for a store without one. The root task's first move to done, blocked or dropped is
    also given as `terminal`: the harness's own end, which a worker's process may outlive."""
    con = sqlite3.connect(db)
    try:
        if not con.execute("SELECT 1 FROM sqlite_master WHERE name='events'").fetchone():
            return []
        rows = con.execute("SELECT at_ms, kind, data FROM events WHERE kind IN (%s) ORDER BY id"
                           % ",".join("?" * len(TIMELINE_KINDS)), TIMELINE_KINDS).fetchall()
    finally:
        con.close()
    tasks, runs = set(tree), set(run_ids)
    out, terminal = [], None
    for at, kind, data in rows:
        try:
            d = json.loads(data)
        except ValueError:
            continue
        if d.get("task") not in tasks and d.get("run") not in runs:
            continue
        label = kind
        if kind == "receipt.recorded" and d.get("tool"):
            label = f"receipt.{d['tool']}"
        out.append((label, at / 1000.0))
        if (kind == "task.changed" and d.get("task") == task and terminal is None
                and d.get("status") in ("done", "blocked", "dropped")):
            terminal = at / 1000.0
    if terminal is not None:
        out.append(("terminal", terminal))
        out.sort(key=lambda x: x[1])
    return out


def event_time(timeline, label, fallback):
    return next((t for l, t in timeline if l == label), fallback)


def with_runner_attempts(events, runs, runner_log, who):
    """The runner's own record of each attempt (its worker process, start to end) in place of
    what HQ's store can say: the store keeps one start per run, the last attempt's. Each worker
    process is agent-active time (`worker_active`: its model calls, tools and own work); before
    a run's first attempt is HQ's queue, between attempts its retry backoff."""
    attempts, _ = read_jsonl(runner_log)
    by_run = {}
    for a in attempts:
        by_run.setdefault(a.get("run"), []).append(a)
    covered = {r["id"] for r in runs if r["id"] in by_run}
    kept = [e for e in events if not (e.get("run") in covered and (
        e["kind"] == "session_start" or
        (e["kind"] == "wait" and e.get("wait_kind") in HARNESS_WAITS)))]
    prev_member = None
    for r in runs:
        agent = who(r)
        if r["id"] not in covered:
            prev_member = r["member"]
            continue
        before = _ms(r["queued_at"])
        for i, a in enumerate(sorted(by_run[r["id"]], key=lambda a: a.get("started") or 0)):
            start, end = a.get("started"), a.get("ended")
            cont = i > 0 or (prev_member is not None and r["member"] == prev_member)
            kept.append({"kind": "session_start", "t_start": start, "t_end": start,
                         "agent": agent, "continuation": cont, "run": r["id"],
                         "status": a.get("how") or "exited"})
            if is_number(before) and is_number(start) and start > before:
                kept.append({"kind": "wait", "t_start": before, "t_end": start, "agent": agent,
                             "wait_kind": "queue" if i == 0 else "retry_backoff",
                             "run": r["id"]})
            if is_number(start) and is_number(end):
                kept.append({"kind": "worker_active", "t_start": start, "t_end": end,
                             "agent": agent, "run": r["id"], "exit": a.get("exit")})
            before = end
        prev_member = r["member"]
    return kept


def final_report_of(posts, runs, transcripts, members):
    """The member's own last words: what the model said in the last transcript, then the
    member's posts since the last run began (a set_status reason becomes such a post). HQ's
    own posts (author `endora`) are not the member's words. A store without authors gives
    its last post, as before."""
    if posts and "author" not in posts[0]:
        return posts[-1]["text"]
    parts = []
    last = runs[-1] if runs else None
    if last is not None and last["id"] in transcripts:
        said = transcript_assistant_text(transcripts[last["id"]]["body"] or "")
        if said:
            parts.append(said)
    names = {m["name"] for m in members.values()}
    since = last["started_at"] if last is not None and last["started_at"] is not None else 0
    for p in posts:
        if p["author"] in names and (p["at_ms"] or 0) >= since:
            text = p["text"]
            # HQ writes a status change as "doing → in_review: <reason>".
            text = re.sub(r"^\w+ → \w+: ", "", text)
            parts.append(text)
    return "\n".join(parts)


def parse_endora(db, task, relay_log=None, family="", seed=0, runner_log=None):
    con = sqlite3.connect(db)
    con.row_factory = sqlite3.Row
    try:
        tree = [r[0] for r in con.execute(
            "WITH RECURSIVE t(id) AS (SELECT id FROM tasks WHERE id=? UNION ALL "
            "SELECT tasks.id FROM tasks JOIN t ON tasks.parent=t.id) SELECT id FROM t", (task,))]
        if not tree:
            raise SystemExit(f"task {task} not found in {db}")
        marks = ",".join("?" * len(tree))
        runs = _select(con, "runs",
                       ["id", "member", "task", "status", "attempts", "queued_at", "started_at",
                        "retry_at", "ended_at", "end_reason", "input_tokens", "cache_read_tokens",
                        "cache_write_tokens", "output_tokens"],
                       f"WHERE task IN ({marks}) ORDER BY id", tree)
        members = {m["id"]: m for m in _select(con, "members", ["id", "name", "brain"])}
        transcripts = {t["run"]: t for t in _select(con, "transcripts", ["run", "brain", "body"])}
        handovers = _select(con, "handovers", ["id", "task", "run", "at_ms"],
                            f"WHERE task IN ({marks}) ORDER BY id", tree)
        root = _select(con, "tasks", ["id", "status"], "WHERE id=?", (task,))[0]
        post_cols = [c[1] for c in con.execute("PRAGMA table_info(posts)")]
        if "author" in post_cols:
            posts = _select(con, "posts", ["text", "at_ms", "author"],
                            "WHERE task=? ORDER BY id", (task,))
        else:
            posts = _select(con, "posts", ["text", "at_ms"],
                            "WHERE task=? ORDER BY id DESC LIMIT 1", (task,))
        run_ids = [r["id"] for r in runs]
        charge_micros, charged = 0, False
        if run_ids:
            rmarks = ",".join("?" * len(run_ids))
            for r in _select(con, "ledger", ["kind", "amount"],
                             f"WHERE kind='charge' AND run IN ({rmarks})", run_ids):
                charge_micros += r["amount"]
                charged = True
    finally:
        con.close()

    def who(r):
        m = members.get(r["member"])
        return "member:" + (m["name"] if m else str(r["member"]))

    def model_of(r):
        t = transcripts.get(r["id"])
        return model_for_brain(t["brain"] if t else None) or \
            model_for_brain((members.get(r["member"]) or {}).get("brain"))

    relay = {}
    reconciliation = {"checked": False}
    if relay_log:
        calls, bad = read_jsonl(relay_log)
        for c in calls:
            if c.get("run") in set(run_ids):
                relay.setdefault(c["run"], []).append(c)
        reconciliation = {"checked": True, "parse_errors": bad, "mismatches": [], "match": True}

    events, usage_list = [], []
    prev_member = None
    for r in runs:
        agent = who(r)
        cont = prev_member is not None and r["member"] == prev_member
        queued, started, ended = _ms(r["queued_at"]), _ms(r["started_at"]), _ms(r["ended_at"])
        events.append({"kind": "session_start", "t_start": started or queued, "t_end": started or queued,
                       "agent": agent, "continuation": cont, "run": r["id"], "status": r["status"]})
        if prev_member is not None and not cont:
            events.append({"kind": "handoff", "t_start": started or queued,
                           "t_end": started or queued, "agent": agent, "source": "member_change",
                           "run": r["id"]})
        # HQ retries a failed turn as another attempt of the same run (a new worker process,
        # same member, same task): each attempt after the first is a continuation. The store
        # keeps only the last attempt's start, so these carry that time.
        for _ in range(max(0, (r["attempts"] or 0) - 1)):
            events.append({"kind": "session_start", "t_start": started or queued,
                           "t_end": started or queued, "agent": agent, "continuation": True,
                           "run": r["id"], "status": "retried_attempt"})
        prev_member = r["member"]
        if queued is not None and started is not None and started > queued:
            kind = "retry_backoff" if (r["attempts"] or 0) > 1 else "queue"
            events.append({"kind": "wait", "t_start": queued, "t_end": started, "agent": agent,
                           "wait_kind": kind, "run": r["id"]})
        elif queued is not None and started is None and r["retry_at"] is not None:
            events.append({"kind": "wait", "t_start": queued, "t_end": _ms(r["retry_at"]),
                           "agent": agent, "wait_kind": "retry_backoff", "run": r["id"]})
        tr = transcripts.get(r["id"])
        if tr:
            events.extend(dict(e, run=r["id"]) for e in transcript_tool_calls(tr["body"], agent))

        model = model_of(r)
        if r["id"] in relay:
            rows = relay[r["id"]]
            sums = {f: 0 for f in FIELDS}
            all_known = True
            for i, c in enumerate(rows):
                status = c.get("status")
                u = make_usage(f"relay:{r['id']}:{i}", agent, c.get("model") or model,
                               c.get("input_uncached"), c.get("cache_read"), c.get("cache_write"),
                               c.get("output"), retry=not _relay_ok(status), ttl_known=False,
                               status=status)
                usage_list.append(u)
                # A wait the rig injected before the call answered: the world's time, not
                # the model's; the call's own interval starts when the wait ends.
                t_call = c.get("t_start")
                if c.get("wait_kind") and is_number(c.get("wait_s")) and is_number(t_call):
                    events.append({"kind": "wait", "t_start": t_call,
                                   "t_end": t_call + c["wait_s"], "agent": agent,
                                   "wait_kind": c["wait_kind"], "run": r["id"]})
                    t_call = t_call + c["wait_s"]
                events.append({"kind": "model_call", "t_start": t_call,
                               "t_end": c.get("t_end"), "agent": agent, "run": r["id"],
                               "is_error": not _relay_ok(status)})
                for f in FIELDS:
                    if is_int(u[f]):
                        sums[f] += u[f]
                    else:
                        all_known = False
            if all_known:
                table = dict(zip(FIELDS, (r["input_tokens"], r["cache_read_tokens"],
                                          r["cache_write_tokens"], r["output_tokens"])))
                bad = {f: {"relay": sums[f], "runs_table": table[f]} for f in FIELDS
                       if sums[f] != table[f]}
                if bad:
                    reconciliation["match"] = False
                    reconciliation["mismatches"].append({"run": r["id"], "fields": bad})
            continue
        if started is None:
            continue  # never started: no model call happened
        toks = [r["input_tokens"], r["cache_read_tokens"], r["cache_write_tokens"], r["output_tokens"]]
        if relay_log and any(toks):
            reconciliation["match"] = False
            reconciliation["mismatches"].append({"run": r["id"], "reason": "runs table has tokens, relay log has no call"})
        if not any(t for t in toks):
            toks = [None] * 4  # the store cannot tell "no tokens" from "never reported"
        usage_list.append(make_usage(f"run:{r['id']}", agent, model, *toks, status=r["status"],
                                     ttl_known=False))
        events.append({"kind": "model_call", "t_start": started, "t_end": ended, "agent": agent,
                       "run": r["id"], "is_error": r["status"] in ("failed", "timed_out")})

    for h in handovers:
        t = _ms(h["at_ms"])
        events.append({"kind": "handoff", "t_start": t, "t_end": t, "agent": "member:?",
                       "source": "handover_note", "run": h["run"]})

    queued_all = [_ms(r["queued_at"]) for r in runs if r["queued_at"] is not None]
    ended_all = [_ms(r["ended_at"]) for r in runs if r["ended_at"] is not None]
    last = runs[-1] if runs else None
    status = root["status"]
    # in_review is the member's work put to HQ's gate (set_status in_review): submitted for
    # judgment, never silently a completion (the researcher, round 1).
    if status == "done":
        terminal = "done"
    elif status == "in_review":
        terminal = "submitted"
    elif status == "blocked":
        terminal = "blocked"
    elif last and last["status"] == "timed_out":
        terminal = "timeout"
    elif last and last["status"] in ("failed", "stopped"):
        terminal = "failed"
    else:
        terminal = "timeout"
    if runner_log:
        events = with_runner_attempts(events, runs, runner_log, who)
    timeline = hq_timeline(db, task, tree, run_ids)
    for (a_label, a_t), (b_label, b_t) in zip(timeline, timeline[1:]):
        # HQ's dispatch (the item made -> its turn queued) and the runner's pickup (queued ->
        # started): the harness's own waiting.
        if (a_label, b_label) in (("task.created", "wake.queued"), ("wake.queued", "run.started")):
            events.append({"kind": "wait", "t_start": a_t, "t_end": b_t, "agent": "hq",
                           "wait_kind": "queue", "source": "events"})
    events.sort(key=lambda e: (e.get("t_start") is None, e.get("t_start") or 0))
    attempt = {"arm": "endora", "family": family, "seed": seed,
               "final_report": final_report_of(posts, runs, transcripts, members),
               "dispatched_at": event_time(timeline, "task.created",
                                           min(queued_all) if queued_all else None),
               "ended_at": event_time(timeline, "terminal",
                                      max(ended_all) if ended_all else None),
               "terminal": terminal, "events": events, "usage": usage_list,
               "meta": {"task": task, "tasks_in_tree": tree, "runs": run_ids,
                        "timeline": [{"label": l, "t": t} for l, t in timeline],
                        "model_calls_observed": sum(
                            _split_kinds(t["body"]).count("reply") for t in transcripts.values()
                            if t["run"] in set(run_ids)),
                        "run_statuses": {r["id"]: r["status"] for r in runs},
                        "reconciliation": reconciliation}}
    if charged:
        attempt["actual_charge_usd"] = charge_micros / 1_000_000
    return attempt


# ----------------------------------------------------------------------------- metrics

def load_tariff(path=None):
    with open(path or DEFAULT_TARIFF, encoding="utf-8") as f:
        return json.load(f)


def _norm_model(model, models):
    if model in models:
        return model
    if isinstance(model, str):
        stripped = re.sub(r"-\d{8}$", "", model)
        if stripped in models:
            return stripped
    return None


def price_call(u, tariff):
    """USD for one fully known call, or None when usage or model price is unknown."""
    if not u.get("known"):
        return None
    key = _norm_model(u.get("model"), tariff["models"])
    if key is None:
        return None
    p = tariff["models"][key]
    w1h = min(u.get("cache_write_1h") or 0, u["cache_write"])
    if w1h and "cache_write_1h" in p:
        write = (u["cache_write"] - w1h) * p["cache_write"] + w1h * p["cache_write_1h"]
    else:
        write = u["cache_write"] * p["cache_write"]
    return (u["input_uncached"] * p["input"] + u["cache_read"] * p["cache_read"]
            + write + u["output"] * p["output"]) / 1_000_000


def attempt_metrics(a, tariff, threshold=DEFAULT_WAIT_THRESHOLD):
    usage = a.get("usage") or []
    sums = {f: None for f in FIELDS}
    for u in usage:
        for f in FIELDS:
            if is_int(u.get(f)):
                sums[f] = (sums[f] or 0) + u[f]
    complete = bool(usage) and all(u.get("known") for u in usage)
    total = None if any(v is None for v in sums.values()) else sum(sums.values())
    if total is None and any(v is not None for v in sums.values()):
        total = sum(v for v in sums.values() if v is not None)  # known part only; complete=false
    # A usage record is not a model call: live HQ keeps one total a run. The model calls are
    # what the trace observed (the transcript's replies; Claude Code's messages and helpers).
    observed = (a.get("meta") or {}).get("model_calls_observed")
    tokens = dict(sums, total=total, calls=len(usage), usage_records=len(usage),
                  model_calls=observed if is_int(observed) else len(usage),
                  unknown_calls=sum(1 for u in usage if not u.get("known")),
                  retry_calls=sum(1 for u in usage if u.get("retry")), complete=complete)

    priced = [price_call(u, tariff) for u in usage]
    norm = sum(p for p in priced if p is not None) if any(p is not None for p in priced) else None
    cash = {"normalized_usd": norm, "normalized_complete": bool(usage) and all(p is not None for p in priced),
            "actual_usd": a.get("actual_charge_usd"), "tariff_version": tariff.get("version")}
    # Cache writes whose TTL the trace does not say (HQ's) are priced both ways: a range.
    unknown_ttl = [u for u in usage if not u.get("ttl_known", True) and is_int(u.get("cache_write"))]
    cash["ttl_unknown_write_tokens"] = sum(u["cache_write"] for u in unknown_ttl)
    if norm is not None and unknown_ttl:
        as_1h = [dict(u, cache_write_1h=u["cache_write"]) if u in unknown_ttl else u for u in usage]
        high = [price_call(u, tariff) for u in as_1h]
        cash["normalized_range_usd"] = [norm, sum(p for p in high if p is not None)]
    elif norm is not None:
        cash["normalized_range_usd"] = [norm, norm]

    events = a.get("events") or []
    lo, hi = a.get("dispatched_at"), a.get("ended_at")
    elapsed, source = None, "trace"
    if lo is not None and hi is not None:
        elapsed = max(0.0, hi - lo)
    else:
        sr = (a.get("meta") or {}).get("stream_result") or {}
        if is_number(sr.get("duration_ms")):
            elapsed, source = sr["duration_ms"] / 1000.0, "stream_result"
    # The harness's own waiting (HQ's queue between ticks, its retry backoff) is the harness's
    # time: inside elapsed, reported apart, and never counted as the world's waiting, which
    # alone makes an attempt contaminated. The world's waiting is never agent-active time,
    # even inside a worker process or a model call.
    waits = [e for e in events if e.get("kind") == "wait"]
    harness = union_length([(e.get("t_start"), e.get("t_end")) for e in waits
                            if e.get("wait_kind") in HARNESS_WAITS], lo, hi)
    external = [(e.get("t_start"), e.get("t_end")) for e in waits
                if e.get("wait_kind") not in HARNESS_WAITS]
    wait = union_length(external, lo, hi)
    active = union_length_less([(e.get("t_start"), e.get("t_end")) for e in events
                                if e.get("kind") in ("model_call", "tool_call", "worker_active")],
                               external, lo, hi)
    frac = (wait / elapsed) if elapsed else None
    time = {"elapsed_s": elapsed, "elapsed_source": source, "agent_active_s": active,
            "harness_wait_s": harness,
            "external_wait_s": wait, "wait_fraction": frac, "wait_threshold": threshold,
            "contaminated": None if frac is None else frac > threshold}

    tools = [e for e in events if e.get("kind") == "tool_call"]
    term = a.get("terminal_at", hi)

    def failed(e):
        return bool(e.get("is_error")) or e.get("refused") or (is_int(e.get("exit")) and e["exit"] != 0)

    def wasted(e):
        after = term is not None and e.get("t_start") is not None and e["t_start"] > term
        return bool(e.get("is_error")) or bool(e.get("refused")) or after

    calls = {"total": len(tools), "failed": sum(1 for e in tools if failed(e)),
             "refused": sum(1 for e in tools if e.get("refused")),
             "wasted": sum(1 for e in tools if wasted(e))}
    sessions = [e for e in events if e.get("kind") == "session_start"]
    rec = (a.get("meta") or {}).get("reconciliation")
    return {"arm": a.get("arm"), "family": a.get("family"), "seed": a.get("seed"),
            "terminal": a.get("terminal"), "tokens": tokens, "cash": cash, "time": time,
            "handoffs": sum(1 for e in events if e.get("kind") == "handoff"),
            "fresh_sessions": sum(1 for e in sessions if not e.get("continuation")),
            "continuations": sum(1 for e in sessions if e.get("continuation")),
            "tool_calls": calls,
            "reconciliation_ok": None if not rec or not rec.get("checked") else rec.get("match"),
            "reconciliation": rec}


def totals(rows):
    def add(vals):
        known = [v for v in vals if v is not None]
        return sum(known) if known else None
    t = {"attempts": len(rows),
         "tokens": {f: add([r["tokens"][f] for r in rows]) for f in FIELDS + ("total", "calls", "unknown_calls", "retry_calls")},
         "cash": {"normalized_usd": add([r["cash"]["normalized_usd"] for r in rows]),
                  "actual_usd": add([r["cash"]["actual_usd"] for r in rows]),
                  "actual_attempts_reported": sum(1 for r in rows if r["cash"]["actual_usd"] is not None)},
         "time": {k: add([r["time"][k] for r in rows]) for k in ("elapsed_s", "agent_active_s", "harness_wait_s", "external_wait_s")},
         "handoffs": sum(r["handoffs"] for r in rows),
         "fresh_sessions": sum(r["fresh_sessions"] for r in rows),
         "continuations": sum(r["continuations"] for r in rows),
         "tool_calls": {k: sum(r["tool_calls"][k] for r in rows) for k in ("total", "failed", "refused", "wasted")},
         "contaminated": sum(1 for r in rows if r["time"]["contaminated"])}
    t["tokens"]["complete"] = bool(rows) and all(r["tokens"]["complete"] for r in rows)
    t["cash"]["normalized_complete"] = bool(rows) and all(r["cash"]["normalized_complete"] for r in rows)
    t["reconciliation_mismatches"] = sum(1 for r in rows if r["reconciliation_ok"] is False)
    return t


def metrics(attempts, tariff, threshold=DEFAULT_WAIT_THRESHOLD):
    rows = [attempt_metrics(a, tariff, threshold) for a in attempts]
    return {"tariff": tariff.get("version"), "wasted_rule": WASTED_RULE,
            "attempts": rows, "totals": totals(rows)}


# ----------------------------------------------------------------------------- CLI

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    cc = sub.add_parser("parse-cc")
    cc.add_argument("--session", action="append", required=True)
    cc.add_argument("--subagents")
    cc.add_argument("--stream-json")
    cc.add_argument("--family", required=True)
    cc.add_argument("--seed", type=int, required=True)
    cc.add_argument("--out", required=True)
    en = sub.add_parser("parse-endora")
    en.add_argument("--db", required=True)
    en.add_argument("--task", type=int, required=True)
    en.add_argument("--relay-log")
    en.add_argument("--runner-log")
    en.add_argument("--family", required=True)
    en.add_argument("--seed", type=int, required=True)
    en.add_argument("--out", required=True)
    me = sub.add_parser("metrics")
    me.add_argument("attempts", nargs="+")
    me.add_argument("--tariff")
    me.add_argument("--wait-threshold", type=float, default=DEFAULT_WAIT_THRESHOLD)
    args = ap.parse_args(argv)
    if args.cmd == "parse-cc":
        rec = parse_cc(args.session, args.subagents, args.stream_json, args.family, args.seed)
    elif args.cmd == "parse-endora":
        rec = parse_endora(args.db, args.task, args.relay_log, args.family, args.seed,
                           runner_log=args.runner_log)
    else:
        attempts = []
        for p in args.attempts:
            with open(p, encoding="utf-8") as f:
                attempts.append(json.load(f))
        json.dump(metrics(attempts, load_tariff(args.tariff), args.wait_threshold), sys.stdout, indent=2)
        print()
        return 0
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(rec, f, indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main())
