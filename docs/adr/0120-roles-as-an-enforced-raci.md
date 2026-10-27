# 0120 — Roles as an enforced RACI

## Status

Superseded by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Amended the same day
by [0124](0124-guards-against-how-agent-teams-fail.md): the ladder climbs only for work the
stopped diagnosis calls parked (held work waits for what will bring it back; work HQ cannot tell
about gets one look from its lead); work sent back three times with no new evidence between goes
to a huddle at once, and is not reviewed again before the lead decides; a lead's drop says why;
each briefing carries its goal's "not now". Amended again 2026-10-19 (hygiene, from a review of
the live record): work leaves an area only by its lead's hand (below). Amended again
2026-10-19, from task 687: changing the lab is the wing's in code (*Branches are the wing's*,
below). Amends
[0111](0111-the-ceo-and-the-chain-of-command.md) (what the CEO does, what wakes it, what it
reads, its briefings; the house log's lines), [0113](0113-projects-budgets-and-reports-to.md)
(leads propose projects, the CEO decides them and proposes none; the CEO's planning) and
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (the manager chain's acts for the
CEO; the hand-up after turns that show nothing; time-bound criteria; repeated readings; the
review-round cap), and [0109](0109-a-brain-per-member.md) (the CEO on a cloud brain reads what
the cloud lead reads). Built: everything below, and the scenarios in
`domains/team/features/roles.feature`.

## Context

The owner, on 2026-10-18, after reading the night's record: "Everything the CEO is doing I
would expect the ops team to do and propose to the CEO." "Basically a RACI chart… being in a
product-model focused org." The CEO "sets direction, owns the success, takes a step back and
thinks strategy long term: goal of the goal, plan of the plan, delegation to leads… Should they
be that deep in the weeds?" No.

The record showed why. Task #667, a home-lab alarm, went from its lead to the CEO after the
lead's turns showed nothing, and the CEO closed it two minutes after a restart on the criterion
"no new alarm for the same containers since"; the alarm came back two hours later. The lead had
taken four turns ten seconds apart, each re-reading the same status. Task #666, the wing lead's
plan inside his own approved project, went in circles: he asked the CEO's leave, the tester
asked another lead, the CEO was out of money, nobody came back, and HQ put "Stuck… nobody came
back to it" on the owner's desk. The owner, again: "Why wouldn't the team, if stuck, propose a
meeting… I would assume the lead would make a decision. Seems like a process gap. Work keeps
going through a process, getting stuck, then lands on my desk."

Agents drift across boundaries: told to stay strategic, a model still acts where it can. So
the roles are held by code, not by the briefing alone.

## Decision

**The RACI** (R does it, A owns the decision, C is consulted, I is informed):

| | Owner (board) | CEO | Ops lead | Eng lead | Members |
| --- | --- | --- | --- | --- | --- |
| Goals: why, what success looks like | A | R | C | C | I |
| Priorities and the budget split across projects | I (caps: A) | A/R | C | C | I |
| Lab health: alarms, diagnosis, restarts | I | I | A | — | R |
| Spotting patterns, proposing fixes | I | A (yes, no, not now) | R | C | C |
| Changing the lab (branches) | A (merge) | I | C (writes criteria) | A | R |
| Testing and acceptance of work | — | — | A (own area) | A (own area) | R (tester) |
| Hiring, furlough, recall | A | C | R (requests) | R (requests) | — |

**Areas come from the record, never from a name.** A lead's area is the lead and whoever
reports to it (however far up), and the projects it leads. A task's area is its project's when
a lead leads the project; otherwise the lead its holder reports up to; otherwise the earliest
lead at work. A task in a project the CEO leads, or held by the CEO in no project (its daily
review, its planning), is the CEO's own. "Ops" is the area whose projects take the home lab's
alarms; "engineering" the area that works in branches.

**1. The CEO out of the weeds.**

- **Reading is open to everyone**, the CEO included (the owner's amendment of the same morning:
  "the CEO can have access to everything, it is their system"). It keeps every read of the
  home lab, and on a cloud brain sees what the cloud lead sees (the scrubbed logs and the
  download queues, `CLOUD_LEAD_MAY_SEE`). It is offered **no act**: an act it calls says to
  direct the area's lead.
- **Never woken by the lab**: alarms and recoveries wake whoever holds the task, unless that
  is the CEO; then the area's lead, who is handed the task back.
- **On a task in a lead's area** the CEO does not claim, move, close, accept or send back
  (`CeoDirects`). Instead, `direct {task, direction, evidence}` posts "Direction for Ada: …"
  in the task's thread and wakes the lead (wake reason `directed`); **the lead decides**.
  The manager chain's acts (0114 §11) stay for a lead over its members.
- It creates and hands work **to leads only**, from its own work; a lead's stalled task is no
  longer handed up to it; it accepts only its own work.
- **Work leaves an area only by its lead's hand** (amended 2026-10-19). Live, a deploy cut a
  developer's turn short, the work was read as stopped by accident, and the CEO handed the
  engineering branch task to the Ops lead: a lead, so `hand_to` allowed it. Now `hand_to`
  refuses a member outside the task's area (`Refusal::OutsideTheArea`: "Ada is outside Kit's
  area: only Kit, who leads it, or the owner hands its work out of it") unless the caller is
  that area's lead; the owner hands work out through Give work and the API, not through this
  tool. Inside the area is not leaving it, so the CEO may still hand a task up to its area's own
  lead, and a lead's `decide_stuck` hand-on was already limited to its own team (the same test,
  `staffing::lead_of`). The CEO's way to have an area's work moved elsewhere is `direct`; the
  lead decides. Scenario: "The CEO cannot hand the wing's branch work to the Ops lead", in
  `domains/team/features/the-engineering-wing.feature`.
- **Branches are the wing's** (amended 2026-10-19). The table gave changing the lab to the
  engineering lead and its members, but `repo_submit` was offered to any lead. Live, on task
  687, the Ops lead built and submitted a branch herself; a lead never accepts its own work and
  never hands on its own task, so it sat in review while the CEO was asked twice. Now only a
  lead or a developer **in the wing** (`wing::changes_the_lab`: it leads a wing project or
  reports up to one who does) submits a branch or asks for a recheck. HQ refuses anyone else
  before anything is kept (`Refusal::BranchesAreTheWings`: "changing the lab is the engineering
  wing's: Kit leads it and its developers submit the branches. Ask for the change with
  propose_project, with its criteria and evidence; the CEO decides and the wing builds it").
  HQ's answer to the node's `/v1/team/turn` says `wing`, and the node neither offers nor runs
  `repo_submit` for a turn outside it, so the fork's mirror keeps nothing for it. An older HQ
  says nothing, and the role decides as before. Reading is unchanged: `repo_checkout` stays a
  lead's, a developer's and the tester's. Scenario: "The Ops lead's change to the lab is the
  wing's to build", in `domains/team/features/the-engineering-wing.feature`.
- **Leads propose; the CEO decides** (`decide_project`: yes, no, not now). The CEO no longer
  proposes projects. It keeps `propose_goal`, `ask_owner` (the board), `read_doc`, the house
  log and the notebook.

**2. Strategy per goal, the CEO's.** Purpose (the goal of the goal), what success looks like,
measures (the named numbers the scorecard shows), current bets (the active projects and why),
and not now. Written with `set_strategy` on evidence, a part at a time, and by the owner on the
goal card (`POST /v1/goals/{id}/strategy`, through the node). Seeded empty: the CEO's daily
review asks for any goal that has none.

**3. Each area's measures**, computed from HQ's record (`scorecard::lab_measures`,
`scorecard::branch_measures`) and shown per area on the scorecard: for an area whose projects
take the lab's alarms, unplanned outages over 10 minutes (alarm to its recovery or its task
closed), median alarm to closed, and repeat alarms for the same containers within 24 hours;
for an area that works in branches, the median from a branch submitted to the owner approving
its push (the merge itself is on GitHub, out of HQ's sight), and branches passing the tester
the first time. The CEO's briefings use these, not raw readings.

**4. Leads report up.** Each lead that leads a project gets, once, a daily "State of my area"
(due at the owner's 18:00) and a weekly "Patterns". The report's turn shows the area's open
work, its alarms in 24 hours and its measures, and `report_area {moved, stuck, need,
proposals}` keeps it as the area's report and closes the turn's task. A report's task is not
work: it is left off the board and the scorecard. The patterns turn shows the area's repeats
(the planning's container patterns, restricted to the area) and asks for `propose_project` on
evidence.

**5. The CEO's briefings.** Its daily review (and first turn): each goal's strategy (marked
when it changed since its last turn, asked for when empty), with the goals' notes; the leads'
latest reports; each area's measures over seven days; the proposals waiting, with their
evidence; the board's decisions of the week. Its planning: strategy, the week's measures, the
proposals, and the budget split (each project's month against its cap, the caps against the
pool). Neither lists the areas' work item by item; its other turns' news carries no raw alarm.
It reads the lab and the threads when it chooses to.

**6. The night's bugs.**

- **Time-bound criteria**: a criterion may wait (`after`, in minutes) from the work's last
  change (its latest act that succeeded, or the latest alarm about it). The lab's "no new alarm
  for the same containers since" waits 60 minutes. A check closed, the tester's pass and the
  lead's acceptance are refused before then: "wait until HH:MM (owner's time)".
- **Repeated readings**: a reading its turn before already took (the arguments compared
  without spaces, `{}` as none) shows nothing, and no longer resets the recovery nudge; a task
  whose member has a check-back waiting is not woken for recovery; a wake that brings nothing
  new waits two minutes after the member's last turn on the task.
- **The house log's lines** carry the year (`- 17 Oct 2026: …`), and a date the writer put at
  the start of the entry is taken off.

**7. A lead decides its own area, and stuck work climbs a ladder** (the owner's amendment of
2026-10-18, morning).

- A project's lead closes its own work in the project (with no branch on it) on its own
  acceptance with evidence: no tester's round, nobody's leave. Its briefing says so.
- Stuck work (stranded, or doing with no turn, once it has sat ten minutes past its nudge;
  work in review keeps its status, since a blocked task is out of the tester's reach: its
  lead is woken, 0115)
  climbs **one rung at a time, each once**:
  1. the area's **lead** is woken to decide: `decide_stuck` (proceed with what, drop, hand_to
     a member of its team, needs_ceo, needs_board), on evidence;
  2. still stuck, a **huddle**: HQ calls the lead and up to four members who touched the task
     in the last 48 hours; each gets one short turn for its view (`huddle_view`, three
     sentences at most, on evidence), then the lead decides (after 30 minutes on what was
     said); the huddle is written as a document on the task;
  3. the **CEO**, only when the lead says it needs the CEO (money, a new goal, another area)
     or the board, or did not decide; it directs, decides a proposal, or puts a board matter
     to the board with `ask_owner`;
  4. the **owner's desk** only as a board matter through the CEO, or when no lead (or no CEO)
     is left to climb to.
- Work sent back three times goes to a huddle, not the owner.
- Events `stuck.climbed`, `huddle.called {task, members}` and `huddle.decided`, in the feed;
  the office may walk a huddle's members to the meeting room on them.

**Stored** in tables made if missing (`strategies`, `area_reports`, `report_tasks`) and in
HQ's settings (`ladder:<task>`, `huddle:<task>`, each lead's routine ids); criteria keep their
wait in their JSON. No existing row changes.

## Why a product-model org

The owner runs the house as a product: the board says what matters, the CEO turns it into
strategy and measures and decides between the bets its leads bring, and each area owns its
outcomes end to end. That only holds when each role keeps to its letter in the table. A CEO on
the dearest brain doing a lead's work costs the most per turn and leaves the lead accountable
for nothing; a lead that waits for leave inside its own project leaves the CEO deciding what it
should not. The RACI is the contract, and HQ enforces it the way it enforces evidence: models
propose, code decides.

## Consequences

- The CEO's turns are fewer and about direction; the leads carry their areas, and the owner's
  desk holds board matters.
- A lead's report turns and huddles cost turns. Members on the house's model cost nothing.
- A CEO-led project is the CEO's own area: there it works as before.
- Measures that need the lab's container names count by an alarm's words until the node has
  said them.
- **Deferred**: the office walking a huddle's members to the meeting room (its events exist);
  measures named in a strategy checked against the ones HQ computes.

## Alternatives considered

- **The CEO's limits in its briefing only.** Rejected: the night's record is the CEO acting
  where it could.
- **No reads for the CEO.** Built first, then dropped by the owner the same morning: reading is
  open; acting is the area's.
- **The leads' reports as tasks on the board.** Rejected: a daily report is not work, and the
  scorecard would count it as work done.
- **Stuck work straight to the CEO.** Rejected: most of it is the area's to decide, and the
  CEO's turns are the dearest.
