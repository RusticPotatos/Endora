# 0039 — Capability repair proposals: noticing its own tooling is wrong

## Status

Accepted (2026-07-26). The **observation half** of
[0038](0038-capability-profiles.md), which described deriving facts from outcome history
and did not build it. Consumes [0035](0035-outcomes-what-happened-after-acting.md).

## Context

[ADR 0038](0038-capability-profiles.md) said Endora should learn about its own tools from
three sources — declared, observed, confirmed — and built only the confirmed one (the
person nominates a server's state reader). The observed half was described and deferred:

> A capability that claims success while the read-back shows nothing changed is either
> lying or being misused. Once is an accident; a pattern across runs is a finding.

That pattern is no longer hypothetical. Live, over an evening:

```
claim: "completed successfully on: Kitchen (area), Kitchen Table (light)."
read : Kitchen Main  | switch | state: 'on'
       Kitchen Table | light  | state: unavailable
```

The claim is entirely **true** and entirely **useless**. Home Assistant acted on an
unavailable light and left alone the switch that was actually on, because in this house
"the kitchen light" is a `switch` named `Kitchen Main` and the model narrows to
`domain: ["light"]`. Every layer behaved correctly and the light stayed on, repeatedly,
across many phrasings.

A person watching that has to work out for themselves that the noun is wrong. **Endora
has strictly more information than they do** — it holds every claim, every reading, and
now whether anything changed — and it says nothing.

## Decision

**Endora derives repair proposals from what it has observed, and proposes them. It does
not apply them, and it does not change anything outside itself.**

### Derived, never stored

A proposal is **computed on read** from outcome history. There is no proposals table, no
row to dismiss, no state to groom. When the underlying outcomes age out, or an action
finally changes something, the proposal simply stops being derived.

This is the anti-queue guarantee made structural rather than promised. [ADR 0029](0029-delete-the-goal-tracker.md)
deleted a store of records the person had to process; the surest way not to rebuild it is
to have nowhere for such records to accumulate.

### Deterministic, with the model only phrasing

The derivation is arithmetic over stored outcomes: *this capability, aimed at this
target, reported success and changed nothing, more than once.* No model is in that path.
The measured reality is that this one obeys an explicit instruction about verification
1 run in 3 ([0034](0034-evidence-verifies.md)); it is not fit to decide what is broken.
It may put a finding into words. It may not find it.

### Proposes a question, not a repair

The honest output is *"nine attempts to act on 'kitchen' reported success and changed
nothing — what did you actually mean?"* Endora deliberately does **not** parse the
reading to guess the answer. Doing so would mean understanding one server's text format,
which is the per-integration patching [0038](0038-capability-profiles.md) exists to stop.

### It repairs its own knowledge, never the world

Two things "fix the kitchen light" could mean, and only one is in scope:

- **Change what Endora knows** — record that here, "the kitchen light" means
  `Kitchen Main`. Internal, reversible, no external effect.
- **Change the world** — add an alias in Home Assistant, or expose the switch as a
  light. That edits the person's smart-home configuration.

**Only the first.** Endora writing to a third-party server to make its own life easier is
a category of action that has to be opened deliberately, per capability, with the
autonomy envelope widened — not something it earns by having been right a few times.

### And when the fact exists, it grounds — it does not substitute

A confirmed alias enters the turn as context, the way understanding does. The runner does
**not** silently rewrite the target the model asked for. A deterministic rewrite of *what
gets acted on* can act on the wrong thing, and it would hide the model's mistake from the
eval battery that exists to measure it (ADR 0028's argument, unchanged).

## Consequences

- Endora surfaces the thing the person would otherwise have to diagnose — the first time
  it uses what it has observed about its own tooling for anything.
- Nothing accumulates, because nothing is stored. A proposal that is ignored costs
  nothing and disappears on its own.
- The finding is only as good as the reading. A server with no nominated reader produces
  no `changed` signal and therefore no proposals — the same honest silence 0038 chose.
- **It will sometimes be wrong**: an action can legitimately change nothing (turning off
  an already-off light). The threshold is repetition, and the output is a question rather
  than an assertion, so being wrong costs a sentence.
- This is the first rung of the self-managing agent in the project's north star, and it
  is deliberately the *smallest* one: notice, and ask.

## Alternatives considered

- **Apply the repair automatically.** Rejected. Even an internal alias changes what
  future actions target, and the evidence is a heuristic over a handful of samples.
- **Edit the Home Assistant config.** Rejected here; see above. Worth its own ADR if ever.
- **Let the model diagnose from the reading.** Rejected — the least reliable component,
  and it would need to parse a server-specific format to do it.
- **Store proposals so they can be dismissed.** Rejected. A dismissible record is a queue
  with extra steps, and deriving on read makes the whole class impossible.
- **Wait for more integrations before generalising.** Rejected for the reason 0038 gave:
  the data is already being recorded, and the failure has already happened repeatedly to
  the only person using this.
