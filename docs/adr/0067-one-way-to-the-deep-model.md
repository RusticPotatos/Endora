# 0067 — One way to the deep model

## Status

Accepted (2026-08-04). Removes a second escalation path that bypassed
[0064](0064-what-a-stranger-said.md)'s taint rule, [0055](0055-the-model-layer.md)'s
consent gate and [0051](0051-where-the-boundary-is.md)'s pseudonym layer.

## Context

Asked for an afternoon briefing, the butler replied with a fill-in-the-blanks template:

```text
Here's a sample afternoon briefing you can use. If you want it personalized, just
share your location, calendar, or top priorities.

### ☀️ Afternoon Briefing — Saturday, May 9, 2026
**📍 Location:** *(Your city)*
#### 1. Weather Check
- *(Insert local conditions here)*
```

It asked for a location Endora holds, invented a date three months off, and offered to set up
a recurring briefing. The activity trail says what happened:

```text
["Used the news skill", "Used the news skill",
 "Asked the deep model (the local model came up empty)"]
```

### There were two ways to the deep model, and they were not equivalent

**Inside the turn**, `run_tool_turn` escalates properly: it refuses when a stranger's words
are in the conversation, checks the person's `escalate` consent, substitutes the values
Endora holds through the pseudonym table, sends the *whole conversation and context* via
`take_turn` — so the deep model is Endora, with a system prompt, a place and a clock — and
restores the real values in the answer.

**Outside the turn**, a second rung did this:

```rust
deep.and_then(|d| d.ask(text))
```

`ask` sent `{"messages": [{"role": "user", "content": question}]}` — the person's raw
sentence, alone. No system prompt, no place, no time, and none of what the turn had just
found. A large model given only *"give me an afternoon briefing"* answers the way any
assistant would, which is exactly what arrived.

The ugly output is the least of it. That path skipped three guarantees:

- **The taint rule.** `news` is `third_party: true`. Reading it marks the turn, the inner
  escalation correctly refuses — and the outer one, which never checks, went anyway. This
  fired. The whole point of [0064](0064-what-a-stranger-said.md) is that a turn carrying
  prose somebody else wrote does not reach a model that cannot be shown it safely.
- **The consent gate.** `deeper()` honours `config.escalate`; the node built the outer asker
  from the same config and dropped that field on the floor. Latent here only because the
  setting happens to be on: with automatic escalation switched *off*, this path would still
  have sent the question to a third-party API.
- **The disguise.** The inner path substitutes name, town and event title. The outer one
  redacted email addresses and nothing else.

Two rungs to the same model, written at different times, and the weaker one ran last —
so it won whenever the stronger one deliberately said no. **A refusal that a later
code path can override is not a refusal**, and every one of those refusals was a safety
decision.

## Decision

### The second rung is deleted

Not fixed. Making it honour taint, consent and pseudonymisation would mean re-implementing
all three outside the turn that already has them — and that duplication is what produced
this. The `DeepAsker` port, its one implementation and the node wiring go with it.

The inner path already fires on precisely the same condition: `not_an_answer` is true when
the reply text is empty, which is the only thing the outer rung tested. So nothing is lost
except escalations the turn had refused on purpose.

### An empty answer says so

When the local model produces nothing and the turn did not escalate, the person gets the
honest fallback that was already written — or, if the turn acted, an account of what it did.
A butler that says *I'm not sure how to help with that yet* is telling the truth. The
template was not.

### `ask_deep_model` stays

The person can still press the button and consult the deep model deliberately. That is a
choice they make, in the open, about one question — which is what
[0055](0055-the-model-layer.md) said the deep model was for before an automatic path
grew beside it.

## Consequences

- **A tainted turn cannot reach the deep model by any route**, which is what
  [0064](0064-what-a-stranger-said.md) claimed and did not have.
- **Automatic escalation now means what the setting says.** Turning it off turns it off.
- **Escalation always carries the conversation**, so the deep model can no longer answer as
  a stranger to the person it is answering about.
- **Some turns that used to produce text now say they are unsure.** That is the intended
  trade and it is not a regression: the text they produced was a template with blanks in it.
- **One fewer port.** `DeepAsker` existed to let a use case reach a model without the turn's
  context, and that was the defect rather than a feature.

## Rejected

- **Giving the outer rung the context.** Then it is the inner rung, written twice, and the
  copies drift — which is the whole story above.
- **Keeping it but checking taint there too.** Three guarantees to re-check at a second site,
  and the next guarantee added to the turn would be missing from here again. The failure mode
  is silent and safety-relevant.
- **Blaming the deep model's output.** It answered a bare question well. Nothing in what it
  was sent said who was asking, where they were, or that two news lookups had already run.
- **A prompt telling the deep model to behave like a butler.** The context was the missing
  thing, not the instruction — and [0065](0065-a-place-is-not-the-models-to-remember.md) is
  three days old.
