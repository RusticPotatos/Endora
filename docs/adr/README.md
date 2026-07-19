# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures a single significant
architectural decision: its context, the decision itself, the consequences, and
the alternatives that were considered. ADRs are short, immutable once accepted,
and numbered in sequence.

## Why we keep ADRs

Endora is designed to outlive specific models, vendors, and frameworks. ADRs
record *why* the structure is the way it is, so future contributors can revisit a
decision deliberately rather than by accident.

## When an ADR is required

- Any change to layer boundaries or dependency directions.
- Adopting or replacing a load-bearing technology (protocol, storage, runtime).
- Any change that touches the deterministic policy boundary around models.
- Adding a new runtime dependency that is not obviously justified.

See [CONTRIBUTING.md](../../CONTRIBUTING.md).

## Status values

`Proposed` → `Accepted` → (later) `Superseded by NNNN` / `Deprecated`.

## Format

Each ADR includes: **Status**, **Context**, **Decision**, **Consequences**, and
**Alternatives considered**. Keep them concise. To add one, copy the structure
of an existing record and take the next number.

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-modular-monolith.md) | Domain-first modular monolith | Accepted |
| [0002](0002-rust-authoritative-core.md) | Rust for the authoritative core | Accepted |
| [0003](0003-http-json-openapi-protocol.md) | HTTP + JSON + OpenAPI application protocol | Accepted |
| [0004](0004-sqlite-first.md) | SQLite-first persistence | Accepted |
| [0005](0005-models-propose-policy-authorizes.md) | Models propose; policy authorizes | Accepted |
| [0006](0006-first-vertical-slice.md) | First vertical slice: the learning loop for one goal | Accepted |
| [0007](0007-async-web-stack.md) | Async runtime and web stack for the node | Accepted |
| [0008](0008-local-model-adapter.md) | Local model adapter | Accepted |
| [0009](0009-node-served-ui-and-single-container.md) | Node-served web UI and single-container packaging | Accepted |
