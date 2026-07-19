# Endora Roadmap

> Status: living document. This roadmap states **intent and sequencing**; it is
> not authoritative for decisions. Architectural decisions are recorded in
> [ADRs](adr/README.md), and execution is tracked in GitHub issues/milestones.
> When this roadmap and an ADR disagree, the ADR wins.

## Where we are

Endora is in the **foundation phase**: principles, architecture, and a minimal
workspace skeleton exist, but there is no protocol, persistence, policy engine,
or model yet. The next milestone is **v1.0**.

## What v1.0 means

> **One complete vertical slice of the learning loop, running locally, with the
> constitutional guarantees (deterministic policy boundary, memory rights, audit)
> actually enforced in code — not just documented.**

v1.0 is a real, usable, honestly-guaranteed release — not a general autonomous
agent. It ships the full learning loop for a single goal, driven from the CLI,
with a local model proposing and deterministic policy authorizing.

## Scope decisions for v1.0

These were chosen deliberately. Where a decision is architectural it is promoted
to an ADR during the work (numbers below are the intended slots).

### 1. The slice: the full learning loop, one goal

v1.0 implements the whole loop for a single user goal:

```text
Direction → Assumption → Experiment → Observation → Reflection
          → Proposed process change → Human approval
```

This exercises every layer and every core guarantee end-to-end. It touches the
**Direction & Goals**, **Experiments & Learning**, and **Reflection** bounded
contexts, plus the cross-cutting **Policy & Consent**, **Memory**, and
**Audit & Accountability** contexts. Detailed entities/APIs are designed *with*
this slice — recorded in **ADR 0006 (first vertical slice)** — not invented
ahead of it.

### 2. Model: real, local, open-weights — behind the deterministic boundary

The **deterministic policy boundary is the load-bearing guarantee** and must be
fully real and independently tested. Behind it, v1.0 uses a **real local model**,
not a stub:

- A well-known **open-weights instruct model that runs on consumer hardware**
  (target: ~7–9B class, quantized, comfortable on 16 GB RAM / Apple Silicon).
  Strong starting candidates: **Llama 3.1 8B Instruct** or **Qwen2.5 7B
  Instruct**.
- Served locally through an **OpenAI-compatible endpoint** (e.g. Ollama or a
  llama.cpp server), consumed via a **replaceable adapter** so the specific model
  and runner are never load-bearing. This keeps Endora local-first and
  privacy-preserving by default; cloud providers remain optional 1.x adapters.
- The final model pick is confirmed in **ADR 0008 (local model adapter)** after a
  small eval on the loop's actual prompts (proposing experiments, summarizing
  observations, drafting reflections).

The model only ever **proposes**. Every consequential effect is gated by
deterministic policy — see
[ADR 0005](adr/0005-models-propose-policy-authorizes.md).

### 3. Client: CLI first

The existing `endora-cli` becomes the **reference client**, speaking the real
protocol. Swift/SwiftUI (the stated first native client) is a **1.x fast-follow**,
not a v1.0 blocker. This is the fastest path to a genuinely usable release.

## Workstreams

Each item lists what "done for v1.0" means. None of these put HTTP, storage, or
model concepts into the Domain layer.

1. **Slice definition & domain modeling** — model the goal/assumption/experiment/
   observation/reflection concepts in `endora-domain` (pure), test-first.
   → ADR 0006.
2. **Persistence** — SQLite behind application-defined ports, in a new
   infrastructure crate, with migrations. → [ADR 0004](adr/0004-sqlite-first.md).
3. **Protocol surface** — versioned HTTP + JSON in the node, OpenAPI-described,
   with SSE for live updates. Introduces the async runtime + HTTP stack.
   → ADR 0007 (async/HTTP stack).
4. **Policy & Consent boundary** — deterministic authorization driven by
   `AutonomyLevel`; every proposal routed through it; independently tested.
   → [ADR 0005](adr/0005-models-propose-policy-authorizes.md).
5. **Model integration** — local open-weights model behind a replaceable
   OpenAI-compatible adapter, sitting *behind* the policy boundary. → ADR 0008.
6. **Memory rights** — slice data is visible, correctable, **exportable, and
   deletable**. Non-negotiable for 1.0.
7. **Audit & Accountability** — record what was proposed, what policy decided,
   and what executed, for consequential actions.
8. **CLI reference client** — drive the full loop from `endora-cli` over the
   protocol.
9. **Release engineering** — native binaries (macOS, Linux/Ubuntu Server) +
   Docker, SemVer commitment, `CHANGELOG.md`, a security-review pass, and a real
   supported-versions entry in [SECURITY.md](../SECURITY.md).

## Sequencing (indicative)

- **Phase 0 — decide.** ADR 0006 (slice), ADR 0007 (async/HTTP), ADR 0008 (model
  adapter). No feature code before the slice is specified.
- **Phase 1 — data skeleton.** Domain modeling + SQLite persistence, test-first.
- **Phase 2 — end-to-end thin path.** Protocol + CLI reading/writing the slice,
  no model, no policy yet — a walking skeleton.
- **Phase 3 — the boundary.** Policy & Consent + Audit made real and tested.
- **Phase 4 — reasoning.** Local model adapter proposing *through* the boundary.
- **Phase 5 — rights & release.** Memory export/delete, hardening, packaging,
  security review, tag v1.0.

## New dependencies expected (each justified per CONTRIBUTING)

v1.0 is the "actual first vertical slice" that justifies dependencies
intentionally deferred during setup. Expected additions, none in `endora-domain`:

- Async runtime + HTTP server/client (Tokio/Axum-class) — protocol surface.
- SQLite driver (e.g. `rusqlite` or `sqlx`) — persistence.
- Serialization (Serde/JSON) and OpenAPI tooling — protocol contracts.
- An HTTP client for the local model's OpenAI-compatible endpoint.

Each significant choice gets a rationale and, where load-bearing, an ADR.

## Explicitly out of scope for v1.0

Swift/SwiftUI, Android, and web clients; cloud sync; multiple/cloud model
providers; MCP; microservices; event sourcing. These remain post-1.0.

## v1.0 exit criteria

- [ ] Full learning loop works for one goal, driven from the CLI.
- [ ] A local open-weights model proposes; deterministic policy authorizes.
- [ ] No model output reaches a consequential action without policy approval
      (tested).
- [ ] Slice data is exportable and deletable (memory rights).
- [ ] Consequential actions are audited.
- [ ] Versioned protocol with an OpenAPI description.
- [ ] Runs on macOS, Linux/Ubuntu Server, and Docker.
- [ ] fmt, clippy (`-D warnings`), and tests green in CI on Linux and macOS.
- [ ] ADRs 0006–0008 written; `CHANGELOG.md` and SECURITY support entry in place.

## Tracking

This roadmap is mirrored by a **v1.0 milestone** with one issue per workstream.
The roadmap describes intent; the issues carry the work.
