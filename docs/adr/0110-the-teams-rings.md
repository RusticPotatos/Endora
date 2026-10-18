# 0110 — The team's rings: a sandbox on the Pi, the team gateway, the egress proxy, and undo

## Status

Accepted, 2026-10-17, by the owner, and revised by the owner the same day. Prepared with AI
assistance (Claude). Amends [0102](0102-endora-hq.md) (the Pi becomes a runner, reaching HQ
through the node), [0106](0106-team-contracts.md) and [0108](0108-the-bridge.md) (the contracts
and the bridge stay the team's only way to the house, now from the Pi too).

Amended 2026-10-17 by [0115](0115-the-engineering-wing-and-the-git-flow.md): the first piece of
ring 2, the lab, is the **check job**: the workbench image run model-free on the Pi's runner,
with a job token alone, to run the fork's own checks on a branch the wing submitted. Code
reaches the sandbox as a bundle from the node's mirror of the fork (`repo_checkout`) and leaves
it as a bundle the node takes by its rules (`repo_submit`); the sandbox still holds no key and
no remote, and the owner alone pushes.

Amended 2026-10-18 by [0117](0117-claude-code-as-a-developer-runtime.md): the workbench image
carries Claude Code (one pinned release, its settings read-only, no credential), and a
developer's turn in it may be Claude Code's, started by the worker, reaching Claude only
through the node's relay with the turn's token. The sandbox stays offline: with egress gone, a
whole Claude Code turn was seen to reach nothing but the gateway's relay, HQ and the node.

**Ring 1 is the sandbox on the Pi, with no internet: a turn's workbench reaches the node's team
gateway and nothing else.** Built: the team gateway on the node's team listener, published on
the house's network on 8788 (`/team-gateway/mcp` and `/team-gateway/runner/...`, passed on to
HQ); the runner reaching HQ through it and starting each turn as a workbench on the Pi's
`endora_sandbox` network. The Pi's Ansible role (Docker, the locked network, the host firewall)
and the workbench image (`make deploy-pi`) are built alongside this record. The **egress proxy
is designed and deferred**, below, with the research behind it, so it can be picked up as it
stands. Rings 2 and 3 and undo are not built.

## Context

The team's members think and act through HQ's workers. On the NAS a worker is a small
container on a fenced network that reaches HQ and the node and nothing else, and does nothing
but talk: the team protocol, the model relays, the contracts and the bridge. To do real work,
a member needs a shell, git and room to try things, and that should not happen on the NAS,
which holds the owner's data and every service of the house.

A Raspberry Pi 5 (8 GB, arm64, running from its SD card) is spare. HQ is not published, and
must not be: it answers the node. The node's team listener already serves the workers, and
everything on it needs a token.

What can go wrong is the same in every agent system: an agent with a key can leak it, an agent
with the internet can send data anywhere, and an agent that changes something can break it.
The owner asked for the research before deciding, and decided layer by layer.

## Decision

**Owner decisions, 2026-10-17.**

### The rings

1. **The sandbox** (the Pi): a fresh container for every turn, with a shell, git and a
   writable `/work` in memory, destroyed when the turn ends. Reading is free; nothing it does
   can touch the house except through Endora.
2. **The lab** (the Pi, later): long-lived test containers a member can build and try things
   in, before anything reaches the house.
3. **Production** (the NAS, later): changes only through the contracts and the bridge, with
   evidence before and undo after.

Agents never hold keys. Every request is logged. Endora is the gatekeeper.

### The Pi

`endora-dev`, the Pi, runs Docker and the runner, and each turn's sandbox. Its images are
built for linux/arm64 on the Mac, never on the Pi or the NAS, and copied over SSH
(`make deploy-pi`), as the NAS's are. Docker's logs are capped and the daemon keeps containers
across its own restarts; writes are kept low, for the SD card.

The sandbox network `endora_sandbox` is locked by the existing `docker_egress` role: no DNS, no
public ports, and of the house's network only the NAS's team gateway (port 8788). The host
firewall denies everything incoming but SSH from the house's network. The NAS's address is
configuration (`ENDORA_NAS_ADDR`, never in git).

Found on the first live run (2026-10-17): Docker's own resolver answers a container on a
user-defined network and forwards its lookups through the host, past the network's egress
rules, so a lookup could carry data out. Each sandbox turn's DNS server is therefore an
address that never answers (`192.0.2.1`, TEST-NET-1): no name resolves, and the workbench
reaches the gateway by address. Tested on the Pi: lookups blocked, gateway reached, the NAS's
other ports, the internet and the Pi itself refused.

### The team gateway

The node's team listener (port 8788) is published on the house's network. It is safe to publish
because everything on it needs a token: a running turn's, which HQ checks, or the runner's. To
the model relays, the contracts and the bridge it already served, it adds two pass-throughs to
HQ, which stays unpublished:

- `/team-gateway/mcp` to HQ's `/v1/team/mcp` (a worker, with its turn's token);
- `/team-gateway/runner/<route>` to HQ's `/v1/runner/<route>` (the runner, with its token).

The node passes the method (GET or POST), the body and the caller's own `Authorization` on
untouched, never adds its own token, and sends nowhere but HQ's configured URL: a runner route
is plain segments only, a request is capped at 4 MB and an answer at 8 MB, HQ has ten seconds,
and a redirect from HQ is passed back, not followed. HQ decides who is let in, as before.

A runner whose `HQ_URL` is the gateway (`http://<nas>:8788/team-gateway`) is the Pi's: it asks
for turns at the gateway's `/runner/...`, and starts each as a workbench (`ENDORA_WORKBENCH=1`,
the `endora-workbench` image, `/work` in memory owned by the image's user, a gigabyte, two CPUs
and 256 processes), with the team protocol and both model relays reached through the gateway.
The NAS's runner is unchanged.

