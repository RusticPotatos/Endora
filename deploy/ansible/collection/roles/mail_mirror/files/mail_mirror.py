#!/usr/bin/env python3
"""A read-only copy of a mailbox, as a Maildir, for a local assistant to read.

Gmail can be read two ways. Through the Gmail API with a `gmail.readonly` token, Google itself
refuses anything but reading. Over IMAP with an app password (the way Home Assistant reads it),
the credential could do more, so this file is what keeps it to reading. iCloud has no such API
and is read over IMAP. Over IMAP it only ever uses EXAMINE (a read-only select) and BODY.PEEK
(which does not mark anything as read); no command that changes a mailbox is in this file.

The window of mail it copies widens one step a day, from a week to a year, so the first runs
are small and the history arrives gradually. Nothing it logs names a sender or a subject:
container logs are collected elsewhere, and a count is all they need.

Standard library only, so the image needs nothing installed.
"""
import base64
import imaplib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

# Fixed, never from configuration: an account can name its credentials, not where they go.
GMAIL_TOKEN_URL = "https://oauth2.googleapis.com/token"
GMAIL_API = "https://gmail.googleapis.com/gmail/v1/users/me"
ICLOUD_HOST = "imap.mail.me.com"
GMAIL_IMAP_HOST = "imap.gmail.com"
IMAP_PORT = 993
# Gmail's IMAP reports each message's own id (X-GM-MSGID). In hex it is the Gmail API's id, so
# a message is named the same whichever way the account is read and is never copied twice.
GM_MSGID = re.compile(rb"X-GM-MSGID (\d+)")

# Days of mail to copy: a week first, then wider, one step a day.
WINDOWS = [7, 14, 28, 56, 112, 224, 365]
WIDEN_AFTER_SECONDS = 20 * 60 * 60
# Most messages copied per account per run, so a widening window is caught up gently.
MAX_PER_RUN = 500
NAME = re.compile(r"^[a-z0-9_-]{1,32}$")
# Google answers "try again" now and then: a rate limit, or a server having a moment. Seen on
# the first live run, 104 messages in. These are waited out, a few times, before the pass
# gives up; anything else (a refusal, a missing message) will not change by waiting.
TRY_AGAIN = {429, 500, 502, 503, 504}
WAITS_SECONDS = [2, 8, 30]
SAFE_ID = re.compile(r"[^A-Za-z0-9._-]")


class ConfigError(Exception):
    pass


def load_accounts(path):
    with open(path, encoding="utf-8") as handle:
        accounts = json.load(handle)
    if not isinstance(accounts, list):
        raise ConfigError("accounts must be a list")
    names = set()
    for account in accounts:
        name = account.get("name", "")
        if not NAME.match(name):
            raise ConfigError("an account name must be 1-32 of a-z, 0-9, _ and -")
        if name in names:
            raise ConfigError("account names must be unique")
        names.add(name)
        kind = account.get("kind")
        needs = {
            "gmail": ("client_id", "client_secret", "refresh_token"),
            "gmail_imap": ("user", "app_password"),
            "icloud": ("user", "app_password"),
        }.get(kind)
        if needs is None:
            raise ConfigError("an account kind is gmail, gmail_imap or icloud")
        for key in needs:
            if not str(account.get(key, "")).strip():
                raise ConfigError("account %s is missing %s" % (name, key))
    return accounts


# ---- the window -----------------------------------------------------------------------


def window_days(state, now):
    """The days of mail to copy now, widening one step once a step has been in place long
    enough. Returns (days, new_state)."""
    index = min(int(state.get("window_index", 0)), len(WINDOWS) - 1)
    widened_at = float(state.get("widened_at", now))
    if index < len(WINDOWS) - 1 and now - widened_at >= WIDEN_AFTER_SECONDS:
        index += 1
        widened_at = now
    return WINDOWS[index], {"window_index": index, "widened_at": widened_at}


def read_state(state_dir, name):
    try:
        with open(os.path.join(state_dir, name + ".json"), encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError):
        return {}


def write_state(state_dir, name, state):
    path = os.path.join(state_dir, name + ".json")
    temporary = path + ".tmp"
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(state, handle)
    os.replace(temporary, path)


