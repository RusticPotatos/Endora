# 0060 — What the turn is offered

## Status

Proposed (2026-08-02). Completes [0058](0058-how-an-integration-gets-in.md) and
[0059](0059-one-fact-source-many-consumers.md), which made it cheap to add a source and
free to be heard by the turn — and thereby made this the next thing to break.

## Context

Asked *"any events in New York this week"*, the butler answered:

> It seems like you're asking about events in New York this week, but the functions
> provided are from Home Assistant and may not directly address external event information.

That is false. It was offered **37** tools:

| | |
| --- | --- |
| 9 | built-in skills — including `local_events`, `news`, `web_fetch`, `knowledge` |
| 20 | Home Assistant |
| 8 | Brave Search |

Every one configured, every one allowed. It reached for none of them and wrote an apology
about the block it saw most of.

Nothing here is unwired. **The catalogue is the failure.**

### This is measured, not a hunch

Published work through 2026 puts the audit threshold at roughly **30 tools**, with selection
accuracy degrading measurably from 20–25. And the fix is not a better model:

> Anthropic's Tool Search: Claude Opus 4 **49% → 74%** correct tool selection.
> Opus 4.5, 79.5% → 88.1%.

**Same weights.** Deferral is plumbing, and the improvement it buys is larger than the
generational gap it sits on top of. This repository runs a 7B model on a NAS, which is
below every number above and cannot be fixed by waiting.

Endora's own eval said it first, before any of this was read:

> `HASS_TOOLS` alone is a curated seven, and the model picks correctly from it every time.
> The live butler, asked to turn off the kitchen light, reached for `HassLightSet` — so the
> thing the curated list fails to reproduce is the **crowding**.

### And the record says which tools deserve to be offered

The outcomes table, live, at the time of writing:

```text
capability                    worked   didn't   errored
home-assistant.HassTurnOff       4        5         4
home-assistant.HassTurnOn        4        2         2
home-assistant.HassLightSet      0        2         3     ← never once
home-assistant.GetDateTime       0        3         0
home-assistant.HassBroadcast     0        1         0

overall: 8 of 30 confirmed a real change
```

`HassLightSet` has never worked, in five attempts, and it is exactly the tool the model
grabs for *"turn off the kitchen light"* — **because its description is a perfect match**.

Every published fix ranks tools by what they *claim*: OpenClaw matches a skill's
description, Tool Search does semantic search over descriptions, Code Mode searches a typed
API surface. All of them would rank `HassLightSet` first, permanently, and none of them
would ever find out. They are stateless retrievers; they never learn whether the call
worked.

Endora **knows**, because [0053](0053-honesty-about-what-it-did.md) made it check. `changed`
is a ground-truth label on a tool choice, recorded on every action and used for nothing.

## Decision

### Defer, never delete

The turn always offers a small set, and everything else is reachable through one lookup.

The distinction is the whole record, because the two are indistinguishable when the routing
is right and opposite when it is wrong:

| | when routing is wrong |
| --- | --- |
| **delete** — the tool is not there | unrecoverable, and silent. The model cannot ask for what it cannot see, and the person gets an apology. |
| **defer** — the tool is one lookup away | recoverable. It costs a round-trip. |

A description that does not match how the model phrased its intent is the acknowledged
failure of every retrieval scheme, including the ones that work. Choosing the recoverable
failure is not caution, it is the only version of this that may be wrong in production —
which it will be.

### Readers stay; actuators are scoped

Of Home Assistant's 20 tools, **3 read** and **17 act**. *Is anyone home* is worth having in
front of a question about anything; `HassBroadcast` is not.

So the split is **not** by domain, which was the tempting version. It is by what a tool
does:

- a **read** is cheap, its result is evidence ([0053]), and it is useful far outside its
  own subject — so it stays;
- an **actuator** is only ever wanted when the request is about its subject — so it defers.

This cuts 37 to roughly 15 before anything clever happens, and it needs no judgement about
what any particular server is for.

### A source says what it is about. Nothing reads its name

Scoping needs a subject, and the subject is **declared**, exactly as
[0058](0058-how-an-integration-gets-in.md) requires: `home-assistant` says *home*, Brave
says *web*, Legistar says *civic*.

