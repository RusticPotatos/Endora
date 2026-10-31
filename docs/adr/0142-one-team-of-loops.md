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

## Amended 2026-10-30: every pass fits one fresh context window

Ralph's rule (Huntley; snarktank/ralph) is that each pass of the loop starts with a fresh context
and does one thing within it; what it learned is carried in files, not in the conversation. HQ's
work turns broke it: a turn went on until the model stopped or its twelve rounds ran out, every
call re-sending the whole conversation, so measured turns read 110,000 to 200,000 prompt tokens
(above) and a long one on the Mac ran its calls into the relay's timeout. Stubbing old results
shrank each call; nothing bounded the turn.

- **A turn has a prompt budget by its kind.** The worker sums the bytes of every call's body it
  sends (about four a token, as the measurements above count them). When the next call would
  take the sum past the budget, the turn ends there, cleanly: every call the model made has been
  answered and receipted, and no call is cut. The first call is always made. The budget is
  240,000 bytes (about 60,000 tokens) on a house turn (the NAS's model, the Mac, DeepSeek; the
  worker cannot tell them apart) and 480,000 (about 120,000) on Claude, unless the runner sets
  `ENDORA_PROMPT_BUDGET`. Why these: the Mac reads about 100 tokens a second, so 60,000 is about
  ten minutes of reading in a turn, and the measured ten-call task turn read about 74,600 tokens
  stubbed; a Claude turn of that length with every result whole read about 119,500, so 120,000
  lets it finish on its own and cuts only the long ones. Claude Code's runtime keeps its own
  compaction and is out of scope.
- **Its last word says it goes on.** The worker posts what the model said on the way and that
  the prompt reached its budget, as its last word (not progress), marked `continues`. Only the
  worker sets the mark: it strips `last_word` and `continues` from the model's own posts, so a
  model cannot buy its item a continuation.
- **HQ continues the item with the same member, from a handover.** Once that turn has ended, and
  while the item is still the member's, in the loops and neither in review nor finished, HQ
  composes the handover above for the same member (the asks and what is cited of each, the
  readings its turns took, to cite rather than take again, and the last posts, the last word
  among them), posts it, and wakes the member with the new reason `continued`, a work turn. The
  fresh turn starts from its briefing and that note instead of the whole conversation.
- **Running out of room is not a failure, up to three times.** It is not one of the three failures
  that move an item up a rank, and a turn that ran out of room is never counted as one that
  showed nothing, even when it took readings the turn before had taken. An item continues with
  one member at most three times; the fourth time is one of its failures, as a turn that never
  finished was, the count starts again, and if the item is still the member's it goes on from a
  fresh handover. The count is kept per item (`loop_continued:<task>` in HQ's settings).
- **The transcript stays whole.** What HQ keeps of each turn's conversation (ADR 0129) is that
  turn's whole conversation; only what the model reads is bounded.

Measured on the engine's real briefing and tools (the worker's test
`a_house_turns_first_call_is_sized_to_the_turn`, a work turn of twelve calls, the worker's most
rounds, on the lab's status, logs, the standards, a diff and a restart; each pass after the first
starting from the briefing with a 2,000-character handover): on a house model 362,984 bytes as
one turn (about 90,700 tokens) became two passes of 235,332 and 100,816 (the larger about 58,800);
on Claude, every result whole, 644,020 bytes (about 161,000 tokens) became 421,162 and 56,016
(the larger about 105,300). No pass reads more than its budget, and the two passes together read
less than the one turn did.

## Amended 2026-10-30: a briefing sized to the item, a review sized to the review

Measured the week of 23 to 30 Oct, work turns read 110,000 to 213,000 prompt tokens, and each
call resends the briefing. A work turn's briefing carried the whole map and world (the house
page's sections, the lab's lines, every map line, the loop's other items' notes) whatever its
item was about; a reviewer's carried the same, and the item's whole thread besides.

