# 0010 — Autonomy model: the act/ask loop and preferences

## Status

Accepted (2026).

## Context

Endora is meant to genuinely *act* for the person and get better over time — not a
passive tool, and not a reminder-bot. But it must never become an over-reaching
agent that redefines the person's life or takes irreversible action without
consent. The constitution already frames this (human autonomy final; consequential
actions require authority; models propose, deterministic policy authorizes;
improve processes more than values; least authority; reversibility). What is
missing is a concrete **autonomy model** that says *how* an acting, evolving
system stays inside those lines.

A useful intuition is a great steward or butler: intensely proactive, yet never
oversteps, always candid, and never stops being yours. This ADR translates that
character into a mechanism. (Any product-facing persona name — "assistant",
"steward", or none — is a UX choice and is deliberately **left open**; nothing
here depends on it.)

## Decision

The autonomy model has four parts.

### 1. The act/ask loop

- **Act** when it knows the person's relevant preference *and* the action is
  reversible / already pre-authorized.
- **Ask** a clarifying question when it is uncertain, the situation is ambiguous,
  or the action is consequential/irreversible.
- **Remember** every answer as a preference, so the same question is not asked
  twice.

This loop *is* "gets smarter over time": it asks a lot early and less as it learns
the person. "Learning" here is the accumulation of preferences, not opaque model
training.

### 2. Preferences are constraints — of two distinct kinds

- **Taste / style preferences** — *may be inferred* from observed patterns (e.g.
  "prefers mornings", "wants terse output"). They set defaults **within**
  already-permitted, reversible space. A wrong inference is cheap because that
  space is reversible and correctable.
- **Grants of authority** — *only ever explicitly stated* by the person (e.g.
  "you may send routine replies on your own"). They **expand** what the system may
  do alone.

The invariant: the system **may infer taste; it may never infer itself new
authority.** Self-expansion of authority is the primary failure mode this model
exists to prevent.

### 3. An autonomy dial, per capability

Each capability carries an [`AutonomyLevel`](0005-models-propose-policy-authorizes.md)
(Observe → Suggest → ConfirmEachAction → ActWithinPolicy). New capabilities start
low and are graduated **only by an explicit grant** (part 2). The deterministic
policy boundary — not the model — enforces the current level.

### 4. Invariants that never bend, at any level

These sit *above* preferences (a preference cannot switch them off), and they are
what keep the system a steward as it grows more capable:

- Honest — states evidence and uncertainty; no flattery, no engagement optimizing.
- Memory (including preferences) is visible, correctable, exportable, deletable.
- It may get better at serving the person's stated values/interests; it may
  **never decide** what those interests are.
- Consequential/irreversible actions always require consent.
- No self-expansion of authority; no hidden objectives; no dependency manipulation.

## Consequences

- The system can act autonomously and evolve while staying bounded and
  correctable — the reconciliation of "acts for me / gets smarter" with the
  constitution.
- Preferences become a first-class, visible, editable concept (a new domain area),
  and memory rights extend to them.
- It requires, over later phases, the **Capabilities** context (things it can do,
  each with an autonomy level and consent) and **Protection** (reversibility /
  proportionality guards) — both behind the policy boundary.
- The safest *first* application of this model is a **reminder** (an
  Observe/Suggest-level action that changes nothing): the 0.3 review-scheduling
  work becomes the first concrete step and gets its own, smaller ADR under this
  one.

## Alternatives considered

- **A pure tool that never acts.** Rejected: the goal is a steward that acts, not
  a database with forms.
- **An autonomous agent that infers goals and acts on them.** Rejected outright:
  it redefines the person's interests, self-expands authority, and acts
  unaccountably — the exact failure mode the constitution forbids.
- **Ask before every action, forever.** Rejected: does not scale and is not a
  steward. A butler who asks permission to draw the curtains is useless. The
  act/ask loop plus learned preferences is the middle path.
