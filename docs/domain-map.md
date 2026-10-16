# Endora Domain Map

> What each bounded context owns. Rewritten 2026-07-25 after
> [ADR 0052](adr/0052-what-it-knows-about-you.md), which deleted the Identity &
> Values, Direction & Targets, Experiments & Learning, and Reflection contexts —
> understanding is now the only model Endora keeps of a person. Since ADR 0101 that model is
> the brain: markdown pages, not beliefs in the database.

Each context owns its own model and vocabulary, and is its own crate under
`domains/`. Contexts collaborate through explicit application-layer boundaries,
never by reaching into one another's internals (see
[architecture.md](architecture.md)).

## Understanding

`domains/understanding` — the heart of the system. Owns **the brain** (ADR 0101): what
Endora knows about the person, as markdown pages in a folder with its own git history.
`memory.md` holds what they told it (where they are based, how to address them, their
preferences) and the little it knows best, and is read on every turn; every other page has a
summary of what is true now and a dated timeline that is only added to. The page format, the
rules for a page name, and the folder store (which never follows a link and runs git with
everything planted in the folder turned off) are all here.

Endora notes what it learns onto the pages on its own: this is its model, not an action, so it
is not gated by per-item confirmation. The person reads, edits, deletes or forgets any of it.

Also owns intentions (now only the record of research the person asked for), the research
itself, and the outcomes of what Endora did (ADR 0053).

## Conversation

`domains/conversation` — the chat with the butler: messages, their roles and
ordering, and the running summary that keeps a long conversation's prompt bounded
without losing the day's thread (ADR 0053).

## Capabilities

`domains/capabilities` — everything Endora *can do*, and the machinery that decides
whether a given call may run. Owns the skill catalog (built-ins and MCP servers), the
per-skill configuration and enablement, the **policy-gated runner**, the **autonomy
envelope**, and the **egress guard** (SSRF protection and the outbound secret
tripwire). Also owns the butler's model configuration and the deep-model escalation
slot.

This is the enforcement boundary: a capability runs only what deterministic policy
authorized, and never trusts a model's request directly (ADRs 0005, 0021, 0022,
0023, 0024).

## Scheduling

`domains/scheduling` — the cadence of Endora's one proactive moment, the daily brief,
kept on the person's local hour across clock changes. Off by default; the person owns
whether it runs and when. The check-in, the nightly loop and the model-tuning sweep were
removed in the clean slate (ADR 0100).

## Team

`domains/team` — **Endora HQ**'s team core (ADR 0102): goals and the tree of tasks under them,
members and their roles, claiming (one atomic step, one owner), threads, wakes and their
receipts (queued, folded, held, refused, started, finished), runs, the tick, the append-only
budget ledger, the owner's desk, the kill switch and the event feed. It has its own SQLite
store and depends on nothing else in the workspace: HQ is its own service, and talks to the
Endora node only over its API. Done is never a member's word: the gate passes and a different
member approves. A simulated runner (scripted turns, no model) drives its tests.

## Platform

`domains/platform` — the **audit trail** (what was proposed, what policy decided,
what executed, and why) and the butler's **event log** (what it did and learned).
Records exist to protect the person and are themselves subject to memory rights —
they are accountability, not surveillance.

## Shared

`shared/kernel` — the cross-cutting primitives every context speaks: typed ids,
time, errors, `AutonomyLevel`, and the `Reversibility` bands with their
deny-by-default `Decision` mapping. Pure, with no dependencies.

`shared/persistence` — the single SQLite handle the context stores share.

## Memory rights

Not a context but an invariant across all of them: everything Endora stores is
**visible, correctable, exportable, and deletable** (constitution §6). Export and
purge span every context; a context that stores something the person cannot reach is
a bug.

---

## Relationships (high level)

```text
                        conversation
                             │
                             ▼
                    ┌────────────────┐
                    │  the butler    │  the model — proposes only
                    │     turn       │
                    └───┬────────┬───┘
      notes on pages    │        │   asks to run a tool
                        ▼        ▼
              understanding    Policy (capabilities)
                        │        │  deny-by-default, reversibility bands
                        │        ▼
                        │    execution ──▶ real result, success or failure,
                        │                  back into the same conversation
                        │        │
                        └────────┴──────▶ platform (audit + event log)

  scheduling ──▶ triggers a proactive turn (the daily brief)
```

The model never reaches execution directly. Understanding is the one thing the
butler writes on its own; everything that touches the world goes through policy.
