#!/usr/bin/env python3
"""How long the owner's new work waits for HQ to take it up: from the item's creation
(`task.created`) to its turn being queued (`wake.queued`) and handed to a runner, measured on a
disposable HQ, with no model and no worker. Python 3 standard library only.

    python3 scripts/harness-probes/admission_latency.py --hq-bin PATH --out FILE \
        [--tick-seconds 30] [--phases 2,9,16,23] [--burst 3]

Each phase runs on a fresh HQ: it waits for a tick to finish (HQ's `/health` says when), waits
`phase` seconds more, then gives a solo member one item and measures. `--burst N` adds a run
that gives N members an item each at once, to check that one pass takes them all up and that no
item is queued twice. The ticks HQ ran while idle are counted over the same span, so a change
that polled more would show.
"""

import argparse
import json
import sqlite3
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import endora_local  # noqa: E402

RUNNER = endora_local.RUNNER_TOKEN


def setup(hq, members):
    goal = hq.ok("/v1/goals", {"title": "Admission latency", "reason": "local rig"})["goal"]
    hq.ok("/v1/pool/top-up", {"amount_micros": 5_000_000})
    made = []
    for name in members:
        member = hq.ok("/v1/members", {"name": name, "brain": "haiku"})["member"]
        project = hq.ok("/v1/projects", {"goal": goal, "title": f"Solo {name}", "reason": "rig",
                                         "lead": member, "solo": member, "cap": "1"})["project"]
        made.append((member, project))
    return goal, made


def last_tick(hq):
    return endora_local.call(hq.url, "/health")[1].get("last_tick_at") or 0


def wait_for_a_tick(hq, timeout=120):
    before = last_tick(hq)
    end = time.time() + timeout
    while time.time() < end:
        now = last_tick(hq)
        if now != before:
            return now / 1000.0
        time.sleep(0.05)
    raise RuntimeError("no tick")


def events(db):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        return [(at / 1000.0, kind, json.loads(data)) for at, kind, data in
                con.execute("SELECT at_ms, kind, data FROM events ORDER BY id")]
    finally:
        con.close()


def measure(hq, db, tasks, deadline_s):
    """For each task: seconds from task.created to its first wake.queued and to the runner
    being handed its turn (polling /v1/runner/next as a free runner would)."""
    handed = {}
    end = time.time() + deadline_s
    while time.time() < end and len(handed) < len(tasks):
        got, turn = endora_local.call(hq.url, "/v1/runner/next", {"free": 1}, token=RUNNER)
        if got == 200:
            if turn.get("task") in tasks and turn["task"] not in handed:
                handed[turn["task"]] = time.time()
            # Every turn handed out (a downtime turn too) is handed back as finished, so the
            # runner is free again; nothing runs.
            endora_local.call(hq.url, f"/v1/runner/{turn['run']}/ended",
                              {"succeeded": True, "lease": turn["lease"], "code": 0}, token=RUNNER)
            continue
        time.sleep(0.05)
    seen = events(db)
    out = {}
    for t in tasks:
        created = next((at for at, k, d in seen if k == "task.created" and d.get("task") == t), None)
        queued = [at for at, k, d in seen if k == "wake.queued" and d.get("task") == t]
        started = [at for at, k, d in seen if k == "run.started" and d.get("task") == t]
        out[t] = {
            "created_to_queued_s": (queued[0] - created) if queued and created else None,
            "created_to_run_started_s": (started[0] - created) if started and created else None,
            "created_to_runner_handed_s": (handed[t] - created) if t in handed and created else None,
            "wakes_queued": len(queued),
            "runs_started": len(started),
        }
    return out


def one_run(args, phase, members):
    folder = Path(tempfile.mkdtemp(prefix="admission-"))
    hq = endora_local.Hq(args.hq_bin, folder, tick_seconds=args.tick_seconds)
    try:
        goal, made = setup(hq, members)
        tick_at = wait_for_a_tick(hq)
        time.sleep(max(0.0, tick_at + phase - time.time()))
        tasks = []
        for i, (member, project) in enumerate(made):
            tasks.append(hq.ok("/v1/tasks", {
                "goal": goal, "project": project, "title": f"Item {i}", "description": "probe",
                "criteria": ["It is done."]})["task"])
        result = measure(hq, folder / "hq.db", tasks, args.tick_seconds * 2 + 10)
        ticks = [at for at, k, d in events(folder / "hq.db") if k == "tick"]
        return {"phase_s": phase, "members": len(members), "tasks": result,
                "tick_events": len(ticks)}
    finally:
        hq.stop()


def idle_ticks(args, span_s):
    """Ticks an idle HQ finishes over `span_s` seconds (from /health), to show no new polling."""
    folder = Path(tempfile.mkdtemp(prefix="admission-idle-"))
    hq = endora_local.Hq(args.hq_bin, folder, tick_seconds=args.tick_seconds)
    try:
        setup(hq, ["Idle"])
        seen, end, last = 0, time.time() + span_s, last_tick(hq)
        while time.time() < end:
            now = last_tick(hq)
            if now != last:
                seen, last = seen + 1, now
            time.sleep(0.05)
        return {"span_s": span_s, "ticks_finished": seen}
    finally:
        hq.stop()


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--hq-bin", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--tick-seconds", type=int, default=30)
    p.add_argument("--phases", default="2,9,16,23")
    p.add_argument("--burst", type=int, default=3)
    p.add_argument("--idle-span", type=float, default=95)
    args = p.parse_args(argv)
    report = {"hq_bin": args.hq_bin, "tick_seconds": args.tick_seconds, "runs": []}
    for phase in [float(x) for x in args.phases.split(",") if x.strip()]:
        report["runs"].append(one_run(args, phase, ["Sol"]))
        print(json.dumps(report["runs"][-1]), flush=True)
    if args.burst > 1:
        report["burst"] = one_run(args, 5.0, [f"Member{i}" for i in range(args.burst)])
        print(json.dumps(report["burst"]), flush=True)
    if args.idle_span > 0:
        report["idle"] = idle_ticks(args, args.idle_span)
        print(json.dumps(report["idle"]), flush=True)
    Path(args.out).write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
