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
| [0010](0010-autonomy-model.md) | Autonomy model: the act/ask loop and preferences | Accepted |
| [0011](0011-review-scheduling-reminders.md) | Review scheduling: the first act of the autonomy model | Accepted |
| [0012](0012-activity-feed-and-change-stream.md) | Activity feed and a server-sent change stream | Accepted |
| [0013](0013-rename-goal-to-target.md) | Rename the second-tier concept from Goal to Target | Accepted |
| [0014](0014-the-butler-conversation-values-attention.md) | The butler: conversational interface, the Values layer, and adaptive attention | Accepted |
| [0015](0015-identity-and-values-context.md) | The Identity & Values context: a "why" above North Stars | Accepted |
| [0016](0016-adaptive-attention.md) | Adaptive attention: ranking and deferral-backoff | Accepted |
| [0017](0017-persona-and-voice.md) | Persona and voice | Accepted |
| [0018](0018-streaming-chat-responses.md) | Streaming chat responses | Accepted |
| [0019](0019-proactive-self-improving-butler.md) | The proactive, self-improving butler: heartbeat, check-ins, capabilities, hospitality | Accepted |
| [0020](0020-intent-first-understanding-loop.md) | Intent-first: the autonomous understanding loop (direction reset) | Accepted |
| [0021](0021-capability-catalog-and-mcp-host.md) | The capability catalog: configuration, enablement, and an MCP host | Accepted |
| [0022](0022-autonomy-envelope-and-self-authored-capabilities.md) | The autonomy envelope and self-authored capabilities | Accepted |
| [0023](0023-egress-guard-and-data-loss-tripwire.md) | The egress guard: SSRF protection and a data-loss tripwire | Accepted |
| [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md) | Reversibility-aware autonomy and the nightly self-improvement loop | Accepted |
| [0025](0025-hospitality-and-the-evolving-persona.md) | Hospitality and the evolving persona | Accepted |
| [0026](0026-package-by-bounded-context.md) | Responsibility-Oriented Clean Architecture (app / domains / shared) | Accepted |
| [0027](0027-self-improving-model-layer.md) | The self-improving model layer (discovery, eval, gated adoption) | Accepted (amended by 0030) |
| [0028](0028-native-tool-calling-turn.md) | One native tool-calling turn (grounded honesty, no deterministic narration) | Accepted |
| [0029](0029-delete-the-goal-tracker.md) | Delete the goal tracker; understanding is the only model | Accepted |
| [0030](0030-measuring-understanding.md) | Measuring understanding: the L3 eval tier and the adoption floor | Accepted |
| [0031](0031-agentic-proactivity.md) | Agentic proactivity: a budget, not a trigger | Accepted |
| [0032](0032-beliefs-decay-and-expire.md) | Beliefs decay and expire | Accepted |
