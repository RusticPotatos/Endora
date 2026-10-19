# 0133 — Caps on talk and runaway

## Status

Proposed, 2026-10-19. Prepared with AI assistance (Claude), from the owner's review of HQ's record
of 17 and 18 October and the brief that followed it. Amends
[0124](0124-guards-against-how-agent-teams-fail.md): the budget per task it deferred "only once
the measure shows it is needed" is built, and tried in shadow first; and the tripwires that
[the plan](../plans/endora-hq.md) promised (section 11) are built for the one failure that had
none, a team running away. Builds on [0114](0114-evidence-acceptance-criteria-and-the-tester.md)
(criteria, and a pass bound to them), [0131](0131-the-two-kinds-of-turn.md) (turns that wait, and
the house's own models) and [0132](0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md)
(one finding for a guard and what it is about, and the desk that interrupts only for decisions).
Built: everything below, and the owner's scenarios in
`domains/team/features/caps-on-talk-and-runaway.feature` and `domains/team/features/guards.feature`.

## Context

HQ's record of 17 and 18 October was read for what the team does when nothing stops it. Four
things came out of it, and a fifth was found on the way:

- **One docs branch took 44 turns, 80 posts and $2.47: 28% of all the spend.** The ceremony
  guard (0124) fired, and stopped nothing: it noticed. It had also seen less than it seemed to.
  It counted only work inside a project, and the owner's work and a routine's belong to none, so
  most of what the team does was outside its sight; and its finding said how many turns, not
  what they cost.
- **The measure 0124 waited for exists.** It deferred a ceremony budget per task that refuses
  until the measure showed it was needed. Four of four ceremony findings have been judged real or
  resolved by the owner.
- **The house's own models have no cap at all.** A turn on the node's model or the owner's Mac
  costs nothing, so no budget stops a member that has run away on them. The scripted demo ran
  1,539 turns in 3 h 40 min.
- **Only the owner can stop the team.** The plan's tripwires, which pull the kill switch
  automatically, were never built: the one caller of the kill switch is the owner's route. A team
  that loops stays running until a person sees it.
- **The ceremony finding tells a lead to "narrow its criteria", and no tool did it.** The engine
  could rewrite a task's criteria (the owner's rewrite, 0124), and nothing a lead could call
  reached it.

A guard that stops things is new authority, and a new authority should be tried before it is
given. The designs we borrow from run a new verdict source in shadow, recording what it would
have done, before it gates anything. The first number a new guard has is a guess; the shadow
shows whether it would have been right, and the owner's verdicts on its findings are how it is
tuned.

## Decision

### The ceremony guard sees all work, and says what it cost

Work in a project is looked at by project, as before. **Work outside a project is looked at by
its top-level task tree**: the top task and everything under it, named on the Guards card by its
top task, and shown there only while it is over the line. A lead's standing reviews (its report
and patterns turns) are not talk about work, there as elsewhere.

The tree, and not the goal, because the finding wakes the lead of the area of the tree's busiest
task, and the tree is the unit that lead can narrow, split or drop; it is already the piece of
work the engine treats as one. A goal pools unrelated work, the owner's tasks and a routine's
under one heading, so a tree that spins hides behind the healthy work around it, and the lead
woken would be asked about work it does not run. Where nobody leads the area there is nobody to
wake, and the finding is still raised for the owner's digest.

**Every ceremony finding states what the window's turns spent**, from the record of what each
turn used: "14 coordination turns, 1 product change in 24 h ($1.23 spent)". The Guards card says
the spend beside each window.

### A coordination budget per task, in shadow first

| Number | Value | Why |
|---|---|---|
| Budget | more than **20** coordination turns on one task in **24 h**, with nothing changing (the 21st is the first over) | The ceremony notices a project at 12 turns and 4:1; a budget that refuses belongs to one task, and 20 sits above that line and well under the 44 the docs branch took. A first guess. |
| Window | 24 h | The ceremony's own day. |

`THRESHOLDS.coordination_budget` and `coordination_window_ms`, beside the ceremony's numbers; the
verdicts on the guard's findings tune them.

**What counts.** The ceremony guard's own labels (`guards::turn_kind`): a turn that coordinated
(talk only, checking, asking) and not one that changed the product. The count is of the task's
turns that ended inside the window **after the task's newest product change and after its lead's
last decision about it**, whichever is later. The cut is by the order the turns ran, not by the
clock, so a coarse or frozen clock cannot put a turn on the wrong side.

