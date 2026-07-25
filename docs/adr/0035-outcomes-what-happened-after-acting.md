# 0035 — Outcomes: what happened after Endora acted

## Status

Accepted (2026-07-25). Completes the fifth clause of the architecture principle in
[docs/direction-reset.md](../direction-reset.md) for *actions*. Consumes the read-back
built by [0034](0034-evidence-verifies.md); applies to
[0028](0028-native-tool-calling-turn.md)'s turn.

## Context

The direction reset states the architecture in five clauses:

> **Models propose. Policy authorizes. Capabilities execute. Evidence verifies.
> Memory learns.**

**"Memory learns" is built for beliefs and absent for actions.** A belief can be
affirmed, corrected, decayed and expired ([0020](0020-intent-first-understanding-loop.md),
[0032](0032-beliefs-decay-and-expire.md)). An *action* leaves nothing behind. The turn
proposes, policy authorizes, the capability executes, [0034](0034-evidence-verifies.md)
reads the world back — and then the turn ends and the reading is discarded. Endora
cannot answer "what have I done for this person, and did any of it help?" because it
does not keep the answer.

That is not merely an omission. The roadmap's destination — interventions **proportional
to confidence** — is uncalibratable without it. Sizing an unprompted action to how sure
Endora is presumes some way to tell whether previous actions at that confidence landed
well. [ADR 0030](0030-measuring-understanding.md) made exactly this argument about
understanding: *until it is measured, every improvement here is guesswork*. The same
argument applies to behaviour, and the same answer follows — measure first.

There is a second gap this closes. The model layer ([0027](0027-self-improving-model-layer.md),
[0030](0030-measuring-understanding.md)) scores a *model* against a synthetic battery.
It says nothing about whether Endora's real actions, in this house, for this person,
have been any use. Those are different questions, and only the second one is the product.

## Decision

**Every capability run that is not in the `Observe` band leaves a durable `Outcome`.**

An `Observe` call has no effect to have an outcome about; its result is already evidence
(0034). Everything else changed something, or claimed to, and that claim is exactly what
deserves a record.

An outcome holds:

- **what was attempted** — the capability id and the input it was called with;
- **the claim** — the actuator's own account of its work, verbatim;
- **the observation** — what the read-back saw afterwards, when 0034 could get one;
- **when**, and **the belief that motivated it**, when the action traces to one;
- **the person's reaction** — `Helped`, `DidNotHelp`, or `NoReaction`, and *absent* until
  they say. Recording nothing is the normal case.

**The claim and the observation are stored separately, both verbatim, and no verdict is
derived from them.** This is 0034's reasoning carried into storage: deciding *confirmed*
versus *contradicted* needs a model of what the caller intended, which does not exist.
Keeping both, unreconciled, is the honest record — and it is the raw material a later
layer can reconcile against real data instead of against an assumption baked in today.

**The person is never asked.** A reaction is offered where an action already appears, and
an outcome nobody reacts to is complete without one. Prompting after every action is the
nagging that makes assistants exhausting, and a pile of records awaiting the person's
attention is an approval queue by another name — the precise thing
[ADR 0029](0029-delete-the-goal-tracker.md) deleted.

Memory rights apply in full: outcomes are visible, exportable and deletable with
everything else.

## Consequences

- Endora keeps a durable, per-action record of what it did and what the world looked like
  afterwards. That record is the input every later "was that a good idea?" question needs,
  and it accrues from the day this ships rather than from the day it is wanted.
- Storage grows with **actions**, not with time. Outcomes are small and an idle Endora
  writes none.
- Reactions will be sparse by construction. This is fine and expected: an outcome with no
  reaction still carries the claim and the observation, which is most of its value.
- The turn gains a write on the action path. It is best-effort — a failed outcome write
  never breaks a working action, the same rule 0034 applied to verification.
- This deliberately does **not** create anything to process. Nothing blocks on an outcome
  and nothing waits for the person.

## Alternatives considered

- **Reuse the audit log.** Rejected. Audit records a *policy verdict* as a one-line
  summary. An outcome needs structure — claim distinct from observation distinct from
  reaction — plus a link to the motivating belief. Conflating them would degrade both:
  the audit trail stops being a clean record of what policy decided, and the outcome
  loses the distinctions that are its whole point.
- **Reuse the activity/events feed.** Rejected. Activity is prose written for the person
  to read and deliberately lossy ("Used the weather skill"). It cannot be reasoned over.
- **Store a verdict — worked / didn't.** Rejected for 0034's reason. Any such verdict
  needs intent modelling that does not exist yet, and a wrong verdict recorded today
  poisons the calibration it is meant to serve.
- **Ask the person after every action.** Rejected, as above — nagging, and an approval
  queue by another name.
- **Wait, and build this together with proportional interventions.** Rejected. The record
  is only useful if it has *history* when the interventions layer arrives. Shipping it
  first means that layer opens with real data instead of an empty table.
