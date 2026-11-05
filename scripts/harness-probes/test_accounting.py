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

    def test_time_parts_never_overlap_and_add_up_to_elapsed(self):
        # Round 18: HQ's model-call interval spans its queue waits, so the same seconds were
        # counted as agent-active and as harness waiting (27.32 + 0.85 > 27.34 elapsed).
        a = {"arm": "endora", "family": "f", "seed": 0, "dispatched_at": 0.0, "ended_at": 27.34,
             "terminal": "done", "final_report": "", "usage": [], "meta": {},
             "events": [{"kind": "wait", "wait_kind": "queue", "t_start": 0.0, "t_end": 0.4},
                        {"kind": "model_call", "t_start": 0.0, "t_end": 28.9},
                        {"kind": "wait", "wait_kind": "queue", "t_start": 0.4, "t_end": 0.85},
                        {"kind": "wait", "wait_kind": "external", "t_start": 10.0, "t_end": 11.0}]}
        t = metrics_of(a)["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 0.85)
        self.assertAlmostEqual(t["external_wait_s"], 1.0)
        self.assertAlmostEqual(t["agent_active_s"], 27.34 - 0.85 - 1.0)
        self.assertEqual(t["agent_requested_wait_s"], 0.0)
        self.assertAlmostEqual(t["agent_active_s"] + t["harness_wait_s"]
                               + t["agent_requested_wait_s"] + t["external_wait_s"]
                               + t["unaccounted_s"], t["elapsed_s"], places=6)
        self.assertGreaterEqual(t["unaccounted_s"], 0.0)

    def test_a_killed_invocations_stream_keeps_what_its_session_never_saved(self):
        # Round 16: Claude Code was stopped mid-attempt; its first stream held a completed Edit
        # (call and result) that the session file never saved, and only the last stream was read.
        u = {"input_tokens": 5, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 7}
        edit = {"id": "m1", "model": "claude-haiku-5-5", "usage": u, "stop_reason": "tool_use",
                "content": [{"type": "tool_use", "id": "tu1", "name": "Edit", "input": {"file_path": "a.py"}}]}
        after = {"id": "m2", "model": "claude-haiku-5-5", "usage": u, "stop_reason": "end_turn",
                 "content": [{"type": "text", "text": "RESULT: done"}]}
        with tempfile.TemporaryDirectory() as d:
            one, two = Path(d) / "stream-1.jsonl", Path(d) / "stream-2.jsonl"
            thinking = {**edit, "content": [{"type": "thinking", "thinking": "…"}]}
            write_jsonl(one, [{"type": "system", "subtype": "init", "session_id": "s1"},
                              {"type": "assistant", "message": thinking, "session_id": "s1"},
                              {"type": "assistant", "message": edit, "session_id": "s1"},
                              {"type": "user", "message": {"role": "user", "content": [
                                  {"type": "tool_result", "tool_use_id": "tu1", "content": "updated"}]}}])
            write_jsonl(two, [{"type": "assistant", "message": after, "session_id": "s1"},
                              {"type": "result", "subtype": "success", "is_error": False, "result": "RESULT: done",
                               "total_cost_usd": 0.001, "duration_ms": 900, "num_turns": 1, "usage": u}])
            a = acc.parse_cc([], stream_json=[(one, 100.0), (two, None)], family="f", seed=0)
        edits = [e for e in a["events"] if e.get("kind") == "tool_call" and e.get("tool") == "Edit"]
        self.assertEqual(len(edits), 1)
        # a stream carries no times: its records take the end of their invocation, so the
        # recovered edit sorts where it happened, not after everything else
        self.assertEqual(edits[0]["t_start"], 100.0)
        self.assertEqual(a["final_report"], "RESULT: done")
        self.assertEqual(len([x for x in a["usage"] if x["agent"] == "main"]), 2)

    def test_a_streams_time_never_moves_what_the_session_already_timed(self):
        a0 = acc.parse_cc([TRACES / "cc_main.jsonl"], stream_json=TRACES / "stream_ok.jsonl", family="f", seed=0)
        a1 = acc.parse_cc([TRACES / "cc_main.jsonl"], stream_json=[(TRACES / "stream_ok.jsonl", 9e9)],
                          family="f", seed=0)
        def times(att):
            return [(e["kind"], e.get("t_start"), e.get("t_end")) for e in att["events"] if e.get("t_start")]
        self.assertEqual(times(a0), times(a1))
        self.assertTrue(times(a0))

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

    def build(self, task_status="in_review", injected_wait=None, attempts=1, runner=None,
              events=None, served=None):
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
--- reply ---
tool_use, 50 tokens out
--- result ---
exit 0
x
--- assistant ---
ANSWER: x
--- call: set_status ---
{"status":"in_review","task":1,"reason":"RESULT: done"}
--- reply ---
tool_use, 30 tokens out
--- result ---
Task 1 is in_review.
--- ended ---
moved on
""",))
        if served is not None:
            con.execute("CREATE TABLE run_served_models (run INTEGER, model TEXT, calls INTEGER)")
            con.executemany("INSERT INTO run_served_models VALUES (1, ?, ?)", served)
        if events is not None:
            con.execute("CREATE TABLE events (id INTEGER PRIMARY KEY, at_ms INTEGER, kind TEXT, data TEXT)")
            con.executemany("INSERT INTO events (at_ms, kind, data) VALUES (?,?,?)",
                            [(at, kind, json.dumps(data)) for at, kind, data in events])
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

    def test_an_unknown_cache_write_ttl_gives_a_cost_range_not_a_guess(self):
        # HQ does not say whether a turn's cache writes were 5-minute or 1-hour ones.
        a = self.build()
        self.assertTrue(all(u["ttl_known"] is False for u in a["usage"]))
        cash = metrics_of(a)["cash"]
        low, high = cash["normalized_range_usd"]
        writes = 500 + 50  # tokens written, at Sonnet 5.5's 2.50 (5m) or 4.00 (1h) a million
        self.assertAlmostEqual(high - low, writes * (4.00 - 2.50) / 1e6, places=12)
        self.assertEqual(cash["ttl_unknown_write_tokens"], writes)

    def test_the_models_the_provider_named_are_kept_apart_from_the_alias(self):
        a = self.build(served=[("claude-sonnet-5-5-20261001", 2)])
        ident = metrics_of(a)["model_identity"]
        self.assertEqual(ident["requested"], ["claude-sonnet-5-5"])
        self.assertEqual(ident["served"], {"claude-sonnet-5-5-20261001": 2})
        # The relay log saw 2 answered calls and both are named: accounted for.
        self.assertEqual((ident["unknown_calls"], ident["accounted"], ident["complete"]),
                         (0, True, True))

    def test_calls_whose_model_is_not_known_leave_the_identity_incomplete(self):
        ident = metrics_of(self.build(served=[("", 1), ("claude-sonnet-5-5-20261001", 1)]))["model_identity"]
        self.assertEqual((ident["unknown_calls"], ident["complete"]), (1, False))
        # A store from before identities were kept says nothing: unknown, never the alias.
        ident = metrics_of(self.build())["model_identity"]
        self.assertEqual((ident["served"], ident["complete"]), ({}, False))

    def test_hqs_event_log_dates_dispatch_terminal_and_its_own_waiting(self):
        # Live, round 3: the task was created at +3.1 s, queued at +11.8, its run started at
        # +12.2, the task was done at +36.9, and the worker's process ended at +41.4.
        a = self.build(task_status="done", events=[
            (3100, "task.created", {"task": 1, "status": "todo"}),
            (11780, "wake.queued", {"task": 1, "member": 1}),
            (12180, "run.started", {"task": 1, "run": 1}),
            (22010, "receipt.recorded", {"task": 1, "run": 1, "tool": "repo_checkout"}),
            (36920, "task.changed", {"task": 1, "status": "done"}),
            (41430, "run.ended", {"task": 1, "run": 1}),
            (50000, "task.changed", {"task": 2, "status": "done"})])
        self.assertEqual((a["dispatched_at"], a["ended_at"]), (3.1, 36.92))
        labels = [t["label"] for t in a["meta"]["timeline"]]
        self.assertEqual(labels[:3], ["task.created", "wake.queued", "run.started"])
        waits = sorted((e["wait_kind"], e["t_start"], e["t_end"])
                       for e in a["events"] if e["kind"] == "wait")
        # HQ's dispatch (created -> queued) and the runner's pickup (queued -> started) are the
        # harness's own waiting.
        self.assertIn(("queue", 3.1, 11.78), waits)
        self.assertIn(("queue", 11.78, 12.18), waits)

    def test_hqs_own_word_on_how_it_closed_the_task_is_kept_apart(self):
        # Round 14: HQ closes a solo item "done, with no review" once its checks passed, or
        # "completed, not verified" when nothing could check the cited evidence. Native
        # completion and native verification are recorded apart; neither is the grader's.
        a = self.build(task_status="done")
        self.assertIsNone(a["meta"]["native_verification"])
        d = tempfile.TemporaryDirectory(); self.addCleanup(d.cleanup)
        for text, want in (("HQ's checks passed and Sol set it ready: done, with no review.", "verified"),
                           ("Sol set it ready with its criteria cited: completed, not verified.", "not_verified")):
            self.assertEqual(acc.native_verification_of([{"author": "endora", "text": text}]), want)

    def test_tool_calls_keep_their_output_for_the_grader(self):
        shell = [e for e in self.build()["events"] if e.get("tool") == "shell"]
        self.assertIn("exit 0", shell[0]["output"])

    def test_model_calls_are_counted_apart_from_usage_records(self):
        # Live HQ keeps one usage total a run; the transcript shows each reply.
        a = self.build()
        m = metrics_of(a)
        self.assertEqual(a["meta"]["model_calls_observed"], 2)
        self.assertEqual(m["tokens"]["usage_records"], 2)
        self.assertEqual(m["tokens"]["model_calls"], 2)

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


class ModelIdentityTests(unittest.TestCase):
    """Whether two arms are shown to have run on the same model (the climb, round 7)."""

    @staticmethod
    def ident(served, unknown=0, requested=("claude-haiku-5-5",)):
        return {"requested": list(requested), "served": served, "unknown_calls": unknown,
                "complete": bool(served) and unknown == 0}

    def test_the_same_named_models_in_both_arms_match(self):
        a = self.ident({"claude-haiku-5-5-20261001": 8})
        b = self.ident({"claude-haiku-5-5-20261001": 5})
        self.assertEqual(acc.identity_equivalence(a, b)["verdict"], "match")

    def test_different_named_models_are_a_mismatch(self):
        a = self.ident({"claude-haiku-5-5-20261001": 8})
        b = self.ident({"claude-haiku-5-5-20261115": 5})
        self.assertEqual(acc.identity_equivalence(a, b)["verdict"], "mismatch")

    def test_a_requested_alias_alone_never_shows_equivalence(self):
        a = self.ident({})
        b = self.ident({"claude-haiku-5-5-20261001": 5})
        self.assertEqual(acc.identity_equivalence(a, b)["verdict"], "incomplete")
        partly = self.ident({"claude-haiku-5-5-20261001": 7}, unknown=1)
        self.assertEqual(acc.identity_equivalence(partly, b)["verdict"], "incomplete")

    def test_claude_code_names_its_models_from_its_messages_and_helpers(self):
        a = acc.parse_cc([TRACES / "cc_main.jsonl"], family="f", seed=0)
        a["meta"]["requested_model"] = "claude-sonnet-5-5"
        ident = metrics_of(a)["model_identity"]
        self.assertTrue(ident["served"])
        self.assertEqual(ident["requested"], ["claude-sonnet-5-5"])
        # No result event, so no billing census: every record named, but not shown complete.
        self.assertEqual((ident["accounted"], ident["complete"]), (None, False))


class IdentityCensusTests(unittest.TestCase):
    """Round 8: an identity is complete only when it accounts for every call an independent
    census saw, and a match needs one and the same model in both arms."""

    def endora(self, relay_statuses, served):
        """An Endora attempt whose relay log (the census) saw calls of `relay_statuses` and
        whose store kept `served` identity rows."""
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        db = Path(d.name) / "hq.db"
        con = sqlite3.connect(db)
        con.executescript("""
        CREATE TABLE members (id INTEGER PRIMARY KEY, name TEXT, brain TEXT);
        CREATE TABLE tasks (id INTEGER PRIMARY KEY, parent INTEGER, status TEXT);
        CREATE TABLE runs (id INTEGER PRIMARY KEY, member INTEGER, task INTEGER, status TEXT,
            attempts INTEGER, queued_at INTEGER, started_at INTEGER, retry_at INTEGER,
            ended_at INTEGER, input_tokens INTEGER, cache_read_tokens INTEGER,
            cache_write_tokens INTEGER, output_tokens INTEGER);
        CREATE TABLE run_served_models (run INTEGER, model TEXT, calls INTEGER);
        """)
        con.execute("INSERT INTO members VALUES (1, 'Sol', 'haiku')")
        con.execute("INSERT INTO tasks VALUES (1, NULL, 'done')")
        ok = sum(1 for s in relay_statuses if s == 200)
        con.execute("INSERT INTO runs VALUES (1, 1, 1, 'succeeded', 1, 0, 1000, NULL, 4000, ?, 0, 0, ?)",
                    (10 * ok, ok))
        con.executemany("INSERT INTO run_served_models VALUES (1, ?, ?)", served)
        con.commit()
        con.close()
        relay = Path(d.name) / "relay.jsonl"
        write_jsonl(relay, [
            {"t_start": float(i), "t_end": i + 0.5, "run": 1, "model": "claude-haiku-5-5",
             "status": st, "input_uncached": 10 if st == 200 else None, "cache_read": 0 if st == 200 else None,
             "cache_write": 0 if st == 200 else None, "output": 1 if st == 200 else None}
            for i, st in enumerate(relay_statuses)])
        return acc.parse_endora(db, 1, relay_log=relay, family="f", seed=0)

    @staticmethod
    def cc(models):
        """A Claude Code identity whose billing census (`modelUsage`) names `models`."""
        return {"arm": "claude-code", "usage": [
            {"model": m, "known": True, "input_uncached": 1, "cache_read": 0, "cache_write": 0,
             "output": 1} for m in models],
            "meta": {"requested_model": "claude-haiku-5-5",
                     "model_usage": {m: {"inputTokens": models.count(m)} for m in set(models)}}}

    def ident(self, a):
        return acc.model_identity(a)

    def test_two_calls_with_one_identity_record_are_not_accounted_for(self):
        i = self.ident(self.endora([200, 200], [("claude-haiku-5-5", 1)]))
        self.assertEqual((i["census"]["answered"], i["accounted"], i["complete"]), (2, False, False))

    def test_a_failed_request_then_a_named_success_is_accounted_for(self):
        i = self.ident(self.endora([529, 200], [("claude-haiku-5-5", 1)]))
        self.assertEqual((i["census"]["answered"], i["census"]["failed"]), (1, 1))
        self.assertTrue(i["complete"], i)

    def test_a_duplicated_usage_report_is_not_accounted_for(self):
        i = self.ident(self.endora([200], [("claude-haiku-5-5", 2)]))
        self.assertFalse(i["accounted"], i)

    def test_no_census_leaves_completeness_unproven(self):
        a = self.endora([200], [("claude-haiku-5-5", 1)])
        a["meta"].pop("census", None)
        i = self.ident(a)
        self.assertEqual((i["accounted"], i["complete"]), (None, False))

    def test_a_helper_with_no_identity_is_not_complete(self):
        a = self.cc(["claude-haiku-5-5", "claude-haiku-5-5"])
        a["usage"].append({"model": None, "known": True, "input_uncached": 1, "cache_read": 0,
                           "cache_write": 0, "output": 1, "agent": "helper"})
        self.assertFalse(self.ident(a)["complete"])

    def test_matching_mixed_model_sets_are_ineligible_never_a_match(self):
        a = self.cc(["claude-haiku-5-5"] * 7 + ["claude-sonnet-5-5"])
        b = self.cc(["claude-haiku-5-5"] + ["claude-sonnet-5-5"] * 6)
        verdict = acc.identity_equivalence(self.ident(a), self.ident(b))["verdict"]
        self.assertEqual(verdict, "mixed")

    def test_one_and_the_same_model_in_both_arms_with_every_call_accounted_for_matches(self):
        e = self.ident(self.endora([200, 200, 200], [("claude-haiku-5-5", 3)]))
        c = self.ident(self.cc(["claude-haiku-5-5"] * 5))
        self.assertEqual(acc.identity_equivalence(e, c)["verdict"], "match")
        other = self.ident(self.cc(["claude-haiku-5-5-20261115"] * 5))
        self.assertEqual(acc.identity_equivalence(e, other)["verdict"], "mismatch")


class UsageCoverageAndNodeCensusTests(unittest.TestCase):
    """Round 9: Claude Code's usage coverage (records against its billing summary, by model and
    by token kind) and Endora's identity from the node's own request census."""

    @staticmethod
    def cc(records, billed):
        return {"arm": "claude-code", "usage": records,
                "meta": {"requested_model": "claude-haiku-5-5", "model_usage": billed}}

    @staticmethod
    def rec(model, i, o, agent="main", status=None):
        r = {"model": model, "agent": agent, "known": True, "input_uncached": i, "cache_read": 0,
             "cache_write": 0, "output": o}
        if status:
            r["status"] = status
        return r

    def test_usage_lost_from_a_model_already_present_is_not_hidden_by_its_name(self):
        billed = {"claude-haiku-5-5": {"inputTokens": 30, "outputTokens": 3,
                                       "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0}}
        whole = self.cc([self.rec("claude-haiku-5-5", 10, 1)] * 3, billed)
        lost = self.cc([self.rec("claude-haiku-5-5", 10, 1)] * 2, billed)
        self.assertTrue(acc.model_identity(whole)["usage_coverage"]["complete"])
        cov = acc.model_identity(lost)["usage_coverage"]
        self.assertFalse(cov["complete"])
        self.assertEqual(cov["unobserved"], {"claude-haiku-5-5": {"input_uncached": 10, "output": 1}})

    def test_a_helper_inferred_from_billing_is_not_an_observed_call(self):
        billed = {"claude-haiku-5-5": {"inputTokens": 1339, "outputTokens": 1473,
                                       "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0}}
        a = self.cc([self.rec("claude-haiku-5-5", 8, 1453),
                     self.rec("claude-haiku-5-5", 1331, 20, agent="helper", status="from_model_usage")],
                    billed)
        cov = acc.model_identity(a)["usage_coverage"]
        self.assertFalse(cov["complete"])
        self.assertEqual(cov["unobserved"], {"claude-haiku-5-5": {"input_uncached": 1331, "output": 20}})

    @staticmethod
    def endora(census_rows, served):
        return {"arm": "endora", "usage": [],
                "meta": {"requested_models": ["claude-haiku-5-5"], "served_models": served,
                         "node_census": census_rows}}

    @staticmethod
    def row(outcome, served="claude-haiku-5-5", hq_told=1):
        return {"run": 1, "outcome": outcome, "served_model": served, "hq_told": hq_told,
                "usage_complete": int(outcome == "answered")}

    def test_the_node_census_names_every_call_even_one_hq_never_heard_of(self):
        a = self.endora([self.row("answered"), self.row("answered", hq_told=0)],
                        [["claude-haiku-5-5", 1]])
        i = acc.model_identity(a)
        self.assertEqual(i["census"]["source"], "node-relay")
        self.assertEqual(i["served"], {"claude-haiku-5-5": 2})
        self.assertTrue(i["complete"], i)
        self.assertEqual(i["hq_discrepancy"], {"census_charged": 2, "hq_recorded": 1, "hq_not_told": 1})

    def test_a_census_that_missed_calls_hq_recorded_is_not_complete(self):
        # Round 10: the observer is best-effort; a row it failed to write must leave the identity
        # unproven, never complete on the rows it did write.
        a = self.endora([self.row("answered")], [["claude-haiku-5-5", 2]])
        i = acc.model_identity(a)
        self.assertFalse(i["complete"], i)
        self.assertEqual(i["hq_discrepancy"]["census_charged"], 1)
        self.assertEqual(i["hq_discrepancy"]["hq_recorded"], 2)

    def test_an_empty_census_is_unavailable_evidence_not_a_complete_one(self):
        a = self.endora([], [["claude-haiku-5-5", 1]])
        self.assertFalse(acc.model_identity(a)["complete"])

    def test_an_unfinished_observation_never_counts_as_complete(self):
        a = self.endora([self.row("answered"), self.row("unfinished", served=None, hq_told=None)],
                        [["claude-haiku-5-5", 1]])
        self.assertFalse(acc.model_identity(a)["complete"])

    def test_a_failed_request_needs_no_model_and_a_cut_one_is_kept_apart(self):
        a = self.endora([self.row("failed", served=None, hq_told=None), self.row("answered"),
                         self.row("interrupted")], [["claude-haiku-5-5", 2]])
        i = acc.model_identity(a)
        self.assertEqual((i["census"]["failed"], i["census"]["interrupted"]), (1, 1))
        self.assertEqual(i["served"], {"claude-haiku-5-5": 2})
        self.assertTrue(i["complete"], i)
        cut_unnamed = self.endora([self.row("answered"), self.row("interrupted", served=None)],
                                  [["claude-haiku-5-5", 1], [None, 1]])
        self.assertFalse(acc.model_identity(cut_unnamed)["complete"])


PARTS = ("agent_active_s", "harness_wait_s", "agent_requested_wait_s", "external_wait_s",
         "unaccounted_s")


def parts_sum(t):
    return sum(t[k] for k in PARTS)


class HqSchedulingWaitTests(unittest.TestCase):
    """HQ's waits come from its event sequence: a retried turn reuses its run row, whose
    queued_at -> started_at spans the whole first attempt (round 21: 13.32 s reported, 1.63 s real)."""

    def build(self, events, attempts=2, started=13200, retry_at=None, ended=30000, with_events=True):
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
        con.execute("INSERT INTO members VALUES (1, 'Jane Doe', 'sonnet')")
        con.execute("INSERT INTO tasks VALUES (1, NULL, 'done')")
        con.execute("INSERT INTO runs VALUES (7, 1, 1, 'succeeded', ?, 0, ?, ?, ?, 10, 20, 30, 40)",
                    (attempts, started, retry_at, ended))
        if with_events:
            con.execute("CREATE TABLE events (id INTEGER PRIMARY KEY, at_ms INTEGER, kind TEXT, data TEXT)")
            con.executemany("INSERT INTO events (at_ms, kind, data) VALUES (?,?,?)",
                            [(int(at * 1000), kind, json.dumps(data)) for at, kind, data in events])
        con.commit()
        con.close()
        return acc.parse_endora(db, 1, family="interrupted", seed=0)

    @staticmethod
    def waits(a, kind=None):
        return sorted((e["wait_kind"], round(e["t_start"], 3), round(e["t_end"], 3))
                      for e in a["events"] if e["kind"] == "wait"
                      and (kind is None or e["wait_kind"] == kind))

    START = [(0.0, "task.created", {"task": 1}), (0.1, "wake.queued", {"task": 1, "reason": "assigned"}),
             (0.4, "run.started", {"run": 7, "task": 1})]
    RETRY_END = (12.0, "run.ended", {"run": 7, "task": 1, "result": "retry_waiting", "interrupted": True})
    LAST_END = (30.0, "run.ended", {"run": 7, "task": 1, "result": "succeeded"})

    def test_initial_admission_only(self):
        a = self.build(self.START + [self.LAST_END], attempts=1, started=400)
        self.assertEqual([w[0] for w in self.waits(a)], ["queue", "queue"])
        t = metrics_of(a)["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 0.4)
        self.assertEqual(a["meta"]["hq_waits"]["source"], "events")
        self.assertEqual(a["meta"]["hq_waits"]["recoveries"], [])

    def test_report_first_recovery_is_harness_waiting_from_the_event_sequence(self):
        a = self.build(self.START + [self.RETRY_END, (12.7, "wake.folded", {"task": 1, "reason": "owner_replied"}),
                                     (13.2, "run.started", {"run": 7, "task": 1}), self.LAST_END])
        recovery = self.waits(a, "recovery")
        self.assertEqual(recovery, [("recovery", 12.0, 12.7), ("recovery", 12.7, 13.2)])
        self.assertEqual([e["part"] for e in a["events"] if e["kind"] == "wait"
                          and e["wait_kind"] == "recovery"], ["news_or_deadline", "admission"])
        rec = a["meta"]["hq_waits"]["recoveries"][0]
        self.assertEqual((rec["order"], rec["status"]), ("report_first", "measured"))
        self.assertEqual((rec["ended_at"], rec["news_at"], rec["started_at"]), (12.0, 12.7, 13.2))
        t = metrics_of(a)["time"]
        # 0.4 first admission + 1.2 recovery; never the reused row's queued_at -> started_at (13.2)
        self.assertAlmostEqual(t["harness_wait_s"], 1.6)
        self.assertNotIn("retry_backoff", {w[0] for w in self.waits(a)})
        self.assertAlmostEqual(parts_sum(t), t["elapsed_s"], places=6)

    def test_reply_first_recovery_waits_for_the_retry_deadline(self):
        a = self.build(self.START + [(11.0, "wake.folded", {"task": 1, "reason": "owner_replied"}),
                                     self.RETRY_END, (13.5, "run.started", {"run": 7, "task": 1}),
                                     self.LAST_END], started=13500, retry_at=13000)
        rec = a["meta"]["hq_waits"]["recoveries"][0]
        self.assertEqual(rec["order"], "reply_first")
        self.assertEqual((rec["ended_at"], rec["news_at"], rec["started_at"]), (12.0, 13.0, 13.5))
        self.assertEqual(self.waits(a, "recovery"), [("recovery", 12.0, 13.0), ("recovery", 13.0, 13.5)])
        t = metrics_of(a)["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 0.4 + 1.5)
        self.assertAlmostEqual(parts_sum(t), t["elapsed_s"], places=6)

    def test_a_retry_with_no_run_started_event_is_left_unaccounted(self):
        a = self.build(self.START + [self.RETRY_END, (12.7, "wake.folded", {"task": 1}), self.LAST_END],
                       started=15000)
        self.assertEqual(self.waits(a, "recovery"), [])
        self.assertEqual(self.waits(a, "retry_backoff"), [])  # never the reused row's span
        rec = a["meta"]["hq_waits"]["recoveries"][0]
        self.assertEqual((rec["status"], rec["started_at"]), ("missing_run_started", None))
        t = metrics_of(a)["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 0.4)
        self.assertAlmostEqual(parts_sum(t), t["elapsed_s"], places=6)

    def test_overlapping_scheduling_intervals_are_counted_once(self):
        a = self.build(self.START + [self.RETRY_END, (12.5, "wake.queued", {"task": 1, "reason": "recovery"}),
                                     (13.0, "run.started", {"run": 7, "task": 1}), self.LAST_END],
                       started=13000)
        self.assertGreater(len(self.waits(a)), 3)  # queue and recovery intervals share seconds
        t = metrics_of(a)["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 0.4 + 1.0)
        self.assertAlmostEqual(parts_sum(t), t["elapsed_s"], places=6)

    def test_a_store_without_these_events_keeps_the_rows_own_span_and_says_so(self):
        a = self.build([], attempts=1, started=5000, ended=30000, with_events=False)
        self.assertEqual(self.waits(a), [("queue", 0.0, 5.0)])
        self.assertEqual(a["meta"]["hq_waits"]["source"], "run_rows")
        b = self.build([(0.0, "task.created", {"task": 99})], attempts=1, started=5000)
        self.assertEqual(self.waits(b), [("queue", 0.0, 5.0)])
        self.assertEqual(b["meta"]["hq_waits"]["source"], "run_rows")


class AgentRequestedWaitTests(unittest.TestCase):
    def test_commands_that_really_sleep_in_order(self):
        s = acc.executed_sleeps
        self.assertEqual(s("sleep 20; cd /work/repo && python3 tools/publish 4.6.4; echo ok"), [(20.0, True)])
        self.assertEqual(s("sleep 2; sleep 3"), [(2.0, True), (3.0, True)])
        self.assertEqual(s("sleep 0.5"), [(0.5, True)])
        self.assertEqual(s("sleep 1m"), [(60.0, True)])
        self.assertEqual(s("python3 -c 'import time; time.sleep(7)'"), [(7.0, True)])
        self.assertEqual(s("make test || sleep 5"), [(5.0, False)])
        self.assertEqual(s("if ping -c1 x; then sleep 5; fi"), [(5.0, False)])

    def test_text_that_only_mentions_sleep_is_not_a_sleep(self):
        s = acc.executed_sleeps
        self.assertEqual(s('echo "sleep 5"'), [])
        self.assertEqual(s("echo 'x; sleep 5'"), [])
        self.assertEqual(s("echo hi # sleep 5"), [])
        self.assertEqual(s("cat <<EOF > notes.txt\nsleep 5\nEOF"), [])
        self.assertEqual(s("exit 1; sleep 5"), [])
        self.assertEqual(s("sleep 5 &"), [])
        self.assertEqual(s("sleep forever"), [])
        self.assertEqual(s("python3 -c \"print('time.sleep(7)')\""), [])
        self.assertEqual(s(""), [])
        self.assertEqual(s(None), [])

    @staticmethod
    def attempt(command, t0=1010.0, t1=1033.0, tool_t=True, extra=()):
        call = {"kind": "tool_call", "tool": "shell", "command": command, "agent": "member:Jane Doe",
                "t_start": t0 if tool_t else None, "t_end": t1 if tool_t else None}
        return {"arm": "endora", "dispatched_at": 1000.0, "ended_at": 1040.0, "terminal": "done",
                "usage": [], "meta": {},
                "events": [{"kind": "model_call", "t_start": 1000.0, "t_end": 1040.0}, call, *extra]}

    def test_a_timed_sleep_is_its_own_part_inside_elapsed(self):
        t = metrics_of(self.attempt("sleep 20; python3 tools/publish 4.6.4"))["time"]
        self.assertEqual(t["elapsed_s"], 40.0)
        self.assertAlmostEqual(t["agent_requested_wait_s"], 20.0)
        self.assertAlmostEqual(t["agent_active_s"], 20.0)
        self.assertEqual(t["external_wait_s"], 0.0)
        self.assertFalse(t["contaminated"])
        self.assertAlmostEqual(parts_sum(t), 40.0, places=6)

    def test_the_part_is_capped_by_the_calls_span(self):
        t = metrics_of(self.attempt("sleep 60", t1=1015.0))["time"]  # the span covers 5 s only
        self.assertEqual(t["agent_requested_wait_s"], 0.0)  # N is not covered by the span: no evidence
        t = metrics_of(self.attempt("sleep 5; sleep 4", t1=1017.0))["time"]
        self.assertAlmostEqual(t["agent_requested_wait_s"], 7.0)  # 9 s asked, span 7 s: capped

    def test_no_timing_means_nothing_attributed_and_a_note(self):
        m = metrics_of(self.attempt("sleep 20; echo hi", tool_t=False))
        t = m["time"]
        self.assertEqual(t["agent_requested_wait_s"], 0.0)
        self.assertEqual(t["agent_active_s"], 40.0)
        notes = t["agent_requested_wait_notes"]
        self.assertEqual((notes[0]["seconds"], notes[0]["attributed"]), (20.0, False))
        self.assertIn("no timing", notes[0]["why"])

    def test_a_sleep_in_a_branch_counts_only_when_the_call_lasted_that_long(self):
        long_call = metrics_of(self.attempt("make test || sleep 5", t1=1016.0))["time"]
        self.assertAlmostEqual(long_call["agent_requested_wait_s"], 5.0)
        short_call = metrics_of(self.attempt("make test || sleep 5", t1=1013.0))["time"]
        self.assertEqual(short_call["agent_requested_wait_s"], 0.0)

    def test_text_mentions_attribute_nothing(self):
        t = metrics_of(self.attempt('echo "sleep 20"'))["time"]
        self.assertEqual(t["agent_requested_wait_s"], 0.0)
        self.assertNotIn("agent_requested_wait_notes", t)

    def test_never_overlaps_harness_or_external_waits(self):
        extra = [{"kind": "wait", "wait_kind": "queue", "t_start": 1010.0, "t_end": 1015.0},
                 {"kind": "wait", "wait_kind": "injected", "t_start": 1015.0, "t_end": 1018.0}]
        t = metrics_of(self.attempt("sleep 20", extra=extra))["time"]
        self.assertAlmostEqual(t["harness_wait_s"], 5.0)
        self.assertAlmostEqual(t["external_wait_s"], 3.0)
        self.assertAlmostEqual(t["agent_requested_wait_s"], 15.0)  # 23 s span less 8 s of other waits
        self.assertAlmostEqual(parts_sum(t), 40.0, places=6)

    def test_endora_transcript_calls_carry_no_times_so_the_sleep_stays_unaccounted(self):
        body = '--- call: shell ---\n{"command": "sleep 20; echo hi"}\n--- result ---\nexit 0\nhi\n'
        calls = acc.transcript_tool_calls(body, "member:Jane Doe")
        self.assertEqual((calls[0]["t_start"], calls[0]["t_end"]), (None, None))
        a = self.attempt("x")
        a["events"] = [{"kind": "model_call", "t_start": 1000.0, "t_end": 1040.0}, *calls]
        t = metrics_of(a)["time"]
        self.assertEqual(t["agent_requested_wait_s"], 0.0)
        self.assertEqual(len(t["agent_requested_wait_notes"]), 1)


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
