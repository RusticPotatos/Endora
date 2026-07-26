# 0050 — The shape of the system

## Status

Accepted (2026-07-26). **Consolidates [0001], [0002], [0003], [0004], [0007], [0009],
[0012], [0018] and [0026]**, which are archived. Nothing here is new; this is what those
nine decided, stated once.

## Context

Endora is a personal intelligence that runs on hardware its owner controls, is expected to
outlive any particular model or vendor, and has exactly one operator. Every structural
choice below follows from those three facts, and each was originally argued separately as
the system was built.

## Decision

### One process, organized by responsibility

A **modular monolith** — not microservices. The workspace is organized by **bounded
context** rather than by layer: `app/` (composition roots), `domains/` (`platform`,
`capabilities`, `understanding`, `conversation`, `scheduling`), `shared/` (`kernel`,
`persistence`), and a thin orchestration crate for use cases that span contexts.

Each context owns its own `domain` / `application` / `infrastructure`, with dependencies
pointing **inward**. A context's `domain` imports only `shared/kernel` and stays free of
HTTP, storage, vendors, UI and OS. Cross-context calls go application→application, and the
graph stays acyclic.

This is **Responsibility-Oriented Clean Architecture**: Clean Architecture's inward layers,
with *responsibility* rather than *layer* as the primary axis. Splitting by layer put every
feature's pieces in four different places; splitting by responsibility keeps a change local.

### Rust, and no unsafe

The authoritative core is **Rust**, stable toolchain, 2024 edition, `unsafe` forbidden at
the workspace level. CI denies clippy warnings and runs `fmt`, `clippy` and `test` on every
change. A system trusted to act on someone's home should not fail on a memory bug, and the
type system is where guarantees are cheapest to enforce.

### HTTP + JSON, versioned, with SSE for liveness

Clients speak a **versioned HTTP/JSON** protocol, described by OpenAPI, with **server-sent
events** for live updates. Backward compatibility within a major version is a hard
requirement.

**MCP is not the application protocol.** Endora *hosts* MCP to reach other people's tools
([0054](0054-other-peoples-services.md)); its own clients use this protocol.

Live updates carry a **nudge, not data**: a broadcast signal after a successful mutation,
which clients respond to by re-reading. One channel serves every view, and no view can go
stale in a way the server has to model.

Streaming is **additive**: the `Butler` port has a one-shot method with a default streaming
implementation that emits the whole reply as one chunk, so every implementation works with
a streaming caller and only the model-backed one streams for real.

### SQLite, behind ports

A single embedded file, zero operations, accessed only through **application-defined
ports**. The domain never names the engine, so it can be replaced without touching domain
code. For one household this is not a compromise: it is the right size, and it makes the
whole system a file the owner can copy.

### One container, serving its own console

The web console is **embedded in the binary** and served same-origin, and the whole system
ships as **one container image** (with a local model as a separate, optional Compose
service). `docker compose up` brings up the product. No build step, no CORS, no separate
static host, no version skew between UI and API.

### Read projections rather than new tables

Views like the activity feed are **computed from facts already stored** — no new entity, no
schema of its own. The same instinct later became "derived, never stored" for findings
([0054](0054-other-peoples-services.md)) and is why there is no queue anywhere in this
system to groom.

## Consequences

- A single deploy artefact, a single database file, and no distributed-systems problems
  bought before there was a distributed system to solve.
- Contexts can be extracted later if a real reason appears. None has.
- The domain layers are testable without a database, a network or a model, which is why the
  test suite runs in seconds.
- **The cost is discipline**: nothing prevents a context from reaching sideways except
  review and the dependency graph. It has held so far.

## Rejected

- **Microservices**, at every juncture. One operator, one machine; the coordination cost
  buys nothing.
- **A separate frontend app and host.** Same-origin embedding removes an entire class of
  deployment and CORS problems.
- **A heavier database up front.** Replaceable behind ports, and nothing has needed it.
- **Package-by-layer.** Argued and reversed in [0026]: it scattered every feature.
- **Exposing the domain over MCP as the primary protocol.** MCP is how Endora *consumes*
  tools, not how its own clients talk to it.

[0001]: archive/0001-modular-monolith.md
[0002]: archive/0002-rust-authoritative-core.md
[0003]: archive/0003-http-json-openapi-protocol.md
[0004]: archive/0004-sqlite-first.md
[0007]: archive/0007-async-web-stack.md
[0009]: archive/0009-node-served-ui-and-single-container.md
[0012]: archive/0012-activity-feed-and-change-stream.md
[0018]: archive/0018-streaming-chat-responses.md
[0026]: archive/0026-package-by-bounded-context.md
