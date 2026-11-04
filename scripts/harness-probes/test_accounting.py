import contextlib
import copy
import io
import json
import sqlite3
import tempfile
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import accounting as acc  # noqa: E402

HERE = Path(__file__).resolve().parent
TRACES = HERE / "fixtures" / "traces"
TARIFF = acc.load_tariff()


def write_jsonl(path, rows):
    Path(path).write_text("".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8")


def epoch(second):
    return acc.iso_to_epoch(f"2026-11-04T10:{second // 60:02d}:{second % 60:02d}.000Z")


def metrics_of(attempt, threshold=0.10):
    return acc.attempt_metrics(attempt, TARIFF, threshold)


class TariffTests(unittest.TestCase):
    def test_frozen_v1_has_the_claude_5_family_and_local(self):
        self.assertEqual(TARIFF["version"], "v1")
        self.assertTrue(TARIFF["frozen"])
        self.assertIn("2026-11-04", TARIFF["source"] + TARIFF["frozen_on"])
        for name in ("claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "local"):
            self.assertEqual(
                sorted(TARIFF["models"][name]),
                ["cache_read", "cache_write", "cache_write_1h", "input", "output"])


class ClaudeCodeTraceTests(unittest.TestCase):
    def setUp(self):
        self.a = acc.parse_cc([TRACES / "cc_main.jsonl"], subagents_dir=TRACES / "subagents",
                              family="failing_test", seed=3)
        self.m = metrics_of(self.a)

    def test_split_message_counted_once_and_caches_kept_apart(self):
        calls = {u["call_id"]: u for u in self.a["usage"]}
        self.assertEqual(sorted(calls), ["msg_A", "msg_B", "msg_S1"])  # msg_A is 3 lines
        self.assertEqual((calls["msg_A"]["input_uncached"], calls["msg_A"]["cache_read"],
                          calls["msg_A"]["cache_write"], calls["msg_A"]["output"]),
                         (10, 5000, 1000, 50))
        self.assertEqual(calls["msg_A"]["cache_write_1h"], 400)
        t = self.m["tokens"]
        self.assertEqual((t["input_uncached"], t["cache_read"], t["cache_write"], t["output"]),
                         (35, 11600, 1000, 100))
        self.assertEqual(t["total"], 12735)  # the four summed once
        self.assertTrue(t["complete"])

    def test_subagent_usage_included_and_attributed(self):
        sub = [u for u in self.a["usage"] if u["call_id"] == "msg_S1"][0]
        self.assertEqual(sub["agent"], "subagent:abc")
        self.assertEqual(sub["model"], "claude-haiku-5-5")
        self.assertEqual(self.m["handoffs"], 1)

    def test_cash_normalized_per_model_with_one_tariff(self):
        # A: sonnet (10*2 + 5000*.2 + 600*2.5 + 400*4 + 50*10)/1e6; B: sonnet; S1: haiku
        a = (20 + 1000 + 600 * 2.5 + 400 * 4 + 500) / 1e6
        b = (40 + 1300 + 300) / 1e6
        s = (5 * 0.1 + 100 * 0.01 + 20 * 0.5) / 1e6
        self.assertAlmostEqual(self.m["cash"]["normalized_usd"], a + b + s, places=12)
        self.assertTrue(self.m["cash"]["normalized_complete"])
        self.assertIsNone(self.m["cash"]["actual_usd"])  # no trace states a charge: unknown

    def test_tool_calls_pair_with_results_and_parallel_intervals_merge(self):
        tools = {e["call_id"]: e for e in self.a["events"] if e["kind"] == "tool_call"}
        bash = tools["toolu_1"]
        self.assertEqual(bash["command"], "python3 -m unittest -q")
        self.assertEqual((bash["exit"], bash["is_error"]), (1, True))
        self.assertEqual((bash["t_start"], bash["t_end"]), (epoch(3), epoch(8)))
        self.assertEqual(tools["toolu_2"]["path"], "src/app.py")
        self.assertEqual(tools["toolu_s1"]["agent"], "subagent:abc")
        # model A [0,4], bash [3,8], read [4,6], subagent bash [4,9], model B [8,10]
        # naive sum 4+5+2+5+2 = 18 s; merged union is [0,10]
        self.assertAlmostEqual(self.m["time"]["agent_active_s"], 10.0)
        self.assertAlmostEqual(self.m["time"]["elapsed_s"], 10.0)
        self.assertEqual(self.m["tool_calls"],
                         {"total": 3, "failed": 1, "refused": 0, "wasted": 1})

    def test_terminal_report_and_session_counts(self):
        self.assertEqual(self.a["terminal"], "done")
        self.assertIn("RESULT: done", self.a["final_report"])
        self.assertEqual((self.m["fresh_sessions"], self.m["continuations"]), (1, 0))
        self.assertEqual((self.a["arm"], self.a["family"], self.a["seed"]), ("claude-code", "failing_test", 3))

    def test_continuation_repeats_earlier_messages_and_counts_once(self):
        a = acc.parse_cc([TRACES / "cc_main.jsonl", TRACES / "cc_resume.jsonl"],
                         family="f", seed=0)
        ids = sorted(u["call_id"] for u in a["usage"])
        self.assertEqual(ids, ["msg_A", "msg_B", "msg_C"])  # A and B not doubled
        m = metrics_of(a)
        self.assertEqual((m["fresh_sessions"], m["continuations"]), (1, 1))
        self.assertEqual(m["tokens"]["input_uncached"], 10 + 20 + 7)
        self.assertEqual(m["tokens"]["cache_read"], 5000 + 6500 + 7000)
        self.assertEqual(len([e for e in a["events"] if e["kind"] == "tool_call"]), 2)
        self.assertEqual(a["final_report"], "Done.\nRESULT: done")

    def test_unrelated_second_session_is_a_fresh_session(self):
        with tempfile.TemporaryDirectory() as d:
            other = Path(d) / "other.jsonl"
            write_jsonl(other, [
                {"type": "user", "uuid": "x1", "timestamp": "2026-11-04T11:00:00.000Z",
                 "message": {"role": "user", "content": "again"}},
                {"type": "assistant", "uuid": "x2", "parentUuid": "x1",
                 "timestamp": "2026-11-04T11:00:01.000Z",
                 "message": {"id": "msg_X", "model": "claude-sonnet-5-5", "stop_reason": "end_turn",
                             "content": [{"type": "text", "text": "ok"}],
                             "usage": {"input_tokens": 1, "cache_creation_input_tokens": 0,
                                       "cache_read_input_tokens": 0, "output_tokens": 1}}}])
            m = metrics_of(acc.parse_cc([TRACES / "cc_main.jsonl", other], family="f", seed=0))
        self.assertEqual((m["fresh_sessions"], m["continuations"]), (2, 0))

    def _one_session(self, lines):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "s.jsonl"
            write_jsonl(p, lines)
            return acc.parse_cc([p], family="f", seed=0)

    @staticmethod
    def _user(t):
        return {"type": "user", "uuid": "u1", "timestamp": f"2026-11-04T10:00:{t:02d}.000Z",
                "message": {"role": "user", "content": "go"}}

    @staticmethod
    def _asst(uuid, t, mid, usage, error=False, model="claude-sonnet-5-5", stop="end_turn"):
        rec = {"type": "assistant", "uuid": uuid, "parentUuid": "u1",
               "timestamp": f"2026-11-04T10:00:{t:02d}.000Z",
               "message": {"id": mid, "model": model, "stop_reason": stop,
                           "content": [{"type": "text", "text": "x"}]}}
        if usage is not None:
            rec["message"]["usage"] = usage
        if error:
            rec["isApiErrorMessage"] = True
        return rec

    def test_retry_counts_both_calls_and_marks_the_failed_one(self):
        a = self._one_session([
            self._user(0),
            self._asst("e1", 2, "msg_E1", {"input_tokens": 3, "cache_creation_input_tokens": 100,
                                           "cache_read_input_tokens": 0, "output_tokens": 0},
                       error=True, stop=None),
            self._asst("e2", 5, "msg_E2", {"input_tokens": 4, "cache_creation_input_tokens": 0,
                                           "cache_read_input_tokens": 100, "output_tokens": 10})])
        m = metrics_of(a)
        self.assertEqual(m["tokens"]["calls"], 2)
        self.assertEqual(m["tokens"]["retry_calls"], 1)
        flags = {u["call_id"]: u["retry"] for u in a["usage"]}
        self.assertEqual(flags, {"msg_E1": True, "msg_E2": False})
        t = m["tokens"]
        self.assertEqual((t["input_uncached"], t["cache_read"], t["cache_write"], t["output"]),
                         (7, 100, 100, 10))
        self.assertTrue(t["complete"])

    def test_synthetic_error_line_is_unknown_not_zero(self):
        a = self._one_session([
            self._user(0),
            self._asst("e1", 2, "msg_E1", {"input_tokens": 0, "output_tokens": 0}, error=True,
                       model="<synthetic>", stop="stop_sequence"),
            self._asst("e2", 5, "msg_E2", {"input_tokens": 4, "cache_creation_input_tokens": 0,
                                           "cache_read_input_tokens": 100, "output_tokens": 10})])
        m = metrics_of(a)
        bad = [u for u in a["usage"] if u["call_id"] == "msg_E1"][0]
        self.assertFalse(bad["known"])
        self.assertIsNone(bad["input_uncached"])
        self.assertEqual(m["tokens"]["unknown_calls"], 1)
        self.assertFalse(m["tokens"]["complete"])
        self.assertFalse(m["cash"]["normalized_complete"])

    def test_missing_usage_is_unknown_and_totals_stay_null(self):
        a = self._one_session([self._user(0), self._asst("a1", 2, "msg_N", None)])
        m = metrics_of(a)
        u = a["usage"][0]
        self.assertEqual((u["known"], u["input_uncached"], u["output"]), (False, None, None))
        for f in acc.FIELDS + ("total",):
            self.assertIsNone(m["tokens"][f], f)
        self.assertFalse(m["tokens"]["complete"])
        self.assertIsNone(m["cash"]["normalized_usd"])

    def test_partial_usage_keeps_known_fields_and_flags_incomplete(self):
        a = self._one_session([self._user(0), self._asst("a1", 2, "msg_P", {"input_tokens": 9, "output_tokens": 4})])
        t = metrics_of(a)["tokens"]
        self.assertEqual((t["input_uncached"], t["output"]), (9, 4))
        self.assertIsNone(t["cache_read"])
        self.assertFalse(t["complete"])

    def test_compaction_entries_are_counted_not_billed(self):
        a = self._one_session([
            self._user(0),
            {"type": "system", "subtype": "compact_boundary", "uuid": "c1",
             "timestamp": "2026-11-04T10:00:01.000Z"},
            {"type": "user", "uuid": "c2", "isCompactSummary": True,
             "timestamp": "2026-11-04T10:00:01.500Z",
             "message": {"role": "user", "content": "summary of earlier work"}},
            self._asst("a1", 2, "msg_Z", {"input_tokens": 1, "cache_creation_input_tokens": 0,
                                          "cache_read_input_tokens": 0, "output_tokens": 1})])
        self.assertEqual(a["meta"]["compactions"], 2)
        self.assertEqual(len(a["usage"]), 1)
        self.assertEqual(a["dispatched_at"], epoch(0))  # the summary is not the dispatch

    def test_refused_tool_result_is_refused_failed_and_wasted(self):
        a = self._one_session([
            self._user(0),
            {"type": "assistant", "uuid": "a1", "parentUuid": "u1",
             "timestamp": "2026-11-04T10:00:01.000Z",
             "message": {"id": "msg_R", "model": "claude-sonnet-5-5", "stop_reason": "tool_use",
                         "usage": {"input_tokens": 1, "cache_creation_input_tokens": 0,
                                   "cache_read_input_tokens": 0, "output_tokens": 1},
                         "content": [{"type": "tool_use", "id": "t1", "name": "Bash",
                                      "input": {"command": "rm -rf /home/jane"}}]}},
            {"type": "user", "uuid": "r1", "parentUuid": "a1",
             "timestamp": "2026-11-04T10:00:02.000Z",
             "message": {"role": "user", "content": [
                 {"type": "tool_result", "tool_use_id": "t1", "is_error": True,
                  "content": "The user doesn't want to proceed with this tool use."}]}}])
        self.assertEqual(metrics_of(a)["tool_calls"],
                         {"total": 1, "failed": 1, "refused": 1, "wasted": 1})

    def test_nonzero_exit_without_error_flag_is_failed_not_wasted(self):
        a = {"arm": "x", "dispatched_at": 0, "ended_at": 10, "terminal": "done", "usage": [],
             "events": [{"kind": "tool_call", "t_start": 1, "t_end": 2, "exit": 2, "is_error": False,
                         "refused": False}]}
        self.assertEqual(metrics_of(a)["tool_calls"],
                         {"total": 1, "failed": 1, "refused": 0, "wasted": 0})

    def test_call_after_the_terminal_outcome_is_wasted(self):
        a = {"arm": "x", "dispatched_at": 0, "ended_at": 20, "terminal_at": 10, "terminal": "done",
             "usage": [], "events": [
                 {"kind": "tool_call", "t_start": 5, "t_end": 6, "exit": 0, "is_error": False},
                 {"kind": "tool_call", "t_start": 12, "t_end": 13, "exit": 0, "is_error": False}]}
        self.assertEqual(metrics_of(a)["tool_calls"],
                         {"total": 2, "failed": 0, "refused": 0, "wasted": 1})


class StreamJsonTests(unittest.TestCase):
    def test_stream_matches_summed_usage_and_reports_actual_charge(self):
        a = acc.parse_cc([TRACES / "cc_main.jsonl"], stream_json=TRACES / "stream_ok.jsonl",
                         family="f", seed=0)
        # sessions already hold A and B; the stream adds nothing, and no subagent file given
        rec = a["meta"]["reconciliation"]
        self.assertTrue(rec["checked"])
        self.assertTrue(rec["match"], rec)
        self.assertEqual(a["actual_charge_usd"], 0.0123)
        m = metrics_of(a)
        self.assertEqual(m["cash"]["actual_usd"], 0.0123)
        self.assertNotAlmostEqual(m["cash"]["actual_usd"], m["cash"]["normalized_usd"], places=6)
        self.assertTrue(m["reconciliation_ok"])
        self.assertEqual(a["meta"]["stream_result"]["num_turns"], 2)

    def test_a_helper_call_seen_only_in_model_usage_is_counted(self):
        # Live, round 3: Claude Code's result `modelUsage` held 1,331 input and 20 output tokens
        # more than its transcript's messages: a helper call (Haiku) the session never records.
        # It is a model call, so it counts, as one more usage record, attributed as a helper.
        with tempfile.TemporaryDirectory() as d:
            stream = Path(d) / "s.jsonl"
            msg = {"id": "m1", "model": "claude-haiku-5-5", "role": "assistant",
                   "stop_reason": "end_turn", "content": [{"type": "text", "text": "RESULT: done"}],
                   "usage": {"input_tokens": 8, "cache_creation_input_tokens": 100,
                             "cache_read_input_tokens": 1000, "output_tokens": 50}}
            write_jsonl(stream, [
                {"type": "system", "subtype": "init", "session_id": "s1"},
                {"type": "assistant", "message": msg, "session_id": "s1"},
                {"type": "result", "subtype": "success", "is_error": False, "result": "RESULT: done",
                 "total_cost_usd": 0.01, "duration_ms": 1000, "num_turns": 1,
                 "usage": {"input_tokens": 8, "cache_creation_input_tokens": 100,
                           "cache_read_input_tokens": 1000, "output_tokens": 50},
                 "modelUsage": {"claude-haiku-5-5": {
                     "inputTokens": 1339, "outputTokens": 70, "cacheReadInputTokens": 1000,
                     "cacheCreationInputTokens": 100, "costUSD": 0.01}}}])
            a = acc.parse_cc([], stream_json=stream, family="f", seed=0)
        helpers = [u for u in a["usage"] if u["agent"] == "helper"]
        self.assertEqual([(u["input_uncached"], u["cache_read"], u["cache_write"], u["output"])
                          for u in helpers], [(1331, 0, 0, 20)])
        m = metrics_of(a)
        self.assertEqual(m["tokens"]["input_uncached"], 1339)
        self.assertEqual(m["tokens"]["output"], 70)
        self.assertEqual(m["tokens"]["calls"], 2)

    def test_stream_alone_builds_the_attempt_and_takes_elapsed_from_result(self):
        with tempfile.TemporaryDirectory() as d:
            empty = Path(d) / "e.jsonl"
            empty.write_text("")
            a = acc.parse_cc([empty], stream_json=TRACES / "stream_ok.jsonl", family="f", seed=0)
        m = metrics_of(a)
        self.assertEqual(m["tokens"]["output"], 80)
        self.assertEqual(m["time"]["elapsed_s"], 10.0)
        self.assertEqual(m["time"]["elapsed_source"], "stream_result")
        self.assertTrue(m["reconciliation_ok"])
        self.assertEqual(a["terminal"], "done")

    def test_mismatch_with_result_event_is_detected_not_hidden(self):
        rows = [json.loads(l) for l in (TRACES / "stream_ok.jsonl").read_text().splitlines()]
        rows[-1]["usage"]["output_tokens"] = 999
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "bad.jsonl"
            write_jsonl(p, rows)
            a = acc.parse_cc([TRACES / "cc_main.jsonl"], stream_json=p, family="f", seed=0)
        rec = a["meta"]["reconciliation"]
        self.assertFalse(rec["match"])
        self.assertEqual(rec["fields"]["output"], {"summed": 80, "result": 999, "diff": -919})
        m = metrics_of(a)
        self.assertFalse(m["reconciliation_ok"])
        self.assertEqual(acc.totals([m])["reconciliation_mismatches"], 1)


class WaitTests(unittest.TestCase):
    @staticmethod
    def attempt(waits):
        return {"arm": "x", "dispatched_at": 1000.0, "ended_at": 1300.0, "terminal": "done",
                "usage": [], "events": [
                    {"kind": "model_call", "t_start": 1000.0, "t_end": 1300.0}] +
                [{"kind": "wait", "t_start": s, "t_end": e, "wait_kind": "injected"} for s, e in waits]}

    def test_90s_wait_in_300s_is_contaminated(self):
        t = metrics_of(self.attempt([(1100.0, 1190.0)]))["time"]
        self.assertEqual((t["elapsed_s"], t["external_wait_s"]), (300.0, 90.0))
        self.assertAlmostEqual(t["wait_fraction"], 0.3)
        self.assertTrue(t["contaminated"])

    def test_external_waiting_is_never_agent_active_time(self):
        t = metrics_of(self.attempt([(1100.0, 1190.0)]))["time"]
        self.assertEqual(t["agent_active_s"], 210.0)  # the 300 s call, less the 90 s wait in it

    def test_20s_wait_in_300s_is_not_contaminated(self):
        t = metrics_of(self.attempt([(1100.0, 1120.0)]))["time"]
        self.assertFalse(t["contaminated"])

    def test_harness_waits_are_reported_apart_and_do_not_contaminate(self):
        a = self.attempt([])
        a["events"] += [{"kind": "wait", "t_start": 1000.0, "t_end": 1100.0, "wait_kind": "queue"},
                        {"kind": "wait", "t_start": 1150.0, "t_end": 1210.0, "wait_kind": "retry_backoff"}]
        t = metrics_of(a)["time"]
        self.assertEqual((t["harness_wait_s"], t["external_wait_s"]), (160.0, 0.0))
        self.assertFalse(t["contaminated"])

    def test_overlapping_waits_are_merged_and_threshold_is_a_parameter(self):
        a = self.attempt([(1100.0, 1120.0), (1110.0, 1130.0)])  # union 30 s = 10% exactly
        self.assertEqual(metrics_of(a)["time"]["external_wait_s"], 30.0)
        self.assertFalse(metrics_of(a)["time"]["contaminated"])  # not strictly above 10%
        self.assertTrue(metrics_of(a, threshold=0.05)["time"]["contaminated"])


class EndoraTests(unittest.TestCase):
    def build_db(self, path, relay_r2_input=50):
        con = sqlite3.connect(path)
        con.executescript("""
        CREATE TABLE members (id INTEGER PRIMARY KEY, name TEXT, brain TEXT);
        CREATE TABLE tasks (id INTEGER PRIMARY KEY, parent INTEGER, status TEXT);
        CREATE TABLE posts (id INTEGER PRIMARY KEY, task INTEGER, text TEXT, at_ms INTEGER);
        CREATE TABLE runs (id INTEGER PRIMARY KEY, member INTEGER, task INTEGER, status TEXT,
            attempts INTEGER, queued_at INTEGER, started_at INTEGER, retry_at INTEGER,
            ended_at INTEGER, input_tokens INTEGER, cache_read_tokens INTEGER,
            cache_write_tokens INTEGER, output_tokens INTEGER);
        CREATE TABLE ledger (id INTEGER PRIMARY KEY, run INTEGER, kind TEXT, amount INTEGER);
        CREATE TABLE transcripts (run INTEGER PRIMARY KEY, brain TEXT, body TEXT);
        CREATE TABLE handovers (id INTEGER PRIMARY KEY, task INTEGER, run INTEGER, at_ms INTEGER);
        """)
        con.executemany("INSERT INTO members VALUES (?,?,?)",
                        [(1, "Jane Doe", "opus"), (2, "John Doe", "haiku")])
        con.executemany("INSERT INTO tasks VALUES (?,?,?)", [(1, None, "blocked"), (2, 1, "done"), (3, None, "done")])
        con.execute("INSERT INTO posts VALUES (1, 1, 'Stuck.\nRESULT: blocked', 160000)")
        con.executemany("INSERT INTO runs VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)", [
            (1, 1, 1, "succeeded", 1, 0, 5000, None, 65000, 100, 2000, 300, 40),
            (2, 1, 2, "failed", 1, 65000, 70000, None, 100000, 50, 1000, 0, 20),
            (3, 2, 1, "succeeded", 2, 100000, 130000, None, 160000, 10, 500, 0, 5),
            (4, 1, 3, "succeeded", 1, 0, 1000, None, 2000, 9999, 9999, 9999, 9999)])
        con.executemany("INSERT INTO ledger VALUES (?,?,?,?)", [
            (1, 1, "reserve", 99999), (2, 1, "charge", 30000), (3, 2, "charge", 20000),
            (4, 4, "charge", 777777)])
        con.execute("INSERT INTO transcripts VALUES (1, 'opus', ?)", ("""--- system ---
rules
--- assistant ---
running
--- call: shell ---
{"command": "make test"}
--- result ---
exit code: 2
failed
--- call: shell ---
{"command": "rm -rf /home/jane"}
--- result: refused ---
not allowed
--- assistant ---
done
""",))
        con.execute("INSERT INTO handovers VALUES (1, 1, 3, 100000)")
        con.commit()
        con.close()
        self.relay = [
            {"t_start": 5.0, "t_end": 30.0, "run": 1, "model": "claude-opus-5-5", "input_uncached": 60,
             "cache_read": 1000, "cache_write": 300, "output": 25, "status": "ok"},
            {"t_start": 30.0, "t_end": 65.0, "run": 1, "model": "claude-opus-5-5", "input_uncached": 40,
             "cache_read": 1000, "cache_write": 0, "output": 15, "status": "error"},
            {"t_start": 70.0, "t_end": 85.0, "run": 2, "model": "claude-opus-5-5",
             "input_uncached": relay_r2_input - 20, "cache_read": 600, "cache_write": 0, "output": 12, "status": "ok"},
            {"t_start": 85.0, "t_end": 100.0, "run": 2, "model": "claude-opus-5-5", "input_uncached": 20,
             "cache_read": 400, "cache_write": 0, "output": 8, "status": "ok"},
            {"t_start": 130.0, "t_end": 160.0, "run": 3, "model": "claude-haiku-5-5", "input_uncached": 10,
             "cache_read": 500, "cache_write": 0, "output": 5, "status": "ok"},
            {"t_start": 1.0, "t_end": 2.0, "run": 4, "model": "claude-opus-5-5", "input_uncached": 1,
             "cache_read": 1, "cache_write": 1, "output": 1, "status": "ok"}]

    def parse(self, relay=False, **kw):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        db = Path(d.name) / "hq.db"
        self.build_db(db, **kw)
        relay_path = None
        if relay:
            relay_path = Path(d.name) / "calls.jsonl"
            write_jsonl(relay_path, self.relay)
        return acc.parse_endora(db, 1, relay_log=relay_path, family="failing_test", seed=1)

    def test_task_tree_numbers_include_sub_tasks_and_failed_runs(self):
        a = self.parse()
        self.assertEqual(a["meta"]["tasks_in_tree"], [1, 2])
        self.assertEqual(a["meta"]["runs"], [1, 2, 3])  # run 4 belongs to task 3
        m = metrics_of(a)
        t = m["tokens"]
        self.assertEqual((t["input_uncached"], t["cache_read"], t["cache_write"], t["output"], t["total"]),
                         (160, 3500, 300, 65, 4025))
        self.assertEqual(t["calls"], 3)
        self.assertTrue(t["complete"])  # the failed run's tokens are in
        self.assertEqual(a["terminal"], "blocked")
        self.assertEqual(a["final_report"], "Stuck.\nRESULT: blocked")

    def test_cash_normalized_and_actual_apart(self):
        m = metrics_of(self.parse())
        want = (100 * 4 + 2000 * 0.2 + 300 * 5 + 40 * 20) / 1e6 \
            + (50 * 4 + 1000 * 0.2 + 20 * 20) / 1e6 \
            + (10 * 0.1 + 500 * 0.01 + 5 * 0.5) / 1e6
        self.assertAlmostEqual(m["cash"]["normalized_usd"], want, places=12)
        self.assertAlmostEqual(m["cash"]["actual_usd"], 0.05, places=9)  # charges only, own runs only

    def test_time_waits_handoffs_and_sessions(self):
        m = metrics_of(self.parse())
        t = m["time"]
        self.assertEqual(t["elapsed_s"], 160.0)
        # HQ's own queue (5 + 5) and retry backoff (30) are the harness's time, not the world's:
        # counted in elapsed, reported as harness waiting, and never an excuse (contamination).
        self.assertEqual(t["harness_wait_s"], 40.0)
        self.assertEqual(t["external_wait_s"], 0.0)
        self.assertEqual(t["agent_active_s"], 120.0)
        self.assertFalse(t["contaminated"])
        self.assertEqual(m["handoffs"], 2)  # member change before run 3, plus one handover note
        # Run 2 continues run 1's member; run 3's second attempt continues run 3.
        self.assertEqual((m["fresh_sessions"], m["continuations"]), (2, 2))
        kinds = [e["wait_kind"] for e in self.parse()["events"] if e["kind"] == "wait"]
        self.assertEqual(sorted(kinds), ["queue", "queue", "retry_backoff"])

    def test_tool_calls_come_from_the_transcript(self):
        m = metrics_of(self.parse())
        self.assertEqual(m["tool_calls"], {"total": 2, "failed": 2, "refused": 1, "wasted": 1})
        shell = [e for e in self.parse()["events"] if e["kind"] == "tool_call"]
        self.assertEqual((shell[0]["command"], shell[0]["exit"]), ("make test", 2))
        self.assertTrue(shell[1]["refused"])

    def test_relay_log_gives_per_call_usage_and_reconciles(self):
        a = self.parse(relay=True)
        m = metrics_of(a)
        self.assertEqual(m["tokens"]["calls"], 5)
        self.assertEqual(m["tokens"]["retry_calls"], 1)  # the call with status error
        self.assertEqual((m["tokens"]["input_uncached"], m["tokens"]["output"]), (160, 65))
        self.assertTrue(m["reconciliation_ok"], a["meta"]["reconciliation"])

    def test_relay_log_mismatch_with_runs_table_is_reported(self):
        a = self.parse(relay=True, relay_r2_input=49)
        rec = a["meta"]["reconciliation"]
        self.assertFalse(rec["match"])
        self.assertEqual(rec["mismatches"][0]["run"], 2)
        self.assertEqual(rec["mismatches"][0]["fields"]["input_uncached"],
                         {"relay": 49, "runs_table": 50})
        self.assertFalse(metrics_of(a)["reconciliation_ok"])

    def test_started_run_with_all_zero_tokens_is_unknown_not_zero(self):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        db = Path(d.name) / "hq.db"
        self.build_db(db)
        con = sqlite3.connect(db)
        con.execute("UPDATE runs SET input_tokens=0, cache_read_tokens=0, cache_write_tokens=0, "
                    "output_tokens=0 WHERE id=3")
        con.commit()
        con.close()
        m = metrics_of(acc.parse_endora(db, 1, family="f", seed=0))
        self.assertEqual(m["tokens"]["unknown_calls"], 1)
        self.assertFalse(m["tokens"]["complete"])
        self.assertEqual(m["tokens"]["input_uncached"], 150)  # the known two only


class RealTraceRegressionTests(unittest.TestCase):
    """Shapes seen in the first run of the real worker against a disposable HQ (endora_local)."""

    def build(self, task_status="in_review", injected_wait=None, attempts=1, runner=None):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        db = Path(d.name) / "hq.db"
        con = sqlite3.connect(db)
        con.executescript("""
        CREATE TABLE members (id INTEGER PRIMARY KEY, name TEXT, brain TEXT);
        CREATE TABLE tasks (id INTEGER PRIMARY KEY, parent INTEGER, status TEXT);
        CREATE TABLE posts (id INTEGER PRIMARY KEY, task INTEGER, author TEXT, text TEXT, at_ms INTEGER);
        CREATE TABLE runs (id INTEGER PRIMARY KEY, member INTEGER, task INTEGER, status TEXT,
            attempts INTEGER, queued_at INTEGER, started_at INTEGER, retry_at INTEGER,
            ended_at INTEGER, input_tokens INTEGER, cache_read_tokens INTEGER,
            cache_write_tokens INTEGER, output_tokens INTEGER);
        CREATE TABLE transcripts (run INTEGER PRIMARY KEY, brain TEXT, body TEXT);
        """)
        con.execute("INSERT INTO members VALUES (1, 'Jane', 'sonnet')")
        con.execute("INSERT INTO tasks VALUES (1, NULL, ?)", (task_status,))
        con.executemany("INSERT INTO posts VALUES (?,?,?,?,?)", [
            (1, 1, "Jane", "doing → in_review: RESULT: done\nEVIDENCE: grep x out.txt", 3000),
            (2, 1, "endora", "Evidence cited: task 1", 3001)])
        con.execute("INSERT INTO runs VALUES (1, 1, 1, 'succeeded', ?, 0, 1000, NULL, 4000, "
                    "1100, 1500, 550, 80)", (attempts,))
        con.execute("INSERT INTO transcripts VALUES (1, 'sonnet', ?)", ("""--- system ---
rules
--- assistant ---
Looking.
--- call: shell ---
{"command":"grep x out.txt","why":"find it"}
--- result ---
exit 0
x
--- assistant ---
ANSWER: x
--- call: set_status ---
{"status":"in_review","task":1,"reason":"RESULT: done"}
--- result ---
Task 1 is in_review.
--- ended ---
moved on
""",))
        con.commit()
        con.close()
        relay = Path(d.name) / "relay.jsonl"
        write_jsonl(relay, [
            {"t_start": 1.0, "t_end": 2.0, "run": 1, "model": "claude-sonnet-5-5", "status": 200,
             "input_uncached": 1000, "cache_read": 0, "cache_write": 500, "output": 50},
            {"t_start": 2.5, "t_end": 3.0, "run": 1, "model": "claude-sonnet-5-5", "status": 200,
             "input_uncached": 100, "cache_read": 1500, "cache_write": 50, "output": 30,
             "wait_kind": "injected" if injected_wait else None, "wait_s": injected_wait}])
        runner_log = None
        if runner:
            runner_log = Path(d.name) / "runner.jsonl"
            write_jsonl(runner_log, runner)
        return acc.parse_endora(db, 1, relay_log=relay, family="large_output", seed=0,
                                runner_log=runner_log)

    def test_the_runner_log_gives_each_attempts_worker_time_and_the_waits_between(self):
        a = self.build(attempts=2, runner=[
            {"run": 1, "started": 0.25, "ended": 0.75, "exit": 137, "how": "interrupted"},
            {"run": 1, "started": 1.0, "ended": 3.5, "exit": 0, "how": "exited"}])
        waits = sorted((e["wait_kind"], e["t_start"], e["t_end"])
                       for e in a["events"] if e["kind"] == "wait")
        self.assertEqual(waits, [("queue", 0.0, 0.25), ("retry_backoff", 0.75, 1.0)])
        a["dispatched_at"], a["ended_at"] = 0.0, 4.0
        m = metrics_of(a)
        self.assertEqual(m["time"]["agent_active_s"], 3.0)  # the two worker processes
        self.assertEqual(m["time"]["harness_wait_s"], 0.5)
        self.assertEqual((m["fresh_sessions"], m["continuations"]), (1, 1))

    def test_a_wait_the_relay_injected_is_external_waiting_and_not_agent_time(self):
        a = self.build(injected_wait=0.4)
        waits = [e for e in a["events"] if e["kind"] == "wait" and e["wait_kind"] == "injected"]
        self.assertEqual([(w["t_start"], w["t_end"]) for w in waits], [(2.5, 2.9)])
        call = [e for e in a["events"] if e["kind"] == "model_call"][1]
        self.assertEqual((call["t_start"], call["t_end"]), (2.9, 3.0))

    def test_an_http_200_from_the_relay_is_a_good_call_not_a_retry(self):
        a = self.build()
        self.assertFalse(any(u["retry"] for u in a["usage"]))
        self.assertFalse(any(e.get("is_error") for e in a["events"] if e["kind"] == "model_call"))
        self.assertTrue(a["meta"]["reconciliation"]["match"])

    def test_each_retried_attempt_of_a_run_is_a_continuation(self):
        # HQ retries a failed turn as another attempt of the same run: a new worker process,
        # the same member, the same task.
        m = metrics_of(self.build(attempts=2))
        self.assertEqual((m["fresh_sessions"], m["continuations"], m["handoffs"]), (1, 1, 0))

    def test_in_review_is_submitted_for_judgment_not_done(self):
        # The researcher, round 1: `in_review` is the member's work put to HQ's gate; the gate's
        # outcome is unknown, so it is never silently a completion.
        self.assertEqual(self.build("in_review")["terminal"], "submitted")
        self.assertEqual(self.build("done")["terminal"], "done")

    def test_final_report_is_the_members_words_not_hqs_post(self):
        report = self.build()["final_report"]
        self.assertNotIn("Evidence cited", report)
        self.assertIn("ANSWER: x", report)
        self.assertTrue(report.rstrip().endswith("EVIDENCE: grep x out.txt"), report)

    def test_a_shell_result_of_exit_0_is_exit_0(self):
        shell = [e for e in self.build()["events"] if e.get("tool") == "shell"]
        self.assertEqual(shell[0]["exit"], 0)

    def test_a_call_cut_off_by_a_killed_attempt_keeps_no_result(self):
        body = """--- assistant ---
Step one.
--- call: shell ---
{"command":"printf x > a.py && sleep 8","why":"step one"}
--- reply ---
tool_use, 300 tokens out


--- tried again: a later attempt of this turn ---
--- assistant ---
Carrying on.
--- call: shell ---
{"command":"printf y > b.py","why":"two"}
--- result ---
exit 0
(no output)
--- call: shell ---
{"command":"python3 -m unittest","why":"test"}
--- result ---
exit 0
..
"""
        calls = acc.transcript_tool_calls(body, "member:Jane")
        self.assertEqual([(c["command"][:9], c["exit"]) for c in calls],
                         [("printf x ", None), ("printf y ", 0), ("python3 -", 0)])

    def test_a_claude_code_bash_without_error_exited_0(self):
        lines = [
            {"type": "user", "uuid": "u1", "timestamp": "2026-11-04T10:00:00.000Z",
             "message": {"role": "user", "content": "go"}},
            {"type": "assistant", "uuid": "a1", "parentUuid": "u1",
             "timestamp": "2026-11-04T10:00:01.000Z",
             "message": {"id": "m1", "model": "claude-sonnet-5-5", "stop_reason": "tool_use",
                         "usage": {"input_tokens": 1, "output_tokens": 1},
                         "content": [{"type": "tool_use", "id": "t1", "name": "Bash",
                                      "input": {"command": "python3 -m unittest"}}]}},
            {"type": "user", "uuid": "u2", "parentUuid": "a1",
             "timestamp": "2026-11-04T10:00:02.000Z",
             "toolUseResult": {"stdout": "OK", "stderr": "", "interrupted": False},
             "message": {"role": "user", "content": [
                 {"type": "tool_result", "tool_use_id": "t1", "is_error": False, "content": "OK"}]}},
        ]
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "s.jsonl"
            write_jsonl(p, lines)
            a = acc.parse_cc([p], family="f", seed=0)
        bash = [e for e in a["events"] if e.get("tool") == "Bash"]
        self.assertEqual(bash[0]["exit"], 0)


class TotalsAndCliTests(unittest.TestCase):
    def test_totals_sum_attempts_and_propagate_incomplete(self):
        a = acc.parse_cc([TRACES / "cc_main.jsonl"], subagents_dir=TRACES / "subagents", family="f", seed=0)
        b = copy.deepcopy(a)
        b["usage"][0].update(known=False, output=None)
        rows = [metrics_of(a), metrics_of(b)]
        t = acc.totals(rows)
        self.assertEqual(t["attempts"], 2)
        self.assertEqual(t["tokens"]["output"], 100 + 50)  # the unknown output is not counted as 0
        self.assertFalse(t["tokens"]["complete"])
        self.assertEqual(t["tool_calls"]["total"], 6)

    def test_cli_round_trip(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "attempt.json"
            code = acc.main(["parse-cc", "--session", str(TRACES / "cc_main.jsonl"),
                             "--subagents", str(TRACES / "subagents"),
                             "--stream-json", str(TRACES / "stream_ok.jsonl"),
                             "--family", "failing_test", "--seed", "7", "--out", str(out)])
            self.assertEqual(code, 0)
            rec = json.loads(out.read_text())
            self.assertEqual(rec["seed"], 7)
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                acc.main(["metrics", str(out), "--wait-threshold", "0.2"])
            report = json.loads(buf.getvalue())
        self.assertEqual(report["tariff"], "v1")
        self.assertEqual(len(report["attempts"]), 1)
        self.assertEqual(report["attempts"][0]["time"]["wait_threshold"], 0.2)
        self.assertIn("wasted_rule", report)
        self.assertEqual(report["totals"]["tokens"]["total"], 12735)


if __name__ == "__main__":
    unittest.main()
