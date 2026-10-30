#!/usr/bin/env python3
"""Offline checks for scripts/bench-model.py, against a fake OpenAI server in-process.

    python3 -m unittest scripts/test_bench_model.py
"""

import contextlib
import http.server
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest

HERE = Path(__file__).parent
SPEC = importlib.util.spec_from_file_location("bench_model", HERE / "bench-model.py")
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)

KEY = "sk-test-not-a-real-key-7f3a"


class FakeOpenAI(http.server.ThreadingHTTPServer):
    """Streams a short reply for a speed call, and answers a tool call from `tool_replies`
    (instruction -> (name, arguments) or None for plain text)."""

    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.tool_replies = {}
        self.timings = None
        self.auth_seen = []
        self.bodies = []

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_address[1]}/v1"


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        srv = self.server
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        srv.auth_seen.append(self.headers.get("Authorization"))
        srv.bodies.append(body)
        if body.get("stream"):
            return self.stream(body)
        message = {"role": "assistant", "content": "ok"}
        if body.get("tools"):
            instruction = body["messages"][-1]["content"]
            call = srv.tool_replies.get(instruction)
            if call:
                name, args = call
                message = {"role": "assistant", "content": None, "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": {"name": name, "arguments": json.dumps(args)}}]}
        self.send_json({"choices": [{"message": message}]})

    def send_json(self, obj):
        data = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def stream(self, body):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        def event(obj):
            self.wfile.write(b"data: " + json.dumps(obj).encode() + b"\n\n")
            self.wfile.flush()

        time.sleep(0.05)  # "reading the prompt"
        for word in ["The ", "failing ", "one ", "is ", "mediabox."]:
            event({"choices": [{"delta": {"content": word}}]})
            time.sleep(0.01)
        last = {"choices": [], "usage": {"prompt_tokens": 2000, "completion_tokens": 5}}
        if self.server.timings:
            last["timings"] = self.server.timings
        event(last)
        self.wfile.write(b"data: [DONE]\n\n")


@contextlib.contextmanager
def fake_server():
    srv = FakeOpenAI()
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    try:
        yield srv
    finally:
        srv.shutdown()
        srv.server_close()


def chunk(text=None, **extra):
    c = {"choices": [{"delta": {"content": text}}] if text else []}
    c.update(extra)
    return c


class TimingMath(unittest.TestCase):
    def test_rates_from_usage_and_arrival_times(self):
        # Asked at 10.0; first token at 12.0; 101 tokens, the last at 22.0.
        timed = [(12.0, chunk("a"))]
        timed += [(12.0 + i * 0.1, chunk("b")) for i in range(1, 101)]
        timed.append((22.1, chunk(usage={"prompt_tokens": 4000, "completion_tokens": 101})))
        m = BENCH.measure(10.0, timed)
        self.assertEqual(m["ttft_s"], 2.0)
        self.assertEqual(m["prompt_tokens"], 4000)
        self.assertEqual(m["prompt_tok_s"], 2000.0)  # 4000 tokens in 2 s
        self.assertEqual(m["gen_tok_s"], 10.0)  # 100 tokens after the first, in 10 s
        self.assertEqual(m["total_s"], 12.1)

    def test_llama_cpp_timings_win_over_wall_clock(self):
        timings = {"prompt_n": 16000, "prompt_per_second": 101.5,
                   "predicted_n": 300, "predicted_per_second": 9.25}
        timed = [(5.0, chunk("a")), (6.0, chunk("b")), (6.5, chunk(timings=timings))]
        m = BENCH.measure(0.0, timed)
        self.assertEqual(m["prompt_tokens"], 16000)
        self.assertEqual(m["prompt_tok_s"], 101.5)
        self.assertEqual(m["output_tokens"], 300)
        self.assertEqual(m["gen_tok_s"], 9.25)

    def test_without_usage_counts_chunks_and_uses_the_estimate(self):
        timed = [(1.0, chunk("a")), (2.0, chunk("b")), (3.0, chunk("c"))]
        m = BENCH.measure(0.0, timed, prompt_estimate=500)
        self.assertEqual(m["prompt_tokens"], 500)
        self.assertEqual(m["prompt_tok_s"], 500.0)
        self.assertEqual(m["output_tokens"], 3)
        self.assertEqual(m["gen_tok_s"], 1.0)

    def test_reasoning_and_tool_call_deltas_are_the_first_token(self):
        think = {"choices": [{"delta": {"reasoning_content": "hm"}}]}
        call = {"choices": [{"delta": {"tool_calls": [{"index": 0}]}}]}
        role_only = {"choices": [{"delta": {"role": "assistant"}}]}
        self.assertTrue(BENCH.has_token(think))
        self.assertTrue(BENCH.has_token(call))
        self.assertFalse(BENCH.has_token(role_only))
        m = BENCH.measure(0.0, [(0.5, role_only), (3.0, think), (4.0, chunk("x"))], 100)
        self.assertEqual(m["ttft_s"], 3.0)

    def test_turn_is_normalised_to_the_asked_length(self):
        m = {"ttft_s": 10.0, "gen_tok_s": 20.0, "total_s": 12.0}
        self.assertEqual(BENCH.turn_seconds(m, 301), 25.0)  # 10 + 300 / 20

    def test_no_output_is_an_error(self):
        with self.assertRaises(ValueError):
            BENCH.measure(0.0, [(1.0, chunk(usage={"prompt_tokens": 1}))])

    def test_sse_parsing_stops_at_done(self):
        lines = [b"data: {\"a\": 1}\n", b"\n", b": comment\n", b"data: [DONE]\n", b"data: {\"b\": 2}\n"]
        self.assertEqual(list(BENCH.parse_sse(lines)), [{"a": 1}])

    def test_medians_over_runs(self):
        runs = [{"ttft_s": 3.0}, {"ttft_s": 1.0}, {"ttft_s": 2.0}]
        self.assertEqual(BENCH.median_of(runs, "ttft_s"), 2.0)


