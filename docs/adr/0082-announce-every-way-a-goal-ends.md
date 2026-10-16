# 0082 — Announce every way a goal ends

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude).

**Superseded 2026-10-16 by [0100](0100-the-clean-slate.md).** Durable goals are removed.
Announcing every way unattended work ends is a rule the agenda loop is to keep.

## Context

The handoff review (finding 2) showed that a goal could run, settle as
`Partial`, and then vanish. The delivery queue and the publisher both selected
only `status = 'complete'`, and the goal list shows only active goals, so a
partial, blocked or failed outcome was stored but never told to the person. The
only observable difference between "stopped with half an answer" and "never
worked on" was an empty chat. The actual-worker acceptance test
`goal_worker_delivers_its_terminal_partial_result` reproduced this.

## Decision

Every way a goal stops on its own is announced: `complete`, `partial`,
`blocked` and `failed`. `GoalStatus::announced` is the one definition.
Cancellation is the person's own decision and is not announced. The SQL list
that restates it (`ANNOUNCED_GOAL_STATUSES`) is checked against the domain by a
test, so the two cannot drift.

`Goal::outcome_notice` builds the message from the goal's own record:

- A completed goal delivers its verified evidence unchanged, as before.
- Any other ending says so in its first line, then lists what was found (the
  most recent five findings), what is still missing (the recorded reason) and
  what did not work (the most recent three dead ends). An ending with no
  findings says it found nothing usable rather than implying some. Live
  evaluation showed this first wording to be unhelpful in places; ADR 0084
  refines which reasons and failures are shown and how `Partial` reads.
- Length is bounded by the notice itself (3,000 characters, with per-line
  bounds), because the chat only refuses an empty message.

Publication keeps its existing atomic, exactly-once transaction. The method
and column names (`publish_goal_completion`, `completion_published`) are
unchanged to avoid churn in persisted schema and call sites; they now mean "the
outcome was published".

## Validation and limits

Unit tests cover the wording per status, the bound, and which statuses are
announced. A storage test covers queueing and exactly-once publication for
partial, blocked and failed goals, and refusal for cancelled and running goals.
The foundation gate's R04 evidence now also requires an exhausted goal to be
queued for delivery with its reason in the notice. The actual worker test
requires one correctly worded message that is not repeated on a later tick.

Not covered here: a persistent link from the notice to the stored evidence,
terminal history in the console list, durable progress and cancellation in
chat, and a real-model recovery evaluation. The delivery test runs in the node
crate, so it does not count toward the foundation gate, which executes only the
infrastructure `foundation_gate` target.

## Amended by 0094 (2026-10-15)

Everything above applies to goals the person asked for. Goals Endora starts for itself
([ADR 0094](0094-work-it-finds-for-itself.md)) are not announced in chat when they end:
they are written to the activity trail and folded into the next brief, and reach chat
only when they need the person, so that always-working does not become always-talking.
