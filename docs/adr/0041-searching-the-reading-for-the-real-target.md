# 0041 — Searching the reading for the real target

## Status

Accepted (2026-07-26). Completes the recovery path begun in
[0039](0039-capability-repair-proposals.md), and makes its alias question mostly
unnecessary. Consumes the failure read-back from [0034](0034-evidence-verifies.md).

## Context

[ADR 0034](0034-evidence-verifies.md) already reads a server's state back when an action
**fails**, deliberately unscoped, on the reasoning that

> a failed action's most useful output is what actually exists, which is what lets the
> model retry against reality instead of guessing again.

The information is therefore in hand at exactly the right moment. What was missing is
that Endora handed over the *whole* reading and hoped the model would use it.

Measured, over fourteen consecutive `HassTurnOn` attempts at a light called
`Kitchen Table`, with `names: Kitchen Table` present in the reading **every single
time** — the model never once sent that string. What it sent instead:

| attempt | what it varied |
| --- | --- |
| `name:"table"`, `domain:["light"]` | — |
| `name:"table"`, `domain:["switch"]` | flipped the domain |
| `name:"table light"` | reworded the name |
| `name:"ceiling light"`, `floor:"1st"` | invented a floor |
| `area:"Living Room"` | changed the room |
| `area:"all"` / `area:"light"` | invented areas |
| `device_class:["table"]`, `name:""` | moved the noun into a filter |

It **is** searching — over the *argument* space, guessing which combination of filters the
server wants. It never does the search a person would: read the list of names, find the
one that resembles the request, copy it exactly.

The reason is visible in the data. The reading is ~5KB containing 40-odd entities, and it
arrives as **one unbroken line**: zero real newlines, 255 escaped ones, because the server
returns its text inside JSON. Finding one line in that and reproducing it exactly is the
thing this model is worst at, and no prompt fixes it —
[0037](0037-disclosure-not-persuasion.md) and the measured 1-in-3 instruction-following
settled that argument.

## Decision

**When a call fails, Endora searches the server's own reading for what the call was aiming
at. It always shows what it found; it retries only when exactly one thing matches.**

### Two layers, because showing and acting deserve different thresholds

- **Shortlist — always.** The failure result carries the names that resemble the request,
  instead of the whole reading. Copying from three lines is a different task from
  searching five kilobytes. Nothing acts, so this needs no threshold at all.
- **Retry — only when unambiguous.** Exactly one candidate may contain **every** word the
  call was aiming at. Two plausible names is not a search result, it is a guess, and a
  guess that actuates something is precisely what [0024](0024-reversibility-bands.md)
  exists to prevent.

Words the server **never uses** are excluded before that test (added the same day, from
the first live run). "Turn on the guest bedroom left lamp" found `Guest Bedroom Left` and
then refused to use it, because nothing in the house is called a *lamp* and the match was
therefore not complete. The person's vocabulary is not the server's, and a word appearing
nowhere in the reading cannot tell one candidate from another.

That stays safe because **uniqueness**, not completeness, is what makes a retry safe:
"the garage lamp" also loses *lamp*, and is left with *garage*, which matches several
things — so nothing is retried, exactly as before.

### Pure text, so it belongs to no integration

The search knows nothing about Home Assistant, YAML, or any schema:

- a **reading** is fragments, split on newlines and JSON structure (including the literal
  `\n` a JSON-wrapped server produces — a fact about transport, not about one server);
- a fragment's **value** is whatever follows its first colon, or the whole fragment;
- a **candidate** is a value sharing whole words with the call's scalar arguments;
- **arrays are not part of what was aimed at**, for the reason the read-back already
  ignores them — a scalar points at something, an array restricts which kinds count.

A calendar whose event is really `Endora Syncup`, and a filesystem with a misspelled path,
are searched by the same functions. This is the general version of the fix
[0040](0040-withdrawing-a-capability-that-never-works.md) demanded, and the reason the
per-integration alternative was rejected there too.

### It never widens a call

The rule written in blood: an earlier argument-hygiene change turned
`{area:null, name:null, domain:["light"]}` into "all lights" and switched on every light
in the house. So a retry **keeps every kind filter**, and drops only scalars the real name
already contains — `area: "kitchen"` adds nothing to `name: "Kitchen Table"`, and leaving
it in is how a retry re-fails on a fragment of the answer.

### Which field the name goes in is not guessed

A call does not say which of its fields is "the name", and deciding that would be exactly
the per-server knowledge this avoids. So a real name is tried in each field that currently
holds a fragment of it, most specific first, capped at three placements, and the first that
works wins.

That is a real search rather than a guess, and it is safe for a specific reason: a
placement that is wrong **fails to match and changes nothing**. The cost of being wrong is
one refused call.

### Recovery only, and disclosed

Identical posture to the alias recovery in [0039](0039-capability-repair-proposals.md),
for identical reasons: it fires only **after** a failure, so it cannot hijack a call that
was about to hit the right thing; it is bounded; and it **says what it did**, so the
substitution reaches the model, the outcome record and the person. The model's mistake
still happened, is still recorded, and is still measurable by the eval battery
([0028](0028-native-tool-calling-turn.md)).

Ordering follows [0038](0038-capability-profiles.md)'s trust ranking exactly: the
**confirmed** alias is tried first, and only then does Endora go looking through what it
merely **observed**.

## Consequences

- The failure that took fourteen attempts and a person's intervention now resolves inside
  one turn, without asking anyone anything.
- The alias question from [0039](0039-capability-repair-proposals.md) becomes the fallback
  rather than the first resort — asked only when the search is ambiguous or empty. Endora
  finds the real name itself instead of putting the work on the person.
- A server with **no nominated reader** searches nothing and behaves exactly as before.
  The honest silence of [0038](0038-capability-profiles.md), and the reason none of this
  needs per-server code.
- Every failed call may now cost up to three extra calls to the same tool plus one read.
  Bounded, and only on a path that had already failed.
- **It can retry against the wrong thing.** The name must be unique and complete, so this
  requires a reading that contains exactly one thing matching every word the person's
  request used. When it happens, the result says what was substituted, and the action was
  already in a band cleared to run.
- The search can only find what the reader reports. A partial reading yields partial
  candidates — visible as a shortlist that does not contain the answer, rather than as
  silence.

## Alternatives considered

- **Tell the model to read the list before retrying.** This is what the existing
  unscoped read-back already amounts to, and it failed fourteen times out of fourteen with
  the answer in context. Prompting is not where a guarantee goes
  ([0037](0037-disclosure-not-persuasion.md)).
- **Parse the server's response format to extract entity names.** Rejected in
  [0039](0039-capability-repair-proposals.md) and still rejected: it means learning one
  server's shape. Word overlap over arbitrary text needs no such knowledge and degrades
  gracefully when the shape is unfamiliar.
- **Retry on the best candidate even when several match.** Rejected. The gap between "one
  thing matches" and "several do" is the gap between a lookup and a coin flip, and this
  path actuates.
- **Ask the person every time instead** (0039's alias, unchanged). Rejected as the *first*
  resort: it puts the work on the person for something Endora can look up. Kept as the
  fallback.
- **Drop the kind filters on retry** so a light/switch mix-up also recovers. Rejected for
  now — it widens what a call can hit, which has already caused one house-wide incident.
  A separate decision if the shortlist proves insufficient.
