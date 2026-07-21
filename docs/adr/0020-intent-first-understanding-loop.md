# 0020 — Intent-first: the autonomous understanding loop (direction reset)

## Status

Accepted (2026-07-20). **Supersedes the goal-centric framing** of earlier ADRs where it
conflicts. The canonical statement of direction is
[docs/direction-reset.md](../direction-reset.md); this ADR records the decision and its
architectural consequences. The invariant of
[ADR 0005](0005-models-propose-policy-authorizes.md) / [ADR 0010](0010-autonomy-model.md)
(*models propose, policy authorizes*) is reaffirmed, not changed.

## Context

Endora drifted into a **goal tracker**: the person creates
Direction→Target→Assumption→Experiment→Observation→Reflection and the model summarizes
afterward. That inverts the point — the person is doing the cognitive work Endora exists
to do. We are resetting: Endora is an **autonomous personal intelligence** whose primary
job is to **understand intent** and safely act in the person's interest. Goals become one
*optional* expression of intent, not the foundation.

## Decision

### 1. Understanding is the foundation, not Goals

Introduce a first-class **Understanding** model: a living set of **beliefs** about the
person (intent, values, preferences, patterns, motivations, frustrations, stressors,
energy, relationships, decision styles). Every belief carries **evidence, a confidence,
a timestamp, and can be corrected or expire**. Nothing is permanent. This — not Goals —
is what the home surface is about ("what does Endora understand?").

### 2. Endora drives the loop

The loop is Endora's, not the person's: observe → build understanding → infer intent →
form hypotheses → identify opportunities → propose/perform safe **interventions** →
observe outcomes → ask for **lightweight feedback** → update understanding. The person
mostly **reviews understanding** and gives light feedback; they rarely manage objects.

### 3. Beliefs vs. interventions — different authority

- **Beliefs** are Endora's internal model, not actions. Endora **forms them itself**
  (with evidence + confidence); the person **reviews and corrects** them (lightweight
  feedback), and they **expire**. This is *not* gated by per-item confirmation — that
  friction is exactly what we are removing.
- **Interventions** are actions in the world. These **stay** behind the boundary:
  models propose, deterministic policy authorizes, capabilities execute, evidence
  verifies. Intervention size is **proportional to confidence** (more uncertainty →
  smaller/ask-first action).

### 4. Goals are demoted, not deleted

The existing Direction/Target/… machinery remains as an *optional* expression of intent
(reachable, but no longer the center). We do not rip it out — the reset is reversible
(this branch), and goals still serve people who want them.

### 5. Reflection is continuous and Endora's

Endora asks itself — did this help? was I wrong? should confidence change? should I ask?
should I stop? — rather than handing "please reflect" to the person.

### 6. Success metric

Not goals completed. Success = Endora understands the person more accurately over time,
helps with less effort, and the person keeps agency.

## Consequences

- New domain: **Belief** (statement, kind, confidence, evidence, timestamps, expiry,
  status) with its repository; the butler **emits beliefs** it forms each turn; an
  **Understanding** view becomes the home surface.
- The butler's prompt is reframed around **understanding intent and proposing small
  interventions**, not organizing goals.
- Interventions reuse the capabilities + autonomy machinery already built (ADR 0019).
- Goals/North Stars move to a secondary place; the persistent-suggestions inbox and
  check-ins become mechanisms in service of understanding, not goal management.
- Guiding test for every feature: *would Endora still feel like an autonomous personal
  intelligence without it?* If yes, it is probably just tooling.
