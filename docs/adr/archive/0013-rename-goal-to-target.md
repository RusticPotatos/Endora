# 0013 — Rename the second-tier concept from Goal to Target

## Status

Accepted (2026).

## Context

The learning-loop hierarchy is: a **North Star** (the domain type `Direction`),
and beneath it the mid-level concept that assumptions hang from, historically
named **Goal** ([ADR 0006](0006-first-vertical-slice.md),
[domain-map](../../domain-map.md)).

In use this reads awkwardly. A North Star is *itself* goal-flavored — someone sets
"get back into running" as their North Star — so the level beneath it, when also
called a "Goal", reads as two goals stacked and blurs the vocabulary. The thing
beneath a North Star is really a **specific, measurable outcome you aim to hit**:
"run a 5k without stopping". That is a *target*, not a second goal.

Endora is a domain-driven project that deliberately keeps one **ubiquitous
language** across the UI, the protocol, and the domain. So the fix is not a UI
relabel but a rename of the concept everywhere.

## Decision

**Rename the concept `Goal` → `Target` end-to-end.** A **Target** is a concrete,
measurable outcome under a North Star; it rests on **assumptions**, which are
tested by **experiments**, which yield **observations**, distilled into
**reflections**. The other ladder terms are unchanged.

The rename is applied through every layer:

- **Domain**: `Goal` → `Target`, `GoalId` → `TargetId`; `Assumption` now references
  a `target`.
- **Application**: `GoalRepository` → `TargetRepository`; use cases and the memory
  snapshot use `target`.
- **Protocol** (breaking): `POST/GET /v1/directions/{id}/goals` →
  `/v1/directions/{id}/targets`; `/v1/goals/{id}/…` → `/v1/targets/{id}/…`; the
  `goal_id` JSON field → `target_id`.
- **Clients**: the CLI `goal` command → `target`; the web console labels
  ("Targets under …", "Add target").
- **Storage**: the `goals` table → `targets` and `goal_id` columns → `target_id`.
  Existing databases are migrated in place on open (rename table + columns, no data
  loss); a fresh database is created with the new names.

This is a breaking protocol change. It is acceptable now because Endora is
**pre-1.0** (see the compatibility rule in the working agreements), the only
clients are the in-repo CLI and console (both updated), and fixing the language is
far cheaper before 1.0 than after.

## Consequences

- One consistent word across UI, API, domain, and storage — the ubiquitous
  language holds, and the North Star / Target distinction is unambiguous.
- Any external client written against `…/goals` or `goal_id` must update. Within
  the repo, the CLI and console are updated in the same change.
- Existing databases migrate automatically and keep their data; the migration is
  a no-op on fresh or already-migrated databases and is covered by a test.
- Earlier ADRs (e.g. [0001](0001-modular-monolith.md),
  [0006](0006-first-vertical-slice.md)) still say "Goal". They are immutable
  historical records; this ADR supersedes the **terminology**, and the living docs
  (domain-map, architecture, README, roadmap) are updated to "Target".

## Alternatives considered

- **Keep "Goal".** Rejected: it is the source of the ambiguity — two goal-flavored
  levels stacked.
- **Relabel only the UI to "Target", leave the API/domain as "Goal".** Rejected:
  it breaks the ubiquitous language and leaves clients and storage speaking a
  different word than users.
- **Add a tier: North Star → Goal → Target.** Rejected: a third level is more
  structure than the loop needs; the two-tier North Star → Target model is what
  people actually use.
