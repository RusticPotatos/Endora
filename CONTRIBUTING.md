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

### The test tiers, and what each one is for

Fast on purpose — the whole offline suite is **~10s for 460+ tests**, and it has to stay
that way to be run constantly. Tiers are chosen to match how bugs have actually reached
production here, not to fill in a pyramid.

| tier | what it covers | where | cost |
| --- | --- | --- | --- |
| **unit** | pure logic, one thing at a time | `#[cfg(test)]` beside the code | ~10s |
| **composition** | the stack **production builds**, not test doubles | same, but composing real runners and the real schema | ~0s |
| **golden** | real captured data from a live system | `tests/` + `fixtures/` | ~0s |
| **console render** | every screen of the web console, in Node | `scripts/check-console.mjs` | ~0s |
| **live smoke** | invariants on the **deployed** node | `tests/live_smoke.rs`, `#[ignore]`d | ~2s |
| **eval battery** | model quality against the real turn machinery | `tests/*_eval.rs`, `#[ignore]`d | minutes |

The first three run in `make ci`. The last two need something CI does not have — a deployed
node, or a live model — so both are `#[ignore]`d and run deliberately.

**Composition tests exist because two bugs got through with a green suite**, both times
because the tests exercised a component while production composed a stack: port methods that
no wrapper passed along, and a table that existed only in the test migration. If a test
constructs its own wiring, it is not testing the wiring.

**Golden tests exist because fixtures are only as imaginative as their author.** Three
confident hypotheses about a name-matching failure were each supported by a tidy five-line
fixture and each refuted the moment the test ran against a captured reading of a real house.
Prefer captured data over invented data for anything that parses or ranks.

**The live smoke check** asserts about real data using the **production rules**, imported
rather than re-implemented — an invariant with its own copy of a rule is testing the copy.
Run it after every deploy:

```bash
make deploy-check    # deploy, wait for the node, then smoke it
make smoke           # just the smoke check (set ENDORA_URL in local.mk)
```

It does not judge whether a screen *reads* well; that still needs a person. The point is to
make a screenshot the last line of defence rather than the first.

**The console needs its own check, because nothing else looks at it.** Every guarantee in the
Rust half is enforced by the compiler; the console has no type checker and no linker. A call
to a function that had been deleted stayed syntactically perfect, passed `node --check`,
passed CI, passed the smoke tier — and rendered a blank page on a phone.

`make console-check` loads `app.js` and **calls every screen**. Two things make it work:

- **Execution, not parsing.** A regex hunting "called but never defined" cannot be made sound
  — a JavaScript regex literal like `/https?:\/\//` reads as a line comment to a naive
  stripper, which swallows real code and reports all-clear on a broken file.
- **Realistic state, not empty state.** The first version populated nothing and *failed to
  catch the bug it was written for*: the missing call sat in a branch that only runs when a
  message has an action trail. An empty state exercises the early returns and little else, so
  every screen is given one representative item — shapes taken from the live node rather than
  invented, for the same reason the golden tier exists.

Stubs are explicit and minimal on purpose. A `Proxy` answering any unknown global would make
everything pass, including the failure this is here to catch.

It also holds a **budget: no screen may stack more than six sections.** A ratchet rather than a
target — the number is where the worst screen already sits. Two sections were added to it in a
single week without anyone noticing what they were being added to, which is the failure a
budget catches and a review does not. Counted from the *rendered* screen, so a section
contributed by a nested call counts like one written inline; the person sees no difference.
Lowering it is the point, and raising it should feel like a decision.

**Budgets belong in tests, not in a performance tier.** A latency suite has never caught
anything here, but volume degrading quality has: a tool that returned five kilobytes where a
timestamp was wanted. Assert budgets — result sizes, round counts — inside the tiers above.

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
  `endora-domain`, which stays dependency-free) needs a clear rationale and, for
  load-bearing choices, an ADR.
- **Prefer `n-1` for dependencies (guidance).** When a dependency is genuinely
  needed, favor one release behind the latest (e.g. latest is `0.40` → use
  `0.39`). This is a supply-chain precaution: it avoids being the first to adopt
  a freshly-published, potentially compromised ("poisoned") release before the
  ecosystem has had time to catch it. It is guidance, not a hard rule — take the
  latest when a **security** patch (or a needed fix/feature) calls for it, and
  say so in the PR.
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

Or run all of them at once with **`make ci`**. See the [Makefile](Makefile) for
every developer task (`make help`); new machines start with `make bootstrap`.
CI runs the same checks on Linux and macOS.

If you deploy, follow it with **`make deploy-check`** (or `make smoke`). It is the only tier
that sees real data, and it takes about two seconds.

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
