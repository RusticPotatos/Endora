#!/usr/bin/env python3
"""Runs one probe through Endora's real worker and a real, disposable HQ, against scripted
model replies: the integration check of the harness-probes rig (round 1 of the agentic-harness
climb). Python 3 standard library only. Costs nothing and touches no live system: HQ is
started on loopback with a fresh store in a temporary folder, and the model is a script.

What is real and what stands in:

    real      endora-hq (its engine, store, tick, MCP team protocol, runner and usage doors)
    real      endora-worker (its turn loop, shell, truncation, transcript), as its own process
    stand-in  the node's model relay: `ScriptedRelay` answers Anthropic's Messages API from a
              script, and does what the node's relay does with HQ (asks whose turn a token
              is, reports each call's usage, passes the transcript on), signed as the node
    stand-in  the runner: this script asks HQ for the next turn, starts the worker as a plain
              process (no Docker) with the environment the runner gives it, and reports
              the end the way the runner does

    python3 scripts/harness-probes/endora_local.py --family large_output --seed 0 \
        --script replies.json --out DIR [--hq-bin target/debug/endora-hq] \
        [--worker-bin target/debug/endora-worker] [--most-attempts 3]

Writes into DIR: `probe/` (the generated repo and task.json), `hq/hq.db` and `hq/hq.log`,
`relay-log.jsonl` (one line per model call, with the usage the script gave it),
`relay-raw.jsonl` (every request and reply body the relay saw, whole), `runner-log.jsonl`
(each turn handed out, the worker's exit and stderr), `observations.json` (what the publish
service saw, sidefx.py), `attempt.json` (the attempt record, from accounting.py), `grade.json` (probes.py's verdict) and `run.json` (a summary).

For `interrupted`, the worker is stopped at its first change to `interrupt_after` (reported to
HQ as a container killed from outside, 137) and the family's resume words are posted in the
task's thread as the owner's reply, which wakes the member: the same words Claude Code gets.

The script (JSON list) is one entry per model call, in order:

    {"text": "…", "tools": [{"name": "shell", "input": {"command": "…"}}],
     "usage": {"input_tokens": 1000, "cache_creation_input_tokens": 500,
               "cache_read_input_tokens": 0, "output_tokens": 80}}
    {"fail": 529}                      the relay answers 529 with no usage
    {"delay_s": 4, "wait_kind": "injected", ...a reply...}   held before answering

`{task}` in any string is replaced by HQ's task id, `{receipt}` by the last receipt id (r12)
the tool results so far named, and a list `["{receipts}"]` by all of them.
"""

import argparse
import http.server
import json
import math
import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import accounting  # noqa: E402
import rig  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

# Loopback-only tokens for a store that lives a few minutes; never a real credential.
NODE_TOKEN = "node-token-for-the-local-rig-only-000000"
RUNNER_TOKEN = "runner-token-for-the-local-rig-only-0000"
MODEL = "claude-sonnet-5-5"
# The stand-in charge, in micros a token: Sonnet 5.5's as the node's relay prices it
# (`price_of` in crates/endora-infrastructure/src/relay.rs), rounded up as its `cost` does. Only
# HQ's ledger (the "actual" charge) uses it; the rig's normalized cash comes from tariff.json.
PRICE = {"input": 2.0, "cache_write": 2.5, "cache_read": 0.10, "output": 10.0}


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def call(url, path, body=None, token=NODE_TOKEN, timeout=10):
    """A JSON call; returns (status, parsed body or text)."""
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        url + path,
        data=data,
        method="GET" if body is None else "POST",
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode()
            status = r.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        status = e.code
    try:
        return status, json.loads(raw) if raw.strip() else None
    except ValueError:
        return status, raw