# ---- the Maildir ----------------------------------------------------------------------


class Maildir:
    """One account's Maildir. A message's file name is derived from its id at the source, so
    copying the same message twice is the same file, not a second one."""

    def __init__(self, root, account):
        self.base = os.path.join(root, account)
        for sub in ("tmp", "new", "cur"):
            os.makedirs(os.path.join(self.base, sub), mode=0o750, exist_ok=True)

    def _name(self, message_id):
        return SAFE_ID.sub("_", message_id)[:200]

    def has(self, message_id):
        name = self._name(message_id)
        return any(
            os.path.exists(os.path.join(self.base, sub, name)) for sub in ("new", "cur")
        )

    def add(self, message_id, raw):
        name = self._name(message_id)
        temporary = os.path.join(self.base, "tmp", name)
        with open(temporary, "wb") as handle:
            handle.write(raw)
        os.replace(temporary, os.path.join(self.base, "new", name))


# ---- Gmail ----------------------------------------------------------------------------


def _json(opener, request, sleep=time.sleep):
    for wait in WAITS_SECONDS + [None]:
        try:
            with opener(request, timeout=60) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            if error.code not in TRY_AGAIN or wait is None:
                raise
            sleep(wait)
    raise AssertionError("unreachable")


def gmail_access_token(account, opener, sleep=time.sleep):
    body = urllib.parse.urlencode(
        {
            "client_id": account["client_id"],
            "client_secret": account["client_secret"],
            "refresh_token": account["refresh_token"],
            "grant_type": "refresh_token",
        }
    ).encode("ascii")
    request = urllib.request.Request(GMAIL_TOKEN_URL, data=body, method="POST")
    return _json(opener, request, sleep)["access_token"]


def _gmail_get(path, token, opener, sleep=time.sleep):
    request = urllib.request.Request(
        GMAIL_API + path, headers={"Authorization": "Bearer " + token}, method="GET"
    )
    return _json(opener, request, sleep)


def sync_gmail(account, maildir, since_epoch, opener, limit=MAX_PER_RUN, sleep=time.sleep):
    """Copies Gmail messages since a moment. Spam and Trash are left out, which is what the
    API's list does unless asked otherwise. Returns how many were added."""
    token = gmail_access_token(account, opener, sleep)
    query = urllib.parse.quote("after:%d" % int(since_epoch))
    added = 0
    page = ""
    while added < limit:
        path = "/messages?maxResults=500&q=" + query
        if page:
            path += "&pageToken=" + urllib.parse.quote(page)
        listing = _gmail_get(path, token, opener, sleep)
        for item in listing.get("messages", []):
            if added >= limit:
                break
            message_id = "gmail-" + item["id"]
            if maildir.has(message_id):
                continue
            raw = _gmail_get(
                "/messages/%s?format=raw" % urllib.parse.quote(item["id"]), token, opener, sleep
            )["raw"]
            maildir.add(message_id, base64.urlsafe_b64decode(raw + "=" * (-len(raw) % 4)))
            added += 1
        page = listing.get("nextPageToken", "")
        if not page:
            break
    return added


# ---- IMAP (iCloud, and Gmail with an app password) -------------------------------------


def _gmail_ids(imap, uids, batch=500):
    """Gmail's own id for each message, asked for in a few cheap requests rather than learned
    by downloading every message: a pass with nothing new then downloads no mail at all."""
    names = {}
    for start in range(0, len(uids), batch):
        status, lines = imap.uid("FETCH", ",".join(uids[start : start + batch]), "(X-GM-MSGID)")
        if status != "OK":
            continue
        for line in lines:
            line = line[0] if isinstance(line, tuple) else line
            gm = GM_MSGID.search(line or b"")
            uid = re.search(rb"UID (\d+)", line or b"")
            if gm and uid:
                names[uid.group(1).decode("ascii")] = "gmail-%x" % int(gm.group(1))
    return names


def _imap_date(since_epoch):
    return time.strftime("%d-%b-%Y", time.gmtime(since_epoch))