class Scoring(unittest.TestCase):
    def call(self, name, args):
        return {"tool_calls": [{"function": {"name": name, "arguments": json.dumps(args)}}]}

    def test_exact_name_and_arguments(self):
        case = {"expect": {"tool": "claim", "args": {"task": 919}}}
        self.assertTrue(BENCH.score_tool_case(case, self.call("claim", {"task": 919}))[0])
        self.assertFalse(BENCH.score_tool_case(case, self.call("claim", {"task": 920}))[0])
        self.assertFalse(BENCH.score_tool_case(case, self.call("post", {"task": 919}))[0])
        self.assertFalse(BENCH.score_tool_case(case, self.call("claim", {}))[0])

    def test_task_number_must_be_an_integer_as_hq_requires(self):
        case = {"expect": {"tool": "claim", "args": {"task": 919}}}
        ok, why = BENCH.score_tool_case(case, self.call("claim", {"task": "919"}))
        self.assertFalse(ok)
        self.assertIn('"919"', why)

    def test_star_asks_only_for_a_value(self):
        case = {"expect": {"tool": "post", "args": {"task": 919, "text": "*"}}}
        self.assertTrue(BENCH.score_tool_case(case, self.call("post", {"task": 919, "text": "done"}))[0])
        self.assertFalse(BENCH.score_tool_case(case, self.call("post", {"task": 919, "text": ""}))[0])

    def test_extra_arguments_are_allowed(self):
        case = {"expect": {"tool": "container_logs", "args": {"container": "sonarr"}}}
        msg = self.call("container_logs", {"container": "sonarr", "lines": 50})
        self.assertTrue(BENCH.score_tool_case(case, msg)[0])

    def test_no_tool_wanted(self):
        case = {"expect": None}
        self.assertTrue(BENCH.score_tool_case(case, {"content": "Service unavailable."})[0])
        self.assertFalse(BENCH.score_tool_case(case, self.call("homelab_status", {}))[0])

    def test_unparseable_arguments_are_a_miss(self):
        case = {"expect": {"tool": "claim", "args": {"task": 919}}}
        msg = {"tool_calls": [{"function": {"name": "claim", "arguments": "{task: 919"}}]}
        self.assertFalse(BENCH.score_tool_case(case, msg)[0])

    def test_sample_cases_are_ten_and_well_formed(self):
        cases = BENCH.load_cases(HERE / "bench-cases.json")
        self.assertEqual(len(cases["tools"]), 10)
        self.assertEqual(sum(1 for c in cases["tools"] if c["expect"] is None), 1)
        names = {d["function"]["name"] for d in cases["tool_definitions"]}
        for c in cases["tools"]:
            if c["expect"]:
                self.assertIn(c["expect"]["tool"], names, c["name"])
        sizes = {p["name"]: BENCH.estimate_tokens(p["messages"]) for p in cases["prompts"]}
        self.assertAlmostEqual(sizes["small"], 2000, delta=300)
        self.assertAlmostEqual(sizes["typical"], 8000, delta=300)
        self.assertAlmostEqual(sizes["large"], 16000, delta=300)


