# 0004 — SQLite-first persistence

## Status

Accepted (2026).

## Context

Endora is local-first and privacy-preserving, targeting consumer hardware. Data
should live on the user's own machine by default, with no server to operate and
no mandatory cloud dependency. We need durable storage that is simple to back
up, export, and delete — consistent with the constitutional memory rights.

## Decision

Use **SQLite** as the initial persistence engine: a single-file, embedded,
zero-operations store. Access it through **application-defined ports** in the
infrastructure layer, so the domain never depends on the storage engine and the
engine can be replaced or supplemented later without touching domain code.

## Consequences

- No database server to run; a user's data is a file they own, can back up, and
  can delete — directly supporting memory rights and local-first goals.
- Excellent fit for single-node, single-user workloads on consumer hardware.
- Because persistence sits behind ports, moving some data to another engine
  later (or adding an optional sync layer) is an infrastructure change, not a
  domain change — and would get its own ADR.
- Heavy multi-writer or large-scale concurrent workloads are not a design target
  at this stage.

## Alternatives considered

- **PostgreSQL** — rejected as the default: requires operating a server, at odds
  with local-first and zero-ops on consumer hardware.
- **Embedded key-value store (e.g. sled/RocksDB)** — rejected initially: SQLite's
  relational model, tooling, portability, and ubiquity are a better fit.
- **Custom file formats** — rejected: reinvents durability, querying, and
  migration that SQLite already provides well.
