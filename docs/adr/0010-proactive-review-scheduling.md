# 0010 — Proactive review scheduling and the activity model

## Status

Proposed (2026). *(Awaiting review — this is the first decision that lets Endora
act on its own, so it is deliberately not self-adopted.)*

## Context

External review names the **retention mechanic** as the make-or-break, unproven
part of the product: the learning loop only closes if experiments actually get
**observed**, and "nobody comes back on their own." Closing that gap
(the 0.3 milestone) requires Endora to do something *proactive* for the first
time — which brushes directly against the constitution: *consequential actions
require explicit human authority*, and *models propose; deterministic policy
authorizes*. So the question is not "should it be proactive" but "what is the
**most** it may do on its own without crossing that line."

## Decision

**Endora may act proactively only to *notice and remind* — never to take a
consequential action.**

- **Review scheduling.** An experiment (and, later, a goal) may carry an optional
  *review-by* time. A background scheduler in the node periodically checks for
  due reviews and raises a **reminder**.
- **Reminders are notifications, not actions.** A due review adds an entry to the
  activity feed; it never auto-concludes an experiment, records an observation,
  approves a change, or mutates any domain state. This is the `Observe`/`Suggest`
  end of [`AutonomyLevel`](0005-models-propose-policy-authorizes.md): the system
  *informs*; the human *acts*.
- **Activity feed.** An append-only record of notable events (entities created,
  reviews due, policy decisions) — a **superset of the existing audit trail** —
  served at `GET /v1/activity`, with **server-sent events** for live updates. It
  is the surface a UI (or, later, a notifier) renders.
- **The scheduler holds no authority.** It reads state and emits reminders on an
  interval; it is not a capability executor. Consequential capabilities (the
  Capabilities and Protection contexts) stay out of scope and, when built, sit
  behind the deterministic policy boundary.

## Consequences

- Endora becomes proactive **without** violating the constitution: the only thing
  it does unprompted is *tell you something*, which is non-consequential and
  trivially reversible (dismiss it).
- The retention loop exists: users are pulled back to observe their experiments —
  the mechanic the review flags as unproven, now testable.
- New surface: a scheduler (a `tokio` interval task), a `review_by` field on the
  `Experiment` aggregate, and an activity feed + SSE. The audit trail becomes one
  *category* within the broader activity feed.
- The scheduler must be **idempotent** — a due review is surfaced once, not
  re-spammed each tick — and must **never act**. These are the load-bearing
  correctness properties and get direct tests.
- Delivery for 0.3 is **in-app only** (the feed + SSE in the web console).
  OS/email/push notifications are later and out of scope here.

## Alternatives considered

- **No scheduling — rely on the user to return.** Rejected: the review is blunt
  that this is exactly how tools like this die.
- **Autonomous action on a due review** (e.g. auto-conclude a stale experiment).
  Rejected outright: consequential action without human authority violates the
  constitution. A reminder is the only safe proactive behavior.
- **External notifications first (email/push).** Deferred: adds delivery
  infrastructure and dependencies; prove the mechanic in-app first.
- **A separate scheduler service.** Rejected: no microservices
  ([ADR 0001](0001-modular-monolith.md)); a background task in the node suffices.