### Four layers against leaks, as researched

1. **No keys in the sandbox; injection instead.** A secret the agent never holds cannot leak.
   Where a member needs a service, Endora holds the key and adds it on the way out; the
   sandbox sees only the result. The house's services (Sonarr, Radarr, Portainer, Home
   Assistant) are reached only through the contracts and the bridge (0106, 0108), never with a
   key and never directly.
2. **Reading is free; writing only to known services, through one exit.** Fetching docs,
   packages and code is most of what a member needs and leaks almost nothing; sending data out
   is where leaks happen. So one exit, the egress proxy, and write methods (POST, PUT, PATCH,
   DELETE) only to known destinations.
3. **Exact-match blocking, with variants.** The node knows its real secrets and the owner's
   personal values (`ENDORA_PERSONAL_VALUES`). Matching those exact strings, and their
   encodings (base64 and base64url at three offsets, hex in both cases, URL-encoded,
   JSON-escaped, reversed, and fragments of twelve characters or more), across the URL, the
   headers and the decompressed body, with a sliding buffer per destination so a value split
   across requests is still seen, is fast (Aho-Corasick, one pass) and has almost no false
   positives. A match blocks and alerts.
4. **Canaries.** Fake credentials, realistic in shape, planted in every sandbox (in the
   environment and a fake `.env`). Nothing legitimate ever uses them, so any appearance outside
   is a certain signal: block and alert at once.

**Presidio and other machine-learning detection of personal data are skipped.** They are slow
(a model per request on the path of every call), and inaccurate both ways: names and addresses
look like ordinary text, so they miss what matters and flag what does not, and a block that is
often wrong gets turned off. Exact matching of what is actually secret, plus canaries, does the
job they would be bought for.

**Scanners catch accidents; narrow exits and keeping data away catch intent.** A scanner finds
the secret an agent pasted by mistake. An agent set on leaking can encode, split or paraphrase
past any scanner, so what stops it is that it never had the data (layer 1, and the contracts
handing out only what a role may see), and that there is nowhere to send it (layer 2, the
sandbox's locked network). The scanners are kept for accidents and as tripwires, never relied
on as the boundary.

### The egress proxy: designed and deferred

The owner tabled the proxy the same day: ring 1 starts with **no internet at all**, so the
sandbox's network allows only the gateway, and the proxy, port 3128, the proxy settings in the
sandbox and the canaries wait until it is picked up. As designed, in two steps:

- **First, an allowlist, no decryption.** An HTTP forward proxy inside the node (another
  listener, port 3128), authenticated with `Proxy-Authorization: Basic` as `turn:<run token>`
  and checked with HQ (`/v1/team/turn`) as the relays are: no token, no service. HTTPS goes
  through `CONNECT` tunnels only to hosts on the team's allowlist (a host or `*.domain`, port
  443); plain HTTP only to allowlisted hosts, scanned (layer 3, with the turn's canaries). A
  canary used as a host name is caught too. It holds no credentials and never reaches the
  house's network. A refused destination is a 403 and a card on the owner's desk, "Allow
  <host> for the team?": Approve adds it to the allowlist, Decline remembers the refusal, one
  card per host while it waits. The allowlist starts with code hosts, package and container
  registries and the house's applications' documentation, and the owner edits it. Every
  request is in Endora's audit (method or `CONNECT`, host, path, size, verdict, member, run).
- **Later, if wanted, inspection.** TLS interception with Endora's own team CA, generated once
  in the node's data folder and trusted by the sandbox alone, so the proxy can read HTTPS too:
  write methods only to known services, credential injection for configured services (layer
  1's injection, for services outside the house such as GitHub), and layer 3's scan on every
  body. The CA's public certificate would be served on the gateway for the sandboxes to trust.

Allowlist first because it needs no trust in a CA, breaks no pinned client, and already closes
most of the ways out; inspection only if the team needs writes to the internet.

### Undo (later rings)

Before a change in production: a copy of the service's config folder, the container's previous
config and image recorded, git for config files (commits never pushed, approved by the lead and
the reviewer), and btrfs snapshots for the media and archive volumes.

### Rollout

In steps, each after the owner has seen the last one work: ring 1 first (the Pi's sandbox,
offline, reaching only the gateway). The egress proxy when it is picked up, allowlist first.
The lab only after the owner has watched the proxy's log for a while. Production changes last,
with undo.

## Consequences

- A member's turn can run a shell and git on the Pi, offline, and still reach the house only
  through the contracts and the bridge, under the same policy and the same desk as on the NAS.
- 8788 is reachable from the house's network. Every route on it needs a token HQ issues, and a
  turn's token opens only its own turn while it runs. HQ itself stays unpublished.
- Until the proxy is picked up, a member cannot fetch packages or documentation from the
  sandbox; it works with what the workbench image holds.
- The NAS's runner and its workers are unchanged.

## Not done yet

- The egress proxy (allowlist, desk cards, audit), the canaries, and later inspection with the
  team CA and credential injection.
- Ring 2 (the lab) beyond its first piece, the check job (0115), and ring 3 (production
  changes with undo).
- Pairing a runner from HQ (0102's phase 3); the Pi's runner is configured by its role.

## Alternatives considered

- **Publishing HQ.** Rejected: HQ answers the node only; one listener that already requires a
  token for everything is a smaller surface than a second one.
- **Machine-learning detection of personal data (Presidio and the like).** Rejected: slow and
  inaccurate, above.
- **Scanners as the boundary.** Rejected: they catch accidents, not intent.
- **Starting with the proxy.** Deferred by the owner: an offline sandbox first, with nothing to
  get wrong, and the proxy when the team needs the internet.
