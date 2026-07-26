# 0011 — Review scheduling: the first act of the autonomy model

## Status

Accepted (2026).

## Context

[ADR 0010](0010-autonomy-model.md) adopted the autonomy model — the act/ask loop,
preferences as constraints, an autonomy dial per capability, and invariants that
never bend. It named its own first application: *"the safest first application of
this model is a **reminder** (an Observe/Suggest-level action that changes
nothing): the 0.3 review-scheduling work becomes the first concrete step and gets
its own, smaller ADR under this one."* This is that ADR.

The learning loop already lets a person run experiments against their assumptions
([ADR 0006](0006-first-vertical-slice.md)), but nothing brings a running
experiment back to their attention. Retention — the review's unproven risk — needs
the system to *resurface* work at the right time without deciding anything for the
person.

## Decision

**An experiment may carry a scheduled review time; the system surfaces reviews
that are due and takes no further action.**

- The domain `Experiment` gains an optional `review_by` timestamp, a
  `schedule_review(at)` command, and `is_review_due(now)` — a review is due when
  its time has arrived **and** the experiment is not concluded. Concluding closes
  it; no reminder outlives it.
- Persistence stores the due time as a nullable `review_by_ms` column, added to
  existing databases by a forward migration (a guarded `ALTER TABLE ... ADD
  COLUMN`), so upgrading never loses or breaks data.
- The application exposes `schedule_experiment_review` (schedule *N* days out via
  the `Clock` port) and `list_due_reviews` (what is due as of now). Both the CLI
  (`experiment review <id> <days>`, `reviews due`) and the web console (a
  "Remind me" control per experiment and a due-review banner) drive them.

Why this fits the autonomy model:

- **It only reminds; it never acts.** Surfacing a due review changes no state and
  makes no decision — it sits at the `Observe`/`Suggest` end of the dial
  ([ADR 0005](0005-models-propose-policy-authorizes.md)). The person still decides
  what to do with the experiment.
- **The person sets the schedule explicitly.** The system does not infer *when* to
  review or *what* to conclude; it stores a time the person asked for. This
  respects "may infer taste, never infer authority."
- **It is fully reversible and correctable.** A review can be rescheduled or
  superseded at any time, and the due time is visible and exportable like all
  other memory.

## Consequences

- The system takes its first step from a passive record toward a steward that
  brings work back at the right time — the concrete beginning of ADR 0010.
- No new authority is introduced: no capability graduates on the autonomy dial,
  and no scheduler acts on the person's behalf. Reminders are computed on read
  (`list_due_reviews`), not pushed.
- A later phase can add an **activity feed** and **push/SSE** delivery of due
  reviews; those are delivery mechanisms over the same read model and do not
  change this decision.
- Time is a domain input via the `Clock` port, so "due" is deterministic and
  testable, consistent with the rest of the core.

## Alternatives considered

- **A background scheduler that concludes or advances experiments when due.**
  Rejected: that is the system *acting* on a consequential lifecycle transition
  without consent — exactly what ADR 0010's invariants forbid for a first step.
- **Reminders as a generic notification entity, decoupled from experiments.**
  Rejected as premature: the only thing worth reviewing today is an experiment.
  A general reminders/capabilities model can come with the Capabilities context
  ADR 0010 anticipates, once there is more than one thing to remind about.
- **Computing "due" on the client from the export snapshot only.** Rejected as the
  source of truth: the node owns "due" (`GET /v1/reviews/due`) so every client —
  CLI, console, future clients — agrees. The console still derives its banner from
  the snapshot for convenience, but the authoritative query is server-side.
