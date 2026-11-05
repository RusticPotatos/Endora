#!/usr/bin/env python3
"""Offline checks for scripts/experiment.py, against a temporary store with fictional rows and
a fake HQ in-process.

    python3 -m unittest scripts/test_experiment.py
"""

import http.server
import importlib.util
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest

HERE = Path(__file__).parent
SCRIPT = HERE / "experiment.py"
SPEC = importlib.util.spec_from_file_location("experiment", SCRIPT)
EXP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXP)

TOKEN = "test-token-not-real-41c9"
MIN = 60_000
T0 = 1_800_000_000_000

# The tables the script reads, with the columns it uses (HQ's own schema has more).
SCHEMA = """
CREATE TABLE members (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
    brain TEXT NOT NULL DEFAULT 'local', furloughed_at INTEGER);
CREATE TABLE goals (id INTEGER PRIMARY KEY, title TEXT NOT NULL, status TEXT NOT NULL);
CREATE TABLE projects (id INTEGER PRIMARY KEY, goal INTEGER NOT NULL, title TEXT NOT NULL,
    reason TEXT NOT NULL, lead INTEGER NOT NULL, status TEXT NOT NULL);
CREATE TABLE tasks (id INTEGER PRIMARY KEY, goal INTEGER NOT NULL, parent INTEGER,
    title TEXT NOT NULL, description TEXT NOT NULL, owner INTEGER, assignee INTEGER,
    status TEXT NOT NULL, project INTEGER, criteria TEXT NOT NULL DEFAULT '');
CREATE TABLE runs (id INTEGER PRIMARY KEY, member INTEGER NOT NULL, task INTEGER,
    status TEXT NOT NULL, queued_at INTEGER NOT NULL, started_at INTEGER, ended_at INTEGER,
    spent INTEGER NOT NULL DEFAULT 0, end_reason TEXT NOT NULL DEFAULT '', label TEXT,
    brain TEXT, moved_forward INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0);
CREATE TABLE wakes (id INTEGER PRIMARY KEY, member INTEGER NOT NULL, task INTEGER,
    reason TEXT NOT NULL, receipt TEXT NOT NULL DEFAULT '', refused TEXT, run INTEGER,
    at_ms INTEGER NOT NULL DEFAULT 0);
CREATE TABLE ledger (id INTEGER PRIMARY KEY, run INTEGER NOT NULL, member INTEGER NOT NULL,
    day INTEGER NOT NULL DEFAULT 0, kind TEXT NOT NULL, amount INTEGER NOT NULL,
    at_ms INTEGER NOT NULL DEFAULT 0);
CREATE TABLE transcripts (run INTEGER PRIMARY KEY, member INTEGER NOT NULL, task INTEGER,
    body TEXT NOT NULL, at_ms INTEGER NOT NULL DEFAULT 0);
CREATE TABLE events (id INTEGER PRIMARY KEY, at_ms INTEGER NOT NULL, kind TEXT NOT NULL,
    data TEXT NOT NULL);
CREATE TABLE desk (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, task INTEGER, text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting', at_ms INTEGER NOT NULL);
CREATE TABLE posts (id INTEGER PRIMARY KEY, task INTEGER NOT NULL, author TEXT NOT NULL,
    text TEXT NOT NULL, at_ms INTEGER NOT NULL, evidence TEXT NOT NULL DEFAULT '');
CREATE TABLE call_receipts (id INTEGER PRIMARY KEY, run INTEGER NOT NULL, member INTEGER NOT NULL,
    task INTEGER, tool TEXT NOT NULL, args TEXT NOT NULL, excerpt TEXT NOT NULL,
    ok INTEGER NOT NULL, act INTEGER NOT NULL, confidential INTEGER NOT NULL,
    at_ms INTEGER NOT NULL);
"""

CRITERIA = json.dumps(
    [
        {"text": "Each role has a README with every default", "live": False},
        {"text": "Nothing on the lab changed", "live": True},
    ]
)
DESCRIPTION = "Write READMEs for the  `alpha` role.\n\nKeep the  double spaces.\n"


