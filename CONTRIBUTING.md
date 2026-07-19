# Contributing to Endora

Thank you for your interest in Endora. This document explains how to contribute
during the project's **foundation stage** and the standards contributions are
held to. By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Where we are

Endora is in its foundation phase: we are establishing principles, architecture,
and a minimal skeleton — not building product features yet. The most valuable
contributions right now are:

- thoughtful discussion and refinement of the [Constitution](docs/constitution.md)
  and [architecture](docs/architecture.md),
- small, focused improvements to documentation and the workspace skeleton,
- clarifying issues and questions.

## Before you start

- **Open an issue before any large or architectural change.** Align on direction
  first; unsolicited large pull requests are hard to accept.
- **Verify your working copy** is the Endora repository root before editing, and
  inspect existing work before changing it. Never modify unrelated repositories.

## Branching and workflow

```text
main      → stable public branch (protected)
develop   → integration branch
```

1. Branch from **`develop`**, not `main`. Use a descriptive prefix, e.g.
   `feat/…`, `fix/…`, `docs/…`.
2. Keep changes focused. Prefer a **complete vertical slice** over a broad but
   shallow change.
3. **Do not open a pull request unless it has been explicitly requested / agreed**
   (e.g. in the tracking issue).

## Engineering standards

### Test-first for behavior

We prefer **test-driven development**. Write tests that define new behavior
*before* implementing it. Every behavioral change should come with tests.

### Architecture rules (enforced in review)

- **Domain-first.** Respect the layering Domain → Application → Infrastructure →
  Interface, with dependencies pointing inward.
- **No infrastructure or UI concerns in the Domain layer** — no HTTP, database,
  AI vendor, UI framework, OS, or model-specific code there.
- **No model-specific logic in the domain.** Models propose; deterministic policy
  authorizes.
- **Architecture changes require an [ADR](docs/adr/README.md).**
- **Preserve public API / protocol compatibility** within a major version.
- **No new dependency without justification.** Adding a dependency (especially in
  `endora-domain`) needs a clear rationale and, for load-bearing choices, an ADR.
- **Avoid speculative abstractions.** Build what the current slice needs.

### Code style prefs

- Prefer **early returns** over `else`; keep the **happy path at the root level**
  of functions.
- Match the surrounding code's naming, idiom, and comment density.

### Before you push

Run, from the workspace root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

CI runs the same checks on Linux and macOS.

## Commit messages

Use **Conventional Commit** prefixes:

- `feat:` — a new capability or behavior
- `fix:` — a bug fix
- `test:` — adding or changing tests
- `refactor:` — behavior-preserving change
- `docs:` — documentation only
- `chore:` — tooling, CI, housekeeping

Prefer a small number of coherent commits over many tiny ones, and do not combine
unrelated changes into one vague commit.

## AI-assisted contributions

Endora is built in the open and we welcome AI-assisted work — but it must be
**clearly disclosed**. If a pull request contains AI-assisted changes:

- Say so explicitly in the PR description (which tool/assistant, and roughly what
  it helped with).
- You remain responsible for the change: understand it, test it, and ensure it
  meets every standard above.
- The same review bar applies; AI assistance is never a reason to relax scrutiny,
  especially around the policy/safety boundary.

## Discussion expectations

Keep discussion public where practical, and respectful always. Assume good faith,
critique ideas rather than people, and prefer written, reviewable decisions
(issues, ADRs) over informal ones.

## License of contributions

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), consistent with the rest of the project.
