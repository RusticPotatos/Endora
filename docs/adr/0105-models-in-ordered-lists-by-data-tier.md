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
account number is refused by shape (an ISO date before a time is not one). Confidential
context, skill results and earlier replies leave disguised: every field of the context is
named at the door, so the running summary, the names things go by, the track record, the
preferences and the skill inputs a model chose are disguised too, and what comes back
(beliefs included) is restored. Where they are based never leaves, and is disguised where it
appears in words. A private message is never noted into the brain or folded into the running
summary, so it cannot ride out later through either.

The marks outlive the turn. A skill's result is stored with its mark, and an earlier reply is
given back with the marks of everything it found, so the door refuses a later turn that
carries them, and a stranger's words carried that way also stop the turn acting on its own.
The door looks for the private mark anywhere in a message. A reply to a 🔒 message, or to a
turn that read a Restricted result, is stored with the mark. A 🔒 standing answer (the brief,
"are any lights on?") is kept marked, and a private brief posts its facts as themselves rather
than being worded by the deep model; the brief's wording refuses marked or account-shaped facts
in any case, and the place they are based is disguised in it.

What Endora writes is judged the way the owner's messages are: a reply, a posted message or a
running summary that carries what looks like an account number is kept with the private mark,
so the turns that later read it stay in the house, and a summary once marked stays marked. The
learning pass and the running summary skip an owner's account-shaped message as they skip a
private one, and the door refuses a turn whose running summary is marked.

A standing answer is marked for what is in it, by the same rule as a turn's skill result. The
brief is Confidential (in the tier table): its reply is not private, and what it carries from
outside keeps the stranger's mark, so the turn after it stays in the house. Prepared with AI
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
  timeout, not one per message. An entry's client reports a failure as one, rather than
  apologising, so the walk can ask the next; the environment's model, last in Everyday, still
  apologises when nothing answers, so the conversation never breaks. An entry is cooled by its
  endpoint, model and key, so a second key for the same model is still tried. A machine on the
  house's network that refuses the keyless probe for want of a key is awake.
- A reply that has begun to stream is never finished by another model.
- An answer from an everyday cloud entry is the everyday answer, not an escalation: only the
  deep list, asked because the everyday answer failed a check, is disclosed as one.
- Every reply says which model wrote it and whether it runs on the house's network, so the
  note on a turn the deep model planned names where the skills' results were read: "read by
  qwen2.5:7b on your network", or "read by claude-sonnet-5-5, disguised" when a cloud model is
  first in Everyday.
- A provider's key belongs to its endpoint: a provider moved to another endpoint without a new
  key loses the old one. "Forget everything" forgets the old settings too, so nothing moves
  over again. When the lists cannot be read, the environment's model answers, never the old
  settings.
- The lists are the only source. The old everyday settings and single deep model are read
  once, to move them over on a start with no lists, and never again as a fallback: not for
  the deep model's status, its balance, or the AI screen. Only Deep's "on its own" switch is
  still kept in the old deep row.
- "On the house's network" is decided from the URL's host, after any user-info: loopback,
  link-local, RFC 1918 and IPv6 unique-local addresses, `.local`, `.lan` and single-label names.
  Every other entry is a cloud entry, reached only through the door.

**Data has a tier; a model has a clearance; code compares them.**

| Tier | Examples | Local model | Cloud model |
| --- | --- | --- | --- |
| Public | web results, news, weather | yes | yes, as is |
| Internal | container and service status, Endora's own settings | yes | yes, as is |
| Confidential | the owner's chat messages (by default), calendar, brain pages and memory, people's names | yes | only disguised |
| Restricted | what the owner marks 🔒, account numbers, health and money details, where they are and where they have been | yes | never |

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
