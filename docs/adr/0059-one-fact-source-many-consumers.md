# 0059 — One fact source, many consumers

## Status

Proposed (2026-08-02). Completes [0058](0058-how-an-integration-gets-in.md), which said how an
integration gets *in* without saying what it then gets for free.

## Context

[0058] made registering a local integration a single line. It did not make the integration
*useful*, and the difference was invisible until somebody asked the right question:

> *"when we add features it should all be plumbed to the chat. Like every skill mcp etc right?
> If I want next door integration to read it should use it with the daily learning but also be
> queryable in the chat."*

The honest answer was no. What a new source actually got:

| | built-in skill | MCP server | native channel |
| --- | --- | --- | --- |
| the model can call it in chat | yes | yes | yes |
| it feeds notions and the learning loop | no | yes | yes |
| it is **watched** — trouble, and change | no | **no** | yes |
| it is **stated** in a turn unprompted | no | **no** | Home Assistant only |

Three consumers each reached for a different thing. The watch loop iterated native channels.
Notions read `current_states`. The turn read `about_the_person` — a method **exactly one
integration in the system has ever implemented**.

So a Nextdoor server would have been callable and would have fed the thinking, while nothing
watched it and nothing ever mentioned it. Reachable in principle, silent in practice, waiting
for a weak local model to think of calling it.

**That is the morning brief's lesson, ungeneralised.** One instruction to *"reach for whatever's
relevant"* produced four days of briefs about the kitchen lights, because every fact worth
having was already reachable and all of it was left to the model to go and get. That was fixed
for the brief, for Home Assistant, once — and the shape of the mistake was never removed.

## Decision

### Say what you currently know. Everything else follows.

**One question — `current_states` — and three consumers derive from it.** A source implements
that and is thereby watched, heard in a turn when it changes, and available to the thinking.
There is no second thing to implement, nothing to nominate, and no line of wiring per
integration.

```text
        any source: current_states()
                     │
    ┌────────────────┼────────────────┐
    │                │                │
 watch loop      the turn          notions
 trouble +     what changed,      records to
 transitions     bounded          think with
```

### Keys carry their source

`server::thing`, everywhere. Two servers may publish the same resource name, and a flat key
would have one silently overwrite the other — with both still appearing present, which is the
kind of loss nothing downstream can detect.

It also means a fact says where it came from without anybody having to be told, which is what
lets one source feed three consumers that know nothing about each other.

### The turn hears what **changed**, not what **is**

An inventory would have to be opted into; a short list of what moved does not.

*"The back door opened ten minutes ago"* is worth a turn. *"`light.hall` is off"* is not, and
forty of those in front of every answer would make the context worse for a weak model rather
than better — which is exactly how this ends up as something the person has to nominate per
source, and then does not.

Bounded twice, and both bounds are load-bearing: **six hours** and **six changes**, newest
first. The budget is the entire reason this can be automatic.

### `about_the_person` stays, for prose

A source with something better to say than a raw state pair may still phrase it — *"2 things on
your Reminders list"* reads better than `todo.reminders 2`. That is now an **optimisation, not
the price of admission**. Implement nothing and you are still watched, still heard when you
change, still feeding the thinking.

## Consequences

- **Adding an integration is one line and no wiring.** A source that only says what it knows is
  fully joined up.
- **MCP servers are watched.** Trouble detection and the transition log now cover them, so a
  third-party server that stops answering is noticed like anything else.
- **Every turn carries recent change**, from every source, without the model choosing to look.
- A source that publishes many volatile resources will generate many transitions. Bounded in
  the turn, and bounded in storage by the fortnight window — but a genuinely chatty server
  could still make the log noisy, and the honest answer today is that nobody has one.
- **Reads cost.** Every watch pass reads every resource of every server. Capped per server, and
  the loop runs every two minutes, but this is a real per-integration cost that grows with the
  number of sources rather than with the size of any one.

## Rejected

- **Nominating what is worth stating, per source.** The obvious design, and it fails the actual
  requirement: the person would have to integrate each one by hand, which is the thing being
  removed. It also repeats the brief's mistake in a new costume — a fact that is reachable only
  if somebody remembers to reach for it.
- **Stating every current fact in every turn.** Affordable for a house of forty entities and
  not for anything larger, and it degrades a weak model's answers with an inventory it did not
  ask for.
- **A new trait for "things that can be watched".** `current_states` already is that trait.
  Adding a second one would mean two ways to be a source, and integrations implementing the
  wrong one — the failure this record exists to remove.
- **Making the watch loop reach into MCP directly.** It would work and it would be the
  per-integration branch [0054](0054-other-peoples-services.md) forbids: the loop would know
  what kinds of source exist, and the next kind would need it changed again.
