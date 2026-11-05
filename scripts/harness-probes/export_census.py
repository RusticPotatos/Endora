#!/usr/bin/env python3
"""Exports the node's own count of model requests (`relay_requests`, ADR 0102 amended for round
9 of the agentic-harness climb) for the runs named, read-only, as a JSON list on stdout. Meant
to be fed on stdin to the Python in the node's container, so nothing is copied in and no
endpoint is added; its arguments follow the `-`:

    docker --context nas exec -i endora-node python3 - --runs 5403,5404 \
        < scripts/harness-probes/export_census.py > census.json

The store is opened read-only (SQLite `mode=ro`), so it is left exactly as it was, and only the
census table is read: nothing else of the store (settings, keys) is touched. A store without the
table exits 3 with why: no census is not an empty one.
"""

import argparse
import json
import sqlite3
import sys

COLUMNS = ("id", "run", "started_ms", "ended_ms", "requested_model", "served_model", "outcome",
           "status", "usage_complete", "hq_told")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--db", default="/data/endora.db")
    p.add_argument("--runs", required=True, help="run ids, comma-separated")
    args = p.parse_args(argv)
    runs = [int(r) for r in args.runs.split(",") if r.strip()]
    con = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    try:
        if not con.execute("SELECT 1 FROM sqlite_master WHERE name='relay_requests'").fetchone():
            print("this store has no relay_requests: the node predates its census", file=sys.stderr)
            return 3
        marks = ",".join("?" * len(runs)) or "NULL"
        rows = con.execute(f"SELECT {', '.join(COLUMNS)} FROM relay_requests "
                           f"WHERE run IN ({marks}) ORDER BY id", runs).fetchall()
    finally:
        con.close()
    json.dump([dict(zip(COLUMNS, r)) for r in rows], sys.stdout, indent=1)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
