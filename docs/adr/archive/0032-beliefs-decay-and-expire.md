# 0032 — Beliefs decay and expire

## Status

Accepted (2026-07-25). Implements a guarantee
[0020](0020-intent-first-understanding-loop.md) and
[docs/direction-reset.md](../../direction-reset.md) both make but no code kept.

## Context

The direction reset says understanding is a **living model**: *"Every belief carries
evidence, confidence, a timestamp, the ability to be corrected, and the ability to
expire. Nothing is assumed permanently."*

`BeliefStatus::Expired` existed. It had a name, it round-tripped through storage, it
parsed back. **No code path ever set it.** A belief became `Corrected` only if the
person said it was wrong; otherwise it was held at its original confidence forever.

Since [0029](0029-delete-the-goal-tracker.md) made understanding the *only* model
Endora keeps of a person, that gap stopped being cosmetic:

- The butler's context is assembled from active beliefs, so every stale belief is
  permanent noise in every future turn.
- `nightly_focus` researches the **highest-confidence intent**, so a year-old guess
  that happened to be recorded as `high` outranks something learned last week.
- Endora would keep acting on "you're stressed about the move" indefinitely after
  the move — which reads less like memory and more like not listening.

It also quietly violated the constitution's honesty clause (§4, *"must not present a
guess as a fact"*). A confidence recorded once and never revisited stops describing
how sure Endora actually is the moment the evidence ages.

## Decision

**Beliefs weaken without reinforcement, and fade out entirely when they get too
weak.**

- Each `BeliefKind` declares a **half-life**. Confidence steps down one level per
  elapsed half-life since the belief was last affirmed; below `Low` it is no longer
  held at all.
- **The rates encode ADR 0020's own claim about people.** Intent and values change
  slowly (365 days) and are what Endora is really modelling. A frustration or
  stressor is usually about a particular week (45 days). Preferences and patterns sit
  between. This is the one place the "intent is slow, goals are fast" thesis becomes
  executable rather than prose.
- **Decay is derived, not stored.** `confidence_at(now)` is a pure function of the
  stored timestamp, so understanding is honest the instant it is read, whether or not
  any background job has run. There is no cache to drift.
- **Affirming resets the clock.** The person saying "that's right" makes a belief
  current again — which is what makes the whole thing correctable rather than a
  countdown they cannot influence.
- **The nightly loop persists it.** `expire_faded_beliefs` marks faded beliefs
  `Expired` and reports each one to the activity trail. Read-side filtering already
  hides them, so this changes no behaviour — it makes the forgetting **durable and
  visible**, which is what memory rights require of anything Endora holds
  (constitution §6). Endora should be able to show you what it let go of, and when.
- **`Expired` stays distinct from `Corrected`.** "You were wrong about me" and "that
  stopped being true" are different facts about a person, and collapsing them would
  lose the difference.
- **The export reports stored confidence; the live view reports decayed.** The memory
  right is to see what Endora actually holds, so the export must not reinterpret the
  record. The console shows the current reading, because presenting a year-old `high`
  as current is the dishonesty this ADR exists to fix.

## Consequences

- Understanding stops being append-only. It can now shrink, which is the point: a
  model that only grows is a log, not a model.
- Old beliefs quietly leave the butler's context, so prompts get *smaller* over time
  rather than monotonically larger — a real benefit on a local model.
- **Cost:** Endora will sometimes forget something true that simply went unmentioned
  for a year. Deliberate. Forgetting something true is a smaller harm than
  confidently asserting something stale, and the person can affirm anything to keep
  it. The rates are set generously for exactly this reason.
- **Risk:** the half-lives are asserted, not measured. They encode a plausible theory
  of how fast different things about a person change, and nothing yet validates it.
  They are one `const fn` and can be revised once there is evidence.
- No migration. Decay is computed from `last_affirmed_at`, which every stored belief
  already has; existing beliefs simply begin ageing from their recorded timestamp.

## Alternatives considered

- **A single half-life for everything.** Simpler, and rejected: it would treat "you
  want to travel in retirement" and "you're stressed about this week's deadline" as
  equally durable, which is precisely the distinction ADR 0020 is built on.
- **Decay by a continuous score rather than discrete steps.** More expressive, but
  `Confidence` is a deliberately small human scale (low/medium/high) shown directly
  to the person and used to size interventions. Introducing a hidden float behind it
  would make the displayed value and the real value diverge.
- **Delete faded beliefs outright.** Rejected: the person could no longer see that
  Endora had held something, and audit/export would silently lose history. `Expired`
  keeps the record inspectable.
- **Let the model decide what is stale.** Rejected on the same grounds as ADR 0030's
  scoring: a model judging its own memory is unauditable and non-deterministic, and
  this is a rule that can simply be written down.
