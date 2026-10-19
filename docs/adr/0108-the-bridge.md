# 0108 — The bridge: Endora's capabilities as the team's tools

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amended by
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (2026-10-17): every call through the
contracts and the bridge, done or refused, is told to HQ as a receipt with the turn's own token,
and its id follows the result for the model to cite. Amends
[0106](0106-team-contracts.md): the team's contracts are now the generic container tools plus
the bridge, and the first ask-first acts exist. Builds on [0062](0062-one-permission-surface.md)
(the stance), [0105](0105-models-in-ordered-lists-by-data-tier.md) (tiers) and
[0064](0064-what-a-stranger-said.md) (a stranger's words). Built: the catalog and policy on the
node, approvals on HQ's desk with one-tap Approve and Decline, the worker ending its turn on
one, Sonarr's and Radarr's stuck-download actions, the owner's reply on any task, and a goal's
notes for the team.

Amended 2026-10-17 by [0109](0109-a-brain-per-member.md): HQ now says `"cloud": true` for a
member whose brain is a Claude model, and by the owner's decision a lead's cloud turn may read
`sonarr_queue` and `radarr_queue` (and the scrubbed logs); every other Confidential tool stays
unlisted and refused for a cloud turn, and no other role's cloud turn gets any. Lessons the lead
or the reviewer note go at the end of a goal's notes for the team.

Amended 2026-10-17 by [0110](0110-the-teams-rings.md): the bridge is how a turn on the Pi reaches
Sonarr, Radarr, Portainer and Home Assistant, through the team gateway, under the same policy;
the sandbox reaches no home service any other way.

Amended 2026-10-17, after a live alarm: the developer asked to run Home Assistant's
`HassTurnOff` on a container's name "to restart" it. **The owner's devices are never the
team's to switch**: every act of the tool server paired with Home Assistant (the owner's
`mcp_server` setting on the Home Assistant connection, `home-assistant` unless named) is kept
off the bridge, never listed and refused if called. The team's remit is the home lab's
containers; turning the owner's lights, switches and appliances on or off never was part of it,
and a container restart has its own contract. This is the simple rule, for every task rather
than only those under the home lab's goal: a narrower one would offer the switches again to any
other work. The tool the owner named that server's reader stays a read, Confidential, for local
turns only. Every other tool of that server counts as an act, as any tool server's does, and is
kept off with the switches.

Amended 2026-10-19 by [0131](0131-the-two-kinds-of-turn.md) and
[0124](0124-guards-against-how-agent-teams-fail.md), after a developer on the house's own small
model on the NAS asked the owner to approve a web map search with coordinates it had made up:
**what the bridge offers a turn depends on where it thinks** (a turn on that model is kept from
the tools that can end on the owner's desk, and refused one it names, and keeps the lab's
contracts and the reads and acts the owner trusts to run without asking; the owner's Mac and
Claude are offered as their role and the tier allow), and **an act that asks the owner says which
of its task's criteria it serves**, or it is refused to the member and never reaches the desk.
Scenarios: `domains/capabilities/features/tools-that-fit-the-turn.feature` and
`domains/team/features/a-request-says-what-it-serves.feature`.

## Context

The team keeps the home lab healthy through contracts (0106), and the owner has more than forty
containers. A hand-written contract per app would never keep up, and each would be a second
place to decide what Endora already decides for its own butler: whether a tool is on, whether it
asks, what tier its result is. The four downloads stuck in Sonarr's and Radarr's queues were the
first case: the team could see them and do nothing.

## Decision

**Three layers.**

1. **Generic container tools** for every container: `homelab_status`, `container_logs` and
   `restart_container`, as 0106 built them.
2. **The bridge.** Every capability Endora has, and every tool of every tool server it has
   installed, is offered to the team automatically, governed by policy Endora already has: the
   data tier, the owner's stance per capability (on, ask, off), and third-party marks. Nothing
   is written per app.
3. **Tool servers** for anything else: installed once, their tools appear on the bridge with
   their own stance. A tool server's action the owner has said nothing about asks.

**Endora decides what a tool is, never the tool.** A built-in skill reads only when it declares
the observation band; anything else acts. A tool server's tool reads only when the owner named
it the server's state reader; every other tool, whatever it calls itself, acts. A server's
self-description is not evidence (0054): it is the one party with a reason to say "harmless".

**The policy, in order** (`domains/capabilities/src/bridge.rs`; served by the node's team
listener, `GET /team-relay/contracts` and `POST /team-relay/contracts/{name}`):

1. **Never offered**, whatever the stance: a tool switched **off**; a **Restricted** one (an
   unclassified skill counts as Restricted, so it fails safe); Endora's own administration (a
   connection's settings entry and its test); and anything about the team, its money, or the
   owner's chat, brain, memory or mail (`mail`, `own_activity`, `read_page`, `brief`,
   `image_review`, `location`, and a tool server's tool whose name touches mail, chat, brain,
   memory, Endora, the team or money). Nor is anything that reaches any address with
   arguments the member chooses (`web_fetch`, `web_search`): what a turn has read could leave
   in a URL, a way out of the team's fence. Nor is an act of the tool server paired with Home
   Assistant: the owner's devices are not the team's to switch (amended above). A tool not set
   up is not offered either.
2. **The tier.** A Confidential result is given only to a turn on a local model, but for what
   the owner allowed a lead thinking with Claude (0109: the download queues and the scrubbed
   logs). The check point is the turn as HQ describes it to the node: HQ says `"cloud": true`
   for a member whose brain is not local, and the node then neither offers nor runs any other
   Confidential tool for that turn.
