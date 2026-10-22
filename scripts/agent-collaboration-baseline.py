#!/usr/bin/env python3
"""Summarize an offline HQ logical capture without printing private record text.

Input: {observed_ms, tables: {runs, wakes, ledger, events, transcripts, consults}}.
The capture must be made in one read-only SQLite transaction and kept ignored.
This tool never connects to HQ, reads credentials, or changes a database.
Costs describe completed turns whose stored latest start is in the chosen window,
including all recorded retries. They are not spend booked during the wall-clock
window, first-start cohorts, or a causal waste estimate: retries overwrite started_at.
"""

import argparse
from collections import defaultdict
import json
from pathlib import Path
import sys


REASONS = frozenset("""
assigned mentioned wait_finished subtasks_finished retry recovery assignment_recovery
check_back owner_replied question_answered wait_dropped sent_back check_done downtime
consulted proposal_decided lead_review alarm review_requested question_asked
acceptance_asked directed decide huddle guard
""".split())
LABELS = frozenset(["advanced", "checked", "blocked", "talk only", "empty"])
# Group model families instead of copying untrusted model strings to public output.
BRAINS = frozenset(["local", "mac", "claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"])


def rows(capture, table):
    """A required, present table; absence is never interpreted as an empty record."""
    found = capture.get("tables", {}).get(table)
    if not isinstance(found, list):
        raise ValueError("capture is missing a required table")
    return found


def indexed(items, key):
    """Reject duplicate identities instead of silently losing rows."""
    result = {}
    for item in items:
        value = item[key]
        if not isinstance(value, int) or isinstance(value, bool) or value in result:
            raise ValueError("invalid or duplicate record identity")
        result[value] = item
    return result


def summarize(capture, hours=72):
    """Return numeric evidence, whitelisted classifications and explicit unknowns."""
    now = capture.get("observed_ms")
    if not isinstance(now, int) or isinstance(now, bool) or now < 0:
        raise ValueError("capture needs its observation time in milliseconds")
    if not isinstance(hours, int) or isinstance(hours, bool) or not 1 <= hours <= 24 * 30:
        raise ValueError("window must be 1 to 720 hours")
    since = now - hours * 3_600_000
    runs = indexed(rows(capture, "runs"), "id")
    wakes = indexed(rows(capture, "wakes"), "id")
    ledger = rows(capture, "ledger")
    events = rows(capture, "events")
    transcripts = rows(capture, "transcripts")
    consults = rows(capture, "consults")
    charges, uses = defaultdict(int), defaultdict(int)
    reserves, releases = defaultdict(int), defaultdict(int)
    witnessed = set()
    for entry in ledger:
        amount = entry["amount"]
        if not isinstance(amount, int) or isinstance(amount, bool) or amount < 0:
            raise ValueError("invalid ledger amount")
        if entry["kind"] == "charge":
            charges[entry["run"]] += amount
        if entry["kind"] == "use":
            uses[entry["run"]] += amount
        if entry["kind"] == "reserve":
            reserves[entry["run"]] += amount
            witnessed.add(entry["run"])
        if entry["kind"] == "release":
            releases[entry["run"]] += amount
    in_window = {
        key: run for key, run in runs.items()
        if run["started_at"] is not None and since <= run["started_at"] <= now
    }
    completed = {
        key: run for key, run in in_window.items()
        if run["ended_at"] is not None and run["ended_at"] <= now
    }
    unfinished = {key: run for key, run in in_window.items() if key not in completed}
    mismatched = [
        key for key, run in completed.items()
        if (charges[key] != run["spent"] or uses[key] != charges[key]
            or reserves[key] != releases[key])
    ]
    unwitnessed = sorted(set(completed) - witnessed)
    missing_wake = sum(run["wake"] not in wakes for run in completed.values())
    started = set()
    for event in events:
        if event["kind"] == "run.started":
            data = event["data"]
            data = json.loads(data) if isinstance(data, str) else data
            started.add(data.get("run"))
    transcript_runs = {item["run"] for item in transcripts}
    missing_brain = sum(run.get("brain") not in BRAINS for run in completed.values())
    missing_start = len(set(completed) - started)
    missing_transcript = len(set(completed) - transcript_runs)
    grouped = {}
    for name, values, allowed in [
        ("by_initial_wake", {key: wakes.get(run["wake"], {}).get("reason")
                             for key, run in completed.items()}, REASONS),
        ("by_label", {key: run.get("label") for key, run in completed.items()}, LABELS),
        ("by_recorded_brain", {key: run.get("brain") for key, run in completed.items()}, BRAINS),
    ]:
        groups = {}
        for key, value in values.items():
            safe = value if value in allowed else "unknown"
            group = groups.setdefault(safe, {"runs": 0, "charge_micros": 0})
            group["runs"] += 1
            group["charge_micros"] += charges[key]
        grouped[name] = groups
    answered = [item for item in consults
                if item["status"] == "answered" and item["answer_run"] in completed]
    return {
        "window": {"since_ms": since, "observed_ms": now, "hours": hours},
        "completed": {"runs": len(completed),
                      "charge_micros": sum(charges[key] for key in completed),
                      "usage_micros": sum(uses[key] for key in completed),
                      "spent_micros": sum(run["spent"] for run in completed.values())},
        "unfinished": {"runs": len(unfinished),
                       "usage_micros": sum(uses[key] for key in unfinished)},
        "accounting": {"verdict": "fail" if mismatched else
                       "cannot_tell" if unwitnessed or not completed else "pass",
                       "mismatched_run_ids": sorted(mismatched),
                       "without_reservation_row": unwitnessed},
        "coverage": {"verdict": "cannot_tell" if not completed or any([
                         missing_wake, missing_start, missing_brain, missing_transcript,
                     ]) else "pass",
                     "without_initial_wake": missing_wake,
                     "without_start_event": missing_start,
                     "without_recorded_brain": missing_brain,
                     "without_transcript": missing_transcript,
                     "cut_transcripts": sum(bool(item["cut"]) for item in transcripts
                                            if item["run"] in completed),
                     "negative_queue_duration": sum(run["queued_at"] > run["started_at"]
                                                    for run in completed.values())},
        "consults": {"answered_in_cohort": len(answered),
                     "answered_with_empty_label": sum(
                         completed[item["answer_run"]]["label"] == "empty" for item in answered)},
        **grouped,
    }


def main():
    """Read a local capture, print only its bounded summary, and report accounting errors."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--hours", type=int, default=72)
    args = parser.parse_args()
    try:
        report = summarize(json.loads(args.capture.read_text()), args.hours)
    except (OSError, ValueError, KeyError, TypeError):
        print("baseline: cannot tell; invalid or incomplete capture (private details withheld)",
              file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["accounting"]["verdict"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
