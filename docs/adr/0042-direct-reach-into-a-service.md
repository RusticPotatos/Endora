# 0042 — Direct reach into a service

## Status

Accepted (2026-07-26). Builds the "named per-integration adapter" that
[0038](0038-capability-profiles.md) called for and never created. Makes
[0041](0041-searching-the-reading-for-the-real-target.md)'s search a fallback rather than
the load-bearing mechanism.

## Context

Every failure this week has been the same failure: a name not matching. `table`,
`table light`, `ceiling light`, `main light`, `kitchen`, `all`. Each one produced a fix —
a confirmed alias ([0039](0039-capability-repair-proposals.md)), then a search over the
server's reading, then a ranking rule for that search, corrected three times against the
live house.

All of it is scaffolding around a question that **does not arise one layer down**.

Home Assistant exposes sixteen tools over MCP. Every one is an *Assist intent*: the voice
assistant surface, which takes a spoken-sounding name and matches it fuzzily. There is no
identifier anywhere in that surface, and nothing in it can read or change configuration.
Endora has been talking to the house through the hatch built for voice.

The same service's own API has none of that ambiguity. `light.kitchen_table` is an id. It
cannot be confused with `Kitchen Main Light`, cannot be mis-spelled into a `MatchFailedError`,
and does not care what anybody calls it. And `GET /api/states` returns **every** entity,
including ones deliberately hidden from Assist — things Endora previously could not find
no matter how well it searched, because they were never in the reading at all.

The credentials were already there. A `home_assistant` skill has existed for months,
storing a URL and a long-lived token and reading `/api/states` — used only to *learn
routines*, never to act, and switched off.

## Decision

**Where Endora is given a service's own interface, it uses it: to see what exists, and to
act on exactly one thing by id.**

### A port, not an integration

Everything above the adapter speaks to a `NativeChannel` with three methods — *what
exists*, *the same as text*, *act on one of them* — and never learns which service it is.
Home Assistant's specifics live in one named module: the URL shape, the token header, the
`/api/states` response, and the single mapping from an Assist intent to the service that
performs it.

Deliberately **not** "run arbitrary API calls". A general remote-call capability would be
a much larger grant, and none of the failures needed it.

### It is recovery, and it inherits every existing gate

Direct reach fires in exactly one place: after a tool call has **already failed**, and only
when the search finds one unambiguous match ([0041](0041-searching-the-reading-for-the-real-target.md)).
It cannot hijack a working call, it does not appear in the model's catalogue, and it adds
no new decision for the model to get wrong.

The exactness does **not** lower the bar for acting. Having ids available makes a chosen
target certain; it does nothing to make *choosing* safe. A tie is still a tie, and still
acts on nothing.

### Fallback, never a new way to fail

A channel that cannot express a tool returns nothing and the existing retry runs. Only
switching on and off is mapped; brightness and colour carry arguments this does not
attempt to translate, and a half-translated action is worse than none.

### A reply is not an outcome (corrected 2026-07-26, from the person noticing)

Home Assistant answers a service call with the list of states it changed, and that list is
routinely **empty for calls that worked** — it answers before its integrations report back,
so the list describes timing rather than result.

Read as an outcome, it produced a false record: a light that verifiably turned off was
stored as *"was already as asked, so 'turn off' changed nothing"*. The person saw the light
go off while Endora said it had done nothing.

So the reply now claims only what it can support — the call was accepted — and the
**read-back** settles whether anything changed, which is what
[0034](0034-evidence-verifies.md) built it for. Endora already had the right instrument and
was overriding it with a worse one.

### Ids stay out of the matching text

Learned immediately, and worth recording: `light.kitchen_table` contains the words
"kitchen" and "table", so including ids in the reading makes each id compete with its own
entity's name and turns every unambiguous match into a tie. The reading carries names; the
mapping to ids is a separate lookup. That is what it is for.

### Which server it is paired with is data

The MCP server's name is chosen by whoever registered it, so the pairing is a setting with
a sensible default, not a constant in Endora's wiring. A name hardcoded here would be
precisely the per-integration guessing [0038](0038-capability-profiles.md) rules out.

## Consequences

- The name-matching failure class **ends** for any service Endora has reach into. Not
  mitigated, not searched around — the retry aims at an id.
- Endora can reach things the tool surface never exposed. This is the larger gain and the
  less obvious one: no amount of searching finds what was never in the reading.
- [0041](0041-searching-the-reading-for-the-real-target.md)'s search remains, and is still
  what makes this work — the channel answers *which thing*, the search answers *which name*.
  Without reach it is unchanged.
- **Endora now holds credentials to a service and uses them outside the person's direct
  request.** The gate is that reach exists at all: no URL or token, no channel, and every
  behaviour is exactly as before. Clearing the token removes it.
- The `home_assistant` skill's settings now do two jobs — a skill's configuration, and
  Endora's own connection. Worth naming as a wart: the skill being switched off stops it
  being offered to the model ([0040](0040-withdrawing-a-capability-that-never-works.md))
  and does **not** withdraw the connection.
- This is the foundation for changing the service's own configuration — writing aliases
  into the entity registry so every client benefits, which was named as the destination in
  [0039](0039-capability-repair-proposals.md) and still needs its own decision, its own
  undo records, and its own gates.

## Alternatives considered

- **Add native tools to the model's catalogue.** Rejected. Crowding the catalogue is a
  measured cause of wrong-tool choice, and this needs no new decision from the model at
  all — it is plumbing under a failure that already happened.
- **Replace the MCP server entirely.** Tempting, and wrong for now: the MCP surface carries
  media, broadcast and timers that would all have to be re-implemented, and the tool
  catalogue is also how the person sees and gates what the butler can do.
- **A general "call any HTTP API" capability.** Rejected as a far larger grant than any
  observed failure required, and impossible to band honestly — one endpoint reads, the
  next one deletes.
- **Keep improving the search instead.** It was corrected three times in one day and would
  still never find an entity hidden from the tool surface. The search is good; it was
  solving the wrong problem.
- **Ask the person to add aliases in Home Assistant by hand.** Correct advice, already
  given, and it does not scale to a butler that is supposed to own its own tooling.
