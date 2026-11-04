"""The publish service: what it records is the runner's, never the agent's to change."""

import json
import sys
import unittest
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import sidefx  # noqa: E402


def ask(url, body=None, token=None):
    req = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(),
                                 method="GET" if body is None else "POST",
                                 headers={"Content-Type": "application/json",
                                          **({"Authorization": f"Bearer {token}"} if token else {})})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.s = sidefx.Service()
        self.addCleanup(self.s.close)

    def test_the_first_publish_fails_transiently_and_the_next_succeeds(self):
        self.assertEqual(ask(self.s.url + "/publish", {"version": "1.2.0"})[0], 503)
        self.assertEqual(ask(self.s.url + "/publish", {"version": "1.2.0"})[0], 200)
        self.assertEqual(ask(self.s.url + "/status")[1], {"published": ["1.2.0"]})
        self.assertEqual([(r["seq"], r["ok"], r["status"]) for r in self.s.records()],
                         [(1, False, 503), (2, True, 200)])

    def test_a_duplicate_publish_stays_on_the_record(self):
        for _ in range(3):
            ask(self.s.url + "/publish", {"version": "1.2.0"})
        self.assertEqual(self.s.published(), ["1.2.0", "1.2.0"])

    def test_the_records_need_the_token_the_agent_never_has(self):
        self.assertEqual(ask(self.s.url + "/records")[0], 401)
        self.assertEqual(ask(self.s.url + "/records", token="guess")[0], 401)
        self.assertEqual(ask(self.s.url + "/records", token=self.s.token), (200, {"records": []}))


if __name__ == "__main__":
    unittest.main()
