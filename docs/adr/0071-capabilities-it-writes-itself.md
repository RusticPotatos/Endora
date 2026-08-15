# 0071 — Capabilities it writes itself

## Status

Accepted (2026-08-08). Proposed 2026-08-04; the three designs its acceptance
required — the recipe format, the proposal surface, and the gap-detection
arithmetic — are now each grounded below against the live record and the
mechanisms that shipped since ([0075](0075-failures-become-specimens.md),
[0076](0076-standing-questions.md)). Code may now ship in slices, each an
ordinary PR against this record. The last rung of the master plan, and the first
step onto the capability ladder's "infra skills" rung
([0055](0055-the-model-layer.md)).

## Context

Every capability Endora has, somebody built by hand. The butler can notice a gap — asked
weekly about a transit line it has no skill for, told repeatedly about a service it cannot
reach — and can do nothing about it except fail the same way next week. The north star is
a butler that closes its own gaps.

The dangerous version of this is obvious: a model writing code that runs. This repository
already knows what it thinks of that shape — the model is never the enforcement boundary
([0051](0051-where-the-boundary-is.md)), a prompt is not where a guarantee goes
([0053](0053-honesty-about-what-it-did.md)), and the week this was written produced four
records taking *facts* away from the model, let alone instructions.

But "the butler writes code" smuggles in an assumption worth refusing: that a new
capability is code at all. Every read-only skill Endora ships is the same five things — a
URL template, the inputs that fill it, a way to summarize what comes back, a declaration
of what it needs, and a name. That is not a program; it is a **recipe**, and a recipe can
be data.

## Decision (proposed)

### A self-authored capability is a recipe, not a program

A declarative description: id, description, input schema, a GET URL template over its
inputs, and what to say about the response. No loops, no branches, no state, no code. The
expressiveness ceiling **is** the sandbox — there is no escape from a format that cannot
express escaping — and the interpreter is machinery Endora already trusts, because it is
the same egress helper every built-in skill uses.

### Born with every restriction the system has

A recipe capability enters the registry as: `reversibility: Observe` (it cannot act, by
construction — the format has no verb), `third_party: true` (its output taints the turn,
[0064](0064-what-a-stranger-said.md)), stance `off` until the person turns it on
([0062](0062-one-permission-surface.md)), and its requests through the standard egress
helpers — which means the caching ([0061](0061-answers-worth-keeping.md)), the outbound
secret scan, and `wants_place` all apply with no new wiring.

### The model proposes; the person enables; the record proves

The butler drafts a recipe when the record shows a repeated gap — the same
arithmetic-over-records shape as notions ([0057](0057-thinking-between-turns.md)), never a
spontaneous urge. The draft surfaces in the console as a proposal with its full recipe
visible: what it would call, with what inputs, saying what. Nothing runs until the person
enables it. After that it is an ordinary tool: proven by read-back where applicable,
withdrawn by their verdict ([0066](0066-their-verdict-decides-too.md)), ranked by how it
lands ([0060](0060-what-the-turn-is-offered.md)).

### What this retires: nothing, and here is the honest accounting

The retirement rule asks every mechanism-adding ADR to name what it supersedes. This one
supersedes no mechanism — it is a new *source* of capabilities, not a second copy of an
existing responsibility. Its compliance with the rule's spirit is structural: recipes run
through the existing registry, the existing stance ladder, the existing egress door and
the existing graduation arithmetic. **Zero parallel machinery.** The day a recipe needs
something the shared path lacks, that need goes into the shared path or the recipe goes
unbuilt.

## Consequences

- **The failure ceiling is a wrong or wasteful GET** to a URL the person saw before
  enabling — tainted on return, cached, secret-scanned, and unable to act. An attacker
  who fully controls a recipe's target controls exactly what any web page already
  controls: words.
- **Real gaps stay closed only where a GET can close them.** Many can — transit,
  air quality, a niche feed. Anything needing auth flows, POSTs, or logic stays human
  work, and the recipe format must not grow toward code one convenience at a time; that
  growth is the failure mode, and each extension needs its own record.
- **The proposal queue is new UI surface** — the one place this touches the console, and
  the reason this ADR is design-first: 0029 deleted an approval queue once, and the
  distinction (that queue held *the model's guesses about the person*; this one holds
  *artifacts the person inspects*) deserves scrutiny before code.

## Rejected

