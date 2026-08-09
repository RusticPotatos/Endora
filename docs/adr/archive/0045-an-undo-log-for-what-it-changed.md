# 0045 — An undo log for what it changed

## Status

Accepted (2026-07-26). Closes the gap [0043](0043-writing-names-back-into-the-service.md)
named in itself.

## Context

[ADR 0043](0043-writing-names-back-into-the-service.md) let Endora write names into a
service's own configuration, and claimed reversibility as one of its four gates:

> **The undo is captured before the write.** Every write returns the complete prior list of
> aliases, and `restore_aliases` puts them back.

It then said what that was actually worth:

> **Limit worth naming:** the undo *value* is captured and returned, but there is no stored
> history of writes and no button that replays them.

The value existed for the length of one function call and was then dropped on the floor. A
prior value nobody keeps is not a reversibility story; it is a claim about one. Four names
have since been written into this house's registry, and until now Endora could not have
told anyone which four, when, or what they replaced.

The other two records Endora keeps — beliefs and outcomes — are both stored and both
visible. The one class of thing it does that reaches *outside itself* was the one it kept
no history of, which is exactly backwards.

## Decision

**Every change Endora makes to a service's own configuration is written down, shown, and
individually reversible.**

### The port returns the change, not a sentence

`teach` previously returned a human-readable string, which is unreplayable by
construction. It now returns the **write** — target, what was added, and what was there
before — and the channel gains an `undo` that takes one back. A channel that can only
report *that* it succeeded has made a change nobody can reverse.

### Undoing marks; it never deletes

The row stays, flagged. What Endora changed about someone's house is not something Endora
should be able to make disappear, and a history that edits itself is not a history. An
undone change reads as *put back*, not as *never happened*.

### The prior value is stored as a list, not a delimited string

Flattening it would corrupt any name containing the delimiter — precisely the detail an
undo cannot afford to get wrong, and precisely the sort of thing that goes unnoticed until
the one time it matters.

### Identity and time belong to the caller

The adapter knows neither the clock nor the id source, and should not: it knows how to talk
to one service. The composition layer stamps both, which also keeps `ConfigWrite` a plain
domain value the store can round-trip.

### Forgetting everything does not forget this

"Forget everything" erases what Endora knows about the person. This log is not that: it is
a **receipt for changes that still exist inside somebody else's service**. Deleting it does
not undo them — it makes them unrecoverable and invisible, which is strictly worse for the
person than keeping the record. So the purge leaves it alone.

### A name can be untold, and that is a change too

Teaching without forgetting leaves a person's own configuration carrying a word they have
changed their mind about, with no way to say so from here. So a name can be removed, and
the removal is logged exactly as an addition is.

Addition and removal are told apart by **what was there before**, not by a stored flag: a
name already in the prior list can only have been taken away. One table, one undo — replay
`was` — and no flag that could ever disagree with the data beside it.

### A no-op is not a change

Teaching a service a name it already knows writes nothing and logs nothing. An undo log
padded with rows that undo nothing is a log nobody reads.

## Consequences

- The reversibility [0043](0043-writing-names-back-into-the-service.md) claimed is now the
  reversibility it has: visible in the console, one button per change.
- The memory rights extend to the world, not just to Endora's own head — it can now be
  asked *what did you change about my house?* and answer precisely.
- This is what makes [0044](0044-policy-acts-on-what-it-has-established.md) safe to extend
  later. "Deterministic finding + reversible action + disclosure" needs the middle term to
  be real, and for config writes it now is.
- History grows without bound. Acceptable at this scale — four rows in a week — and a
  retention rule can come when there is anything to retain.
- **A purge leaves this table standing**, which is a deliberate exception to "forget
  everything" and has to be explained rather than discovered.
- **Undo depends on the channel still existing.** A change made to a service Endora has
  since lost reach into cannot be replayed from here; the record still says exactly what to
  put back by hand.

## Alternatives considered

- **Delete the row on undo.** Rejected. It makes the log a to-do list rather than a
  history, and it lets Endora erase evidence of what it did.
- **One "undo everything" button.** Rejected as a worse fit for the thing being undone:
  these are individual facts about individual devices, and a person wants the one they
  disagree with gone, not all of them.
- **Re-read the current value at undo time and diff.** Rejected. If someone edited that
  name themselves in the meantime, replaying a stored prior value is honest and
  predictable; a diff would silently reinterpret their edit.
- **Keep the log only in outcomes** ([0035](0035-outcomes-what-happened-after-acting.md)).
  Tempting, since outcomes already record what happened. Rejected because an outcome
  records a *claim and an observation*, not a **restorable prior value** — and undo needs
  the third thing.
