# 0031 — Agentic proactivity: a budget, not a trigger

## Status

Accepted (2026-07-25). Amends the check-in half of
[0019](0019-proactive-self-improving-butler.md). The brief
([0025](0025-hospitality-and-the-evolving-persona.md)) and the nightly loop
([0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md)) are deliberately
left on the clock — see below.

## Context

[ADR 0028](0028-native-tool-calling-turn.md) and
[0029](0029-delete-the-goal-tracker.md) made the butler's proactive moments *agentic
in how they run*: a check-in is a real tool-calling turn, grounded in what Endora
understands, with no scripted floor underneath it.

But the **decision to speak at all** was still `now >= next_at`. Every 24 hours the
butler was obliged to produce a check-in, whether or not it had noticed anything —
which is precisely the condition that manufactures filler. "Good to see you, what
would you like to work on?" is what a system says when the clock told it to talk and
it had nothing.

That inverts the point in the same way the goal tracker did. An autonomous butler
reaches out **because it noticed something**, and can say what. One that reaches out
because a timer fired is a reminder app with better prose.

The naive fix — ask the model every tick "do you want to say something?" — is worse.
A model asked often enough will eventually say yes for a bad reason, and each "yes"
is an interruption the person did not ask for. Restraint cannot live in the prompt;
this project has already learned that lesson about honesty (ADR 0028) and about
belief de-duplication ([0030](0030-measuring-understanding.md)).

## Decision

**Split the decision in two: deterministic code owns *how often*, the butler owns
*whether*.**

`CheckinSchedule` stops being a trigger and becomes a **budget**:

- `may_reach_out(now, last_person_activity)` is the gate. It requires the feature to
  be on, the minimum interval to have elapsed, and the person to have been quiet for
  at least an hour — if they just spoke they are present and can simply ask, so
  reaching out on top of that is noise.
- Within the budget, the butler is asked whether anything is genuinely worth raising,
  and told plainly that an empty answer is the right one most of the time.
- **The budget is spent whether or not it speaks.** A "nothing to say" costs exactly
  what speaking costs. This is the load-bearing rule: without it, a declined turn
  becomes a retry every thirty seconds until the model talks itself into a reason.
- **Silence is the default on every failure path.** Empty reply, unavailable model,
  errored turn — all mean no message. Nothing is posted to fill the slot.

The reason it gives is written to the activity trail, so *"why did it message me?"*
always has an answer.

**What stays on the clock, and why.** A *daily brief* is time-anchored by definition —
"tell me about my day, in the morning" is a standing request, not something the butler
should second-guess. The *nightly loop* is explicitly "while you sleep". Making either
agentic would be change for its own sake. Only the check-in — the one that was
inventing reasons to speak — moves.

## Consequences

- The butler is quiet by default and speaks when it has something. The failure mode
  flips from "interrupts with filler on a timer" to "occasionally stays quiet when it
  might have spoken" — the right direction for something that lives in your house.
- `run_due_checkin` becomes `consider_reaching_out` and returns an activity trail
  alongside the message.
- `CheckinSchedule::is_due` becomes `may_reach_out`. The stored `interval_ms` keeps
  its value but changes meaning from "how often it speaks" to "the most often it may".
  No migration needed; the existing value is a sensible budget.
- **Cost:** a person who liked a guaranteed daily nudge no longer gets one. That is
  the intended trade — a guaranteed nudge is exactly what forced the filler.
- **Risk:** with a weak model, "is anything worth raising?" may reliably answer no,
  and the butler goes silent for good. Silence is the safe failure, but a butler that
  never speaks is not proactive either. This is measurable — the L3 tier
  ([0030](0030-measuring-understanding.md)) already scores whether a model can tell a
  revealing turn from a barren one, which is the same judgement.

## Alternatives considered

- **Ask the model every tick, no budget.** Rejected: restraint in a prompt is not
  restraint, and the cost of being wrong is an interruption.
- **Trigger on specific events (a new high-confidence belief, a due reminder).**
  Cleaner to reason about, and genuinely tempting. Rejected for now because it
  re-encodes "what is worth saying" as a fixed rule list — the thing ADR 0029 removed.
  Worth revisiting as an *additional* gate that opens the budget early, rather than as
  the decision itself.
- **Make the brief and nightly loop agentic too.** Rejected as change for its own
  sake; both are legitimately time-anchored, and pretending otherwise would add
  judgement where none is wanted.
