# Endora Roadmap

> Status: living document. This roadmap states **intent and sequencing**; it is
> not authoritative for decisions. Architectural decisions are recorded in
> [ADRs](adr/README.md), and execution is tracked in GitHub issues/milestones.
> When this roadmap and an ADR disagree, the ADR wins.

## Where we are

The foundation is built and **v0.1.0 has shipped**: the full learning loop, the
deterministic policy boundary, audit, memory rights, a local model proposer, a
SQLite store, a node-served web console, review scheduling, and a live activity
feed all exist. Work since 0.1.0 (the console, retention, and reachability
milestones, plus the Goal→Target rename) sits on `develop`.

The next arc is **the butler** ([ADR 0014](adr/0014-the-butler-conversation-values-attention.md)):
turning the system from a tree the user operates into an assistant the user talks
to. That arc — releases **0.5 → 1.0** — is planned in
[The road to 1.0](#the-road-to-10--the-butler) below.

## What v1.0 means

> **One complete vertical slice of the learning loop, running locally, with the
> constitutional guarantees (deterministic policy boundary, memory rights, audit)
> actually enforced in code — not just documented.**

v1.0 is a real, usable, honestly-guaranteed release — not a general autonomous
agent. It ships the full learning loop for a single target, driven from the CLI,
with a local model proposing and deterministic policy authorizing.

## Scope decisions for v1.0

These were chosen deliberately. Where a decision is architectural it is promoted
to an ADR during the work (numbers below are the intended slots).

### 1. The slice: the full learning loop, one target

v1.0 implements the whole loop for a single user target:

```text
Direction → Assumption → Experiment → Observation → Reflection
          → Proposed process change → Human approval
```

This exercises every layer and every core guarantee end-to-end. It touches the
**Direction & Targets**, **Experiments & Learning**, and **Reflection** bounded
contexts, plus the cross-cutting **Policy & Consent**, **Memory**, and
**Audit & Accountability** contexts. Detailed entities/APIs are designed *with*
this slice — recorded in **ADR 0006 (first vertical slice)** — not invented
ahead of it.

### 2. Model: real, local, open-weights — behind the deterministic boundary

The **deterministic policy boundary is the load-bearing guarantee** and must be
fully real and independently tested. Behind it, v1.0 uses a **real local model**,
not a stub:

- **Default: Qwen3.5 9B** (~6.6 GB quantized) — fits 8 GB VRAM / 16 GB Apple
  Silicon, the sweet spot for the majority-hardware goal.
- **Lighter fallback: Qwen3.5 4B** (~3.4 GB quantized) for older laptops.
- Served locally through an **OpenAI-compatible endpoint** (e.g. Ollama or a
  llama.cpp server; MLX builds available on Apple Silicon), consumed via a
  **replaceable adapter** so the specific model and runner are never load-bearing.
  This keeps Endora local-first and privacy-preserving by default; larger models
  (e.g. Qwen3.6 27B) and optional cloud providers are post-1.0 adapters.
- The final pick and the model's **license compatibility with Apache-2.0** are
  confirmed in **ADR 0008 (local model adapter)**, after a small eval on the
  loop's actual prompts (proposing experiments, summarizing observations,
  drafting reflections).

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

1. **Slice definition & domain modeling** — model the target/assumption/experiment/
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

- [ ] Full learning loop works for one target, driven from the CLI.
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

---

## Post-0.1: toward a usable product

> The foundational vertical slice above shipped as the **v0.1.0** tag. The
> versions below stay in `0.x` until the constitution's full stewardship vision
> is reached.

v0.1.0 is a CLI against a local HTTP node. The gap to something a *non-author*
uses daily is **not** more domain features — it is a UI, one-command deployment,
and a mechanic that pulls people back to observe their experiments. These phases
target exactly that.

**Deliberately out of this roadmap:** native/mobile apps and offline multi-device
sync. The node is authoritative and clients are thin, so a responsive web console
reached from an **always-on** node covers "capture from my phone" without the
hardest unsolved problem (sync). Native apps are a far-future possibility, not a
plan.

### 0.2 — A usable product in one container

- The **node serves a self-contained web console at `/`** — same-origin, so no
  CORS and no separate front-end build. It exercises the whole loop (create/list
  Direction → … → Reflection), the propose → approve → decide policy flow, the
  audit trail, and export/purge. Static assets are embedded in the binary, so the
  container stays self-contained. See [ADR 0009](adr/0009-node-served-ui-and-single-container.md).
- **Single-container deployment**: `docker compose up` runs the node (UI + API +
  CLI all in the image) plus an optional local model — the whole system in one
  command.
- Small enabler: a `direction list` endpoint/command so buckets are discoverable.

Web-first is deliberate: it is the fastest path to *usable* and works on any
device with a browser. A native (Swift/SwiftUI) client is a later follow-up, not
a prerequisite.

### 0.3 — Retention: the loop that pulls you back

