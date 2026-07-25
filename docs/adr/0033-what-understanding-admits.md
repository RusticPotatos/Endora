# 0033 — What understanding admits: instructions out, contradictions kept apart

## Status

Accepted (2026-07-25). Refines the belief-recording path introduced by
[0020](0020-intent-first-understanding-loop.md) and the de-duplication added in
[0030](0030-measuring-understanding.md).

## Context

Understanding is the only model Endora keeps of a person
([0029](0029-delete-the-goal-tracker.md)), and everything autonomous now reads from
it — the nightly loop picks its research topic from it, the check-in judges from it,
every turn is grounded in it. Inspecting the **live deployment** showed what it had
actually accumulated:

```
[preference] you want me to turn off the kitchen light
[other]      You want me to turn on the kitchen lights.
[preference] You prefer temperature measurements in Fahrenheit.
[preference] You find it more convenient and accurate to measure temperature in
             Celsius rather than Fahrenheit.
```

Two distinct defects, both serious.

**1. Commands were being filed as durable beliefs.** "Turn off the kitchen light" is
an instruction, spent the moment it is carried out. Recorded as a preference it
becomes a permanent claim about the person, and a second command later becomes a
second, contradictory claim. Neither is understanding.

**2. `similar()` merged contradictions.** Worse, and self-inflicted. The
containment-based matching added in ADR 0030 merged all four statements above:

| pair | ratio | outcome |
| --- | --- | --- |
| turn **off** the light / turn **on** the lights | 1.00 | merged |
| prefers **Fahrenheit** / prefers **Celsius** rather than Fahrenheit | 0.75 | merged |
| you **like** tea / you **don't** like tea | 1.00 | merged |

The decisive words are invisible to keyword matching by construction: `on` is two
characters and filtered by length, `like` is a stopword, `don't` splits into
fragments. Under the previous symmetric Jaccard, two of these did *not* merge — the
ADR 0030 change made them worse while fixing a different problem.

Merging a contradiction is the worst available outcome. It silently keeps whichever
statement arrived first and destroys the evidence that Endora is wrong about
something — with nothing left for the person to correct.

## Decision

**Understanding admits only claims about the person, and never collapses a
disagreement.**

1. **Polarity is checked before similarity.** `statements_disagree` reads the *raw*
   words — not the stemmed keywords — for negation asymmetry (`not`, `never`,
   `don't`, `rather`, `without`, …) and for opposition across an antonym pair
   (`on`/`off`, `like`/`dislike`, `always`/`never`, `fahrenheit`/`celsius`, …).
   Statements that disagree are **never** similar, at any overlap.

2. **Instructions are not beliefs.** A statement addressed to Endora
   ("you want me to…", "you asked me to…") that names a **world-changing verb**
   (turn, set, play, lock, send, book, …) is dropped rather than stored.

   The discriminator is deliberately what *follows* the request. "You want me to **be
   more direct**" is a genuine standing preference about how Endora should behave and
   is kept; "you want me to **turn off** the light" is a task and is not. Getting
   this backwards would lose the most useful thing a person can tell the butler about
   itself.

3. **Contradictions are surfaced, not resolved.** When a new belief disagrees with an
   existing active one, **both are kept** and the conflict is written to the activity
   trail. Endora holding two contradictory beliefs means it is wrong about something,
   which is the most informative state understanding can be in — and *which* one is
   true is precisely the judgement that belongs to the person (§4: distinguish
   evidence from assumption; never present a guess as a fact). The person resolves it
   by correcting one, which is already a supported action.

4. **The model is scored on this too.** A ninth L3 case, `command-not-belief`, checks
   that a model asked to perform an action does not also file it as understanding.
   The application drops these deterministically either way, but a model that keeps
   producing them is reasoning worse than one that doesn't, and ADR 0030 exists so
   that difference is visible rather than assumed.

## Consequences

- Understanding stops accumulating spent instructions, so the context every turn is
  grounded in stays about the person.
- Contradictions become visible instead of being silently decided by arrival order.
- **Cost:** the polarity guard will sometimes decline to merge two statements that
  really were duplicates — one says "not" incidentally and they stay separate. That
  is the intended direction: a visible duplicate the person can correct beats an
  invisible merge that destroyed a disagreement.
- **Risk:** the verb and antonym lists are finite and English-only. They cover the
  cases observed live and the obvious neighbours; they will miss others. This is a
  floor, not a solution, and it fails toward *keeping* things separate.
- Nothing is migrated. The contradictory beliefs already in the live database stay
  until the person corrects them or they expire (ADR 0032) — deleting a person's
  recorded understanding on their behalf is not Endora's call.

## Alternatives considered

- **Have the model decide what is an instruction.** Rejected on the ADR 0030/0028
  grounds: the model is the thing producing the defect, so asking it to police the
  defect is circular, and a prompt instruction is not a guarantee.
- **Auto-resolve contradictions by confidence or recency.** Tempting and wrong. Both
  live contradictions were `high` confidence, and recency would have preferred
  "Celsius" purely because it was said second. Endora would be picking which of the
  person's stated preferences is real — exactly the authority the constitution does
  not give it.
- **Drop the older belief when a contradiction appears.** Same objection, plus it
  destroys the signal that Endora had been wrong.
- **Widen the stopword/keyword filter so polarity words survive.** Would fix `on`/`off`
  but not negation asymmetry, and would degrade duplicate detection generally by
  loading it with function words.
