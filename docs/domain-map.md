# Endora Domain Map

> What each bounded context owns. Rewritten 2026-07-25 after
> [ADR 0029](adr/0029-delete-the-goal-tracker.md), which deleted the Identity &
> Values, Direction & Targets, Experiments & Learning, and Reflection contexts —
> understanding is now the only model Endora keeps of a person.

Each context owns its own model and vocabulary, and is its own crate under
`domains/`. Contexts collaborate through explicit application-layer boundaries,
never by reaching into one another's internals (see
[architecture.md](architecture.md)).

## Understanding

`domains/understanding` — the heart of the system. Owns Endora's **living model of
the person**: beliefs about what they are reaching for, what they value, their
patterns, motivations, frustrations, stressors, and relationships. Every belief
carries **evidence, a confidence, timestamps**, and can be **affirmed, corrected, or
expire** — nothing is held permanently, and nothing is hidden from the person
(ADR 0020).

Also owns **preferences**: things the person has stated outright, which the butler
honours rather than re-asks.

Endora forms beliefs on its own — this is its internal model, not an action, so it is
not gated by per-item confirmation. The person reviews and corrects.

## Conversation

`domains/conversation` — the chat with the butler: messages, their roles and
ordering, and the running summary that keeps a long conversation's prompt bounded
without losing the day's thread (ADR 0028).

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

`domains/scheduling` — the cadences for Endora's proactive moments: check-ins, the
daily brief, the nightly self-improvement loop, and the model-tuning sweep. Off by
default; the person owns whether each runs and when.

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
        forms beliefs   │        │   asks to run a tool
                        ▼        ▼
              understanding    Policy (capabilities)
                        │        │  deny-by-default, reversibility bands
                        │        ▼
                        │    execution ──▶ real result, success or failure,
                        │                  back into the same conversation
                        │        │
                        └────────┴──────▶ platform (audit + event log)

  scheduling ──▶ triggers a proactive turn (check-in / brief / nightly loop)
```

The model never reaches execution directly. Understanding is the one thing the
butler writes on its own; everything that touches the world goes through policy.
