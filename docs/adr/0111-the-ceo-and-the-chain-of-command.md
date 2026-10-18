# 0111 — The CEO and the chain of command

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0102](0102-endora-hq.md) (the owner is the board, and a CEO member takes most of the desk),
[0107](0107-routines-alarms-and-check-backs.md) (the reviewer checks the CEO's decisions before
the desk, and the CEO has a daily review), [0108](0108-the-bridge.md) and
[0109](0109-a-brain-per-member.md) (brains by role, the pool shared among the paid, the CEO a
cloud member refused Confidential results). Built: the CEO's role, its brains and its turn cap;
the chain of command and decisions for the board; the house log; `read_doc`; the CEO and its
daily review seated once on an existing team; the relay's Opus and cap for the CEO; the bridge
and the contracts treating the CEO as a cloud member that only reads; the HQ screen's CEO,
House log card, decision cards and renaming. Amended by [0113](0113-projects-budgets-and-reports-to.md)
(2026-10-17): who asks whom follows each member's `reports_to`, which the owner may change, and
the CEO also proposes projects and goals, decides projects, and plans once a week. Amended by
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (2026-10-17): the reviewer is shown
as the **tester** and re-checks work on her own readings before its lead accepts it; the CEO's
decision for the board, the tester's passing it on, and the lead's question up to the CEO each
carry evidence from HQ's record, refused without; and whom a member reports to, and anyone
above, may move, close, accept, send back or hand on that member's task. Amended 2026-10-17
(evening) by the owner: **a question raised on a task in a project goes to that project's
lead**, when the lead is at work and is not the one asking; otherwise up the asker's own chain,
as before. Live, the tester (who reports to Ada) asked Ada about a task of Kit's project, which
Ada knew nothing of. The briefing says whom the turn's questions go to by the same rule.

## Context

Everything a member could not settle reached the owner's desk: the developer's question about
a container's memory, the lead's plan for the week, the reviewer's checked drafts. The desk
became the team's inbox, and the owner its manager. The plan (section 10) foresaw "a CEO member
[that] may take most of these, with the owner as the board".

## Decision

**Owner decisions, 2026-10-17.**

1. **The owner is the board.** The team gets a new top member, the **CEO** (role `ceo`), above
   the lead. Nothing operational reaches the board.
2. **Brains by role**, Claude only (no other provider is added):

   | Role | May think with | The owner's choice |
   | --- | --- | --- |
   | CEO | Claude Opus 5.5, Claude Sonnet 5.5, Local | Opus |
   | Lead | Claude Sonnet, Claude Haiku, Local | Sonnet |
   | Developer, reviewer | Claude Sonnet, Claude Haiku, Local | Local |
   | Researcher | Claude Haiku, Local | Haiku |

   HQ seats the CEO on Opus; the others are the owner's to set on each member's card.
   **Opus is the CEO's alone**: HQ refuses it for any other role, and so does the node's relay
   on every call.
3. **The CEO's turn may spend $1.00**; everyone else's stays at $0.40 (`money::MOST_A_TURN`).
   HQ reserves at most that, and the relay holds each call to it whatever HQ says is left.
4. **The pool is shared only among the members with a paid brain**, weighted by their roles'
   shares: CEO 35, lead 25, developer 40, reviewer 20, researcher 15. A member on the house's
   model costs nothing, reserves nothing and is never turned away for money. With only the CEO
   and the lead paid, they split the day's $2.33 by 35 to 25: $1.36 for the CEO, $0.97 for the
   lead. The shares are the members' (`share_percent`), hired with their role's weight.
5. **The CEO wakes only for escalations** (a question up the chain, its decision sent back or
   decided by the board) **and its own work** (its daily review, what it handed on, its own
   check-backs and retries). Being named in a thread, an alarm, a look round and a review
   request are refused as `not_for_the_ceo`.

**The chain of command.** While the team has a CEO at work (not paused):

- `ask_owner` goes to whom the member **reports to** ([0113](0113-projects-budgets-and-reports-to.md)):
  set at hire by role (the developer, the researcher and the reviewer to the earliest lead, or
  the CEO when there is none; the lead to the CEO), backfilled on start for members hired
  before, and the owner's to change on the member's card, never to itself or into a circle. A
  manager who is paused is passed over, one step up at a time; a chain that ends goes to the
  CEO. So by default, as first decided, the developer's, researcher's and reviewer's go to the
  **lead**, the lead's to the **CEO**, and with no lead at work a member's to the CEO. It is posted in
  the task's thread as "Question for <name>: …", the asker's turn ends as before, and the one
  above is woken (`question_asked`), its briefing saying "<asker> asks you: …".
