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
  endora-domain/          # Domain layer — pure, zero dependencies
  endora-application/     # Application layer — use cases + ports; depends on domain
  endora-infrastructure/  # Infrastructure layer — adapters (SQLite persistence)
apps/
  endora-node/            # Authoritative backend runtime — serves the HTTP/JSON API
  endora-cli/             # Thin, replaceable client
```

### Running the node

```bash
make run-node            # starts on 127.0.0.1:8787 (override: ENDORA_ADDR, ENDORA_DB)
curl -s localhost:8787/health
curl -s -X POST localhost:8787/v1/directions \
  -H 'content-type: application/json' -d '{"title":"Be healthier"}'
```

**Web console:** with the node running, open **http://localhost:8787** in a
browser. The node serves a self-contained UI for the whole loop — create and
navigate Direction → Goal → Assumption → Experiment → Observation → Reflection,
run the propose → approve → policy-decide flow, view the audit trail, and
export/purge. No separate app to install (see ADR 0009 — node-served UI and
single-container packaging).

Or in a container (data persists in `./endora-data`, published on loopback only):

```bash
make docker-build && make docker-run    # → http://localhost:8787
```

**Running it always-on and reaching it from your phone:** see
[docs/hosting.md](docs/hosting.md) — how to keep the node running (systemd /
container restart) and reach it securely from other devices over a private
network (e.g. Tailscale) or an authenticating reverse proxy. The `0.x` API is
unauthenticated, so it must stay on a trusted network — see
[SECURITY.md](SECURITY.md).

### See the whole thing in one command

```bash
make demo    # spins up a throwaway node and drives the full learning loop
```

This runs `scripts/demo.sh`: direction → goal → assumption → experiment →
observation → reflection → proposed change → policy decision → audit → export,
printing each CLI command and its response.

### Using the CLI

With the node running, the `endora` CLI (a thin client) talks to it:

```bash
make run-cli ARGS="health"     # or run the binary directly:
endora direction create "Be healthier"
endora goal create <direction-id> "Run a 5k"
endora goal list <direction-id>
# override the node URL with ENDORA_URL (default http://127.0.0.1:8787)
```

### Optional: a local model that *proposes*

The node can ask a local, open-weights model to **draft** a process change from a
reflection. The model only ever proposes — its output becomes an ordinary
*pending* proposal that still needs human approval and passes through the
deterministic policy boundary like any other. Nothing breaks without it: the
node runs fine and only the drafting endpoint returns `503`.

```bash
ollama serve &                       # a local OpenAI-compatible endpoint
ollama pull qwen3.5:9b               # or qwen3.5:4b on lighter machines
# point the node at it (defaults shown):
ENDORA_MODEL_URL=http://localhost:11434/v1 ENDORA_MODEL=qwen3.5:9b make run-node

endora process-change draft <reflection-id>   # model drafts a pending change
endora process-change approve <id>            # a human approves
endora process-change decide <id> act_within_policy   # policy authorizes; audited
```

The node serves the whole learning loop (Direction → Goal → Assumption →
Experiment → Observation → Reflection → Proposed process change), with the policy
boundary and audit trail on consequential decisions — see the
[Roadmap](docs/roadmap.md) for what remains before a tagged release.

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
