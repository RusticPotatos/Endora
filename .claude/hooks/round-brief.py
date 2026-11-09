#!/usr/bin/env python3
"""SessionStart: brief a fresh session from the record, not from a pasted prompt.

Prints, as context for the session: the checkout and its branch, the round branches being
built, the latest round's table, and the defects still open, all read from
`docs/proposals/baseline-experiment-2026-10-28.md`. Read-only: it runs nothing on the lab.
"""
import json
import os
import re
import subprocess

ROOT = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
RECORD = os.path.join(ROOT, "docs/proposals/baseline-experiment-2026-10-28.md")


def git(*args):
    try:
        return subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True,
                              timeout=10).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def brief():
    lines = [f"Checkout: {ROOT}, branch {git('rev-parse', '--abbrev-ref', 'HEAD')} at "
             f"{git('log', '-1', '--format=%h %s')}"]
    building = [b for b in git("branch", "--no-merged", "HEAD", "--list", "round*", "--format=%(refname:short)").split()
                if b]
    if building:
        lines.append("Round branches being built (leave them to their session): "
                     + ", ".join(building))
    try:
        text = open(RECORD, encoding="utf-8").read()
    except OSError:
        return "\n".join(lines)
    rounds = list(re.finditer(r"^### Round (\d+) \((?!proposed)[^\n]*$", text, re.M))
    if rounds:
        last = rounds[-1]
        end = text.find("\n### ", last.end())
        section = text[last.start(): end if end > 0 else len(text)]
        table = [l for l in section.splitlines() if l.startswith("|")]
        lines.append(f"Latest round: {last.group(0)[4:]}")
        lines.extend(table[:14])
    open_defects = re.findall(r"^- \[ \] \*\*(D\d+\.[^*]*)\*\*", text, re.M)
    if open_defects:
        lines.append("Open defects: " + "; ".join(re.sub(r"\s+", " ", d).strip() for d in open_defects))
    lines.append("The baseline-experiment rounds are archived; the record above is their history.")
    return "\n".join(lines)


print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart",
                                         "additionalContext": brief()[:9000]}}))
