# 0025 — Hospitality and the evolving persona

## Status

Accepted (2026). Extends [ADR 0017](0017-persona-and-voice.md) (persona and voice),
[ADR 0019](0019-proactive-self-improving-butler.md) (proactive butler, hospitality),
and [ADR 0020](0020-intent-first-understanding-loop.md) (understanding). Bounded by
[ADR 0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md) (nothing
irreversible on its own).

## Context

The butler should not merely answer — it should **strive to please** (anticipate,
serve, delight) and have a **personality that grows with the relationship** (learns
how the person likes to be spoken to, their humour, when a joke lands versus just the
facts). Today the persona is a fixed prompt (ADR 0017); it does not evolve, and
proactivity (ADR 0019) is framed as scheduled check-ins rather than acts of service.

Both goals are dangerous if taken naively:

- **"Striving to please" can rot into sycophancy** — a butler who only flatters and
  agrees is a bad butler.
- **"Evolving personality" can drift** — mutate ungrounded, or worse, use rapport to
  manipulate.

So both must be made first-class *and* fenced.

## Decision

### Hospitality — acts of service

Reframe proactivity from "check-ins" to **anticipatory acts of service** produced by
the nightly loop (ADR 0024) and the Understanding (ADR 0020): a briefing timed to the
person's morning, an unsolicited-but-wanted heads-up, a well-timed suggestion. Each is
**sized to confidence** (more sure → more forward; unsure → smaller, or just ask) and
lives **entirely in the reversible band** — it surfaces, drafts, and offers; it never
does anything it can't undo.

### The evolving persona — a learned *manner*

Symmetric to Understanding (beliefs *about the person*), the butler holds a small,
evolving model of its **manner with this person** — how they like to be addressed
(the honorific, already started), their preferred formality, brevity, warmth, and
humour. It is:

- **Learned from evidence** — how the person actually responds — not random mutation.
- **Grounded on the base persona** (ADR 0017): it adapts *from* that starting
  character, never abandons its core.
- **Expressed in voice**, fed into the reply prompt like the rest of the context.
- **Correctable** — if it gets too jokey or too stiff, the person nudges it, exactly
  as they correct a belief.

### Guardrails (the whole point)

- **Candor over flattery.** The anti-sycophancy rule (ADR 0017) is absolute and
  *overrides* "please me." The best service is sometimes an honest "I wouldn't."
- **Grounded, not drifting.** Manner changes only on real evidence, stays bounded
  (never sheds honesty or kindness), and is correctable.
- **Never manipulative.** Rapport serves the person; it is never used to steer them
  toward the butler's — or anyone's — ends (constitution).
- **Reversibility holds.** It may delight with anything undoable; never with anything
  that isn't (ADR 0024).

## Consequences

- A warmer, deepening relationship: the butler feels like *your* butler, and gets more
  so — while staying honest and safe.
- New surface: a small **manner** model (like beliefs) and its grounding in the
  prompt; acts of service produced by the nightly loop.
- The risks (sycophancy, drift, manipulation) are answered by explicit, enforced
  guardrails plus correctability — not left to the model's disposition.

## Alternatives considered

- **Keep a fixed persona (ADR 0017 only).** Safe and simple, but never grows — the
  person explicitly wants a personality that evolves. Rejected as the ceiling.
- **Unbounded personality learning.** Maximally adaptive, but invites drift and
  manipulation. Rejected — manner is grounded, bounded, and correctable.
- **Optimise for agreeableness.** The straightest road to "pleasing", and exactly the
  sycophancy we forbid. Rejected.
