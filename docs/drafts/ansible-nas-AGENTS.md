# Draft: the fork's root AGENTS.md

> **A draft, not applied.** This is the text the engineering wing proposes to the owner's
> ansible-nas fork as its first branch (an `hq/task-N-agents-md` branch, reviewed and checked
> like any other, pushed and merged only by the owner). Endora keeps the draft in its library
> so HQ's members can read it with `read_doc docs/drafts/ansible-nas-AGENTS.md`. Everything
> below the line is the proposed file.

---

# Working agreements

This is the owner's fork of ansible-nas, which builds the home lab. It builds on the owner's
global standards, kept in Endora:
<https://github.com/RusticPotatos/Endora/blob/develop/docs/standards.md> (test first,
behaviour in the owner's words, abstract only when needed, evidence over claims, nothing
personal in tracked files, and the rest). What follows is this repository's own.

## The repository

- **Base branch: `develop`.** Branch from it and propose back to it. `main` mirrors upstream
  and is never a base.
- **Keep upstream's conventions**, so changes stay easy to compare with upstream:
  - a new application starts from the `hello_world` template role, with everything it has
    (defaults, tasks, docs, molecule);
  - every variable of a role is named `<app>_*` (`<app>_enabled`, `<app>_data_directory`,
    `<app>_port`, ...);
  - one documentation page per application under `website/docs/applications/`;
  - a molecule scenario per role, whose `verify` shows the application starts and stops.
- **Lint is pinned** as the fork's CI pins it: ansible-lint 6.14.6 and yamllint 1.30.0. Before
  submitting, the branch passes `ansible-playbook --syntax-check nas.yml -i
  tests/inventories/integration_testing`, `ansible-lint nas.yml` and `yamllint .`.

## The owner's values

**Never edit, cite or copy from `group_vars/`, `host_vars/` or any inventory, or anything
named for a vault.** They hold the owner's own settings. A branch that touches them is refused
whole. Use a role's `defaults/main.yml` and fictional examples instead.

## What may change without asking

- **May change without asking:** a role's settings and defaults, documentation, the clean-up
  of a retired application, and lint fixes.
- **Needs a proposal first:** the network, the firewall, backups, credentials, storage, and
  anything that deletes data. Propose it to the wing's lead with the evidence (what you read,
  by receipt), and the owner gets a desk item; no branch is cut until the owner has said yes.

## Branches and commits

- **One branch per task**, named `hq/task-<N>-<slug>`, cut from `develop`, kept under about
  **200 changed lines**. A larger change is split into tasks.
- **Conventional Commits** (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`),
  authored `Endora HQ`, each carrying the member trailer
  (`Made-by: Endora HQ (<members>) for task #N`).
- HQ never pushes on its own, never opens a pull request and never merges: the owner pushes an
  accepted branch and merges it on GitHub.

## A branch's description

Every branch's description says, in this order:

1. **What changed:** the files and roles, in a line or two each.
2. **Why:** the task, and what was found that called for it.
3. **What was run, with receipts:** each check and its result (syntax, ansible-lint,
   yamllint, molecule where it ran), cited by receipt.
4. **How to roll back:** what to revert, and anything to redo on the lab after reverting.
