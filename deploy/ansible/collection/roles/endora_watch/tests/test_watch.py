"""The house watch (files/watch.py): what it says, and when, as the checks come and go.

Run: python3 -I roles/endora_watch/tests/test_watch.py
"""
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "files"))
import watch  # noqa: E402

GRACE = 600
T0 = 1_700_000_000


def config(**overrides):
    base = {
        "alert_url": "https://ntfy.example.com/house",
        "grace_minutes": 10,
        "checks": [
            {"name": "NAS", "tcp": "192.0.2.10:22"},
            {"name": "Pi-hole", "dns": {"server": "192.0.2.2", "name": "nas.home.arpa",
                                        "expect": "192.0.2.10"}},
        ],
    }
    base.update(overrides)
    return base


class TheSettings(unittest.TestCase):
    def test_good_settings_load(self):
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
            json.dump(config(), handle)
        try:
            loaded = watch.load_config(handle.name)
        finally:
            os.unlink(handle.name)
        self.assertEqual([c["name"] for c in loaded["checks"]], ["NAS", "Pi-hole"])

    def refused(self, settings):
        with self.assertRaises(ValueError):
            watch.check_config(settings)

    def test_no_alert_address_is_refused(self):
        self.refused(config(alert_url=""))

    def test_an_alert_address_in_the_clear_is_refused(self):
        self.refused(config(alert_url="http://ntfy.example.com/house"))

    def test_no_checks_is_refused(self):
        self.refused(config(checks=[]))

    def test_two_checks_of_one_name_are_refused(self):
        self.refused(config(checks=[{"name": "NAS", "tcp": "192.0.2.10:22"},
                                    {"name": "NAS", "tcp": "192.0.2.10:80"}]))

    def test_a_check_of_no_kind_is_refused(self):
        self.refused(config(checks=[{"name": "NAS"}]))

    def test_a_check_of_two_kinds_is_refused(self):
        self.refused(config(checks=[{"name": "NAS", "tcp": "192.0.2.10:22",
                                     "dns": {"server": "192.0.2.2", "name": "a", "expect": "b"}}]))

    def test_a_tcp_check_without_a_port_is_refused(self):
        self.refused(config(checks=[{"name": "NAS", "tcp": "192.0.2.10"}]))


class WhenItSpeaks(unittest.TestCase):
    """advance() turns a minute's results into what changed, after the grace."""

    def step(self, state, now, **results):
        return watch.advance(state, {k: (v, "refused") for k, v in results.items()}, now, GRACE)

    def test_all_well_is_silent(self):
        state, down, back = self.step({}, T0, NAS=True)
        self.assertEqual((down, back), ([], []))

    def test_a_short_outage_is_silent(self):
        # The NAS's nightly reboot takes a few minutes: no word of it.
        state, down, back = self.step({}, T0, NAS=False)
        state, down, back = self.step(state, T0 + 300, NAS=False)
        self.assertEqual(down, [])
        state, down, back = self.step(state, T0 + 360, NAS=True)
        self.assertEqual((down, back), ([], []))

    def test_an_outage_past_the_grace_is_said_once(self):
        state, down, _ = self.step({}, T0, NAS=False)
        state, down, _ = self.step(state, T0 + GRACE, NAS=False)
        self.assertEqual([d["name"] for d in down], ["NAS"])
        self.assertEqual(down[0]["since"], T0)
        self.assertEqual(down[0]["detail"], "refused")
        state, down, _ = self.step(state, T0 + GRACE + 60, NAS=False)
        self.assertEqual(down, [])

    def test_its_return_is_said_once(self):
        state, _, _ = self.step({}, T0, NAS=False)
        state, _, _ = self.step(state, T0 + GRACE, NAS=False)
        state, down, back = self.step(state, T0 + GRACE + 120, NAS=True)
        self.assertEqual(down, [])
        self.assertEqual([b["name"] for b in back], ["NAS"])
        self.assertEqual(back[0]["since"], T0)
        state, down, back = self.step(state, T0 + GRACE + 180, NAS=True)
        self.assertEqual((down, back), ([], []))

    def test_a_check_no_longer_listed_is_forgotten(self):
        state, _, _ = self.step({}, T0, NAS=False, Plex=False)
        state, _, _ = self.step(state, T0 + 60, NAS=False)
        self.assertNotIn("Plex", state)


