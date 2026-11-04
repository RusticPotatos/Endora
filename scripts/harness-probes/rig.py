"""What both arms' runners share: making a probe, the one intervention a family allows (stopping
an `interrupted` attempt at its first change to `interrupt_after`), and grading. Python 3
standard library only."""

import hashlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))


def make_probe(family, seed, probe, copy_from=None):
    """The probe's folder (`repo/`, `task.json`): generated, or copied from one made already."""
    if copy_from:
        shutil.copytree(copy_from, probe, symlinks=True)
    else:
        subprocess.run(
            [sys.executable, str(HERE / "probes.py"), "generate", "--family", family,
             "--seed", str(seed), "--out", str(probe)],
            check=True,
        )
    return json.loads((Path(probe) / "task.json").read_text())


def digest(path):
    p = Path(path)
    return hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else None


def run_watched(command, *, cwd, env, timeout, interrupt_file=None, stdout=None, poll=0.1):
    """Runs `command` to its end, or until `timeout`, or until `interrupt_file` first changes
    (then it is killed: the family's interruption). Returns (exit code or None, how it ended:
    "exited" | "interrupted" | "timeout", stderr text)."""
    before = digest(interrupt_file) if interrupt_file else None
    proc = subprocess.Popen(command, cwd=cwd, env=env, stdout=stdout or subprocess.DEVNULL,
                            stderr=subprocess.PIPE, text=True)
    deadline = time.time() + timeout
    while proc.poll() is None:
        if interrupt_file and digest(interrupt_file) != before:
            proc.kill()
            _, err = proc.communicate()
            return proc.returncode, "interrupted", err
        if time.time() > deadline:
            proc.kill()
            _, err = proc.communicate()
            return proc.returncode, "timeout", err
        time.sleep(poll)
    _, err = proc.communicate()
    return proc.returncode, "exited", err


def side_effects():
    """The publish service for the attempt (sidefx.Service), started by the runner: its
    address goes to the agent in PROBE_PUBLISH_URL, its records stay with the runner."""
    import sidefx
    return sidefx.Service()


def keep_observations(service, path):
    """What the service saw, written where the grader reads it, after the attempt."""
    Path(path).write_text(json.dumps(service.records(), indent=2) + "\n")
    return path


def grade(family, seed, probe, attempt_path, observations=None, publish_url=None):
    """The grader's verdict on an attempt, as JSON."""
    command = [sys.executable, str(HERE / "probes.py"), "grade", "--family", family,
               "--seed", str(seed), "--repo", str(Path(probe) / "repo"),
               "--attempt", str(attempt_path)]
    if observations:
        command += ["--observations", str(observations)]
    if publish_url:
        command += ["--publish-url", publish_url]
    done = subprocess.run(command, capture_output=True, text=True)
    try:
        return json.loads(done.stdout)
    except ValueError:
        return {"pass": False, "reasons": ["the grader said nothing readable"],
                "stdout": done.stdout[-2000:], "stderr": done.stderr[-2000:]}