class Store:
    """A temporary store with a fictional round 0 (tasks 1 and 2, in project 1, later moved to
    a dropped archive project 9) and a fictional earlier round (task 3 open, with an open
    subtask 4; task 5 done)."""

    def __init__(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "hq.db")
        db = self.db()
        db.executescript(SCHEMA)
        db.executemany(
            "INSERT INTO members (id, name) VALUES (?, ?)",
            [(1, "John Doe"), (2, "Jane Doe")],
        )
        db.execute("INSERT INTO members (id, name, brain) VALUES (3, 'Richard Roe', 'haiku')")
        db.execute("INSERT INTO goals VALUES (1, 'Tidy the fork', 'active')")
        db.execute("INSERT INTO projects VALUES (1, 1, 'Fork clean-up', '', 2, 'active')")
        db.execute("INSERT INTO projects VALUES (9, 1, 'Round 0 (archived)', '', 2, 'dropped')")
        for task, title, status, project, parent in [
            (1, "READMEs for alpha", "dropped", 9, None),
            (2, "Pin beta off latest", "dropped", 9, None),
            (3, "READMEs for alpha", "doing", 1, None),
            (4, "A piece of 3", "todo", 1, 3),
            (5, "Pin beta off latest", "done", 1, None),
        ]:
            db.execute(
                "INSERT INTO tasks (id, goal, parent, title, description, status, project, criteria)"
                " VALUES (?, 1, ?, ?, ?, ?, ?, ?)",
                (task, parent, title, DESCRIPTION, status, project, CRITERIA),
            )
            self.event(db, T0, "task.created", {"task": task, "project": 1, "status": "todo"})
        db.commit()
        db.close()

    def db(self):
        return sqlite3.connect(self.path)

    @staticmethod
    def event(db, at, kind, data):
        db.execute(
            "INSERT INTO events (at_ms, kind, data) VALUES (?, ?, ?)", (at, kind, json.dumps(data))
        )

    def close(self):
        self.dir.cleanup()


class FakeHq(http.server.ThreadingHTTPServer):
    """The five routes `start` uses, writing to the temporary store as HQ would (minus all the
    rest HQ does); refuses a missing or wrong token."""

    daemon_threads = True

    def __init__(self, store):
        super().__init__(("127.0.0.1", 0), FakeHandler)
        self.store = store
        self.calls = []
        self.solo = {}
        self.round = []

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_address[1]}"


class FakeHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def answer(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        if self.headers.get("Authorization") != f"Bearer {TOKEN}":
            return self.answer(401, {"error": "no"})
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.calls.append((self.path, body))
        db = self.server.store.db()
        try:
            return self.answer(200, self.route(db, self.path.split("/")[2:], body))
        finally:
            db.commit()
            db.close()

    def route(self, db, parts, body):
        if parts == ["projects"]:
            cur = db.execute(
                "INSERT INTO projects (goal, title, reason, lead, status) VALUES (?,?,?,?,'active')",
                (body["goal"], body["title"], body["reason"], body["lead"]),
            )
            if "solo" in body:
                self.server.solo[cur.lastrowid] = body["solo"]
            return {"project": cur.lastrowid}
        if parts[0] == "projects":
            assert body == {"action": "drop"}
            project = int(parts[1])
            db.execute("UPDATE projects SET status = 'dropped' WHERE id = ?", (project,))
            db.execute(
                "UPDATE tasks SET status = 'dropped' WHERE project = ? AND status NOT IN"
                " ('done', 'dropped')",
                (project,),
            )
            return {"project": project, "status": "dropped"}
        if parts == ["tasks"]:
            kept = []
            for line in body["criteria"]:
                live = line[:5].lower() == "live:"
                kept.append({"text": line[5:].strip() if live else line.strip(), "live": live})
            cur = db.execute(
                "INSERT INTO tasks (goal, title, description, status, project, criteria)"
                " VALUES (?, ?, ?, 'todo', ?, ?)",
                (
                    body["goal"],
                    body["title"].strip(),
                    body["description"],
                    body.get("project"),
                    json.dumps(kept, separators=(",", ":")),
                ),
            )
            data = {"task": cur.lastrowid, "project": body.get("project"), "status": "todo"}
            Store.event(db, T0 + 999 * MIN, "task.created", data)
            return {"task": cur.lastrowid}
        if parts == ["round"]:
            self.server.round.extend(body["items"])
            return {"items": self.server.round, "running": True}
        if parts[0] == "tasks" and parts[2] == "project":
            db.execute("UPDATE tasks SET project = ? WHERE id = ?", (body["project"], int(parts[1])))
            return {"task": int(parts[1]), "project": body["project"]}
        raise AssertionError(f"unexpected route {parts}")


def run(argv):
    out = io.StringIO()
    code = EXP.main(argv, out=out)
    return code, out.getvalue()


class Start(unittest.TestCase):
    def setUp(self):
        self.store = Store()
        self.hq = FakeHq(self.store)
        threading.Thread(target=self.hq.serve_forever, daemon=True).start()
        token = os.path.join(self.store.dir.name, "token")
        with open(token, "w", encoding="utf-8") as f:
            f.write(TOKEN + "\n")
        self.base = ["--db", self.store.path, "--url", self.hq.url, "--token-file", token]

    def tearDown(self):
        self.hq.shutdown()
        self.hq.server_close()
        self.store.close()

    def test_without_yes_nothing_is_written(self):
        code, out = run(self.base + ["start", "--round", "2", "--from-tasks", "1-2"])
        self.assertEqual(code, 0)
        shown = json.loads(out)
        self.assertTrue(shown["dry_run"])
        self.assertEqual(shown["would"]["archive"]["tasks"], [3, 4])
        self.assertEqual(self.hq.calls, [])

    def test_archives_the_open_items_and_recreates_round_zero(self):
        code, out = run(self.base + ["start", "--round", "2", "--from-tasks", "1-2", "--yes"])
        self.assertEqual(code, 0, out)
        done = json.loads(out)
        paths = [p for p, _ in self.hq.calls]
        # The archive first: made, the open task and its open piece moved in, then dropped.
        self.assertEqual(
            paths[:4],
            ["/v1/projects", "/v1/tasks/3/project", "/v1/tasks/4/project", "/v1/projects/10"],
        )
        self.assertEqual(self.hq.calls[0][1]["title"], "Round 1 (archived)")
        self.assertEqual(self.hq.calls[0][1]["lead"], 2)
        self.assertEqual(done["archived"], {"project": 10, "tasks": [3, 4]})
        # Last, HQ is told the round's tasks: no downtime while any is open.
        self.assertEqual(self.hq.calls[-1], ("/v1/round", {"items": [6, 7]}))
        self.assertTrue(done["downtime_paused"])
        db = self.store.db()
        statuses = dict(db.execute("SELECT id, status FROM tasks WHERE id IN (3,4,5)").fetchall())
        self.assertEqual(statuses, {3: "dropped", 4: "dropped", 5: "done"})
        # Then the round's tasks, in round 0's project of origin (1, not the archive 9).
        self.assertEqual(done["ids"], [6, 7])
        self.assertEqual(done["mismatches"], [])
        for new, old in [(6, 1), (7, 2)]:
            a = db.execute(
                "SELECT title, description, criteria, goal, project FROM tasks WHERE id = ?", (new,)
            ).fetchone()
            b = db.execute(
                "SELECT title, description, criteria, goal FROM tasks WHERE id = ?", (old,)
            ).fetchone()
            self.assertEqual(a[:2], b[:2])
            self.assertEqual(json.loads(a[2]), json.loads(b[2]))
            self.assertEqual((a[3], a[4]), (b[3], 1))
        body = self.hq.calls[4][1]
        self.assertEqual(body["description"], DESCRIPTION)
        self.assertEqual(
            body["criteria"],
            ["Each role has a README with every default", "live: Nothing on the lab changed"],
        )
        db.close()

    def test_nothing_open_makes_no_archive(self):
        db = self.store.db()
        db.execute("UPDATE tasks SET status = 'done' WHERE id IN (3, 4)")
        db.commit()
        db.close()
        code, out = run(self.base + ["start", "--round", "2", "--from-tasks", "1-2", "--yes"])
        self.assertEqual(code, 0, out)
        self.assertIsNone(json.loads(out)["archived"])
        self.assertEqual([p for p, _ in self.hq.calls], ["/v1/tasks", "/v1/tasks", "/v1/round"])

    def test_refuses_a_project_no_longer_active(self):
        db = self.store.db()
        db.execute("UPDATE projects SET status = 'paused' WHERE id = 1")
        db.commit()
        db.close()
        code, out = run(self.base + ["start", "--round", "2", "--from-tasks", "1-2", "--yes"])
        self.assertEqual(code, 1)
        self.assertIn("project 1 is paused", out)
        self.assertEqual(self.hq.calls, [])


class StartSolo(unittest.TestCase):
    """Round 4's solo arm (ADR 0142, amended 2026-11-01): the round's tasks in a project of
    their own, worked by one member alone, beside the team's arm."""

    setUp = Start.setUp
    tearDown = Start.tearDown

    def test_without_yes_nothing_is_written(self):
        argv = ["start", "--round", "4", "--from-tasks", "1-2", "--solo", "Richard Roe"]
        code, out = run(self.base + argv)
        self.assertEqual(code, 0, out)
        shown = json.loads(out)["would"]
        self.assertIsNone(shown["archive"])
        self.assertEqual(shown["solo"]["solo"], 3)
        self.assertEqual(self.hq.calls, [])

    def test_makes_a_project_worked_alone_and_archives_nothing(self):
        argv = ["start", "--round", "4", "--from-tasks", "1-2", "--solo", "richard roe", "--yes"]
        code, out = run(self.base + argv)
        self.assertEqual(code, 0, out)
        done = json.loads(out)
        paths = [p for p, _ in self.hq.calls]
        self.assertEqual(paths, ["/v1/projects", "/v1/tasks", "/v1/tasks", "/v1/round"])
        self.assertEqual(self.hq.round, [6, 7], "the solo arm's tasks join the round")
        made = self.hq.calls[0][1]
        self.assertEqual((made["lead"], made["solo"], made["goal"]), (3, 3, 1))
        self.assertEqual(made["title"], "Round 4 solo (richard roe)")
        self.assertEqual(done["solo"], {"project": 10, "member": 3})
        self.assertEqual(self.hq.solo, {10: 3})
        self.assertIsNone(done["archived"])
        self.assertEqual(done["mismatches"], [])
        db = self.store.db()
        projects = [r[0] for r in db.execute("SELECT project FROM tasks WHERE id IN (6, 7)")]
        self.assertEqual(projects, [10, 10])
        statuses = dict(db.execute("SELECT id, status FROM tasks WHERE id IN (3, 4)").fetchall())
        self.assertEqual(statuses, {3: "doing", 4: "todo"}, "the team's arm is left alone")
        db.close()

    def test_refuses_a_member_who_is_not_there_or_retired(self):
        argv = ["start", "--round", "4", "--from-tasks", "1-2", "--solo", "Nobody", "--yes"]
        code, out = run(self.base + argv)
        self.assertEqual(code, 1)
        self.assertIn("no member is called Nobody", out)
        db = self.store.db()
        db.execute("UPDATE members SET furloughed_at = 1 WHERE id = 3")
        db.commit()
        db.close()
        argv[-2] = "Richard Roe"
        code, out = run(self.base + argv)
        self.assertEqual(code, 1)
        self.assertIn("Richard Roe has retired", out)
        self.assertEqual(self.hq.calls, [])

    def test_a_member_off_haiku_is_noted(self):
        argv = ["start", "--round", "4", "--from-tasks", "1-2", "--solo", "John Doe"]
        code, out = run(self.base + argv)
        self.assertEqual(code, 0, out)
        self.assertIn("not haiku", json.loads(out)["would"]["notes"][0])


class Reading(unittest.TestCase):
    """Task 3 of the fictional store as a finished round task: made at T0, in review at +82
    min, sent back, in review again, done at +200 min; a subtask 4 with its own turn."""

    def setUp(self):
        self.store = Store()
        db = self.store.db()
        for at, status in [(82, "in_review"), (90, "doing"), (150, "in_review"), (200, "done")]:
            Store.event(db, T0 + at * MIN, "task.changed", {"task": 3, "status": status})
        db.execute("UPDATE tasks SET status = 'done' WHERE id = 3")
        runs = [
            (1, 3, "succeeded", 0, 1, 20_000, "local"),
            (2, 3, "failed", 30, 31, 10_000, "mac"),
            (1, 4, "succeeded", 40, 41, 5_005, "flash"),
            (2, 3, "queued", 300, None, 0, None),
        ]
        for member, task, status, queued, started, spent, brain in runs:
            db.execute(
                "INSERT INTO runs (member, task, status, queued_at, started_at, spent, brain)"
                " VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    member,
                    task,
                    status,
                    T0 + queued * MIN,
                    None if started is None else T0 + started * MIN,
                    spent,
                    brain,
                ),
            )
        # Desk: two questions answered with words, one seen without words, one dismissed, a
        # notice quietly seen, and a question on the subtask answered.
        desk = [
            ("question", 3, "seen", 10),
            ("question", 3, "seen", 20),
            ("question", 3, "seen", 60),
            ("question", 3, "dismiss", 70),
            ("notice", 3, "seen", 75),
            ("question", 4, "seen", 45),
            ("question", 3, "waiting", 160),
        ]
        for kind, task, status, at in desk:
            db.execute(
                "INSERT INTO desk (kind, task, text, status, at_ms) VALUES (?, ?, 'q', ?, ?)",
                (kind, task, status, T0 + at * MIN),
            )
        for task, author, at in [(3, "owner", 12), (3, "owner", 25), (4, "owner", 46), (3, "Jane Doe", 26)]:
            db.execute(
                "INSERT INTO posts (task, author, text, at_ms) VALUES (?, ?, 'answer', ?)",
                (task, author, T0 + at * MIN),
            )
        db.commit()
        db.close()

    def tearDown(self):
        self.store.close()

    def status(self, task):
        code, out = run(["--db", self.store.path, "status", "--tasks", str(task)])
        self.assertEqual(code, 0)
        return json.loads(out)[0]

    def test_elapsed_is_made_to_first_ready(self):
        s = self.status(3)
        self.assertEqual(s["elapsed_min"], 82)
        self.assertEqual(s["status"], "done")
        self.assertIsNotNone(s["done"])

    def test_cost_and_turns_include_subtasks_and_skip_unstarted_turns(self):
        s = self.status(3)
        self.assertAlmostEqual(s["cost_usd"], 0.035005)
        self.assertEqual(s["turns"], 3)
        self.assertEqual(s["subtasks"], [4])
        self.assertEqual(s["last_runner"]["name"], "Jane Doe")
        self.assertEqual(s["last_runner"]["run_status"], "queued")

    def test_interventions_are_answered_questions(self):
        s = self.status(3)
        # 10 and 20 on task 3 answered (posts at 12 and 25); 60 seen with no owner post after
        # it; the dismissed one, the notice and the waiting one do not count; 45 on task 4 does.
        self.assertEqual(s["interventions"], 3)
        self.assertEqual(len(s["desk_cards"]), 7)

    def test_a_task_never_ready(self):
        db = self.store.db()
        db.execute("INSERT INTO tasks (id, goal, title, description, status) VALUES (8, 1, 'x', '', 'doing')")
        Store.event(db, T0, "task.created", {"task": 8})
        db.commit()
        db.close()
        s = self.status(8)
        self.assertIsNone(s["elapsed_min"])
        self.assertEqual((s["cost_usd"], s["turns"], s["interventions"]), (0, 0, 0))

    def test_fields_table(self):
        db = self.store.db()
        db.execute("INSERT INTO tasks (id, goal, title, description, status) VALUES (8, 1, 'x', '', 'doing')")
        db.commit()
        db.close()
        code, out = run(["--db", self.store.path, "fields", "--tasks", "3,8", "--header"])
        self.assertEqual(code, 0)
        self.assertEqual(
            out.splitlines(),
            [
                "| # | HQ task | Accepted | Cost | Elapsed | Interventions | Rework |",
                "|---|---|---|---|---|---|---|",
                "| 1 | 3 |  | $0.04 | 82 min | 3 | |",
                "| 2 | 8 |  | $0.00 | not ready | 0 | |",
            ],
        )

    def test_runs_fed_on_stdin_as_in_the_container(self):
        with open(SCRIPT, "rb") as script:
            done = subprocess.run(
                [sys.executable, "-", "fields", "--tasks", "3"],
                stdin=script,
                capture_output=True,
                env={**os.environ, "HQ_DB": self.store.path},
                check=False,
            )
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.decode().strip(), "| 1 | 3 |  | $0.04 | 82 min | 3 | |")

    def test_the_store_is_opened_read_only(self):
        conn = EXP.open_store(self.store.path)
        with self.assertRaises(sqlite3.OperationalError):
            conn.execute("DELETE FROM tasks")
        conn.close()


