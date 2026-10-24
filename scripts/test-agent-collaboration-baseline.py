#!/usr/bin/env python3
"""Offline checks for baseline accounting, coverage and output privacy."""

import copy
import importlib.util
import json
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "baseline", Path(__file__).with_name("agent-collaboration-baseline.py")
)
BASELINE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASELINE)


def capture():
    """A completed, paid turn with one initial wake and all settlement rows."""
    return {
        "observed_ms": 400_000_000,
        "tables": {
            "runs": [{
                "id": 1, "wake": 10, "started_at": 399_000_000,
                "ended_at": 399_100_000, "queued_at": 398_999_000,
                "spent": 120, "brain": "local", "label": "talk only",
                "status": "succeeded", "attempts": 1,
            }],
            "wakes": [{"id": 10, "run": 1, "reason": "question_asked"}],
            "ledger": [
                {"run": 1, "kind": "reserve", "amount": 400},
                {"run": 1, "kind": "use", "amount": 120},
                {"run": 1, "kind": "charge", "amount": 120},
                {"run": 1, "kind": "release", "amount": 400},
            ],
            "transcripts": [{"run": 1, "cut": 0}],
            "events": [{"kind": "run.started", "data": '{"run":1}'}],
            "consults": [],
        },
    }


class BaselineTests(unittest.TestCase):
    def test_charges_are_not_added_to_reservations_usage_or_releases(self):
        report = BASELINE.summarize(capture())
        self.assertEqual(report["completed"]["charge_micros"], 120)
        self.assertEqual(report["accounting"]["verdict"], "pass")
        self.assertEqual(report["by_initial_wake"]["question_asked"], {
            "runs": 1, "charge_micros": 120,
        })

    def test_corrupted_settlement_is_a_failure(self):
        data = capture()
        data["tables"]["ledger"][2]["amount"] = 121
        report = BASELINE.summarize(data)
        self.assertEqual(report["accounting"]["verdict"], "fail")
        self.assertEqual(report["accounting"]["mismatched_run_ids"], [1])

    def test_window_is_started_turn_cohort_not_charge_time(self):
        data = capture()
        older = copy.deepcopy(data["tables"]["runs"][0])
        older.update(id=2, started_at=1, ended_at=399_500_000)
        data["tables"]["runs"].append(older)
        data["tables"]["ledger"].append({"run": 2, "kind": "charge", "amount": 900})
        self.assertEqual(BASELINE.summarize(data)["completed"]["runs"], 1)
        self.assertEqual(BASELINE.summarize(data)["completed"]["charge_micros"], 120)

    def test_unfinished_work_reports_incurred_usage_separately(self):
        data = capture()
        data["tables"]["runs"][0].update(ended_at=None, status="running")
        data["tables"]["ledger"] = data["tables"]["ledger"][:2]
        report = BASELINE.summarize(data)
        self.assertEqual(report["completed"]["charge_micros"], 0)
        self.assertEqual(report["unfinished"], {"runs": 1, "usage_micros": 120})

    def test_missing_events_transcripts_and_model_are_unknown_not_success(self):
        data = capture()
        data["tables"]["events"] = []
        data["tables"]["transcripts"] = []
        data["tables"]["runs"][0]["brain"] = None
        report = BASELINE.summarize(data)
        self.assertEqual(report["coverage"]["verdict"], "cannot_tell")
        self.assertEqual(report["coverage"]["without_start_event"], 1)
        self.assertEqual(report["coverage"]["without_transcript"], 1)
        self.assertEqual(report["coverage"]["without_recorded_brain"], 1)

    def test_useful_consult_answers_are_not_inferred_from_turn_labels(self):
        data = capture()
        data["tables"]["runs"][0]["label"] = "empty"
        data["tables"]["consults"] = [{"status": "answered", "answer_run": 1}]
        report = BASELINE.summarize(data)
        self.assertEqual(report["consults"]["answered_in_cohort"], 1)
        self.assertEqual(report["consults"]["answered_with_empty_label"], 1)
        self.assertNotIn("wasted", json.dumps(report))

    def test_unknown_enum_and_private_input_text_never_reach_report(self):
        data = capture()
        for field in ["brain", "label", "status"]:
            data["tables"]["runs"][0][field] = "private-content-canary"
        data["tables"]["wakes"][0]["reason"] = "private-content-canary"
        data["tables"]["runs"][0]["told"] = "private-content-canary"
        data["tables"]["transcripts"][0]["body"] = "private-content-canary"
        self.assertNotIn("private-content-canary", json.dumps(BASELINE.summarize(data)))

    def test_missing_ledger_or_duplicate_run_fails_loudly(self):
        for table in ["ledger", "wakes"]:
            data = capture()
            del data["tables"][table]
            with self.assertRaises(ValueError):
                BASELINE.summarize(data)
        data = capture()
        data["tables"]["runs"].append(copy.deepcopy(data["tables"]["runs"][0]))
        with self.assertRaises(ValueError):
            BASELINE.summarize(data)

    def test_no_charge_row_cannot_pass_even_when_spent_is_zero(self):
        data = capture()
        data["tables"]["ledger"] = []
        data["tables"]["runs"][0]["spent"] = 0
        self.assertEqual(BASELINE.summarize(data)["accounting"]["verdict"], "cannot_tell")

    def test_zero_cost_turn_is_witnessed_by_zero_reservation_not_a_charge(self):
        data = capture()
        data["tables"]["runs"][0]["spent"] = 0
        data["tables"]["ledger"] = [{"run": 1, "kind": "reserve", "amount": 0}]
        report = BASELINE.summarize(data)
        self.assertEqual(report["accounting"]["verdict"], "pass")

    def test_ended_turn_with_money_still_reserved_is_a_failure(self):
        data = capture()
        data["tables"]["ledger"] = data["tables"]["ledger"][:-1]
        self.assertEqual(BASELINE.summarize(data)["accounting"]["verdict"], "fail")

    def test_an_empty_cohort_cannot_certify_a_baseline(self):
        data = capture()
        data["tables"]["runs"] = []
        self.assertEqual(BASELINE.summarize(data)["accounting"]["verdict"], "cannot_tell")
        self.assertEqual(BASELINE.summarize(data)["coverage"]["verdict"], "cannot_tell")


if __name__ == "__main__":
    unittest.main()
