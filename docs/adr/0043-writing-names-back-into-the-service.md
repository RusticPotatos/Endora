# 0043 — Writing names back into the service

## Status

Accepted (2026-07-26). The destination [0039](0039-capability-repair-proposals.md) named
and deferred; built on the direct reach of [0042](0042-direct-reach-into-a-service.md).

## Context

[ADR 0039](0039-capability-repair-proposals.md) drew a line and said which side it was
staying on:

> Two things "fix the kitchen light" could mean, and only one is in scope:
> **change what Endora knows** … **change the world** … Only the first.

and then named the cost of stopping there:

> **This is a workaround and should be named as one.** An alias in Home Assistant fixes
> the problem for every client — voice assistants included — while an alias here only ever
> helps Endora.

That cost is now concrete. This house has four confirmed aliases — `table`,
`table light`, `kitchen table light`, `ceiling light` — every one of them a fact the
person took the trouble to state, and every one of them invisible to the house itself. The
same person speaking to their own voice assistant gets the same failure Endora had. Endora
holds the answer and keeps it to itself.

The reach to fix it now exists ([0042](0042-direct-reach-into-a-service.md)), with one
wrinkle: naming is **not** on the REST API. Reading state and calling services are; the
entity registry, where a thing's aliases live, is behind the same WebSocket the service's
own front end uses. Owning the names costs a socket that seeing and acting did not.

## Decision

**Endora writes the names the person has confirmed into the service that owns them —
additively, reversibly, and only when explicitly allowed.**

### Only what was confirmed. It invents nothing.

The one source written upstream is the person's **confirmed** aliases: the top of
[0038](0038-capability-profiles.md)'s trust ranking, facts they stated themselves. Endora
does not push names it merely inferred from a successful recovery.

The temptation is obvious and worth refusing out loud. A recovery that resolves
`{name: "lamp", area: "guest bedroom left"}` proves nothing about what "lamp" means — this
house has a `living room lamp` too, and writing `lamp` onto one entity would teach the
whole house something false. Observed facts recover a single call. Only confirmed facts
are written down.

### Additive, never destructive

Existing aliases are read first, preserved, and the new one appended. There is no rename,
no removal, no reordering. The most this can do to a name that already exists is nothing.

### The undo is captured before the write

Every write returns the complete prior list of aliases, and `restore_aliases` puts them
back exactly. This is what [0039](0039-capability-repair-proposals.md) meant by "its own
reversibility story", and the reason the band is `OutwardReversible` rather than
`Irreversible`: it reaches outside Endora, and it can be taken back.

**Limit named here, and closed by [0045](0045-an-undo-log-for-what-it-changed.md):** this
ADR captured the undo *value* and stored nothing, so it existed for the length of one
function call. A prior value nobody keeps is not a reversibility story but a claim about
one. Writes are now logged, shown, and individually reversible.

### Off until turned on

Seeing and acting are one grant; **editing the service's own configuration is another**,
and it does not come free with the first. The channel refuses to teach until the person
turns it on, and the default in the repository is off.

This is [0024](0024-reversibility-bands.md)'s posture applied one level up: not "which
actions may run", but "which *kinds of reach* Endora has at all".

## Consequences

- A name the person explains once is fixed **everywhere**, not just inside Endora. The
  voice assistant in the same house stops failing on it too.
- The alias table stops being a workaround and becomes what it always should have been:
  the place answers are collected before being written where they belong.
- **Endora now edits a third party's configuration.** That is a real threshold, and the
  gates are: only confirmed facts, only additive, undo captured, off by default, disclosed
  in the result.
- One more dependency, and a second protocol to Home Assistant. Both are the price of the
  registry not being on the REST API.
- Nothing above the adapter learns any of this: `teach` is a port with a default that
  refuses, so a channel earns editing separately from seeing and acting.

## Alternatives considered

- **Write names Endora inferred from successful recoveries.** Rejected, and it is the
  tempting one. A recovery proves what worked for one call, not what a word means in a
  house — `lamp` resolved once next to a `living room lamp` that would then be wrong.
- **Rename the entity instead of adding an alias.** Rejected. Destructive, visible in
  every dashboard the person owns, and it presumes Endora's opinion of the right name
  beats theirs.
- **Ask before each write.** Rejected as the wrong grain: the person already answered this
  exact question when they confirmed the alias. Asking again is asking twice.
- **Wait for a stored undo log first.** Considered seriously. Rejected because the write
  is additive and the prior value is returned with it, so nothing is lost meanwhile — but
  the gap is recorded above rather than glossed.
- **Do it by hand in Home Assistant.** Correct, already advised, and exactly the work a
  butler is supposed to take off the person.
