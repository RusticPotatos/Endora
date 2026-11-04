"""The census export: read-only, only the runs asked for, the store left as it was."""

import hashlib
import io
import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent

SCHEMA = """CREATE TABLE relay_requests (
    id INTEGER PRIMARY KEY, run INTEGER NOT NULL, started_ms INTEGER NOT NULL, ended_ms INTEGER,
    requested_model TEXT NOT NULL, served_model TEXT, outcome TEXT NOT NULL, status INTEGER,
    usage_complete INTEGER NOT NULL, hq_told INTEGER) STRICT;
CREATE TABLE secrets (name TEXT, value TEXT);"""


def store(path):
    con = sqlite3.connect(path)
    con.executescript(SCHEMA)
    con.executemany("INSERT INTO relay_requests VALUES (?,?,?,?,?,?,?,?,?,?)", [
        (1, 5403, 1000, 2000, "claude-haiku-5-5", "claude-haiku-5-5", "answered", 200, 1, 1),
        (2, 5403, 2100, 2300, "claude-haiku-5-5", None, "failed", 529, 0, None),
        (3, 9999, 3000, 3500, "claude-opus-5-5", "claude-opus-5-5", "answered", 200, 1, 1),
        (4, 5404, 4000, None, "claude-haiku-5-5", None, "unfinished", None, 0, None)])
    con.execute("INSERT INTO secrets VALUES ('key', 'never-exported')")
    con.commit()
    con.close()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class ExportTests(unittest.TestCase):
    def run_export(self, db, runs):
        # As it runs in the node's container: the script on stdin, its arguments after `-`.
        script = (HERE / "export_census.py").read_text()
        done = subprocess.run([sys.executable, "-", "--db", str(db), "--runs", runs],
                              input=script, capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        return json.loads(done.stdout)

    def test_only_the_runs_asked_for_and_nothing_else_of_the_store(self):
        with tempfile.TemporaryDirectory() as d:
            db = Path(d) / "endora.db"
            store(db)
            before = sha(db)
            rows = self.run_export(db, "5403,5404")
            self.assertEqual(sha(db), before, "the store is left exactly as it was")
        self.assertEqual([r["id"] for r in rows], [1, 2, 4])
        self.assertEqual({r["run"] for r in rows}, {5403, 5404})
        self.assertNotIn("never-exported", json.dumps(rows))
        self.assertEqual(rows[2]["outcome"], "unfinished")

    def test_a_store_without_the_census_says_so_rather_than_an_empty_list(self):
        with tempfile.TemporaryDirectory() as d:
            db = Path(d) / "old.db"
            sqlite3.connect(db).execute("CREATE TABLE secrets (name TEXT)").connection.close()
            script = (HERE / "export_census.py").read_text()
            done = subprocess.run([sys.executable, "-", "--db", str(db), "--runs", "1"],
                                  input=script, capture_output=True, text=True)
        self.assertEqual(done.returncode, 3)
        self.assertIn("no relay_requests", done.stderr)


if __name__ == "__main__":
    unittest.main()
