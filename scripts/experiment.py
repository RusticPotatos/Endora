#!/usr/bin/env python3
"""Runs and reads a round of the baseline experiment on HQ
(docs/proposals/baseline-experiment-2026-10-28.md, "Rounds after the baseline").

Python 3 standard library only. Meant to be fed on stdin to the Python inside the HQ
container, so nothing has to be copied in; the arguments follow the `-`, which Python passes
on as `sys.argv[1:]`:

    docker --context nas exec -i endora-hq python3 - status --tasks 913-922 < scripts/experiment.py

Subcommands (every one but `start` only reads):

    start  --round N --from-tasks 882-890,903 [--previous 913-922] [--yes]
    status --tasks 913-922
    fields --tasks 913-922 [--header]
    check  --task 919

Where things are, from the environment (each also a flag, which wins):

    HQ_DB          the store, opened read-only            (default /data/hq.db; --db)
    HQ_URL         HQ's API, for `start` only             (default http://127.0.0.1:8790; --url)
    HQ_TOKEN_FILE  the file holding HQ's bearer token     (--token-file); never printed

`start` without `--yes` only says what it would do.
"""

import argparse
import datetime
import json
import os
import re
import sqlite3
import sys
import urllib.error
import urllib.request

FINISHED = ("done", "dropped")
READY = ("in_review", "done")


# ---- reading the store ----------------------------------------------------------------------


def ids_of(text):
    """`882-890,903` -> [882, ..., 890, 903], in the order given."""
    found = []
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            low, high = (int(p) for p in part.split("-", 1))
            if high < low:
                raise ValueError(f"range runs backwards: {part}")
            found.extend(range(low, high + 1))
            continue
        found.append(int(part))
    return found


def open_store(path):
    """The store, read-only: nothing here can change it."""
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def task_row(conn, task):
    row = conn.execute("SELECT * FROM tasks WHERE id = ?", (task,)).fetchone()
    if row is None:
        raise SystemExit(f"no task {task} in the store")
    return row


def family(conn, task):
    """The task and every task under it, at any depth: work a member split off is the task's."""
    rows = conn.execute(
        """WITH RECURSIVE under(id) AS (
               SELECT ? UNION SELECT t.id FROM tasks t JOIN under u ON t.parent = u.id)
           SELECT id FROM under ORDER BY id""",
        (task,),
    ).fetchall()
    return [r["id"] for r in rows]


def task_events(conn, task):
    """The task's own `task.created` and `task.changed` events, oldest first."""
    rows = conn.execute(
        """SELECT at_ms, kind, data FROM events
           WHERE kind IN ('task.created', 'task.changed')
             AND json_extract(data, '$.task') = ? ORDER BY id""",
        (task,),
    ).fetchall()
    return [(r["at_ms"], r["kind"], json.loads(r["data"])) for r in rows]


def marks(conn, task):
    """When the task was made, first ready for review (in review, or done straight away) and
    first done, in ms; None for what has not happened. Made falls back to the first event."""
    made = ready = done = None
    for at, kind, data in task_events(conn, task):
        if kind == "task.created" and made is None:
            made = at
        status = data.get("status")
        if kind == "task.changed" and status in READY and ready is None:
            ready = at
        if kind == "task.changed" and status == "done" and done is None:
            done = at
    return made, ready, done


def minutes_between(start, end):
    if start is None or end is None:
        return None
    return round((end - start) / 60_000)


def placeholders(items):
    return ",".join("?" for _ in items)


def cost_and_turns(conn, tasks):
    """Dollars spent and turns taken on these tasks: every run's `spent` (micro-dollars), and the
    runs that started (a turn still queued has cost nothing and done nothing)."""
    row = conn.execute(
        f"""SELECT COALESCE(SUM(spent), 0) AS spent,
                   COALESCE(SUM(started_at IS NOT NULL), 0) AS turns
            FROM runs WHERE task IN ({placeholders(tasks)})""",
        tasks,
    ).fetchone()
    return row["spent"] / 1_000_000, row["turns"]


