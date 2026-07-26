# 0047 — A thing and its diagnostics are not two things

## Status

Accepted (2026-07-26). A consequence of [0042](0042-direct-reach-into-a-service.md)'s
richer view, resolved inside [0041](0041-searching-the-reading-for-the-real-target.md)'s
rules.

## Context

[ADR 0042](0042-direct-reach-into-a-service.md) claimed seeing everything as a benefit,
and specifically this:

> Endora can reach things the tool surface never exposed. This is the larger gain and the
> less obvious one: no amount of searching finds what was never in the reading.

It is also a cost, and the cost showed up immediately. Asked to *turn off the kitchen main
light*, the search found:

```text
- Kitchen Main Light
- Kitchen Main Light LED
- Kitchen Main Light Cloud connection
- Garage Main
- Kitchen Table
```

The first three all contain *kitchen*, *main* and *light* — a **three-way tie at the top**
— so [0041](0041-searching-the-reading-for-the-real-target.md)'s rule refused to act, which
is exactly what it should do with a tie, and exactly the wrong outcome here. Two of those
three are not controls at all: an LED configuration entry and a connectivity indicator, both
named after the device they belong to.

The tool surface never had this problem because it only ever showed things Assist could
act on. Seeing more meant seeing a device's own paperwork alongside the device.

Notably, *"turn on the kitchen table"* worked in the same breath — that light has no
similarly-named siblings. The failure is not about phrasing at all; it is about which
devices happen to come with diagnostics.

## Decision

**A tie is broken by whether the tool could actually operate the candidate — and only a
tie.**

### It narrows an ambiguity; it never resolves one

The rule runs *after* [0041](0041-searching-the-reading-for-the-real-target.md)'s "one
candidate must beat every other" has already declined. It cannot overrule a clear winner,
and if **more than one operable thing** remains, the ambiguity was real and nothing is
acted on. A diagnostic can only ever lose; it can never cause something to be chosen.

### The service decides what it can operate

`actionable` is a question for the channel, answered from what the service is. For Home
Assistant that is the set of domains its switch service applies to — lights, switches,
covers, media players and so on — and not sensors, diagnostics or configuration entries.

That is Home-Assistant knowledge, and it lives in the Home Assistant adapter, which is
where [0038](0038-capability-profiles.md) says such knowledge belongs. A channel that
cannot tell answers *true* and narrows nothing.

### Why not filter the reading instead

Tempting: show only operable things and the tie never forms. Rejected, because the reading
is also what the **person and the model see** in a shortlist, and "these exist and look
like what you asked for" should be honest about what exists. A diagnostic named after a
device is genuinely useful information when the answer really is ambiguous.

So the filtering happens at the moment of *choosing to act*, which is the only moment it
is needed.

## Consequences

- Requests naming a device that ships with diagnostics work again, without weakening the
  tie rule for requests that are genuinely ambiguous.
- The list of switchable domains is a per-integration constant. It will drift as Home
  Assistant grows, and drifting **fails safe**: an unrecognised domain is treated as not
  operable, so at worst a tie stays a tie and Endora asks.
- Direct reach's wider view remains a net gain, but this is the second cost it has
  produced — the first being ids competing with names in the matching text
  ([0042](0042-direct-reach-into-a-service.md)). Both were "more information made the
  search worse", and both were fixed by being clearer about what a given piece of
  information is *for*.

## Alternatives considered

- **Filter the reading to operable things.** Rejected; see above — the shortlist should
  describe what exists.
- **Prefer the shortest matching name.** Rejected as a coincidence dressed as a rule:
  `Kitchen Main Light` is shorter than its diagnostics here, and would not be in a house
  where the device is called something longer than a sibling.
- **Ask the person to pick.** Already what happens when more than one operable thing
  matches. Using it here would mean asking someone to choose between a light and its
  cloud-connection sensor, which is not a choice anyone should be offered.
- **Give up on direct reach's wider view.** Rejected. It is what makes entities hidden
  from the tool surface reachable at all, and this cost is narrow and now handled.
