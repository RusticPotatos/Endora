#!/usr/bin/env python3
"""Whether a capped project lets a stopped turn be tried again (round 20), on a disposable HQ
with a scripted runner and no model. Python 3 standard library only.

    python3 scripts/harness-probes/admission_check.py --hq-bin PATH [--caps 0.05,0.10] [--charge 2000]

For each cap: a solo member's item in a project with that cap; the runner takes the turn; the
turn is charged `--charge` micros (as the node's relay reports a model call); the runner reports
it killed from outside (137); the owner replies, as the interrupted family's resume does; then
the runner asks for turns. Admitted = the item's turn is handed again. HQ's own rule decides: a
project with less than its SPENT_WITHIN ($0.05) of its cap left is spent and its Claude turns
wait, so a cap of $0.05 admits one turn and never a second. Exit 0 when every cap of at least
`--need` admits the retry and the result is printed as JSON.
"""

import argparse
import json
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import admission_latency as adm  # noqa: E402
import endora_local  # noqa: E402
import recovery_latency as rec  # noqa: E402

RUNNER = endora_local.RUNNER_TOKEN


def one(hq_bin, cap, charge):
    folder = Path(tempfile.mkdtemp(prefix="admission-check-"))
    hq = endora_local.Hq(hq_bin, folder, tick_seconds=1)
    try:
        goal = hq.ok("/v1/goals", {"title": "Admission check", "reason": "local rig"})["goal"]
        hq.ok("/v1/pool/top-up", {"amount_micros": 5_000_000})
        member = hq.ok("/v1/members", {"name": "Sol", "brain": "haiku"})["member"]
        project = hq.ok("/v1/projects", {"goal": goal, "title": "Capped", "reason": "rig", "lead": member,
                                         "solo": member, "cap": str(cap)})["project"]
        task = hq.ok("/v1/tasks", {"goal": goal, "project": project, "title": "Item", "description": "probe",
                                   "criteria": ["It is done."]})["task"]
        _, first = rec.take(hq, task, time.time() + 15)
        if not first:
            return {"cap": cap, "admitted_first": False}
        endora_local.call(hq.url, "/v1/team/usage", {"run": first["run"], "micros": charge, "input": 10,
                                                     "cache_read": 0, "cache_write": 0, "output": 10,
                                                     "served_model": "claude-haiku-5-5"})
        endora_local.call(hq.url, f"/v1/runner/{first['run']}/ended",
                          {"succeeded": False, "lease": first["lease"], "code": 137, "why": "killed from outside"},
                          token=RUNNER)
        rec.wait_event(folder / "hq.db", "run.ended", lambda d: d.get("run") == first["run"], time.time() + 10)
        hq.ok(f"/v1/tasks/{task}/post", {"text": "You were interrupted. Please carry on and finish the change."})
        again_at, again = rec.take(hq, task, time.time() + 15)
        seen = rec.events(folder / "hq.db")
        paused = [d for _, k, d in seen if k == "project.changed" and d.get("pause") == "budget"]
        return {"cap": cap, "charge_micros": charge, "admitted_first": True, "admitted_retry": bool(again),
                "paused_for_budget": bool(paused)}
    finally:
        hq.stop()


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--hq-bin", required=True)
    p.add_argument("--caps", default="0.05,0.10")
    p.add_argument("--charge", type=int, default=2000)
    p.add_argument("--need", type=float, default=0.10, help="caps at least this must admit the retry")
    args = p.parse_args(argv)
    results = [one(args.hq_bin, float(c), args.charge) for c in args.caps.split(",") if c.strip()]
    print(json.dumps(results, indent=1))
    ok = all(r.get("admitted_retry") for r in results if r["cap"] >= args.need)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
