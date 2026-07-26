# Architecture Decision Records

An **Architecture Decision Record (ADR)** captures a single significant
architectural decision: its context, the decision itself, the consequences, and
the alternatives that were considered. ADRs are short, immutable once accepted,
and numbered in sequence.

## What they add up to

Forty-nine decisions, and a handful of ideas underneath them. Read this before reading
any individual record; each line is a rule the code actually follows, with the decision
that established it.

### The spine

- **Models propose; deterministic policy authorizes** ([0005](0005-models-propose-policy-authorizes.md)).
  Every autonomy question in this repository reduces to this one. A model may suggest,
  phrase, or interpret; it is never the enforcement boundary.
- **Deny by default, banded by reversibility** ([0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md)). What a
  capability can do unattended depends on whether its effect can be taken back — not on how
  confident anything is.
- **Deterministic autonomy is still autonomy** ([0044](0044-policy-acts-on-what-it-has-established.md)).
  Where a finding is arithmetic over stored records and the action is reversible and
  announced, policy acts without asking. All three conditions are load-bearing; drop one
  and it becomes a model acting on a guess.

### What we learned the hard way about models

- **Prompting is not where a guarantee goes** ([0037](0037-disclosure-not-persuasion.md),
  [0034](0034-evidence-verifies.md)). This model obeys an explicit, direct instruction about
  verification roughly one run in three. Fourteen consecutive times it failed to copy a name
  that was in its context. Anything that must be true belongs in code.
- **The interface discloses; the reply is not the record** ([0037](0037-disclosure-not-persuasion.md)).
  What was done is shown regardless of what the model says about it, and a turn that changed
  nothing says so.
- **Claim and observation are stored apart and never reconciled** ([0034](0034-evidence-verifies.md),
  [0035](0035-outcomes-what-happened-after-acting.md)). A tool reporting success while
  nothing changed is the failure the record exists to catch; merging the two would hide it.

### What we learned the hard way about other people's services

- **Mechanisms, not per-integration patches** ([0038](0038-capability-profiles.md)). Six
  hardcodes and a leak into the protocol adapter came from fixing one integration at a time.
  Genuine quirks are allowed — behind that integration's own named boundary, never in shared
  code.
- **Ranked trust: confirmed > observed > declared** ([0038](0038-capability-profiles.md)).
  What the person said beats what Endora saw, which beats what the server claims about
  itself. A server announcing "I only read" is not evidence of anything.
- **A tool surface is a product decision, not the service** ([0042](0042-direct-reach-into-a-service.md)).
  Where Endora is given the service's own interface it uses it: things have ids there, and an
  id cannot be mis-matched.
- **Recovery only, bounded, disclosed** ([0039](0039-capability-repair-proposals.md),
  [0041](0041-searching-the-reading-for-the-real-target.md)). Repair fires after a failure so
  it can never hijack a working call, is capped, and says what it substituted.
- **Never widen a call while recovering** ([0041](0041-searching-the-reading-for-the-real-target.md)).
  Written after argument hygiene turned one aimed-at-nothing call into every light in the
  house.

### What we learned about the person's side

- **Derived, never stored** ([0039](0039-capability-repair-proposals.md)). Findings are
  computed on read, so there is no queue to groom and nothing to dismiss — the structural
  version of the promise [0029](0029-delete-the-goal-tracker.md) made by deletion.
- **Answering is the dismissal.** A finding whose answer has been given stops being derived.
- **What it changed outside itself is kept and reversible** ([0045](0045-an-undo-log-for-what-it-changed.md)).
  A prior value nobody stores is not a reversibility story. That log survives "forget
  everything", because deleting a receipt does not undo the change.
- **Understanding decays** ([0032](0032-beliefs-decay-and-expire.md)) and admits only what it
  should ([0033](0033-what-understanding-admits.md)).

### How we work

- **Test against real data, not tidy fixtures.** Four corrections to one rule shipped and
  failed live in a single evening, every one because the tests used a five-line reading
  written by hand. The real 5KB reading — one unbroken line, forty entities with overlapping
  names — is now a fixture, and it catches what synthetic ones cannot.
- **Deploy, then read production.** Two features passed CI and could not work at all: one
  added a column existing databases never re-run, one added a table production never
  creates. Both were found by exercising the endpoints on the running system.
- **Count the rules.** Four signals now decide which thing a person meant. Each came from an
  observed failure and each has a test — and a fifth would be evidence that the instrument is
  wrong, not that it needs another refinement.

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
| [0033](0033-what-understanding-admits.md) | What understanding admits: instructions out, contradictions kept apart | Accepted |
| [0034](0034-evidence-verifies.md) | Evidence verifies: an unobserved effect is never reported as fact | Accepted (amended by 0037, 0038) |
| [0035](0035-outcomes-what-happened-after-acting.md) | Outcomes: what happened after Endora acted | Accepted |
| [0036](0036-durable-intentions.md) | Durable intentions: work that outlives a turn | Accepted |
| [0037](0037-disclosure-not-persuasion.md) | Disclosure, not persuasion: an unverified action is always visible | Accepted |
| [0038](0038-capability-profiles.md) | Capability profiles: learning what a tool does, instead of patching each one | Accepted |
| [0039](0039-capability-repair-proposals.md) | Capability repair proposals: noticing its own tooling is wrong | Accepted |
| [0040](0040-withdrawing-a-capability-that-never-works.md) | Withdrawing a capability that never works | Accepted |
| [0041](0041-searching-the-reading-for-the-real-target.md) | Finding the thing the person meant | Accepted (consolidates 0046–0048) |
| [0042](0042-direct-reach-into-a-service.md) | Direct reach into a service | Accepted |
| [0043](0043-writing-names-back-into-the-service.md) | Writing names back into the service | Accepted |
| [0044](0044-policy-acts-on-what-it-has-established.md) | Policy acts on what it has established | Accepted |
| [0045](0045-an-undo-log-for-what-it-changed.md) | An undo log for what it changed | Accepted |
| [0046](0046-a-made-up-category-is-part-of-the-target.md) | A made-up category is part of the target | Superseded by 0041 |
| [0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md) | A thing and its diagnostics are not two things | Superseded by 0041 |
| [0048](0048-the-tightest-match-wins.md) | The tightest match wins | Superseded by 0041 |