class WhatItSays(unittest.TestCase):
    def test_nothing_changed_says_nothing(self):
        self.assertIsNone(watch.notification([], [], 0))

    def test_down_is_urgent_and_names_each(self):
        note = watch.notification(
            [{"name": "NAS", "since": T0, "detail": "timed out"},
             {"name": "Plex", "since": T0, "detail": "refused"}], [], 0)
        self.assertEqual(note["title"], "2 down: NAS, Plex")
        self.assertEqual(note["priority"], "high")
        self.assertIn("NAS: timed out", note["body"])

    def test_back_alone_is_quiet(self):
        note = watch.notification([], [{"name": "NAS", "since": T0}], 0)
        self.assertEqual(note["title"], "Back: NAS")
        self.assertEqual(note["priority"], "default")

    def test_it_says_when_it_starts(self):
        note = watch.started(5)
        self.assertEqual(note["title"], "House watch started")
        self.assertIn("5 checks", note["body"])
        self.assertEqual(note["priority"], "low")

    def test_it_is_plain_ascii(self):
        note = watch.notification([{"name": "NAS", "since": T0, "detail": "x"}],
                                  [{"name": "Plex", "since": T0}], 0)
        for value in note.values():
            value.encode("ascii")


class TheOutbox(unittest.TestCase):
    def test_what_could_not_be_sent_waits_for_the_next_minute(self):
        sent = []

        def send(note):
            if note["title"] == "b":
                raise OSError("no route")
            sent.append(note["title"])

        left = watch.deliver([{"title": "a"}, {"title": "b"}, {"title": "c"}], send)
        self.assertEqual(sent, ["a"])
        self.assertEqual([n["title"] for n in left], ["b", "c"])

    def test_the_outbox_keeps_only_the_newest(self):
        outbox = [{"title": str(i)} for i in range(watch.OUTBOX_LIMIT + 5)]

        def send(_note):
            raise OSError("offline")

        left = watch.deliver(outbox, send)
        self.assertEqual(len(left), watch.OUTBOX_LIMIT)
        self.assertEqual(left[-1]["title"], str(watch.OUTBOX_LIMIT + 4))


class TheDnsCheck(unittest.TestCase):
    def answer(self, query, addresses, rcode=0):
        qid = query[:2]
        question = query[12:]
        header = qid + struct.pack(">HHHHH", 0x8180 | rcode, 1, len(addresses), 0, 0)
        records = b""
        for address in addresses:
            records += b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 60, 4) + socket.inet_aton(address)
        return header + question + records

    def test_the_query_asks_for_the_name(self):
        query = watch.dns_query("nas.home.arpa", 0x1234)
        self.assertEqual(query[:2], b"\x12\x34")
        self.assertIn(b"\x03nas\x04home\x04arpa\x00\x00\x01\x00\x01", query)

    def test_answers_are_read(self):
        query = watch.dns_query("nas.home.arpa", 7)
        self.assertEqual(watch.dns_answers(self.answer(query, ["192.0.2.10", "192.0.2.11"]), 7),
                         ["192.0.2.10", "192.0.2.11"])

    def test_another_query_s_answer_is_refused(self):
        query = watch.dns_query("nas.home.arpa", 7)
        with self.assertRaises(ValueError):
            watch.dns_answers(self.answer(query, ["192.0.2.10"]), 8)

    def test_a_name_error_is_an_error(self):
        query = watch.dns_query("nas.home.arpa", 7)
        with self.assertRaises(ValueError):
            watch.dns_answers(self.answer(query, [], rcode=3), 7)

    def test_a_live_server(self):
        server = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        server.bind(("127.0.0.1", 0))
        port = server.getsockname()[1]

        def serve():
            query, client = server.recvfrom(512)
            server.sendto(self.answer(query, ["192.0.2.10"]), client)

        thread = threading.Thread(target=serve)
        thread.start()
        try:
            ok, detail = watch.probe({"name": "Pi-hole", "dns": {
                "server": "127.0.0.1", "port": port, "name": "nas.home.arpa",
                "expect": "192.0.2.10"}}, 2)
        finally:
            thread.join()
            server.close()
        self.assertTrue(ok, detail)


