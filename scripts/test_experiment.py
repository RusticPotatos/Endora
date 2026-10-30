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
CREATE TABLE members (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE goals (id INTEGER PRIMARY KEY, title TEXT NOT NULL, status TEXT NOT NULL);
CREATE TABLE projects (id INTEGER PRIMARY KEY, goal INTEGER NOT NULL, title TEXT NOT NULL,
    reason TEXT NOT NULL, lead INTEGER NOT NULL, status TEXT NOT NULL);
CREATE TABLE tasks (id INTEGER PRIMARY KEY, goal INTEGER NOT NULL, parent INTEGER,
    title TEXT NOT NULL, description TEXT NOT NULL, owner INTEGER, assignee INTEGER,
    status TEXT NOT NULL, project INTEGER, criteria TEXT NOT NULL DEFAULT '');
CREATE TABLE runs (id INTEGER PRIMARY KEY, member INTEGER NOT NULL, task INTEGER,
    status TEXT NOT NULL, queued_at INTEGER NOT NULL, started_at INTEGER, ended_at INTEGER,
    spent INTEGER NOT NULL DEFAULT 0, end_reason TEXT NOT NULL DEFAULT '', label TEXT,
    brain TEXT);
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
    """The four routes `start` uses, writing to the temporary store as HQ would (minus all the
    rest HQ does); refuses a missing or wrong token."""

    daemon_threads = True

    def __init__(self, store):
        super().__init__(("127.0.0.1", 0), FakeHandler)
        self.store = store
        self.calls = []

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
        self.assertEqual([p for p, _ in self.hq.calls], ["/v1/tasks", "/v1/tasks"])

    def test_refuses_a_project_no_longer_active(self):
        db = self.store.db()
        db.execute("UPDATE projects SET status = 'paused' WHERE id = 1")
        db.commit()
        db.close()
        code, out = run(self.base + ["start", "--round", "2", "--from-tasks", "1-2", "--yes"])
        self.assertEqual(code, 1)
        self.assertIn("project 1 is paused", out)
        self.assertEqual(self.hq.calls, [])


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


class Pieces(unittest.TestCase):
    def test_ids(self):
        self.assertEqual(EXP.ids_of("882-884,903"), [882, 883, 884, 903])
        with self.assertRaises(ValueError):
            EXP.ids_of("9-3")

    def test_short_backticks_are_names_not_quotes(self):
        self.assertEqual(EXP.quoted_lines("the `gluetun` role, `a quoted log line`"), ["a quoted log line"])


if __name__ == "__main__":
    unittest.main()
