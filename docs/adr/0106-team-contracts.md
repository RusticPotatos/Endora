# 0106 — Team contracts: what Endora offers HQ's workers

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0102](0102-endora-hq.md): a turn reaches the home lab only through these contracts. The first
slice is built: three contracts on the node's team listener (`homelab_status`,
`container_logs`, `restart_container`), the worker offering them beside the team's tools, the
owner's two switches on the HQ screen, and restarts told on the owner's desk. There are no
ask-first contracts yet; `request_action` stays as it was. (Amended 2026-10-19: `request_action`
was never built, and is no longer listed; a member asks the owner with `ask_owner`.)

Amended 2026-10-17 by [0108](0108-the-bridge.md): these three are now the **generic container
tools**, and beside them the **bridge** offers every capability Endora has, and every tool of
its tool servers, under Endora's own policy (tier, the owner's stance, third-party marks, a
profile by role). The bridge's acts are the first ask-first acts: an approval on the owner's
desk that runs exactly what it shows. The owner's "every contract" switch covers the bridge
too. The Confidential check point this record asks for is in place: HQ says when a member
thinks on a cloud model, and the node then neither offers nor runs a Confidential tool for that
turn (`container_logs` stays as it was until HQ says it).

Amended 2026-10-17 by [0109](0109-a-brain-per-member.md): the decision owed below is made. HQ
now says when a member thinks with Claude, and `container_logs` is listed and run for a cloud
turn only when it is the lead's, by the owner's decision; a cloud developer, reviewer or
researcher is refused it as Confidential.

Amended 2026-10-17 by [0110](0110-the-teams-rings.md): the contracts are the team's only way to
the house from the Pi's sandbox too, reached through the team gateway; no key and no direct
route to a home service enters a sandbox.

Amended 2026-10-17 by [0112](0112-the-team-knows-how-the-lab-is-built.md): two more reads,
offered to every role on any brain (Internal: names and wiring): `homelab_map`, how the lab is
built (who manages each container, what depends on what), through one more fixed read-only
route on the Docker observer; and `ansible_nas_doc`, the lab's ansible-nas docs by an
allowlist.

## Context

The owner gave the team the goal "Keep the home lab healthy", and the team could not see the
home lab: a worker's container reaches only HQ and the node's team listener, and holds no key.
That fence is the point of [0102](0102-endora-hq.md), and it stays. The owner decided against
both obvious ways round it: **no keys** handed to a turn, and **no raw access** (no shell, no
Docker socket, no Portainer API passed through).

## Decision

Endora offers the team **contracts**: narrow, declared abilities, each a tool with a name, a
description and an input schema, served by the node and decided by its deterministic policy.
A member's model proposes a call; the node decides it. The model is never the enforcement
boundary.

**Three kinds.**

- **Read**: looks and changes nothing. Free: no limit beyond the turn's own.
- **Automatic act**: a safe fix. It runs at once, within rate limits the node keeps, and every
  one is told to the owner as a notice on their desk in HQ.
- **Ask-first act**: anything riskier, which waits for the owner. None exists yet; until one
  does, a member asks the owner (`ask_owner`).

**The first contracts.**

| Contract | Kind | Tier | What it does, and its limits |
| --- | --- | --- | --- |
| `homelab_status` | read | Internal | The observer's snapshot, compact: each service's state and items; for each container its name, state, status text, health and exit code. Marked stale when unseen for a minute. No ids. |
| `container_logs` | read | Confidential | The newest 100 lines of one container's log, at most 16 KB, scrubbed of secrets. A name resolved through Portainer; an unknown name is refused, and so is Endora's own. Refused with a plain message when Portainer is not set up. |
| `restart_container` | automatic act | Internal | Restarts one container through the console's own container action (its allowlist, protection, journal and read-back), with a reason. Never Endora's own, and never what a restart will not fix (below). At most once a container an hour, and three an hour across the team. |

**The policy is code, in the node.**

- A call is opened by the turn's own token, checked with HQ exactly as the model relays check
  it (`/v1/team/turn`); a token that opens no running turn gets 401, before anything is read.
- **Endora's own containers are never the team's**: a name starting `endora`, what the
  console already protects (Portainer, cloudflared, the observer, any container of Endora's
  image, which covers every worker), and Endora's compose services by name or label (node,
  hq, runner, worker, docker-observer, sound, senders, spotify, runtime).