class Check(unittest.TestCase):
    def setUp(self):
        self.store = Store()
        db = self.store.db()
        receipts = [
            (1, 3, "read_log", "12:01 Indexer TheAlpha: HTTP 429 Too Many Requests\n12:02 retry"),
            (2, 3, "read_log", "Connection refused   by host beta.example.com"),
            (3, 5, "read_log", "another task's receipt"),
        ]
        for rid, task, tool, excerpt in receipts:
            db.execute(
                "INSERT INTO call_receipts (id, run, member, task, tool, args, excerpt, ok, act,"
                " confidential, at_ms) VALUES (?, 1, 1, ?, ?, '{}', ?, 1, 0, 0, ?)",
                (rid, task, tool, excerpt, T0),
            )
        db.commit()
        db.close()

    def tearDown(self):
        self.store.close()

    def post(self, text, evidence=""):
        db = self.store.db()
        db.execute(
            "INSERT INTO posts (task, author, text, at_ms, evidence) VALUES (3, 'Jane Doe', ?, ?, ?)",
            (text, T0 + MIN, evidence),
        )
        db.execute("INSERT INTO posts (task, author, text, at_ms) VALUES (3, 'owner', 'thanks', ?)", (T0 + 2 * MIN,))
        db.commit()
        db.close()
        code, out = run(["--db", self.store.path, "check", "--task", "3"])
        return code, json.loads(out)

    def test_quotes_found_in_cited_receipts_pass(self):
        code, result = self.post(
            "Found it (r1, receipt 2):\n> 12:01 Indexer TheAlpha: HTTP 429 Too Many Requests\n"
            "```\nConnection refused by host beta.example.com\n```\n",
        )
        self.assertEqual(code, 0, result)
        self.assertEqual(result["cited"], [1, 2])
        self.assertEqual(result["author"], "Jane Doe")

    def test_invented_uncited_and_foreign_quotes_fail(self):
        code, result = self.post(
            "See r3 and r77.\n> HTTP 503 from TheAlpha at noon\n"
            "Log: `Connection refused by host beta.example.com`\n",
            evidence="r3,task 5",
        )
        self.assertEqual(code, 1)
        problems = "\n".join(result["problems"])
        self.assertIn("r3 is cited but belongs to task 5", problems)
        self.assertIn("r77 is cited but no such receipt exists", problems)
        self.assertIn("in no receipt of this task: HTTP 503", problems)
        self.assertIn("uncited receipt(s) [2] only", problems)


