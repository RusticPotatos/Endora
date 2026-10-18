# 0116 — The team's standards

## Status

Accepted, 2026-10-18 (proposed 2026-10-17 at the owner's request). Prepared with AI
assistance (Claude). Amends [0111](0111-the-ceo-and-the-chain-of-command.md) (`read_doc` is
every member's, not the CEO's alone, and its library gains the standards and the drafts) and
[0115](0115-the-engineering-wing-and-the-git-flow.md) (the wing is told the fork's own
`AGENTS.md` is the repository's rules, and its lead that the draft is the first branch to
propose). Built: `docs/standards.md`, the pointers in the root and the terminal's `AGENTS.md`,
the library, the briefings, and the fork's draft.

## Context

The owner asked for one set of global standards: the base principles used every day (test
first, behaviour in the owner's words, SOLID, abstract only when needed, domain first, small
slices), owned by the team over time, describing each project at a high level, with a root
`AGENTS.md` in each project and nested ones only where an architectural boundary needs
guarding. Endora's rules lived in `AGENTS.md`, `CONTRIBUTING.md` and the constitution; HQ's
members, who now change the owner's ansible-nas fork, could read none of them: `read_doc` was
the CEO's alone, and the fork has no `AGENTS.md`.

## Decision

1. **`docs/standards.md` is the global base**, in Endora's repository, which is canonical. The
   owner approves every change; the team proposes changes as branches with evidence. Each
   repository's root `AGENTS.md` builds on it and links to it without repeating it; a nested
   `AGENTS.md` only guards a boundary, never contradicts its parent, and wins on its own subject
   only.
2. **Every member reads the library.** `read_doc` is offered to every role. The library HQ
   embeds at build time is `docs/adr/*.md`, `docs/plans/*.md`, `docs/drafts/*.md` (top level
   only) and `docs/standards.md`; any other path is refused as before. They are public on
   GitHub already; nothing is read from the disk at run time. The house log stays the CEO's.
3. **Every briefing names the standards once**: "The team's standards: read_doc
   docs/standards.md", after who the member is. The CEO's quiet briefing is held to under half
   its whole one, so the CEO's `read_doc` description names the standards instead.
4. **The wing**: a lead or developer in the wing is told "The fork's own AGENTS.md, when it
   exists, is the repo's rules; propose one if missing." Its lead is told too that, until it
   exists, it is the first branch to propose, from the draft `docs/drafts/ansible-nas-AGENTS.md`.
   The seeded goal note and first task are left as they are: the live HQ is seeded already, and
   the briefing reaches it.

## Consequences

- One text of the standards; every project and every member reads the same one.
- Members' briefings grow by one line (two in the wing); every member's tool list by one.
- The fork's `AGENTS.md` reaches it the way every change does: an `hq/` branch, checked,
  reviewed, and pushed and merged by the owner.

## Alternatives considered

- **The standards in every briefing, whole.** Rejected: a briefing is paid for at every round.
- **A copy in each repository.** Rejected: copies drift; a link keeps one text.
- **`read_doc` still the CEO's, the standards handed down in threads.** Rejected: a member
  cannot be held to rules it cannot read.
