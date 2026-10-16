# Hosting Endora: always-on and reachable

Endora is **local-first and single-user**. The node holds all state and
authority; clients are thin and replaceable. This guide covers running the node
**always-on** (so it is there when you want to capture a thought) and reaching it
**securely** from your other devices — including a phone browser — without
exposing it to the open internet.

> The node authenticates `/v1` requests using bearer tokens. Normal sign-in uses
> a password and authenticator code and creates a 30-day session. Initial enrollment
> requires the bootstrap token held on the node. Keep the service on a trusted
> network and protect transport with HTTPS or a private encrypted overlay.
> Authentication is not transport encryption. See [SECURITY.md](../SECURITY.md).

## 1. Where to run it

Any always-on machine you control works: a home server or mini-PC, a Raspberry
Pi, or a small VPS. The node is a single static binary plus a SQLite file, so it
is light. Two ways to run it:

- **The binary** — `cargo build --release -p endora-node`, then run
  `endora-node` with `ENDORA_ADDR` and `ENDORA_DB` set (below).
- **The container** — `make docker-build` produces the `endora-node` image; it
  keeps the database on a `/data` volume. See [README](../README.md#running-the-node).

Your data lives in one SQLite file (`ENDORA_DB`, default `endora.db`; `/data/endora.db`
in the container) — your settings and credentials as well as its memory. How to back it
up properly, and where to keep it so a backup tool can reach it, is
[below](#backing-up-your-data). `endora export` is **not** that backup.

### Building somewhere other than the box that runs it

By default `make deploy` builds the image on the machine that runs it. On a small always-on
box that machine is also running your house: a release build there once ran a 16 GB NAS out
of memory and swap, and Home Assistant stopped answering. To build on your laptop instead,
set a Docker context on the laptop in the git-ignored `local.mk`:

```make
DOCKER_CONTEXT = nas                    # where it runs
ENDORA_BUILD_CONTEXT = desktop-linux    # where it is built
```

The image is then built on the laptop for the target's architecture (cross-compiled, not
emulated: an arm64 Mac builds an x86_64 NAS image natively), copied over, and started without
building. The Rust used is the one in the build image (the workspace MSRV), never a download.

### Backing up your data

**What is in `/data`.** `endora.db` holds everything: your settings, API keys and MCP
tokens, sign-in secrets, and the node's memory. `tls-cert.pem` and `tls-key.pem` are the
self-signed certificate (regenerated if lost; your browser asks you to accept it again).

**`endora export` is not a backup.** It writes the node's *memory* — messages, beliefs,
preferences, goals, audit — as your right to take it with you ([constitution](constitution.md)).
It contains none of your configuration, and nothing can import it, so a node rebuilt from an
export has lost every setting and key.

**Take a snapshot, don't copy the file.** The node writes continuously, so copying
`endora.db` while it runs can capture a write half-finished: a file that looks whole and
will not open on the day you need it. Take the copy through SQLite instead:

```bash
make backup                       # the container; respects DOCKER_CONTEXT
endora-node backup /safe/place/snapshot.db   # the binary, with ENDORA_DB set
```

It writes a consistent, integrity-checked copy that is readable only by its owner, and
refuses to overwrite an earlier one. In the container it lands in `/data/backups/`, so a
backup job that covers the data folder picks it up; old snapshots are not pruned, which is
that job's business. Run it on a schedule just before the backup runs (on a NAS, a task
that calls `docker exec endora-node endora-node backup /data/backups/<name>.db`). Copying
the file by hand is only safe with the node stopped.

**Keep the data where a backup tool can reach it.** In the container the data is a Docker
*named volume* by default, which sits inside Docker's own storage. To keep it in a folder
instead, set an **absolute path on the machine Docker runs on** in the git-ignored
`local.mk` (on a NAS, a path on the NAS):

```make
ENDORA_DATA_DIR = /srv/endora
```

and move what you have there once — in this order:

```bash
make deploy-down       # stop the node: its database must not change mid-copy
make migrate-data      # copy the volume into that folder, verified file by file; deletes nothing
make deploy            # now starts on the folder
```