**Shadow.** A new guard starts in shadow (`GuardMode`, kept as the setting `guard_mode:<guard>`;
only a guard that changes what the team does has a mode, the others only record). In shadow a
task over its budget makes a finding, once for the guard and the task (0132): *"would have held
coordination on #N (budget 20, 27 turns, $X)"*. It is on the Guards card and in the owner's
digest, takes verdicts like any other, and **changes nothing**: no wake is held and nobody is
asked. The owner switches the guard **on**, or back to shadow, from a small toggle on the Guards
card (`POST /v1/hq/guards/{guard}/mode` through the node, audited; `POST /v1/guards/{guard}/mode`
in HQ), and it is told in the feed (`guard.mode`).

**On.** The wakes that only ask members to talk are **held** when their task is over its
budget: a mention, a review or an acceptance asked for, a question up the chain and its answer
(`WakeReason::is_talk`, an exhaustive match, so a new reason does not compile until it is
classified). They are held with the receipt a wake behind a live turn already has, not dropped:
a second from the same member is the first's duplicate; a member already on the task takes it
into its turn as ever; and the tick asks held wakes again once no turn is live on the task, so
they go when the day has passed, when the lead has decided, or when the guard goes back to
shadow. **Never held**: the owner's word, a decision, a check-back, work handed on, a send-back,
an alarm, a guard's look. The ways out of a spinning task always reach it.

**One decision, to the lead.** The first held wake puts one question to the lead of the task's
area, once while it stands: a post in the thread, naming the three ways out, and the lead woken
(a Guard wake). The ways out are *narrow it* (`narrow_criteria`), *split it* (`create_task`) or
*drop it* (`decide_stuck`, on evidence, as for stuck work). Narrowing, splitting or deciding it
renews the count, so the talk before is not counted again. Where nobody leads the area nothing
is held, there being nobody to decide, and the finding says so.

### The lead's tool: `narrow_criteria {task, criteria}`

The lead of a task's area may narrow or reword what done means for it, in its own turn, and only
the lead (the engine checks the area's lead, not the list of tools offered; the tool is offered
to leads alone). Open tasks only; at least one criterion; **no more than the task had** (a task
with none may be given some), and no more than any task may have. It is posted in the task's
thread ("narrowed what done means here", with what was and what is now), recorded in the feed
(`task.criteria`), and **a tester's pass bound to the old criteria needs checking again**, as for
the owner's rewrite. It counts as the lead's progress on the task, and renews the budget.

### A free-brain daily turn cap, on from the start

**60 turns a day for a member** on the house's own models (the node's and the Mac: `Brain::house`),
`THRESHOLDS.free_turns_a_day`. Past it the member's work turns **wait until the day turns**: the
61st is queued as *"waiting for tomorrow: 60 turns a day is the most a member takes on the house's
own models"*, which is a wait like the two before it (for budget, for the Mac): the work is held,
not stuck, and nothing climbs for it. It is counted by the owner's days, per member, and a turn on
a Claude brain is bounded by its budget and is not counted, so a decision asked of the member
still reaches its judgment brain. It is shown where waits already are: a chip in the top bar, the
waiting line of the outcomes (0132), and the member's roster line.

It is on from the start because a runaway on free models is the failure it prevents, and because
nothing is lost: the turns wait.

### A runaway tripwire, on from the start

**More than 40 turns started by the team in any 10 minutes** (`THRESHOLDS.runaway_turns`,
`runaway_window_ms`): when 40 have started and another is about to, HQ **stops the team itself**,
the way the owner's switch does. Every turn stops and nothing new starts; the reason is *"stopped
by the runaway tripwire"*; the forty-first never starts. **One card** goes to the owner's desk
and it interrupts (kind `tripwire`): how many turns each member started in the window, so the
owner can see who was looping. The header says who stopped the team (`stopped_by`: the owner or
the tripwire), and the feed says it with the evidence (`stopped`).

The owner resumes as today, from the card or the top bar. Turns started before the resume are
not counted against what starts after it (`tripwire_since`), and the card is answered. The
owner's own kill switch is still the owner's and puts no card on the desk. Forty in ten minutes
is a turn started every fifteen seconds across the whole team; the scripted demo started about
seven a minute and crosses it in under six minutes. A real burst (a lead fanning a goal out to
many members) can come near it, and a false trip costs the owner one tap.

**The scripted demo is exempt from both caps** (`Settings.scripted`, set by HQ in demo mode): its
pace is the point of it. The setting is off everywhere else, so every other run and every test
has the caps.

### Stored

