# 0071 — Capabilities it writes itself

## Status

Proposed (2026-08-04). Design only — **no code ships under this record until it is
Accepted.** The last rung of the master plan, and the first step onto the capability
ladder's "infra skills" rung ([0055](0055-the-model-layer.md)).

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
- **Building it now.** This record stays Proposed until the recipe format, the proposal
  surface and the gap-detection arithmetic have each been designed against real examples
  from the live record — the standard 0061 set: accepted on the record, not the intent.
