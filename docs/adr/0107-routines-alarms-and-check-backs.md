# 0107 — Routines, alarms and check-backs: how the team keeps working

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amended by
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (2026-10-17): the back-off reads each
turn's label (posts alone are no longer progress), a member's turns that show nothing are nudged
twice and then its task goes up, the home lab's checks carry two live criteria and close on a
fresh reading, and a check-back on a task blocked on a wait is replaced by the wait. Amends
[0102](0102-endora-hq.md) (the engine's wakes and tick) and builds on
[0106](0106-team-contracts.md) (the contracts a routine's member uses). Built: routines in HQ's
store, engine and API, the node's routes and the HQ screen's Routines card; alarms from the
node's home-lab observer; the `check_back` tool; wakes on a clock that find nothing to do;
the back-off for empty turns; and the home lab's routine made once on first start.

Amended 2026-10-17, after a live alarm: a VPN container's restart stopped deluge, nzbget and
prowlarr for a minute, and the team spent an hour diagnosing what had already passed, in two
duplicate tasks. Now **coming back is news too**, the worker's rules say **an alarm is a
moment**, and a member's sub-task is refused when an open task is already about the same
containers (**one task a problem**). See the decision's sections of those names.

## Context

The owner set the team the goal "Keep the home lab healthy". After one good turn the team went
silent: nothing woke it. The engine had a `timers` table and a tick step for it, but nothing in
production created a timer, and a timer's wake (the lead's look round) named no task, so a
turn it started had nothing in particular to do.

Two easy fixes were wrong. A member that wakes on a timer and looks for work spends a turn
every time, mostly finding nothing, and decides for itself what the team works on. A goal that
"runs" on its own has no task to carry its why, its thread, its owner or its record.

## Decision

**The principle.** Goals are passive: they say what the work is for, and do nothing. Work
always arrives as a **task**, handed to one member. A member never goes looking for
unassigned work. Everything below hands out a task, or wakes a member about a task it has.

**Routines.** A routine is work that comes round again: a title, a description, the goal it
serves, the member it is handed to, a rhythm (`every_ms`, between 15 minutes and 30 days) and
when it is next due, on or off, and whether it takes the home lab's alarms. Firing it creates
a `todo` task under its goal, for its member, with its title and description and why it fired
("It comes round every 6 hours.", or what the alarm said). Creating it wakes the member as any
assignment does. The tick fires the routines that are due.

- **One open task a routine.** While its last task is open, a firing on the clock (or the
  owner's "run now") is skipped, and the skip is recorded. An alarm is posted into the open
  task's thread instead, as Endora's, which wakes its member with something new.
- **Missed firings are skipped, never caught up.** A late tick fires once and counts the next
  time from then; a routine switched back on counts from then.
- **Every firing, skip and post is in HQ's feed** (`routine.fired`, `routine.skipped`,
  `routine.posted`, and `routine.changed` for making and switching one).
- **A routine's check is closed by its member.** A routine's task is a check: it changes no
  code, so there is nothing for the gate to test. Its member, once it has taken the task,
  closes it as `done` with what it found; ordinary work is still done only by the gate and a
  different member's review. Without this, a routine's first task would stay open for good
  and every later firing would be skipped. Any fix the check made went through a contract and
  is already on the owner's desk as a notice.

**Alarms.** The node's home-lab observer (every 30 seconds) keeps, in memory, how each
container and service was last seen. When one goes from well (running and not unhealthy; a
service that answers) to trouble (unhealthy, restarting, exited, dead; a service unavailable),
the node tells HQ, signed with HQ's token, in one factual line: "sonarr became unhealthy at
14:02", at the owner's time of day. HQ fires the first routine that is on and takes the home
lab's alarms.

- Only new trouble: whatever was already wrong when the node started, or when it first saw a
  container, raises nothing; only a change from well raises one, never every 30 seconds.
- At most one alarm a container (or service, which shares its container's hour) an hour.
- Never Endora's own containers: `endora-*` and the same set the contracts refuse.
- Nothing when HQ is not set up on the node, and HQ refuses alarms while its demo is on.
- Each alarm told is in Endora's audit log.

**Coming back is news too.** An alarm is a moment, not the state now. When a container or
service that alarmed within the last day is well again and settled (running and its health
check not still starting; a service that answers), the node tells HQ once for that alarm,
`POST /v1/alarms/recovered` with `{ "text": "deluge is back up at 13:43" }`, signed. HQ posts
"Recovered: deluge is back up at 13:43" as Endora's in the open task of the routine that takes
the alarms, and wakes whoever has it as an alarm does (`alarm`: a new reason would have added a
name to the screen, the store and the back-off, and the post already says what is new). With no
such task open it does nothing: a recovery hands out no work. Down again within the hour raises
no alarm, so its coming back is no news either. The worker's home-lab rules tell every member:
an alarm is a moment; check `homelab_status` before acting, since what alarmed may be back up;
if everything has recovered, say so and close the task (a routine's check as done, other work
moved on with `set_status`).

**One task a problem.** Every alarm already goes into the routine's one open task. A member's
new task (`create_task`) is refused when an open task under the same goal is already about the
same home-lab containers: those its title names, or its description's when the title names
none (a description also names what to leave alone), matched case aside as whole words against
the names the node last said. The refusal names the task and what to do: "#664 already covers
this (deluge, nzbget, prowlarr): add to it with post in #664, rather than a second task". The
member's own task above the new one, and everything above that, is the work being split and
never counts; so a sub-task made on another member's task about the same containers, the live
case, is refused. Refusing was chosen over merging the new task into the old: it changes
nothing behind the model's back, and the model is told exactly where its words go. The owner
and Endora may hand out what they like, and nothing is judged before the node has said what
the home lab has.

**Check-backs.** A team-protocol tool, `check_back { minutes, why }`: one later wake for the
turn's member, on the turn's task, after the minutes given, held to 5–720. It fires only while
the task is open, through the normal wake path (so it folds, holds and is refused like any
wake), and the member's next briefing says "You said you would check back: …". A newer
check-back from the same member on the task replaces its older one, and a task holds at most
three, the oldest making room. The worker is told: after a fix, check back to confirm it held.

**Nothing to do.** A wake on a clock (a timer, a check-back, an alarm) that names no task, for
a member with no open task of its own, is refused as `nothing_to_do` and starts no turn.

**Back-off.** After two turns in a row on a task that moved nothing forward (the engine's own
verdict on each ended turn), a wake for that task that brings nothing new — a recovery nudge,
a retry — waits: 2 minutes, doubling, at most 30. Anything new is never kept waiting: an
owner's post or answer, someone else's post that names the member, a blocker finished,
sub-tasks finished, a check-back or an alarm. Such a wake brings a waiting turn on its task
forward to start at once.

**The home lab's routine.** Not in the schema. When HQ starts with the demo off, with no
routine at all yet, an active goal titled "Keep the home lab healthy" and a lead, it makes one
routine, as Endora's and in the feed: "Check the home lab", every 6 hours, for the lead,
taking the home lab's alarms, asking it to look with `homelab_status`, read the logs of
anything unhealthy, fix what a restart can fix, check back after a fix, and put anything else
to the owner as a plan. It first fires 6 hours later; the owner can run it at once. The owner
adds, switches and runs routines from the HQ screen.

**The reviewer between the home lab and the desk** (added 2026-10-17). Live, `in_review` was a
dead end: nothing woke a reviewer, no worker could approve or send back, and approval needed a
gate that does not exist yet. Now work moved to review, and a member's question about the home
lab (under this routine's goal, or naming containers), wake the team's reviewer
(`review_requested`) instead of reaching the desk; the reviewer's `review` tool approves or
sends back (`sent_back` wakes the owner of the work), and the owner of work in review takes no
turn on it meanwhile, except for the owner's word or a send-back. The home lab's work changes
no code, so it needs no gate: an approval closes it. Every question that does reach the desk
lists the names in it the home lab does not have, against the containers the node tells HQ.
The details are in the plan, §5 and §6.

## Guards

Firing goes through the engine's rules, and a firing they refuse hands out nothing (so no task
is left open to block the next one): it is skipped, and says why, when the kill switch is on,
the routine's member is paused, its budget for today is spent, or its goal is not active. The
tick does not run under the kill switch at all. Every wake a routine, an alarm or a check-back
raises is an ordinary wake, with its receipt. Contracts still decide everything a member does
on the home lab ([0106](0106-team-contracts.md)).

## Consequences

- **Costs.** A routine every 6 hours is at most four turns a day for its member, each reserved
  and charged like any other; a skip and a refusal cost nothing. Alarms are capped at one a
  container an hour, and pass into an open task rather than making more. Check-backs are capped
  at three pending a task and twelve hours out. The back-off caps a task whose turns keep moving
  nothing at about two stale turns an hour.
- An alarm reaches the team only through a routine that takes it: with none, HQ answers that
  nobody took it, and the node does not try again.
- The node's alarm memory and debounce reset when it restarts; the cost is that trouble present
  at the restart raises no alarm until it clears and comes back.
- A routine's check closed as `done` reaches no desk: it was a look, not a change.

## Alternatives considered

- **Members waking on a timer to look for work.** Rejected: a turn spent looking, mostly
  finding nothing, and the member, not the owner, deciding what the team works on.
- **A new task on every alarm.** Rejected: a flapping container would bury the board; the open
  task's thread takes the news instead.
- **Catching up missed firings.** Rejected: six checks in a row after an outage say nothing six
  times.
- **The gate for routine checks.** Rejected for checks only: there is no change to test, and the
  routine would never fire again.
