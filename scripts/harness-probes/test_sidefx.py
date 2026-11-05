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


class ObserverModeTests(unittest.TestCase):
    """Round 15: the service as its own process (an observer container beside the workbench),
    on a port and with a token the runner chose, so the runner can read the records later."""

    def test_a_chosen_port_and_token(self):
        import socket
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        s = sidefx.Service(port=port, token="t" * 48)
        self.addCleanup(s.close)
        self.assertEqual(s.url, f"http://127.0.0.1:{port}")
        self.assertEqual(ask(s.url + "/records", token="t" * 48), (200, {"records": []}))
        self.assertEqual(ask(s.url + "/records", token="t" * 47)[0], 401)

    def test_a_short_token_is_refused(self):
        with self.assertRaises(ValueError):
            sidefx.Service(token="short")

    def test_run_as_a_script_it_announces_itself_and_serves(self):
        import os
        import subprocess
        env = {**os.environ, "SIDEFX_TOKEN": "k" * 48}
        proc = subprocess.Popen([sys.executable, str(Path(sidefx.__file__)), "--port", "0"], env=env,
                                stdout=subprocess.PIPE, text=True)
        self.addCleanup(proc.wait)
        self.addCleanup(proc.terminate)
        line = proc.stdout.readline().strip()
        self.assertTrue(line.startswith("serving http://127.0.0.1:"), line)
        url = line.split()[1]
        self.assertEqual(ask(url + "/publish", {"version": "1.0.0"})[0], 503)
        self.assertEqual(ask(url + "/records", token="k" * 48)[1]["records"][0]["status"], 503)

    def test_run_as_a_script_without_a_token_it_refuses_to_start(self):
        import os
        import subprocess
        env = {k: v for k, v in os.environ.items() if k != "SIDEFX_TOKEN"}
        proc = subprocess.run([sys.executable, str(Path(sidefx.__file__)), "--port", "0"], env=env,
                              capture_output=True, text=True, timeout=10)
        self.assertNotEqual(proc.returncode, 0)


if __name__ == "__main__":
    unittest.main()
