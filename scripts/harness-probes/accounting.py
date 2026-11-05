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
import shlex
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
    # Every invocation's stream, in order (round 16: a killed invocation's stream can hold calls
    # its session never saved); one path is still accepted.
    # Each item is a path, or (path, the time its invocation ended), which its records take,
    # streams carrying no times of their own.
    stream_recs = []
    streams = [stream_json] if isinstance(stream_json, (str, Path)) else list(stream_json or [])
    for one in streams:
        path, at = one if isinstance(one, tuple) else (one, None)
        recs, bad = read_jsonl(path)
        stream_recs += [(r, at) for r in recs]
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
    session_msgs, session_results = set(msgs), set(tool_results)
    for r, at in stream_recs:
        if r.get("type") == "result":
            stream_result = r
            results_seen += 1
            continue
        default = None
        if r.get("parent_tool_use_id"):
            default = str(r["parent_tool_use_id"])
        before = set(msgs)
        mid = (r.get("message") or {}).get("id") if isinstance(r.get("message"), dict) else None
        uses = [b.get("tool_use_id") for b in ((r.get("message") or {}).get("content") or [])
                if isinstance(b, dict) and b.get("type") == "tool_result"] if isinstance(r.get("message"), dict) \
            and isinstance((r.get("message") or {}).get("content"), list) else []
        known = (mid in session_msgs) if r.get("type") == "assistant" else \
            bool(uses) and all(u in session_results for u in uses)
        feed(r, default, None if known else at)
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
        if isinstance(stream_result.get("modelUsage"), dict):
            meta["model_usage"] = stream_result["modelUsage"]
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


# Waits that are the harness's own doing, not the world's: never contamination. "recovery" is
# HQ's wait between a turn it reports interrupted (run.ended, retry_waiting) and that run's next
# run.started (settlement, news or retry deadline, admission).
HARNESS_WAITS = ("queue", "retry_backoff", "recovery")
# A wait the agent asked for itself (a `sleep` in its own command): inside elapsed, apart from
# both the harness's waiting and the world's, and never contamination.
AGENT_WAITS = ("agent_sleep",)

_SLEEP_UNITS = {"": 1.0, "s": 1.0, "m": 60.0, "h": 3600.0, "d": 86400.0}
_SLEEP_ARG = re.compile(r"^(\d+(?:\.\d*)?|\.\d+)([smhd]?)$")
_PY_SLEEP = re.compile(r"^time\.sleep\(\s*(\d+(?:\.\d*)?|\.\d+)\s*\)$")
_PY_COMPOUND = re.compile(r"^(if|for|while|def|try|with|class|else|elif|except|finally|lambda)\b")
_HEREDOC = re.compile(r"<<-?\s*(['\"]?)([A-Za-z_]\w*)\1")
_LEAD_WORDS = ("sudo", "env", "command", "time", "nohup")
_CONDITIONAL_OPENERS = ("if", "then", "else", "elif", "do", "while", "until", "!")
_COMPOUND_WORDS = ("for", "case", "select", "function", "fi", "done", "esac")


