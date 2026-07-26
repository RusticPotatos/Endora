# 0007 — Async runtime and web stack for the node

## Status

Accepted (2026).

## Context

[ADR 0003](0003-http-json-openapi-protocol.md) commits the node to a versioned
HTTP + JSON protocol, OpenAPI-described, with server-sent events. The foundation
phase intentionally shipped **no** async runtime or HTTP dependency
([ADR 0002](0002-rust-authoritative-core.md)). The [first vertical slice](0006-first-vertical-slice.md)
is the real feature that now justifies adding them. We need to choose the async
runtime and web framework that carry the protocol, keeping infrastructure/
interface concerns out of the Domain layer.

## Decision

Adopt **Tokio** as the async runtime and **Axum** as the HTTP framework for the
node's Interface/Infrastructure layers.

- Axum's `tower`/`tower-http` middleware ecosystem covers routing, extractors,
  timeouts, and CORS without bespoke plumbing.
- SSE is supported natively by Axum for the protocol's live updates.
- OpenAPI is generated from code using a `utoipa`-class crate, so the spec stays
  in sync with handlers rather than drifting.
- These crates live only in the node's outer layers. `endora-domain` stays
  dependency-free; `endora-application` stays free of transport types. HTTP
  handlers translate to/from application use cases at the edge.

## Consequences

- Adds the largest dependency set so far (Tokio, Axum, tower, hyper, an OpenAPI
  crate, and serde for JSON). Each is mainstream and well-maintained; this is the
  justified moment per [CONTRIBUTING](../../../CONTRIBUTING.md).
- The node becomes an async service; care is needed to keep blocking work (e.g.
  SQLite calls) off the async executor (via the storage layer's strategy).
- Framework choice is confined to the interface/infrastructure layers, so a
  future swap would not touch domain or application code.
- Team must be comfortable with async Rust; this is a real cost we accept for a
  networked node.

## Alternatives considered

- **actix-web** — capable and fast, but a heavier actor model and a separate
  ecosystem; Axum's tower alignment fits our layering better.
- **poem / warp / raw hyper** — viable, but smaller ecosystems or more
  boilerplate than Axum for the same result.
- **A blocking/synchronous server** — rejected: SSE and concurrent clients are
  far more natural on an async stack.
- **No OpenAPI generation (hand-written spec)** — rejected: drifts from the
  implementation; contradicts [ADR 0003](0003-http-json-openapi-protocol.md).