TRANSCRIPT = """--- system ---
SYSTEM-10B
--- user ---
BRIEF-12345
--- assistant ---
looking
--- call: my_turn ---
{}
--- call: shell ---
{"c":1}
--- result ---
MYTURN-RESULT
--- result ---
SHELL-RESULT-1
--- call: repo_checkout ---
{}
--- result: refused ---
NO
--- user ---
not the briefing
"""


class Metrics(unittest.TestCase):
    """Two fictional rounds: tasks 3 (with subtask 4) and 5 in the first, task 6 in the second.
    Task 3 is done at +200 min (first ready at +82); 5 is done at +30; 6 never ready."""

    def setUp(self):
        self.store = Store()
        db = self.store.db()
        db.execute(
            "INSERT INTO tasks (id, goal, title, description, status) VALUES (6, 1, 'x', '', 'doing')"
        )
        Store.event(db, T0, "task.created", {"task": 6})
        for at, task, status in [(82, 3, "in_review"), (200, 3, "done"), (30, 5, "done")]:
            Store.event(db, T0 + at * MIN, "task.changed", {"task": task, "status": status})
        db.execute("UPDATE tasks SET status = 'done' WHERE id = 3")
        # run id, task, started, ended (min), moved_forward, spent, tokens in/cached/out
        runs = [
            (1, 3, 0, 10, 1, 20_000, 100, 50, 10),
            (2, 3, 5, 15, 0, 10_000, 200, 0, 20),
            (3, 4, 40, 50, 0, 5_000, 0, 0, 0),
            (4, 3, None, None, 0, 0, 0, 0, 0),
            (5, 5, 0, 20, 1, 1_000, 0, 0, 0),
            (6, 6, 0, 60, 0, 0, 0, 0, 0),
        ]
        for rid, task, start, end, moved, spent, tin, cached, tout in runs:
            db.execute(
                "INSERT INTO runs (id, member, task, status, queued_at, started_at, ended_at,"
                " spent, moved_forward, input_tokens, cache_read_tokens, output_tokens)"
                " VALUES (?, 1, ?, 'succeeded', ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    rid,
                    task,
                    T0,
                    None if start is None else T0 + start * MIN,
                    None if end is None else T0 + end * MIN,
                    spent,
                    moved,
                    tin,
                    cached,
                    tout,
                ),
            )
            if spent:
                for kind, amount in [("reserve", 99_000), ("charge", spent), ("release", 99_000)]:
                    db.execute(
                        "INSERT INTO ledger (run, member, kind, amount) VALUES (?, 1, ?, ?)",
                        (rid, kind, amount),
                    )
        wakes = [
            (3, "continued", None),
            (3, "continued", None),
            (4, "recovery", "held"),
            (3, "recovery", None),
            (3, "assigned", "away"),
            (3, "review_requested", None),
            (6, "assigned", None),
        ]
        for task, reason, refused in wakes:
            db.execute(
                "INSERT INTO wakes (member, task, reason, refused) VALUES (1, ?, ?, ?)",
                (task, reason, refused),
            )
        for rid, task in [(1, 3), (2, 4), (3, 3), (4, 6)]:
            db.execute(
                "INSERT INTO call_receipts (run, member, task, tool, args, excerpt, ok, act,"
                " confidential, at_ms) VALUES (?, 1, ?, ?, '{}', '', 1, 0, 0, 0)",
                (rid, task, "repo_checkout" if rid < 4 else "shell"),
            )
        posts = [
            (3, "endora", "Handover of #3 to Jane Doe (its turn ran out of room). Go on."),
            (3, "endora", "It failed 3 times at John Doe's rank: it goes to Jane Doe, one rank up."),
            (3, "endora", "Jane Doe reviews this, one rank up."),
            (4, "endora", "Handover of #4 to John Doe (continuation 1 of 3)."),
            (3, "Jane Doe", "Handover of #3 to me, said a member"),
        ]
        for task, author, text in posts:
            db.execute(
                "INSERT INTO posts (task, author, text, at_ms) VALUES (?, ?, ?, ?)",
                (task, author, text, T0),
            )
        for kind, task, status, at in [("question", 3, "seen", 10), ("notice", 3, "seen", 11)]:
            db.execute(
                "INSERT INTO desk (kind, task, text, status, at_ms) VALUES (?, ?, 'q', ?, ?)",
                (kind, task, status, T0 + at * MIN),
            )
        db.execute(
            "INSERT INTO posts (task, author, text, at_ms) VALUES (3, 'owner', 'yes', ?)",
            (T0 + 12 * MIN,),
        )
        db.execute(
            "INSERT INTO transcripts (run, member, task, body) VALUES (1, 1, 3, ?)", (TRANSCRIPT,)
        )
        db.commit()
        db.close()

    def tearDown(self):
        self.store.close()

    def metrics(self, *argv):
        code, out = run(["--db", self.store.path, "metrics", *argv])
        self.assertEqual(code, 0, out)
        return out

    def test_a_round_on_one_measure(self):
        m = json.loads(self.metrics("--tasks", "3,5", "--label", "R1", "--json"))["R1"]
        self.assertEqual(m["with_subtasks"], [3, 4, 5])
        self.assertEqual((m["reported_done"], m["reported_done_tasks"]), (2, [3, 5]))
        self.assertIsNone(m["verified"])
        self.assertEqual(m["interventions"], 1)
        self.assertAlmostEqual(m["spend_usd"], 0.036)
        # Started turns only; changed nothing is the store's own flag.
        self.assertEqual((m["turns"], m["turns_changed_nothing"]), (4, 2))
        self.assertEqual((m["tokens_in"], m["tokens_in_cached"], m["tokens_out"]), (350, 50, 30))
        self.assertEqual(m["checkouts"], 3)
        self.assertEqual(m["wakes"]["continued"], {"all": 2, "refused": 0})
        self.assertEqual(m["wakes"]["recovery"], {"all": 2, "refused": 1})
        self.assertEqual(m["wakes_refused"], 2)
        # HQ's handovers on the task and its subtask; a member's words are not one; a review one
        # rank up is not a climb.
        self.assertEqual((m["handovers"], m["climbs"]), (2, 1))
        self.assertEqual(m["desk_questions"], 1)
        self.assertEqual(m["first_ready"], {"n": 2, "median_min": 56.0})
        self.assertEqual(m["done"], {"n": 2, "median_min": 115.0})
        # Working: 10 + 10 + 10 + 20 min; waiting: task 3's 200 min less 15 (0-15) and 10
        # (40-50) covered, task 5's 30 less 20.
        self.assertEqual(m["working_h"], round(50 / 60, 1))
        self.assertEqual(m["waiting_h"], round((175 + 10) / 60, 1))

    def test_done_with_nothing_checked_is_counted_apart(self):
        # ADR 0124, amended 2026-11-03: a solo item with no branch closes completed, not
        # verified; its done never counts as a verified outcome. A store from before the closures
        # table, or a closing met on evidence, counts none.
        m = json.loads(self.metrics("--tasks", "3,5", "--label", "R1", "--json"))["R1"]
        self.assertEqual(m["reported_done_unverified"], 0)
        db = self.store.db()
        db.execute(
            "CREATE TABLE closures (task INTEGER PRIMARY KEY, status TEXT NOT NULL,"
            " why TEXT NOT NULL, duplicate_of INTEGER, successor INTEGER, by TEXT NOT NULL,"
            " at_ms INTEGER NOT NULL)"
        )
        db.execute("INSERT INTO closures VALUES (3, 'done', 'met_criteria', NULL, NULL, 'endora', 0)")
        # Kept apart from closures, so a build from before it was named never misreads it.
        db.execute(
            "CREATE TABLE unverified_closings (task INTEGER PRIMARY KEY, by TEXT NOT NULL,"
            " at_ms INTEGER NOT NULL)"
        )
        db.execute("INSERT INTO unverified_closings VALUES (5, 'endora', 0)")
        db.commit()
        db.close()
        m = json.loads(self.metrics("--tasks", "3,5", "--label", "R1", "--json"))["R1"]
        self.assertEqual((m["reported_done"], m["reported_done_unverified"]), (2, 1))
        rows = {
            line.split(" | ")[0][2:]: line.split(" | ")[1:]
            for line in self.metrics("--tasks", "3,5", "--label", "R1").splitlines()[2:]
        }
        self.assertEqual(rows["Reported (HQ done)"], ["2 (1 not verified) |"])

    def test_transcript_bytes_by_what_they_were_spent_on(self):
        m = json.loads(self.metrics("--tasks", "3", "--json"))["Tasks 3"]
        self.assertEqual(m["transcripts_kept"], 1)
        self.assertEqual(m["transcript_calls"], {"protocol": 1, "checkout": 1, "other": 1})
        self.assertEqual(
            m["transcript_bytes"],
            {
                "briefing": len("SYSTEM-10B") + len("BRIEF-12345"),
                "protocol": len("{}") + len("MYTURN-RESULT"),
                "checkout": len("{}") + len("NO"),
                "other": len('{"c":1}') + len("SHELL-RESULT-1"),
            },
        )

    def test_groups_and_verified_and_the_table(self):
        out = self.metrics(
            "--tasks", "3,5,6", "--label", "Round 9", "--groups", "fixed=3,5,outside=6",
            "--verified", "3,6",
        )
        lines = out.splitlines()
        self.assertEqual(lines[0], "| | Round 9 | fixed | outside |")
        rows = {line.split(" | ")[0][2:]: line.split(" | ")[1:] for line in lines[2:]}
        self.assertEqual(rows["Reported (HQ done)"], ["2", "2", "0 |"])
        self.assertEqual(rows["Verified (judge)"], ["2", "1", "1 |"])
        self.assertEqual(rows["Turns"], ["5", "4", "1 |"])
        self.assertEqual(rows["Handovers / climbs one rank up"], ["2 / 1", "2 / 1", "0 / 0 |"])
        self.assertEqual(rows["First ready: tasks / median from made"][2], "0 / — |")
        self.assertIn(
            "Wakes: continued / recovery (refused, any reason) / review asked / assigned", rows
        )
        self.assertEqual(
            rows["Wakes: continued / recovery (refused, any reason) / review asked / assigned"][0],
            "2 / 2 (2) / 1 / 2",
        )

    def test_rounds_side_by_side(self):
        out = self.metrics("--tasks", "3,5", "--label", "A", "--tasks", "6", "--label", "B")
        self.assertTrue(out.startswith("| | A | B |"))

    def test_a_task_not_in_the_store_is_refused(self):
        with self.assertRaises(SystemExit):
            run(["--db", self.store.path, "metrics", "--tasks", "77"])


class Pieces(unittest.TestCase):
    def test_groups(self):
        self.assertEqual(
            EXP.groups_of("fixed=1-3,7,fresh=8,outside=9"),
            [("fixed", [1, 2, 3, 7]), ("fresh", [8]), ("outside", [9])],
        )
        with self.assertRaises(ValueError):
            EXP.groups_of("4,fixed=5")

    def test_covered_counts_overlaps_once(self):
        self.assertEqual(EXP.covered_ms([(0, 10), (5, 15), (40, 50), (90, 120)], 0, 100), 35)

    def test_ids(self):
        self.assertEqual(EXP.ids_of("882-884,903"), [882, 883, 884, 903])
        with self.assertRaises(ValueError):
            EXP.ids_of("9-3")

    def test_short_backticks_are_names_not_quotes(self):
        self.assertEqual(EXP.quoted_lines("the `gluetun` role, `a quoted log line`"), ["a quoted log line"])


if __name__ == "__main__":
    unittest.main()
