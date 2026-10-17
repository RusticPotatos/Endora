# 0105 — Models in ordered lists, cleared by data tier

## Status

Accepted, 2026-10-16, from an owner design interview (Q1–Q6, Q3a–Q3e). Phases 1 and 2 are
built: providers, the ordered lists and their walking, every everyday cloud entry through the
door; then the tiers, the 🔒 and the door's refusal of Restricted words. Sign in with ChatGPT as
a provider is not yet.

How phase 2 is built, for the record: Restricted words carry a mark (`[private] `) the door
refuses, as a stranger's words carry theirs (0064), derived from what the turn holds rather
than stored beside it. A 🔒 message is kept with the mark, and so is Endora's reply to it; a
Restricted skill's result is marked as it enters the turn, by one table of skill tiers in the
capabilities context (a skill not listed is Restricted); an owner's message shaped like an
account number is refused by shape. Confidential context, skill results and earlier replies
leave disguised. A private message is never noted into the brain or folded into the running
summary, so it cannot ride out later through either. Prepared with AI
assistance (Claude). Step 4 of [the always-on butler plan](../plans/always-on-butler.md). It
replaces the everyday model's three mechanisms (the preferred rung of
[0072](0072-the-base-has-rungs-too.md), the base config and its router-and-writer mixture of
[0055](0055-the-model-layer.md), and the environment fallback) and the single deep model, with
one shape. It keeps [0069](0069-one-door-off-the-device.md)'s one door, and extends it.

## Context

The plan puts Claude, through the owner's own API key, in front of the other providers, with
Sign in with ChatGPT and DeepSeek behind it. Endora could not express that. The everyday model
was a preferred rung, a base model (optionally a router and a writer), and a fallback built
from the environment, each with its own rules; the deep model was one slot. A provider used for
both meant its key typed twice. And "may this model see this?" was answered by which slot a
model sat in, not by what the request held, so a cloud model in the everyday slot saw whatever
a local one would.

## Decision

**Providers once, ordered lists per job.**

- A **provider** is an endpoint and its key: the house's Ollama, the Mac's bigger model,
  Anthropic, DeepSeek, later the ChatGPT sign-in. Each key is entered once, stored in the node,
  and never returned to a browser.
- **Everyday** and **Deep** are each an **ordered list** of entries, an entry being a provider
  and a model. The same provider may appear in both. Entries are sorted with ↑ ↓ and can be
  switched off without being deleted.
- The router-and-writer mixture goes: an entry is one model.
- Deep keeps its one switch, "Endora may use this on its own", for the whole list.

**The first entry that works, and is cleared for the request, answers.**

- Endora walks the list in order. An entry that fails (unreachable, an error, out of credit) is
  skipped for that request and for the next five minutes, so a sleeping machine costs one
  timeout, not one per message.
- A reply that has begun to stream is never finished by another model.

**Data has a tier; a model has a clearance; code compares them.**

| Tier | Examples | Local model | Cloud model |
| --- | --- | --- | --- |
| Public | web results, news, weather | yes | yes, as is |
| Internal | container and service status, Endora's own settings | yes | yes, as is |
| Confidential | the owner's chat messages (by default), calendar, brain pages and memory, people's names, where they are | yes | only disguised |
| Restricted | what the owner marks 🔒, account numbers, health and money details | yes | never |

- **Tier comes from the source, set by code, never by a model's judgement.** A request's tier
  is the highest tier of anything in it.
- **The owner's own messages are Confidential at least.** A 🔒 on the composer makes a message
  Restricted, and with it the turn it starts.
- **A cloud model sees Confidential only disguised**: the existing pseudonym layer of
  [0051](0051-where-the-boundary-is.md) swaps names, places and appointment titles for
  placeholders and puts them back on the way in. **Restricted never leaves the house, disguised
  or not.**
- **A local model is cleared for every tier.**
- **Secrets are not a tier.** Keys, passwords and raw mail bodies reach no model's context, as
  now; the quarantined mail reader of [0097](0097-it-reads-the-mail.md) stays the one thing that
  reads a body, and it returns only a checked summary.
- **Entries a request outranks are skipped, never upgraded.** When nothing cleared answers (a
  Restricted turn and the local model down), Endora says so and offers to retry, or to ask a
  cloud model without the parts it may not see. It never sends them quietly.

**Every cloud entry goes through the one door.** [0069](0069-one-door-off-the-device.md)'s
`Deeper` is the only way off the device, for Everyday's cloud entries as much as Deep's: the
disguise, the outbound secret scan, the stranger's-words refusal of
[0064](0064-what-a-stranger-said.md), and now the tier check all run inside it.

## Consequences

- Everyday can be "Claude first, then the local model" safely: an ordinary message goes to
  Claude disguised; a 🔒 message, or a turn touching Restricted data, is answered locally.
- The AI screen becomes a list of providers and two sorted lists, instead of a form per slot.
- Existing settings move over on the first start, with no key typed again: Everyday becomes
  the preferred rung (if set) then the base model; Deep becomes the deep model; each endpoint
  and key becomes a provider.
- Every context section the turn assembles must carry its tier. That is the bulk of the work,
  and a section that does not say is treated as Restricted, so a forgotten label fails safe.
- The single deep fallback built first on `feat/deep-fallback` is superseded; its
  "first, then second" client becomes the list's walker.
- Sign in with ChatGPT becomes one more provider when it is built, through OpenAI's documented
  route for self-hosted apps (dynamic client registration, the Responses API), never another
  tool's client id.

## Alternatives considered

- **Judging each model, not the data** ("this slot is local, that one is cloud"). Rejected: it
  is the reason a cloud everyday model saw everything, and it cannot express "ordinary chat to
  Claude, private things locally".
- **A model classifying the data.** Rejected: the model is never the enforcement boundary.
- **Disguising Restricted too.** Rejected by the owner: what is Restricted must not leave in
  any form; disguise is good, not perfect.
- **Keeping the mixture.** Rejected: unused, and it doubles what every entry must say.
- **Dragging to sort.** Rejected for ↑ ↓: dragging is unreliable on a phone.
