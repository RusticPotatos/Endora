#!/usr/bin/env python3
"""The house watch: checks the NAS and its services from outside it, once a minute, and says
on your phone (an ntfy topic) when one has been down past the grace, and when it is back.

Run by endora-watch.timer as a oneshot. The settings come as a systemd credential
($CREDENTIALS_DIRECTORY/watch.json), or as the first argument when run by hand; the state
(what is failing since when, and what could not be sent yet) lives in $RUNTIME_DIRECTORY, in
memory, so a reboot of this Pi starts afresh and says so. Standard library only.
"""
import concurrent.futures
import json
import os
import random
import socket
import ssl
import struct
import sys
import time
import urllib.request

KINDS = ("tcp", "https", "dns")
TIMEOUT = 5
OUTBOX_LIMIT = 20


# The settings

def check_config(config):
    """Refuse settings the watch cannot act on, loudly, before anything is checked."""
    url = config.get("alert_url") or ""
    if not url.startswith("https://"):
        raise ValueError("alert_url must be an https:// address (the ntfy topic)")
    checks = config.get("checks") or []
    if not checks:
        raise ValueError("no checks: nothing to watch")
    names = set()
    for check in checks:
        name = check.get("name")
        if not name or name in names:
            raise ValueError("every check needs a name of its own: %r" % name)
        names.add(name)
        kinds = [kind for kind in KINDS if kind in check]
        if len(kinds) != 1:
            raise ValueError("%s: give exactly one of %s" % (name, ", ".join(KINDS)))
        if "tcp" in check:
            host, _, port = check["tcp"].rpartition(":")
            if not host or not port.isdigit():
                raise ValueError("%s: tcp is <address>:<port>" % name)
        if "dns" in check:
            for key in ("server", "name", "expect"):
                if not check["dns"].get(key):
                    raise ValueError("%s: dns needs %s" % (name, key))
        if "https" in check:
            for key in ("address", "host"):
                if not check["https"].get(key):
                    raise ValueError("%s: https needs %s" % (name, key))
    return config


def load_config(path):
    with open(path) as handle:
        return check_config(json.load(handle))


# The checks

def probe(check, timeout=TIMEOUT):
    """(up, what was seen) for one check. Never raises."""
    try:
        if "tcp" in check:
            host, _, port = check["tcp"].rpartition(":")
            socket.create_connection((host, int(port)), timeout).close()
            return True, "open"
        if "dns" in check:
            return probe_dns(check["dns"], timeout)
        return probe_https(check["https"], timeout)
    except (OSError, ValueError) as error:
        return False, describe(error)


def describe(error):
    if isinstance(error, socket.timeout):
        return "timed out"
    if isinstance(error, ConnectionRefusedError):
        return "refused"
    return str(error) or type(error).__name__


def probe_dns(dns, timeout):
    qid = random.randrange(0x10000)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(timeout)
        sock.sendto(dns_query(dns["name"], qid), (dns["server"], int(dns.get("port", 53))))
        packet = sock.recv(4096)
    addresses = dns_answers(packet, qid)
    if dns["expect"] in addresses:
        return True, "answers"
    return False, "%s is %s, not %s" % (dns["name"], ", ".join(addresses) or "nothing",
                                        dns["expect"])


def dns_query(name, qid):
    """A recursive query for the A record of `name`."""
    question = b"".join(bytes([len(label)]) + label.encode("ascii")
                        for label in name.rstrip(".").split("."))
    return struct.pack(">HHHHHH", qid, 0x0100, 1, 0, 0, 0) + question + b"\x00\x00\x01\x00\x01"


def dns_answers(packet, qid):
    """The IPv4 addresses in an answer to query `qid`."""
    got, flags, questions, answers = struct.unpack(">HHHH", packet[:8])
    if got != qid:
        raise ValueError("an answer to another query")
    if flags & 0x000F:
        raise ValueError("DNS error %d" % (flags & 0x000F))
    offset = 12
    for _ in range(questions):
        offset = skip_name(packet, offset) + 4
    addresses = []
    for _ in range(answers):
        offset = skip_name(packet, offset)
        rtype, _rclass, _ttl, length = struct.unpack(">HHIH", packet[offset:offset + 10])
        offset += 10
        if rtype == 1 and length == 4:
            addresses.append(socket.inet_ntoa(packet[offset:offset + 4]))
        offset += length
    return addresses


def skip_name(packet, offset):
    while True:
        length = packet[offset]
        if length == 0:
            return offset + 1
        if length & 0xC0 == 0xC0:
            return offset + 2
        offset += length + 1


def tls_context(cafile):
    """The system's trust, or the house's own CA alone. Python 3.13 adds the X509_STRICT profile
    by default, which refuses a home-made CA without an Authority Key Identifier; for the house's
    CA it is left off, and the chain and the name are still checked."""
    context = ssl.create_default_context(cafile=cafile)
    if cafile:
        context.verify_flags &= ~getattr(ssl, "VERIFY_X509_STRICT", 0)
    return context


