# 0041 — Finding the thing the person meant

## Status

Accepted (2026-07-26). **Consolidates 0046, 0047 and 0048**, which were four days' worth of
refinements to one rule arrived at in one evening; each is now a stub pointing here.
Consumes the failure read-back from [0034](0034-evidence-verifies.md) and the reach of
[0042](0042-direct-reach-into-a-service.md).

## Context

A person says *turn on the kitchen table*. A model turns that into arguments. Those
arguments are wrong often enough that this document exists.

Endora already read a service's state back whenever an action **failed**
([0034](0034-evidence-verifies.md)), on the reasoning that "a failed action's most useful
output is what actually exists". The information was in hand and unused: the whole reading
went to the model, which then had to find one line in it and copy that line exactly.

Measured over fourteen consecutive attempts at a light called `Kitchen Table`, with
`names: Kitchen Table` in the reading every single time, the model never once sent that
string. It searched the **argument space** instead — `domain` flipped between `light` and
`switch`, an invented floor, an invented area, the noun moved into `device_class`, the name
left empty. It never did the search a person would: read the names, find the one that
resembles the request, copy it.

Why is visible in the data. The reading is ~5KB of forty-odd entities arriving as **one
unbroken line** — zero real newlines, 255 escaped ones, because the service JSON-wraps its
text. Finding one line in that and reproducing it exactly is the thing this model is worst
at, and [0037](0037-disclosure-not-persuasion.md) settled that prompting is not where a
guarantee goes.

## Decision

**When a call fails, Endora searches the service's own reading for what the call was aiming
at. It always shows what it found; it acts only when one candidate is clearly the answer.**

### Two layers, because showing and acting deserve different thresholds

- **Shortlist — always.** The failure carries the resembling names instead of the whole
  reading. Copying from three lines is a different task from searching five kilobytes.
  Nothing acts, so no threshold is needed.
- **Act — only when unambiguous.** The rules below. A guess that actuates something is what
  [0024](0024-reversibility-bands.md) exists to prevent.

### What counts as the answer

Four signals, each added because a specific request failed without it. In order:

| signal | what it asks | the failure it came from |
| --- | --- | --- |
| **matched words** | how much of the request does this name contain? | the original: nothing ranked candidates at all |
| **leftover words** | how much of *itself* does the request not account for? | `Kitchen Main Light LED` tying with `Kitchen Main Light` |
| **operability** | could the tool actually act on this? | a cloud-connection sensor tying with the light it belongs to |
| **uniqueness** | does exactly one candidate survive? | everything: a tie is a coin flip |

The first two rank; the third breaks a tie and can only ever *remove* a candidate; the
fourth is the gate. A single shared word counts only when nothing else in the reading
resembles the request — one word in common is a coincidence.

**Ranking, not completeness.** Requiring a candidate to contain *every* word looked
stricter and was wrong: it made the decision hostage to whether an unrelated entity
elsewhere shares a word with the request. "The guest bedroom left lamp" would not act on
`Guest Bedroom Left` — matching three words of four — because a **living room lamp** exists
in a different room. What makes acting safe was never completeness. It is that nothing else
comes close.

**Tightness, not brevity.** `Kitchen Main Light` and `Kitchen Main Light LED` match a
request for the former equally well, and are not equally good answers: one **is** the
request, the other is the request plus a word nobody said. Leftover words are the signal;
shorter-name-wins is a coincidence that happens to agree here.

**Operability narrows, never resolves.** It runs only after ranking has failed to separate
candidates, so it cannot overrule a winner, and if more than one operable thing remains the
ambiguity was real. A diagnostic can only lose; it can never cause something to be chosen.

### A kind the service never uses is part of the target

Arrays were skipped on reasoning that was correct — *an array restricts which kinds count,
it does not point at one* — and that holds exactly while the array holds a **kind**:

```text
"turn on the kitchen table"
  -> {area: "kitchen", device_class: ["table"], domain: ["light"]}
```

