#!/usr/bin/env python3
"""Runs and reads a round of the baseline experiment on HQ
(docs/proposals/baseline-experiment-2026-10-28.md, "Rounds after the baseline").

Python 3 standard library only. Meant to be fed on stdin to the Python inside the HQ
container, so nothing has to be copied in; the arguments follow the `-`, which Python passes
on as `sys.argv[1:]`:

    docker --context nas exec -i endora-hq python3 - status --tasks 913-922 < scripts/experiment.py

Subcommands (every one but `start` only reads):

    start  --round N --from-tasks 882-890,903 [--previous 913-922] [--yes]
    start  --round N --from-tasks 1001-1003 --solo NAME [--yes]
    status --tasks 913-922
    fields --tasks 913-922 [--header]
    check  --task 919
    metrics --tasks 947-956 [--label "Round 3"] [--tasks ... --label ...]
            [--groups fixed=947-956,fresh=957-959,outside=960] [--verified 948,952] [--json]

`metrics` scores rounds on one measure, a column each (and one each group, so a round is read
three ways: the fixed ten, the homelab's fresh tasks, the task outside the homelab): reported
done (verified is the judge's, given with --verified), interventions, spend (the ledger's
charges), turns and turns that changed nothing (`runs.moved_forward`, HQ's own flag), checkouts,
wakes by reason and refused, handovers and climbs (HQ's posts), desk questions, time to first
ready and to done, hours working against waiting, tokens, and the bytes of the kept transcripts
spent on the briefing, the protocol's calls (`my_turn`, `claim`, `find_tools`), checkouts and the
rest. A task's numbers include the tasks under it.

Where things are, from the environment (each also a flag, which wins):

    HQ_DB          the store, opened read-only            (default /data/hq.db; --db)
    HQ_URL         HQ's API, for `start` only             (default http://127.0.0.1:8790; --url)
    HQ_TOKEN_FILE  the file holding HQ's bearer token     (--token-file); never printed

`start` without `--yes` only says what it would do. With `--yes` it also tells HQ the round's
tasks (`POST /v1/round`, ADR 0142, amended 2026-11-01): no downtime or chess turn starts while
any of them is open, so the round has the runner and the models to itself. Both arms' starts
add up; a round whose items are all done or dropped is over on its own, and
`POST /v1/round {"close": true}` ends one sooner. With `--solo NAME` it starts round 4's
solo arm ("one member, one long turn, no ladder", ADR 0142, amended 2026-11-01): the tasks are
made in a project of their own, `Round N solo (NAME)`, worked by that member alone, so the
team's arm and the solo arm share no loop; nothing is archived, since the team's arm of the
same round is running beside it.
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


# ---- a round's numbers, on one measure ------------------------------------------------------

# The team protocol's own calls, set against the work's: what a turn spends on being a turn.
PROTOCOL_TOOLS = ("my_turn", "claim", "find_tools")
CHECKOUT_TOOL = "repo_checkout"
# HQ's own posts that mark a hand-on of an item: a handover (domain::handover::START), and a
# climb after three failures at a rank (engine::loops::climb). A review "one rank up" (rounds
# 0-2's wording) is a review, not a climb: it is counted with the wakes, as `review_requested`.
HANDOVER_POST = "Handover of #%"
CLIMB_POST = "It failed % times at %'s rank: it goes to %, one rank up."
# A transcript's section heads (ADR 0129): `--- call: NAME ---`, `--- result ---`, and others.
SECTION_HEAD = re.compile(r"^--- ([^\n]*?) ---$", re.M)


def families(conn, roots):
    """Every task under the roots, at any depth, the roots included, once each."""
    found = []
    for root in roots:
        task_row(conn, root)
        found.extend(t for t in family(conn, root) if t not in found)
    return sorted(found)


def median(values):
    values = sorted(v for v in values if v is not None)
    if not values:
        return None
    middle = len(values) // 2
    if len(values) % 2:
        return values[middle]
    return (values[middle - 1] + values[middle]) / 2


def covered_ms(spans, start, end):
    """How much of [start, end] the spans cover, overlaps counted once."""
    total, reached = 0, start
    for low, high in sorted(spans):
        low, high = max(low, reached), min(high, end)
        if high > low:
            total += high - low
            reached = high
    return total


def transcript_bytes(body):
    """A kept transcript's bytes by what they were spent on: the briefing (the system prompt and
    the first user message), the protocol's calls, checkouts, and every other call; a call's
    bytes are its arguments and its result. Results answer the calls before them in order (a
    turn may make several calls before their results come back)."""
    spent = {"briefing": 0, "protocol": 0, "checkout": 0, "other": 0}
    calls = {"protocol": 0, "checkout": 0, "other": 0}
    heads = list(SECTION_HEAD.finditer(body))
    waiting = []
    users = 0
    for i, head in enumerate(heads):
        end = heads[i + 1].start() if i + 1 < len(heads) else len(body)
        size = len(body[head.end() : end].strip("\n").encode())
        name = head.group(1)
        if name == "system" or (name == "user" and users == 0):
            users += name == "user"
            spent["briefing"] += size
            continue
        if name.startswith("call: "):
            tool = name[len("call: ") :]
            kind = "protocol" if tool in PROTOCOL_TOOLS else "other"
            kind = "checkout" if tool == CHECKOUT_TOOL else kind
            calls[kind] += 1
            spent[kind] += size
            waiting.append(kind)
            continue
        if name.startswith("result") and waiting:
            spent[waiting.pop(0)] += size
    return spent, calls


def round_metrics(conn, roots, verified=None):
    """One column of the rounds' table: the roots (a round's tasks, or a group of them) and the
    work under them, read from the store. `verified` is the judge's list, which the store does
    not hold; None when not given."""
    tasks = families(conn, roots)
    q = placeholders(tasks)
    done = [t for t in roots if task_row(conn, t)["status"] == "done"]
    turns = conn.execute(
        f"""SELECT COALESCE(SUM(started_at IS NOT NULL), 0) AS turns,
                   COALESCE(SUM(started_at IS NOT NULL AND moved_forward = 0), 0) AS idle,
                   COALESCE(SUM(input_tokens + cache_read_tokens + cache_write_tokens), 0) AS tin,
                   COALESCE(SUM(cache_read_tokens), 0) AS cached,
                   COALESCE(SUM(output_tokens), 0) AS tout
            FROM runs WHERE task IN ({q})""",
        tasks,
    ).fetchone()
    charged = conn.execute(
        f"""SELECT COALESCE(SUM(amount), 0) FROM ledger
            WHERE kind = 'charge' AND run IN (SELECT id FROM runs WHERE task IN ({q}))""",
        tasks,
    ).fetchone()[0]
    wakes = {
        r["reason"]: {"all": r["n"], "refused": r["refused"]}
        for r in conn.execute(
            f"""SELECT reason, COUNT(*) AS n, SUM(refused IS NOT NULL) AS refused FROM wakes
                WHERE task IN ({q}) GROUP BY reason ORDER BY reason""",
            tasks,
        )
    }

    def count(sql, *extra):
        return conn.execute(sql.format(q=q), (*extra, *tasks)).fetchone()[0]

    checkouts = count(
        "SELECT COUNT(*) FROM call_receipts WHERE tool = ? AND task IN ({q})", CHECKOUT_TOOL
    )
    handovers = count(
        "SELECT COUNT(*) FROM posts WHERE author = 'endora' AND text LIKE ? AND task IN ({q})",
        HANDOVER_POST,
    )
    climbs = count(
        "SELECT COUNT(*) FROM posts WHERE author = 'endora' AND text LIKE ? AND task IN ({q})",
        CLIMB_POST,
    )
    questions = count("SELECT COUNT(*) FROM desk WHERE kind = 'question' AND task IN ({q})")
    per_task = []
    working_ms = waiting_ms = 0
    for root in roots:
        made, ready, finished = marks(conn, root)
        under = family(conn, root)
        spans = [
            (r["started_at"], r["ended_at"])
            for r in conn.execute(
                f"""SELECT started_at, ended_at FROM runs WHERE task IN ({placeholders(under)})
                    AND started_at IS NOT NULL AND ended_at IS NOT NULL""",
                under,
            )
        ]
        working_ms += sum(high - low for low, high in spans)
        start = made if made is not None else min((s[0] for s in spans), default=None)
        end = finished if finished is not None else max((s[1] for s in spans), default=start)
        if start is not None and end is not None:
            waiting_ms += max(0, (end - start) - covered_ms(spans, start, end))
        per_task.append(
            {
                "task": root,
                "status": task_row(conn, root)["status"],
                "to_first_ready_min": minutes_between(made, ready),
                "to_done_min": minutes_between(made, finished),
            }
        )
    kept = []
    if has_table(conn, "transcripts"):
        kept = conn.execute(f"SELECT body FROM transcripts WHERE task IN ({q})", tasks).fetchall()
    spent = {"briefing": 0, "protocol": 0, "checkout": 0, "other": 0}
    calls = {"protocol": 0, "checkout": 0, "other": 0}
    for row in kept:
        b, c = transcript_bytes(row["body"])
        for k in spent:
            spent[k] += b[k]
        for k in calls:
            calls[k] += c[k]
    ready = [t["to_first_ready_min"] for t in per_task]
    finished = [t["to_done_min"] for t in per_task]
    return {
        "tasks": list(roots),
        "with_subtasks": tasks,
        "reported_done": len(done),
        "reported_done_tasks": done,
        # Of those, the ones HQ closed with nothing checked (ADR 0124, amended 2026-11-03).
        "reported_done_unverified": unverified(conn, done),
        "verified": None if verified is None else len([t for t in roots if t in verified]),
        "interventions": len(interventions(conn, tasks)),
        "spend_usd": round(charged / 1_000_000, 6),
        "turns": turns["turns"],
        "turns_changed_nothing": turns["idle"],
        "tokens_in": turns["tin"],
        "tokens_in_cached": turns["cached"],
        "tokens_out": turns["tout"],
        "checkouts": checkouts,
        "wakes": wakes,
        "wakes_refused": sum(w["refused"] for w in wakes.values()),
        "handovers": handovers,
        "climbs": climbs,
        "desk_questions": questions,
        "first_ready": {"n": sum(m is not None for m in ready), "median_min": median(ready)},
        "done": {"n": sum(m is not None for m in finished), "median_min": median(finished)},
        "working_h": round(working_ms / 3_600_000, 1),
        "waiting_h": round(waiting_ms / 3_600_000, 1),
        "transcripts_kept": len(kept),
        "transcript_bytes": spent,
        "transcript_calls": calls,
        "per_task": per_task,
    }


def unverified(conn, done):
    """How many of the `done` tasks HQ closed as completed, not verified: a solo item with no
    branch, whose cited evidence nothing checked. None in a store from before they were kept."""
    if not done or not has_table(conn, "unverified_closings"):
        return 0
    return conn.execute(
        f"SELECT COUNT(*) FROM unverified_closings WHERE task IN ({placeholders(done)})",
        done,
    ).fetchone()[0]


def has_table(conn, name):
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)
    ).fetchone()
    return row is not None


def groups_of(text):
    """`fixed=947-955,fresh=956-957,outside=958` -> [("fixed", [947..955]), ...]: a part with
    `=` starts a group, and a part without one goes on the group before it."""
    found = []
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        if "=" in part:
            name, ids = part.split("=", 1)
            found.append((name.strip(), ids))
            continue
        if not found:
            raise ValueError(f"{part} is in no group: name it first, e.g. fixed={part}")
        found[-1] = (found[-1][0], f"{found[-1][1]},{part}")
    return [(name, ids_of(ids)) for name, ids in found]


def kb(n):
    return f"{n / 1000:,.0f} KB" if n < 10_000_000 else f"{n / 1_000_000:,.1f} MB"


def minutes(value):
    return "—" if value is None else f"{value:.0f} min"


METRIC_ROWS = [
    ("Tasks", lambda m: str(len(m["tasks"]))),
    (
        "Reported (HQ done)",
        lambda m: str(m["reported_done"])
        + (f" ({m['reported_done_unverified']} not verified)" if m["reported_done_unverified"] else ""),
    ),
    ("Verified (judge)", lambda m: "—" if m["verified"] is None else str(m["verified"])),
    ("Interventions", lambda m: str(m["interventions"])),
    ("Spend (HQ's runs)", lambda m: f"${m['spend_usd']:.2f}"),
    ("Turns", lambda m: str(m["turns"])),
    ("Turns that changed nothing (HQ's own flag)", lambda m: str(m["turns_changed_nothing"])),
    ("Fork checkouts", lambda m: str(m["checkouts"])),
    (
        "Wakes: continued / recovery (refused, any reason) / review asked / assigned",
        lambda m: "{} / {} ({}) / {} / {}".format(
            m["wakes"].get("continued", {}).get("all", 0),
            m["wakes"].get("recovery", {}).get("all", 0),
            m["wakes_refused"],
            m["wakes"].get("review_requested", {}).get("all", 0),
            m["wakes"].get("assigned", {}).get("all", 0),
        ),
    ),
    ("Handovers / climbs one rank up", lambda m: f"{m['handovers']} / {m['climbs']}"),
    ("Questions on the desk", lambda m: str(m["desk_questions"])),
    (
        "First ready: tasks / median from made",
        lambda m: f"{m['first_ready']['n']} / {minutes(m['first_ready']['median_min'])}",
    ),
    (
        "Done: tasks / median from made",
        lambda m: f"{m['done']['n']} / {minutes(m['done']['median_min'])}",
    ),
    ("Hours working (sum of turns) / waiting (no turn)", lambda m: f"{m['working_h']} / {m['waiting_h']}"),
    (
        "Tokens in (cached) / out",
        lambda m: f"{m['tokens_in']:,} ({m['tokens_in_cached']:,}) / {m['tokens_out']:,}",
    ),
    (
        "Kept transcripts: briefing / protocol / checkout / other calls",
        lambda m: "{} / {} ({} calls) / {} ({}) / {} ({}); {} of {} turns kept".format(
            kb(m["transcript_bytes"]["briefing"]),
            kb(m["transcript_bytes"]["protocol"]),
            m["transcript_calls"]["protocol"],
            kb(m["transcript_bytes"]["checkout"]),
            m["transcript_calls"]["checkout"],
            kb(m["transcript_bytes"]["other"]),
            m["transcript_calls"]["other"],
            m["transcripts_kept"],
            m["turns"],
        ),
    ),
]


def metrics_table(columns):
    """The rounds' table: a row a measure, a column a round or group."""
    lines = [
        "| | " + " | ".join(name for name, _ in columns) + " |",
        "|---|" + "---|" * len(columns),
    ]
    for label, cell in METRIC_ROWS:
        lines.append(f"| {label} | " + " | ".join(cell(m) for _, m in columns) + " |")
    return "\n".join(lines)


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


