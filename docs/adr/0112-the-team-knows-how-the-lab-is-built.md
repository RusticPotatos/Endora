# 0112 — The team knows how the lab is built

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0106](0106-team-contracts.md): two more read contracts, `homelab_map` and `ansible_nas_doc`,
offered to every role on any brain, and one more fixed read-only route on the Docker observer.
Built: both contracts on the node's team listener, the observer's inspection route, the house
log seeded once, and the same text in the worker's instructions.

## Context

Gluetun, the VPN container, restarted. Deluge, nzbget and prowlarr stopped with it, because
they run in its network (`NetworkMode: container:<gluetun>`), and came back by themselves a
minute later. The team knew none of that: not the wiring, and not that the lab is **managed
by ansible-nas** (each app is a role, and the playbook creates its container). So it chased a
problem that had fixed itself, and reached for tools that could not have fixed it. The owner
wants the team to know how the lab is set up.

## Decision

**`homelab_map`** (read, Internal). For every container, from Docker's own metadata through
the observer's fixed routes: its name, image (`repo:tag`, never a digest), state and restart
policy; what it depends on (`container:<id>` network modes resolved to the owner's name, and
compose `depends_on` labels); what depends on it; who manages it (`ansible-nas role <name>`
when a role of that name exists, `Endora` for Endora's own, else `unknown`); and the owner's
note when it is on the retired list (`ENDORA_LAB_RETIRED`, `name=note` pairs). Grouped and
compact: one wiring line a container others hang on ("gluetun → deluge, nzbget, prowlarr (run
in its network)"), and the containers by who manages them. Kept for a minute.

- **Names and wiring only.** The map keeps a fixed handful of Docker's fields and drops the
  rest, wherever the inspection came from: no environment, no mounts, no binds, no label but
  compose's service, project and `depends_on`, and anything secret-shaped is scrubbed.
- **One more route on the observer** (`scripts/homelab-docker-proxy.py`): `GET
  /containers/<name>/json`, the name matching `[A-Za-z0-9][A-Za-z0-9_.-]{0,127}` and nothing
  else (no query, no other segment). The proxy passes on only the id, name, image, state,
  restart policy, network mode and those three labels. Its list route is unchanged.

**`ansible_nas_doc`** (read, Internal). The lab's ansible-nas is the owner's public fork on
GitHub, named in the node's environment: `ENDORA_ANSIBLE_NAS_REPO` (`owner/name`) and
`ENDORA_ANSIBLE_NAS_REF` (`master` unless set), set in compose from `local.mk`. The node reads
the role names (`roles/`) and **only** the docs: `docs/**/*.md`, `website/docs/**/*.md` and
`roles/*/README.md`, listed through GitHub's contents API inside those folders alone and read
from its raw files, with a timeout, kept for a day. Never an inventory, `group_vars`,
`host_vars`, a vault or any other file: every path a turn gives, and every path GitHub lists,
passes one allowlist, which also refuses `..`, a dot-segment and anything but plain names. A
folder is listed (`roles` lists every role); a doc is read, at most 16 KB. Unset, the contract
says the docs are not configured, and every container's manager is `unknown`.

**Standing knowledge.** One text, in the shared kernel (`endora_kernel::lab`), says how this
lab is built: managed by ansible-nas, each app a role; a crashed container may be restarted,
and anything else (settings, memory limits, new or removed containers) is a proposal to change
ansible-nas for the owner to approve and run; containers in another's network stop when it
restarts and come back on their own, so check `homelab_map` and the current state first. HQ
seeds the CEO's house log with it **once, and only when the log is empty**, said in the feed
as Endora's; a log with text in it is never overwritten, and one the owner clears stays clear.
The worker's home-lab instructions carry the same text as one paragraph.

## Consequences

- A stop that follows a restart of the container it hangs on is read as what it is, and a
  change to the lab is put to the owner as a change to ansible-nas, not attempted by hand.
- The map costs one inspection a container a minute at most, and only when a turn asks.
- Until the new observer is deployed, its inspection route answers 403: the map then has the
  names and states and says it has no wiring.
- The role list and docs reach GitHub over the node's normal network; without it, the map's
  managers are `unknown` and the docs say GitHub could not be read.

## Alternatives considered

- **Portainer's inspection** instead of the observer's: it is set up only for logs and
  restarts, and the observer is the read path the node already keeps for the lab's state.
- **Cloning the fork**, or GitHub's whole recursive tree: either reads, at least by name, what
  the team must never see; the contents API walked inside the allowed folders does not.
- **Writing the knowledge into each turn's briefing in HQ**: the house log is already read by
  the CEO and the lead every turn, and is the owner's to edit; the worker's instructions reach
  every other role.