def result(model, large_turn, misses, total=10, digest="d1"):
    median = {"ttft_s": large_turn / 2, "prompt_tok_s": 100.0, "gen_tok_s": 10.0,
              "turn_s": large_turn, "prompt_tokens": 16000}
    return {"model": model, "cases_digest": digest,
            "prompts": {"large": {"median": median}},
            "tools": {"passed": total - misses, "total": total, "misses": misses}}


class CompareRule(unittest.TestCase):
    def test_twice_as_fast_and_as_accurate_switches(self):
        switch, lines = BENCH.compare(result("27b", 200.0, 1), result("14b", 100.0, 1))
        self.assertTrue(switch)
        self.assertIn("DECISION: SWITCH", lines[-1])

    def test_just_short_of_twice_keeps(self):
        switch, lines = BENCH.compare(result("27b", 200.0, 1), result("14b", 101.0, 1))
        self.assertFalse(switch)
        self.assertIn("not twice as fast", lines[-1])

    def test_one_more_miss_is_allowed(self):
        switch, _ = BENCH.compare(result("27b", 200.0, 1), result("14b", 80.0, 2))
        self.assertTrue(switch)

    def test_two_more_misses_keeps(self):
        switch, lines = BENCH.compare(result("27b", 200.0, 1), result("14b", 80.0, 3))
        self.assertFalse(switch)
        self.assertIn("tool-call misses", lines[-1])

    def test_different_case_files_are_flagged(self):
        _, lines = BENCH.compare(result("27b", 200.0, 0), result("14b", 90.0, 0, digest="d2"))
        self.assertTrue(any(line.startswith("WARNING") for line in lines))

    def test_compare_from_the_command_line(self):
        with tempfile.TemporaryDirectory() as d:
            a, b = Path(d, "a.json"), Path(d, "b.json")
            a.write_text(json.dumps(result("27b", 300.0, 0)))
            b.write_text(json.dumps(result("14b", 120.0, 1)))
            out = []
            self.assertEqual(BENCH.main(["--compare", str(a), str(b)], say=out.append), 0)
        self.assertIn("DECISION: SWITCH to B.", out)


