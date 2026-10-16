# 0096 — Patterns it notices

## Status

Accepted, 2026-10-15, for the first standing job only: the house's daily rhythms. The mail
triage and the terminal sandbox the owner asked about in the same message are **proposed in
[the standing-jobs plan](../plans/standing-jobs.md) and not accepted here**: both need a
decision that is the owner's to make. Prepared with AI assistance (Claude).

## Context

[0094](0094-work-it-finds-for-itself.md) and [0095](0095-more-sources-for-work-it-finds.md)
give Endora goals it starts for itself. Every one of them is a *problem*: a device that has
stopped answering, a question it failed, a belief to look into. A goal ends. The owner's
description of what they want is different in kind: things it is "constantly doing,
iterating on", such as sorting the mail into buckets and surfacing what matters, noticing
patterns in the house through the day. Those do not end. They are reviews that run on a
schedule, produce a living result, and are worth running again because the data has changed.
Calling them goals, and forcing each to a `Complete` or `Partial`, would be the wrong shape.

So this introduces the other shape: a **standing job**. It is a scheduled review over records
Endora already holds, deterministic wherever it can be, saying something only when the
picture has changed, and putting what it says in the activity trail like the lane does. It is
not a goal, takes no slot in the lane, and does not use the model unless a later job needs
words made.

## Decision

**The first standing job is the house's rhythms.** The transition log already holds a
fortnight of every change the house settled on (ADR 0058). `find_rhythms` reads it for what
happens at about the same time on most days: the first change of a given kind each night
falling inside an hour, on at least five days and at least three in five of the days the log
covers. It is counting. It reports a count the person can check ("the front door goes on
around 07:30, on 11 of the last 14 days"), and says nothing about why.

Rules that keep it honest, each a unit test:

- A reading (a number) and a thing that cannot be seen (`unavailable`, `unknown`) are not
  routines. The first has no "happened"; the second is trouble, which is the lane's.
- A thing that changes more than six times a day on average flaps; it would "have a pattern"
  at every hour.
- A day the log has nothing for does not count against a routine.
- Nights are counted, not calendar dates, so a routine across midnight is one routine.
- The days are the person's: `ENDORA_TIME_ZONE` (a zone name, whose offset follows its
  clock changes, so a routine at 07:30 stays one routine across them), or the fixed
  `ENDORA_UTC_OFFSET_MINUTES`, because the node stores instants and does not know where the
  person is. Unset, it is UTC, and the times it says are UTC. (As built first, only the fixed
  offset existed: an hour out from the first clock change on.)
- Only the strongest dozen are kept, strongest first.

**It says something when the picture changes, at most every six hours.** The line goes to
the activity trail ("Regular patterns in the house: …"). The trail is also where the last one
is found, so there is no new store: if the newest line of this kind is the same text, or was
written under six hours ago, nothing is written. It runs on the same switch as the rest of
the lane (`ENDORA_OWN_WORK`), because it is Endora doing something unasked, and uses no model
and no skill, so it needs neither the quiet window nor a lane slot.

## What this does not do

- It does not say *why*, or what the person should do, or that a pattern is good or bad.
- It does not look at the network as a network. It reads what the house's sensing already
  records, which includes any device-presence entities Home Assistant exposes, and nothing
  else. Router-level traffic is a separate source (see the plan).
- It does not yet reach the brief or the overnight note. That is the same Phase 5 work as
  for the rest of the lane.
- It finds one window per kind of change. A light that goes on at 07:00 and again at 19:00
  yields the stronger of the two.

## Consequences

- With the switch on and a fortnight of log, the trail gains a line like the one above, once,
  and again only when the picture moves. A node with no Home Assistant has no log and says
  nothing.
- The count is over what was *recorded*. A house whose sensing was off for days reports on
  fewer days, and says how many.
