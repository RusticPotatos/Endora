# 0015 — The Identity & Values context: a "why" above North Stars

## Status

Accepted (2026). Elaborates [ADR 0014](0014-the-butler-conversation-values-attention.md) §2.

## Context

The learning-loop hierarchy is North Star (`Direction`) → Target → Assumption →
Experiment. But North Stars currently float without their *why*. A person says "I
want to get back into running"; the reason — health, or community (a running
group, almost therapy), or the craft of it — is what actually makes the North Star
matter, and it is how a life organizes. [ADR 0014](0014-the-butler-conversation-values-attention.md)
placed a **Values** layer above North Stars and named the **Identity & Values**
context (already drawn in the [domain map](../domain-map.md)) as its home. This ADR
fixes how that context is modeled for its first, minimal build.

## Decision

Add a **`Value`** aggregate — a durable theme the person cares about (health,
community, craft) — as the top of the hierarchy: **Value → North Star → Target**.

- A `Value` has an id and a name. It is deliberately minimal: a value is an
  enduring category, not a task, so it has **no achieve/abandon lifecycle** (unlike
  Targets). It can be deleted, guarded the same way as the rest of the tree
  (refused while North Stars still reference it; archiving/re-filing first).
- A **`Direction` (North Star) gains an optional `value` link.** Optional because
  existing North Stars have none and because the person (or the butler, by asking)
  assigns the *why* — the system never invents it. A North Star with no value is
  valid and simply "unfiled".
- The link points **upward** (North Star → Value), matching the existing pattern
  (Target → Direction, Assumption → Target): the child names its parent.

Everything else follows the established layering: a `ValueRepository` port, a
`values` table plus a nullable `value_id` column on `directions` (added to existing
databases by the standard forward migration), use cases to create/list/delete
values and to assign a value to a North Star, protocol endpoints, CLI verbs, and a
console that **groups North Stars under their value**.

Why this shape:

- **The value is the organizing key.** Grouping North Stars by value is the
  DDD-by-what-you-care-about organization the person asked for — the same
  domain-first move we used for the codebase, applied to a life.
- **Optional, never inferred.** Assigning a North Star to a value is a user
  decision (the butler may *ask*, per ADR 0014, but never *infer* it). The optional
  link keeps the system honest and backward-compatible.
- **Minimal now.** No value lifecycle, no nested values, no per-value policy. Those
  can come if a real slice needs them; today the job is only to give North Stars a
  home.

## Consequences

- North Stars organize under a *why*; the console can present a life by value
  rather than a flat list of directions — the backbone the butler files into.
- A new top-level context (`Identity & Values`) exists in code, matching the domain
  map. It is small and behind the same layering; it introduces no new authority.
- Existing databases migrate cleanly: `values` is created and `directions` gains a
  nullable `value_id`; every existing North Star reads back as "unfiled" (null).
- Deletion stays guarded and reversible-friendly: a value in use cannot be deleted
  until its North Stars are re-filed, mirroring the 0.5 lifecycle guards.

## Alternatives considered

- **Fold "value" into the North Star as a tag/string.** Rejected: a value is a
  shared, reusable category multiple North Stars point at; a first-class aggregate
  lets the console group by it and the butler reason over it.
- **Make the value link required.** Rejected: it would break existing North Stars
  and force a categorization the person may not have made yet. Optional + "the
  butler asks" fits the autonomy model.
- **Give values the same achieve/abandon lifecycle as Targets.** Rejected as
  premature: you do not "achieve" health. Values may gain archiving later if a slice
  needs it; today deletion (guarded) is enough.
- **A deeper hierarchy (values → sub-values → North Stars).** Rejected: more
  structure than the person needs now; the two-tier Value → North Star is what was
  asked for.