def _shell_units(cmd):
    """[(text, operator before it, backgrounded)] for a shell command, split on newlines, `;`,
    `&`, `&&`, `||` and `|`. Quote-aware (single, double, backtick, $( ) kept whole), `#` comments
    dropped, and heredoc bodies skipped (their text is data, not commands). Conservative by
    design: it only has to tell a command that runs from text that merely mentions one."""
    units, cur, op = [], [], None
    i, n, quote, depth = 0, len(cmd), None, 0
    bodies = []  # heredoc delimiters opened on the current line

    def flush(next_op, bg=False):
        nonlocal op
        text = "".join(cur).strip()
        cur.clear()
        if text:
            units.append((text, op, bg))
            op = next_op
        elif next_op in ("&&", "||", "|"):
            op = next_op

    while i < n:
        ch = cmd[i]
        if quote:
            cur.append(ch)
            if ch == "\\" and quote == '"' and i + 1 < n:
                cur.append(cmd[i + 1])
                i += 2
                continue
            if ch == quote:
                quote = None
            i += 1
            continue
        if ch == "\\" and i + 1 < n:
            cur.append(cmd[i:i + 2])
            i += 2
            continue
        if ch in "'\"`":
            quote = ch
            cur.append(ch)
            i += 1
            continue
        if cmd.startswith("$(", i):
            depth += 1
            cur.append("$(")
            i += 2
            continue
        if depth:
            depth += (ch == ")") * -1 + (ch == "(")
            cur.append(ch)
            i += 1
            continue
        if ch == "#" and (not cur or "".join(cur)[-1:].isspace()):
            end = cmd.find("\n", i)
            i = n if end < 0 else end
            continue
        if cmd.startswith("<<", i) and not cmd.startswith("<<<", i):
            m = _HEREDOC.match(cmd, i)
            if m:
                bodies.append(m.group(2))
                cur.append(m.group(0))
                i = m.end()
                continue
        if ch == "\n":
            flush(None)
            i += 1
            for delim in bodies:
                while i < n:
                    end = cmd.find("\n", i)
                    end = n if end < 0 else end
                    line = cmd[i:end]
                    i = min(n, end + 1)
                    if line.strip() == delim:
                        break
            bodies = []
            continue
        two = cmd[i:i + 2]
        if two in ("&&", "||"):
            flush(two)
            i += 2
            continue
        if ch == "|":
            flush("|")
            i += 1
            continue
        if ch == ";":
            flush(None)
            i += 1
            continue
        if ch == "&" and not (i > 0 and cmd[i - 1] in "<>") and not cmd.startswith("&>", i):
            flush(None, bg=True)
            i += 1
            continue
        cur.append(ch)
        i += 1
    flush(None)
    return units


def executed_sleeps(command):
    """[(seconds, unconditional)] for the `sleep N` (or `time.sleep(N)` in a `python3 -c`) a shell
    command actually executes, in command order. Not counted: text inside quotes, comments and
    heredoc bodies; anything after an unconditional `exit`; a backgrounded sleep; an argument that
    is not a plain number. `unconditional` is False when the sleep sits behind `&&`/`||` or inside
    an if/while/for/case/brace/subshell, where the command may not reach it; a caller then
    needs timing evidence that it ran. A deliberately small parser, not a shell."""
    if not isinstance(command, str) or not command.strip():
        return []
    out, nested = [], False
    for text, op, bg in _shell_units(command):
        try:
            words = shlex.split(text)
        except ValueError:
            words = text.split()
        guarded = nested or op in ("&&", "||")
        while words and (words[0] in _CONDITIONAL_OPENERS or words[0] in _LEAD_WORDS
                         or re.match(r"^\w+=", words[0])):
            if words[0] in _CONDITIONAL_OPENERS:
                nested = guarded = True
            words = words[1:]
        if not words:
            continue
        head = words[0]
        if head in _COMPOUND_WORDS or head.startswith(("{", "(")) or head.endswith("()"):
            nested = True
            continue
        if head in ("exit", "return"):
            if not guarded:
                break
            continue
        if bg:
            continue
        if head == "sleep" and len(words) > 1:
            secs = []
            for arg in words[1:]:
                m = _SLEEP_ARG.match(arg)
                if not m:
                    secs = []
                    break
                secs.append(float(m.group(1)) * _SLEEP_UNITS[m.group(2)])
            if secs and sum(secs) > 0:
                out.append((sum(secs), not guarded))
        elif re.match(r"^python[\d.]*$", head) and "-c" in words[1:-1]:
            code = words[words.index("-c") + 1]
            lines = [ln for ln in code.split("\n") if ln.strip()]
            stmts = [s for ln in lines for s in ln.split(";") if s.strip()]
            compound = any(ln[:1].isspace() or _PY_COMPOUND.match(ln.strip()) for ln in lines)
            for s in stmts:
                m = _PY_SLEEP.match(s.strip())
                if m and float(m.group(1)) > 0:
                    out.append((float(m.group(1)), not (guarded or compound)))
    return out