def last_runner(conn, tasks):
    row = conn.execute(
        f"""SELECT r.member, m.name, r.brain, r.status FROM runs r
            LEFT JOIN members m ON m.id = r.member
            WHERE r.task IN ({placeholders(tasks)})
            ORDER BY COALESCE(r.started_at, r.queued_at) DESC, r.id DESC LIMIT 1""",
        tasks,
    ).fetchone()
    if row is None:
        return None
    return {
        "member": row["member"],
        "name": row["name"],
        "brain": row["brain"],
        "run_status": row["status"],
    }


def desk_cards(conn, tasks):
    rows = conn.execute(
        f"""SELECT id, kind, task, status, at_ms FROM desk
            WHERE task IN ({placeholders(tasks)}) ORDER BY at_ms, id""",
        tasks,
    ).fetchall()
    return [dict(r) for r in rows]


def interventions(conn, tasks):
    """The owner's answers: question cards on these tasks the owner answered. An answer is a
    card marked seen with the owner's words, which HQ writes in the task's thread as the owner's
    post; so each seen question is paired with the first unpaired owner post on the same task at
    or after it. A question merely dismissed, or seen with no words, is not counted; nor is a
    loop's quiet notice (marked seen by HQ, not a question). Returns the answered card ids."""
    cards = [c for c in desk_cards(conn, tasks) if c["kind"] == "question" and c["status"] == "seen"]
    posts = conn.execute(
        f"""SELECT id, task, at_ms FROM posts
            WHERE author = 'owner' AND task IN ({placeholders(tasks)}) ORDER BY at_ms, id""",
        tasks,
    ).fetchall()
    used = set()
    answered = []
    for card in cards:
        for post in posts:
            if post["id"] in used or post["task"] != card["task"] or post["at_ms"] < card["at_ms"]:
                continue
            used.add(post["id"])
            answered.append(card["id"])
            break
    return answered


def when(ms):
    if ms is None:
        return None
    moment = datetime.datetime.fromtimestamp(ms / 1000, tz=datetime.timezone.utc)
    return moment.astimezone().isoformat(timespec="minutes")


def status_of(conn, task):
    row = task_row(conn, task)
    tasks = family(conn, task)
    made, ready, done = marks(conn, task)
    dollars, turns = cost_and_turns(conn, tasks)
    cards = desk_cards(conn, tasks)
    return {
        "task": task,
        "title": row["title"],
        "status": row["status"],
        "made": when(made),
        "first_ready": when(ready),
        "done": when(done),
        "elapsed_min": minutes_between(made, ready),
        "cost_usd": round(dollars, 6),
        "turns": turns,
        "subtasks": tasks[1:],
        "desk_cards": [
            {"id": c["id"], "kind": c["kind"], "task": c["task"], "status": c["status"]}
            for c in cards
        ],
        "interventions": len(interventions(conn, tasks)),
        "last_runner": last_runner(conn, tasks),
    }


def field_row(number, status):
    """One row of the round's table: # | HQ task | Accepted | Cost | Elapsed | Interventions |
    Rework. Accepted and rework are left for the judge."""
    elapsed = status["elapsed_min"]
    shown = "not ready" if elapsed is None else f"{elapsed} min"
    cost = f"${status['cost_usd']:.2f}"
    return f"| {number} | {status['task']} |  | {cost} | {shown} | {status['interventions']} | |"


FIELDS_HEADER = (
    "| # | HQ task | Accepted | Cost | Elapsed | Interventions | Rework |\n"
    "|---|---|---|---|---|---|---|"
)


# ---- checks of a final post -----------------------------------------------------------------

RECEIPT_REF = re.compile(r"\b(?:receipt\s+r?(\d+)|r(\d+))\b", re.IGNORECASE)
BACKTICKED = re.compile(r"`([^`\n]+)`")
SHORTEST_QUOTE = 12