def sync_imap(account, maildir, since_epoch, connect, limit=MAX_PER_RUN, gmail=False):
    """Copies messages since a day over IMAP, read-only: every folder is opened with
    EXAMINE and every message fetched with BODY.PEEK. Returns how many were added.

    For Gmail, "All Mail" is read (everything but Spam and Trash, sent mail included), and each
    message is named by Gmail's own id rather than by its place in a folder."""
    default = ["[Gmail]/All Mail"] if gmail else ["INBOX", "Sent Messages"]
    folders = account.get("folders") or default
    imap = connect(GMAIL_IMAP_HOST if gmail else ICLOUD_HOST)
    added = 0
    try:
        imap.login(account["user"], account["app_password"])
        for folder in folders:
            if added >= limit:
                break
            status, _ = imap.select('"%s"' % folder.replace('"', ""), readonly=True)
            if status != "OK":
                continue
            _, validity = imap.response("UIDVALIDITY")
            uid_validity = (validity[0] or b"0").decode("ascii", "replace")
            status, found = imap.uid("SEARCH", None, "SINCE", _imap_date(since_epoch))
            if status != "OK" or not found or not found[0]:
                continue
            slug = re.sub(r"[^a-z0-9]+", "-", folder.lower()).strip("-") or "folder"
            uids = [uid.decode("ascii") for uid in found[0].split()]
            names = (
                _gmail_ids(imap, uids)
                if gmail
                else {uid: "%s.%s.%s" % (slug, uid_validity, uid) for uid in uids}
            )
            for uid in uids:
                if added >= limit:
                    break
                message_id = names.get(uid)
                if message_id is None or maildir.has(message_id):
                    continue
                status, parts = imap.uid("FETCH", uid, "(BODY.PEEK[])")
                if status != "OK":
                    continue
                raw = next((p[1] for p in parts if isinstance(p, tuple) and len(p) == 2), None)
                if raw is None:
                    continue
                maildir.add(message_id, raw)
                added += 1
    finally:
        try:
            imap.logout()
        except Exception:  # noqa: BLE001 - leaving is best effort
            pass
    return added


# ---- one pass -------------------------------------------------------------------------


def run_once(config_path, maildir_root, state_dir, now=None, opener=None, connect=None,
             sleep=time.sleep):
    """One pass over every account. One account failing never stops the others, and what is
    reported is a kind of failure, never a message's content."""
    now = time.time() if now is None else now
    opener = opener or urllib.request.urlopen
    connect = connect or (lambda host: imaplib.IMAP4_SSL(host, IMAP_PORT))
    report = {}
    for account in load_accounts(config_path):
        name = account["name"]
        days, state = window_days(read_state(state_dir, name), now)
        since = now - days * 24 * 60 * 60
        maildir = Maildir(maildir_root, name)
        try:
            if account["kind"] == "gmail":
                added = sync_gmail(account, maildir, since, opener, sleep=sleep)
            else:
                added = sync_imap(
                    account, maildir, since, connect, gmail=account["kind"] == "gmail_imap"
                )
            write_state(state_dir, name, state)
            report[name] = "%d new, window %d days" % (added, days)
        except Exception as error:  # noqa: BLE001 - one account must not stop the rest
            # The kind of failure and, for HTTP, its status: never the error's text, which
            # can quote a request or a reply.
            code = getattr(error, "code", None)
            report[name] = "failed (%s%s)" % (
                type(error).__name__, " %d" % code if isinstance(code, int) else ""
            )
    return report


def main(argv):
    config = os.environ.get("MAIL_MIRROR_CONFIG", "/config/accounts.json")
    maildir = os.environ.get("MAIL_MIRROR_MAILDIR", "/maildir")
    state = os.environ.get("MAIL_MIRROR_STATE", "/state")
    every = int(os.environ.get("MAIL_MIRROR_EVERY_SECONDS", "300"))
    os.umask(0o027)
    while True:
        try:
            report = run_once(config, maildir, state)
            print("mail mirror: " + "; ".join("%s: %s" % kv for kv in sorted(report.items())))
        except ConfigError as error:
            print("mail mirror: configuration problem: %s" % error)
        sys.stdout.flush()
        if "--once" in argv:
            return 0
        time.sleep(every)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
