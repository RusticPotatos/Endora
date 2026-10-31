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

## Amended 2026-10-30: round 2 of closing the gap, context sized to the turn

The ladder's rung 4 ("context sized to the turn") moved up to be round 2, since rungs 1 and 2
are measured together. A house turn (brains `local` and `mac`, and DeepSeek, which speaks the
same OpenAI shape through the node's local relay) read every tool's definition and the lab's
whole rulebook on every call: a chess move read about 9,700 tokens over its three calls, and a
turn on the Mac timed out at its relay once it had read a few 16 KB logs.

- **Tools on demand, sized to the turn's kind.** A house turn with more than a dozen tools starts
  with HQ's core (`my_turn`, `claim`, `post`, `set_status`), what HQ marks for the turn's kind
  (MCP `_meta` `endora/up_front`: `chess` in a downtime turn, `review` in a review turn, a
  match's own tool), the workbench's shell, and on a turn with a task `ask_owner` and the lab's
  container tools. The rest are named in its instructions and brought in with the worker's
  `find_tools`, by name or by a few words, for its next call. `find_tools` only ever picks from
  what HQ and the node listed for the turn, where every narrowing already happens (a project's
  tool list, `only` and `never`, the Restricted tier, the NAS's keep from tools that can end on
  the owner's desk); a name it was not offered is said to be none of the turn's tools and is
  never added, and a call to it anyway is refused by HQ and never reaches the node. HQ's mark
  grants nothing: every call is decided as before. Claude keeps Anthropic's tool search and its
  loaded set, unchanged.
- **The lab's rules where its tools are.** A house turn with no task whose lab tools are to be
  found (a chess move, a look round) is told the tools are there and how to bring them in, not
  their rules; a turn with a task, or with all its tools listed, is told them in full.
- **Shorter descriptions, the same rules.** HQ's core tools, `chess`, `review` and the lab's
  contracts say each rule in fewer words; tests hold each rule's words and a size per tool.
- **Less of a log on the house's models.** `container_logs` gives a turn on the NAS's model or
  the Mac the newest 40 lines and 6 KB, says when older lines were left out and that `lines`
  (up to 100) asks for more; a Claude or DeepSeek turn reads the newest 100 lines and 16 KB, as
  before. HQ's narrowing comes first: a turn not given the log is refused it however much it
  asks for.

Measured on the engine's real briefings and HQ's and the node's real tools (the worker's test
`a_house_turns_first_call_is_sized_to_the_turn`, bytes of the relay's body, about four a token):
a chess move's first call 12,497 to 7,798 bytes and its three calls 38,857 to 24,760 (about
9,700 to 6,200 tokens); a task turn's first call 12,991 to 11,866. A log on a house model is
at most 6 KB instead of 16 KB unless the turn asks.

## Amended 2026-10-30: HQ judges what can be judged mechanically

In the baseline experiment a person re-checked every claim HQ's work made: whether quoted log
lines were really in the logs, whether a branch changed only allowed files, whether a README
named every default with its value. The review one rank up is a model's turn and is fallible.
What a rule can decide, HQ now decides itself, when an item reaches review
(`domains/team/src/domain/mechanical.rs`, wired in `application/engine/mechanical.rs`):

- **Quoted evidence.** Each line the worker's newest post quotes (in backticks or a fenced
  block) and attributes to a receipt (`r12`) is looked for in that receipt's kept result,
  ignoring whitespace. A receipt keeps only the start of a result (300 characters), so a quote
  missing from a cut excerpt is reported as "cannot tell", never as false.
- **Branch scope.** The files the work's newest branch changes, which the node now hands HQ on
  `repo_submit`, are checked against the paths nobody touches (`group_vars/`, `host_vars/`,
  inventories, anything named for a vault) and, when the task names paths, against those.
- **Defaults coverage.** When a criterion asks for a README of the defaults, each role README
  in the branch must name every top-level key of the role's `defaults/main.yml` with its value
  (trailing comments and surrounding quotes ignored). The node hands HQ those two files as they
  are at the branch's head; a file over 32 KB is not handed over, and its README is reported
  unchecked.

The report is posted on the item by Endora and the reviewer's briefing points to it, or repeats
it when the thread's news no longer shows it, as facts to weigh. The reviewer still decides,
except in one case: a branch that touches a forbidden path is sent back by HQ at once, with the
reason, counts as one of the item's failures, and wakes no reviewer. The node already refuses
such a branch at submission; the check at review keeps the rule from resting on one place.

## Amended 2026-10-30: fewer handoffs, a handover when an item changes hands

Rung 5 of closing the gap to Claude Code. When an item changed hands (a climb after three
failures, a paused or away member's hand-up, a reviewer's send-back), the next member started a
fresh turn that re-read the task and its thread and took the same readings again: in the
baseline of 28 Oct the same container logs were read at every rank, and a send-back's reasons
had to be found again. The work restarted instead of continuing.

- **HQ composes a handover.** At each change of hands HQ builds, by code from the record and with
  no model call, a note of what the item asks (each criterion on one line, or its title for work
  with none) with what the record shows of each (the references last cited for it, or nothing
  cited yet, and whether it was sent back since), the reviewer's or the checks' reasons if it was
  sent back, the readings its turns already took (newest first, the newest of each tool and
  arguments, each `rN tool args: gist`) for the next member to cite rather than take again, and
  the members' last posts on it. Only that item's record is read.
- **Sized to the receiver.** The note is held to the receiving member's model: 900 characters on
  the NAS's model, 1,200 on the Mac, 2,000 on Claude. What does not fit is dropped from the end of
  each part, in order of worth: the asks, the reasons, the readings, the tries. Members' words
  are cleaned to one line each.
- **Told and shown.** The note is kept for the receiving member and told in its own section of
  each of its briefings on the item while the item is still its (and not again among the thread's
  news); it carries members' words, so a turn told it is marked as having read input of unknown
  origin. It is posted on the item's thread as Endora's, so the owner sees what was handed over.
- **Not a member's own note.** A member's handover note at a restart (ADR 0134) is unchanged.

## Amended 2026-10-30: old tool results read as stubs on the house's models

A work turn makes 5 to 15 model calls and each resends the whole conversation, so measured
turns read 110,000 to 200,000 prompt tokens, mostly old tool results (container logs, the
lab's status, a branch's diff) sent again on every later call. On the Mac's model, reading
about 100 tokens a second, that alone ran calls into the relay's timeout.

- **An old result becomes a stub.** In the worker's own loop (`app/worker/src/turn.rs`), once
  the model has replied after a result, and the result is not among the newest two, the next
  calls read one line in its place: the tool and its arguments (cut to 120 characters), the
  receipt the node gave it (`(receipt r41)`), its size in characters, the first line with
  anything in it, and that calling the same tool with the same arguments reads it whole again.
  A result of 600 characters or fewer is never cut. A result is cut once, and stays cut; a
  result the model has not replied to (one of several in its last round) is never cut.
- **The way back is the call, not the receipt.** HQ keeps only the start of a result in a
  receipt (300 characters), and has no tool to read one, so the stub does not promise the
  receipt holds it whole. A call asked for again after its result was cut is sent again and
  answered in full: the repeat guard forgets it rather than saying it was already had.
- **Kept whole where it is kept.** What HQ keeps of the conversation (ADR 0129) is the whole
  conversation; only what the model reads is cut.
- **Only house turns.** The worker cannot tell the NAS's model, the Mac and DeepSeek apart (the
  node's relay routes them by member), so all three keep the slow machines' two. Claude turns
  are unchanged: the worker caches Claude's whole prompt with its breakpoint on the last block,
  so cutting a result would change the prefix of every later round and make each round write
  again everything after the cut (at a write's price) rather than read it from the cache (at a
  tenth). Claude Code's runtime is out of scope.

Measured on the engine's real briefing and tools for a task turn and ten realistic calls (the
lab's status twice, four container logs of 40 and 100 lines, the standards, a branch's diff, a
restart and a post; the worker's test `a_house_turns_first_call_is_sized_to_the_turn`, bytes of
the relay's bodies after the first call, about four a token): 478,129 bytes with every result
whole, 298,407 with old ones stubbed, about 119,500 to 74,600 tokens. About 12 KB a call is the
floor, the instructions, the tools and the briefing.