def probe_https(https, timeout):
    """GET a path from an address, checking the certificate against the name it should carry
    (so the check needs no DNS), with the house's own CA when one is given."""
    context = tls_context(https.get("ca") or None)
    raw = socket.create_connection((https["address"], int(https.get("port", 443))), timeout)
    with context.wrap_socket(raw, server_hostname=https["host"]) as sock:
        request = "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n" % (
            https.get("path", "/"), https["host"])
        sock.sendall(request.encode("ascii"))
        status_line = sock.recv(256).split(b"\r\n", 1)[0].decode("ascii", "replace")
    parts = status_line.split()
    if len(parts) >= 2 and parts[1].isdigit() and 200 <= int(parts[1]) < 400:
        return True, parts[1]
    return False, status_line or "no answer"


# What changed

def advance(state, results, now, grace):
    """The new state, and which checks went down past the grace or came back, from this run's
    results ({name: (up, detail)}). A check no longer listed is forgotten."""
    new, down, back = {}, [], []
    for name, (up, detail) in results.items():
        seen = dict(state.get(name) or {})
        if up:
            if seen.get("alerted"):
                back.append({"name": name, "since": seen["failing_since"]})
            new[name] = {}
            continue
        if seen.get("failing_since") is None:
            seen["failing_since"] = now
        if not seen.get("alerted") and now - seen["failing_since"] >= grace:
            seen["alerted"] = True
            down.append({"name": name, "since": seen["failing_since"], "detail": detail})
        new[name] = seen
    return new, down, back


def clock(at):
    return time.strftime("%H:%M", time.localtime(at))


def notification(down, back, now):
    """One message for a run's changes, or None. Plain ASCII: it goes in HTTP headers."""
    if not down and not back:
        return None
    lines = ["%s: %s (since %s)" % (d["name"], d["detail"], clock(d["since"])) for d in down]
    lines += ["%s: back (down since %s)" % (b["name"], clock(b["since"])) for b in back]
    if down:
        names = [d["name"] for d in down]
        title = "%d down: %s" % (len(names), ", ".join(names))
        return note(title, "\n".join(lines), "high", "warning")
    return note("Back: %s" % ", ".join(b["name"] for b in back), "\n".join(lines), "default",
                "white_check_mark")


def started(count):
    return note("House watch started", "Watching %d checks from outside the NAS." % count,
                "low", "eyes")


def note(title, body, priority, tag):
    clean = lambda text: text.encode("ascii", "replace").decode("ascii")  # noqa: E731
    return {"title": clean(title), "body": clean(body), "priority": priority, "tags": tag}


# Saying it

def deliver(outbox, send):
    """Send in order, stopping at the first that fails; what is left waits for the next run.
    Only the newest OUTBOX_LIMIT are kept."""
    outbox = list(outbox)[-OUTBOX_LIMIT:]
    while outbox:
        try:
            send(outbox[0])
        except OSError:
            break
        outbox.pop(0)
    return outbox


def ntfy(url):
    def send(message):
        request = urllib.request.Request(url, data=message["body"].encode("ascii"), method="POST")
        request.add_header("Title", message["title"])
        request.add_header("Priority", message["priority"])
        request.add_header("Tags", message["tags"])
        urllib.request.urlopen(request, timeout=TIMEOUT).close()
    return send


# The state

def read_state(path):
    try:
        with open(path) as handle:
            return json.load(handle)
    except FileNotFoundError:
        return None


def write_state(path, state):
    temporary = path + ".new"
    with open(temporary, "w") as handle:
        json.dump(state, handle)
    os.replace(temporary, path)


def main(argv):
    if len(argv) == 3 and argv[1] == "--check":
        load_config(argv[2])
        return 0
    if len(argv) > 1:
        config_path = argv[1]
    else:
        config_path = os.path.join(os.environ["CREDENTIALS_DIRECTORY"], "watch.json")
    config = load_config(config_path)
    state_path = os.path.join(os.environ.get("RUNTIME_DIRECTORY", "/tmp"), "state.json")
    state = read_state(state_path)
    outbox = []
    if state is None:
        state = {"checks": {}, "outbox": []}
        outbox.append(started(len(config["checks"])))
    outbox = state.get("outbox", []) + outbox

    checks = config["checks"]
    with concurrent.futures.ThreadPoolExecutor(len(checks)) as pool:
        results = dict(zip([c["name"] for c in checks], pool.map(probe, checks)))
    now = int(time.time())
    grace = int(config.get("grace_minutes", 10)) * 60
    seen, down, back = advance(state.get("checks", {}), results, now, grace)
    message = notification(down, back, now)
    if message:
        outbox.append(message)
    for name, (up, detail) in sorted(results.items()):
        if not up:
            print("%s: %s" % (name, detail))
    outbox = deliver(outbox, ntfy(config["alert_url"]))
    if outbox:
        print("%d message(s) wait to be sent" % len(outbox))
    write_state(state_path, {"checks": seen, "outbox": outbox})

    heartbeat = config.get("heartbeat_url")
    if heartbeat:
        try:
            urllib.request.urlopen(heartbeat, timeout=TIMEOUT).close()
        except OSError as error:
            print("heartbeat: %s" % describe(error))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