- **Whoever is woken answers in the thread** (`post`). Their first post after the question that
  is not itself a question up the chain or a draft wakes the asker (`question_answered`), as an
  owner's reply does. They may instead decide, hand work on with `create_task`, or ask up the
  chain in turn; a lead that asks the CEO still owes the developer its answer. One question a
  member about a task waits for its answer at a time.
- **Only the CEO's `ask_owner` reaches the board's desk, as a decision**: its input is
  `{ "decision", "recommendation", "why" }`, each on one line and at most 400 characters. The
  card shows the three, with Approve, Decline and an answer box; the board's word ("The board
  approves: …", with what the owner wrote) goes in the task's thread and wakes the CEO.
- **The reviewer's fact-check stays, as one mechanism.** The CEO's decision goes through the
  reviewer as drafts did (0107): the task moves to `in_review`, the decision is posted as a
  draft, and the reviewer, told it is the CEO's, checks it. The reviewer's `ask_owner` then
  passes it to the board as the CEO's ("Checked by Grace, the reviewer."), or `review` sends it
  back. With no reviewer at work it reaches the desk at once.
- **With no CEO at work, today's behaviour stands**: home-lab drafts to the reviewer, and
  questions on the owner's desk.
- **The board's items**, said in the CEO's briefing: money beyond the pool; new goals and
  priorities; hiring or letting go a member; anything only the owner can know, framed as a
  decision with a recommendation. Approvals for actions set to "ask" keep going straight to the
  desk from the bridge (0108).

**The CEO's context.** Every turn of the CEO's starts with:

- **the house log**: a document of HQ's own (per team, not per goal, in HQ's settings), which
  the CEO keeps with `note_house` (`{ "entry" }`, one dated line, "- 17 Oct: …", at most 500
  characters) and `rewrite_house` (`{ "log" }`, the whole of it, at most 6,000 characters, for
  when it fills). The owner reads and edits it on HQ's screen. **The lead's turns start with it
  too**, read-only;
- its team, each member's role and brain; the goals with their notes and lessons; the open
  tasks with status and owner;
- the home lab's trouble in the last 7 days: the alarms HQ received (from its own
  `routine.fired` and `routine.posted` events);
- what reached the board's desk in the last 7 days, and how the board decided;
- the board's items, and that it wakes only for escalations and its review.

Since 2026-10-17 ([0109's cost notes](0109-a-brain-per-member.md#cost-notes)), only the CEO's
first turn and its daily review carry the goals' notes, the open tasks, the alarms and the
desk. Every other turn starts with the house log, the team, the board's items and the goals'
titles, then what changed since its last turn began: tasks made or moved, alarms, the board's
word and lessons noted.

**The cloud data rule (0109) holds.** The CEO, on a cloud model, is a cloud member that is not
the lead: the bridge and the contracts give it nothing Confidential (no logs, no queues, no Home
Assistant). The bridge's profile for the CEO is read only, and the contracts offer it no act
(no restart): it decides and hands work on. It reads members' findings in their threads. It
never sees the owner's brain, mail or chat.

**The public docs.** The CEO has `read_doc` (`{ "path", "part"? }`): Endora's public decision
records and plans, `docs/adr/*.md` and `docs/plans/*.md` (top level only), embedded in HQ at
build time by its build script, so nothing is read from the disk at run time and no path a turn
gives can reach a file. A path is a plain Markdown name in one of the two folders (no `..`, no
leading dot, no other folder); a folder alone lists them; a document is read 20,000 characters
a part. They are already public on GitHub.

**Seating the CEO on an existing team.** When HQ starts with the demo off and no CEO, it seats
one, once (a setting remembers it): "Mara", role CEO, share 35, Claude Opus, in the feed as
`member.hired`. Once the team has an active goal (the home lab's, if there is one), it makes
the CEO's routine "Daily review", every 24 hours, once, as it makes the home lab's routine. The
owner may rename any member (`POST /v1/hq/members/{id}/name`, signed by the node and recorded
in the audit log; names stay unique, since members are named in threads by them).

## Consequences

- The owner's desk holds decisions, approvals and notices, not the team's questions.
- The CEO's turns are the dearest (Opus, up to a dollar), and rare: escalations and one review
  a day. Members on the house's model no longer hold money while they run.
- Questions go up and answers come down in the task's thread, so the whole exchange is on the
  record where the work is.
- A question asked of a member renamed since is matched by the name it was asked of; renaming
  in the middle of an exchange leaves that question to the owner's reply or a nudge.

## Alternatives considered

- **A new `ask_board` tool for the CEO.** Rejected for one name: the CEO's `ask_owner` takes
  the decision's fields instead, and the worker's handling of a question (the turn ends) holds
  unchanged.
- **The node serving the docs.** Rejected: embedding them in HQ at build time is smaller and
  reads nothing at run time.
- **The pool split among every member.** Rejected by the owner: a local member costs nothing,
  and its share would sit unused while the paid ran out.
