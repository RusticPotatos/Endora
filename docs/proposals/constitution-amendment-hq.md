# PROPOSED — Amendment to the Constitution's preamble and §9

> **Status: PROPOSED 2026-10-20.** Not in force. Constitution §11: *"A model may draft a
> proposal; only maintainers, acting deliberately, may adopt one."* This is that draft, written
> with AI assistance after [ADR 0136](../adr/0136-endora-is-hq.md) retired the butler's chat.
> Nothing in [the constitution](../constitution.md) has been changed.

## Why an amendment is needed

Two parts of the constitution describe the product that was retired:

- **The preamble** calls Endora *"a local-first, open-source personal intelligence platform"*
  whose purpose is *"to help a person live intentionally through reflection, experimentation,
  stewardship, and continuous improvement"*. Endora is now HQ: a team of agents that does the
  household's technical work, with the owner deciding.
- **§9** describes Endora forming *"its own model of the person"* through *Observe → Understand
  (beliefs…)*. Nothing forms one now: the brain and the beliefs before it are gone.

Every other section holds as written, and HQ already follows them: least authority (§2), models
propose and policy authorizes (§3), memory rights (§6) for the node's record, autonomy levels
(§8) through the desk and each tool's stance, auditability (§10).

## The options

### A. Amend the preamble and §9 (recommended)

Preamble, replacing its first paragraph:

> Endora is a local-first, open-source platform where a small team of AI agents does a
> household's technical work — its home lab first — under the owner's direction. Its purpose is
> to take work off a person's hands without taking control of their house or their life away from
> them. This constitution defines the limits within which every part of Endora — code, models,
> policies, and clients — must operate.

§9, replacing its loop and first bullet:

> Endora improves through an explicit, inspectable loop:
>
> ```text
> Work → Evidence (receipts, tests, a reviewer who did not do the work)
>      → Outcome → Lesson (in the team's notes, with what shows it)
>      → Proposal → the owner's decision
> ```
>
> - The team records what it learns about its own work, visibly and correctably; a lesson
>   without evidence is not kept as fact. Endora keeps no model of the person beyond what the
>   owner writes down for it.

The second and third bullets of §9 stand unchanged. The North Star stays.

### B. Leave the constitution as it is

It still constrains the code correctly where it matters (§2, §3, §6, §8, §10, §11); the
preamble and §9 would simply describe more than Endora now does. Cheapest, but a reader new to
the project is told it is something it is not.

### C. Rewrite it whole for HQ

Not proposed: the principles hold, and only their framing changed. A rewrite would invite
changes nobody has argued for.

## What adopting it changes in code

Nothing. HQ already works this way (ADRs 0102, 0114, 0132). Adopting A is a text change to
[docs/constitution.md](../constitution.md) and a dated status line here.