No name at all. The model read "kitchen table" as a room plus a sort of thing. There is no
category called `table`, so the service ignored the word and acted on what remained — *every
light in the kitchen*, two of them, reporting success.

So a filter value the service has never used as a category is treated as part of what was
named. **The service supplies the vocabulary** — every domain it has and every device class
it uses, read off its own data, never a list of kind words in Endora's source. Where there
is no vocabulary there is no change: guessing would make every ordinary `domain: ["light"]`
pour "light" into the search.

This changes what is **searched**, never what is **sent**. A heuristic may inform a search
without being allowed to aim an action.

### Pure text, so it belongs to no integration

A reading is fragments split on newlines and JSON structure (including the literal `\n` a
JSON-wrapped service produces — a fact about transport, not about one service). A fragment's
value is what follows its first colon. A candidate shares whole words with the call's
arguments. A calendar whose event is really `Endora Syncup`, and a filesystem with a
misspelled path, are searched by the same functions.

### It never widens, and it does not guess which field the name goes in

A retry **keeps every kind filter** and drops only scalars the real name already contains —
`area: "kitchen"` adds nothing to `name: "Kitchen Table"`. The rule is written in blood: an
earlier argument-hygiene change turned `{area: null, name: null, domain: ["light"]}` into
"all lights" and switched on every light in the house.

A call does not say which field means "the name", and deciding that would be per-service
knowledge. So the name is tried in **each** scalar field the call carried, capped at three
placements, first success wins. A wrong placement fails to match and changes nothing — which
is what makes searching this way safe.

Where [direct reach](0042-direct-reach-into-a-service.md) exists, none of that is needed:
the matched name resolves to an id and the action goes out by id, which cannot mis-match.

### Recovery only, and disclosed

It fires **after** a failure, so it cannot hijack a call that was about to hit the right
thing; it is bounded; and it **says what it did**, so the substitution reaches the model,
the outcome record and the person. Ordering follows [0038](0038-capability-profiles.md)'s
trust ranking: the person's **confirmed** answer is tried first, and only then does Endora
use what it merely **observed**.

## Consequences

- Failures that took fourteen attempts and a person's intervention resolve inside one turn
  without asking anyone anything.
- The alias question of [0039](0039-capability-repair-proposals.md) becomes the fallback
  rather than the first resort — asked only when the search is ambiguous or empty.
- A service with **no nominated reader** searches nothing and behaves exactly as before:
  the honest silence of [0038](0038-capability-profiles.md), and why none of this needs
  per-service code.
- A failed call may cost a handful of extra local calls. All on a path that had already
  failed; none on a working call.
- **Four rules now decide one thing.** Each arrived from an observed failure and each has a
  test against a real house. A fifth should be treated as evidence that ranking names is the
  wrong instrument, not as the next refinement.

## What was tried and rejected

- **Telling the model to read the list before retrying.** That is what the unscoped
  read-back already amounted to; it failed fourteen times out of fourteen with the answer
  in context.
- **Parsing the service's response format** to extract entity names. Learning one service's
  shape is what [0038](0038-capability-profiles.md) exists to stop; word overlap over
  arbitrary text degrades gracefully instead.
- **Requiring every word to match** — hostage to unrelated entities, see above.
- **Preferring the shortest name** — a coincidence dressed as a rule.
- **Acting on the best candidate when several tie** — the gap between "one thing matches"
  and "several do" is the gap between a lookup and a coin flip.
- **Filtering non-controls out of the reading** so ties never form. The shortlist is what
  the person and the model *see*, and "these exist and look like what you asked for" should
  be honest about what exists. Filtering belongs at the moment of choosing to act.
- **Dropping kind filters on retry** so a light/switch mix-up also recovers. Widens what a
  call can hit; already caused one house-wide incident.
- **Moving an unrecognised word into the name field.** Which field is "the name" is
  per-service knowledge, and the word is there because the model was already confused about
  fields.
- **Asking the person every time.** Kept as the fallback; rejected as the first resort
  because it puts the work on them for something Endora can look up.
