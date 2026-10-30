#!/usr/bin/env python3
"""Benchmark an OpenAI-compatible model server on HQ's workload, and compare two runs.

Measures what decides whether a machine (or a model on it) can hold a seat on the team:

* speed, on prompts the size of HQ's turns (small, typical, large): time to the first token,
  prompt reading rate, writing rate and the whole turn, each the median of several runs;
* tool calls, on fixed cases whose right answer is known: the tool's name and its required
  arguments, scored exactly (a task number is an integer, as HQ requires).

Run it once per model, then compare the two result files:

    python3 scripts/bench-model.py --url http://192.168.1.10:8080/v1 --model big \\
        --prompts scripts/bench-cases.json --out big.json [--key-file ~/.config/x/key]
    python3 scripts/bench-model.py --compare big.json small.json

The comparison applies the owner's rule for replacing A with B: B finishes a large turn in
half A's time or less, and misses at most one more tool case than A.

The key, if the server needs one, is read from a file and sent only as the Authorization
header. It is never taken on the command line, printed, or written to the results.

Python 3 standard library only.
"""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import sys
import time
import urllib.error
import urllib.request
import uuid

DEFAULT_RUNS = 3
DEFAULT_TIMEOUT_S = 900
DEFAULT_CHARS_PER_TOKEN = 3.5
# The owner's rule (30 Oct): switch only at half the time or less, and no more than one
# extra tool-call miss.
SPEED_FACTOR = 2.0
EXTRA_MISSES_ALLOWED = 1


# ---------------------------------------------------------------------------- cases


def load_cases(path):
    """The cases file, with each prompt case's messages built and its digest recorded."""
    path = Path(path)
    raw = path.read_bytes()
    cases = json.loads(raw)
    prompts = []
    for case in cases.get("prompts", []):
        prompts.append(build_prompt_case(case, path.parent))
    return {
        "digest": hashlib.sha256(raw).hexdigest()[:16],
        "prompts": prompts,
        "tool_system": cases.get("tool_system", ""),
        "tool_definitions": cases.get("tool_definitions", []),
        "tools": cases.get("tools", []),
    }


def build_prompt_case(case, base):
    """One prompt case as {name, messages, tools, max_tokens}.

    A case gives its messages in one of two ways:
    * `messages_file`: a JSON file (relative to the cases file) holding a real captured
      request, either a list of messages or an object with `messages` and optional `tools`;
    * `messages` with a `{fill}` placeholder in one of them, and `fill`: log-line templates
      repeated (with `{n}` and `{ts}` filled in, so no two lines are the same) until the
      text reaches about `target_tokens`.
    """
    built = {"name": case["name"], "max_tokens": int(case.get("max_tokens", 300)), "tools": None}
    if "messages_file" in case:
        captured = json.loads((base / case["messages_file"]).read_text())
        if isinstance(captured, list):
            built["messages"] = captured
            return built
        built["messages"] = captured["messages"]
        built["tools"] = captured.get("tools")
        return built
    fill = case.get("fill")
    text = fill_text(fill) if fill else ""
    built["messages"] = [
        {"role": m["role"], "content": m["content"].replace("{fill}", text)}
        for m in case["messages"]
    ]
    return built