- **Restart limits are kept in the node's database** (`team_restarts`), so restarting the node
  gives none back. A restart is taken from the limits before it is tried, in one step, so a
  failed one counts and two turns cannot both take the last.
- **A restart that will not fix it is refused**, and costs no restart: Docker's inspection
  (state, restart count, out-of-memory flag, last start; never the configuration) says the
  container is already restarting, was killed for memory, or is crash-looping (three restarts
  or more by Docker, the last start within the hour). The member is told to put it on the
  owner's desk with `ask_owner` (to raise its memory, say). A live test of the local model
  restarted a container crash-looping out of memory; this rule is why it cannot again.
- **A restart never starts a stopped container** (added 2026-10-17). Portainer's restart
  starts a container that has exited, and a container may be stopped on purpose: an old sound
  server kept for its settings would take the ports the sound hub needs. Only a `running`
  container is restarted; any other state, or one that cannot be read, is refused before the
  limits are touched, with "it is stopped, perhaps on purpose: restart only fixes a running
  container; ask the owner with ask_owner if it should run".
- **A turn's claims sit beside what it actually did** (added 2026-10-17). A lead once posted
  that it had restarted two containers in a turn whose audit showed only reads. The worker
  keeps the calls its model made and how each came back (done, refused, failed), naming the
  tool and its main argument, and writes them under the turn's last word ("This turn did: read
  the home lab's state; tried to restart sonarr, refused.", "This turn did nothing but read:
  …", "This turn made no tool calls.") and under any question to the owner ("What I actually
  did this turn: …"). The reviewer and the owner read the truth beside the claim. A call
  answered from what the turn already had, and the worker's own claim before the first round,
  are not listed: they did nothing new.
- **The same call again, with nothing new, is not answered anew**: the worker answers an
  identical call (same tool, same arguments) made straight after itself from what the turn
  already has, without sending it, and one made again later that says just what it said
  before is answered the same way. A small model otherwise asks for the same log round after
  round.
- **The owner's switches**: contracts are on by default; the owner can switch
  `restart_container` off, or every contract off, on the HQ screen. A switched-off contract is
  not offered, and a call to it is refused.

**Data tiers** (as [0105](0105-models-in-ordered-lists-by-data-tier.md)). The home lab's state
is Internal. A container's log is **Confidential**: it is scrubbed first (every credential
shape the egress tripwire knows, a value given to a name that says it is secret, what follows
`Bearer` or `Basic`, and a private key's whole block), and it is given only to turns on a local
model. Every turn is on the local model today, so nothing more is enforced yet. **When the
first member thinks with Claude**, the node must refuse `container_logs` to that member's
turns, or the logs must leave through the door's disguise; that decision is recorded here so it
is not forgotten.

**The audit.** Every call, done or refused, is written to Endora's audit log and Activity with
the member and the run ("HQ: Linus (run 7) restarted sonarr: it was unhealthy. It is now
running"). Each restart, and each restart that did not confirm, is also a notice on the owner's
desk in HQ ("Linus restarted sonarr: it was unhealthy. It is now running."), which takes only
seen or dismiss. The owner's switches are recorded the same way.

## Consequences

- The team can work towards keeping the home lab healthy: look, diagnose from logs, and
  restart what is stuck, without any credential or raw access entering a worker's container.
- Each new ability is a contract, with its kind, tier and limits decided in code and in an ADR
  amendment, not a wider door.
- Docker's restart count is not offered: the observer's fixed route does not carry it, and
  inspecting each container would not be cheap. It can be added with the observer's route.
- A turn that has read a log could, in future, pass it to a cloud model; that is the decision
  above, owed before any Claude member.

## Alternatives considered

- **A key or the Docker socket for the team.** Rejected by the owner: no keys and no raw access.
- **Routing each fix through `request_action` to the owner.** Rejected for safe fixes: a
  restart at night would wait for the morning, and the owner's desk would fill with approvals
  of the obvious. The limits and the notice keep the owner informed instead.
- **Serving the contracts from HQ.** Rejected: HQ holds nothing private and reaches nothing;
  the node is the trust boundary and already holds the home lab's adapters.
