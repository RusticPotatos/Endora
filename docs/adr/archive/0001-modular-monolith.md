# 0001 — Domain-first modular monolith

## Status

Accepted (2026). Refined by [0026](0026-package-by-bounded-context.md), which
realizes the "organize by bounded context" intent below as the concrete
`app / domains / shared` workspace layout.

## Context

Endora is a local-first personal intelligence platform that must run on consumer
hardware across macOS, Linux, Ubuntu Server, and Docker, and must survive changes
in models, vendors, and client technologies. It has, initially, a small
contributor base and a single authoritative backend. We need strong internal
boundaries between bounded contexts without paying for distributed-systems
complexity.

## Decision

Build the backend as a **single, domain-first modular monolith**. Organize the
code by **bounded context**, and layer each context as
Domain → Application → Infrastructure → Interface with strictly inward
dependencies. The Domain layer depends on nothing external. Do **not** create
microservices.

## Consequences

- One deployable process; no network hops, distributed transactions, or
  per-service operations. Well suited to local-first, consumer hardware.
- Boundaries are enforced in code (crate/module structure and dependency
  direction), giving us the seams of a service architecture without the cost.
- If a service ever needs to be extracted, the bounded-context seams already
  exist, making extraction a deliberate, bounded effort.
- Contributors must respect layer rules; violations are caught in review and CI
  rather than by process boundaries.

## Alternatives considered

- **Microservices from the start** — rejected: operational and cognitive
  overhead with no benefit for a local-first, single-node app; premature
  distribution.
- **Unstructured monolith** — rejected: loses the bounded-context boundaries
  that keep the domain clean and future extraction possible.
- **Event sourcing / CQRS as the baseline** — rejected for now: significant
  complexity not justified before the first vertical slice exists.
