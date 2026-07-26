# 0036 — Durable intentions: work that outlives a turn

## Status

Accepted (2026-07-25). Builds on [0035](0035-outcomes-what-happened-after-acting.md)
and the nightly loop of [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md).
Constrained by [0029](0029-delete-the-goal-tracker.md), whose lesson this ADR is mostly
about not re-learning.

## Context

Nothing Endora does survives a turn. `run_tool_turn` is capped at three to six rounds
and reseeds from chat history every time; the nightly loop picks a focus belief, looks
into it for one night, writes a note, and starts from nothing the next night. If it was
half-way through understanding something at 03:00, that is where the thought ends.

This is the difference between a butler that *looks things up* and one that *is working
on something for you*. Every remaining item on the roadmap needs it. Proportional
interventions need an action to be a step in something rather than a one-off. The
capability ladder and self-managed infrastructure — the WALL-E end of the vision — are
by definition multi-night work.

**The obvious design is the one that already failed.** A record with a title, a status
and a next step, that the person can add to and has to keep tidy, is a task list. Endora
had one; [ADR 0029](0029-delete-the-goal-tracker.md) deleted it, because the user was
doing the cognitive work the product exists to do. Any ADR that reintroduces persistent
units of work has to say precisely why this is not that, in terms that can be tested
rather than promised.

## Decision

**An `Intention` is Endora's own working memory: what it is currently pursuing on the
person's behalf, why, and where it got to.** Four constraints separate it from a task
list, and each is enforced in code rather than asserted here.

### 1. The person cannot create one

There is no endpoint and no console affordance to add or edit an intention. Endora forms
them, from what it understands. The only verb the person has is **drop it** — that is
their authority over boundaries ([direction-reset](../../direction-reset.md), "human
authority"), and it is the whole of their side of the interface. A UI with an "add
intention" button would mean this ADR failed.

### 2. An intention must trace to a belief

`motivating_belief` is a `BeliefId`, not an `Option<BeliefId>`. There is no such thing as
a free-floating intention, so Endora cannot pursue something it cannot explain, and
"why is it doing this?" always has an answer that points at reviewable, correctable
understanding. This is what keeps understanding driving the loop rather than a parallel
store of work quietly becoming the real model.

### 3. At most one is active at a time

Not a queue, a **cursor**. Endora pursues one thing; forming a new intention while one is
active is refused, not stacked. Accumulation is the failure mode that turns any such
store into a backlog, and the cheapest way to prevent a backlog is to make it
structurally impossible to have two. A butler quietly juggling fifteen open threads at
03:00 is not a thing anyone asked for.

### 4. It retires itself

An intention ends without anyone tending it, two ways:

- **Exhausted** — a step budget (7 nights). Seven nights of looking into something
  without conclusion is Endora's answer that it is not getting anywhere.
- **Stale** — no progress in 14 days, which is what happens if the nightly loop is off or
  the schedule lapses.

Both retire it to `Abandoned`, visibly, in the activity trail. Nothing rots and nothing
waits for the person to close it — the same reasoning [0032](0032-beliefs-decay-and-expire.md)
applied to beliefs, for the same reason.

### How progress is captured

The butler's own reply text becomes the intention's **note**, deterministically, and is
fed back the next night: *"you're already looking into X; here is what you found last
time; continue."*

No structured field for the model to fill in, no new channel to parse. The measured
reality is that this model misreports its own tool failures 1 run in 3
([0030](0030-measuring-understanding.md)); asking it to also maintain a state machine
would be building on the least reliable thing in the system. Capturing prose it already
wrote is deterministic and cannot fail in a way that corrupts state.

## Consequences

- Overnight work continues across nights and across restarts. The nightly loop stops
  being seven unrelated evenings and starts being a week of attention on one thing.
- Endora can only pursue what it can already explain, which makes the belief layer more
  load-bearing — a wrong belief now produces wasted nights, not just a wrong sentence.
  That is a real cost, and the mitigation is that beliefs are visible and correctable,
  and the intention names the belief it came from.
- One intention at a time means Endora will sometimes drop a thread to start a better
  one, or decline a better one because it is mid-thread. Accepted: the alternative is a
  backlog, and this is reversible in a later ADR if one thread proves too few. It is much
  harder to go the other way.
- `nightly_focus` stops being "pick the strongest belief each night, afresh" and becomes
  "continue, or pick" — which is the actual behaviour change this ADR is for.

## Alternatives considered

- **A queue of intentions with priorities.** Rejected. It is the goal tracker with new
  nouns, and it needs exactly the grooming ADR 0029 removed.
- **Let the model manage intention state through structured output.** Rejected — see
  above. The model is the least reliable component and this is state that must not
  corrupt.
- **Make the person confirm each new intention.** Rejected. An approval queue by another
  name, and the reversible band already means overnight work needs no permission
  ([0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md), and the
  reversible-only clamp on unattended turns).
- **Store intentions in their own bounded context.** Deferred, not rejected. They are
  derived from beliefs and read by one consumer, so they live in `understanding` until a
  second consumer earns them a context of their own (ADR 0026's rule, and "avoid
  speculative abstractions").
- **Skip this and go straight to proportional interventions.** Rejected. An unprompted
  action that cannot survive a restart is a cron job, and sizing a one-off action to
  confidence is a much weaker idea than sizing a *step* in something Endora is pursuing.
