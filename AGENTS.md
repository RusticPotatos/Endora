# Working agreements for AI assistants

This is the **canonical** instructions file for AI coding assistants working in
the Endora repository, following the cross-tool `AGENTS.md` convention.
`CLAUDE.md` is a symlink to this file, so Claude Code reads the same content —
edit this file, never a copy. For the full picture, read [README.md](README.md),
[docs/constitution.md](docs/constitution.md),
[docs/architecture.md](docs/architecture.md), and
[CONTRIBUTING.md](CONTRIBUTING.md).

## Before you touch anything

- **Verify the repository root** is this Endora repo before editing
  (`git rev-parse --show-toplevel`). **Never modify unrelated repositories or the
  user's other work.**
- **Inspect existing work first.** Do not assume the repo is empty; preserve valid
  existing files, branches, and history.
- Branch from `develop` (see the branching model in the README). Do not force-push
  or rewrite published history.

## Architecture rules

- Endora follows **Responsibility-Oriented Clean Architecture (ROCA)** — a
  domain-first modular monolith organized **by responsibility**, not by layer
  (ADR 0050). The workspace is `app/` (composition roots), `domains/` (one crate
  per bounded context: `platform`, `capabilities`, `understanding`,
  `conversation`, `scheduling`), and `shared/` (`kernel`, `persistence`).
- **Each context owns its own `domain` / `application` / `infrastructure`**
  (and, over HTTP, its interface), with dependencies pointing inward. A context's
  `domain` module imports only `shared/kernel`; keep it pure (no HTTP, database,
  AI vendor, UI, OS, or model-specific code). Cross-context calls go
  application→application/ports only — the dependency graph stays acyclic.
- `shared/` is for genuinely cross-cutting code only; the thin **orchestration
  layer** (currently `endora-application`) holds the butler-turn contract and the
  use cases that compose several contexts.
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
- **Think the design through before running it.** A checker that reported
  all-clear on a broken file, and a duplicate-detector compared on exact strings
  against model output, both cost far more to discover than they would have to
  reason about. When something is going to be verified live, spend the minute
  first.

## Before handoff

Run from the workspace root and make sure they pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

- **Do not create pull requests unless explicitly requested.**
- **Disclose AI assistance.** Any change that is AI-assisted must be clearly
  flagged as such in the PR description (see
  [CONTRIBUTING.md](CONTRIBUTING.md#ai-assisted-contributions)).
- Report outcomes honestly: if a command failed or a step was skipped, say so.
  Never claim a command succeeded unless you ran it and saw the result.