class AgainstFakeServer(unittest.TestCase):
    def run_bench(self, srv, d, runs=2):
        key_file = Path(d, "key")
        key_file.write_text(KEY + "\n")
        out_file = Path(d, "out.json")
        lines = []
        stdout, stderr = io.StringIO(), io.StringIO()
        argv = ["--url", srv.url, "--model", "fake-14b", "--key-file", str(key_file),
                "--prompts", str(HERE / "bench-cases.json"), "--out", str(out_file),
                "--runs", str(runs)]
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            BENCH.main(argv, say=lambda s: (lines.append(s), print(s)))
        return json.loads(out_file.read_text()), out_file.read_text(), stdout.getvalue() + stderr.getvalue()

    def test_full_run_measures_scores_and_never_shows_the_key(self):
        with fake_server() as srv, tempfile.TemporaryDirectory() as d:
            cases = json.loads((HERE / "bench-cases.json").read_text())
            for c in cases["tools"]:
                if c["expect"] and c["name"] != "claim_task":
                    args = {k: ("x" if v == "*" else v) for k, v in c["expect"]["args"].items()}
                    srv.tool_replies[c["instruction"]] = (c["expect"]["tool"], args)
            # claim_task is answered with the task as a string: a miss.
            srv.tool_replies["Claim task 919."] = ("claim", {"task": "919"})
            res, raw, printed = self.run_bench(srv, d)

            self.assertEqual(set(res["prompts"]), {"small", "typical", "large"})
            large = res["prompts"]["large"]
            self.assertEqual(len(large["runs"]), 2)
            self.assertGreaterEqual(large["median"]["ttft_s"], 0.05)
            self.assertEqual(large["median"]["prompt_tokens"], 2000)
            self.assertEqual(large["median"]["output_tokens"], 5)
            self.assertGreater(large["median"]["gen_tok_s"], 0)
            self.assertGreaterEqual(large["median"]["total_s"], large["median"]["ttft_s"])
            self.assertEqual(res["tools"]["passed"], 9)
            miss = [c for c in res["tools"]["cases"] if not c["pass"]]
            self.assertEqual([c["name"] for c in miss], ["claim_task"])

            # The key went to the server, and nowhere else.
            self.assertTrue(all(a == "Bearer " + KEY for a in srv.auth_seen))
            self.assertNotIn(KEY, printed)
            self.assertNotIn(KEY, raw)
            self.assertNotIn(KEY, repr(BENCH.Server(srv.url, "m", key=KEY)))

            # Every speed run starts with a fresh line, so no prompt cache serves it.
            firsts = [b["messages"][0]["content"].split("\n")[0] for b in srv.bodies if b.get("stream")]
            self.assertEqual(len(firsts), 6)
            self.assertEqual(len(set(firsts)), 6)
            self.assertTrue(all(b["stream_options"] == {"include_usage": True}
                                for b in srv.bodies if b.get("stream")))

    def test_llama_cpp_timings_are_used_when_streamed(self):
        with fake_server() as srv, tempfile.TemporaryDirectory() as d:
            srv.timings = {"prompt_n": 16100, "prompt_per_second": 98.0,
                           "predicted_n": 5, "predicted_per_second": 12.0}
            res, _, _ = self.run_bench(srv, d, runs=1)
            m = res["prompts"]["large"]["median"]
            self.assertEqual(m["prompt_tok_s"], 98.0)
            self.assertEqual(m["gen_tok_s"], 12.0)

    def test_a_refused_key_says_so_without_the_key(self):
        class Refusing(Handler):
            def do_POST(self):
                self.send_response(401)
                self.end_headers()
        srv = FakeOpenAI()
        srv.RequestHandlerClass = Refusing
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            with tempfile.TemporaryDirectory() as d:
                with self.assertRaises(SystemExit) as stop:
                    self.run_bench(srv, d)
            self.assertIn("401", str(stop.exception))
            self.assertNotIn(KEY, str(stop.exception))
        finally:
            srv.shutdown()
            srv.server_close()

    def test_the_key_is_not_an_argument(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err), self.assertRaises(SystemExit):
            BENCH.main(["--key", KEY, "--compare", "a", "b"])
        self.assertIn("unrecognized arguments", err.getvalue())

    def test_a_key_given_as_the_key_file_is_not_echoed(self):
        with self.assertRaises(SystemExit) as stop:
            BENCH.read_key(KEY)
        self.assertNotIn(KEY, str(stop.exception))


class CapturedPrompts(unittest.TestCase):
    def test_a_captured_request_replaces_a_synthetic_case(self):
        with tempfile.TemporaryDirectory() as d:
            captured = {"messages": [{"role": "system", "content": "s"}, {"role": "user", "content": "u"}],
                        "tools": [{"type": "function", "function": {"name": "claim"}}]}
            Path(d, "large.json").write_text(json.dumps(captured))
            cases = {"prompts": [{"name": "large", "messages_file": "large.json", "max_tokens": 200}]}
            Path(d, "cases.json").write_text(json.dumps(cases))
            loaded = BENCH.load_cases(Path(d, "cases.json"))
        case = loaded["prompts"][0]
        self.assertEqual(case["messages"], captured["messages"])
        self.assertEqual(case["tools"], captured["tools"])
        self.assertEqual(case["max_tokens"], 200)

    def test_fill_lines_are_all_different(self):
        text = BENCH.fill_text({"target_tokens": 500, "lines": ["{ts} a[{n}] x"]})
        lines = text.split("\n")
        self.assertEqual(len(lines), len(set(lines)))


if __name__ == "__main__":
    unittest.main()