def squeeze(text):
    return " ".join(text.split())


def quoted_lines(text):
    """What a post quotes: `> ` lines, lines inside ``` fences, and `inline` spans long enough
    to be a quotation rather than a name (at least 12 characters)."""
    found = []
    fenced = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            if stripped:
                found.append(stripped)
            continue
        if stripped.startswith(">"):
            quote = stripped.lstrip(">").strip()
            if quote:
                found.append(quote)
            continue
        found.extend(s.strip() for s in BACKTICKED.findall(line) if len(s.strip()) >= SHORTEST_QUOTE)
    return found


def cited(text, evidence):
    """The receipts a post cites, in its evidence list (`r12,task 5`) and in its words."""
    found = []
    for source in (evidence or "", text):
        for a, b in RECEIPT_REF.findall(source):
            number = int(a or b)
            if number > 0 and number not in found:
                found.append(number)
    return found


def check_task(conn, task):
    """Mechanical checks of the task's final post (the last one by a member): each receipt it
    cites exists and is this task's; each line it quotes is in a cited receipt's stored result
    (`call_receipts.excerpt`), whitespace aside. A quote found only in one of the task's other
    receipts is reported as such. Everything else is the judge's."""
    task_row(conn, task)
    tasks = family(conn, task)
    post = conn.execute(
        f"""SELECT id, task, author, text, evidence, at_ms FROM posts
            WHERE task IN ({placeholders(tasks)}) AND author NOT IN ('owner', 'endora')
            ORDER BY at_ms DESC, id DESC LIMIT 1""",
        tasks,
    ).fetchone()
    if post is None:
        return {"task": task, "ok": False, "problems": ["no post by a member on this task"]}
    receipts = {
        r["id"]: r
        for r in conn.execute(
            f"""SELECT id, task, tool, excerpt, confidential FROM call_receipts
                WHERE task IN ({placeholders(tasks)})""",
            tasks,
        ).fetchall()
    }
    problems = []
    refs = cited(post["text"], post["evidence"])
    for ref in refs:
        if ref in receipts:
            continue
        other = conn.execute("SELECT task FROM call_receipts WHERE id = ?", (ref,)).fetchone()
        if other is None:
            problems.append(f"r{ref} is cited but no such receipt exists")
            continue
        problems.append(f"r{ref} is cited but belongs to task {other['task']}")
    quotes = []
    for line in quoted_lines(post["text"]):
        wanted = squeeze(line)
        in_cited = [r for r in refs if r in receipts and wanted in squeeze(receipts[r]["excerpt"])]
        elsewhere = [
            r for r, row in receipts.items() if r not in refs and wanted in squeeze(row["excerpt"])
        ]
        quotes.append({"line": line, "in_cited": in_cited, "in_uncited": elsewhere})
        if in_cited:
            continue
        if elsewhere:
            problems.append(f"quoted line is in uncited receipt(s) {elsewhere} only: {line[:80]}")
            continue
        problems.append(f"quoted line is in no receipt of this task: {line[:80]}")
    return {
        "task": task,
        "post": post["id"],
        "author": post["author"],
        "cited": refs,
        "quotes": quotes,
        "ok": not problems,
        "problems": problems,
    }


# ---- starting a round -----------------------------------------------------------------------


