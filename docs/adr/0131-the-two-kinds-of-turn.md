# 0131 — The two kinds of turn: the house's models work, Claude judges

## Status

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Builds on
[0109](0109-a-brain-per-member.md) (a brain per member), [0111](0111-the-ceo-and-the-chain-of-command.md)
(the CEO, the chain of command, and who may think with what) and
[0130](0130-the-macs-brain.md) (the Mac's brain and the queued turn that waits for it), and on
the constitution's "models propose, code authorizes". Amends 0109, 0119 and 0130 (see their
notes). Built: everything below, and the owner's scenarios in
`domains/team/features/the-two-brains.feature`.

Amended 2026-10-19, after review, in five places, each marked below: every member has a Claude to
judge with (Haiku until the owner says otherwise), so a judgment turn never waits for a brain it
was never given; there is no switch, so the service runs the rules the tests run; the
engineering wing's lead needs the Mac for its work like its developers and testers; an alarm
acts at once when the money is gone; and a turn that waits is a wake, not a stall.

Amended again 2026-10-19, from a live failure: **what the bridge offers a turn depends on where
it thinks.** A turn on the house's own model on the NAS is kept from the tools that can end on
the owner's desk, and keeps the ones the owner trusts to run without asking (*What the bridge
offers a turn*, below). The first build kept every bridged tool from it; the evidence supports
less than that, and the amendment is narrowed to it. Scenarios:
`domains/capabilities/features/tools-that-fit-the-turn.feature`.

## Context

Until now a member had **one** brain, and every turn it took ran on it: a member on Sonnet did
its reading, its building and its deciding with Sonnet, and paid for all of it; a member on the
house's model or the Mac decided with a model too small to be trusted with decisions. Four
things, studied before this record, point the same way:

- **The oversight doctrine we studied:** cheap models read and do; the expensive one decides on
  their summaries. Judgment is the scarce thing, so it is what the money is for.
- **Our failure-mode study:** a small local model as developer *and* reviewer makes a send-back
  carousel the likeliest failure. It was seen live: the NAS model invented a web map search that
  had nothing to do with the task. Code and review work must not fall to the smallest model.
- **The borrowed queue design:** a budget blocks work at enqueue and never falls back to another
  model. Ours already did the second half for the Mac (0130); for money it did the first half
  wrongly: a wake refused for budget (`Refused::OutOfBudget`) was thrown away, and whatever it
  asked for was lost.
- **The owner's decision (0130):** local models do the work; paid Claude judges.

## Decision

### Two brains per member

A member has a **work brain**, the Mac or the house's own model (house-only; Claude never does
the work), and a **judgment brain**, a Claude its role may use (0111's table: Opus the CEO's
alone). *Amended:* **every member has both.** A new member's work brain is the Mac, and its
judgment brain is **Haiku**, the owner's cheap Claude; the CEO, whom HQ seats on Opus, has
Sonnet if it is ever given one by default, since Haiku may not be the CEO's. There is no taking
a judgment brain away, only changing it, so a turn of judgment never waits for a brain that does
not exist; it waits only for money. The old `brain` stays what it was named for: the brain the pool's shares, the data tier
and the roster reckon on, and it is the judgment brain. A turn's own brain is kept on the turn
(below), and a turn on a house model is read as one on a house model whoever the member is.

The roster (HQ, Team tab) shows and sets both: *Work: Mac · Judgment: Sonnet*. The owner's API
is `POST /v1/members/{id}/work-brain` and `/judgment-brain` (through the node, as `brain` is); the
old `/brain` puts the one brain it is given into the slot it belongs to (the house's model or the
Mac into the work slot, a Claude into the judgment slot) and leaves the other. A hire names either
or both (`work_brain`, `judgment_brain`, or the old `brain`), and the other has its default: an
owner who hires "on the house's model", as owners always have, gets a member that works there and
judges with Haiku.

### The kind of a turn is decided by code, never by the member

`domain/turns.rs` holds it as pure functions over the reason a member was woken
(`WakeReason`), its role, and two facts the engine reads (the turn's task is one of a lead's
standing reviews; the member sits in the engineering wing). Nothing the model says is an input.
A new wake reason does not compile until it is classified, and the table test in that file
fails until its row is added.

