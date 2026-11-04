#!/usr/bin/env python3
"""Collects one live Endora attempt of a probe after it ends: a read-only copy of HQ's store,
the submitted tree from the node's mirror of the probe's outside repository, the attempt
record (accounting.py) and the grade (probes.py). Python 3 standard library and the docker CLI.

    python3 scripts/harness-probes/endora_live.py --family F --seed S --task 1075 \
        --repo-name OWNER_NAME --dispatched-at EPOCH --out DIR [--context nas]

The probe reaches the live workbench as a public outside repository (ADR 0115, amended
2026-11-01); the member's branch is kept in the node's mirror as `refs/submissions/<branch>`
and pushed nowhere. This reads that ref, never the member's sandbox. Nothing here writes to
HQ or the node: the store is copied with SQLite's backup API inside HQ's container, and the
mirror is read with `git bundle`.
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import accounting  # noqa: E402
import rig  # noqa: E402

# HQ's container has a read-only root: the store is copied into memory there and streamed out.
COPY_STORE = (
    "import sqlite3, sys; s=sqlite3.connect('file:/data/hq.db?mode=ro', uri=True); "
    "d=sqlite3.connect(':memory:'); s.backup(d); sys.stdout.buffer.write(d.serialize())"
)

# What a copy keeps of the store: this probe's rows only, so the climb's evidence holds nothing
# else of the house's.
PRUNE = """
DELETE FROM transcripts WHERE task IS NULL OR task NOT IN (SELECT id FROM keep_tasks);
DELETE FROM posts WHERE task NOT IN (SELECT id FROM keep_tasks);
"""


def prune(db, task):
    import sqlite3
    con = sqlite3.connect(db)
    con.execute("CREATE TEMP TABLE keep_tasks AS WITH RECURSIVE t(id) AS (SELECT ? UNION ALL "
                "SELECT tasks.id FROM tasks JOIN t ON tasks.parent=t.id) SELECT id FROM t", (task,))
    con.executescript(PRUNE)
    con.commit()
    con.execute("VACUUM")
    con.close()


def docker(context, *args, **kw):
    return subprocess.run(["docker", "--context", context, *args], check=True,
                          capture_output=True, text=True, **kw)


def run(args):
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    probe = out / "probe"
    task = rig.make_probe(args.family, args.seed, probe)
    start = (probe / "repo")

    # HQ's store, copied read-only.
    with open(out / "hq.db", "wb") as sink:
        subprocess.run(["docker", "--context", args.context, "exec", args.hq, "python3", "-c",
                        COPY_STORE], check=True, stdout=sink)
    prune(out / "hq.db", args.task)

    # The submitted branch, from the node's mirror of the outside repository.
    # The node keeps the mirror as a bare repository, under the repository's lower-cased name.
    mirror = f"/data/outside-repos/{args.repo_name.lower()}/mirror.git"
    try:
        refs = docker(args.context, "exec", args.node, "git", "-C", mirror, "for-each-ref",
                      "--format=%(refname) %(objectname)", "refs/submissions/").stdout.split("\n")
    except subprocess.CalledProcessError:
        refs = []  # no mirror yet: the turn never checked the repository out
    refs = [r.split() for r in refs if r.strip()]
    collected = {"submissions": refs}
    if refs:
        docker(args.context, "exec", args.node, "git", "-C", mirror, "bundle", "create",
               "/tmp/harness-probe.bundle", *[r[0] for r in refs], "HEAD")
        docker(args.context, "cp", f"{args.node}:/tmp/harness-probe.bundle", str(out / "submission.bundle"))
        docker(args.context, "exec", args.node, "rm", "-f", "/tmp/harness-probe.bundle")
        # This task's own submission (`refs/submissions/hq/task-<N>-…`), never another's.
        mine = [r for r in refs if f"/task-{args.task}-" in r[0]]
    else:
        mine = []
    if mine:
        ref, sha = mine[-1]
        # The generated start is the same tree as the outside repo's main; its commit differs
        # only by when it was made. The grader judges file content, so the submitted tree's
        # files go over the generated start's.
        subprocess.run(["git", "-C", str(start), "fetch", str(out / "submission.bundle"),
                        f"{ref}:refs/submitted"], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(start), "checkout", "-q", "refs/submitted", "--", "."],
                       check=True)
        collected["graded_ref"] = ref
        collected["graded_sha"] = sha
    else:
        # No submission of this task (already_satisfied, impossible): what is graded is the
        # starting tree as it stands, unchanged, and the record says so.
        collected["graded_ref"] = None
        collected["graded_sha"] = None
        collected["no_submission"] = True
    (out / "collected.json").write_text(json.dumps(collected, indent=2) + "\n")

    # The node's own count of the attempt's model requests (round 9), read-only, through the
    # node container's Python: its runs are the task tree's, from the store just copied.
    import sqlite3
    con = sqlite3.connect(out / "hq.db")
    runs = [r[0] for r in con.execute(
        "WITH RECURSIVE t(id) AS (SELECT ? UNION ALL SELECT tasks.id FROM tasks JOIN t "
        "ON tasks.parent=t.id) SELECT runs.id FROM runs JOIN t ON runs.task = t.id", (args.task,))]
    con.close()
    census = None
    if runs:
        exported = subprocess.run(
            ["docker", "--context", args.context, "exec", "-i", args.node, "python3", "-",
             "--runs", ",".join(map(str, runs))],
            input=(HERE / "export_census.py").read_text(), capture_output=True, text=True)
        collected["census_exit"] = exported.returncode
        if exported.returncode == 0:
            census = out / "node-census.json"
            census.write_text(exported.stdout)
        (out / "collected.json").write_text(json.dumps(collected, indent=2) + "\n")
    attempt = accounting.parse_endora(out / "hq.db", args.task, family=args.family, seed=args.seed,
                                      node_census=census)
    # HQ's event log dates the item's creation and its terminal move; the rig's own clock is
    # the fallback for a store without one (it stamps the request a few seconds early).
    attempt["meta"]["rig_dispatched_at"] = args.dispatched_at
    if attempt.get("dispatched_at") is None:
        attempt["dispatched_at"] = args.dispatched_at
    if attempt.get("ended_at") is None:
        attempt["ended_at"] = time.time()
    if args.deadline_at is not None:
        apply_deadline(attempt, args.deadline_at, not args.deadline_reached, args.teardown_at)
    attempt_path = out / "attempt.json"
    attempt_path.write_text(json.dumps(attempt, indent=2) + "\n")
    # tool_failure: the side effects as the item's observer recorded them, exported by the rig
    # through the runner host's Docker (round 18); without them the criterion is unknown.
    verdict = rig.grade(args.family, args.seed, probe, attempt_path, observations=args.observations)
    (out / "grade.json").write_text(json.dumps(verdict, indent=2) + "\n")
    print(json.dumps({"collected": collected, "terminal": attempt["terminal"],
                      "artifact_pass": verdict.get("artifact_pass"), "pass": verdict.get("pass"),
                      "reasons": verdict.get("reasons")}, indent=2))
    return 0


def apply_deadline(attempt, deadline_at, finished_by_deadline, teardown_at):
    """An attempt still going at its deadline ends there, as a timeout (round 20): the rig's
    teardown after it (the item put away) and its collection are recorded apart, never elapsed."""
    attempt.setdefault("meta", {})["deadline"] = {"at": deadline_at, "reached": not finished_by_deadline}
    if finished_by_deadline:
        return attempt
    attempt["meta"]["deadline"].update({"teardown_at": teardown_at, "terminal_before": attempt.get("terminal"),
                                        "ended_at_before": attempt.get("ended_at")})
    attempt["terminal"] = "timeout"
    attempt["ended_at"] = deadline_at
    return attempt


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--family", required=True)
    p.add_argument("--seed", type=int, required=True)
    p.add_argument("--task", type=int, required=True)
    p.add_argument("--repo-name", required=True, help="the mirror's folder, OWNER_NAME")
    p.add_argument("--dispatched-at", type=float, required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--context", default="nas")
    p.add_argument("--hq", default="endora-hq")
    p.add_argument("--node", default="endora-node")
    p.add_argument("--observations", help="tool_failure: the observer's exported records (JSON list)")
    p.add_argument("--deadline-at", type=float, help="the attempt's deadline (epoch seconds)")
    p.add_argument("--deadline-reached", action="store_true", help="the item was still going at the deadline")
    p.add_argument("--teardown-at", type=float, help="when the rig put the item away after the deadline")
    return run(p.parse_args(argv))


if __name__ == "__main__":
    sys.exit(main())
