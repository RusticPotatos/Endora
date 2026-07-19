# Working agreements for AI assistants (agents)

This file gives AI coding assistants (Codex and others) the core rules for
working in the Endora repository. `CLAUDE.md` carries the same agreements for
Claude. Keep both in sync. For the full picture, read [README.md](README.md),
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

- Endora is a **domain-first modular monolith**. Respect the layering
  Domain → Application → Infrastructure → Interface, with dependencies pointing
  inward.
- **No UI or infrastructure concerns in the Domain layer** — no HTTP, database,
  AI vendor, UI framework, OS, or model-specific code there. `endora-domain` has
  zero dependencies; keep it that way.
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
