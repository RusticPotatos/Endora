# Testing: the owner's scenarios

The tiers of tests and what each is for are in
[CONTRIBUTING.md](../CONTRIBUTING.md#the-test-tiers-and-what-each-one-is-for). This page is
about one of them, the **scenarios**: behaviour the owner decided, written in the owner's own
words, and run against the real code. It is the pilot agreed for Endora HQ; the plan is to
grow it, and to prune the unit tests that pin how the code is written rather than what it does,
once the scenarios cover that behaviour.

## What a scenario is for

A scenario says what the team does, as the owner would say it, and nothing about how. It
reads like the decision it protects:

```gherkin
Scenario: One task a problem
  Given Ada has the task "Check the home lab"
  And Ada hands Linus the task "Diagnose and fix exited deluge, nzbget, prowlarr" under "Check the home lab"
  When Ada hands Linus the task "Confirm state and restart deluge, nzbget, prowlarr" under "Diagnose and fix exited deluge, nzbget, prowlarr"
  Then it is refused: "#2 already covers this (deluge, nzbget, prowlarr): add to it with post in #2, rather than a second task"
  And the team still has 2 tasks
```

The owner can read that and say whether it is what they meant. A unit test asserting the
shape of a `Refusal` cannot be read that way, and it is also what breaks when the code is
reorganised without the behaviour changing. Scenarios are the tests that should survive a
refactor and fail on a change of mind.

Every scenario so far is a decision from an [ADR](adr/README.md) or from the live incident of
2026-10-17 that produced three of them (ADRs 0107, 0108 and 0112): one task a problem, coming
back is news, the owner's devices are never the team's.

## Where they live

| context | the owner's words | the steps | runs as |
| --- | --- | --- | --- |
| `domains/team` (HQ's engine) | `domains/team/features/*.feature` | `domains/team/tests/scenarios.rs` | `cargo test -p endora-team --test scenarios` |
| `domains/capabilities` (the bridge) | `domains/capabilities/features/*.feature` | `domains/capabilities/tests/scenarios.rs` | `cargo test -p endora-capabilities --test scenarios` |

Each context owns its own scenarios, as it owns its domain (ADR 0050): a rule that lives in
the bridge is tested in the bridge's crate, not through HQ. `cargo test --workspace` runs them
all, so the gate does too. One scenario name, or part of one, runs just that scenario:

```bash
cargo test -p endora-team --test scenarios -- "One task"
```

The harness is [cucumber-rs](https://github.com/cucumber-rs/cucumber) (`cucumber`, a
dev-dependency only, never shipped). Release 0.21 is the last that builds on the workspace's
minimum Rust; the runner prints each step as it goes, and the test target has `harness = false`
so it can. The flags `cargo test` passes every binary (`--test-threads`, `--nocapture` and the
like) are dropped before cucumber sees them.

## How to write one

1. **Start from the decision.** Find the sentence in the ADR, or the owner's words in the
   thread, and write the scenario so that the owner would recognise it. Name things as the
   owner does: Ada, Linus and Mara are the team; the containers are the home lab's own names;
   money is in dollars. Nothing personal ever goes in a feature file (see
   [AGENTS.md](../AGENTS.md)).
2. **Go through the front door.** A step does what the owner, the node or a member's worker
   would do: hire, set a goal, hand out a task, raise an alarm, use a protocol tool in a turn,
   decide on the desk. It reads what the HQ screen would show: the tasks, a thread, the desk,
   the wakes and their receipts, a project's cap. A step never reaches into the engine's
   internals, and never asserts a string the model is told unless that string *is* the
   decision (a member is told its tools are its own).
3. **Reuse steps before adding them.** The steps already written are the vocabulary; read the
   harness first. A new step should be a sentence the owner would say, with the variable parts
   in `{string}`, `{word}` or `{int}` (a cucumber expression), or a regex when the sentence
   has a possessive or a dollar sign.
4. **Make it fail first.** Write the scenario before the behaviour, or, for behaviour that
   exists, break the behaviour locally and watch the scenario fail, then put it back. A scenario
   that passes with the behaviour removed tests nothing. (The pilot's check: with the one-task
   rule disabled, "One task a problem" fails on its `Then`.)
5. **Keep it short.** One decision a scenario; a `Background` for the team and the goal every
   scenario in the file shares; no more steps than the sentence needs.

## What they cost

The pilot's 14 scenarios (11 in `domains/team`, 3 in `domains/capabilities`) run in well under
a second together; the engine's scenarios share one in-memory SQLite store each and the
simulated runner, like the unit tests beside the engine. The crate adds compile time to the
two crates' test targets only.