class Hq:
    """HQ's API, with the owner's token."""

    def __init__(self, url, token):
        self.url = url.rstrip("/")
        self.token = token

    def post(self, path, body):
        request = urllib.request.Request(
            self.url + path,
            data=json.dumps(body).encode(),
            method="POST",
            headers={"Content-Type": "application/json", "Authorization": f"Bearer {self.token}"},
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as answer:
                return json.loads(answer.read() or b"{}")
        except urllib.error.HTTPError as failed:
            said = failed.read().decode(errors="replace")[:500]
            raise SystemExit(f"HQ refused POST {path}: {failed.code} {said}") from None


def criteria_lines(kept):
    """A task's kept criteria (a JSON list) as the lines `POST /v1/tasks` takes: `live:` before
    a live one. Also returns what cannot be carried that way (a criterion's `after`)."""
    if not kept:
        return [], []
    lines, lost = [], []
    for c in json.loads(kept):
        text = c["text"]
        lines.append(f"live: {text}" if c.get("live") else text)
        if c.get("after") is not None:
            lost.append(f"criterion {text[:60]!r} waits {c['after']} min; the API cannot set that")
    return lines, lost


def source_of(conn, task):
    """What a round-0 task is copied from: its exact words, its goal, and the project it was
    made in (from its `task.created` event, since round 0's items were later moved into an
    archive project), else the one it is in now."""
    row = task_row(conn, task)
    project = row["project"]
    for _at, kind, data in task_events(conn, task):
        if kind == "task.created":
            project = data.get("project", project)
            break
    lines, lost = criteria_lines(row["criteria"])
    return {
        "from": task,
        "goal": row["goal"],
        "project": project,
        "title": row["title"],
        "description": row["description"],
        "criteria": lines,
        "lost": lost,
    }


def open_items(conn, sources, previous):
    """What the previous round left open, with any open task under it: the tasks named, or,
    when none are, every open task bearing one of round 0's titles."""
    if previous:
        roots = previous
    else:
        titles = [s["title"] for s in sources]
        roots = [
            r["id"]
            for r in conn.execute(
                f"SELECT id FROM tasks WHERE title IN ({placeholders(titles)}) ORDER BY id", titles
            ).fetchall()
        ]
    found = []
    for root in roots:
        for task in family(conn, root):
            row = task_row(conn, task)
            if row["status"] not in FINISHED and task not in found:
                found.append(task)
    return found


def archive_lead(conn, sources):
    """A member that exists to lead the archive project: the lead of a project round 0's tasks
    were made in, else the first member."""
    projects = [s["project"] for s in sources if s["project"] is not None]
    if projects:
        row = conn.execute(
            f"""SELECT p.lead FROM projects p JOIN members m ON m.id = p.lead
                WHERE p.id IN ({placeholders(projects)}) ORDER BY p.id LIMIT 1""",
            projects,
        ).fetchone()
        if row is not None:
            return row["lead"]
    row = conn.execute("SELECT id FROM members ORDER BY id LIMIT 1").fetchone()
    if row is None:
        raise SystemExit("no member to lead the archive project")
    return row["id"]


def plan_round(conn, round_number, from_tasks, previous):
    sources = [source_of(conn, t) for t in from_tasks]
    problems = []
    for s in sources:
        if s["project"] is None:
            continue
        p = conn.execute("SELECT status FROM projects WHERE id = ?", (s["project"],)).fetchone()
        if p is None or p["status"] != "active":
            state = "missing" if p is None else p["status"]
            problems.append(f"task {s['from']}'s project {s['project']} is {state}, not active")
    leftover = open_items(conn, sources, previous)
    archive = None
    if leftover:
        archive = {
            "goal": task_row(conn, leftover[0])["goal"],
            "title": f"Round {round_number - 1} (archived)",
            "reason": f"Set aside before round {round_number} of the baseline experiment; "
            "history kept.",
            "lead": archive_lead(conn, sources),
            "tasks": leftover,
        }
    return {"round": round_number, "archive": archive, "sources": sources, "problems": problems}


def run_round(conn, hq, plan):
    """Sets aside the open items, then makes the round's tasks, and checks each against its
    source as the store now has it."""
    archived = None
    archive = plan["archive"]
    if archive:
        made = hq.post(
            "/v1/projects",
            {k: archive[k] for k in ("goal", "title", "reason", "lead")},
        )
        project = made["project"]
        for task in archive["tasks"]:
            hq.post(f"/v1/tasks/{task}/project", {"project": project})
        hq.post(f"/v1/projects/{project}", {"action": "drop"})
        archived = {"project": project, "tasks": archive["tasks"]}
    made_tasks = []
    for s in plan["sources"]:
        body = {
            "goal": s["goal"],
            "title": s["title"],
            "description": s["description"],
            "criteria": s["criteria"],
        }
        if s["project"] is not None:
            body["project"] = s["project"]
        made_tasks.append({"from": s["from"], "task": hq.post("/v1/tasks", body)["task"]})
    mismatches = []
    for s, m in zip(plan["sources"], made_tasks):
        copy = source_of(conn, m["task"])
        for key in ("title", "description", "criteria", "goal", "project"):
            if copy[key] != s[key]:
                mismatches.append(f"task {m['task']} {key} differs from task {s['from']}'s")
    return {
        "round": plan["round"],
        "archived": archived,
        "tasks": made_tasks,
        "ids": [m["task"] for m in made_tasks],
        "warnings": [w for s in plan["sources"] for w in s["lost"]],
        "mismatches": mismatches,
    }


def token_from(path):
    if not path:
        raise SystemExit("start needs HQ's token: set HQ_TOKEN_FILE or pass --token-file")
    with open(path, encoding="utf-8") as f:
        token = f.read().strip()
    if not token:
        raise SystemExit("the token file is empty")
    return token


# ---- the command line -----------------------------------------------------------------------


def parser():
    top = argparse.ArgumentParser(prog="experiment.py", description=__doc__.split("\n\n")[0])
    top.add_argument("--db", default=os.environ.get("HQ_DB", "/data/hq.db"))
    top.add_argument("--url", default=os.environ.get("HQ_URL", "http://127.0.0.1:8790"))
    top.add_argument("--token-file", default=os.environ.get("HQ_TOKEN_FILE"))
    sub = top.add_subparsers(dest="command", required=True)
    start = sub.add_parser("start", help="set aside the previous round and make this one's tasks")
    start.add_argument("--round", type=int, required=True)
    start.add_argument("--from-tasks", required=True, help="round 0's tasks, e.g. 882-890,903")
    start.add_argument("--previous", help="the previous round's tasks (default: open by title)")
    start.add_argument("--yes", action="store_true", help="really write; without it, a dry run")
    for name in ("status", "fields"):
        one = sub.add_parser(name)
        one.add_argument("--tasks", required=True)
        if name == "fields":
            one.add_argument("--header", action="store_true")
    check = sub.add_parser("check")
    check.add_argument("--task", type=int, required=True)
    return top


def main(argv=None, out=sys.stdout):
    args = parser().parse_args(argv)
    conn = open_store(args.db)
    try:
        return command(args, conn, out)
    finally:
        conn.close()


def command(args, conn, out):
    if args.command == "status":
        statuses = [status_of(conn, t) for t in ids_of(args.tasks)]
        print(json.dumps(statuses, indent=2), file=out)
        return 0
    if args.command == "fields":
        if args.header:
            print(FIELDS_HEADER, file=out)
        for number, task in enumerate(ids_of(args.tasks), start=1):
            print(field_row(number, status_of(conn, task)), file=out)
        return 0
    if args.command == "check":
        result = check_task(conn, args.task)
        print(json.dumps(result, indent=2), file=out)
        return 0 if result["ok"] else 1
    if args.round < 1:
        raise SystemExit("--round is 1 or more: round 0 is the baseline itself")
    previous = ids_of(args.previous) if args.previous else None
    plan = plan_round(conn, args.round, ids_of(args.from_tasks), previous)
    if plan["problems"]:
        print(json.dumps({"refused": plan["problems"]}, indent=2), file=out)
        return 1
    if not args.yes:
        shown = {"dry_run": True, "would": plan, "to_do_it": "add --yes"}
        print(json.dumps(shown, indent=2), file=out)
        return 0
    done = run_round(conn, Hq(args.url, token_from(args.token_file)), plan)
    print(json.dumps(done, indent=2), file=out)
    return 1 if done["mismatches"] else 0


if __name__ == "__main__":
    sys.exit(main())