def agent_requested_waits(events, taken=()):
    """([(t_start, t_end)], notes) for the waits the agent asked for in its own tool calls. A
    sleep is attributed only with execution evidence: it is in the command's executed sleeps
    (executed_sleeps) and the call's own t_start/t_end span covers at least that many seconds.
    Each call attributes min(sum of those sleeps, its span), placed in the part of its span that
    no interval in `taken` (harness and external waits) covers, earliest first: only the total is
    a claim, the position inside the call is not observable. With no timing on the call, or a
    span shorter than the sleep, nothing is attributed and a note says why."""
    spans, notes = [], []
    cut = merge_intervals(taken)
    for i, e in enumerate(events):
        if e.get("kind") != "tool_call":
            continue
        sleeps = executed_sleeps(e.get("command"))
        if not sleeps:
            continue
        t0, t1 = e.get("t_start"), e.get("t_end")
        label = {"tool": e.get("tool"), "run": e.get("run"), "event_index": i}
        if not (is_number(t0) and is_number(t1) and t1 >= t0):
            notes.extend(dict(label, seconds=n, unconditional=u, attributed=False,
                              why="no timing: the tool call has no t_start/t_end")
                         for n, u in sleeps)
            continue
        duration = t1 - t0
        evidenced = [(n, u) for n, u in sleeps if duration >= n]
        notes.extend(dict(label, seconds=n, unconditional=u, attributed=(n, u) in evidenced,
                          why="" if (n, u) in evidenced else
                          "the call's span (%.2f s) is shorter than the sleep" % duration)
                     for n, u in sleeps)
        if evidenced:
            left, at = min(sum(n for n, _ in evidenced), duration), t0
            for cs, ce in cut + [(t1, t1)]:
                if ce <= at:
                    continue
                gap = min(max(0.0, cs - at), left)
                if gap > 0:
                    spans.append((at, at + gap))
                    left -= gap
                at = max(at, ce)
                if left <= 0 or at >= t1:
                    break
    return spans, notes


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


SCHEDULING_KINDS = ("wake.queued", "wake.folded", "run.started", "run.ended")


def hq_scheduling(db, tree, run_ids):
    """[(kind, epoch seconds, data)] of HQ's scheduling events for the task tree and its runs, in
    store order; empty for a store without an event log."""
    con = sqlite3.connect(db)
    try:
        if not con.execute("SELECT 1 FROM sqlite_master WHERE name='events'").fetchone():
            return []
        rows = con.execute("SELECT at_ms, kind, data FROM events WHERE kind IN (%s) ORDER BY id"
                           % ",".join("?" * len(SCHEDULING_KINDS)), SCHEDULING_KINDS).fetchall()
    finally:
        con.close()
    tasks, runs, out = set(tree), set(run_ids), []
    for at, kind, data in rows:
        try:
            d = json.loads(data)
        except ValueError:
            continue
        if isinstance(d, dict) and (d.get("task") in tasks or d.get("run") in runs):
            out.append((kind, at / 1000.0, d))
    return out