def solo_member(conn, name):
    """The member named to work the solo arm: (id, problems, notes)."""
    row = conn.execute(
        "SELECT id, brain, furloughed_at FROM members WHERE name = ? COLLATE NOCASE", (name,)
    ).fetchone()
    if row is None:
        return None, [f"no member is called {name}"], []
    if row["furloughed_at"] is not None:
        return None, [f"{name} has retired"], []
    notes = []
    if row["brain"] != "haiku":
        notes.append(f"{name} thinks with {row['brain']}, not haiku: the arm as planned is Haiku's")
    return row["id"], [], notes


def plan_solo(conn, round_number, from_tasks, name):
    """Round `round_number`'s solo arm: its tasks, copied from `from_tasks`, in a project of its
    own worked by `name` alone. One goal, since a project serves one."""
    sources = [source_of(conn, t) for t in from_tasks]
    member, problems, notes = solo_member(conn, name)
    goals = sorted({s["goal"] for s in sources})
    if len(goals) != 1:
        problems.append(f"the tasks serve goals {goals}: a solo arm's project serves one")
    project = None
    if member is not None and len(goals) == 1:
        project = {
            "goal": goals[0],
            "title": f"Round {round_number} solo ({name})",
            "reason": f"Round {round_number}'s solo arm of the baseline experiment: one member, "
            "one long turn, no ladder (ADR 0142, amended 2026-11-01).",
            "lead": member,
            "solo": member,
        }
    return {
        "round": round_number,
        "archive": None,
        "solo": project,
        "notes": notes,
        "sources": sources,
        "problems": problems,
    }


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
    solo = plan.get("solo")
    if solo:
        made = hq.post("/v1/projects", solo)["project"]
        solo = {"project": made, "member": solo["solo"]}
        for s in plan["sources"]:
            s["project"] = made
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
    # No downtime while the round runs: HQ holds the round's tasks, the other arm's beside them.
    ids = [m["task"] for m in made_tasks]
    paused = hq.post("/v1/round", {"items": ids}).get("running", False)
    mismatches = []
    for s, m in zip(plan["sources"], made_tasks):
        copy = source_of(conn, m["task"])
        for key in ("title", "description", "criteria", "goal", "project"):
            if copy[key] != s[key]:
                mismatches.append(f"task {m['task']} {key} differs from task {s['from']}'s")
    return {
        "round": plan["round"],
        "archived": archived,
        "solo": solo,
        "tasks": made_tasks,
        "ids": ids,
        "downtime_paused": paused,
        "warnings": [w for s in plan["sources"] for w in s["lost"]] + plan.get("notes", []),
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
    start.add_argument(
        "--solo", metavar="NAME", help="the solo arm: the tasks in a project NAME works alone"
    )
    for name in ("status", "fields"):
        one = sub.add_parser(name)
        one.add_argument("--tasks", required=True)
        if name == "fields":
            one.add_argument("--header", action="store_true")
    check = sub.add_parser("check")
    check.add_argument("--task", type=int, required=True)
    metrics = sub.add_parser("metrics", help="a round's numbers on one measure, as the record's")
    metrics.add_argument(
        "--tasks", action="append", required=True, help="a round's tasks; repeat for each round"
    )
    metrics.add_argument(
        "--label", action="append", default=[], help="a column's name, one per --tasks, in order"
    )
    metrics.add_argument(
        "--groups", help="columns for parts of the round, e.g. fixed=947-955,fresh=956,outside=957"
    )
    metrics.add_argument("--verified", help="the tasks the judge verified, e.g. 948,952")
    metrics.add_argument("--json", action="store_true", help="JSON instead of a Markdown table")
    return top


def print_metrics(args, conn, out):
    if len(args.label) > len(args.tasks):
        raise SystemExit("more --label than --tasks: one label a round, in order")
    verified = ids_of(args.verified) if args.verified else None
    columns = []
    for i, text in enumerate(args.tasks):
        name = args.label[i] if i < len(args.label) else f"Tasks {text}"
        columns.append((name, ids_of(text)))
    columns += groups_of(args.groups) if args.groups else []
    measured = [(name, round_metrics(conn, ids, verified)) for name, ids in columns]
    if args.json:
        print(json.dumps({name: m for name, m in measured}, indent=2), file=out)
        return 0
    print(metrics_table(measured), file=out)
    return 0


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
    if args.command == "metrics":
        return print_metrics(args, conn, out)
    if args.round < 1:
        raise SystemExit("--round is 1 or more: round 0 is the baseline itself")
    previous = ids_of(args.previous) if args.previous else None
    if args.solo:
        plan = plan_solo(conn, args.round, ids_of(args.from_tasks), args.solo)
    else:
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
