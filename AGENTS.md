# Working agreements for AI assistants

This is the **canonical** instructions file for AI coding assistants working in
the Endora repository, following the cross-tool `AGENTS.md` convention.
`CLAUDE.md` is a symlink to this file, so Claude Code reads the same content —
edit this file, never a copy. For the full picture, read [README.md](README.md),
[docs/constitution.md](docs/constitution.md),
[docs/architecture.md](docs/architecture.md), and
[CONTRIBUTING.md](CONTRIBUTING.md).

This file builds on the owner's global standards, [docs/standards.md](docs/standards.md):
the base principles every project shares. What follows is Endora's own.

## Before you touch anything

- **Verify the repository root** is this Endora repo before editing
  (`git rev-parse --show-toplevel`). **Never modify unrelated repositories or the
  user's other work.**
- **Inspect existing work first.** Do not assume the repo is empty; preserve valid
  existing files, branches, and history.
- Branch from `develop` (see the branching model in the README). Do not force-push
  or rewrite published history.
- **Put new things where [docs/where-things-go.md](docs/where-things-go.md) says.**
  Before adding a screen, card, setting, menu entry, file or crate, find its place on
  that map. If nothing fits, add the place to the map in the same change; never
  invent a spot.

## Architecture rules

- Endora follows **Responsibility-Oriented Clean Architecture (ROCA)** — a
  domain-first modular monolith organized **by responsibility**, not by layer
  (ADR 0050). The workspace is `app/` (composition roots), `domains/` (one crate
  per bounded context: `platform`, `capabilities`, `team`, `sound`, and `mail`, kept as a
  library for a later HQ contract), `shared/` (`kernel`, `persistence`, `protocol`), and
  `clients/` (the handheld terminal, ADR 0099, which talks to the node only over HTTP and
  has its own `AGENTS.md`).
- **Each context owns its own `domain` / `application` / `infrastructure`**
  (and, over HTTP, its interface), with dependencies pointing inward. A context's
  `domain` module imports only `shared/kernel`; keep it pure (no HTTP, database,
  AI vendor, UI, OS, or model-specific code). Cross-context calls go
  application→application/ports only — the dependency graph stays acyclic.
- `shared/` is for genuinely cross-cutting code only; the thin **orchestration
  layer** (`endora-application`) holds the use cases that compose several contexts (the
  activity feed, the owner's export and delete). Endora is HQ: the butler's chat was retired
  (ADR 0136).
- **No premature microservices**, and no speculative abstractions. Build what the
  current vertical slice needs; prefer a complete slice over broad shallow
  scaffolding.
- **Models propose; deterministic policy authorizes.** Never route a consequential
  action directly from model output. The language model is never the enforcement
  boundary.
- **Preserve public API / protocol compatibility** within a major version.
- **Update documentation and ADRs when architecture changes.** Architectural
  decisions require an [ADR](docs/adr/README.md).

## How to work

- **Test behavior before implementing it** (TDD). Write failing tests that define
  the behavior, then make them pass.
- Prefer **early returns** over `else`; keep the **happy path at the root level**
  of functions. Match surrounding style.
- **No new dependency without justification** (especially in the domain).
- Use **Conventional Commits** (`feat:`, `fix:`, `test:`, `refactor:`, `docs:`,
  `chore:`). Prefer a few coherent commits over many tiny ones.
- **Edit files with tools that fail loudly.** Scripted string replacement (a `sed`
  or Python one-liner over a source file) does *nothing* when its anchor does not
  match, and reports success. Every instance from one session: a function inserted
  into the wrong file; doc comments displaced onto the wrong items three times; a
  Rust line-continuation mangled into a literal `\n`; and a second replacement
  deleting the function the first had just added — which shipped a blank console
  to a phone. Prefer an edit that refuses when the anchor is missing or ambiguous.
  Where only a script is available, assert the anchor exists **and** re-read the
  result before moving on.
- **Nothing personal goes in a file git carries.** No real name, address, city,
  postcode, network address, coordinate, phone number or credential — in source,
  in a doc comment, and above all in a test fixture, which is where every one of
  these has actually got in. Use `John Doe`, `Jane Doe`, `New York`,
  `192.168.1.10`, `@example.com`. Real values belong in the git-ignored
  `local.mk`, and `ENDORA_PERSONAL_VALUES` there lets `make personal-check` prove
  they are not in the tree. This is not a caution: a city, a county, a postcode
  and three people's names — one of them a third party who never agreed to be in
  a public repository — reached this repo through fixtures, and every one was
  caught by a person reading a diff rather than by anything that runs.
- **Think the design through before running it.** A checker that reported
  all-clear on a broken file, and a duplicate-detector compared on exact strings
  against model output, both cost far more to discover than they would have to
  reason about. When something is going to be verified live, spend the minute
  first.

## Use time and model usage carefully

- **Work one bounded problem at a time.** State the acceptance criteria, use the
  existing checkpoint and inspect only the relevant code and evidence. Recheck
  repository state; reopen wider research only when new evidence requires it.
- **Match reasoning to the task.** The owner's Codex default is GPT-6.1 Sol at
  Medium when available. Routine edits can use lighter settings; escalate for
  unresolved complexity, then return to the default. Respect the selected model;
  do not silently change models, providers or account settings. Other assistants
  should use their equivalent balanced setting rather than assume these names.
- **Test narrowly while iterating.** Reproduce the behavior with a focused failing
  test, fix it, then run the required handoff gate once for the final implementation.
  Repeat checks when changes, failures or new evidence invalidate their result.
- **Prove the experiment before spending a live run.** Inspect the actual rendered
  briefing, wake purpose, advertised tools and guard decisions together. Replay
  captured valid calls offline and show that invalid calls fail. Freeze scope and
  limits; preserve failed results rather than retrying until a model succeeds.
- **Keep communication and checkpoints compact.** Report outcomes, material
  uncertainty and the next action. Reuse the existing research and checkpoint
  documents; update changed facts rather than creating repeated summaries. At a
  stopping point, record the commit, checks, blocker and exact next step.
- **Stop unproductive loops.** Follow the shared three-attempt limit: report the
  evidence and change the approach offline before proposing another live attempt.
  Cost savings never waive privacy, authorization, TDD or release requirements.

## Before handoff

Run from the workspace root and make sure they pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node scripts/check-console.mjs
node scripts/check-nothing-personal.mjs
sh scripts/check-collection-parity.sh
git diff --check
```

For prose-only follow-ups, an observed passing gate for the unchanged executable
code, dependencies and configuration may be reused. Run fresh whitespace,
personal-value and relevant document/link checks, and name the earlier gate and
what was reused. Otherwise run the full gate above; never infer a pass or reuse
results after a behavioral change.

When the handheld terminal changed, also, from `clients/terminal`:

```bash
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/check-private.py
python3 scripts/check-pty.py ../../target/debug/endora-terminal
```

- **Do not create pull requests unless explicitly requested.**
- **Disclose AI assistance.** Any change that is AI-assisted must be clearly
  flagged as such in the PR description (see
  [CONTRIBUTING.md](CONTRIBUTING.md#ai-assisted-contributions)).
- Report outcomes honestly: if a command failed or a step was skipped, say so.
  Never claim a command succeeded unless you ran it and saw the result.
