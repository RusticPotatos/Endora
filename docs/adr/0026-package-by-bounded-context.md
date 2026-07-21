# 0026 — Package by bounded context (app / domains / shared)

## Status

Accepted (2026). Refines [0001](0001-modular-monolith.md).

## Context

ADR 0001 committed Endora to a domain-first modular monolith "organized by
bounded context, each layered Domain → Application → Infrastructure → Interface."
In practice the workspace drifted to **layer-first** packaging: three library
crates split by technical layer (`endora-domain`, `endora-application`,
`endora-infrastructure`) plus `apps/`. The domain crate is cleanly split by
concept, but the application and infrastructure layers became god-files — one
`usecases.rs` (~3.7k lines) holds every use case, `ports.rs` every port, and
`sqlite.rs` (~2.6k lines) every repository.

The cost: a single responsibility (the brief, the butler turn, a skill) is
smeared across three large files in three crates. There is no place that "owns"
a subdomain end to end, which is exactly the seam ADR 0001 wanted. This is the
opposite of organizing by responsibility, and it makes the codebase harder to
navigate and reason about as it grows.

The dependency discipline (pure domain, dependencies pointing inward) is sound
and is kept. Only the **packaging** changes.

## Decision

Organize the workspace **by bounded context**, mirroring the convention proven
in a sibling project:

```
app/        composition root(s) — the node binary, the CLI
domains/    one crate per bounded context
shared/     cross-cutting code only (kernel, persistence)
```

Each `domains/<context>` is a crate, internally layered by module:
`domain/` (pure — imports only the shared kernel), `application/` (use cases +
this context's port traits), `infrastructure/` (repository impls over the shared
persistence handle, plus external adapters), and `interface/` (Axum handlers and
a `routes()` the composition root mounts).

The six contexts are **conversation**, **understanding**, **direction**,
**capabilities**, **scheduling**, and **platform**. `shared/` holds **kernel**
(typed ids, timestamps, `DomainError`, `Clock`/`IdSource` traits, and
`AutonomyLevel` — the one type two contexts share) and **persistence** (a shared
SQLite `Db` handle; the single-connection semantics are preserved).

Dependencies still point inward and cross-context calls go application→
application/ports only, keeping the graph acyclic: `capabilities`,
`understanding`, `direction`, and `platform` are leaves; `conversation` composes
the first three; `scheduling` orchestrates over `conversation`/`capabilities`/
`understanding`; the composition root depends on all.

Domain-layer purity is enforced by convention and review within each crate
(the `domain/` module imports only the kernel) rather than by a separate crate
per layer — one crate per context, not four, to avoid a crate explosion.

## Consequences

- Each subdomain is navigable and owned in one place; the god-files dissolve.
- Compile-time boundaries between contexts (a context cannot reach into another's
  internals except through its published `application`/ports).
- The public HTTP API, JSON contracts, and web UI are unchanged — this is an
  internal restructuring, verified by the existing test suite staying green and a
  live smoke test.
- More crates and `Cargo.toml` wiring; a shared persistence handle replaces the
  single `SqliteStore`. A one-time, phased migration (strangler: one context at a
  time, green after each) rather than a big-bang rewrite.
- Domain purity is a discipline within each context crate, not a compiler
  guarantee across a dedicated domain crate. Accepted trade-off for a sane crate
  count; caught in review.

## Alternatives considered

- **Keep layer-first crates** — rejected: it is what produced the god-files and
  never delivered the by-context ownership ADR 0001 called for.
- **A crate per layer *per context*** (four crates × six contexts) — rejected:
  compile-time domain purity at the price of ~24 crates; "too crazy" for a
  single-node app. Module-level purity within one crate per context is enough.
- **One `endora-core` crate, contexts as modules** — rejected: lighter Cargo
  churn, but loses the compile-time boundary *between contexts* that separate
  crates give; the seams are the point.
