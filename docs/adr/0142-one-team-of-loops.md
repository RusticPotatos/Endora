# 0142 - One team of loops

## Status

Accepted, 2026-10-27, by the owner. AI-assisted (Claude). Supersedes
[0110](0110-the-teams-rings.md), [0111](0111-the-ceo-and-the-chain-of-command.md),
[0120](0120-roles-as-an-enforced-raci.md),
[0137](0137-addressed-conversations-and-bounded-assistance.md) and
[0138](0138-one-conversation-and-assistance-lifecycle.md). Amends
[0113](0113-projects-budgets-and-reports-to.md) (projects become loops; no reporting lines),
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (checks replace the tester),
[0115](0115-the-engineering-wing-and-the-git-flow.md) (no wing; a public PR is reviewed one
rank up), [0118](0118-elastic-staffing-and-notebooks.md) (no staffing or notebooks),
[0119](0119-the-open-office.md) (no office proposals or votes),
[0121](0121-games-in-the-office.md) (ping-pong and pool are animation only),
[0122](0122-the-teams-world-stages-stories-and-situations.md) and
[0126](0126-the-teams-map.md) (the house and lab pages, loop notes and a self-growing map
remain), [0124](0124-guards-against-how-agent-teams-fail.md) (only the safety caps remain),
[0132](0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md) (five kinds of card) and
[0139](0139-one-team-escalate-by-tier.md) (escalation by model tier is the only escalation).
The full plan and its evidence:
[one team of loops](../proposals/one-team-of-loops-2026-10-27.md).

## Context

Each of the decisions above added a role, a rule or a guard against what the one before it
caused. On the record:
- talk outweighed work (80 turns to 46 in a day);
- #796 went round the chain of command for a day without reaching the owner;
- no help request was ever answered;
- of the memory tools, only reading a page and searching history were used;
- about 3 of 20 desk decisions in a day needed the owner.

The team context had grown to about 104,000 lines. The owner wants it boring under the hood
and fun to watch, and every line of code is another bug.

The design borrows from Ralph (Huntley; snarktank/ralph):
- a loop takes the most important unfinished item from its plan;
- it does one thing, runs the checks, and commits or reverts;
- it writes down what it learned, and goes round again.

Checks push back on bad work, not reviewers, and a loop is steered by a line in its plan rather
than by a new role.

## Decision

- **Members are models, ranked:** Mara (Opus), Kit (Sonnet), Ada (Haiku), Rae (the Mac's
  model), Bo (the NAS's local model). A title is a name only. The eight names are fixed; a
  generator names any members past eight, and every name is unique. There are no roles, chain
  of command, testers, conversations, help requests, stuck ladder or huddles.
- **A loop is a project with a plan (its ordered task list), its checks and a starting rank.**
  A free member takes the top unfinished item. One turn at a time per member; the local
  members work whenever there is work.
- **Escalation is by rank only.** Three failed checks (a failed check, or a send-back) move an
  item up one rank. A failure at the top rank goes to the owner's desk.
- **A branch for a public PR is reviewed by the member one rank up** (Mara's by Kit). Then it
  reaches the owner's desk as a ready PR. Playbook runs stay the owner's; container restarts
  stay capped and announced.
- **The owner's desk holds five kinds of card:** money, credentials, accepting a risk,
  anything irreversible, and taste. Plus ready PRs. Notices clear themselves. HQ sees merges
  itself.
- **Memory:**
  - the house page and the lab page;
  - one notes page per loop, appended each turn;
  - history search;
  - a map that members add one line to when they find where something lives. Unread lines fade
    after 30 days; used lines rise.
- **What stays:** the safety caps (runaway, free turns, pool and per-loop caps, kill switch,
  drain), "models propose, code authorizes", confidentiality tiers and the record.
- **What leaves the code:** everything listed under "What goes" in the plan. Retired tables
  are backed up and archived before they are dropped.

## Consequences

- The team context falls to about 55,000–60,000 lines within a day, and under 50,000 after.
- Fewer cards reach the owner; those that do are the owner's by nature.
- A member may now do anything its loop's tools allow. The checks, not a colleague, are what
  stop bad work, so a loop without good checks (the lab) needs them built before it is trusted.
- Endora working on its own code, and loops running the playbook, wait for evidence from the
  Holodeck and the league.

## Alternatives considered

- **Keep the structure and add rules for what reaches the owner.** Rejected: each rule so far
  bred another, which is how the structure grew.
- **Literal Ralph: bash loops calling a cloud CLI.** Rejected: every turn would be paid, and
  nothing would gate a model's actions on the house.
- **A week's pilot beside the team.** Rejected by the owner: cut straight over, with the
  `before-loops` image and snapshots as the way back.

## Amended 2026-10-30: round 1 of closing the gap

After the baseline experiment (`docs/proposals/baseline-experiment-2026-10-28.md`: Claude Code
10 of 10, HQ 1 of 10), the owner had its design defects fixed and the bottom of the ranks
strengthened, as the experiment's first rounds.

- **Rank 1 runs.** The member whose brain is Opus thinks with it, at the dollar a turn the CEO
  had, in the relay and in HQ's reservation. Before, the relay kept Opus for a role nobody had.
- **A failure at the top rank lets its loop go on.** The item is blocked and let go, and the
  owner is told once; before, it stayed with Mara and stalled the loop.
- **Only finished turns count against an item.** A turn the system ended (refused, timed out,
  failed) is not the member showing nothing.
- **A read-only task's turns are refused the lab's acts.** When a task's words leave the lab as
  it is, HQ tells the node to refuse `restart_container`; and the node refuses to restart a
  container others run in the network of (gluetun) whatever the task.
- **A chess move is given three turns.** Then the side to move forfeits, so nobody is woken for
  a move it never makes.
- **The owner's projects need no lead role**, and a paused world or league neither forfeits
  nor opens games.
- **DeepSeek Flash is a brain**, ranked between Haiku and the Mac (Opus 1, Sonnet 2, Haiku 3,
  DeepSeek 4, Mac 5, the house's model 6). It is a cloud model, so it is offered nothing
  Restricted (ADR 0141), which was the owner's condition; its calls go through the node's local
  relay to the DeepSeek entry on the Everyday list alone, are held to the turn's budget and
  charged at DeepSeek's peak prices. A DeepSeek member is refused every Claude.
- **The house's model is qwen3.5:9b** on the NAS's A2000, in place of qwen3:8b (a setting on the
  Everyday list, not code).

## Amended 2026-10-30: a loop has items side by side

In the baseline experiment eight of the ten tasks sat in one loop, and a loop handed out one
item at a time, so they ran one after another for hours while other members sat idle.

- **A loop has up to N items in hand at once**: two unless the owner says, from 1 (one after
  another, as before) to 4, set per loop like its starting rank (`POST /v1/projects/{id}/items`
  on HQ, `/v1/hq/projects/{id}/items` through the node) and kept in HQ's settings. An item is in
  hand while it is given to a member and still todo or doing.
- **Each goes to a different member.** The loop's items go to the members at its starting rank
  or nearest above it, as many of them as the loop has items at once, and never two to one
  member. A paused member, or the Mac's while the Mac is away, is passed over; when none of
  those members is free, the item waits rather than stacking or climbing further.
- **Per item, nothing else changes:** its failures, its climb by rank, its review one rank up,
  and the hand-up of a paused or away member's items (which may land on a member who already
  holds one of the loop's items, as a climb may).
- **A world is not a loop.** A project holding scenarios (the Holodeck league) is skipped as
  before: its games go one at a time.
