# 0003 — HTTP + JSON + OpenAPI application protocol

## Status

Accepted (2026).

## Context

Clients are thin and replaceable, and the platform must survive changes in
client technologies (Swift/SwiftUI first; Android, web, CLI, and
accessibility-focused clients later). The node needs a stable contract that any
client, in any language, can speak, plus a simple mechanism for live updates.

## Decision

The node exposes a **versioned** application protocol over **HTTP + JSON**,
described by **OpenAPI**, with **server-sent events (SSE)** for simple live
updates. Backward compatibility within a major protocol version is a hard
requirement.

**MCP is not the application protocol.** MCP may later expose selected Endora
capabilities to *external* AI systems, but Endora's own clients use this
HTTP/JSON protocol.

## Consequences

- Any platform with an HTTP client can build an Endora client; no bespoke SDK is
  required to start.
- OpenAPI gives machine-readable contracts, generated clients, and documentation.
- SSE covers live updates with far less complexity than bidirectional streaming;
  if richer needs appear, they get their own ADR.
- Versioning discipline is required from the first slice so clients can evolve
  independently.

## Alternatives considered

- **gRPC** — rejected for now: heavier tooling and browser friction; not needed
  at this stage.
- **WebSockets for all live updates** — rejected initially: SSE is simpler and
  sufficient for one-way live updates.
- **GraphQL** — rejected: added complexity without a demonstrated need for its
  query flexibility at the foundation stage.
- **MCP as the primary protocol** — rejected: MCP is for exposing capabilities
  to external AI systems, not the internal client contract.
