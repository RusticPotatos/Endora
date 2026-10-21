# Endora Domain Map

> What each bounded context owns. Rewritten 2026-10-20 after
> [ADR 0136](adr/0136-endora-is-hq.md), which retired the butler's chat: Endora is HQ, a team
> of agents that does the home lab's work, and the node is the trust boundary its turns cross.
> The understanding, conversation, scheduling and agenda contexts went with the chat.

Each context owns its own model and vocabulary, and is its own crate under
`domains/`. Contexts collaborate through explicit application-layer boundaries,
never by reaching into one another's internals (see
[architecture.md](architecture.md)).

## Team

`domains/team` — **Endora HQ**'s team core (ADR 0102): goals and the tree of tasks under them,
members and their roles, claiming (one atomic step, one owner), threads, wakes and their
receipts (queued, folded, held, refused, started, finished), runs, the tick, the append-only
budget ledger, the owner's desk, the kill switch and the event feed, and everything built on
them since (the office, the team's world and map, guards, scenarios and trials). It has its
own SQLite store and depends on nothing else in the workspace but the kernel: HQ is its own
service, and talks to the Endora node only over its API. Done is never a member's word: the
gate passes and a different member approves. A simulated runner (scripted turns, no model)
drives its tests.

## Capabilities

`domains/capabilities` — everything the house *can do*, and the machinery that decides
whether a given call may run. Owns the skill catalog (built-ins and MCP servers), each
skill's settings and stance (ADR 0062), the **bridge** that offers the house's tools to a team
turn (ADR 0108), the **contracts**' policy for the home lab (ADR 0106), the **egress guard**
(SSRF protection and the outbound secret tripwire), and the providers and the ordered
Everyday list the team's relay reads (ADR 0105).

This is the enforcement boundary: a capability runs only what deterministic policy
authorized, and never trusts a model's request directly (ADRs 0005, 0021, 0022,
0023, 0024).

Some of it served only the chat and waits to be removed: recipes, aliases, the autonomy
envelope, the chat's model configuration and the deep model, and the runner chain the chat's
turn built (ADR 0136, step 5b2).

## Platform

`domains/platform` — the **audit trail** (what was proposed, what policy decided,
what executed, and why) and the **event log** (what Endora did, and the owner's setting
changes). Records exist to protect the person and are themselves subject to memory rights —
they are accountability, not surveillance.

## Sound

`domains/sound` — the speakers and what plays on them (ADR 0104): sources, speakers, the wires
between them and saved setups. Owned by the sound hub (`app/sound`), which the node reaches
over HTTP.

## Mail

`domains/mail` — the rules over a read-only Maildir copy (ADR 0097): parsing MIME, the
cautions about senders and links, the buckets, bills and dates, and a sentence that carries no
link. Nothing reads mail now: the chat's reader was retired, and this is kept as a library for
a later HQ contract that reads the copy locally (ADR 0136).

## Shared

`shared/kernel` — the cross-cutting primitives every context speaks: the audit id, time,
errors, `AutonomyLevel`, the data tiers, and the `Reversibility` bands with their
deny-by-default `Decision` mapping. Pure, with no dependencies.

`shared/persistence` — the single SQLite handle the context stores share, and secrets at rest.

`shared/protocol` — the wire types the node and the handheld both speak.

## Memory rights

Not a context but an invariant across all of them: everything Endora stores is
**visible, correctable, exportable, and deletable** (constitution §6). The node's export and
delete cover its record (the audit trail and the events); HQ's own store is in neither yet,
which is known and is its own piece of work (ADR 0136).

---

## Relationships (high level)

```text
   the owner ──▶ HQ (team) ──▶ a runner starts a turn in a fresh container
                    ▲                       │
                    │                       ▼
            desk, receipts          the worker (one member's turn)
                    │                       │  a model call, a house tool, a lab act
                    │                       ▼
                    │            the node's team listener (8788)
                    │              relay ──▶ the house's model or Claude, metered
                    │              bridge ─▶ capabilities: stance, policy, egress guard
                    │              contracts ▶ the home lab, within limits
                    │                       │
                    └───────────────────────┴──────▶ platform (audit + event log)
```

The model never reaches execution directly: every act crosses the node, where policy decides
and the audit trail records it.