| Wake reason | Kind | Floor for its work |
|---|---|---|
| `assigned`, `mentioned`, `wait_finished`, `subtasks_finished`, `retry`, `recovery`, `assignment_recovery`, `check_back`, `owner_replied`, `question_answered`, `wait_dropped` | work | the Mac if free, else the NAS (a wing developer's, the wing lead's and a tester's: the Mac) |
| `sent_back`, `check_done` | work | **the Mac** (mending what a reviewer sent back; reading a branch's checks) |
| `downtime`, `consulted` | work | **the NAS only**: not quality work, and the Mac is scarce (0130) |
| `proposal_decided`, `lead_review`, `alarm`, `review_requested`, `question_asked`, `acceptance_asked`, `directed`, `decide`, `huddle`, `guard` | **judgment** | — |

On top of the table: **every turn of the CEO is judgment** except the office's and a consult
(it is woken only for escalations, the board's word and its own plans); a lead's standing
reviews (its daily report, its weekly patterns), which are handed over as ordinary tasks and so
woken as `assigned`, are judgment whatever woke them; and a turn several wakes asked for (they
fold into one) needs the strictest of them: one wake of judgment makes it a turn of judgment.

*Amended:* **the engineering wing's lead needs the Mac for any work turn**, as its developers
and the testers do. Planning code on the NAS's 7B model is the carousel risk the failure study
found; a lead outside the wing, who reads the lab and writes status, may still use the NAS's
model when the Mac is busy. The floor is `(Developer | Lead, in the wing) | (Reviewer, anywhere)`,
and the table test and a scenario hold it.

### Where a turn runs

- **Judgment runs only on the member's Claude brain.** No budget: the turn waits (*waiting for
  budget*). It is never run on the Mac or the NAS, however free either is, with one exception.
- *Amended:* **An alarm acts at once.** An alarm is the one kind of judgment that may not wait
  for money, the way an emergency change acts first and is reviewed after: with the member's, the
  pool's or its project's budget spent, the alarm turn **runs at once on the member's best free work brain**
  (the Mac if the member works with it and it is free, otherwise the NAS's model), reserving
  nothing, and a notice on the owner's desk says an alarm was handled on a house model for lack
  of budget (once for that turn). What the turn may do is still gated by the same contracts, in
  code, whoever thinks it: the model was never the enforcement boundary. With the money, an
  alarm is judged on Claude like any decision. An alarm folded into a decision does not carry the
  decision with it: only a turn whose every wake of judgment is an alarm is an emergency.
- **Work runs on the best free work brain it is allowed on.** The Mac when the member works
  with it and it is free (0130's rule: plugged in, not busy, owner idle, one turn at a time);
  otherwise the NAS's model, **unless the work needs the Mac**: code and branch work (a wing
  developer, the wing's lead, anything using the repo's tools; the work after a review) and a
  tester's work require at least the Mac, wait for it with the reason 0130 gives (*waiting for
  the Mac: owner active*), and are never run on the NAS. A member whose work brain is the house's
  model cannot do such work at all: its turn says *this work needs the Mac, and the member's
  work brain is the house's model*, and the owner sees why on the roster.
- **Work costs nothing**, whoever the member is: a member who judges with Claude is not charged
  for the reading it does on the house's models. The pool is shared among every member by role
  weight and the price class of the Claude each judges with; members' shares and a project's cap
  still bound what judgment spends.

The brain a turn was started on is kept on the turn (`runs.brain`) and is what everything
downstream reads: the runner is told it, the node's relay is told it when it asks whose turn a
token is (so a work turn on the Mac reaches the Mac, never Anthropic), the data tier follows it
(a turn on a house model may read what a turn on Claude may not, and what it said is not shown
to a turn on Claude), and a Claude Code runtime (0117) applies only to a turn that runs on
Claude: a developer may be set to it by default now that every member judges with Claude, and it
runs the turns of judgment, while the same developer's work turns stay on the worker.

### Nothing is dropped

A wake that would have been refused for budget is **queued like any other**, and the turn waits
at the one place turns are started (`start_queued`, which 0130 already made the gate for the
Mac). The gate re-asks every tick, so a waiting turn is released by whatever frees the money,
with no second mechanism: the pool's day turning over, the owner topping up the pool, the owner
giving that member an allowance on its *out for the day* card (asked once a day), a project's
cap raised. A wake for a member who already has a live turn on the task folds into it as before,
so a stuck day does not pile duplicates: one waiting turn per member and task, which any later
wake only strengthens (a wake of judgment folds into a waiting work turn and makes it judgment).
A wake of judgment does *not* join a work turn that is already running on the house's models: it
is held and released when that turn ends (about no task it is a turn of its own). Waiting turns
are rows in HQ's store and survive a restart.

*Amended:* a routine is no longer skipped for budget (its turn would only wait), and a routine
that an alarm makes a task for wakes its member for an `alarm`, not only for the task it was
handed, so the alarm that makes a task is as much an emergency as one passed into an open task.

### A turn that waits is a wake, not a stall

*Amended:* a turn that waits for budget or for the Mac **counts as a live wake in the stuck
diagnosis**. In `domain/guards.rs` the held-health rule reads a fact, `waiting_turn`: a queued
turn on the task whose words are a wait's, and work whose only turn waits is diagnosed **held**,
never parked. The tick's steps 5 and 5b already skip a task with a live turn, and a waiting turn
is a queued run; the stuck-finding's own closing reads it as live too. So a waiting turn never
produces a stuck finding, a nudge, a ladder climb to the lead or the CEO, a huddle or an item on
the owner's desk, however long it waits. Held work is not hidden: the roster says what each
member waits for and the top bar counts the turns waiting for budget. A scenario holds both
cases (budget; the Mac) and that the CEO is never woken for them.

### What the bridge offers a turn

*Amended 2026-10-19, and narrowed the same day on review.* The bridge
([0108](0108-the-bridge.md)) offers a turn Endora's capabilities and the tools of the tool
servers the owner installed, as its role and the data tier allow. It is also the place a model
has to choose well among many outside tools, and the failure study named a small local model as
developer the likeliest way to a carousel. Live (the Context above has it), a developer on the
NAS's model asked the owner to approve `brave-search__brave_place_search` with coordinates it had
made up, "to find the NAS backup location": a tool server's tool asks by default, so **the
request reached the owner's desk as an approval**. A trusted read would not have: it runs without
asking, and the audit log is where it is recorded.

What a turn is offered depends on **where it thinks**, which the node reads from what HQ says of
the turn when it asks whose a token is (`cloud`, and `brain`, the turn's own, above):
`capabilities::bridge::Thinks`.

| Where the turn thinks | The bridge |
|---|---|
| The house's own model on the NAS (`local`, or a turn HQ says nothing of) | **As its role and the data tier allow, less whatever can end on the owner's desk.** A tool that asks the owner is not listed, and is refused (403) if named, with the way on: say what is needed in the task's thread, so the lead can hand the step to a member on the Mac. Kept, so the Ops lead's lab checks stand: every read (the download queues, the weather, a tool server's reader the owner named), and an act the owner lets run on its own that takes no arguments |
| The owner's Mac (`mac`) | As its role allows (0108's profile), and the data tier as for any local model |
| A Claude | As its role and the data tier allow, as ever (a tool server's tools are Confidential and stay with local turns) |

**What can end on the owner's desk** is the policy's own answer (`bridge::way`, asked of the tool
as `Bridged::can_end_on_the_owners_desk`), and not a list of names:

- an act whose stance is **ask**: a built-in outward act the owner left asking, and every tool of
  a tool server the owner has said nothing about, which starts out asking (0108). A web or search
  tool of such a server is one, and is kept from the NAS's model. The owner reviews a server's
  tool by naming it the server's reader, which then reads, and a read does not ask;
- an act that runs on its own but **takes arguments**, because it asks once the turn has read a
  stranger's words (0108), and a read of the queues is one. It is kept as well, so that no turn on
  the NAS's model can reach the owner's desk through the bridge at all;
- never a read, whatever stance it starts as, and never an act that runs on its own and takes
  nothing (`sonarr_retry_import`): a stranger cannot steer what it is not given.

Not the bridge: the contracts of [0106](0106-team-contracts.md) (`homelab_status`, the logs,
`restart_container`, the map, the docs, the wing's git), which a turn on the NAS's model has in
full. The node knows a turn's brain and not its kind. A work turn is on the NAS's model when the
Mac is busy and the work does not need it; a judgment turn is there only for an alarm out of
budget (above), and that turn has the contracts and the trusted reads as the rest do. Unknown is
the NAS's: what is not known to be a stronger model is kept from what can end on the desk.

### What the owner sees

The roster line says both brains and, for a member whose turn waits, what it waits for (the same
words as the Mac's); the replay shows it as it shows the Mac's; the HQ top bar says **N turns
wait for budget** when N is above zero (`snapshot.pool.waiting`); the run in the snapshot carries
the brain it ran on; an alarm run on a house model for want of budget leaves a notice on the desk.
The hire form takes *works with* and *judges with*; there is no *None*.

### One path, tested

*Amended:* the first build put these rules behind `Settings.turn_kinds`, on in the service and
off in the engine's older tests, so a bug in the new path would have lived at the seam between
what is tested and what runs. **The switch is gone.** The rules are always on, except that the
scripted demo, which has no node to say whether the Mac is free, has the Mac told free on every
tick. The older tests were not given the switch, one by one, or weakened: the shared fixtures
changed so that the members they hire have the two brains every member has (a default judgment
brain of Haiku and a work brain of the Mac, the Mac told free where a test starts a turn that
needs it, the pool's money asked for with a wake of judgment where a test is about money), and
the few that asserted a one-brain rule that no longer exists were rewritten to the new rule.

### The migration

Idempotent, with no data lost: two nullable columns are added if missing (`members.work_brain`,
`runs.brain`), and every member is made to have two brains **once**:

- a member whose brain is `local` or `mac` keeps it as its work brain; a member on Claude gets
  the **Mac** to work with;
- *Amended:* a member with no Claude to judge with (the house's model in its `brain`, or a name
  it does not know) is given **Haiku**, or **Sonnet** for the CEO, who may not think with Haiku.
  It is not left to wait for a brain the owner has not given it. A member on a Claude keeps it.

A start that finds the columns and the brains changes nothing; an owner's later choice is never
overwritten. A member hired since is created with both, by the same defaults. Turns already on
record have no brain and read as their member's.

## Consequences

- Cost falls to judgment: reading, building, checking and drafting no longer draw on the pool,
  and Haiku, the default, is cheap; what a member's share and its project's cap allow still
  bounds the spend, and the owner raises either on the cards that ask.
- Every member can decide, so no team stops deciding for want of a Claude. The owner's step is
  to choose a dearer one for a member whose judgment matters most.
- Code and review work, and the wing lead's planning, wait for the Mac, which on a working day
  may be long. That is the choice (0130): quality work from the stronger model, not fast work
  from the weaker one. It is held work and not stuck work (above).
- A turn's kind is a property of why it was asked for, not of what it does. A work turn that
  finds it needs a decision should say so in the thread, and the team's wakes bring the decision
  its Claude turn. Where a reason is sometimes one and sometimes the other the table errs towards
  judgment (the CEO) or towards the Mac (a tester).
- An alarm may be thought by the house's model for want of money. That is a risk taken on
  purpose, for the emergency class only; the contracts, not the model, say what it may do, and
  the owner is told.
- The owner's allowance for **paid downtime** (0119) is gone: a downtime turn is work on the
  NAS's model, free for every member whatever it judges with. Its setting, its route
  (`/v1/office/downtime`, the node's `/v1/hq/office/downtime`) and its card line are removed. A
  consult is work on the NAS too, free whoever asks and whoever is asked, and the nightly quiz
  is open to every free member.
- `Refused::OutOfBudget` is no longer raised, and stays readable in the record.
- A turn on the NAS's model keeps the lab's contracts and the reads the owner trusts, and is
  kept from what can end on the owner's desk (amended 2026-10-19). A lead whose turn lands on the
  NAS's model while the Mac is busy still reads the download queues, which the first build kept
  from it: the cost named there is gone. What it cannot do on the NAS's model is what a tool
  server's unreviewed tool, or an outward act, would have asked the owner for; that step waits for
  a member on the Mac, said in the thread.

## Alternatives considered

- **Letting the member's model say whether its turn needs judgment.** Rejected: the model is not
  the enforcement boundary, and a small model would call everything work.
- **Falling back to the house's model when Claude is out of budget.** Rejected by the owner's
  decisions of 0130 and the failure study: a decision made by the model that must not make it is
  worse than a decision that waits. The alarm alone is excepted, because an alarm that waits for
  tomorrow's money is an outage nobody is looking at.
- **A member with no judgment brain, whose decisions wait for the owner to give it one.** The
  first build did this. Rejected on review: it parks a team on a setting nobody knew to touch.
  Haiku is cheap and bounded by the shares and caps.
- **A new queue for waiting turns.** Rejected: `start_queued` already gates Mac turns and
  re-asks each tick; a second queue would need its own release.
- **A switch, off in the older tests.** The first build had one. Rejected on review: the path
  that runs must be the path that is tested.
- **Offering the NAS's model no bridged tool at all** (amended 2026-10-19; the first build did
  this). Rejected on review: the live failure was a request that reached the owner's desk, not a
  trusted read, and keeping every tool from the model cost the Ops lead the download queues its
  lab checks need, when the Mac was busy, for no gain. What is kept from it is what can end on the
  desk.
- **Keeping from the NAS's model by name, or by what a tool does ("a web or search tool").**
  Rejected: the node cannot tell what a server's tool does, and a server's account of it is not
  evidence (0054). The policy already knows which tools ask, and a server the owner has not
  reviewed is exactly one whose tools do.
- **Keeping from it only the stance "ask".** Rejected in part: an act that runs on its own but
  takes arguments asks once a stranger's words have been read, which the queues' results are, so
  stance alone would leave a way to the desk. The question asked is the one that matters: can a
  call end there.
- **Leaving the owner's desk to catch a request a small model invented** (amended 2026-10-19).
  Rejected: that is what happened, and the owner's attention is what a desk spends. The other
  half of the answer is [0124](0124-guards-against-how-agent-teams-fail.md)'s: nothing reaches
  the desk that serves none of its task's criteria, whoever asks.
