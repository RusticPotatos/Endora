#!/usr/bin/env python3
"""Runs one probe through Claude Code, headless, the Claude Code arm of the harness-probes rig
(round 1 of the agentic-harness climb). Python 3 standard library only.

    python3 scripts/harness-probes/cc_local.py --family F --seed S --out DIR --model M \
        [--claude-bin claude] [--claude-home ~/.claude] [--probe-dir DIR] [--paid-ok]

A real Claude Code run spends model money, so any binary but the tests' fake
(`fixtures/fake-claude/claude`) runs only with `--paid-ok`, which the owner gives for an
approved batch.

Claude Code runs as `claude -p <words> --output-format stream-json --verbose --model M
--permission-mode bypassPermissions` in the probe's repository, with its own instructions and
tools, and nothing added. For `interrupted`, the attempt is stopped at its first change to
`interrupt_after` and resumed with `claude -p <resume_words> --resume <session>`.

Writes into DIR: `probe/`, `stream-N.jsonl` (each invocation's stream, whole), `sessions/`
(copies of the session transcripts and their subagents), `observations.json` (what the
publish service saw, from sidefx.py), `attempt.json`, `grade.json` and `run.json`.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import accounting  # noqa: E402
import rig  # noqa: E402


def session_of(stream_path):
    """The session id the stream's init event names, else its result event's."""
    found = None
    for line in Path(stream_path).read_text().splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("session_id"):
            found = event["session_id"]
            if event.get("type") == "system":
                return found
    return found


def session_files(claude_home, session):
    """The session's transcript and its subagents' folder, wherever its project folder is."""
    found = sorted(Path(claude_home).glob(f"projects/*/{session}.jsonl"))
    if not found:
        return None, None
    main = found[0]
    sub = main.parent / session / "subagents"
    return main, sub if sub.is_dir() else None


def run(args):
    fake = HERE / "fixtures" / "fake-claude"
    if fake not in Path(args.claude_bin).resolve().parents and not args.paid_ok:
        print("cc_local: the real Claude Code spends model money; run it only for a batch the "
              "owner approved, with --paid-ok", file=sys.stderr)
        return 2
    out = Path(args.out).resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    probe = out / "probe"
    task = rig.make_probe(args.family, args.seed, probe, args.probe_dir)
    repo = probe / "repo"
    env = dict(os.environ)
    publish = rig.side_effects()
    env["PROBE_PUBLISH_URL"] = publish.url
    base = [args.claude_bin, "-p", None, "--output-format", "stream-json", "--verbose",
            "--model", args.model, "--permission-mode", "bypassPermissions"]
    if args.effort:
        base += ["--effort", args.effort]
    for extra in args.cc_arg:
        base.append(extra)
    interrupt = task.get("interrupt_after")
    steps = [(task["words"], None)]
    summary = {"family": args.family, "seed": args.seed, "model": args.model,
               "effort": args.effort, "command": [c for c in base if c is not None],
               "dispatched_at": time.time(), "invocations": []}
    deadline = summary["dispatched_at"] + args.timeout
    session = None
    n = 0
    while steps:
        words, resume = steps.pop(0)
        n += 1
        command = list(base)
        command[2] = words
        if resume:
            command += ["--resume", resume]
        stream = out / f"stream-{n}.jsonl"
        with open(stream, "w") as sink:
            code, how, err = rig.run_watched(
                command, cwd=repo, env=env, timeout=max(1.0, deadline - time.time()),
                interrupt_file=(repo / interrupt) if interrupt and n == 1 else None,
                interrupt_when=task.get("interrupt_milestone"),
                stdout=sink,
            )
        session = session_of(stream) or session
        summary["invocations"].append({"n": n, "exit": code, "ended": how, "session": session,
                                       "stderr_tail": err[-1000:], "at": time.time()})
        if how == "interrupted":
            summary["invocations"][-1]["tree_at_stop"] = {
                "status": subprocess.run(["git", "status", "--porcelain"], cwd=repo, capture_output=True, text=True).stdout,
                "diff": subprocess.run(["git", "diff"], cwd=repo, capture_output=True, text=True).stdout}
        if how == "interrupted" and session:
            steps.append((task["resume_words"], session))
    summary["ended_at"] = time.time()

    sessions = out / "sessions"
    sessions.mkdir()
    main, sub = session_files(args.claude_home, session) if session else (None, None)
    session_paths = []
    if main:
        shutil.copy(main, sessions / main.name)
        session_paths.append(sessions / main.name)
        if sub:
            shutil.copytree(sub, sessions / "subagents")
    streams = sorted(out.glob("stream-*.jsonl"))
    attempt = accounting.parse_cc(
        session_paths, subagents_dir=(sessions / "subagents") if sub else None,
        stream_json=streams[-1] if streams else None, family=args.family, seed=args.seed,
    )
    attempt["dispatched_at"] = summary["dispatched_at"]
    attempt["ended_at"] = summary["ended_at"]
    attempt["meta"]["requested_model"] = args.model
    attempt_path = out / "attempt.json"
    attempt_path.write_text(json.dumps(attempt, indent=2) + "\n")
    observations = rig.keep_observations(publish, out / "observations.json")
    verdict = rig.grade(args.family, args.seed, probe, attempt_path, observations, publish.url)
    publish.close()
    (out / "grade.json").write_text(json.dumps(verdict, indent=2) + "\n")
    summary["session"] = session
    summary["session_found"] = bool(main)
    summary["pass"] = verdict.get("pass")
    (out / "run.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--family", required=True)
    p.add_argument("--seed", type=int, required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--model", default="claude-sonnet-5-5")
    p.add_argument("--claude-bin", default="claude")
    p.add_argument("--effort", help="Claude Code's effort, to match the other arm's (none: its default)")
    p.add_argument("--cc-arg", action="append", default=[],
                   help="one more argument for claude, as is (repeatable), e.g. --cc-arg=--strict-mcp-config")
    p.add_argument("--claude-home", default=str(Path.home() / ".claude"))
    p.add_argument("--probe-dir", help="a probe generated already (copied, not generated)")
    p.add_argument("--timeout", type=float, default=600)
    p.add_argument("--paid-ok", action="store_true",
                   help="the owner approved a paid batch: let the real claude run")
    return run(p.parse_args(argv))


if __name__ == "__main__":
    sys.exit(main())
