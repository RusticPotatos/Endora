# 0048 — The tightest match wins

## Status

Accepted (2026-07-26). Completes the ranking of
[0041](0041-searching-the-reading-for-the-real-target.md); the case
[0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md) could not reach.

## Context

[ADR 0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md) broke ties by asking
whether the tool could operate a candidate, and predicted that a diagnostic "can only ever
lose". Deployed, *turn off the kitchen main light* failed again. The house explains why:

```text
light.kitchen_kitchen_main_light   Kitchen Main Light
switch.kitchen                     Kitchen Main Light            <- same name
switch.kitchen_led                 Kitchen Main Light LED
binary_sensor.kitchen_cloud_...    Kitchen Main Light Cloud connection
```

The connectivity sensor did lose. **The LED did not** — it is a `switch`, which is
perfectly operable, so two operable candidates remained and the tie stood.

Operability was the wrong question for this pair. `Kitchen Main Light` and
`Kitchen Main Light LED` both contain every word of the request, so
[0041](0041-searching-the-reading-for-the-real-target.md)'s score is equal — and yet
nobody would call them equally good answers. One **is** what was asked for. The other is
what was asked for **plus a part nobody mentioned**.

## Decision

**A candidate that accounts for all of its own words beats one with words left over.**

Two numbers now describe a candidate: how much of the *request* it matches, and how much
of *itself* the request did not account for. Ranking is the first descending, then the
second ascending, and a tie requires **both** to be equal.

```text
request: kitchen main light

Kitchen Main Light                   matched 3, extra 0   <- wins
Kitchen Main Light LED               matched 3, extra 1
Kitchen Main Light Cloud connection  matched 3, extra 2
```

### It breaks looseness, not ties

Two candidates that are each *exactly* what was asked for remain a genuine ambiguity and
nothing is acted on. The rule does not make ambiguity go away; it stops a **longer name
that merely contains the request** from masquerading as an equal answer.

### It does not require a tight match

A loose match still wins when it is the only sensible one — "guest bedroom left lamp"
still resolves to `Guest Bedroom Left` despite the leftover word. Tightness is a
preference between candidates, not a threshold a candidate must clear.

### Two things can genuinely share a name

This house has a `light` and a `switch` both called `Kitchen Main Light` — the same
duplication that made the original "is it a light or a switch?" confusion so hard. They
dedupe to one candidate, and the action goes to the first thing the service lists under
that name. Acceptable because both are that ceiling light, and the alternative — refusing
whenever a service names two things identically — would refuse a request the person
considers completely unambiguous.

## Consequences

- Devices whose accessories are named after them ("… LED", "… Cloud connection", "…
  Battery") stop swallowing requests for the device itself. This is a common shape, not a
  quirk of one house.
- The ranking is now three ideas deep — matched, leftover, then length — and that is
  enough. Any further tie is a genuine one and belongs to the person.
- [0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md)'s operability rule stays
  and is still load-bearing: it removes candidates that are not controls at all, which
  this rule would happily rank. They solve different halves.
- **Four rules now decide one thing.** Worth naming as a cost: matched words, leftover
  words, operability, and uniqueness. Each arrived from a specific observed failure and
  each has a test, but the next one should be viewed with suspicion — a fifth would be a
  sign that ranking names is the wrong instrument.

## Alternatives considered

- **Prefer the shortest name.** Already the third tiebreak, and rejected as the primary
  rule for the reason [0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md) gave:
  it is a coincidence here, not a principle. Leftover *words* is the actual signal.
- **Require an exact match.** Rejected — "guest bedroom left lamp" would stop working, and
  people do not name things the way they refer to them.
- **Ask when a device has accessories.** Rejected as asking someone to choose between a
  light and its own LED indicator, which is not a real choice.
- **Filter accessories out of the reading.** Same rejection as
  [0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md): the shortlist should be
  honest about what exists.
