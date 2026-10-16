#!/usr/bin/env python3
"""Gets a read-only Gmail token for the mail mirror, once per account, on your own computer.

    python3 gmail_consent.py CLIENT_ID CLIENT_SECRET

It opens Google's consent page in your browser, asking for one permission only:
`gmail.readonly` (read your mail; nothing else). After you agree, Google sends the browser
back to this script on 127.0.0.1, and it writes the refresh token to a file only you can read.
Put that token in your Ansible vault and delete the file.

Nothing here is sent anywhere except to Google. Standard library only.
"""
import base64
import hashlib
import http.server
import json
import os
import secrets
import sys
import urllib.parse
import urllib.request
import webbrowser

AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth"
TOKEN_URL = "https://oauth2.googleapis.com/token"
SCOPE = "https://www.googleapis.com/auth/gmail.readonly"


def consent_url(client_id, redirect_uri, state, challenge):
    """The consent page: read-only scope, offline access so a refresh token is issued, and
    `prompt=consent` so one is issued even if this account agreed before."""
    return AUTH_URL + "?" + urllib.parse.urlencode(
        {
            "client_id": client_id,
            "redirect_uri": redirect_uri,
            "response_type": "code",
            "scope": SCOPE,
            "access_type": "offline",
            "prompt": "consent",
            "state": state,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        }
    )


def pkce():
    verifier = secrets.token_urlsafe(64)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip("=")
    return verifier, challenge


def code_from(path, state):
    """The authorization code from Google's redirect, if it is the reply to *this* request."""
    query = urllib.parse.parse_qs(urllib.parse.urlparse(path).query)
    if query.get("state", [""])[0] != state:
        raise ValueError("the reply was not for this request")
    if "error" in query:
        raise ValueError("Google said: " + query["error"][0])
    return query["code"][0]


def exchange(client_id, client_secret, code, verifier, redirect_uri, opener=urllib.request.urlopen):
    body = urllib.parse.urlencode(
        {
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "code_verifier": verifier,
            "grant_type": "authorization_code",
            "redirect_uri": redirect_uri,
        }
    ).encode()
    with opener(urllib.request.Request(TOKEN_URL, data=body, method="POST"), timeout=60) as response:
        reply = json.loads(response.read().decode())
    if reply.get("scope") != SCOPE:
        # Google lets the person untick a permission; anything other than exactly read-only
        # is refused here rather than stored.
        raise ValueError("the token is not read-only (scope: %s)" % reply.get("scope"))
    return reply["refresh_token"]


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    client_id, client_secret = argv
    state = secrets.token_urlsafe(24)
    verifier, challenge = pkce()
    received = {}

    class Reply(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802 - the http.server name
            try:
                received["code"] = code_from(self.path, state)
                message = "Done. You can close this tab and go back to the terminal."
            except (ValueError, KeyError) as error:
                received["error"] = str(error)
                message = "That did not work: %s" % error
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(message.encode())

        def log_message(self, *args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), Reply)
    redirect_uri = "http://127.0.0.1:%d/" % server.server_port
    url = consent_url(client_id, redirect_uri, state, challenge)
    print("Opening your browser. If it does not open, visit:\n\n" + url + "\n")
    webbrowser.open(url)
    while "code" not in received and "error" not in received:
        server.handle_request()
    if "error" in received:
        print("Stopped: " + received["error"])
        return 1
    token = exchange(client_id, client_secret, received["code"], verifier, redirect_uri)
    path = os.path.abspath("gmail-refresh-token.txt")
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as handle:
        handle.write(token + "\n")
    print("Read-only token written to %s (only you can read it)." % path)
    print("Put it in your Ansible vault, then delete the file.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