def hq_recovery_waits(sched, runs):
    """(wait events, meta) for HQ's scheduling waits, from the event sequence and never from a
    reused run row. HQ retries an interrupted turn on the SAME run row (attempts 1 -> 2, started_at
    overwritten), so the row's queued_at -> started_at spans the whole first attempt.

    A recovery runs from a run.ended with result retry_waiting to that run's next run.started. It
    is split at the first sign HQ had what it waited for: an owner reply folded into the wake
    (wake.folded), a wake queued, or the retry deadline (a wake's delayed_until, else the row's
    retry_at); before it, `news_or_deadline`, after it, `admission`. Settlement (the runner's
    report reaching HQ) is not observable in the store: a recovery starts at run.ended. A reply
    folded before run.ended is `reply_first` (the wait is then the retry deadline); after it,
    `report_first`. A retry with no later run.started leaves its interval unaccounted. Runs with
    no run.started/run.ended event are `rows_fallback_runs`: they keep the row's own span."""
    events, recoveries, covered = [], [], set()
    for r in runs:
        rid = r["id"]
        starts = sorted(at for kind, at, d in sched if kind == "run.started" and d.get("run") == rid)
        ends = [(at, d) for kind, at, d in sched if kind == "run.ended" and d.get("run") == rid]
        if not starts and not ends:
            continue
        covered.add(rid)
        folds = [at for kind, at, d in sched if kind == "wake.folded"]
        wakes = [(at, d) for kind, at, d in sched if kind == "wake.queued"]
        for e_at, d in ends:
            if d.get("result") != "retry_waiting":
                continue
            after = [s for s in starts if s > e_at]
            before = [s for s in starts if s <= e_at]
            prev = max(before) if before else None
            reply_first = any((prev is None or prev < f < e_at) for f in folds)
            rec = {"run": rid, "ended_at": e_at, "order": "reply_first" if reply_first else "report_first",
                   "news_at": None, "started_at": None, "status": "missing_run_started",
                   "settlement": "not observable in HQ's store: the recovery starts at run.ended"}
            recoveries.append(rec)
            if not after:
                continue
            r_at = min(after)
            rec["started_at"] = r_at
            cand = [f for f in folds if e_at < f <= r_at]
            for w_at, wd in wakes:
                if not e_at < w_at <= r_at:
                    continue
                until = wd.get("delayed_until")
                cand.append(until / 1000.0 if is_number(until) and e_at < until / 1000.0 <= r_at else w_at)
            retry_at = _ms(r.get("retry_at"))
            if retry_at is not None and e_at < retry_at <= r_at:
                cand.append(retry_at)
            rec["status"] = "measured"
            if cand:
                rec["news_at"] = min(cand)
                parts = [("news_or_deadline", e_at, rec["news_at"]), ("admission", rec["news_at"], r_at)]
            else:
                parts = [("recovery", e_at, r_at)]
            events.extend({"kind": "wait", "t_start": a, "t_end": b, "agent": "hq",
                           "wait_kind": "recovery", "part": part, "source": "events",
                           "recovery_of_run": rid} for part, a, b in parts if b >= a)
    fallback = [r["id"] for r in runs if r["id"] not in covered]
    meta = {"source": "events" if covered and not fallback else ("run_rows" if not covered else "mixed"),
            "recoveries": recoveries, "rows_fallback_runs": fallback,
            "note": "scheduling waits per attempt from HQ's event sequence; a reused run row's "
                    "queued_at -> started_at is used only for runs with no such events"}
    return events, meta, covered


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


def node_census_rows(path, run_ids):
    """The node's own request census (its `relay_requests` rows, exported as a JSON list), for
    the attempt's runs only (round 9)."""
    with open(path, encoding="utf-8") as f:
        rows = json.load(f)
    runs = set(run_ids)
    return [r for r in rows if r.get("run") in runs]


def census_of(relay):
    """The rig's relay log as a census of the attempt's model requests: every request it saw,
    answered (2xx) or failed (round 8)."""
    rows = [c for calls in relay.values() for c in calls]
    answered = sum(1 for c in rows if _relay_ok(c.get("status")))
    return {"source": "relay-log", "requests": len(rows), "answered": answered,
            "failed": len(rows) - answered}


def served_models_of(db, run_ids):
    """The models that answered the runs' calls, as the provider named them, with their calls;
    `None` (written "") for calls whose model is not known. Empty for a store from before HQ
    kept them (the climb, round 7): nothing known, never the alias in its place."""
    con = sqlite3.connect(db)
    try:
        if not con.execute("SELECT 1 FROM sqlite_master WHERE name='run_served_models'").fetchone():
            return []
        marks = ",".join("?" * len(run_ids)) or "NULL"
        rows = con.execute(f"SELECT model, sum(calls) FROM run_served_models WHERE run IN ({marks}) "
                           "GROUP BY model ORDER BY min(rowid)", run_ids).fetchall()
    finally:
        con.close()
    return [[m or None, int(n)] for m, n in rows]


