# Hosting Endora: always-on and reachable

Endora is **local-first and single-user**. The node holds all state and
authority; clients are thin and replaceable. This guide covers running the node
**always-on** (so it is there when you want to capture a thought) and reaching it
**securely** from your other devices — including a phone browser — without
exposing it to the open internet.

> **Read this first.** The `0.x` HTTP API is **unauthenticated** by design (see
> [SECURITY.md](../SECURITY.md) and [ADR 0050](adr/0050-the-shape-of-the-system.md)).
> Anything that can reach the node's port can read and write everything. So the
> whole security model here is **network reachability**: keep the node on a
> trusted network (loopback or a private overlay), and if you ever expose it more
> widely, put authentication in front of it. Authentication in the node itself is
> tracked as pre-1.0 work.

## 1. Where to run it

Any always-on machine you control works: a home server or mini-PC, a Raspberry
Pi, or a small VPS. The node is a single static binary plus a SQLite file, so it
is light. Two ways to run it:

- **The binary** — `cargo build --release -p endora-node`, then run
  `endora-node` with `ENDORA_ADDR` and `ENDORA_DB` set (below).
- **The container** — `make docker-build` produces the `endora-node` image; it
  keeps the database on a `/data` volume. See [README](../README.md#running-the-node).

Your data lives in one SQLite file (`ENDORA_DB`, default `endora.db`; `/data/endora.db`
in the container). Back it up by copying that file when the node is stopped, or
at any time with `endora export > backup.json` (the export is a complete,
portable snapshot — a memory right, see the [constitution](constitution.md)).

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

## 3. Reach it securely from your phone and laptop

The goal — "mobile capture with no sync" — is to open the console from your phone
and jot an observation, while the node stays private. Pick one:

### Recommended: a private overlay network (Tailscale / WireGuard)

Put the node's machine and your devices on the same private network, then reach
the node over that network only. Nothing is published to the public internet, and
because the network itself is the trust boundary, you do not need to add
authentication.

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
directly — the API has no auth. Put a reverse proxy in front that terminates TLS
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
- [ ] `ENDORA_DB` on persistent storage; periodic `endora export` backups.
- [ ] Reviewed [SECURITY.md](../SECURITY.md) — you understand the API is
      unauthenticated in `0.x`.
