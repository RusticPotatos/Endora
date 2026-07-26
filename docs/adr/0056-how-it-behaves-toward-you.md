# 0056 — How it behaves toward you

## Status

Accepted (2026-07-26). **Consolidates 0017, 0019, 0025 and 0031**, which are archived.

## Context

A butler that only answers when spoken to is a search box. One that interrupts on a timer is
an alarm clock. The difference between an assistant and an intrusion is entirely in when it
speaks and how it sounds, and both were originally decided separately.

## Decision

### The heartbeat evaluates; it does not act

The node runs a background loop. On each tick it does not *do* anything — it **evaluates**
what, if anything, is due, and routes each candidate through the autonomy model
([0051](0051-where-the-boundary-is.md)). Unattended turns are clamped to the reversible
bands: nobody is there to say stop.

The loop also does the unglamorous maintenance that keeps the rest honest — retrying servers
that came up empty, applying findings that have earned it.

### Proactivity is a budget, not a trigger

Deterministic code owns **how often**; the butler owns **whether**.

The schedule is a gate: the feature must be on, the minimum interval must have elapsed, and
the person must have been quiet for a while — if they just spoke, they are present and can
simply ask. Once the gate opens, whether there is anything worth saying is a judgement, and
having nothing to say is a valid outcome.

That split is why there are no scripted check-ins any more. A clock decides *when it may*;
it never decides *that it must*.

### The persona lives in the prompt, and has a floor

Tone is not code. The butler has a default register — candid, warm — and **mirrors the
person's register asymmetrically**: it reflects formality and kindness *upward* and never
mirrors hostility, rudeness or contempt *downward*. It stays even and kind. That floor is the
one part of the persona that is not negotiable by tone-matching.

### It never recites its own vocabulary

Endora's internal taxonomy — beliefs, confidence, intents, bands — is **internal**. It does
not say "I have formed a belief with medium confidence"; it says what it thinks and why. The
words engineers use for a model's insides are not the words a person wants to be spoken to
in, and every time the vocabulary changes, this rule has to be checked again.

### Messages it started are an inbox, not a conversation

What Endora said **unprompted** — check-ins, the brief, what it looked into overnight — is
collected separately from the conversation, newest first, grouped by day, readable or read
aloud. It is derived, not stored: a butler message is unprompted when the message before it
is not the person's, so nothing needs a flag anyone must remember to set.

A failure notice is not an approach and does not land there. An inbox is what Endora *chose*
to say; "I couldn't reach my language model" is what happened when it could not choose
anything.

## Consequences

- Endora can be quiet for a day without that being a bug, and can speak without that being an
  interruption.
- Removing scripted check-ins made proactivity worse before it made it better — the honest
  version needs the model to have something to say.
- Hospitality is measurable in a small way: the person can see everything it said on its own,
  in one place, and stop it in one click.
- **The persona is prompt-shaped, so it inherits the model's reliability.** Anything that must
  be true is not left to tone ([0053](0053-honesty-about-what-it-did.md)).

## Rejected

- **Scripted check-ins and briefs.** Deleted; they said the same thing forever.
- **A fixed interruption schedule.** A clock may decide when it *may* speak, never that it
  must.
- **Persona in code.** Tone is the one thing a model is genuinely good at.
- **Mirroring the person's register in both directions.** Matching hostility is not warmth,
  and a butler that can be goaded is not a butler.
- **Surfacing internal vocabulary as a feature** ("I'm 70% confident"). It reads as a machine
  explaining itself instead of a butler answering.