def model_identity(a):
    """Which models were asked for, which the provider said answered, and whether those names
    account for every call an independent census saw (round 8). "Every reported call named" is
    not "every call accounted for": a lost or duplicated report leaves no unknown row.

    - Endora: the names are HQ's `run_served_models`; the census is the relay log's requests
      (the rig's; live HQ has none, so completeness stays unproven there). Every answered
      request must be accounted for exactly once; failed requests answered nothing.
    - Claude Code: the names are its messages' and helpers'; the census is the result's
      `modelUsage`, the billing summary by model. Every billed model must be named by the
      records and every record's model billed.
    Without a census, `accounted` is `None` and the identity is not complete. A requested alias
    never stands in for a name."""
    meta = a.get("meta") or {}
    if a.get("arm") == "endora" and meta.get("node_census") is not None:
        return node_census_identity(meta)
    if a.get("arm") == "endora":
        named = meta.get("served_models") or []
        requested = meta.get("requested_models") or []
        served = {m: n for m, n in named if m}
        unknown = sum(n for m, n in named if not m)
        census = meta.get("census")
        reported = sum(n for _, n in named)
        accounted = None if not census else reported == census.get("answered")
    else:
        counts = {}
        for u in a.get("usage") or []:
            key = u.get("model") or None
            counts[key] = counts.get(key, 0) + 1
        requested = [meta["requested_model"]] if meta.get("requested_model") else []
        served = {m: n for m, n in counts.items() if m}
        unknown = counts.get(None, 0)
        billed = meta.get("model_usage")
        census = None
        accounted = None
        if isinstance(billed, dict):
            census = {"source": "modelUsage", "models": sorted(billed), "answered": None}
            accounted = set(billed) == set(served)
    out = {"requested": requested, "served": served, "unknown_calls": unknown,
           "census": census, "accounted": accounted,
           "single_model": len(served) == 1,
           "complete": accounted is True and unknown == 0 and bool(served)}
    if a.get("arm") != "endora":
        out["usage_coverage"] = usage_coverage(a.get("usage") or [], meta.get("model_usage"))
    return out


# Claude Code's billing summary (`modelUsage`) by token kind, against the record's fields.
BILLED_KINDS = {"input_uncached": "inputTokens", "output": "outputTokens",
                "cache_read": "cacheReadInputTokens", "cache_write": "cacheCreationInputTokens"}


def usage_coverage(usage, billed):
    """Claude Code's usage coverage (round 9): the usage its observed records carry, by model
    and token kind, against its billing summary. Records inferred from the summary itself (a
    helper seen only there) are not observed calls, so usage lost from a model the records
    already name shows as unobserved, never hidden by the name. This is coverage of usage, not
    a census of requests. `complete` is `None` without a summary."""
    if not isinstance(billed, dict):
        return {"complete": None, "unobserved": None}
    observed = [u for u in usage if u.get("status") != "from_model_usage"]
    unobserved = {}
    for model, seen in billed.items():
        if not isinstance(seen, dict):
            continue
        for field, key in BILLED_KINDS.items():
            have = sum(u[field] for u in observed if u.get("model") == model and is_int(u.get(field)))
            if is_int(seen.get(key)) and seen[key] != have:
                unobserved.setdefault(model, {})[field] = seen[key] - have
    return {"complete": not unobserved, "unobserved": unobserved}


