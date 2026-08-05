# 0072 — The base has rungs too

## Status

Accepted (2026-08-05). Extends [0055](0055-the-model-layer.md)'s capability ladder to the
everyday model, and answers the question that prompted it: *is there a reason we can't
have multiple endpoints for the base — highest available, falling back when a device is
offline?*

## Context

The everyday model is one endpoint: the NAS, always on, running the 7B that fits it. The
person's desk machine has a 32 GB GPU that runs a 14B comfortably — a materially better
brain — and is sometimes asleep, at work, or busy. Until now that machine could serve
only the deep rung, which fires on escalation triggers rather than on every turn.

The obvious objection is this repository's own history: the mixture was removed because
it thrashed. That lesson does not transfer. The mixture put **two models on one box**,
which load-and-unloaded each other on every switch; two *endpoints on two machines* share
no resource. What killed the mixture was contention; what this needs is a health check.

The privacy shape matters more than the plumbing. Base turns carry the raw conversation —
the pseudonym door ([0069](0069-one-door-off-the-device.md)) guards only the deep rung. A
"list of base endpoints" that could contain a cloud URL would quietly move every turn off
the device on whichever days that endpoint answered.

## Decision

### One preferred rung above the floor

`ButlerModelConfig` gains `preferred_url` + `preferred_model` — a bigger model on a
sometimes-away machine, tried first while it answers. The stored config stays the
always-on floor. Two rungs, not a list: the person has two machines, and a `Vec` is a
refactor away if a third ever exists. No premature abstraction.

### Available is a cheap, remembered fact

A fail-fast probe (`GET /v1/models`, 1.5 s timeout) whose verdict is trusted for sixty
seconds. A sleeping machine costs one short timeout a minute, not one per turn; a machine
waking up is serving again within the same minute. Falling back is silent and per-turn —
the model is never asked, told, or trusted about any of this.

### The base stays on the person's own network, structurally

The API refuses a public `preferred_url`: loopback, RFC 1918, `.local`/`.lan`, and bare
hostnames pass; everything else is turned away with the reason — *cloud models go under
the deep model instead*, where the door is. This is enforced at save time, so the unsafe
configuration cannot be stored, not merely warned about.

### Escalation skips itself

With the preferred rung serving, "ask a bigger model" may name the same endpoint and
model. `deeper()` compares against what is serving and declines — a paid no-op is not a
ladder.

### Which brain answered is a fact on a screen

`/health` reports `serving_url` and `serving_model`. Rung changes are otherwise silent by
design — a fallback mid-conversation is the system working, not an event — but "which
model wrote that?" must be answerable without guessing.

## Consequences

- **Turn quality follows the best machine that is awake**, automatically, with the NAS as
  the floor that never moves.
- **Answer style can shift between turns** when the rung changes. Accepted: the butler
  contract never promised one voice, and the alternative — pinning to the floor — wastes
  the better brain the person already owns.
- **A cold preferred model pays a load on its first turn** after idling out. Mitigated
  outside Endora (the endpoint's keep-alive), not compensated for in code.
- **The nightly model layer still tunes only the floor.** Evaluating candidates across
  both rungs is real future work for the layer, not smuggled in here.

## Rejected

- **An ordered list of N endpoints.** Two machines exist. The list buys generality nobody
  has asked for at the cost of a UI, an ordering story and N health states.
- **Reusing the mixture machinery.** It solved routing-by-question; this is
  routing-by-availability. Different axis, and the mixture's lesson (contention) does not
  apply across machines.
- **Letting the preferred rung be a cloud endpoint.** The whole privacy design assumes
  base turns stay home; a cloud base would bypass the pseudonym door on every turn it
  served. The deep rung exists for exactly that endpoint.
- **Probing on every turn.** A dead endpoint would tax every turn by its timeout; the
  remembered verdict caps the cost at one probe per window.
- **Announcing every fallback in chat.** A rung change is infrastructure behaving;
  narrating it would make the butler talk about its plumbing, which
  [0056](0056-how-it-behaves-toward-you.md) exists to prevent. The fact lives on `/health`.