class Hq:
    """A disposable HQ: its own store in `folder`, on a free loopback port."""

    def __init__(self, binary, folder, tick_seconds=1):
        self.folder = Path(folder)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HQ_DB": str(self.folder / "hq.db"),
            "HQ_ADDR": f"127.0.0.1:{self.port}",
            "HQ_TOKEN": NODE_TOKEN,
            "HQ_RUNNER_TOKEN": RUNNER_TOKEN,
            "HQ_TICK_SECONDS": str(tick_seconds),
        }
        self.log = open(self.folder / "hq.log", "w")
        self.proc = subprocess.Popen([str(binary)], env=env, stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(100):
            try:
                if call(self.url, "/health")[0] == 200:
                    return
            except OSError:
                pass
            time.sleep(0.1)
        raise RuntimeError("HQ did not start; see " + str(self.folder / "hq.log"))

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.log.close()

    def running_turns(self, task):
        """How many of the task's runs HQ's store still has running (read-only)."""
        con = sqlite3.connect(f"file:{self.folder / 'hq.db'}?mode=ro", uri=True)
        try:
            return con.execute(
                "SELECT count(*) FROM runs WHERE task=? AND status='running'", (task,)
            ).fetchone()[0]
        finally:
            con.close()

    def ok(self, path, body=None, token=NODE_TOKEN):
        status, answer = call(self.url, path, body, token)
        if status != 200:
            raise RuntimeError(f"HQ {path} answered {status}: {answer}")
        return answer


class ScriptedRelay:
    """The node's relay, played from a script: Anthropic's Messages API for the worker, and the
    node's two calls to HQ (whose turn, what it used) for each call it answers."""

    def __init__(self, hq, script, folder):
        self.hq = hq
        self.script = list(script)
        self.at = 0
        self.task = None
        self.lock = threading.Lock()
        self.log = open(Path(folder) / "relay-log.jsonl", "a")
        self.raw = open(Path(folder) / "relay-raw.jsonl", "a")
        self.unknown = []
        relay = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _body(self):
                n = int(self.headers.get("content-length") or 0)
                raw = self.rfile.read(n) if n else b""
                try:
                    return json.loads(raw) if raw else None
                except ValueError:
                    return raw.decode(errors="replace")

            def _send(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def _token(self):
                auth = self.headers.get("authorization", "")
                return auth.split(" ", 1)[1] if " " in auth else self.headers.get("x-api-key", "")

            def do_GET(self):
                if self.path == "/team-relay/contracts":
                    return self._send(200, {"tools": []})
                relay.unknown.append(("GET", self.path))
                self._send(404, {"error": "not in the rig"})

            def do_POST(self):
                body = self._body()
                if self.path == "/team-relay/v1/messages":
                    status, answer = relay.messages(self._token(), body)
                    return self._send(status, answer)
                if self.path == "/team-relay/transcript":
                    status, answer = call(
                        relay.hq.url,
                        "/v1/team/transcript",
                        {"token": self._token(), "runtime": body.get("runtime", "worker"),
                         "text": body.get("text", "")},
                    )
                    return self._send(200 if status == 200 else 502, answer or {})
                relay.unknown.append(("POST", self.path))
                self._send(404, {"error": "not in the rig"})

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.log.close()
        self.raw.close()

    def fill(self, value, receipts=()):
        if value == "{task}":
            return self.task
        if isinstance(value, str):
            value = value.replace("{task}", str(self.task))
            if receipts:
                value = value.replace("{receipt}", receipts[-1])
            return value
        if isinstance(value, list):
            filled = [self.fill(v, receipts) for v in value]
            # `["{receipts}"]` is every receipt HQ named so far, in order.
            if filled == ["{receipts}"]:
                return list(receipts)
            return filled
        if isinstance(value, dict):
            return {k: self.fill(v, receipts) for k, v in value.items()}
        return value

    @staticmethod
    def receipts_in(request):
        """The receipt ids (r12) HQ's and the worker's tool results named, in order."""
        found = []
        for message in (request or {}).get("messages", []):
            content = message.get("content")
            if not isinstance(content, list):
                continue
            for block in content:
                if block.get("type") != "tool_result":
                    continue
                text = block.get("content")
                if isinstance(text, list):
                    text = " ".join(b.get("text", "") for b in text if isinstance(b, dict))
                for rid in re.findall(r"\b(r\d+)\b", str(text)):
                    if rid not in found:
                        found.append(rid)
        return found

    def messages(self, token, body):
        # One at a time, as the node's relay takes a turn's calls.
        with self.lock:
            t_start = time.time()
            status, turn = call(self.hq.url, "/v1/team/turn", {"token": token})
            run = turn.get("run") if status == 200 and isinstance(turn, dict) else None
            if run is None:
                return 401, {"type": "error", "error": {"type": "authentication_error",
                                                        "message": "no running turn has that token"}}
            if self.at >= len(self.script):
                entry = {"fail": 500, "why": "the script ran out"}
            else:
                entry = self.fill(self.script[self.at], self.receipts_in(body))
            index = self.at
            self.at += 1
            if entry.get("delay_s"):
                time.sleep(float(entry["delay_s"]))
            if "fail" in entry:
                answer = {"type": "error", "error": {"type": "overloaded_error",
                                                     "message": entry.get("why", "scripted failure")}}
                self._note(index, run, t_start, entry, None, body, answer, int(entry["fail"]))
                return int(entry["fail"]), answer
            content = []
            if entry.get("text"):
                content.append({"type": "text", "text": entry["text"]})
            for n, tool in enumerate(entry.get("tools", [])):
                content.append({"type": "tool_use", "id": f"toolu_rig_{index}_{n}",
                                "name": tool["name"], "input": tool.get("input", {})})
            usage = dict(entry.get("usage") or {})
            # The model the answer names: the entry's `served_model`, or the alias when the entry
            # says nothing; `"served_model": null` answers naming no model (round 7).
            served = entry.get("served_model", MODEL)
            answer = {
                "id": f"msg_rig_{index}", "type": "message", "role": "assistant",
                "content": content,
                "stop_reason": "tool_use" if entry.get("tools") else "end_turn",
                "stop_sequence": None, "usage": usage,
            }
            if served is not None:
                answer["model"] = served
            known = bool(usage)
            if known:
                used = {
                    "input": int(usage.get("input_tokens", 0)),
                    "cache_read": int(usage.get("cache_read_input_tokens", 0)),
                    "cache_write": int(usage.get("cache_creation_input_tokens", 0)),
                    "output": int(usage.get("output_tokens", 0)),
                }
                micros = math.ceil(sum(used[k] * PRICE[k] for k in used))
                # As the node's relay does: the model the answer named, or null (round 7).
                call(self.hq.url, "/v1/team/usage",
                     {"run": run, "micros": micros, **used, "served_model": answer.get("model")})
            self._note(index, run, t_start, entry, usage if known else None, body, answer, 200)
            return 200, answer

    def _note(self, index, run, t_start, entry, usage, request, answer, status):
        t_end = time.time()
        line = {
            "index": index, "run": run, "t_start": t_start, "t_end": t_end, "model": MODEL,
            "status": status,
            "input_uncached": usage.get("input_tokens") if usage else None,
            "cache_read": usage.get("cache_read_input_tokens") if usage else None,
            "cache_write": usage.get("cache_creation_input_tokens") if usage else None,
            "output": usage.get("output_tokens") if usage else None,
            "wait_kind": entry.get("wait_kind"),
            "wait_s": float(entry["delay_s"]) if entry.get("delay_s") else None,
        }
        self.log.write(json.dumps(line) + "\n")
        self.log.flush()
        self.raw.write(json.dumps({"index": index, "request": request, "answer": answer}) + "\n")
        self.raw.flush()


TERMINAL = ("done", "dropped", "blocked", "in_review")


def task_status(hq, task):
    shown = hq.ok(f"/v1/tasks/{task}")
    return (shown.get("task") or shown).get("status")


def run(args):
    out = Path(args.out).resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    probe = out / "probe"
    probe_task = rig.make_probe(args.family, args.seed, probe, args.probe_dir)
    task_words = probe_task["words"]
    interrupt = probe_task.get("interrupt_after")
    interrupted = False
    script = json.loads(Path(args.script).read_text())

    hq = Hq(args.hq_bin, out / "hq")
    publish = rig.side_effects()
    relay = ScriptedRelay(hq, script, out)
    runner_log = open(out / "runner-log.jsonl", "w")
    summary = {"family": args.family, "seed": args.seed, "attempts": []}
    try:
        member = hq.ok("/v1/members", {"name": "Jane", "brain": "sonnet"})["member"]
        goal = hq.ok("/v1/goals", {"title": "Harness probe", "reason": "local rig"})["goal"]
        hq.ok("/v1/pool/top-up", {"amount_micros": 5_000_000})
        dispatched = time.time()
        task = hq.ok("/v1/tasks", {
            "goal": goal, "title": f"Probe {args.family} seed {args.seed}",
            "description": task_words, "assignee": member,
            "criteria": ["The final message ends with the RESULT and EVIDENCE lines the task asks for."],
        })["task"]
        relay.task = task
        summary.update(task=task, member=member, dispatched_at=dispatched)
        deadline = dispatched + args.timeout
        attempts = 0
        while time.time() < deadline:
            status = task_status(hq, task)
            if status in TERMINAL:
                break
            got, turn = call(hq.url, "/v1/runner/next", {"free": 1}, token=RUNNER_TOKEN)
            if got == 204:
                time.sleep(0.5)
                continue
            if got != 200:
                raise RuntimeError(f"runner/next answered {got}: {turn}")
            if attempts >= args.most_attempts:
                # Hand it back as the runner would a turn it could not take.
                call(hq.url, f"/v1/runner/{turn['run']}/ended",
                     {"succeeded": False, "lease": turn["lease"], "code": None,
                      "why": "the rig's attempt limit"}, token=RUNNER_TOKEN)
                break
            attempts += 1
            env = {
                "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                "HOME": str(out / "home"),
                "ENDORA_RUN_TOKEN": turn["token"],
                "ENDORA_HQ_MCP": f"{hq.url}/v2/team/mcp",
                "ENDORA_MODEL_URL": f"{relay.url}/team-relay/v1",
                "ENDORA_MODEL": turn["brain"],
                "ENDORA_PROMPT_BUDGET": str(turn["prompt_budget"]),
                "ENDORA_BUDGET_COUNTS": turn["budget_counts"],
            }
            if turn.get("shell"):
                env["ENDORA_WORKBENCH"] = "1"
                env["ENDORA_WORKBENCH_DIR"] = str(probe / "repo")
            (out / "home").mkdir(exist_ok=True)
            started = time.time()
            env["PROBE_PUBLISH_URL"] = publish.url
            code, how, stderr = rig.run_watched(
                [str(args.worker_bin)], cwd=probe / "repo", env=env,
                timeout=max(1.0, deadline - time.time()),
                interrupt_file=(probe / "repo" / interrupt) if interrupt and not interrupted else None,
                interrupt_when=probe_task.get("interrupt_milestone"),
            )
            ended = time.time()
            why = stderr.strip()[-600:]
            if how != "exited":
                # Killed from outside, as a container stopped by the runner: 137.
                code, why = 137, f"stopped by the rig ({how})"
            told = call(hq.url, f"/v1/runner/{turn['run']}/ended",
                        {"succeeded": code == 0, "lease": turn["lease"],
                         "code": code, "why": why or None}, token=RUNNER_TOKEN)
            if how == "interrupted":
                interrupted = True
                # The family's one intervention: the same resume words Claude Code is given,
                # as the owner's reply in the task's thread, which wakes whoever has it.
                call(hq.url, f"/v1/tasks/{task}/post", {"text": probe_task["resume_words"]})
            record = {"run": turn["run"], "lease": turn["lease"], "ended_told": told,
                      "started": started,
                      "ended": ended, "exit": code, "how": how, "stderr": stderr}
            runner_log.write(json.dumps(record) + "\n")
            runner_log.flush()
            summary["attempts"].append({k: record[k] for k in ("run", "started", "ended", "exit")})
        # HQ applies a reported end on its next tick: wait for it before reading the store.
        settle = time.time() + 10
        while time.time() < settle and hq.running_turns(task):
            time.sleep(0.25)
        summary["status"] = task_status(hq, task)
        summary["ended_at"] = time.time()
        summary["script_used"] = relay.at
        summary["script_len"] = len(script)
        summary["unknown_relay_paths"] = relay.unknown
    finally:
        relay.close()
        hq.stop()
        runner_log.close()
    attempt = accounting.parse_endora(out / "hq" / "hq.db", summary["task"],
                                      relay_log=out / "relay-log.jsonl",
                                      family=args.family, seed=args.seed,
                                      runner_log=out / "runner-log.jsonl")
    # Time runs from dispatch (the task given to HQ) to the terminal outcome the rig saw.
    attempt["dispatched_at"] = summary["dispatched_at"]
    attempt["ended_at"] = summary["ended_at"]
    attempt_path = out / "attempt.json"
    attempt_path.write_text(json.dumps(attempt, indent=2) + "\n")
    observations = rig.keep_observations(publish, out / "observations.json")
    verdict = rig.grade(args.family, args.seed, probe, attempt_path, observations, publish.url)
    publish.close()
    (out / "grade.json").write_text(json.dumps(verdict, indent=2) + "\n")
    summary["pass"] = verdict.get("pass")
    (out / "run.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--family", required=True)
    p.add_argument("--seed", type=int, required=True)
    p.add_argument("--script", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--hq-bin", default=str(ROOT / "target/debug/endora-hq"))
    p.add_argument("--worker-bin", default=str(ROOT / "target/debug/endora-worker"))
    p.add_argument("--probe-dir", help="a probe generated already (copied, not generated)")
    p.add_argument("--most-attempts", type=int, default=3)
    p.add_argument("--timeout", type=float, default=600)
    return run(p.parse_args(argv))


if __name__ == "__main__":
    sys.exit(main())