def node_census_identity(meta):
    """Endora's identity from the node's own request census (round 9): every request the relay
    passed on, how it ended and the model that answered, kept by the node apart from HQ. Failed
    requests (no model response) need no name; answered and interrupted ones each need one; an
    unfinished observation leaves it incomplete. What HQ recorded is compared, not trusted: a
    usage report HQ never received shows as a discrepancy."""
    rows = meta.get("node_census") or []
    by = {}
    for r in rows:
        by.setdefault(r.get("outcome"), []).append(r)
    charged = by.get("answered", []) + by.get("interrupted", [])
    served, unknown = {}, 0
    for r in charged:
        if r.get("served_model"):
            served[r["served_model"]] = served.get(r["served_model"], 0) + 1
        else:
            unknown += 1
    unfinished = len(by.get("unfinished", []))
    hq_recorded = sum(n for _, n in meta.get("served_models") or [])
    census = {"source": "node-relay", "requests": len(rows), "answered": len(by.get("answered", [])),
              "interrupted": len(by.get("interrupted", [])), "failed": len(by.get("failed", [])),
              "unfinished": unfinished}
    # The observer is best-effort (round 10): HQ recording more calls than the census saw means
    # the census missed some, so it cannot vouch for every call.
    accounted = unfinished == 0 and hq_recorded <= len(charged)
    return {"requested": meta.get("requested_models") or [], "served": served,
            "unknown_calls": unknown, "census": census, "accounted": accounted,
            "single_model": len(served) == 1,
            "complete": accounted and unknown == 0 and bool(served),
            "hq_discrepancy": {"census_charged": len(charged), "hq_recorded": hq_recorded,
                               "hq_not_told": sum(1 for r in charged if r.get("hq_told") == 0)}}


def identity_equivalence(a, b):
    """Whether two attempts are shown to have run on one and the same model: `match` when both
    identities are complete (every call accounted for against a census) and both name the same
    single model; `mixed` when either ran on more than one (ineligible for a single-model
    comparison); `mismatch` when each ran on one, but not the same; `incomplete` otherwise. A
    requested alias is never evidence."""
    if not (a.get("complete") and b.get("complete")):
        return {"verdict": "incomplete", "a": a, "b": b}
    if len(a["served"]) != 1 or len(b["served"]) != 1:
        return {"verdict": "mixed", "a": a, "b": b}
    same = set(a["served"]) == set(b["served"])
    return {"verdict": "match" if same else "mismatch", "a": a, "b": b}


def native_verification_of(posts):
    """How HQ itself closed the task (round 14), from its own posts: `verified` when HQ's checks
    passed ("done, with no review" on a solo loop: HQ's mechanical checks, no reviewer),
    `not_verified` when HQ closed it "completed, not verified", else `None`. Kept apart from
    the harness's terminal state and from the independent grade."""
    for p in reversed(posts or []):
        if p.get("author") != "endora":
            continue
        text = p.get("text", "")
        if "completed, not verified" in text:
            return "not_verified"
        if "HQ's checks passed" in text:
            return "verified"
    return None


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


