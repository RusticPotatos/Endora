# 0131 — The two kinds of turn: the house's models work, Claude judges

## Status

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Builds on
[0109](0109-a-brain-per-member.md) (a brain per member), [0111](0111-the-ceo-and-the-chain-of-command.md)
(the CEO, the chain of command, and who may think with what) and
[0130](0130-the-macs-brain.md) (the Mac's brain and the queued turn that waits for it), and on
the constitution's "models propose, code authorizes". Amends 0109 and 0130 (see their notes).
Built: everything below, and the owner's scenarios in
`domains/team/features/the-two-brains.feature`.

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
the work), and may have a **judgment brain**, a Claude its role may use (0111's table: Opus the
CEO's alone). The old `brain` stays what it was named for: the brain the pool's shares, the data
tier and the roster reckon on. It is the judgment brain when the member has a Claude, and its
work brain otherwise. The roster (HQ, Team tab) shows and sets both: *Work: Mac · Judgment:
Sonnet*, with *None* offered for the judgment brain. The owner's API is
`POST /v1/members/{id}/work-brain` and `/judgment-brain` (through the node, as `brain` is); the
old `/brain` still sets the old field, with the old meaning.

A member who has no judgment brain is a member whose **judgment turns wait**, and say so.

### The kind of a turn is decided by code, never by the member

`domain/turns.rs` holds it as pure functions over the reason a member was woken
(`WakeReason`), its role, and two facts the engine reads (the turn's task is one of a lead's
standing reviews; the member sits in the engineering wing). Nothing the model says is an input.
A new wake reason does not compile until it is classified, and the table test in that file
fails until its row is added.

| Wake reason | Kind | Floor for its work |
|---|---|---|
| `assigned`, `mentioned`, `wait_finished`, `subtasks_finished`, `retry`, `recovery`, `assignment_recovery`, `check_back`, `owner_replied`, `question_answered`, `wait_dropped` | work | the Mac if free, else the NAS (a wing developer's and a tester's: the Mac) |
| `sent_back`, `check_done` | work | **the Mac** (mending what a reviewer sent back; reading a branch's checks) |
| `downtime`, `consulted` | work | **the NAS only**: not quality work, and the Mac is scarce (0130) |
| `proposal_decided`, `lead_review`, `alarm`, `review_requested`, `question_asked`, `acceptance_asked`, `directed`, `decide`, `huddle`, `guard` | **judgment** | — |

On top of the table: **every turn of the CEO is judgment** except the office's and a consult
(it is woken only for escalations, the board's word and its own plans); a lead's standing
reviews (its daily report, its weekly patterns), which are handed over as ordinary tasks and so
woken as `assigned`, are judgment whatever woke them; and a turn several wakes asked for (they
fold into one) needs the strictest of them: one wake of judgment makes it a turn of judgment.

### Where a turn runs

- **Judgment runs only on the member's Claude brain.** No judgment brain: the turn waits
  (*waiting for a judgment brain*). No budget: the turn waits (*waiting for budget*). It is
  never run on the Mac or the NAS, however free either is.
- **Work runs on the best free work brain it is allowed on.** The Mac when the member works
  with it and it is free (0130's rule: plugged in, not busy, owner idle, one turn at a time);
  otherwise the NAS's model, **unless the work needs the Mac**: code and branch work (a wing
  developer, anything using the repo's tools; the work after a review) and a tester's work
  require at least the Mac, wait for it with the reason 0130 gives (*waiting for the Mac: owner
  active*), and are never run on the NAS. A member whose work brain is the house's model cannot
  do such work at all: its turn says *this work needs the Mac, and the member's work brain is
  the house's model*, and the owner sees why on the roster.
- **Work costs nothing**, whoever the member is: a member who judges with Claude is not charged
  for the reading it does on the house's models.

The brain a turn was started on is kept on the turn (`runs.brain`) and is what everything
downstream reads: the runner is told it, the node's relay is told it when it asks whose turn a
token is (so a work turn on the Mac reaches the Mac, never Anthropic), the data tier follows it,
and a Claude Code runtime (0117) applies only to a turn that runs on Claude.

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

### What the owner sees

The roster line says both brains and, for a member whose turn waits, what it waits for (the same
words as the Mac's); the replay shows it as it shows the Mac's; the HQ top bar says **N turns
wait for budget** when N is above zero (`snapshot.pool.waiting`); the run in the snapshot carries
the brain it ran on.

### A switch, off in the older tests

`Settings.turn_kinds` (as `downtime`, `games` and `quiz` before it): the service turns it on
unless it is the scripted demo, and the engine's older tests, whose members think with one brain
and run every turn on it, run without it and keep their meaning. With it off a member's one
brain runs everything and a wake out of budget is refused, as before.

### The migration

Idempotent, with no data lost: two nullable columns are added if missing (`members.work_brain`,
`runs.brain`), and every member without a work brain is given one **once**:

- a member whose brain is `local` or `mac` keeps it as its work brain and has **no judgment
  brain** (its judgment turns wait for the owner to give it one);
- a member on Claude keeps it as its judgment brain and gets the **Mac** as its work brain.

A start that finds the columns changes nothing; an owner's later choice is never overwritten.
A member read with no work brain (hired since) is read by the same rule. Turns already on record
have no brain and read as their member's.

## Consequences

- Cost falls to judgment: reading, building, checking and drafting no longer draw on the pool.
  Existing house-only members lose their decisions until the owner gives them a Claude, which is
  the point, and is said on their roster line.
- A team of members on the house's model only (no Claude anywhere) stops deciding: leads do not
  review, the ladder does not climb, the tester does not give a verdict. The owner's step is one
  tap per member on the roster.
- Code and review work wait for the Mac, which on a working day may be long. That is the choice
  (0130): quality work from the stronger model, not fast work from the weaker one.
- A turn's kind is a property of why it was asked for, not of what it does. A work turn that
  finds it needs a decision should say so in the thread, and the team's wakes bring the decision
  its Claude turn. Where a reason is sometimes one and sometimes the other the table errs towards
  judgment (the CEO) or towards the Mac (a tester).
- The owner's allowance for **paid downtime** (0119) no longer draws money: a downtime turn runs
  on the NAS and is free; the knob and its card are left as they are, to be tidied once the
  owner has seen this work.
- `Refused::OutOfBudget` is no longer raised with kinds on, and stays readable in the record.

## Alternatives considered

- **Letting the member's model say whether its turn needs judgment.** Rejected: the model is not
  the enforcement boundary, and a small model would call everything work.
- **Falling back to the house's model when Claude is out of budget.** Rejected by the owner's
  decisions of 0130 and the failure study: a decision made by the model that must not make it is
  worse than a decision that waits.
- **A new queue for waiting turns.** Rejected: `start_queued` already gates Mac turns and
  re-asks each tick; a second queue would need its own release.
- **Dropping the switch and rewriting the older tests.** Rejected for now: the older tests
  describe one-brain members; the service runs the new rules, and the scenarios and tests of
  this record run with them on.
