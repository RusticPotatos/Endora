"""Tests for the read-only mail mirror. Standard library only; no network, no mailbox.

Run: python3 -I roles/mail_mirror/tests/test_mail_mirror.py
"""
import base64
import io
import json
import os
import re
import sys
import tempfile
import unittest
import urllib.error
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "files"))
import mail_mirror as mm  # noqa: E402

DAY = 24 * 60 * 60
NOW = 1_800_000_000.0


def raw_message(subject, sender="Jane Doe <jane@example.com>"):
    return (
        "From: %s\r\nTo: john@example.com\r\nSubject: %s\r\n\r\nHello.\r\n" % (sender, subject)
    ).encode("utf-8")


class FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class FakeGoogle:
    """Serves the token endpoint and a read-only slice of the Gmail API, and fails any test
    that asks it for anything else."""

    def __init__(self, messages, page_size=500):
        self.messages = messages  # id -> raw bytes
        self.page_size = page_size
        self.requests = []

    def __call__(self, request, timeout=None):
        url, method = request.full_url, request.get_method()
        self.requests.append((method, url))
        if url == mm.GMAIL_TOKEN_URL:
            assert method == "POST", method
            return FakeResponse(json.dumps({"access_token": "t"}).encode())
        assert method == "GET", "only reads may reach Gmail: %s %s" % (method, url)
        assert url.startswith(mm.GMAIL_API), url
        assert request.get_header("Authorization") == "Bearer t"
        path = url[len(mm.GMAIL_API):]
        if path.startswith("/messages?"):
            query = urllib.parse.parse_qs(urllib.parse.urlparse(path).query)
            assert "includeSpamTrash" not in query
            ids = sorted(self.messages)
            start = int(query.get("pageToken", ["0"])[0])
            page = ids[start : start + self.page_size]
            body = {"messages": [{"id": i} for i in page]}
            if start + self.page_size < len(ids):
                body["nextPageToken"] = str(start + self.page_size)
            return FakeResponse(json.dumps(body).encode())
        match = re.match(r"^/messages/([^?]+)\?format=raw$", path)
        assert match, path
        raw = base64.urlsafe_b64encode(self.messages[match.group(1)]).decode().rstrip("=")
        return FakeResponse(json.dumps({"raw": raw}).encode())


class FakeImap:
    """An IMAP server that only knows the read-only commands. Anything else is a failure."""

    def __init__(self, folders, gm_ids=None, user="john@example.com"):
        self.folders = folders  # name -> {uid: raw}
        self.gm_ids = gm_ids or {}  # uid -> Gmail's own message id, as Gmail's IMAP reports it
        self.user = user
        self.selected = None
        self.logged_out = False
        self.bodies_fetched = 0

    def login(self, user, password):
        assert (user, password) == (self.user, "app-password")
        return "OK", [b""]

    def select(self, mailbox, readonly=False):
        assert readonly, "a folder must be opened read-only (EXAMINE)"
        name = mailbox.strip('"')
        if name not in self.folders:
            return "NO", [b""]
        self.selected = name
        return "OK", [str(len(self.folders[name])).encode()]

    def response(self, code):
        assert code == "UIDVALIDITY"
        return code, [b"42"]

    def uid(self, command, *args):
        if command == "SEARCH":
            return "OK", [b" ".join(str(u).encode() for u in sorted(self.folders[self.selected]))]
        assert command == "FETCH", command
        uids, what = args
        if what == "(X-GM-MSGID)":
            # Just the ids, for a whole set of messages in one reply.
            return "OK", [
                b"%d (X-GM-MSGID %d UID %d)" % (n, self.gm_ids[int(u)], int(u))
                for n, u in enumerate(uids.split(","), 1)
            ]
        assert what == "(BODY.PEEK[])", "a fetch must not mark mail as read: %s" % what
        self.bodies_fetched += 1
        head = b"%s (UID %s BODY[] {n}" % (uids.encode(), uids.encode())
        return "OK", [(head, self.folders[self.selected][int(uids)]), b")"]

    def logout(self):
        self.logged_out = True

    def __getattr__(self, name):
        raise AssertionError("the mirror called IMAP %s, which can change a mailbox" % name)


GMAIL = {"name": "personal", "kind": "gmail", "client_id": "c", "client_secret": "s", "refresh_token": "r"}
ICLOUD = {"name": "icloud", "kind": "icloud", "user": "john@example.com", "app_password": "app-password"}