- **A work turn is told the world its item names.** The item's words (its title, description
  and criteria, and its project's title) are compared with each line of the world
  (`domains/team/src/domain/relevance.rs`): a line is kept when it shares a word with the item
  that is neither common (the language's own and the lab's every-day words, in one list) nor
  on more than a third of the lines weighed (three or more), which tells them apart from
  nothing. Kept: the item's project and goal pages and their sections; the house, lab and team
  pages a line each; the house's sections, the lab's lines, the map's lines and the loop's
  other items' notes that name what the item names. Everything else is one line saying which
  tool reads it: `read_world world:house` for the house's other sections, `read_world
  world:lab` for the lab's other lines, `read_world world:map` (new: the map's lines, whole,
  most confirmed first) for the other map lines, `ask_record` with the project for the loop's
  other notes, and `read_world` with a page's address for other pages. A turn about no task,
  and downtime, are told as before; the nightly quiz's map is unchanged.
- **A reviewer is told what the review needs.** A turn woken to review an item is told who it
  is, the item and its criteria, who asks and how a review is done, the worker's handover (its
  ask for review, the evidence it cited, and HQ's handover note on the item if there is one),
  HQ's mechanical checks in full (no longer pointed to in the thread), the owner's last three
  posts on the item and the goal's notes, and the branch: its check, its stat, the files it
  changes (at most 20 named) and its diff, cut at a line's end within 2,400 characters on the
  house's models and 12,000 on Claude, with `read_document` for the whole. Not the thread's
  history, the map, the house, the lab or the standards: one line says which tool reads each
  (`my_turn` gives the thread's newest posts). The whole thread's lineage is still inherited,
  as before, since what the review is told is drawn from it.
- **Measured** on one scenario (`application/briefing_size_tests.rs`: ten house sections, twelve
  map lines, four projects, a loop with two earlier items, an item with fifteen posts and a
  branch with a 7 KB diff), bytes of the briefing, about four a token: Bo's work turn on the
  NAS's model 6,287 to 3,133 bytes (about 1,570 to 780 tokens), held under 3,400; Rae's review
  on the Mac 11,967 to 5,557 (about 2,990 to 1,390 tokens), held under 5,800, while now carrying
  the diff it had none of. A work turn of ten calls saves about 8,000 prompt tokens on this
  world, more as the world grows: what is cut grows with it, what is kept with the item.

## Amended 2026-10-30: a long result is read in a side call, and its gist kept

Ralph's other rule (Huntley; snarktank/ralph) is to read in a side context and keep only the
conclusion. A house turn read a long log, diff or listing whole, and read it again on each of the
next two calls before it became a stub (above): a 100-line log of 12 KB was read three times, about
9,300 tokens, for the few lines that mattered to the task.

- **A long result is read first by a side call.** On a house turn, a result of `container_logs`
  (the lab's logs), `homelab_status` or `homelab_map` (its listings) or `read_document` (a branch's
  diff, a check's report) longer than 10,240 bytes is sent, by the worker (`app/worker/src/gist.rs`),
  in a one-shot call to the same relay and the same brain, with the turn's token: told the turn's
  task (its line and description from the briefing, 600 characters at most), what the model said
  before the call (300), the call and the raw result, with no tools, and asked for a gist of at
  most 800 characters, the lines that matter quoted each on a line of its own after `> `. Prose the
  model must follow whole (`read_doc`, `ansible_nas_doc`) is not gisted.
- **Quotes are checked by code.** Each quoted line, and each span in backticks, must be an exact
  part of the raw result; a line that quotes what the result does not hold is dropped, and the turn
  is told how many were. The model's thinking is left out, and the gist is held to 800 characters
  by whole lines (a quote is never cut).
- **The turn reads the gist and the way back.** In the result's place it reads what the result
  was (the call, its receipt, its size), the gist, and `To read it whole, call <tool> with
  <its arguments and "raw": true>`. The worker takes `raw` off before the call goes to HQ or the
  node, and a call asked for raw is never gisted. Once old, a gisted result is cut to its stub as
  any result is.
- **Never an enforcement boundary.** Nothing is decided on a gist; it only changes what the model
  reads, and the model can always read the result whole. The raw result came from a tool the turn
  was already allowed, so the side call sees nothing the turn could not.
- **Failure reads the raw result.** A gist call that fails, is refused, times out, reaches its
  1,024-token limit (thinking included: the team's turns always think, ADR 0105), keeps nothing, or
  would not be shorter, leaves the turn reading the capped raw result, as before. It may take as
  long as the reading it spares the Mac (the result read three times less its gist read three
  times, at 400 bytes a second; 69 s at 10 KB, 115 s at 16 KB), at least 10 s.
- **Counted.** The gist call's body is added to what the turn has read, against its prompt budget
  (above); a gist call that would take the turn past it is not made, and the raw result is read.
  The relay charges it to the turn like every call, so a DeepSeek turn pays for it and the Mac's
  and the NAS's are logged as their own use.
- **Kept whole where it is kept.** The conversation HQ keeps (ADR 0129) holds the raw result.
- **House turns only, on by default.** A Claude turn reads every result whole: its prompt is
  cached, and a gist in place of a result would change the prefix every later round reads at a
  tenth of the price. The runner may set `ENDORA_GIST_PAST` to another size or `off`.

Why 10,240 bytes. A gist pays when the reading it spares is more than the call costs. A result of
R bytes is read whole by three calls; gisted, it is read once by the side call with about 1,000
bytes of instructions and task, and its gist (620 to 910 bytes, measured) three times: about
2R − 3,700 bytes less reading. The side call also writes: a gist of about 200 tokens and its
thinking, on the Mac at about 14 tokens a second against 100 read (measured 28 Oct, 11 to 17
written, 95 to 108 read), so each token written costs the time of about 29 bytes read. With no
thinking it pays past about 4.7 KB; with 300 tokens of thinking, past about 9 KB. The house's logs
are already held to 40 lines and 6 KB, so they stay whole; the 100 lines asked for, a diff and the
lab's long listings are gisted.

Measured on the engine's real briefing and tools (the worker's test
`a_house_turns_first_call_is_sized_to_the_turn`, bytes of the relay's bodies, the gist calls' among
them, with a gist call answering a sentence and four quoted lines): the ten-call task turn read
298,407 bytes with old results stubbed and 258,368 gisted (about 74,600 to 64,600 tokens; the
100-line log of 12,432 bytes and the diff of 10,511 are gisted). The twelve-call work turn read
362,984 bytes as one turn and 323,113 gisted (about 90,700 to 80,800 tokens); held to the house's
budget its passes read 225,475 and 70,620 bytes instead of 235,332 and 100,816. On the Mac (prompt
at 400 bytes a second, gists written at 14 tokens a second) that turn takes about 907 s ungisted
and 824 s gisted, or 867 s if each gist call thinks 300 tokens. Gisting everything past 2 or 4 KB
(nine gist calls) reads less, 292,825 bytes, but takes about 822 s with no thinking and 1,015 s with
300 tokens of it, slower than not gisting at all; past 12 KB, only the 100-line log is gisted
(341,642 bytes, about 864 s). The first call of a turn is unchanged (11,878 bytes for the task
turn). The thinking a gist call does on the Mac is not measured yet: the first live turns'
`team-mac` usage records will say, and the size moves with it.

## Amended 2026-10-30: what a review of the loops found

A bug and security review of the loops (round 2) found rules that rested on a model's or a
worker's word, or that held an item where the owner would not. Each is now decided by HQ.

- **HQ decides whether a turn ran out of room.** The `continues` mark on the worker's last word
  is the worker's word, not HQ's authorization: a Claude Code turn has the turn's token in its
  environment and could post the mark itself. HQ continues the item only when the turn ran on
  the worker (never Claude Code, which keeps its own context and has no prompt budget of the
  worker's) and the relay metered at least half its brain's budget in prompt tokens (about
  60,000 tokens on the house's models and DeepSeek, 120,000 on Claude): the worker ends a turn
  when its next call would pass the budget, and the next call is about as large as the last.
  Otherwise the last word is posted and the turn ends as any other, and is judged as any other.
  So that HQ sees them, the relay now tells HQ the tokens of the house's free calls too
  (`/v1/team/tokens`), charging nothing.
- **A quote is checked against this item's readings only.** HQ's quoted-evidence check took any
  receipt a post named, so a line quoted from another item's reading (or a turn's with no item)
  passed as found. It now takes only a receipt whose task is the item's, as the handover does;
  any other is reported as "not this item's receipt", and counts as a failed check.
