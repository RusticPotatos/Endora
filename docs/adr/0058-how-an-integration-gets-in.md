# 0058 — How an integration gets in

## Status

Proposed (2026-08-01). Extends [0054](0054-other-peoples-services.md).

## Context

Endora reaches other people's services two ways, and there has never been a rule for choosing.

It has an **MCP host** ([0021](0054-other-peoples-services.md)) — a row in a table, no code —
and a handful of **native capabilities** compiled into the binary. Home Assistant is
*both* at once, which looks like indecision and turns out to be the answer.

The question arrived as *"when should we use MCP and when should we have built-in?"*, and the
honest state of things was that nobody could have said. Worse, the built-in path was not a
path at all. The composition root assembled the one existing channel **by name**:

```rust
let Some(home_settings) = settings.get("home_assistant") else { return Vec::new(); };
let Some(home) = HomeAssistant::from_settings(home_settings) else { return Vec::new(); };
vec![(server, Arc::new(home))]
```

Three faults in nine lines. It **early-returns on the first miss**, so a second integration
could not have been reached even after somebody wrote it. It lives in `app/node`, so the
composition root has to know each integration by name — backwards for [0050](0050-the-shape-of-the-system.md),
where dependencies point inward. And the alias wiring is inlined and Home-Assistant-specific,
which is the per-integration patch in shared code that [0054] exists to forbid.

## Decision

### MCP is the default; native has to earn it

**MCP's protocol expresses exactly one thing: here are tools, call them.** Everything else
Endora needs from a service has nowhere to live in it.

| what Endora needs | expressible over MCP |
| --- | --- |
| call something, get an answer | **yes** |
| read the whole world, continuously, for the watch loop | **yes, via resources** |
| supply context for every turn (`about_the_person`) | no |
| carry a setup flow — *"your URL and a long-lived token"* | no |
| declare per-tool reversibility, so policy can gate it | no |
| write configuration back, and undo it | no |
| push a notification | no |

So the test is one question: **does Endora need a relationship with this service, or just
answers from it?** Answers → MCP. Relationship → native. Where both are true, do what Home
Assistant does and run both: native for the relationship, MCP for the verbs.

#### Correction, the same day: the watch loop *is* expressible over MCP

This record first claimed the watch loop had no MCP equivalent. **That was wrong.** MCP
standardises `resources/list`, `resources/read` and `resources/subscribe`; Endora's client
simply did not implement them, speaking only `initialize`, `tools/list`, `tools/call` and
`notifications/message`. Reading the protocol rather than the client would have caught it.

The row above is corrected, and resources are now implemented over both transports and
surfaced as `current_states`. The consequence is larger than a table cell: **a third-party MCP
server can feed the watch loop, the transition log and notions with no Rust in this repository
at all.** That is the plugin standard this project needs, and it did not have to be invented.

What still has no MCP expression is the person-facing half — a setup form, a config write with
an undo, presence phrased for a turn. Those keep native for now.

One thing deliberately **not** adopted: MCP tool annotations (`readOnlyHint`,
`destructiveHint`) look like reversibility, but they are a server describing itself, and
[0054](0054-other-peoples-services.md) already settled that a server announcing "I only read"
is not evidence of anything. They may inform a default; they may never authorize.

The default matters because the costs are wildly asymmetric. Native is Rust carried in every
release, owned forever, needing coverage in five test tiers, and breaking when the vendor
changes. MCP is a row in a table. **No native integration without one of the "no" rows above** —
the same instinct [`AGENTS.md`](../../AGENTS.md) applies to dependencies. *"It would be
neater"* is not a reason to write a thousand lines you then own.

A corollary worth stating, because it decides real cases: hardware that can reach Endora
*through* something already native is not a new integration at all. An alarm panel or a
network controller behind Home Assistant inherits the whole native side for free.

### Registration is a seam, not a wiring job

`Capability` gains one defaulted method:

```rust
fn channel(&self, settings, aliases) -> Option<(String, Arc<dyn NativeChannel>)> { None }
```

Every declared skill is asked; almost all say no. Registration runs off
`default_capabilities()` — **the same list every skill is already declared in** — so adding a
local integration is the line you were adding anyway, and the composition root learns nothing
new.

Three properties are load-bearing rather than incidental:

- **No early return.** One unconfigured integration must never silence the others. That was
  the actual bug, and it was invisible because there was only ever one.
- **"Not configured" is the integration's own judgement.** A skill with no settings is still
  asked, with an empty set, rather than the caller guessing which keys matter.
- **Aliases are handed over whole and the integration filters them.** Which confirmed names
  belong to it is knowledge only it has; deciding out here puts the per-integration branch
  straight back into shared code.

`NativeChannel` already had seventeen methods and only **three required** — `known`,
`reading`, `act` — so the trait was never the obstacle. Registration was.

### Adding one

1. implement `Capability` (`info` + `invoke`) — as any skill does;
2. implement `NativeChannel`, which is three methods unless more is wanted;
3. implement `channel()` to hand one back when configured;
4. add a line to `default_capabilities()`.

## Consequences

- A second local integration is now possible, which it demonstrably was not. There is a test
  for exactly that, and it could not have been written against the old shape.
- The node no longer names any integration.
- Home Assistant's quirks moved into `home_assistant.rs`, behind its own boundary, which is
  where [0054] says genuine quirks are allowed to live.
- **A skill can now supply a channel without anybody reviewing whether it should.** Accepted:
  the policy boundary is unchanged — a channel still acts only through capabilities the
  deterministic layer has cleared ([0051](0051-where-the-boundary-is.md)) — but the ease is
  new, and ease is how speculative integrations get written. The "MCP unless" default above
  is the counterweight, and it is a rule rather than a mechanism.

## Rejected

- **Native for everything, for consistency.** Consistency bought with a thousand lines per
  service, in a project maintained by one person.
- **Inventing a private state-reading extension to MCP.** Rejected before it was attempted,
  because the protocol already has `resources/*` — the correction above. A private extension
  would have been a fork of the standard, and would have made every third-party server
  incompatible with the one thing this project most needs from them.
- **A plugin ABI / dynamic loading.** Real modularity, real cost — versioning, unsafe
  boundaries, sandboxing — for a benefit that a `Vec` in one file already delivers. Revisit
  only when integrations arrive faster than releases.
- **A config file listing native integrations.** Data has to name a type the binary already
  contains, so it buys nothing over the `Vec` and adds a way to be wrong at runtime.