class Paths(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = self.dir.name
        self.maildir = os.path.join(self.root, "maildir")
        self.state = os.path.join(self.root, "state")
        os.makedirs(self.state)
        self.config = os.path.join(self.root, "accounts.json")

    def tearDown(self):
        self.dir.cleanup()

    def accounts(self, *accounts):
        with open(self.config, "w") as handle:
            json.dump(list(accounts), handle)

    def files(self, account):
        new = os.path.join(self.maildir, account, "new")
        return sorted(os.listdir(new)) if os.path.isdir(new) else []


class TheWindow(unittest.TestCase):
    def test_it_starts_at_a_week(self):
        self.assertEqual(mm.window_days({}, NOW)[0], 7)

    def test_it_widens_one_step_once_a_step_has_had_most_of_a_day(self):
        days, state = mm.window_days({"window_index": 0, "widened_at": NOW - 19 * 3600}, NOW)
        self.assertEqual(days, 7)
        days, state = mm.window_days({"window_index": 0, "widened_at": NOW - 20 * 3600}, NOW)
        self.assertEqual((days, state["widened_at"]), (14, NOW))

    def test_it_stops_at_a_year(self):
        state = {"window_index": 0, "widened_at": 0}
        seen = []
        for day in range(20):
            days, state = mm.window_days(state, NOW + day * DAY)
            seen.append(days)
        self.assertEqual(seen[:7], [14, 28, 56, 112, 224, 365, 365])
        self.assertEqual(seen[-1], 365)


class Gmail(Paths):
    def test_it_copies_what_is_there_and_only_reads(self):
        google = FakeGoogle({"a1": raw_message("one"), "b2": raw_message("two")})
        maildir = mm.Maildir(self.maildir, "personal")
        self.assertEqual(mm.sync_gmail(GMAIL, maildir, NOW - 7 * DAY, google), 2)
        self.assertEqual(self.files("personal"), ["gmail-a1", "gmail-b2"])
        with open(os.path.join(self.maildir, "personal", "new", "gmail-a1"), "rb") as handle:
            self.assertEqual(handle.read(), raw_message("one"))
        listing = [u for m, u in google.requests if "/messages?" in u][0]
        self.assertIn(urllib.parse.quote("after:%d" % int(NOW - 7 * DAY)), listing)

    def test_copying_again_adds_nothing(self):
        google = FakeGoogle({"a1": raw_message("one")})
        maildir = mm.Maildir(self.maildir, "personal")
        mm.sync_gmail(GMAIL, maildir, 0, google)
        self.assertEqual(mm.sync_gmail(GMAIL, maildir, 0, google), 0)
        fetched = [u for m, u in google.requests if "format=raw" in u]
        self.assertEqual(len(fetched), 1, "a message already copied was fetched again")

    def test_it_follows_pages_and_stops_at_its_limit(self):
        google = FakeGoogle({"m%03d" % n: raw_message(str(n)) for n in range(25)}, page_size=10)
        maildir = mm.Maildir(self.maildir, "personal")
        self.assertEqual(mm.sync_gmail(GMAIL, maildir, 0, google, limit=17), 17)
        self.assertEqual(mm.sync_gmail(GMAIL, maildir, 0, google, limit=17), 8)


class GoogleHiccups(Paths):
    """Found on the first live run: 104 messages copied, then one HTTPError, then the same pass
    by hand went through clean. Google's limits and servers say "try again" now and then."""

    def flaky(self, failures, code):
        google = FakeGoogle({"a1": raw_message("one"), "b2": raw_message("two")})
        left = {"n": failures}

        def opener(request, timeout=None):
            if "format=raw" in request.full_url and left["n"] > 0:
                left["n"] -= 1
                raise urllib.error.HTTPError(request.full_url, code, "busy", {}, io.BytesIO(b"{}"))
            return google(request, timeout)

        return opener

    def test_a_brief_rate_limit_or_server_error_is_waited_out(self):
        waits = []
        for code in (429, 500, 503):
            maildir = mm.Maildir(self.maildir, "personal-%d" % code)
            added = mm.sync_gmail(GMAIL, maildir, 0, self.flaky(2, code), sleep=waits.append)
            self.assertEqual(added, 2, code)
        self.assertTrue(waits and all(w > 0 for w in waits), waits)

    def test_it_gives_up_after_a_few_tries_and_says_which_status(self):
        self.accounts(GMAIL)
        report = mm.run_once(self.config, self.maildir, self.state, now=NOW,
                             opener=self.flaky(99, 429), sleep=lambda _: None)
        self.assertEqual(report["personal"], "failed (HTTPError 429)")

    def test_a_refusal_is_not_retried(self):
        waits = []
        maildir = mm.Maildir(self.maildir, "personal")
        with self.assertRaises(urllib.error.HTTPError):
            mm.sync_gmail(GMAIL, maildir, 0, self.flaky(1, 403), sleep=waits.append)
        self.assertEqual(waits, [], "a 403 will not change by waiting")


GMAIL_IMAP = {"name": "work", "kind": "gmail_imap", "user": "jane@example.com",
              "app_password": "app-password"}


class GmailOverImap(Paths):
    """Gmail with an app password, read-only over IMAP: the way Home Assistant already reads it."""

    def server(self):
        return FakeImap(
            {"[Gmail]/All Mail": {11: raw_message("hello"), 12: raw_message("again")}},
            gm_ids={11: 0x18F2A3B4C5D6E7F8, 12: 0x18F2A3B4C5D6E7F9},
            user="jane@example.com",
        )

    def test_it_reads_all_mail_read_only_from_gmails_own_server(self):
        hosts = []
        server = self.server()

        def connect(host):
            hosts.append(host)
            return server

        maildir = mm.Maildir(self.maildir, "work")
        self.assertEqual(mm.sync_imap(GMAIL_IMAP, maildir, 0, connect, gmail=True), 2)
        self.assertEqual(hosts, ["imap.gmail.com"])

    def test_a_message_is_named_as_the_gmail_api_names_it_so_nothing_is_copied_twice(self):
        # Gmail's API id is Gmail's IMAP X-GM-MSGID in hex. Mail the API already copied must
        # not come back under a second name when the account moves to IMAP.
        maildir = mm.Maildir(self.maildir, "work")
        maildir.add("gmail-18f2a3b4c5d6e7f8", raw_message("hello"))
        server = self.server()
        added = mm.sync_imap(GMAIL_IMAP, maildir, 0, lambda host: server, gmail=True)
        self.assertEqual(added, 1, "the message the API copied was copied again")
        self.assertEqual(server.bodies_fetched, 1, "a message already here was downloaded again")
        self.assertEqual(self.files("work"), ["gmail-18f2a3b4c5d6e7f8", "gmail-18f2a3b4c5d6e7f9"])

    def test_a_pass_through_run_once_uses_gmails_server_for_gmail_and_apples_for_icloud(self):
        self.accounts(GMAIL_IMAP, ICLOUD)
        hosts = []

        def connect(host):
            hosts.append(host)
            return self.server() if host == "imap.gmail.com" else ICloud.server(self)

        report = mm.run_once(self.config, self.maildir, self.state, now=NOW, connect=connect)
        self.assertTrue(report["work"].startswith("2 new"), report)
        self.assertTrue(report["icloud"].startswith("3 new"), report)
        self.assertEqual(sorted(hosts), ["imap.gmail.com", "imap.mail.me.com"])

    def test_a_pass_with_nothing_new_downloads_no_mail(self):
        maildir = mm.Maildir(self.maildir, "work")
        mm.sync_imap(GMAIL_IMAP, maildir, 0, lambda host: self.server(), gmail=True)
        server = self.server()
        self.assertEqual(mm.sync_imap(GMAIL_IMAP, maildir, 0, lambda host: server, gmail=True), 0)
        self.assertEqual(server.bodies_fetched, 0)

    def test_it_needs_a_user_and_an_app_password(self):
        self.accounts(dict(GMAIL_IMAP, app_password=""))
        with self.assertRaises(mm.ConfigError):
            mm.load_accounts(self.config)


class ICloud(Paths):
    def server(self):
        return FakeImap(
            {
                "INBOX": {1: raw_message("hello"), 2: raw_message("again")},
                "Sent Messages": {7: raw_message("reply", sender="John Doe <john@example.com>")},
            }
        )

    def test_it_copies_read_only_from_the_inbox_and_sent(self):
        server = self.server()
        maildir = mm.Maildir(self.maildir, "icloud")
        self.assertEqual(mm.sync_imap(ICLOUD, maildir, NOW - 7 * DAY, lambda host: server), 3)
        self.assertEqual(self.files("icloud"), ["inbox.42.1", "inbox.42.2", "sent-messages.42.7"])
        self.assertTrue(server.logged_out)

    def test_copying_again_adds_nothing(self):
        maildir = mm.Maildir(self.maildir, "icloud")
        mm.sync_imap(ICLOUD, maildir, 0, lambda host: self.server())
        self.assertEqual(mm.sync_imap(ICLOUD, maildir, 0, lambda host: self.server()), 0)

    def test_a_folder_that_is_not_there_is_skipped(self):
        account = dict(ICLOUD, folders=["Nope", "INBOX"])
        maildir = mm.Maildir(self.maildir, "icloud")
        self.assertEqual(mm.sync_imap(account, maildir, 0, lambda host: self.server()), 2)


class OnePass(Paths):
    def test_one_account_failing_does_not_stop_the_others_and_says_nothing_of_the_mail(self):
        self.accounts(GMAIL, ICLOUD)

        def broken(request, timeout=None):
            raise OSError("Subject: Your invoice from Example Dental is ready")

        report = mm.run_once(
            self.config, self.maildir, self.state, now=NOW, opener=broken,
            connect=lambda host: ICloud.server(self),
        )
        self.assertEqual(report["personal"], "failed (OSError)")
        self.assertTrue(report["icloud"].startswith("3 new"), report)
        self.assertNotIn("invoice", json.dumps(report))
        self.assertFalse(os.path.exists(os.path.join(self.state, "personal.json")),
                         "a failed pass must not widen the window")
        self.assertTrue(os.path.exists(os.path.join(self.state, "icloud.json")))

    def test_the_window_it_used_is_kept_for_next_time(self):
        self.accounts(GMAIL)
        google = FakeGoogle({})
        mm.run_once(self.config, self.maildir, self.state, now=NOW, opener=google)
        report = mm.run_once(self.config, self.maildir, self.state, now=NOW + DAY, opener=google)
        self.assertIn("window 14 days", report["personal"])


class Configuration(Paths):
    def bad(self, *accounts):
        self.accounts(*accounts)
        with self.assertRaises(mm.ConfigError):
            mm.load_accounts(self.config)

    def test_a_name_cannot_reach_outside_the_maildir(self):
        self.bad(dict(GMAIL, name="../etc"))
        self.bad(dict(GMAIL, name="Personal"))

    def test_names_are_unique(self):
        self.bad(GMAIL, dict(ICLOUD, name="personal"))

    def test_an_account_has_what_it_needs(self):
        self.bad(dict(GMAIL, refresh_token=""))
        self.bad(dict(ICLOUD, app_password=" "))
        self.bad(dict(GMAIL, kind="outlook"))

    def test_a_good_configuration_loads(self):
        self.accounts(GMAIL, ICLOUD)
        self.assertEqual(len(mm.load_accounts(self.config)), 2)


class ItCannotChangeAMailbox(unittest.TestCase):
    """A guard on the source, so a later edit cannot quietly add a way to change mail."""

    def test_no_command_that_changes_mail_appears_in_the_mirror(self):
        with open(os.path.join(HERE, "..", "files", "mail_mirror.py"), encoding="utf-8") as handle:
            source = handle.read()
        for word in ("STORE", "COPY", "MOVE", "EXPUNGE", "APPEND", ".store(", ".copy(",
                     ".expunge(", ".append(", ".delete(", "/modify", "/trash", "/send",
                     "batchDelete", "batchModify", "readonly=False"):
            if word == ".append(":
                continue  # list.append is fine; imap.append is caught by the fake server
            self.assertNotIn(word, source, word)
        self.assertIn("readonly=True", source)
        self.assertIn("BODY.PEEK[]", source)


import gmail_consent as gc  # noqa: E402


class Consent(unittest.TestCase):
    def test_it_asks_for_read_only_and_for_a_refresh_token(self):
        url = gc.consent_url("cid", "http://127.0.0.1:5555/", "st", "ch")
        query = urllib.parse.parse_qs(urllib.parse.urlparse(url).query)
        self.assertEqual(query["scope"], ["https://www.googleapis.com/auth/gmail.readonly"])
        self.assertEqual(query["access_type"], ["offline"])
        self.assertEqual(query["code_challenge_method"], ["S256"])

    def test_a_reply_for_another_request_is_refused(self):
        self.assertEqual(gc.code_from("/?state=st&code=abc", "st"), "abc")
        with self.assertRaises(ValueError):
            gc.code_from("/?state=other&code=abc", "st")
        with self.assertRaises(ValueError):
            gc.code_from("/?state=st&error=access_denied", "st")

    def test_a_token_wider_than_read_only_is_refused(self):
        def google(scope):
            def opener(request, timeout=None):
                return FakeResponse(json.dumps({"refresh_token": "r", "scope": scope}).encode())
            return opener

        self.assertEqual(
            gc.exchange("c", "s", "code", "v", "http://127.0.0.1:1/", google(gc.SCOPE)), "r"
        )
        with self.assertRaises(ValueError):
            gc.exchange("c", "s", "code", "v", "http://127.0.0.1:1/",
                        google("https://mail.google.com/"))


if __name__ == "__main__":
    unittest.main(verbosity=1)
