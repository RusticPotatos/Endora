# 0002 — Rust for the authoritative core

## Status

Accepted (2026).

## Context

The authoritative node holds all state and enforces the deterministic policy
boundary that stands between probabilistic models and privileged capabilities.
It must be correct, memory-safe, efficient on consumer hardware, and deployable
natively on macOS and Linux as well as in Docker — with no mandatory runtime or
garbage-collector tuning for end users.

## Decision

Implement the authoritative core (the node) in **Rust**, using the current
**stable** toolchain and the **2024 edition**. Enforce quality with
`cargo fmt`, `cargo clippy` (warnings denied in CI), and `cargo test`. Forbid
`unsafe` at the workspace level.

## Consequences

- Memory safety without a garbage collector; strong compile-time guarantees for
  a component whose correctness is a security property.
- Single static binaries that run natively across the target platforms.
- Some ecosystem work (e.g. certain model or media dependencies) may be easier
  in Python; those are isolated behind adapters or optional workers, not the
  core (see the technology direction in the README).
- Contributors need Rust familiarity; MSRV is pinned in `Cargo.toml`.

## Alternatives considered

- **Go** — simpler for some, but weaker type-level guarantees and a GC; the
  policy boundary benefits from Rust's stronger compile-time enforcement.
- **TypeScript/Node** — large ecosystem, but a runtime and weaker guarantees for
  an authoritative, security-sensitive core.
- **Python for the core** — rejected: not the right fit for the authoritative
  boundary; reserved for optional workers where a specific dependency requires
  it.