3. **The profile by role**: lead and developer read and act; reviewer and researcher only
   read; a role not listed does nothing. A small table in code that a later owner setting can
   replace. *Amended 2026-10-19:* **and by where the turn thinks** (`bridge::Thinks`, told by HQ
   with the turn): a turn on the house's own model on the NAS, or one HQ says nothing of, is
   offered what its role allows less whatever can end on the owner's desk (an act that asks, or
   that takes arguments and so asks once a stranger's words are read), and a name it makes up of
   one is refused (403); the reads and the acts that run on their own and take nothing stay. The
   Mac's and Claude's turns are offered what the profile allows. The lab's contracts (0106) are
   not the bridge and stay with it.
4. **The stance.** A read runs through Endora's own invoke path (the outbound secret check and
   redaction, the connection's settings) and keeps a stranger's mark when it is a third
   party's words. An act whose stance is **on** runs at once, at most three times an hour a
   tool across the team (taken before it is tried, kept in the node's database), and is a
   notice on the owner's desk saying what ran, for whom and why. An act whose stance is
   **ask** runs nothing: it is stored on the node, put on the desk as an **approval** saying the
   tool, its arguments in plain words and the member's reason, and the member's turn ends as
   `ask_owner` ends it, with the same limit (one question or approval a member about a task at
   a time). *Amended 2026-10-19:* the request also says which of its task's criteria it serves
   (`serves`, its number or its words), and the card shows that criterion as it was asked. HQ,
   which holds the criteria, reads it before anything is put on the desk or the turn is ended: a
   request that names none, or sits on a task with no criteria, is refused to the member with the
   reason (400) and never reaches the desk. An act given arguments in a turn that has read a stranger's words asks too: a
   stranger can choose what an act is given, while an act given nothing can only be done or
   not, and an on one is limited and told.
5. **The approval runs exactly what was shown.** Approve on the card makes the node run the
   stored tool and arguments, nothing a model can change afterwards, through the same path,
   after checking again that the stance is not off and the role may act, and only while HQ's
   desk still holds that approval as it was asked. What came of it goes in the task's thread
   and wakes the member. Decline runs nothing, says so in the thread and wakes the member.
6. **Expiry.** An approval nobody decides within a day goes unrun, with a note in the thread.

Every call, read, act, ask or refusal, leaves an audit line with the member and the run, as
the contracts do. Every act needs a `why`, added to its schema by the bridge, and one that
always asks needs a `serves` as well (every act has the property, since a turn that has read a
stranger's words asks before one); neither is passed to the tool, kept with what an approval
will run, or said among its arguments.

**Sonarr's and Radarr's stuck downloads.** Their queue read now gives each item's number,
the app's tracking state and its own status messages (why it is stuck). Two actions join it,
on Endora's own capability so the butler has them too: **retry the imports** (the app's
`RefreshMonitoredDownloads` command; given nothing, removes and deletes nothing; reversible,
so on by default) and **remove, blocklist and search again** (one queue item by its number,
refused when the queue does not hold it; asks by default). The apps share one API, so an app is
a row of configuration, not code. The defaults come from the stance mechanism of 0062: a
reversible act defaults to on, an outward one to ask.

**The owner speaks to the team directly.** The owner's reply on any open task, from the HQ
screen, goes in its thread and wakes whoever has it at once (`owner_replied`), mention or not,
past the back-off. Each goal has the owner's notes for the team (at most 2,000 characters),
edited on the screen; every briefing for a task under it carries them near the top, for
standing facts such as what is being retired and what is the owner's own.

## Consequences

- Each new ability is the owner's stance on a capability or a tool server, not a contract
  written in code. A new app of the same API is a configuration row.
- The bridge offers what is set up. Today that is every public read (weather, news and the
  like), the house's reads (containers, the NAS, Home Assistant's state, the download queues,
  Confidential and local only) and the two download actions; Endora's settings entries and the
  owner's own skills are not offered.
- Retrying the imports cannot clear an item the app has blocked for a reason; the queue read
  says which, and removing it, blocklisting and searching again is the fix, which asks.
- Approvals are kept on the node, and HQ's desk holds only their words, so a node and an HQ
  whose stores part ways refuse to run rather than run the wrong thing. An approval decided
  while the node was down runs nothing until the owner taps again.
- The name guard on tool servers' tools fails safe: a tool kept off by mistake is asked for in
  a thread, never quietly given.
- A tool server's tools are Confidential, so they reach local turns only. A turn on the NAS's own
  model is not offered one that asks (which is every tool of a server the owner has not
  reviewed, its web and search tools among them), but still reads, as the Mac's does: the owner's
  named readers, the download queues, and the lab through the contracts. The rest is left to a
  member on the Mac, said in the thread.
- A request for the owner's yes that serves nothing its task asked for costs the owner no card:
  the member reads why it was refused, and its turn goes on.

## Alternatives considered

- **A contract per app.** Rejected by the owner: forty containers, forty contracts, each a
  second copy of a decision Endora already makes.
- **Trusting a tool's own description of itself** (read-only hints, annotations). Rejected: it
  is the claim of the one party that cannot be the judge.
- **The team asking with `ask_owner` and the owner doing it by hand.** Rejected for the safe
  and the obvious: approvals carry the exact call, so the owner's yes is the whole job.
- **Letting HQ run approved actions.** Rejected: HQ holds nothing private and reaches nothing;
  the node is the trust boundary.
