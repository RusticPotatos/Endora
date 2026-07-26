# 0046 — A made-up category is part of the target

## Status

Accepted (2026-07-26). Refines [0041](0041-searching-the-reading-for-the-real-target.md)
using the vocabulary [0042](0042-direct-reach-into-a-service.md) made available.

## Context

Every search rule so far has skipped array arguments, on reasoning that was carefully
stated and correct:

> a scalar points at something, an array restricts which kinds count. `domain: ["light"]`
> is not part of what was aimed at, and treating it as such would make every light a
> candidate.

That holds exactly as long as the array actually holds a **kind**. Observed live, and
reported by the person as *"if I say turn on the kitchen table, it fails, but I have to say
light and it knows kitchen table light"*:

```text
"turn on the kitchen table"
  -> {area: "kitchen", device_class: ["table"], domain: ["light"]}
```

**No name at all.** The model read "kitchen table" as a room plus a sort of thing, and
filed `table` as a category. There is no category called `table`; Home Assistant ignored
the word it had never heard of, and acted on what remained — *every light in the kitchen*:

```text
The action completed successfully on: Kitchen (area),
  Kitchen Main Light (light), Kitchen Table (light).
```

Two lights, for a request that named one, reported as success. Adding the word "light"
fixes it only because the category slot is then occupied, so the whole phrase lands in
`name` instead.

Endora's own recovery could not help either: with `table` discarded as a filter, the search
had only "kitchen" to work with, which resembles half the room, so it correctly refused to
act and offered a shortlist the model ignored.

## Decision

**A kind filter the service has never used as a category is not a category. It is part of
what the person named.**

### The service says what a category is. Endora does not.

The list comes from the service's own data — for Home Assistant, every domain it actually
has and every device class it actually uses, read off `/api/states`. Not a list of kind
words in Endora's source: that list already exists in two places in this codebase, is
Home-Assistant-shaped, and is on [0038](0038-capability-profiles.md)'s inventory of things
to delete rather than extend.

### No vocabulary, no change

A service Endora has no direct reach into cannot say what a category is, and **guessing is
worse than doing nothing**: without a real list, every ordinary `domain: ["light"]` would
pour "light" into the search and make every lamp in the house a candidate. An empty list
means the previous rule applies unchanged.

This is the same shape as [0038](0038-capability-profiles.md)'s honest silence — a
capability with no nominated reader gets no read-back — and it is why this could not have
been built before direct reach existed.

### It changes what is *searched*, never what is *sent*

The unrecognised word joins the words used to find a candidate. It does not get moved into
a name field, and the call is not rewritten on that basis. Everything downstream is
unchanged: one candidate must still beat every other, a tie still acts on nothing, and the
action still goes out by id through direct reach.

That matters because the inference is a heuristic — *this word is not a kind* — and a
heuristic may inform a search without being allowed to aim an action.

## Consequences

- "Turn on the kitchen table" resolves to `Kitchen Table` and acts on that one thing,
  rather than switching on the whole room and calling it success.
- The class of failure this fixes is **wider than one phrase**: any request whose noun the
  model files as a kind was previously discarded, silently, in favour of a broader target.
- **Failure paths now read the service three times** — once for the reading, once for the
  vocabulary, once to resolve the name to an id. All on a path that had already failed, all
  local, and none of it on a working call. Worth collapsing when it stops being cheap.
- A service that uses a *word* as both a category and part of a name (a light actually
  called "Switch") keeps the category reading, and the search loses that word. Rare, and
  it fails the safe way: no unambiguous match, so it asks rather than acts.

## Alternatives considered

- **Keep a list of kind words in Endora.** Rejected — it exists twice already, it is
  Home-Assistant-shaped, and [0038](0038-capability-profiles.md) lists it for deletion. It
  also cannot know that *this* house has no `table` device class.
- **Move the unrecognised word into the `name` field and retry.** Rejected. Which field is
  "the name" is per-server knowledge, and the word is there because the model was already
  confused about fields. Letting a heuristic aim an action is the line
  [0024](0024-reversibility-bands.md) draws.
- **Drop unrecognised filters and let the call widen.** Rejected outright: that is what
  Home Assistant already did, and it is what switched on two lights.
- **Ask the person what they meant.** Rejected as the first resort for the reason
  [0041](0041-searching-the-reading-for-the-real-target.md) gave — it puts the work on them
  for something Endora can look up. Still the fallback when nothing wins.
