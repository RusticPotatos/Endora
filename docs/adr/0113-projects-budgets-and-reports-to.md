# 0113 — Projects, budgets and reports_to

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amended by
[0120](0120-roles-as-an-enforced-raci.md) (2026-10-18): only a lead proposes a project, and the
CEO decides (yes, no, not now) and proposes none; the CEO's planning shows strategy, the week's
measures, the proposals and the budget split, and the container patterns go to each lead's
weekly patterns turn. Amended by
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (2026-10-17): the CEO decides a
project on evidence from HQ's record (`decide_project` takes `evidence`, at least one reference,
each looked up), by the one rule proposals are checked by; the deferred "turn verdicts that
weigh evidence" are built there. Amends
[0102](0102-endora-hq.md) (a project between a goal and its tasks),
[0107](0107-routines-alarms-and-check-backs.md) (a routine may name a project, and the CEO has
a weekly planning beside its daily review), [0109](0109-a-brain-per-member.md) (a project's cap
joins the limits on a cloud turn) and [0111](0111-the-ceo-and-the-chain-of-command.md) (the
chain of command follows each member's `reports_to`). Built: projects and their store; the
ledger's project column; caps, the 80% note and the budget pause; `reports_to`, its backfill and
the owner's changes; the project lead woken for work handed back; `propose_project`,
`decide_project` and `propose_goal`; the CEO's weekly planning; the start-up migration of the
home lab's work; the HQ screen's Projects card, desk items and "reports to". Amended
2026-10-17 (evening) by the owner, below under **Amendment: soft shares, the owner's own
requests, and claims by brain price**: a member's share is a first claim rather than a wall,
work the owner asked for is the pool's alone, and a member's claim is weighted by its brain's
price. Amended by [0118](0118-elastic-staffing-and-notebooks.md) (2026-10-17): a member on
furlough takes no share (it is left out of the claims the day's pool is divided by), and the
scorecard counts each team's seats, how many are at work and how many are on furlough.

## Context

Step 2 of HQ's roadmap. Goals were the only thing above tasks, and money was counted per member
and per day only. So the home lab's upkeep and a one-off piece of work drew on the same pool
with nothing to say what each was worth, the lead had no work of its own to own beyond "every
goal", and the CEO could see trouble repeating but had no way to say "this deserves its own
effort" except a decision for the board. The chain of command was fixed by role: the lead's
question went to the CEO, everyone else's to the earliest lead.

The design HQ borrows has projects under goals with their own lead and monthly budget, an
explicit reporting line per member, proposals that come up the line, and a recurring planning
pass. Endora takes the shape, and keeps its own rules: models propose and code decides, the
owner is the board, and a local member costs nothing.

## Decision

**Owner decisions, 2026-10-17.**

1. **The tree.** Goals stay at the top: the owner's are active at once; the CEO proposes others
   to the board. A **project** serves exactly one goal. A task belongs to a goal and, optionally,
   to a project of that goal (refused otherwise, `OtherGoal`); a sub-task goes in its parent's
   project whatever it was asked; a routine may name a project of its goal, and its tasks go in
   it. A project has a title, a reason, a lead (a lead or the CEO, `NotAProjectLead`), a status
   (`proposed`, `active`, `paused`, `done`, `dropped`), an optional monthly cap in dollars, who
   made it, its evidence, and when it was made and changed. Only an active or paused project
   takes a task (`ProjectClosed`).
2. **The live store.** HQ's schema has no migrations folder, and the live database exists, so
   every new column (`members.reports_to`, `tasks.project`, `routines.project`, `desk.project`,
   `ledger.project`) is added by an `ALTER TABLE` guarded by `pragma_table_info`, empty. Adding a
   column changes no row, so the ledger's append-only triggers stay. On start, once, if there is
   no project and a routine takes the home lab's alarms: an active project **"Home lab upkeep"**
   under that routine's goal, led by the earliest lead at work (or the CEO), with no cap; every
   routine of that goal but the CEO's own goes in it, and so does every open task of that goal
   in no project. Tested from a store made with the schema as it was.
3. **Money, by the calendar month in UTC.** Each new ledger row carries its turn's project: the
   reservation takes it from the turn's task, and the rows after it from the reservation, so a
   turn's rows are one project's (old rows stay empty). A project's month is its rows booked in
   the month its turn reserved, as days already are. For a cloud member's turn on a capped
   project, what is left of the cap joins the minimum `affordable()` takes (the member's day,
   the pool's day, the role's turn cap), so the turn reserves no more; the node's relay already
   holds each call to the turn's `left` (`reserved − spent`), so the cap reaches the relay with
   no change to the node. The caps together (of the proposed, active and paused projects: a
   paused one comes back with its cap) may not pass the pool's month (`CapsOverPool`, with the
   room left).
   - **At 80%** of its cap: one notice a month per project on the owner's desk ("Project #1
     Upkeep has spent 80% of its $10.00 cap this month ($8.10)."), a `project.budget` event, and
     a line in every CEO briefing while it lasts. Nothing is blocked.
   - **At 100%** (less than five cents left, about one call): the project is paused for its
     budget. Its Claude turns are refused (`out_of_budget`); members on the house's model go on.
     One desk item offers **raise the cap** (an amount above this month's spend, within the pool)
     or **wait for next month**. A budget pause lifts itself when the month turns (the tick looks);
     the owner's own pause does not, and holds every turn on the project, local ones too
     (`project_paused`).
4. **reports_to.** Each member reports to one other, or to nobody, which for the CEO is the
   board. Set at hire: the lead to the CEO; the developer, the researcher and the reviewer to the
   earliest lead, or the CEO when there is none; the CEO to nobody. Members hired before (or
   before anyone was there to report to) are backfilled on start, the same way. The owner may
   change it (`POST /v1/members/{id}/reports_to`, through the node and its audit log, and on each
   member's card); HQ refuses a member reporting to itself, into a circle (walked up with a visited
   set), or a manager for the CEO (`NotAManager`). A question up the chain goes to the manager,
   or past one who is paused, one step up at a time; a chain that ends goes to the CEO; the CEO's
   is a decision for the board, as before. With no CEO at work the team works as before there
   was one. Who may hand work to whom keeps the rank rule.
5. **Proposals, with evidence.** Three tools, every check in code:
   - `propose_project` (a lead or the CEO): the goal (default: the turn's task's), title,
     reason, lead (default: the proposer), an optional cap, and its evidence: task numbers and
     event ids from HQ's own record, at least one, each looked up (`NoEvidence`,
     `NoSuchEvidence("event 99999")`). A lead's waits for the CEO, as `proposed`. The CEO's starts
     at once, since the CEO decides projects.
   - `decide_project` (the CEO): accept or send back, with why, posted in the thread the proposal
     was made in and waking its proposer. Accepted, the project is active and its lead is handed
     a first task in it, to plan its work. Sent back, it is dropped (a lead may propose again).
   - A cap asked for, by the proposal or by the CEO when it decides, goes to the **owner's desk**
     with the CEO's why as its recommendation; the owner grants it as asked or at another amount,
     or declines. Until then the project has no cap of its own (the pool's and members' limits
     still hold).
   - `propose_goal` (the CEO): title, reason and evidence; a proposed goal on the owner's desk,
     the card saying why and on what evidence.
   The owner, who is the board, may overrule from the screen: pause, resume, drop (its open work
   is dropped with it, and its routines go on under the goal), reopen (or start a proposed one),
   or mark done once nothing in it is open.
6. **The project's lead.** A task handed back (`todo`) wakes its project's lead only, while at
   work; with no project, or the lead paused or the one handing it back, every lead, as before.
   The lead's briefing lists its projects with this month's spend against the cap.
7. **Planning.** A new CEO routine, **"Planning"**, every 7 days, made once on start when there
   is a CEO (a setting remembers it), apart from the daily review. Its briefing is for proposing
   from patterns, and lean (the CEO is on Opus): the CEO's opening lines, then, per goal, its
   projects with spend, cap and status; the last 30 days' tasks grouped by the home-lab
   containers they named (the matching one-task-a-problem uses), with counts and task numbers,
   and the alarms about the same containers ("deluge, nzbget, prowlarr: 3 tasks (#4, #9, #12),
   4 alarms"); the alarms that came back, with their event ids; the proposals waiting; and the
   goals with no project. No open-task list and no desk. The daily review stays as it was, and
   says how many proposals wait.
8. **The screen.** A Projects card, grouped by goal: title, lead, status (and which pause),
   spend against the cap on a bar (amber from 80%, red once spent). Forms for a new project
   (goal, title, lead, cap), the cap, handing a project work, pause, resume, drop, reopen. The
   desk shows the CEO's proposed goals (as before), cap requests with the CEO's recommendation,
   80% notices and budget pauses. Each member's card says whom it reports to. Everything a model
   wrote is escaped.

## Amendment: soft shares, the owner's own requests, and claims by brain price

Decided by the owner on 2026-10-17, in the evening, from that day's record: Mara, the CEO, spent
her share (about $0.82) while Ada and Cy had most of theirs unused, and every task waiting on her
blocked until midnight; and hiring two developers on Haiku cut the lead on Sonnet to $0.32 a day
while each of them got $0.52.

1. **A share is a first claim, not a wall.** Once a paid member's share for the day is spent,
   it draws on what the *other* paid members have not used today (each one's share less its
   spend; one past its own share lends nothing). The pool's day ($70 / 30) stays the hard
   ceiling. What a turn may reserve (`affordable()`) is the least of the per-turn cap, what is
   left of the pool's day, what is left of the project's cap, and the member's share left plus
   the others' unused (`money::may_draw`). A member on the house's model stays out of it, as
   before. (Borrowing is capped by rule 4, below, from 2026-10-18.)
2. **The owner's own requests are the pool's.** A turn that exists because the owner asked (a
   task they handed, their reply in a thread, their answer or decision on the desk, "Run now"
   on a routine) is marked `asked_by_owner` from the wake that started it (a wake raised while
   the engine does what the owner asked; a held wake keeps the mark, and a wake that folds into
   a turn not yet started marks it). Its ledger rows carry an `owner` flag, and the share
   arithmetic (`share_committed`) leaves them out: such a turn counts against the pool's day
   only, never against the member's share. It is never a way past the pool's ceiling or a
   project's cap.
3. **A claim is weighted by the brain's price.** A member's claim on the pool is its role's
   weight times its brain's price class (`Brain::price_class`, in thousandths of Opus by the
   relay's input prices: Opus 1000, Sonnet 500, Haiku 25; the house's model 0), and the day's
   pool is shared by claims. The relay's tests hold the classes to its prices, so a price
   change there fails a test. With the live team (Mara on Opus; Ada and Kit on Sonnet; Cy, Rae
   and Zed on Haiku) the lead on Sonnet now has more than a developer on Haiku. The roster shows
   each member's daily share.

### Borrowing is capped at one extra share, and a member at the cap asks the owner

Decided by the owner on 2026-10-18. Uncapped borrowing (rule 1 above) meant a member was out
for the day only once the pool's day was spent, so an "out for the day" desk item could never be
offered. Borrowing is now limited, and the item is built.

4. **A paid member's day reaches at most twice its share.** With `s` its share for the day,
   `u` its own spend today (the owner's asking left out, as in rule 2) and `o` the sum of the
   other paid members' unused shares today (each one's share less its spend, never below
   nothing; furloughed members and members on the house's model are not counted), the member
   may spend while `u < s + min(s, o)`, with the allowance below added to the right-hand side.
   What a turn may reserve is the least of the per-turn cap, the pool's day left, the project's
   cap left, and `s + min(s, o) - u + allowance` (`money::may_draw`, `Engine::affordable`).
   `o` is read at the moment of the turn, so the limit moves as the others spend. Owner-asked
   turns stay outside it, charged to the pool's day alone; members on the house's model stay out
   of it. The pool's day is the hard ceiling for everyone.
5. **"Out for today".** The first time a paid member is turned away because it reached that
   limit while the pool's day (and its project's month) still has at least a turn's money, one
   desk item (`out_for_day`) is put for that member for the owner's day, and no more that day:
   "Mara is out for today: $1.64 spent (share $0.82, borrowed $0.82); the pool has $X left
   today. Give Mara $0.50 more today, or wait for tomorrow." Approving gives the member an
   allowance for that day: the amount written on the card, or $0.50 (or what the pool's day has
   left, if that is less) when nothing is written; it must be at least a turn's money and at
   most what the pool's day has left, or it is refused. The allowance is a setting keyed by
   member and owner's day (`allowance:<member>:<day>`), so it is gone when the owner's next
   day begins (the day follows the owner's time zone); declining changes nothing. A member
   furloughed or with no share is never offered. Nothing is woken on approval: the member's
   next wake, or a turn already queued for it, starts under the allowance.

6. **The owner's one-day top-up.** Decided by the owner on 2026-10-18: when the pool's day is
   short and the money is there, the owner adds to it for today. `POST /v1/pool/top-up
   {amount_micros}` on HQ (the node's `/v1/hq/pool/top-up`, signed and audited; only the node's
   token reaches it) stores the setting `pool_topup:<owner-day>`, summed if the owner tops up
   twice, so it is gone when the owner's next day begins, like an allowance. The amount is
   above nothing, and a day's top-ups together are at most three times the base pool day
   (`money::most_top_up`); beyond that HQ refuses and says what room is left
   (`Refusal::TopUpOutOfRange`). HQ records a `pool.topped_up` event (`amount`, `total`,
   `by: "owner"`, `day`); the ledger is untouched. Today's pool is the base day plus the
   top-up, and that is the pool wherever the pool's day is used: the hard ceiling in
   `affordable()`, what is left for an allowance (rule 5), downtime's room and the scorecard.
   **Shares scale with it.** A member's share is its claim's part of today's pool
   (`money::share_of_day`), so every share grows by the factor (base + top-up) / base, and the
   borrowing cap of rule 4, which is one more share and is built from the shares (its own and
   the others' unused), grows by the same factor: a member whose day reached twice a share of
   $0.94 reaches twice $1.88 with a one-pool-day top-up. A member out for the day (rule 5)
   can therefore take a turn at once, with no allowance. Spend already booked stays booked.
   The scorecard's today row says "pool $2.33 + $2.33 top-up" and has a "Top up today"
   button (a prompt prefilled with the base pool day, then a confirmation); an "is out for
   today" card offers "Top up the pool first" when the pool has nothing left.

Scenarios: "A member past its share borrows what the others left", "The owner's own request
is the pool's, not the member's" and "A member may borrow one more share, then asks the owner"
(`features/projects-and-their-money.feature`).

## Amendment (2026-10-20): the tools a project's work is given

The Holodeck plan's phase 4. A project may carry a list of the tools its work is given, set by
the owner (`POST /v1/projects/{id}/tools` with `{"tools": [...]}`, through the node's
`/v1/hq/projects/{id}/tools`; `null` takes the list away). With no list, nothing changes: a
turn has what its role and brain give it. With one, a turn on the project's work, its
sub-tasks' too, is given its own four (`my_turn`, `claim`, `post`, `set_status`) and the list,
and nothing else, in the three places the check after a deploy is narrowed (ADR 0115, amended):

- HQ offers only those of its tools, and refuses any other, whatever the model was told.
- HQ tells the node the list (`only` on `/v1/team/turn`); the node offers and runs only the
  contracts and bridged tools named in it, and keeps a refusal as a receipt that failed.
- The runner gives the workbench's shell only when the list names `shell`.

An empty list leaves a turn its own four and nothing of the node's: the shape of a scenario's
project, which reads and acts only through its own tool. The list is kept as a setting
(`project_tools:<id>`), so nothing in the store's tables changes. Not built yet: a field for it
on the owner's screen.

## Consequences

- Money has a second axis: what a piece of work may cost in a month, beside what a member may
  spend in a day. A project the owner caps cannot quietly eat the pool; one it does not cap
  costs what it did before.
- The CEO's planning reads 30 days of HQ's events once a week. HQ prunes its feed at 20,000
  events, not counting its ticks (amended 2026-10-19, [0124](0124-guards-against-how-agent-teams-fail.md)),
  so a very busy month may show less than a month; the counts are a pattern, not an account.
- A task's project is fixed when it is made (and by the migration); moving a task between
  projects is not built.
- **Deferred:** hiring that acts (a proposal to hire is still a decision for the board, made by
  the owner); per-project effort or brain; moving tasks between projects; the
  owner setting the pool itself on screen.

## Alternatives considered

- **Caps checked by the node's relay.** Rejected: the relay already holds each call to the
  turn's `left`, which HQ computes; putting the project in `affordable()` needs no node change.
- **A rolling 30-day budget.** Rejected for the calendar month: the owner's credits are monthly,
  and "wait for next month" has to mean a day the owner can name.
- **Every routine of the home lab's goal into the upkeep project, the CEO's review too.**
  Rejected: the daily review looks over the whole house, and its turns are not upkeep's spend.
- **Projects the CEO accepts with no first task.** Rejected: an accepted project with nothing in
  it waits for the owner or a routine to hand it work, and the lead never hears of it.