Do the migration *before* deploying. If `ENDORA_DATA_DIR` is set but empty, the node would
start on nothing: a brand-new node with a new token and none of your settings, which looks
exactly like data loss. `make deploy` refuses that case and tells you to migrate. The old
volume is left in place as the way back; remove it (`docker volume rm endora_endora-data`)
only once you have checked the node is right. Do not unset `ENDORA_DATA_DIR` afterwards —
the node would go back to that now-stale volume.

The folder must be on a **local disk** of that machine, not an NFS or SMB share: SQLite's file
locking is not reliable over a network filesystem. It is created root-owned, since the
container runs as root.

**Treat the data folder and every snapshot as secrets.** They hold API keys, MCP bearer
tokens, sign-in and authenticator secrets, and the TLS private key. Keep backups on storage
you control and, if it leaves the machine, encrypted. Never commit them.

**Restoring.** Stop the node, put the snapshot at `<data folder>/endora.db` (delete any
`endora.db-journal` beside it), and start the node.

## 2. Keep it running (always-on)

### systemd (running the binary on Linux)

```ini
# /etc/systemd/system/endora.service
[Unit]
Description=Endora node
After=network-online.target
Wants=network-online.target

[Service]
# Bind to loopback only; reach it over a private network (section 3).
Environment=ENDORA_ADDR=127.0.0.1:8787
Environment=ENDORA_DB=/var/lib/endora/endora.db
ExecStart=/usr/local/bin/endora-node
Restart=on-failure
# Minimal hardening; the node needs only its data directory.
DynamicUser=yes
StateDirectory=endora
ReadWritePaths=/var/lib/endora

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now endora
```

### Docker (restart on boot)

Add a restart policy and bind the published port to loopback:

```bash
docker run -d --name endora --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -v /var/lib/endora:/data \
  endora-node
```

The image binds `0.0.0.0` **inside** the container so the mapped port works; the
`127.0.0.1:` in `-p` is what keeps it off your machine's other interfaces. Only
change that once a private overlay or an authenticating proxy sits in front.

### Optional: let it look into things on its own

Set **`ENDORA_OWN_WORK=1`** on the node and it will, while you are not talking to it,
look into things one at a time and write what it found to the activity trail. It is off
by default. In this order, and only when the one before has nothing:

1. Something in the house that has not answered for three days or more (a sensor, a
   device). It reads only Home Assistant for this, not the web or your other skills.
2. A question it once failed to answer, asked again a day after it failed. Judged by
   the same check that filed it; it gives up after the same number of tries as before.
3. Something it understands about you with confidence, looked into with the same
   read-only skills the nightly loop's long-running thread used, **including the web**.
   This is the one thing the lane does that sends a query out of the house, built from
   a belief about you. Leave the switch off if you do not want that.

It reads and looks; it cannot change anything. It uses the local model only (never the
paid one), waits until you have been quiet for ten minutes, never takes the place of
something you asked for, and posts nothing to chat. Pressing Stop on one, or marking a
device "it's fine", means that item is not looked into again. With the switch on, the
nightly loop stops replaying a question and keeping its own thread, since the lane does
both; with it off, the nightly loop is exactly as it was.
With the switch on, and **`ENDORA_MAIL_DIR`** set in `local.mk` to the read-only mail copy the
ansible-nas `mail_mirror` role keeps (by default `/srv/mail-mirror/maildir` on the NAS), it
also reads your mail in quiet time, one message at a time, with the local model only, and
shows what matters in the trail, your brief and the console's **Mail** screen (ADR 0097).