- **A code sandbox** (WASM, Lua, containers). A dependency, an attack surface, and an
  invitation for the format to become a language. The recipe's ceiling does more safety
  work than any runtime fence, and needs none.
- **Butler-managed MCP servers.** Real, and later — that is the infra-skills rung
  ([0055](0055-the-model-layer.md)), which starts with managing *existing* services, not
  authoring new ones.
- **Auto-enabling a proven-looking recipe.** Enabling is the person's; the record can
  only graduate what the person first let run. Same line 0062 drew: read-back grants,
  people consent.
- **Building it now** *(resolved 2026-08-08 — the three designs follow)*. This record
  stayed Proposed until the recipe format, the proposal surface and the gap-detection
  arithmetic had each been designed against real examples from the live record — the
  standard 0061 set: accepted on the record, not the intent.

## The three designs (2026-08-08)

### Gap detection: a specimen that gave up is a proven gap

The arithmetic this record was waiting for shipped as
[0075](0075-failures-become-specimens.md). A specimen that retires **unresolved** —
fourteen nightly replays, every one rejected by the deterministic verdicts — is the
live record saying, with no model judgment anywhere in the chain: *this house asks
this, and the machinery cannot answer it, persistently.* That is the only trigger.
The butler drafts a recipe proposal **only** from an unresolved specimen whose ask
is recipe-shaped (a fact a GET could fetch), at most one draft per specimen, filed
with the specimen quoted as its evidence. No counter is added, no threshold
invented: the shelf's own retirement rule is the threshold, already bounded and
already tested. A gap the specimen loop never proves does not get a draft — and
this week's real gap (the drive) is the honest negative example: it needed the
house's own sensors, not a GET, and a recipe could not have closed it. Real gaps
stay closed only where a GET can close them, exactly as the consequences above
said.

### The recipe format, worked against a real service

One worked example, chosen because it is real, keyless, and the shape of half the
asks a house makes — air quality from the same provider the weather skill already
trusts:

```json
{
  "id": "air_quality",
  "description": "Today's air quality where you are — good, fair, or poor.",
  "inputs": { "lat": "number", "lon": "number" },
  "get": "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&current=us_aqi",
  "say": "The air quality index is {current.us_aqi} right now."
}
```

Five fields, and the ceiling is visible in them: `get` is a template whose `{...}`
slots are filled **only** from declared inputs, each value URL-encoded by the
interpreter (a format that cannot express escaping cannot be escaped from);
`say` is a template whose slots are JSON paths into the response, stringified —
no expressions, no conditionals, no second request. Anything the format cannot
say, a recipe cannot do; that sentence is the sandbox, and every extension to the
format needs its own record.

### Authoring is a loop, not a form (added 2026-08-15)

The five fields are the format; they are not, on their own, a way to author
anything. Hand-writing a JSON path against an API you cannot see is guesswork,
and guesswork is what made the first version of the form unusable in practice.

So authoring runs the loop the format already implies: **fetch it once, see the
real answer, tap the field you want, read the sentence back.** `try_recipe` runs
a draft through the *same* validation, the *same* guarded fetch and the *same*
templating a saved recipe uses — a trial that took a shortcut would be a demo
rather than evidence — and reports the address it really fetched, every path a
`say` template could address (with the value found there), and the rendered
sentence. Nothing is stored and nothing is enabled by trying.

Two things the loop is careful about. It offers **only** paths that resolve:
`say` walks object keys, so a field inside a list has no expressible path, and
offering one would hand the person a template that always fails — those are
skipped and *said to be skipped*, because "my field isn't there" otherwise reads
as a bug rather than the format's edge. And it is the same evidence a
butler-drafted proposal will show, so the two paths converge rather than
growing separate explanations of the same recipe.

### The proposal surface: the trouble card's shape, not the deleted queue's

A proposal renders exactly like a standing-trouble card — one card, the full
recipe visible (the URL it would call, the inputs, the sentence it would say),
the specimen it answers quoted beneath, and two honest buttons: **enable** (sets
the stance, 0062's one permission surface — nothing else) and **not this**
(records the refusal; the specimen never drafts again). What distinguishes this
from the queue 0029 deleted, stated so the distinction is checkable: that queue
held the model's *guesses about the person*, which only the person could judge
and therefore nagged; this card holds an *artifact* — a URL and two templates —
that the person inspects the way they inspect any setting. No count badge, no
backlog: at most one open proposal at a time, because the trigger (an unresolved
specimen) arrives at most once every two weeks by construction.
