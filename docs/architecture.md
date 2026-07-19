# Endora Architecture

> Status: foundation phase. This describes the intended shape of the system and
> the boundaries the current skeleton already enforces. Implementation grows one
> vertical slice at a time; this document leads the code.

## Overview

Endora is a **domain-first modular monolith**. A single authoritative backend —
the **node** — is the brain. It holds all authority and state. Clients (a CLI
today; native and other clients later) are **thin and replaceable**, and reach
the node only through a stable, versioned protocol.

```text
        ┌─────────────────────────────────────────────┐
        │                 endora-node                  │
        │            (authoritative brain)             │
        │                                              │
        │   Interface → Application → Domain           │
        │   Infrastructure → Application/Domain ports  │
        └───────────────▲──────────────────────────────┘
                        │  stable, versioned protocol
                        │  (HTTP + JSON, OpenAPI-described)
     ┌──────────────────┼───────────────────┐
     │                  │                    │
  endora-cli      native client        later clients
 (thin client)   (Swift/SwiftUI)      (Android, web, …)
```

## The node is authoritative; clients are replaceable

- All state, policy, and decision-making live in the node. A client renders and
  requests; it never holds authority.
- Because clients are replaceable, no client technology is load-bearing. Endora
  must **survive changes in AI models, vendors, frameworks, and client
  technologies.** The protocol — not any UI — is the stable contract.

## Bounded contexts (DDD)

The domain is organized into bounded contexts, each owning its own model and
language. See [domain-map.md](domain-map.md) for what each context owns.
Contexts do not reach into each other's internals; they collaborate through
explicit application-layer boundaries.

Planned contexts: Identity & Values, Direction & Goals, Experiments & Learning,
Reflection, Memory, Policy & Consent, Capabilities, Protection, Audit &
Accountability.

We do **not** create all of these as modules up front. We add a context's code
when a vertical slice needs it.

## Layers within each context

Each bounded context is layered:

| Layer | Responsibility | May depend on |
| --- | --- | --- |
| **Domain** | Pure concepts and rules of the context. | Nothing. |
| **Application** | Use cases; orchestrates the domain; defines ports. | Domain. |
| **Infrastructure** | Adapters that implement ports (DB, HTTP clients, model providers, OS). | Domain/Application *abstractions*. |
| **Interface** | Delivery: the protocol surface, CLI, client-facing edges. | Application. |

### Allowed dependency directions

```text
Interface  →  Application  →  Domain
Infrastructure  →  Domain / Application abstractions
```

The **Domain layer must not depend on**: HTTP, databases, AI vendors, UI
frameworks, OS integrations, or model-specific concepts. This is enforced today:
`endora-domain` has zero dependencies, and higher crates depend inward. See
[ADR&nbsp;0001](adr/0001-modular-monolith.md).

## The deterministic policy boundary around probabilistic models

AI models are reasoning components, not authorities. Around every model sits a
**deterministic boundary**:

```text
   model output (a proposal)
            │
            ▼
   ┌──────────────────────┐
   │  deterministic policy │  ← the enforcement boundary
   │  (Policy & Consent)   │
   └──────────┬───────────┘
              │ authorized?              │ denied / escalate to human
              ▼                          ▼
        capability execution        no action
```

Models never call privileged capabilities directly. A model *proposes*;
deterministic policy code decides; capabilities execute only what policy
authorized. The language model is never the final enforcement boundary. See
[ADR&nbsp;0005](adr/0005-models-propose-policy-authorizes.md) and
[constitution §3](constitution.md).

## Stable, versioned application protocol

- The node exposes a **versioned** protocol. Backward compatibility within a
  major version is a hard requirement so clients can evolve independently.
- Transport is **HTTP + JSON**, described by **OpenAPI**, with **server-sent
  events** for simple live updates. See
  [ADR&nbsp;0003](adr/0003-http-json-openapi-protocol.md).
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
touching the domain. See [ADR&nbsp;0004](adr/0004-sqlite-first.md).

## Why microservices are deliberately deferred

A modular monolith gives us the module boundaries of a service architecture
without the operational cost — no network hops, no distributed transactions, no
per-service deploys — which is the wrong trade for a local-first app on consumer
hardware with, initially, one primary developer audience. Clean bounded contexts
keep the door open: if a genuine need to extract a service appears, the seams
already exist. Until then, microservices would be ceremony. See
[ADR&nbsp;0001](adr/0001-modular-monolith.md).

## Current code map

```text
crates/
  endora-domain/       # Domain layer — pure, zero dependencies
  endora-application/  # Application layer — depends on domain only
apps/
  endora-node/         # Authoritative backend runtime (the brain)
  endora-cli/          # Thin, replaceable client
```

We start with the fewest crates that preserve meaningful boundaries.
Infrastructure and Interface layers become their own crates when a real slice
needs them — not before.
