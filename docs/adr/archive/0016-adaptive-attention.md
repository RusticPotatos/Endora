# 0016 — Adaptive attention: ranking and deferral-backoff

## Status

Accepted (2026). Elaborates [ADR 0014](0014-the-butler-conversation-values-attention.md) §3.

## Context

The butler should bring the right things back to the person — due reviews, North
Stars drifting without a value, aims with no concrete target — *without* nagging
about everything. And when the person says "not now," it should ask **less**, not
keep pestering. [ADR 0014](0014-the-butler-conversation-values-attention.md) §3
set the shape (an attention ranking with deferral-backoff, reprioritizing on new
evidence); this ADR fixes the first, minimal computation.

The load-bearing constraint from ADR 0014: attention must serve the person's
**stated values, not engagement**. This is not a feed that maximizes interaction;
it surfaces what the person themselves would want surfaced, and it recedes when
they defer.

## Decision

Attention is a **read projection** computed on demand, plus a small amount of
**deferral state**. Nothing is pushed; the person (or the console) reads
`GET /v1/attention` and the butler draws on the same computation.

### What surfaces (the first, cleanly-computable set)

- **Due reviews** — an experiment whose scheduled review has arrived and is not
  concluded ([ADR 0011](0011-review-scheduling-reminders.md)).
- **Unfiled North Stars** — an active North Star not yet filed under a value
  ([ADR 0015](0015-identity-and-values-context.md)); the butler should ask what it
  serves.
- **Empty North Stars** — an active North Star with no active target under it yet.

These need no new timestamps. Time-based *staleness* (an item untouched for a
while) is a natural next signal but needs per-item activity timestamps; it is
deferred rather than half-built.

### Deferral-backoff (the point)

Each attention item can be **snoozed** ("not now"). A snooze records a count and a
time to stay hidden until; **each further snooze doubles the interval** (1 day, 2,
4, 8, … capped), so a repeatedly-deferred item asks less and less. Snoozed items
are suppressed from the ranking until their time passes. The person's implicit "not
now" is respected without the item being lost — the intent is never deleted, only
quieted, and it returns.

**Reprioritization** is automatic: because attention is computed fresh each time
from current state, resolving an item (scheduling the review, filing the North
Star, adding a target) removes it, and new due reviews or new unfiled North Stars
appear on their own. A future signal — new evidence raising an item's rank — slots
into the same computation.

### Not engagement optimization

The ranking is a fixed, inspectable ordering of *what the person set up* (their
reviews, their North Stars), snoozable by them. It has no notion of interaction
frequency, no reward for pulling the person back, and no hidden objective. That
invariant is a property of the computation, not a model's discretion.

## Consequences

- The butler becomes proactive — "it comes to you" — while staying quiet under
  deferral. Snooze is the person's control, and backoff makes it stick.
- New, small persisted state: a snooze per `(kind, subject)`. It is user-owned
  memory and is cleared by purge like everything else.
- Attention is computed, cheap, and transparent: a person can see *why* something
  surfaced and can always snooze it. No background scheduler pushes anything.
- The first item set is intentionally narrow. Widening it (time-based staleness,
  value-weighted priority, evidence-driven reprioritization) is additive and needs
  no protocol change.

## Alternatives considered

- **A fixed reminder schedule / surface everything.** Rejected: it ignores the
  person's "not now" and becomes noise — the exact failure this ADR exists to
  avoid.
- **Delete/complete an item on deferral.** Rejected: deferral is not resolution;
  the intent must return, just less often.
- **A learned/engagement-style ranker.** Rejected as a constitutional violation
  (ADR 0014): attention serves stated values, and that must be enforced by the
  computation, not left to a model.
- **Persist a full attention timeline with per-item activity timestamps now.**
  Deferred: the cleanly-computable set delivers the mechanic (including backoff)
  without that machinery; add timestamps when staleness is built.
