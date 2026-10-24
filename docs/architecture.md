# Endora Architecture

> Describes the shape of the system and the boundaries it enforces. Implementation
> grows one vertical slice at a time. Last reconciled with the code 2026-10-20
> (ADR 0136: Endora is HQ, and the butler's chat is retired).

## Overview

Accepted collaboration target ([ADR 0138](adr/0138-one-conversation-and-assistance-lifecycle.md)):
one conversation/assistance lifecycle replaces legacy task-thread consultation
execution, while the existing Work UI retains readable historical conversations
and task/run links. The old application consultation scheduler and record-answer
fallback and their legacy storage writers have been removed. Context/wake
cleanup, migration rehearsal and live cutover remain required before calling
this complete.
The migration boundary now includes HQ `/v2/team/mcp` and the node's team-only
`/team-gateway/v2/mcp`, which select canonical assistance semantics. Worker/runner
defaults use v2 and reject stale configured routes. The v1 worker endpoint
authenticates first, then returns HTTP 410 with the supported route.
Before publishing HTTP, HQ startup cancels legacy questions, stops and settles
their expert runs, preserving ordinary task work and canonical assistance.
Cleanup failure aborts startup and remains recoverable. Task-linked v2 admission checks
the authorized conversation against trusted task/project context, including
transactional revalidation and original-task replay identity. Atomic content/source
readers and smaller source-classified ordinary task briefings are implemented.
The owner's messaging switch selects this input policy for ordinary developer
and researcher work; there is no separate migration selector. Messaging-disabled
work retains its world/notebook context. Specialized task briefings retain their
authority responsibilities and conservative input guard. Both contexts use the
same canonical peer policy; neither selects legacy consultation execution.
Classified context retains owner notes near the top, actual management routing
and the latest indexed handover, inheriting that note's original source tier.
This is not a completed production cutover.

Protocol tool directories and familiar call names use the same canonical policy.
The worker's first-line identity contract is preserved; version guidance follows
the briefing and restart notices remain last. The automatic claim acknowledgment
contains only the checked task ID and does not invent a private input or clear
existing private inputs. Automatic task recovery waits while that task's accepted
help or result-verification turn remains pending, then resumes without completing
the task or changing its owner. These checks use indexed canonical metadata,
independent of retained question/answer bodies.

Task-thread posts and worker final words remain readable reporting. Their member
mentions dispatch no peer work; only explicit owner-authored posts may schedule
mentions. Real management question/reply and owner-decision pathways retain their
own authority responsibilities, including replies that name their requester.

Retained task questions have a bounded owner-only projection through
`/v1/tasks/{id}/consultations`, proxied by the node into the existing Work task
inspector. Exact original IDs, project snapshots and answer-post associations
are retained; missing links remain unavailable. Reading history creates no
conversation permissions or work. This separates historical inspection from
the retired execution path. Production keeps the historical readers and bounded
startup cancellation, with no legacy admission/claim/answer/delivery writer API.

Endora is a **domain-first modular monolith**. A single authoritative backend —
the **node** — is the trust boundary. It holds all authority and the secrets. **Endora HQ**
(`app/hq`), the team of agents that does the home lab's work, is a separate service the node
fronts; every act a team turn takes crosses the node (ADR 0102). Clients (the web console, the
handheld, a CLI) are **thin and replaceable**, and reach the node only through a stable,
versioned protocol.

```text
        ┌─────────────────────────────────────────────┐
        │                 endora-node                  │
        │       (authoritative trust boundary)         │
        │                                              │
        │   Interface → Application → Domain           │
        │   Infrastructure → Application/Domain ports  │
        └───────────────▲──────────────────────────────┘
                        │  stable, versioned protocol
                        │  (HTTP + JSON, OpenAPI-described)
     ┌──────────────────┼───────────────────┐
     │                  │                    │
  web console      the handheld         endora-cli
 (HQ first)     (clients/terminal)    (thin client)
```

## The node is authoritative; clients are replaceable

The home-lab dashboard is a separate read projection in capabilities: fixed-route
adapters collect bounded Docker and media queue observations, and `/v1/homelab`
serves them to authenticated clients. Observations and credentials are not included
in model context or the model tool catalogue. See [ADR 0092](adr/0092-the-home-lab-is-a-server-owned-read-view.md).

- All state, policy, and decision-making live in the node. A client renders and
  requests; it never holds authority.
- Because clients are replaceable, no client technology is load-bearing. Endora
  must **survive changes in AI models, vendors, frameworks, and client
  technologies.** The protocol — not any UI — is the stable contract.

## Workspace layout — Responsibility-Oriented Clean Architecture (ROCA)

The design patterns in use are a **closed, named set** — see
[the pattern budget](patterns.md). A pattern not on that list needs an ADR that
also names what it retires.

The code is organized **by responsibility**, not by technical layer (ADR 0050).
Each bounded context is its own crate under `domains/`, owning its full
Clean-Architecture stack; cross-cutting primitives live in `shared/`; the
composition roots live in `app/`.

```text
app/         node (the trust boundary), hq, worker, runner, sound, cli — compose the contexts
domains/     one crate per bounded context, each layered internally:
             <context>/src/{domain, application, infrastructure[, interface]}
  platform         audit trail + event log
  capabilities     skills and their stances, the bridge, contracts' policy, egress guard, MCP
  team             Endora HQ's team core: goals, tasks, wakes, runs, the tick, the ledger
  sound            speakers, sources and wires (the sound hub's)
  mail             rules over a read-only mail copy, kept as a library (ADR 0136)
shared/      kernel (ids, time, errors, AutonomyLevel, Reversibility, tiers),
             persistence (Db handle, secrets at rest), protocol (the handheld's wire types)
```

The thin **orchestration layer** (`endora-application`) holds the use cases that compose
several contexts: the activity feed, and the owner's export and delete of Endora's record. The
butler's turn, its brief, research, the brain and the agenda loop that once lived here were
retired with the chat ([ADR 0136](adr/0136-endora-is-hq.md)); the work that outlives a request
is HQ's now. Contexts do not reach into each
other's internals; they collaborate through application-layer ports, and the
dependency graph is acyclic.

See [domain-map.md](domain-map.md) for what each context owns.

HQ's information conversations are also owned by `team` ([ADR 0137](adr/0137-addressed-conversations-and-bounded-assistance.md)).
The owner authorizes fixed project/participant scopes through the node. Existing live turns can
exchange bounded Internal information without creating tasks, wakes or model calls. Conversation
history is retained until explicit owner deletion; routine access uses indexed cursor pages and
deletion uses bounded batches. The owner inspects message and content-free operation pages in
HQ Work. The feature defaults off. Explicit assistance uses one bounded expert
and requester-result lifecycle, owner-granted original evidence and trusted
project charging. Ordinary consultation is a thin adapter to that lifecycle;
it has no task-thread queue or automatic historical-answer substitution.

## Layers within each context

Each bounded context is layered:

| Layer | Responsibility | May depend on |
| --- | --- | --- |
| **Domain** | Pure concepts and rules of the context. | The shared kernel only. |
| **Application** | Use cases; orchestrates the domain; defines ports. | Domain; other contexts' ports (for cross-context orchestration). |
| **Infrastructure** | Adapters that implement ports (DB, HTTP clients, model providers, OS). | Domain/Application *abstractions*; `shared/persistence`. |
| **Interface** | Delivery: the protocol surface, CLI, client-facing edges. | Application. |

### Allowed dependency directions

```text
Interface  →  Application  →  Domain
Infrastructure  →  Domain / Application abstractions
```

The **Domain layer must not depend on**: HTTP, databases, AI vendors, UI
frameworks, OS integrations, or model-specific concepts. This is enforced today:
each context's `domain` module imports only `shared/kernel`, which itself has zero
dependencies, and higher layers depend inward. See
[ADR&nbsp;0001](adr/0050-the-shape-of-the-system.md).

## The deterministic policy boundary around probabilistic models

AI models are reasoning components, not authorities. Around every model sits a
**deterministic boundary**:

```text
   model output (a proposal)
            │
            ▼
   ┌───────────────────────┐
   │  deterministic policy │  ← the enforcement boundary
   │     (capabilities)    │
   └──────────┬────────────┘
              │ authorized?              │ denied / escalate to human
              ▼                          ▼
        capability execution        no action
```

Models never call privileged capabilities directly. A model *proposes*;
deterministic policy code decides; capabilities execute only what policy
authorized. The language model is never the final enforcement boundary. See
[ADR&nbsp;0005](adr/0051-where-the-boundary-is.md) and
[constitution §3](constitution.md).

## Stable, versioned application protocol

- The node exposes a **versioned** protocol. Backward compatibility within a
  major version is a hard requirement so clients can evolve independently.
- Transport is **HTTP + JSON**, described by **OpenAPI**, with **server-sent
  events** for simple live updates. See
  [ADR&nbsp;0003](adr/0050-the-shape-of-the-system.md).
- **MCP is not the application protocol.** MCP may *later* expose selected Endora
  capabilities to external AI systems, but Endora's own clients speak the
  versioned HTTP/JSON protocol.

## Local-first deployment

- Endora runs on consumer hardware and prioritizes the majority of users over
  high-end or specialized systems.
- Targets: **macOS, Linux, Ubuntu Server, and Docker.**
- Cloud services (including cloud model providers) are **optional and
  replaceable** adapters behind the policy boundary.

## Persistence: SQLite first

Persistence starts with **SQLite** — a single-file, local, zero-operations
store that fits a local-first platform. Storage sits in the infrastructure layer
behind application-defined ports, so the engine can change later without
touching the domain. See [ADR&nbsp;0004](adr/0050-the-shape-of-the-system.md).

## Why microservices are deliberately deferred

A modular monolith gives us the module boundaries of a service architecture
without the operational cost — no network hops, no distributed transactions, no
per-service deploys — which is the wrong trade for a local-first app on consumer
hardware with, initially, one primary developer audience. Clean bounded contexts
keep the door open: if a genuine need to extract a service appears, the seams
already exist. Until then, microservices would be ceremony. See
[ADR&nbsp;0001](adr/0050-the-shape-of-the-system.md).

## Current code map

```text
app/
  node/                # The trust boundary: the console, the team's relay, bridge and contracts
  hq/                  # Endora HQ: the team service, its own store, no dependency on the node
  worker/ runner/      # One member's turn, and what starts it in a fresh container
  sound/               # The sound hub
  cli/                 # Thin, replaceable client
domains/
  team/                # Endora HQ's team core
  capabilities/        # Skills and stances, the bridge, contracts' policy, egress guard, MCP
  platform/            # Audit trail + event log
  sound/               # Speakers, sources and wires
  mail/                # Rules over a read-only mail copy, kept as a library
shared/
  kernel/              # The audit id, time, errors, AutonomyLevel, Reversibility, tiers — pure
  persistence/         # The shared SQLite handle, secrets at rest
  protocol/            # The handheld's wire types
crates/
  endora-application/  # Orchestration: the activity feed, the owner's export and delete
  endora-infrastructure/ # Adapters: SQLite store, HQ and sound clients, the relay, vault
clients/
  terminal/            # The handheld (talks to the node over HTTP only)
```

The understanding, conversation, scheduling and agenda contexts were retired with the chat
([ADR 0136](adr/0136-endora-is-hq.md)), as Identity & Values, Direction & Targets,
Experiments & Learning and Reflection were in [ADR 0052](adr/0052-what-it-knows-about-you.md).
See [domain-map.md](domain-map.md).
