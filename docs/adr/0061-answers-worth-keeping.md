# 0061 — Answers worth keeping

## Status

Accepted (2026-08-03). Shipped the same day it was proposed and has been serving repeated
questions since; accepted on that record. Originally proposed 2026-08-02. Sits beside [0059](0059-one-fact-source-many-consumers.md): that
record is about facts arriving unasked, this one is about answers being remembered.

## Context

Every skill that reaches outside asks the same question from scratch every time. Ask what is
on at a venue twice in a minute and the request goes twice; ask on Tuesday what was asked on
Monday and nothing remembers Monday.

That costs three things, and the third is the one that shapes the design.

- **Quota.** The sources this runs on are free tiers, and a free tier is the budget.
- **Time.** Every question waits for a round trip, on top of a local model that is already
  slow.
- **What can be attempted at all.** A butler that pays full price for every look-up cannot
  afford to look things up often, so it does not — and the honest consequence is a butler
  that knows less than it could.

The question that produced this record was *"why don't we subscribe, or fetch on a schedule,
so when it's asked we just know?"* — and the first answer was wrong.

### Prefetching a question is not possible; remembering an answer is

The reply was that facts can be prefetched and questions cannot: the house is forty things
and a snapshot is well defined, while *"what's on"* has no fixed subject — which city, which
venue, which week. That is true, and it is not an argument for anything, because
**cache-aside does not prefetch questions. It remembers answers.**

A venue in a city nobody anticipated is remembered the moment somebody asks. There is no
standing list of cities, no per-source cadence, and no decision made in advance.

The second suggestion — refresh things nobody has asked for in weeks — is declined for the
same reason inverted: that is quota spent on an answer nobody wants. The instinct behind it
is right and mis-aimed, because the case it imagines is the **briefing**, and a briefing is
an asker. It asks on a clock instead of by typing.

So there is no "nobody asked". There is only who asked, and how often:

> **Freshness is a consequence of demand, not a setting.**

## Decision

### Remember at the egress helper, not at the skill

Two functions carry every built-in skill's outbound GET. Ten call sites, one place.

The tempting layer is the capability runner, and it is the wrong one: `run` returns a string,
so the origin's own caching headers are gone by the time it is reached, and recovering them
would mean **every skill reporting its own freshness** — per-skill work, which is the thing
this must not become.

At the egress helper the headers are still in hand, and a skill written normally is cached
without opting in. A new skill does nothing to take part.

### The key is the question, because the URL already is

City, venue, window, category — all of it is in the URL. There is no cache key to design,
and therefore no way to design it wrong.

That matters more than it sounds. The obvious hand-rolled version keys on a skill id and some
arguments, and the failure it invites is serving one place's answer for another: not slowness
but **a wrong answer, delivered confidently**. HTTP settled this long ago.

### What is kept is a fingerprint, never the question itself

The URL contains the API key, and often the person's town. **Neither is stored.** What is
stored is a pair of independent hashes of the URL — enough to recognise the same question
again, not enough to reconstruct it.

The node already holds those secrets elsewhere and must; the rule this record adds is that
the remembering layer creates **no second place** they can be read from, and nothing to
redact in a log that does not exist. A stray debug print here cannot leak a credential
because there is no credential to print.

Two hashes rather than one because a collision would not be a slow answer, it would be the
**wrong** answer.

### How long is the origin's business, within bounds

`Cache-Control: max-age` decides it, per response, per source. No table of which source is
hourly and which is monthly — the table would go stale, and it is exactly the
per-integration knowledge [0054](0054-other-peoples-services.md) keeps out of shared code.

Bounded at both ends, because a source is not owed unlimited trust about its own freshness: a
floor so that a source claiming zero cannot make this pointless, and a ceiling so that one
claiming a year cannot make the butler wrong for a year. A source that says nothing gets the
floor.

### A stale answer beats no answer when the source is down

Quota exhausted, network gone, service down: if there is an old answer, it is served. The
alternative is nothing, and nothing is worse — the failure that has cost most in this
repository is a screen quietly saying it has no information when it does.

### Only GETs, and never the house

These two helpers are GETs, so nothing that acts can be cached. **MCP tool calls go through
their own transports and are untouched**, which is correct rather than a gap: Home Assistant
must be live, and remembering whether anyone is home would be a bug wearing an
optimisation's clothes.

### In memory, bounded, and gone on restart

No table, no migration, no purge wiring, no export. It refills on demand, and the
alternative — a cache on disk keyed by what somebody asked — is a **record of questions**,
which would have to be covered by *forget everything* and is an obligation not worth
creating for a performance win.

## Consequences

- **A repeated question is free**, including the same question tomorrow.
- **Free tiers stop being the limit on how often the butler may look.** That is the point:
  the constraint was never the value of asking, it was the price.
- **A new skill is cached without doing anything**, and there is no flag to forget to set.
- **An answer can be up to its ceiling out of date.** Accepted, and the reason the ceiling
  exists rather than trusting the origin.
- **The first ask is never faster.** Nothing here helps a question nobody has asked.
- **A restart forgets everything.** Accepted for what it removes: schema, migration and a
  privacy obligation.

## Rejected

- **Caching at the capability runner.** The layer where the origin's headers no longer
  exist, so it would need every skill to report freshness — a per-skill patch, and one that
  drifts silently when a skill forgets.
- **A per-source cadence setting.** The table somebody maintains, goes stale, and encodes in
  shared code what each integration is like. The source already knows and will say.
- **Refreshing what nobody asked for.** Quota spent on an unwanted answer. The briefing is
  an asker; when it stops asking, the answer stops mattering.
- **Conditional requests (`ETag` / `If-None-Match`).** Genuinely useful for bandwidth, and
  it buys little here: a revalidation still counts against most quotas, and the HTTP client
  treats a `304` as an error by default, so it would mean a second agent with different
  error semantics. Deferred rather than dismissed — worth adding when bandwidth is the
  constraint, which it is not.
- **Storing the URL as the key.** It is the obvious implementation and it would put an API
  key and a home town in a map for the convenience of debugging. A fingerprint answers the
  only question the cache asks — *have I seen this exact request?*
- **Persisting it.** Survives restarts, and creates a stored record of everything the person
  ever asked about. Not worth it for a warm start.
