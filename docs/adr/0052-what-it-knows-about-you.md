# 0052 — What it knows about you

## Status

Accepted (2026-07-26). **Consolidates 0006, 0011, 0013, 0014, 0015, 0016, 0020, 0029, 0032,
0033 and 0036**, which are archived. Includes one large reversal, kept because it is the
most instructive thing in this repository.

## Context

Endora began with a **tree**: Value → North Star → Target → Experiment → Reflection, plus
suggestions awaiting approval. It was coherent, it was implemented, and it was wrong — a
project-management tool wearing a butler's clothes. It asked the person to maintain a
hierarchy, and a queue of proposals accumulated for them to process.

[0029] deleted the entire thing. What replaced it is smaller and does more.

## Decision

### Understanding is the only model

Endora holds **beliefs about the person** — statements with evidence, a kind, and a
confidence — and nothing else. No goals, no targets, no north stars, no experiments, no
approval queue. If something cannot be expressed as "here is what I think is true about you,
and here is why", Endora does not track it.

### Conversation is the interface; the model is internal

The person talks. Understanding is formed from the conversation and is visible and
correctable, but they are never asked to curate it. Every belief can be seen, affirmed or
corrected — memory rights, not a maintenance task.

### What understanding admits

- **Instructions are not beliefs.** "Turn off the light" is a command, not a fact about the
  person; treating it as one filled the model with noise.
- **Contradictions are kept apart, not merged.** Two conflicting statements are two beliefs
  with their own evidence. Averaging them loses the fact that the person changed their mind.
- **Confidence comes from evidence**, not from the model's enthusiasm.

### Beliefs decay and expire

A belief that is never re-evidenced loses confidence over time and eventually expires.
Without this, understanding is a ratchet: everything ever inferred stays true forever, and
the person is permanently characterised by a passing remark. Decay is what makes it a
*model* rather than a transcript.

### One intention at a time — a cursor, not a queue

Endora may hold **one active intention**: work that outlives a turn, traceable to a belief
that motivates it, with a step budget and a staleness horizon so it retires itself. The
person's only verb is *stop*.

There is deliberately **no "add"**. A console that let you file work would rebuild [0029]'s
goal tracker with a new name.

### Attention is computed, never pushed

What deserves attention is a **read projection** over facts already stored, plus a little
deferral state. Nothing accumulates; nothing needs clearing. The same instinct produced
"derived, never stored" for capability findings
([0054](0054-other-peoples-services.md)) and is why this system has no inbox of chores.

## Consequences

- The person maintains nothing. That is the entire point, and it is what the tree got wrong.
- Understanding is small enough to read in one screen and correct in one click.
- Endora can be wrong about the person, visibly, and be told so — which is better than being
  wrong invisibly.
- **A real loss from the deletion**: explicit long-term goals are no longer tracked as
  first-class things. Accepted, because they were tracked and not used, and the honest
  version of that feature is a belief that says the person cares about something.

## Rejected

- **The Value → North Star → Target tree.** Built, shipped, deleted. It made the person the
  system's administrator.
- **An approval queue for proposals.** [0029]'s specific mistake; anything with a badge and
  a count is the same mistake returning.
- **Letting the person edit beliefs freely as a primary flow.** Correction, yes. Curation as
  a chore, no.
- **Merging contradictory beliefs** into one averaged statement.
- **Keeping beliefs forever.** A model that only accumulates is a transcript.
