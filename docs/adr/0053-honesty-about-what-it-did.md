# 0053 — Honesty about what it did

## Status

Accepted (2026-07-26). **Consolidates 0028, 0034, 0035 and 0037**, which are archived.

## Context

The failure this document exists for is not a model doing the wrong thing. It is a model
doing **nothing** and saying it did something.

Observed repeatedly: the butler announcing it had turned a light on when no tool ran at all;
a tool reporting *"completed successfully on: Kitchen, Kitchen Table"* while the light stayed
exactly as it was; a reply announcing the state of a device the turn never touched. Every one
of those is worse than an error, because an error is visible.

The instinct to fix this by narrating actions deterministically — having code write "I turned
on the light" — was tried and rejected: it produced sentences the model then contradicted, and
it hid from the eval battery exactly the behaviour the battery exists to measure.

## Decision

### One native tool-calling turn

A turn is a single conversation with the model using **native tool calls**, bounded by a round
budget and a failure cap. No scripted pre-gather, no deterministic narration, no separate
"decide then act" phases. What the model asks for, policy authorizes, and the result goes back
into the same conversation.

### The answer streams; the tool rounds do not

A turn is several rounds, and only the last one produces prose. Streaming is therefore
per-round: `content` deltas are relayed as they arrive, and a round that turns out to be
**tool calls** emits nothing — its fragments are a machine format, and while it is working
the person should be watching the action trail, not the model thinking out loud.

Two properties keep it honest. The turn tracks what actually reached the person, so the
notes it appends afterwards are sent as a **suffix** and the reply never arrives twice. And
a streamed round is reassembled into exactly the shape a one-shot round returns, then parsed
by the same code — so streamed and non-streamed turns cannot drift apart in how they are
understood.

The port keeps a default that answers in one piece, so a butler without native streaming
still works with a streaming caller; only the model-backed one overrides it.

### Evidence verifies. An unobserved effect is never reported as fact.

An actuator's own receipt is a **claim**, not evidence. After an action, Endora reads the
world back through the capability the person nominated as that service's reader, and the
result is an **observation**.

- Read **before and after**, so "did anything change?" is answerable at all.
- On success, scope the read to what the action was aimed at.
- On **failure**, deliberately widen it — a failed action's target is the prime suspect, so
  reading back with the same target fails identically and tells nobody anything.
- No reader, no observation, and results stay marked unverified. Silence is the honest answer.

### Claim and observation are stored apart, and never reconciled

Every action leaves an **outcome**: what the tool claimed, what Endora observed, and whether
anything changed. The two are never merged into a verdict, because a tool claiming success
while nothing changed is precisely the failure the record exists to catch. Merging them
destroys the evidence.

### The interface discloses. The reply is not the record.

What was done is shown **regardless of what the model says about it**. Disclosure is
deterministic; persuasion is not attempted. Concretely:

- every action a turn took is listed, with its claim and its observation, whether or not the
  reply mentions it;
- a turn whose actions **all failed** appends one true sentence — *nothing was changed* —
  because the trail is not what the person reads first;
- a service's own reply is reported, never interpreted into an outcome it does not support. An
  empty response from a service that reports state asynchronously means *accepted*, not
  *already as asked*; reading it as an outcome once put a false claim in the record about a
  light the person had watched turn off.

### Endora asserts what it observed, and only that

Being careful about the service's reply left a gap on the other side: the read-back said so
when nothing changed, and said **nothing** when something did. So the model was left with
only the service's hedge — "accepted; its reply says nothing about the result" — and had to
guess. Endora had already compared two readings and knew.

It now states both verdicts. That is not narration on the model's behalf: it is reporting an
observation Endora made, which is the one thing it is entitled to assert.

### An apology is a claim too

When the model returned **no words at all**, Endora answered "I'm not sure how to help with
that yet" — on a turn where it had just switched a light on. The generic fallback was
asserting something false about work that had succeeded, and left the person unable to tell
whether to try again.

If the turn acted, the fallback now says what happened, quoting and attributing the tool's
own report rather than paraphrasing it.

**Why this is not the deterministic narration rejected above.** That was rejected because
code-written sentences got contradicted by the model. There is nothing to contradict when
the model produced no sentence at all. The distinction is exactly the presence of a reply:
where there is one, Endora appends facts and never edits it; where there is none, silence
would be its own claim.

## Consequences

- The person can always answer "did that actually happen?" without trusting the prose.
- The eval battery still sees the model's mistakes, because nothing papers over them.
- Outcomes accumulate into something later decisions can be derived from — which is what made
  capability findings possible at all ([0054](0054-other-peoples-services.md)).
- **Verification costs a read per action.** Accepted; it is the cheapest honest option.
- Unverified results say so, which reads as hedging until the one time it matters.

## Rejected

- **Deterministic narration.** Code writing "I turned on the light" produced sentences the
  model contradicted, and hid the behaviour the eval exists to measure.
- **Trusting the actuator's receipt.** It is the party whose work is in question.
- **Merging claim and observation into a verdict.** Destroys the only signal that catches a
  lying tool.
- **Asking the model to respect the read-back.** Measured at roughly 1 run in 3; the guarantee
  moved into code ([0051](0051-where-the-boundary-is.md)).
- **Interpreting a service's empty reply.** Tried, produced a demonstrably false record.
- **Rewriting what the model said.** Endora appends what is true; it does not edit the reply.
