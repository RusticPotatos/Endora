# 0044 — Policy acts on what it has established

## Status

Accepted (2026-07-26). Turns [0040](0040-withdrawing-a-capability-that-never-works.md)'s
proposal into an action.

## Context

[ADR 0040](0040-withdrawing-a-capability-that-never-works.md) taught Endora to work out,
from stored outcomes alone, that one of its tools has never worked — and then to put the
finding on a card and wait.

The person's ask was the opposite: *"I want the butler to use and own every skill I give
it, as a product owner, make the choices and changes that give the best experience."*

Waiting is the wrong answer to that, and it is also the wrong answer on the evidence. The
derivation is arithmetic over records Endora already holds. It is not a hunch, it does not
improve by being looked at, and the thing it identifies is a tool that has failed on
several different targets and never once succeeded. Asking someone to confirm that is
asking them to rubber-stamp a calculation.

## Decision

**Where a finding is established deterministically and the action is reversible, policy
applies it and says so.**

Concretely: a capability with a `StopOfferingIt` finding is turned off, on the heartbeat,
without being asked.

### Policy acts. The model does not.

This is not a widening of what the language model is trusted with — the distinction
[0005](0005-models-propose-policy-authorizes.md) exists for. Nothing generative is in this
path: the finding is counted from stored outcomes, and the action is a boolean flag. The
model cannot cause a withdrawal, request one, or prevent one.

That is what makes it safe to automate something that was previously a question. The
autonomy being granted is *deterministic* autonomy.

### Only the finding that can be answered without the person

Of the two remedies in [0040](0040-withdrawing-a-capability-that-never-works.md), only one
is Endora's to answer:

- **"What is this really called?"** is a fact only the person holds. It stays a question,
  and it always will.
- **"This has never worked — stop offering it?"** is a conclusion from evidence Endora
  already has.

Guessing at the first would mean inventing names, which [0043](0043-writing-names-back-into-the-service.md)
refuses for good reason.

### Three properties, and it needs all three

- **It only ever turns something off.** No action is taken in the world; a capability
  stops being offered. This cannot break something that was working, because the premise
  is that nothing was.
- **The bar is high and was already raised once.** Several distinct targets, repeated
  *outright refusals*, and not one success of any kind — including unverified ones. That
  bar exists because the first version of it proposed withdrawing `HassTurnOn`, the most
  useful tool in the house, on five successes misread as failures
  ([0040](0040-withdrawing-a-capability-that-never-works.md)).
- **It is one click to undo, and it is announced.** The activity trail carries the count
  that justified it and the way back. A capability quietly disappearing is the silent
  narrowing [0040](0040-withdrawing-a-capability-that-never-works.md) warned about; the
  difference between that and this is entirely in the saying.

## Consequences

- A tool that has proved useless stops being offered without anyone having to notice,
  which is the first thing in this system Endora decides about itself and then does.
- **It will sometimes be wrong**, and the cost is bounded: a capability is off until
  someone turns it back on, and the trail says why. No world state is touched.
- The repair card for a withdrawal effectively disappears — the composition layer already
  drops findings whose answer has been given
  ([0040](0040-withdrawing-a-capability-that-never-works.md)), and now the answer gives
  itself.
- **The precedent is the real consequence.** "Deterministic finding + reversible action +
  disclosure" is now a shape that can be applied elsewhere, and it should be applied
  *only* where all three hold. The moment any of them is dropped, this stops being safe
  autonomy and becomes a model acting on a guess.

## Alternatives considered

- **Keep asking.** Rejected — it is asking someone to confirm arithmetic, and it means the
  same broken tool keeps being offered until they happen to look.
- **Let the model decide which tools to withdraw.** Rejected. It is the component whose bad
  choices created the finding, and it would put a generative step inside a policy decision.
- **Auto-adopt an inferred alias too.** Rejected. That is the other remedy, and it is a
  fact about what a person means — [0043](0043-writing-names-back-into-the-service.md)
  already refuses to write inferred names for exactly this reason.
- **Act immediately on the finding rather than on a heartbeat.** Rejected as needless: a
  tool that has failed a dozen times can wait two minutes, and keeping it off the turn path
  means it can never slow a reply or interleave with one.
