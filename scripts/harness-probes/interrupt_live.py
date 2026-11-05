#!/usr/bin/env python3
"""The interrupted family's live adapter for Endora (round 16): watches the runner's Docker for
the attempt's own turn container (the one with a run label among those mounting the item's
sandbox volume, endora-item-<task>), reads the watched file from inside it, and at the family's
milestone (rig.milestone_reached: the first complete, valid step 1) stops that container, as
anything outside the worker stopping it would: the runner sees it end (137) and tells HQ. The
item's kept sandbox (its keeper, ADR 0115) holds /work, so the tree is read again from the
keeper right after the stop. Nothing else is touched: only a turn on this item's sandbox is a
target; the keeper (item label, no run) and check jobs never are.

    interrupt_live.py --task 1200 --file pkg/model.py --milestone '{...}' \\
        --docker "ssh runner.example.com docker" --out DIR [--timeout 600]

Writes DIR/interrupt.json: when the turn was first seen, every change of the file seen (time
and digest), when the milestone was seen, when the stop was sent and returned, the stopped
container's end, and the tree before the stop and after it (git status and diff). Python 3
standard library."""

import argparse
import hashlib
import json
import shlex
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rig  # noqa: E402

RUN_LABEL, ITEM_LABEL = "endora.run", "endora.item"


def pick(ps_lines, task):
    """(turn container id and run, keeper id) for `task`, from `docker ps` lines of
    "<id>\\t<labels>" already filtered to the item's volume: the runner labels a turn with its
    run (and lease) only, and the keeper with the item."""
    turn = keeper = None
    for line in ps_lines:
        if "\t" not in line:
            continue
        cid, labels = line.split("\t", 1)
        kv = dict(p.split("=", 1) for p in labels.split(",") if "=" in p)
        if "endora.check" in kv:
            continue
        if RUN_LABEL in kv:
            turn = turn or (cid, kv[RUN_LABEL])
        elif kv.get(ITEM_LABEL) == str(task):
            keeper = keeper or cid
    return turn, keeper


class Docker:
    def __init__(self, prefix):
        self.prefix = shlex.split(prefix)

    def argv(self, *args):
        """ssh hands the remote shell one string, which splits it again: so after an ssh
        destination the docker command goes as one quoted string (round 16's live failure)."""
        if self.prefix and self.prefix[0] == "ssh":
            at = self.prefix.index("docker") if "docker" in self.prefix else len(self.prefix)
            return self.prefix[:at] + [shlex.join(self.prefix[at:] + list(args))]
        return self.prefix + list(args)

    def __call__(self, *args, timeout=30):
        p = subprocess.run(self.argv(*args), capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr

    def ps(self, task):
        """Lines of `docker ps` for the containers mounting `task`'s sandbox (the runner's
        endora-item-<task> volume); raises when docker itself fails, so a watch is never silent."""
        code, out, err = self("ps", "--no-trunc", "--filter", f"volume=endora-item-{task}",
                              "--format", "{{.ID}}\t{{.Labels}}")
        if code != 0:
            raise RuntimeError(err.strip() or f"docker ps exited {code}")
        return out.splitlines()

    def read(self, cid, path):
        code, out, _ = self("exec", cid, "cat", path)
        return out if code == 0 else None

    def tree(self, cid, repo):
        _, status, _ = self("exec", cid, "git", "-C", repo, "status", "--porcelain")
        _, diff, _ = self("exec", cid, "git", "-C", repo, "diff")
        return {"status": status, "diff": diff}


def watch(docker, task, rel, milestone, timeout, poll=0.25, repo="/work/repo"):
    path = f"{repo}/{rel}"
    record = {"task": task, "file": rel, "milestone": milestone, "started": time.time(), "changes": []}
    deadline = time.time() + timeout
    last = None
    failures = 0
    while time.time() < deadline:
        try:
            lines = docker.ps(task)
            failures = 0
        except (RuntimeError, subprocess.TimeoutExpired) as e:
            failures += 1
            record["docker_error"] = str(e)
            if failures >= 5:
                record["outcome"] = "docker_failed"
                return record
            time.sleep(poll)
            continue
        (turn, keeper) = pick(lines, task)
        if not turn:
            time.sleep(poll)
            continue
        cid, run = turn
        record.setdefault("turn_seen", {"at": time.time(), "container": cid, "run": run})
        text = docker.read(cid, path)
        seen = time.time()
        d = hashlib.sha256(text.encode()).hexdigest() if text is not None else None
        if d != last:
            record["changes"].append({"at": seen, "digest": d, "milestone": rig.milestone_reached(milestone, text)})
            last = d
        if rig.milestone_reached(milestone, text):
            record["milestone_seen"] = {"at": seen, "container": cid, "run": run, "text": text}
            record["tree_before"] = docker.tree(cid, repo)
            record["stop_sent"] = time.time()
            code, out, err = docker("kill", cid)
            record["stop_returned"] = time.time()
            record["stop"] = {"exit": code, "out": out.strip(), "err": err.strip()}
            _, state, _ = docker("inspect", "--format", "{{.State.Status}} {{.State.ExitCode}}", cid)
            record["stopped_state"] = state.strip()
            record["keeper"] = keeper
            record["tree_after"] = docker.tree(keeper, repo) if keeper else None
            record["file_after"] = docker.read(keeper, path) if keeper else None
            record["outcome"] = "interrupted"
            return record
        time.sleep(poll)
    record["outcome"] = "milestone_not_seen"
    return record


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--task", type=int, required=True)
    ap.add_argument("--file", required=True)
    ap.add_argument("--milestone", required=True)
    ap.add_argument("--docker", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--timeout", type=float, default=600)
    a = ap.parse_args(argv)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    record = watch(Docker(a.docker), a.task, a.file, json.loads(a.milestone), a.timeout)
    (out / "interrupt.json").write_text(json.dumps(record, indent=1))
    print(json.dumps({k: record.get(k) for k in ("outcome", "stop_sent", "stopped_state")}))
    if record["outcome"] == "docker_failed":
        print("interrupt_live: docker keeps failing:", record.get("docker_error"), file=sys.stderr)
    return 0 if record["outcome"] == "interrupted" else 3


if __name__ == "__main__":
    sys.exit(main())
