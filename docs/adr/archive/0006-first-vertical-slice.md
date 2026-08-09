# 0006 — First vertical slice: the learning loop for one goal

## Status

Accepted (2026).

## Context

The roadmap (`docs/roadmap.md`) sets v1.0 as *one complete vertical slice of the
learning loop, running locally, with the constitutional guarantees enforced in
code*. Our architecture docs deliberately defer entity and API design until a
slice is selected ([architecture.md](../../architecture.md),
[domain-map.md](../../domain-map.md)). This ADR selects that slice and names — at a
high level only — the concepts it introduces. Field-level schemas and the
OpenAPI contract are defined in the implementing PRs, not here.

## Decision

The first vertical slice is the **full learning loop for a single user goal**:

```text
Direction → Assumption → Experiment → Observation → Reflection
          → Proposed process change → Human approval
```

**Bounded contexts touched:** Direction & Goals, Experiments & Learning,
Reflection (primary); Policy & Consent, Memory, Audit & Accountability
(cross-cutting). No other contexts are built for v1.0.

**Core concepts introduced** (one line each; these live in `endora-domain` as
pure types, modeled test-first):

- **Direction** — the user's stated direction / North Star context for a goal.
- **Goal** — a single intentional objective under a Direction.
- **Assumption** — a belief the user holds about the goal, made explicit.
- **Experiment** — a small, bounded test of an assumption, with a status.
- **Observation** — recorded results/evidence from an experiment.
- **Reflection** — a retrospective over observations.
- **ProposedProcessChange** — a change a Reflection proposes; it is *proposed*,
  never auto-applied, and requires human approval through the policy boundary.

**Primary use cases (v1.0):** capture a Direction and Goal; state Assumptions;
design and run an Experiment; record Observations; write a Reflection; and have
the model *propose* the next step or a ProcessChange that the human approves or
rejects. Every state-changing step that a model proposes passes through the
deterministic **Policy & Consent** boundary
([ADR 0005](0005-models-propose-policy-authorizes.md)); consequential outcomes
are recorded by **Audit**; slice data is subject to **Memory** rights (export +
delete).

**Protocol surface (high level):** versioned REST-ish resources for the concepts
above, with SSE for live updates; the concrete OpenAPI contract is authored in
the protocol PR ([ADR 0007](0007-async-web-stack.md)).

## Consequences

- Every layer and every core guarantee is exercised end-to-end by one coherent
  feature, rather than broad shallow scaffolding.
- The domain gains its first real types; because the slice is fixed, they are not
  speculative.
- Anything outside this loop (values management UI, multiple goals dashboards,
  scheduling, notifications) is explicitly out of scope for v1.0 and waits for a
  later slice.
- The concepts here set naming precedent for their contexts; renames later cost
  churn, so the vocabulary is chosen deliberately.

## Alternatives considered

- **A thinner slice** (capture North Star + one experiment only) — rejected:
  chosen per roadmap decision to prove the *whole* loop in v1.0.
- **Multiple goals / full Direction & Goals management up front** — rejected:
  broadens scope without exercising more of the stack.
- **Design all entities across all contexts now** — rejected: violates
  "no detailed entities before the slice"; invites speculative modeling.
