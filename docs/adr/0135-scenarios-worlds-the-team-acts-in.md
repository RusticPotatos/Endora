# 0135 - Scenarios: worlds the team acts in, which only the world closes

## Status

Accepted, 2026-10-20, by the owner, as phase 5 of the plan towards the Holodeck (the phase 0
audit of 2026-10-19). Prepared with AI assistance (Claude). Builds on
[0121](0121-games-in-the-office.md)'s chess (a world kept by HQ, its rules in code, played through
one tool), on [0113](0113-projects-budgets-and-reports-to.md) as amended on 2026-10-20 (a
project lists the tools its work is given), and on [0114](0114-evidence-acceptance-criteria-and-the-tester.md)
(a model's word is not evidence). Built: everything below, with one scenario, the lighthouse.

## Context

The Holodeck is the team acting in simulated worlds, to try configurations of members and brains
against each other on work whose outcome is known. Its first gate (G3 of the owner's roadmap):
a tiny world that resets and replays, where a model's claim can never change the world. HQ
already had most of it: tasks, turns, briefings, budgets, events and replay know nothing of the
home lab, and chess showed a world kept in HQ's store and changed only by a tool checked in code.
What it lacked was a world that ends a task by itself, a record of every action that is never
pruned or changed, and a way to give a turn that world's tool and nothing else.

## Decision

1. **A scenario is a task.** The owner starts one (`POST /v1/scenarios {project, member, seed}`,
   through the node's `/v1/hq/scenarios`) in a project, for a member. Its description says
   what the world is and what can be done in it. Starting it again with the same seed is its
   reset: a new task, the same world.
2. **Its project gives it the scenario tool alone.** A scenario starts only in a project whose
   list of tools (0113, amended) is exactly `["scenario"]`, so its turns are offered their own
   four tools and `scenario`, nothing of the node's (no lab, bridge or repo tool) and no shell,
   and the three layers that enforce the list refuse anything else.
3. **The world is a pure function of its seed and its log.** Its rules are in the domain
   (`domain/scenario.rs`), deterministic from a seed (splitmix64, as chess's matches). Its state
   is never stored: it is the seed's world with the log's accepted actions replayed in order.
4. **The log is append-only.** Every action a member takes is kept in `scenario_actions` (which
   turn, which member, the action, whether the world accepted it, and what the world said),
   refused ones too; triggers refuse any change or deletion, as for the ledger and receipts. A
   refused action changes nothing and costs nothing in the world. Each action is also an event,
   `scenario.acted`, for the feed and replay.
5. **Only the world closes it.** The rules say when it is solved or failed; HQ then closes the
   task in code (done, met criteria; or dropped, blocked for good), posts the world's words in
   its thread, and never asks a tester or a lead to judge it. A member's `set_status` on a
   scenario's task is refused, whatever it claims.
6. **The owner reads it step by step** (`GET /v1/scenarios/{task}`, through the node): its seed,
   where it stands, how it ended and why, and its log, who did what, what was refused and what
   the world said.

The first scenario is **the lighthouse**: its lamp is dark, for a cause the seed hides (the bulb
or the fuse). Three actions (`inspect`, `test_circuit`, `replace_part`), each with a precondition
and a cost in minutes, an hour to light it, and two ways to do so in time.

## Consequences

- A scenario costs nothing on the house's models, and its project may be capped at $0.
- The log grows with every action and is never pruned: small, and the record a trial compares.
- What a scenario does not have yet: a panel on the owner's screen (the owner reads it through
  the API), more than one world, and the trials that compare configurations (phase 7).

## Alternatives considered

- **Keep the world's state in a table.** Rejected: two records of one world can disagree; a
  state derived from the log cannot, and replay is then the same code as play.
- **Let a tester judge the outcome.** Rejected: the point of a simulation is that the outcome is
  known; a model judging it brings back what the roadmap's G3 rules out.
- **Log only the accepted actions.** Rejected: which actions models try and the world refuses
  is what a comparison of brains most wants to see, and chess lost exactly that.