With the switch on it also reads the house's own log of what changed, and writes a line to
the activity trail when something happens at about the same time on most days ("the front
door goes on around 07:30, on 11 of the last 14 days"). That needs no model and sends
nothing anywhere. Set **`ENDORA_UTC_OFFSET_MINUTES`** (for example `-300` for US Eastern
standard time, `60` for central Europe in winter) so the times are yours; unset, they are UTC.
See [ADR 0094](adr/0094-work-it-finds-for-itself.md),
[ADR 0095](adr/0095-more-sources-for-work-it-finds.md) and
[ADR 0096](adr/0096-patterns-it-notices.md).

## 3. Reach it securely from your phone and laptop

The goal — "mobile capture with no sync" — is to open the console from your phone
and jot an observation, while the node stays private. Pick one:

### Recommended: a private overlay network (Tailscale / WireGuard)

Put the node's machine and your devices on the same private network, then reach
the node over that network only. Nothing is published to the public internet, and
the node's own sign-in remains required over the overlay.

1. Install Tailscale (or set up WireGuard) on the node's machine and on your
   phone/laptop; sign them into the same tailnet.
2. Keep the node bound to loopback, or bind it to the overlay interface, e.g.
   `ENDORA_ADDR=100.x.y.z:8787` (your Tailscale IP) — **not** `0.0.0.0`.
3. On your phone (connected to the tailnet), open
   `http://<node-tailscale-name>:8787`. The console loads, and its live updates
   (server-sent events, [ADR 0050](adr/0050-the-shape-of-the-system.md))
   work over the overlay just as they do locally.

This is the simplest secure setup and the one we recommend for personal use.

### Alternative: an authenticating reverse proxy (for wider exposure)

If you must expose the node beyond a private network, **do not** publish its port
directly. Put a reverse proxy in front that terminates TLS
**and** requires authentication, and let only the proxy reach the node
(bind the node to loopback). Example with Caddy and HTTP basic auth:

```caddyfile
endora.example.com {
    # Generate the hash with: caddy hash-password
    basic_auth {
        you JDJhJDE0J...   # bcrypt hash, not a plaintext password
    }
    reverse_proxy 127.0.0.1:8787
}
```

For more than one user or real accounts, front it with an identity-aware proxy
(e.g. an OAuth2 proxy) instead of basic auth. Whatever you choose, the invariant
is the same: **the node is only reachable through something that authenticates.**

### Never do this

- Publishing the raw node port to the public internet (`-p 8787:8787` on a public
  host, or `ENDORA_ADDR=0.0.0.0:8787` reachable from outside) with no proxy. That
  hands full read/write of your data to anyone who finds it.

## 4. Voice: it can speak, but speaking *to* it needs HTTPS

The web console can read the butler's replies aloud (text-to-speech) and let you
speak your message (speech-to-text), using the browser's built-in Web Speech API —
open **💬 Chat**, toggle **🔊 Speak**, and use the **🎤** button.

One browser rule to know: **the microphone only works on a secure page** — HTTPS or
`localhost`. So:

- **Text-to-speech (it speaks to you)** works everywhere, including plain
  `http://<host>:8787`.
- **Speech-to-text (you speak to it)** is **blocked** on a plain-HTTP LAN address.
  Give the console an HTTPS origin to enable the mic:
  - **Built-in self-signed HTTPS (easiest — no domain, proxy, or extra tools):**
    set **`ENDORA_TLS=1`** on the node (the compose does this by default). It
    serves HTTPS with a self-signed cert persisted in `/data`, so you accept the
    browser's "not private" warning **once** per browser, then the mic works. Set
    `ENDORA_TLS_SAN` to this host's LAN IP/hostname for a cleaner cert (in
    `docker-compose.override.yml`). Access `https://<host>:8787`.
  - **Tailscale:** `tailscale serve https / http://127.0.0.1:8787` gives a *trusted*
    HTTPS URL on your tailnet — no warning at all, and it's private. Best if you
    already run Tailscale (§3).
  - **A TLS reverse proxy** (Caddy/nginx, §3) in front of the node.
  - **Quick local test:** in Chrome, `chrome://flags/#unsafely-treat-insecure-origin-as-secure`
    → add `http://<host>:8787` → relaunch. (Dev only; not a real fix.)

  A fully **local** speech path (a Whisper model on your host, so audio never
  leaves the machine) is the privacy-first end state and is the next voice step.

Speech recognition also needs a supporting browser (Chrome/Edge) and, in some
browsers, streams audio to a cloud service — so voice is opt-in, off by default.

## 5. Checklist

- [ ] Node bound to `127.0.0.1` (or a private-overlay IP), never public `0.0.0.0`.
- [ ] Reached only over a private overlay, or through an authenticating TLS proxy.
- [ ] Runs under a supervisor (systemd / `--restart`) so it survives reboots.
- [ ] `ENDORA_DB` on persistent storage a backup tool can reach, and snapshots taken with
      `make backup` / `endora-node backup`. `endora export` holds memory only, none of your
      settings — it is not a backup (see [Backing up your data](#backing-up-your-data)).
- [ ] Reviewed [SECURITY.md](../SECURITY.md) — you understand the API is
      bearer-authenticated, with public console assets and health checks.