def parse_endora(db, task, relay_log=None, family="", seed=0, runner_log=None, node_census=None):
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
    sched_events, hq_waits_meta, event_runs = hq_recovery_waits(hq_scheduling(db, tree, run_ids), runs)
    events.extend(sched_events)
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
        if r["id"] in event_runs:
            pass  # its waits come from HQ's event sequence, never from a reused row's span
        elif queued is not None and started is not None and started > queued:
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
    _, requested_notes = agent_requested_waits(events)
    attempt = {"arm": "endora", "family": family, "seed": seed,
               "final_report": final_report_of(posts, runs, transcripts, members),
               "dispatched_at": event_time(timeline, "task.created",
                                           min(queued_all) if queued_all else None),
               "ended_at": event_time(timeline, "terminal",
                                      max(ended_all) if ended_all else None),
               "terminal": terminal, "events": events, "usage": usage_list,
               "meta": {"task": task, "tasks_in_tree": tree, "runs": run_ids,
                        "timeline": [{"label": l, "t": t} for l, t in timeline],
                        "hq_waits": hq_waits_meta,
                        **({"agent_requested_wait_notes": requested_notes}
                           if requested_notes else {}),
                        "model_calls_observed": sum(
                            _split_kinds(t["body"]).count("reply") for t in transcripts.values()
                            if t["run"] in set(run_ids)),
                        "run_statuses": {r["id"]: r["status"] for r in runs},
                        "requested_models": sorted({m for m in (model_of(r) for r in runs) if m}),
                        "native_verification": native_verification_of(
                            posts if posts and "author" in posts[0] else []),
                        "served_models": served_models_of(db, run_ids),
                        **({"census": census_of(relay)} if relay_log else {}),
                        **({"node_census": node_census_rows(node_census, run_ids)}
                           if node_census else {}),
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
    harness_spans = [(e.get("t_start"), e.get("t_end")) for e in waits
                     if e.get("wait_kind") in HARNESS_WAITS]
    harness = union_length(harness_spans, lo, hi)
    external = [(e.get("t_start"), e.get("t_end")) for e in waits
                if e.get("wait_kind") not in HARNESS_WAITS + AGENT_WAITS]
    # The parts are exclusive, taken in this order: the harness's waiting, the world's, the
    # agent's own requested waits (a `sleep` it ran, with timing evidence), then agent-active
    # time. A second claimed on the same seconds is not counted twice.
    wait = union_length_less(external, harness_spans, lo, hi)
    sleep_spans, sleep_notes = agent_requested_waits(events, harness_spans + external)
    sleep_spans += [(e.get("t_start"), e.get("t_end")) for e in waits
                    if e.get("wait_kind") in AGENT_WAITS]
    requested = union_length_less(sleep_spans, harness_spans + external, lo, hi)
    # Waiting of any kind is never agent-active time: a span HQ reports as one model call
    # can cover its own queue (round 18), and the parts must not overlap.
    active = union_length_less([(e.get("t_start"), e.get("t_end")) for e in events
                                if e.get("kind") in ("model_call", "tool_call", "worker_active")],
                               external + harness_spans + sleep_spans, lo, hi)
    frac = (wait / elapsed) if elapsed else None
    parts = [active, harness, requested, wait]
    unaccounted = (max(0.0, elapsed - sum(parts))
                   if elapsed is not None and all(is_number(x) for x in parts) else None)
    time = {"elapsed_s": elapsed, "elapsed_source": source, "agent_active_s": active,
            "harness_wait_s": harness, "agent_requested_wait_s": requested,
            "unaccounted_s": unaccounted,
            "external_wait_s": wait, "wait_fraction": frac, "wait_threshold": threshold,
            "contaminated": None if frac is None else frac > threshold}
    if sleep_notes:
        time["agent_requested_wait_notes"] = sleep_notes

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
            "reconciliation": rec, "model_identity": model_identity(a)}


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
    out = {"tariff": tariff.get("version"), "wasted_rule": WASTED_RULE,
           "attempts": rows, "totals": totals(rows)}
    # A pair of the two arms: whether they are shown to have run on the same model.
    if len(rows) == 2 and rows[0]["arm"] != rows[1]["arm"]:
        out["identity"] = identity_equivalence(rows[0]["model_identity"],
                                               rows[1]["model_identity"])
    return out


# ----------------------------------------------------------------------------- CLI

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    cc = sub.add_parser("parse-cc")
    cc.add_argument("--session", action="append", required=True)
    cc.add_argument("--subagents")
    cc.add_argument("--stream-json", action="append", help="each invocation's stream, in order (repeat)")
    cc.add_argument("--family", required=True)
    cc.add_argument("--seed", type=int, required=True)
    cc.add_argument("--out", required=True)
    en = sub.add_parser("parse-endora")
    en.add_argument("--db", required=True)
    en.add_argument("--task", type=int, required=True)
    en.add_argument("--relay-log")
    en.add_argument("--runner-log")
    en.add_argument("--node-census", help="the node's relay_requests rows, as a JSON list")
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
                           runner_log=args.runner_log, node_census=args.node_census)
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