class TheTcpCheck(unittest.TestCase):
    def test_an_open_port_is_up(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        try:
            ok, _ = watch.probe({"name": "x", "tcp": "127.0.0.1:%d" % listener.getsockname()[1]}, 2)
        finally:
            listener.close()
        self.assertTrue(ok)

    def test_a_closed_port_is_down(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
        listener.close()
        ok, detail = watch.probe({"name": "x", "tcp": "127.0.0.1:%d" % port}, 2)
        self.assertFalse(ok)
        self.assertTrue(detail)


class TheHttpsCheck(unittest.TestCase):
    def test_the_system_s_trust_is_strict(self):
        context = watch.tls_context(None)
        self.assertEqual(context.verify_mode, watch.ssl.CERT_REQUIRED)
        self.assertTrue(context.check_hostname)

    def test_a_house_ca_is_trusted_without_the_strict_profile(self):
        # Python 3.13 turns on X509_STRICT by default, which refuses a home-made CA that lacks an
        # Authority Key Identifier; with the house's CA the chain and the name are still checked.
        if not shutil.which("openssl"):
            self.skipTest("openssl is not installed")
        with tempfile.TemporaryDirectory() as directory:
            ca = os.path.join(directory, "ca.pem")
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                            "-subj", "/CN=Test CA", "-keyout", os.path.join(directory, "ca.key"),
                            "-out", ca], check=True, capture_output=True)
            context = watch.tls_context(ca)
        self.assertFalse(context.verify_flags & getattr(watch.ssl, "VERIFY_X509_STRICT", 0))
        self.assertEqual(context.verify_mode, watch.ssl.CERT_REQUIRED)
        self.assertTrue(context.check_hostname)
        self.assertEqual(len(context.get_ca_certs()), 1)


class TheState(unittest.TestCase):
    def test_no_state_yet_means_just_started(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(watch.read_state(os.path.join(directory, "state.json")))

    def test_it_is_kept_between_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "state.json")
            watch.write_state(path, {"checks": {"NAS": {"failing_since": T0}}, "outbox": []})
            self.assertEqual(watch.read_state(path)["checks"]["NAS"]["failing_since"], T0)


class ARun(unittest.TestCase):
    """main(), one minute at a time, with the phone stood in for."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(4)
        self.addCleanup(listener.close)
        self.settings = os.path.join(self.tmp.name, "watch.json")
        with open(self.settings, "w") as handle:
            json.dump(config(checks=[{"name": "Here", "tcp": "127.0.0.1:%d"
                                      % listener.getsockname()[1]}]), handle)
        self.sent, self.offline = [], False
        os.environ["RUNTIME_DIRECTORY"] = self.tmp.name

    def tearDown(self):
        del os.environ["RUNTIME_DIRECTORY"]
        self.tmp.cleanup()

    def run_once(self):
        def ntfy(_url):
            def send(note):
                if self.offline:
                    raise OSError("offline")
                self.sent.append(note["title"])
            return send
        real, watch.ntfy = watch.ntfy, ntfy
        try:
            self.assertEqual(watch.main(["watch.py", self.settings]), 0)
        finally:
            watch.ntfy = real

    def test_the_first_run_says_it_started_and_later_ones_are_quiet(self):
        self.run_once()
        self.run_once()
        self.assertEqual(self.sent, ["House watch started"])

    def test_check_reads_the_settings_and_runs_nothing(self):
        self.assertEqual(watch.main(["watch.py", "--check", self.settings]), 0)
        self.assertIsNone(watch.read_state(os.path.join(self.tmp.name, "state.json")))

    def test_what_could_not_be_sent_goes_on_the_next_run(self):
        self.offline = True
        self.run_once()
        self.offline = False
        self.run_once()
        self.assertEqual(self.sent, ["House watch started"])


if __name__ == "__main__":
    unittest.main()