- **Scheduled review prompts** — "you said you'd check this experiment in two
  weeks." *Delivered* ([ADR 0011](adr/0011-review-scheduling-reminders.md)): an
  experiment can carry a scheduled review time, and the system surfaces reviews
  that are due (`GET /v1/reviews/due`, a console banner) without acting on them —
  Endora's first *proactive* behavior, and the first application of the autonomy
  model ([ADR 0010](adr/0010-autonomy-model.md)). Reviews are computed on read,
  not pushed by a background scheduler; live delivery comes next.
- An **activity feed** (`GET /v1/activity`) recording what happened, and
  **server-sent events** so the console reflects new activity — including due
  reviews — live. *Delivered*
  ([ADR 0012](adr/0012-activity-feed-and-change-stream.md)): the feed is a read
  projection over the persisted, timestamped facts (observations and audited
  decisions), and `GET /v1/activity/stream` pushes a `changed` signal after every
  write so the console refreshes live. The feed widens for free as more of the
  loop gains durable timestamps.

This is the make-or-break mechanic: the loop only closes if experiments actually
get observed. The most design care goes here.

### 0.4 — Reachable from where you live

- A **responsive** console usable from a phone browser. *Delivered*: a mobile
  breakpoint stacks forms and tightens the frame so the whole loop is usable at
  phone widths.
- Docs for running an **always-on** node (home server or a small VPS) and
  reaching it securely — e.g. Tailscale, or a reverse proxy that adds the
  authentication the API still lacks. This delivers "mobile capture" with no sync.
  *Delivered*: [docs/hosting.md](hosting.md) covers always-on supervision
  (systemd / container restart), backups, and secure reach over a private overlay
  or an authenticating proxy; `make docker-run` now binds loopback by default.

## The road to 1.0 — the butler

The original 1.0 slice (the learning loop, driven from the CLI and console) is
essentially built and shipping on `develop`. What makes Endora *the product* is the
**butler** ([ADR 0014](adr/0014-the-butler-conversation-values-attention.md)): you
talk to it, it organizes your life by your values, and it works the loop for you —
proposing, never deciding, with the policy boundary intact. These releases integrate
that one complete vertical slice at a time. (The Goal→Target rename is breaking, so
the release that carries it is **0.5.0**; tagging is a human decision.)

### 0.5 — Artifacts, complete and correct
- **Goal → Target rename** ([ADR 0013](adr/0013-rename-goal-to-target.md)) — a
  breaking rename of the second-tier concept; done, pending release.
- **Lifecycle**: North Stars and Targets gain states (active / achieved / abandoned
  / archived) plus archive and delete, across the domain, API, CLI, and console.
  Today only experiments can "conclude" and nothing else can be finished, dropped,
  or removed except a global purge — the butler needs to close things out.

### 0.6 — Values: organize by *why*
- Build the **Identity & Values** context (its own ADR): **Value → North Star →
  Target**. A North Star is filed under the value it serves (health, community,
  craft); the console groups by value and the API/CLI follow. This is the organizing
  backbone the butler files into. *Delivered*
  ([ADR 0015](adr/0015-identity-and-values-context.md)): a `Value` aggregate, an
  optional North Star → Value link (assigned, never inferred), full CRUD across
  domain/storage/API/CLI, and a console home grouped by value. Existing databases
  migrate cleanly (a new `values` table + nullable `value_id`).

### 0.7 — The butler, MVP (chat)
- A **conversation** surface: a chat endpoint and a console chat panel. The model
  runs the **act/ask loop** — you talk, it proposes structure and plans (North
  Stars, targets, experiments), asks clarifying questions, and records answers as
  **preferences**. Every consequential step still routes **propose → policy
  authorizes → confirm if irreversible**. This is where the AI becomes the driver
  rather than a single drafting call. The **anti-sycophancy eval harness** starts
  here — the moment the model drives.

### 0.8 — Attention & proactivity
- **Adaptive attention**: the deferral-backoff ranking (ADR 0014 §3) decides what to
  raise, with **hybrid triggers** — events (the existing change stream) + scheduled
  sweeps + conversation. The butler surfaces stale North Stars and due reviews and
  asks *less* as they are deferred, reprioritizing on new evidence. Its own ADR pins
  the attention formula. "It comes to you."

### 0.9 — Voice & character
- **Personality**: style mirroring with the golden-rule floor (ADR 0014 §4), and the
  candor / anti-sycophancy invariants hardened into evals. **Voice**: STT/TTS as a
  thin client over the same protocol. Its own ADR for persona + voice.

### 1.0 — The butler is the product
- Everything integrated: the person lives in the conversation, values organize the
  structure, attention is proactive and calibrated, and the butler is candid and
  never sycophantic — all local-first, memory rights intact, policy boundary
  enforced. 1.0 is redefined around this; the CLI learning loop was an early
  milestone, now passed.

Cross-cutting, in **every** release: the deterministic **policy boundary** authorizes
consequential actions, **memory stays visible / correctable / exportable / deletable**,
and **sycophancy is treated as a defect** measured by evals — never left to the
model's discretion.

### Beyond 1.0

Native clients, real offline sync (if it is ever justified), and the
**Capabilities** and **Protection** bounded contexts — *bounded, reversible*
autonomy executed under the Policy boundary — remain the long arc toward the
constitution's full vision. Deliberately after the butler is usable and trusted.
