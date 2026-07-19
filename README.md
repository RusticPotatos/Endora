# Endora

**An open platform for continuous growth.**

Endora is a local-first, open-source personal intelligence platform intended to
help people live intentionally through reflection, experimentation, stewardship,
and continuous improvement.

The authoritative backend acts as the brain. Applications are replaceable clients
that communicate through a stable, versioned protocol. AI models are reasoning
components, not sources of authority: models may *propose* actions, but
deterministic policy code controls permission and execution.

## Project status

**Foundation phase.** This repository currently establishes the project's
principles, architecture, and a minimal Rust workspace skeleton. **Endora is not
yet a general autonomous agent** and does not yet expose an application protocol
or product features. Interfaces, schemas, and internals will change. Work grows
one complete vertical slice at a time. The path to the first release is in the
[Roadmap](docs/roadmap.md).

## North Star

> **Help people build lives worth remembering without taking control of those
> lives away from them.**

## What Endora helps people do

Define their own values and direction; set long-term North Stars and intermediate
goals; surface assumptions; form hypotheses; run small experiments; observe
results; reflect and hold retrospectives; propose process improvements; retain
useful evidence and memory; and gradually improve over time.

The system may learn *how* to serve a user better. It may **not** autonomously
redefine *what serving the user means*.

## The initial learning loop

```text
Direction
    ↓
Assumption
    ↓
Experiment
    ↓
Observation
    ↓
Reflection
    ↓
Proposed process change
    ↓
Human approval
```

## Foundational principles

- Human autonomy remains final. The user owns their purpose, values, goals,
  memories, and their definition of a good life.
- Consequential actions require explicit authority.
- Prefer reversible and proportionate actions.
- State uncertainty, evidence, inference, and assumptions clearly.
- Learn from repeated evidence rather than isolated interactions.
- Improve processes more readily than values.
- Memory must be visible, correctable, exportable, and deletable.
- No hidden objectives. No engagement optimization. No dependency manipulation.
- No surveillance business model. No advertising-driven incentives.
- No direct model access to privileged capabilities. The language model is never
  the final enforcement boundary.
- Operate locally where practical; cloud services stay optional and replaceable.
- The person is not a productivity machine. Rest, joy, relationships, meaning,
  community, and changing direction are legitimate parts of life.

These are defined in full in the [Constitution](docs/constitution.md).

## High-level architecture

Endora is a **domain-first modular monolith**. A single authoritative backend —
the **node** — holds all state and authority. Clients are thin and replaceable
and talk to the node through a stable, versioned protocol. AI models sit *behind*
a deterministic policy boundary.

```text
 clients (CLI now; native, web, … later)
        │  stable, versioned protocol (HTTP + JSON, OpenAPI, SSE)
        ▼
 endora-node  ── the authoritative brain
        │  Interface → Application → Domain
        │  Infrastructure → Application/Domain abstractions
        ▼
 deterministic policy boundary → capabilities → local storage
```

Each bounded context is layered Domain → Application → Infrastructure →
Interface, with dependencies pointing inward. The Domain layer depends on no
HTTP, database, AI vendor, UI framework, OS integration, or model-specific
concept.

See [docs/architecture.md](docs/architecture.md), the
[domain map](docs/domain-map.md), and the
[Architecture Decision Records](docs/adr/README.md).

### Current code map

```text
crates/
  endora-domain/       # Domain layer — pure, zero dependencies
  endora-application/  # Application layer — depends on domain only
apps/
  endora-node/         # Authoritative backend runtime (the brain)
  endora-cli/          # Thin, replaceable client
```

## Current technology direction

This is the *intended* direction, not a finished stack. Dependencies are added
only when a real vertical slice needs them.

- **Authoritative core:** Rust (stable, 2024 edition), running natively on macOS
  and Linux (incl. Ubuntu Server) and in Docker.
- **Persistence:** SQLite first, behind application-defined ports.
- **Protocol:** HTTP + JSON, described by OpenAPI, with server-sent events for
  simple live updates.
- **First client:** Swift + SwiftUI (iOS and macOS). Possible later: Android
  (Kotlin/Jetpack Compose), an optional web UI, and CLI/accessibility clients.
- **AI integrations:** local or cloud providers behind replaceable adapters;
  OpenAI-compatible APIs where practical; Anthropic via a separate adapter.
  Python workers only where a specific model/speech/vision/research dependency
  requires Python.

## Non-goals

- Not a general autonomous agent (in this phase).
- No microservices; no Kubernetes; no gRPC; no event sourcing at this stage.
- MCP is **not** the application protocol (it may later expose capabilities to
  external AI systems).
- No surveillance, advertising, or engagement-optimization business model.
- No direct model access to privileged capabilities.
- Not built to require high-end or specialized hardware.

## Branching model

```text
main      → stable public branch
develop   → integration branch
init/*, feat/*, fix/*, …  → topic branches, taken from develop
```

Contributors branch from `develop`. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

Endora is in its foundation stage. The most valuable contributions right now are
discussion, refinement of the architecture and principles, and small, focused
improvements. Please open an issue before large changes, and read
[CONTRIBUTING.md](CONTRIBUTING.md), the
[Code of Conduct](CODE_OF_CONDUCT.md), and [GOVERNANCE.md](GOVERNANCE.md).

Security issues: see [SECURITY.md](SECURITY.md) — please report privately.

**AI-assisted contributions must be clearly flagged as such** in the pull
request. See [CONTRIBUTING.md](CONTRIBUTING.md#ai-assisted-contributions).

## License

Licensed under the [Apache License 2.0](LICENSE). Copyright 2026 Endora
contributors. See [NOTICE](NOTICE).