def fill_text(fill):
    """Log lines from templates, cycled until about `target_tokens` tokens long."""
    target_chars = int(fill["target_tokens"] * fill.get("chars_per_token", DEFAULT_CHARS_PER_TOKEN))
    templates = fill["lines"]
    out = []
    size = 0
    n = 0
    while size < target_chars:
        template = templates[n % len(templates)]
        seconds = n * 7
        ts = "2026-10-01T%02d:%02d:%02dZ" % ((seconds // 3600) % 24, (seconds // 60) % 60, seconds % 60)
        line = template.replace("{n}", str(n + 1)).replace("{ts}", ts)
        out.append(line)
        size += len(line) + 1
        n += 1
    return "\n".join(out)


def estimate_tokens(messages):
    """A rough count, used only when the server reports none."""
    chars = sum(len(m.get("content") or "") for m in messages)
    return max(1, round(chars / DEFAULT_CHARS_PER_TOKEN))


# ---------------------------------------------------------------------------- server


class Server:
    """Posts chat completions to an OpenAI-compatible base URL."""

    def __init__(self, url, model, key=None, timeout=DEFAULT_TIMEOUT_S):
        self.endpoint = url.rstrip("/") + "/chat/completions"
        self.model = model
        self._key = key
        self.timeout = timeout

    def __repr__(self):
        # Never let the key reach a log line by way of a repr.
        return f"Server({self.endpoint!r}, {self.model!r})"

    def _request(self, body):
        headers = {"Content-Type": "application/json"}
        if self._key:
            headers["Authorization"] = "Bearer " + self._key
        data = json.dumps({"model": self.model, **body}).encode()
        return urllib.request.Request(self.endpoint, data=data, headers=headers, method="POST")

    def complete(self, body):
        """A non-streamed completion, as parsed JSON."""
        with urllib.request.urlopen(self._request(body), timeout=self.timeout) as resp:
            return json.loads(resp.read())

    def stream(self, body, clock=time.monotonic):
        """A streamed completion: (start time, [(arrival time, chunk), ...])."""
        body = {**body, "stream": True, "stream_options": {"include_usage": True}}
        start = clock()
        timed = []
        with urllib.request.urlopen(self._request(body), timeout=self.timeout) as resp:
            for chunk in parse_sse(resp):
                timed.append((clock(), chunk))
        return start, timed


def parse_sse(lines):
    """The JSON chunks of a server-sent event stream, up to `data: [DONE]`."""
    for raw in lines:
        line = raw.decode() if isinstance(raw, bytes) else raw
        line = line.strip()
        if not line.startswith("data:"):
            continue
        payload = line[5:].strip()
        if payload == "[DONE]":
            return
        if payload:
            yield json.loads(payload)


# ---------------------------------------------------------------------------- speed


def has_token(chunk):
    """Whether a chunk carries generated output: text, reasoning or a tool call."""
    for choice in chunk.get("choices") or []:
        delta = choice.get("delta") or {}
        if delta.get("content") or delta.get("reasoning_content") or delta.get("reasoning"):
            return True
        if delta.get("tool_calls"):
            return True
    return False


def measure(start, timed, prompt_estimate=None):
    """One streamed turn's numbers from its chunks and their arrival times (seconds).

    Prompt and output token counts come from the server's `usage`, else llama.cpp's
    `timings`, else an estimate (prompt) or the number of chunks with output (output).
    The rates prefer llama.cpp's own `timings`, measured inside the server; otherwise the
    prompt rate is prompt tokens over the time to the first token, and the writing rate is
    the tokens after the first over the time after it.
    """
    token_times = [t for t, c in timed if has_token(c)]
    if not token_times:
        raise ValueError("the stream carried no output")
    end = timed[-1][0]
    usage = {}
    timings = {}
    for _, chunk in timed:
        usage = chunk.get("usage") or usage
        timings = chunk.get("timings") or timings
    ttft = token_times[0] - start
    prompt_tokens = usage.get("prompt_tokens") or timings.get("prompt_n") or prompt_estimate
    output_tokens = usage.get("completion_tokens") or timings.get("predicted_n") or len(token_times)
    prompt_rate = timings.get("prompt_per_second")
    if not prompt_rate and prompt_tokens and ttft > 0:
        prompt_rate = prompt_tokens / ttft
    gen_rate = timings.get("predicted_per_second")
    writing = token_times[-1] - token_times[0]
    if not gen_rate and output_tokens > 1 and writing > 0:
        gen_rate = (output_tokens - 1) / writing
    return {
        "ttft_s": round(ttft, 4),
        "prompt_tokens": prompt_tokens,
        "prompt_tok_s": round(prompt_rate, 2) if prompt_rate else None,
        "output_tokens": output_tokens,
        "gen_tok_s": round(gen_rate, 2) if gen_rate else None,
        "total_s": round(end - start, 4),
    }


def turn_seconds(m, output_tokens):
    """A turn normalised to a fixed output length: the time to the first token, plus
    the rest of `output_tokens` at the measured writing rate. Two models asked the same
    thing write different lengths; this compares them on the same work."""
    if not m.get("gen_tok_s"):
        return m["total_s"]
    return round(m["ttft_s"] + max(0, output_tokens - 1) / m["gen_tok_s"], 4)


def median_of(runs, key):
    values = [r[key] for r in runs if r.get(key) is not None]
    return round(statistics.median(values), 4) if values else None


def bench_prompt(server, case, runs, clock=time.monotonic, say=print):
    """A prompt case run `runs` times, each with a fresh first line so no prompt cache
    serves it, and the median of each number."""
    results = []
    for i in range(runs):
        nonce = f"Run {uuid.uuid4().hex[:12]}.\n"
        messages = [dict(m) for m in case["messages"]]
        if isinstance(messages[0].get("content"), str):
            messages[0]["content"] = nonce + messages[0]["content"]
        else:
            messages.insert(0, {"role": "system", "content": nonce})
        body = {"messages": messages, "max_tokens": case["max_tokens"], "temperature": 0}
        if case.get("tools"):
            body["tools"] = case["tools"]
        start, timed = server.stream(body, clock=clock)
        m = measure(start, timed, prompt_estimate=estimate_tokens(messages))
        m["turn_s"] = turn_seconds(m, case["max_tokens"])
        results.append(m)
        say(f"  {case['name']} run {i + 1}/{runs}: prompt {m['prompt_tokens']} tok, "
            f"first token {m['ttft_s']:.2f}s, read {m['prompt_tok_s']} tok/s, "
            f"write {m['gen_tok_s']} tok/s, total {m['total_s']:.2f}s")
    keys = ["ttft_s", "prompt_tokens", "prompt_tok_s", "output_tokens", "gen_tok_s", "total_s", "turn_s"]
    return {"max_tokens": case["max_tokens"], "runs": results,
            "median": {k: median_of(results, k) for k in keys}}


# ---------------------------------------------------------------------------- tool calls


def first_tool_call(message):
    """(name, arguments) of a reply's first tool call, or None when it called none."""
    calls = message.get("tool_calls") or []
    if not calls:
        return None
    fn = calls[0].get("function") or {}
    args = fn.get("arguments") or {}
    if isinstance(args, str):
        try:
            args = json.loads(args) if args.strip() else {}
        except json.JSONDecodeError:
            args = {"_unparsed": args}
    return fn.get("name"), args


def same_value(expected, got):
    """Exact, except that `"*"` asks only for a non-empty value. An integer is not a
    string of digits, and a boolean is not an integer, as HQ's own argument checks hold."""
    if expected == "*":
        return got not in (None, "", [], {})
    if type(expected) is not type(got):
        return False
    return expected == got


def score_tool_case(case, message):
    """Whether a reply is right for a case, and why not."""
    want = case.get("expect")
    got = first_tool_call(message)
    if want is None:
        if got is None:
            return True, "no tool, as expected"
        return False, f"called {got[0]} where no tool was wanted"
    if got is None:
        return False, f"called no tool; wanted {want['tool']}"
    name, args = got
    if name != want["tool"]:
        return False, f"called {name}; wanted {want['tool']}"
    for key, value in (want.get("args") or {}).items():
        if key not in args:
            return False, f"{name} without {key}"
        if not same_value(value, args[key]):
            return False, f"{name} {key}={json.dumps(args[key])}; wanted {json.dumps(value)}"
    return True, f"{name} as expected"


def bench_tools(server, cases, say=print):
    """Each tool case once, at temperature 0, scored."""
    results = []
    for case in cases["tools"]:
        messages = []
        if cases["tool_system"]:
            messages.append({"role": "system", "content": cases["tool_system"]})
        messages.append({"role": "user", "content": case["instruction"]})
        body = {"messages": messages, "tools": cases["tool_definitions"],
                "tool_choice": "auto", "temperature": 0, "max_tokens": 400}
        started = time.monotonic()
        try:
            reply = server.complete(body)
            message = (reply.get("choices") or [{}])[0].get("message") or {}
            ok, why = score_tool_case(case, message)
        except (urllib.error.URLError, TimeoutError, ValueError) as e:
            ok, why = False, f"request failed: {type(e).__name__}"
        seconds = round(time.monotonic() - started, 3)
        results.append({"name": case["name"], "pass": ok, "why": why, "seconds": seconds})
        say(f"  {'pass' if ok else 'MISS'} {case['name']}: {why}")
    passed = sum(1 for r in results if r["pass"])
    return {"passed": passed, "total": len(results), "misses": len(results) - passed, "cases": results}


# ---------------------------------------------------------------------------- compare


def large_case_name(result):
    """The case a decision rests on: the one named `large`, else the largest prompt."""
    prompts = result["prompts"]
    if "large" in prompts:
        return "large"
    return max(prompts, key=lambda n: prompts[n]["median"].get("prompt_tokens") or 0)


def compare(a, b):
    """The owner's rule for replacing A with B, as (switch?, lines to print)."""
    lines = [f"A: {a['model']}    B: {b['model']}"]
    if a.get("cases_digest") != b.get("cases_digest"):
        lines.append("WARNING: the two runs used different case files; the comparison is not like for like.")
    names = [n for n in a["prompts"] if n in b["prompts"]]
    lines.append(f"{'case':<10}{'':>2}{'first token':>14}{'read tok/s':>14}{'write tok/s':>14}{'turn s':>12}")
    for n in names:
        for label, r in (("A", a), ("B", b)):
            m = r["prompts"][n]["median"]
            lines.append(f"{n:<10}{label:>2}{fmt(m['ttft_s']):>14}{fmt(m['prompt_tok_s']):>14}"
                         f"{fmt(m['gen_tok_s']):>14}{fmt(m['turn_s']):>12}")
    large = large_case_name(a)
    if large not in b["prompts"]:
        lines.append(f"DECISION: cannot decide; B has no '{large}' case.")
        return False, lines
    ta = a["prompts"][large]["median"]["turn_s"]
    tb = b["prompts"][large]["median"]["turn_s"]
    fast = tb * SPEED_FACTOR <= ta
    ma, mb = a["tools"]["misses"], b["tools"]["misses"]
    accurate = mb <= ma + EXTRA_MISSES_ALLOWED
    speedup = ta / tb if tb else float("inf")
    lines.append(f"Large turn ('{large}'): A {ta:.2f}s, B {tb:.2f}s, B is {speedup:.2f}x as fast "
                 f"(needs {SPEED_FACTOR:.0f}x): {'yes' if fast else 'no'}")
    lines.append(f"Tool calls: A {a['tools']['passed']}/{a['tools']['total']}, "
                 f"B {b['tools']['passed']}/{b['tools']['total']} "
                 f"(B may miss at most {EXTRA_MISSES_ALLOWED} more): {'yes' if accurate else 'no'}")
    switch = fast and accurate
    if switch:
        lines.append("DECISION: SWITCH to B.")
    else:
        reasons = []
        if not fast:
            reasons.append("not twice as fast on a large turn")
        if not accurate:
            reasons.append("too many more tool-call misses")
        lines.append("DECISION: KEEP A (B is " + " and ".join(reasons) + ").")
    return switch, lines


def fmt(v):
    return "-" if v is None else f"{v:.2f}"


# ---------------------------------------------------------------------------- main


def read_key(path):
    """The key from a file, stripped; None without a file. A file that cannot be read is
    reported without its path, in case the key itself was given in its place."""
    if not path:
        return None
    try:
        key = Path(path).expanduser().read_text().strip()
    except (OSError, UnicodeDecodeError) as e:
        raise SystemExit(f"cannot read the key file ({type(e).__name__})") from None
    if not key:
        raise SystemExit("the key file is empty")
    return key


def run(args, say=print):
    cases = load_cases(args.prompts)
    server = Server(args.url, args.model, key=read_key(args.key_file), timeout=args.timeout)
    say(f"Benchmarking {args.model} with {args.prompts} ({cases['digest']})")
    # One small call first, so loading the model is not counted against the first case.
    try:
        server.complete({"messages": [{"role": "user", "content": "Say ok."}], "max_tokens": 4})
    except urllib.error.HTTPError as e:
        raise SystemExit(f"the server refused the warm-up call: HTTP {e.code}")
    except urllib.error.URLError as e:
        raise SystemExit(f"cannot reach the server: {e.reason}")
    result = {"model": args.model, "cases_digest": cases["digest"], "runs": args.runs,
              "when": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "prompts": {}}
    say("Speed:")
    for case in cases["prompts"]:
        result["prompts"][case["name"]] = bench_prompt(server, case, args.runs, say=say)
    say("Tool calls:")
    result["tools"] = bench_tools(server, cases, say=say)
    say(f"Tool calls right: {result['tools']['passed']}/{result['tools']['total']}")
    Path(args.out).write_text(json.dumps(result, indent=2) + "\n")
    say(f"Wrote {args.out}")
    return result


def main(argv=None, say=print):
    # No abbreviations: `--key sk-...` must be refused, not taken as `--key-file`.
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], allow_abbrev=False)
    p.add_argument("--url", help="the server's OpenAI-compatible base URL, ending /v1")
    p.add_argument("--model", help="the model name the server knows")
    p.add_argument("--key-file", help="a file holding the API key, if the server needs one")
    p.add_argument("--prompts", help="the cases file (see scripts/bench-cases.json)")
    p.add_argument("--out", help="where to write the results (JSON)")
    p.add_argument("--runs", type=int, default=DEFAULT_RUNS, help="runs of each prompt case (median reported)")
    p.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT_S, help="seconds allowed for one call")
    p.add_argument("--compare", nargs=2, metavar=("A.json", "B.json"),
                   help="compare two results: A the current model, B the candidate")
    args = p.parse_args(argv)
    if args.compare:
        a, b = (json.loads(Path(f).read_text()) for f in args.compare)
        switch, lines = compare(a, b)
        for line in lines:
            say(line)
        return 0
    missing = [f"--{n}" for n in ("url", "model", "prompts", "out") if not getattr(args, n)]
    if missing:
        p.error("needs " + ", ".join(missing) + " (or --compare A.json B.json)")
    run(args, say=say)
    return 0


if __name__ == "__main__":
    sys.exit(main())