No schema change and no migration. HQ's settings gain keys, each read as absent until written:
`guard_mode:coordination` (default shadow), `coordination_since:<task>` and
`coordination_asked:<task>` (when the lead last decided, and whom it was asked), `tripwire_since`
and `stopped_by`. New events: `guard.mode`, `task.criteria`; `wake.held` carries
`why: coordination_budget` when it is held for the budget, and `stopped` carries `by` and, for the
tripwire, `turns`, `window_ms` and the members. The snapshot gains `stopped_by`, `pool.capped`,
`outcomes.waiting.cap`, a `mode` on a guard's precision row, the caps among the thresholds, and
the spend of a ceremony window; the desk gains the kind `tripwire`, which interrupts. HQ offers
`narrow_criteria` to leads.

**What this retires.** The ceremony's blindness to work outside a project is replaced in place,
not accompanied by a second detector. The feed's unconditional "You pulled the kill switch" now
says who stopped the team. The ceremony finding's "narrow its criteria" is now something a lead
can do. No mechanism is duplicated: a held talk wake is the held wake of 0131, and the question to
the lead is the Guard wake and `decide_stuck` the stuck work already uses.

## Consequences

- **In shadow, a returning owner reads what the guard would have done**, a finding on the Guards
  card and one in the digest, each with the turns and the spend, and says whether it was right.
  Shadow findings are findings: they count in the Work tab's badge. The owner switches the guard
  on when its verdicts are mostly real; until then the team works as it did.
- **Switched on, a task over its budget stops being asked for more talk** until its lead decides
  or the day passes, and the lead spends a turn deciding. A hold that was wrong costs the task
  that delay. The numbers are first guesses; the verdicts tune them, and putting the guard back in
  shadow releases everything held at the next tick.
- **A busy member has no override for a day's turns on the free models**: a member that is working
  legitimately past 60 waits until tomorrow, and the owner sees it in the top bar. The number is a
  constant, and an override is a decision for when the measure asks for one.
- **A wake that arrives while the team is stopped is turned away**, as it is for the owner's
  switch; a false trip stops the team until the owner taps Resume. The card says who started what,
  which is how the numbers are tuned.
- **The snapshot's `desk` may now hold a `tripwire` card.** A client that does not know the kind
  treats it as one that interrupts (0132) and may not know how to answer it; the console does.
- **Rolling back past this record needs the `coordination` findings removed first.** An older
  binary reading a finding for a guard it does not know reads it as a corrupt row, and every
  call that lists findings fails. (`DELETE FROM findings WHERE guard = 'coordination'` in HQ's
  database; nothing else in the record changes shape.)
- The ceremony now sees work that comes from "Give work" and routines. Findings that were never
  raised for it may now be, and the first raised are for work that has been spinning all along.
- The budget counts coordination as the ceremony labels it; the labels' errors are its errors.

## Alternatives considered

- **Grouping project-less work by goal.** Rejected, for the reasons above: a goal pools unrelated
  work, hides a spinning tree, and wakes a lead for work it does not run.
- **Switching the budget on straight away.** Rejected: a guard that holds the team's talk is new
  authority, and a first guess at 20 could be wrong in either direction. Shadow costs a finding
  and a toggle, and the verdicts say whether to switch it on.
- **Trying the caps in shadow too.** Rejected: the failure they prevent is a runaway, the
  daily cap loses nothing (the turns wait), and the tripwire stops what the owner resumes in one
  tap. A runaway is the wrong thing to watch happen.
- **Holding talk as a waiting turn** (the wait for budget or the Mac, as 0131 has them). Rejected:
  a task has one live turn, and a waiting turn is a live one, so it would stand between the lead
  and the wake that asks the lead to decide. A held wake is behind nothing the lead needs.
- **Asking the lead as a rung of the stuck ladder.** Rejected: a rung means the work is stuck, and
  talk is not stuck work. The ladder's diagnosis would call it something it is not, and its
  timers would climb. The question is a Guard wake, and `decide_stuck` answers it when it was asked.
- **Dropping the held talk.** Rejected: nothing is silently dropped. A mention may be the only
  word a member sent, and a held wake is still there when the lead has decided.
- **A cap in dollars for the free models.** Rejected: they cost nothing, so there is nothing to
  cap; the unit that bounds a runaway is a turn.
- **A tripwire that counts only the free models.** Rejected: a runaway on Claude is bounded by
  its budget but still a runaway, and costs the owner real money before it does.
- **A model that decides whether the team has run away.** Rejected: models propose and code
  decides. Forty starts in ten minutes is arithmetic over HQ's own record, and the action is
  reversible and announced (0044).
