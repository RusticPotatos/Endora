#!/usr/bin/env python3
"""How long a stopped turn waits for HQ to take it up again (round 17), measured on a disposable
HQ with a scripted runner, no model and no worker. Python 3 standard library only.

    python3 scripts/harness-probes/recovery_latency.py --hq-bin PATH --out FILE \
        [--tick-seconds 10] [--phases 1,4,7]

For each phase, on a fresh HQ: an item is given to a solo member, the runner takes its turn,
then `phase` seconds after a tick the runner reports the turn killed from outside (137), as the
interrupted family's stop does. Measured, from HQ's own events and the runner's view:

- report -> settled: the report accepted, until `run.ended` (retry_waiting);
- reply -> handed: the owner's reply posted at once after settling, until the runner is handed
  the turn again (polling /v1/runner/next as a free runner would);
- each attempt handed once: no run is handed twice under one lease, and the retried run is
  handed exactly once.

`--order reply-first` posts the reply while the turn still runs and reports the end after it,
to show what the retry policy does then (reported as measured, not changed). The ticks an idle
HQ finishes over `--idle-span` seconds are counted, so a change that polled more would show.
"""

import argparse
import json
import sqlite3
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import admission_latency as adm  # noqa: E402
import endora_local  # noqa: E402

RUNNER = endora_local.RUNNER_TOKEN


def events(db):
    return adm.events(db)


def take(hq, task, deadline):
    """Polls /v1/runner/next until the turn on `task` is handed; other turns are handed back."""
    while time.time() < deadline:
        got, turn = endora_local.call(hq.url, "/v1/runner/next", {"free": 1}, token=RUNNER)
        if got == 200:
            if turn.get("task") == task:
                return time.time(), turn
            endora_local.call(hq.url, f"/v1/runner/{turn['run']}/ended",
                              {"succeeded": True, "lease": turn["lease"], "code": 0}, token=RUNNER)
            continue
        time.sleep(0.02)
    return None, None


def wait_event(db, kind, pred, deadline):
    while time.time() < deadline:
        for at, k, d in events(db):
            if k == kind and pred(d):
                return at
        time.sleep(0.02)
    return None


def one_run(args, phase, order):
    folder = Path(tempfile.mkdtemp(prefix="recovery-"))
    hq = endora_local.Hq(args.hq_bin, folder, tick_seconds=args.tick_seconds)
    db = folder / "hq.db"
    try:
        goal, made = adm.setup(hq, ["Sol"])
        member, project = made[0]
        task = hq.ok("/v1/tasks", {"goal": goal, "project": project, "title": "Item",
                                   "description": "probe", "criteria": ["It is done."]})["task"]
        _, first = take(hq, task, time.time() + args.tick_seconds * 3)
        assert first, "the first turn was never handed"
        handed = [(first["run"], first["lease"])]
        tick_at = adm.wait_for_a_tick(hq)
        time.sleep(max(0.0, tick_at + phase - time.time()))
        out = {"phase_s": phase, "order": order, "run": first["run"]}
        reply = {"text": "You were interrupted. Please carry on and finish the change."}
        if order == "reply-first":
            out["reply_at"] = time.time()
            hq.ok(f"/v1/tasks/{task}/post", reply)
        report_at = time.time()
        code, _ = endora_local.call(hq.url, f"/v1/runner/{first['run']}/ended",
                                    {"succeeded": False, "lease": first["lease"], "code": 137,
                                     "why": "killed from outside"}, token=RUNNER)
        out["report_status"] = code
        settled = wait_event(db, "run.ended", lambda d: d.get("run") == first["run"],
                             time.time() + args.tick_seconds * 2 + 5)
        out["report_to_settled_s"] = (settled - report_at) if settled else None
        if order == "report-first":
            out["reply_at"] = time.time()
            hq.ok(f"/v1/tasks/{task}/post", reply)
        again_at, again = take(hq, task, time.time() + args.retry_wait_s)
        out["reply_to_handed_s"] = (again_at - out["reply_at"]) if again_at else None
        out["report_to_handed_s"] = (again_at - report_at) if again_at else None
        if again:
            handed.append((again["run"], again["lease"]))
            endora_local.call(hq.url, f"/v1/runner/{again['run']}/ended",
                              {"succeeded": True, "lease": again["lease"], "code": 0}, token=RUNNER)
        # anything more handed for the task in the next few seconds would be a duplicate
        extra_at, extra = take(hq, task, time.time() + 3)
        out["handed"] = handed + ([(extra["run"], extra["lease"])] if extra else [])
        out["duplicate_handed"] = len(set(out["handed"])) != len(out["handed"]) or bool(extra)
        seen = events(db)
        out["run_started_events"] = sum(1 for _, k, d in seen if k == "run.started" and d.get("task") == task)
        out["ticks"] = sum(1 for _, k, _d in seen if k == "tick")
        return out
    finally:
        hq.stop()


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--hq-bin", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--tick-seconds", type=int, default=10)
    p.add_argument("--phases", default="1,4,7")
    p.add_argument("--orders", default="report-first,reply-first")
    p.add_argument("--retry-wait-s", type=float, default=75)
    p.add_argument("--idle-span", type=float, default=35)
    args = p.parse_args(argv)
    report = {"hq_bin": args.hq_bin, "tick_seconds": args.tick_seconds, "runs": []}
    for order in [o for o in args.orders.split(",") if o]:
        for phase in [float(x) for x in args.phases.split(",") if x.strip()]:
            report["runs"].append(one_run(args, phase, order))
            print(json.dumps(report["runs"][-1]), flush=True)
    if args.idle_span > 0:
        report["idle"] = adm.idle_ticks(args, args.idle_span)
        print(json.dumps(report["idle"]), flush=True)
    Path(args.out).write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