`if server == "home-assistant"` is the same mistake in a new place — the twelfth
integration would need the thirteenth branch, and
[0054](0054-other-peoples-services.md) exists because that is how this ends. Declared
metadata makes routing work for a source nobody has written yet.

### Rank by what has worked, not by what is claimed

Candidates are ordered by their **confirmed** record: `changed: true` against attempts, from
outcomes already stored.

This is the half no published approach can copy, and the reason is structural rather than
clever — they are stateless retrievers over a catalogue, and Endora is one long-lived
person's system with read-back and a night pass that already runs. A tool that has never
once worked should not be the first thing offered for the request it keeps failing.

**Unseen decays to neutral, never to blocked.** A new tool has no record and must not be
punished for it, which is the same rule as a new tool on a trusted server being opened:
absence of a decision is not a decision.

### Nothing here is an authorization boundary

Offering is about attention. **Allowed/blocked stays exactly where it is** — per tool, from
the person, deny-by-default ([0051](0051-where-the-boundary-is.md)).

A deferred tool is still blocked if it was blocked. A ranked-first tool still asks before it
acts. Conflating the two would turn a relevance heuristic into a permission, which is the
one thing a heuristic must never become.

### The catalogue shrinks as the system grows

[0059](0059-one-fact-source-many-consumers.md) already puts what changed in front of every
turn without the model asking. **Every recurring read that becomes a fact is a tool that
stops needing to be offered.**

That is the only mechanism here that improves with scale rather than degrading under it, and
it is worth stating as a decision rather than leaving as a side effect: when a read is
wanted often, it belongs in the fact stream, not in the catalogue.

### It is a wish until it is a number

Two, both of which exist already:

- the **eval battery**, whose `crowded_catalogue` case is this exact failure — before and
  after, as a count;
- the **live confirmed-change rate**, 8 of 30 at the time of writing, which is a standing
  measurement of whether the butler picks tools that work.

A ranking that claims to improve with time and is not measured over time is a story. This
record is not accepted until both move.

## Consequences

- **A wrong guess costs a round-trip**, on a local model that is already slow. That is the
  price of the recoverable failure and it is paid on every turn the lookup is needed.
- **The catalogue stops growing without bound.** Adding a server no longer degrades every
  unrelated question, which is what made shipping Brave Search make the butler worse.
- **Ranking is cold for a long time.** 30 outcomes across 5 tools is nearly nothing, so
  structure carries this at first and evidence earns its weight slowly — the same arithmetic
  as a notion maturing ([0057](0057-thinking-between-turns.md)).
- **A rarely-used good tool ranks below a often-used mediocre one** until it has a record.
  Mitigated by decay-to-neutral, not solved.
- **Two more things to get wrong**, and both are invisible from a screen: a subject declared
  badly, and a record that says a tool fails when the settings were the problem.
- **The 7B ceiling remains.** Everything here raises the floor and none of it makes a small
  model good at choosing; that is what escalation is for.

## Rejected

- **Filtering the catalogue down.** The obvious version, and it fails on the day the router
  is wrong: the model cannot ask for what it cannot see, so the failure is silent, and the
  person receives an apology about the tools that were left.
- **Embeddings over tool descriptions.** What the published fixes do, and it is beaten here
  by declared structure plus a record of what worked — exactly, and with no model, no index
  to rebuild and no new dependency in the domain. Descriptions are also the thing that made
  `HassLightSet` look right.
- **Code execution instead of tool choice** (Code Mode, MCP-as-TypeScript). It scales
  better than anything in this record — two tools, forever. It also needs a sandbox this
  repository does not have, and asks a 7B model to write correct code rather than pick from
  a short list, which is a harder task and not an easier one. Revisit when there is a
  sandbox.
- **Waiting for a better model.** The measured gain was on unchanged weights, and the model
  here is far below the one that was measured.
- **Hiding an integration's tools by name.** A per-skill patch, forbidden by
  [0054](0054-other-peoples-services.md), and it does not survive the next integration.
- **Letting the person nominate which tools a turn may use.** It would work and it makes
  them the system's administrator, which is what
  [0052](0052-what-it-knows-about-you.md) deleted a whole feature for.
- **Ranking by how often a tool is used.** Popularity is not correctness: `HassLightSet` is
  the *most*-reached-for wrong answer in the table above.
