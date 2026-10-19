# 0132 — Outcomes, and a desk that interrupts only for decisions

## Status

Proposed, 2026-10-19. Prepared with AI assistance (Claude), from the owner's review of HQ's record
of 17 and 18 October and the brief that followed it. Amends
[0124](0124-guards-against-how-agent-teams-fail.md): the "while you were away" digest it deferred
is built, for every kind of item that needs no decision, and a finding becomes one per guard and
what it is about, refreshed while it stands and not raised again for what was judged. Builds on
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (evidence), [0115](0115-the-engineering-wing-and-the-git-flow.md)
(the owner's push), [0129](0129-the-teams-history-and-asking-the-past.md) (the record and consults)
and [0131](0131-the-two-kinds-of-turn.md) (turns that wait). Built: everything below, and the
owner's scenarios in `domains/team/features/what-the-work-came-to.feature` and
`domains/team/features/a-desk-that-interrupts-only-for-decisions.feature`.

## Context

HQ's record of 17 and 18 October, about 39 hours, was read for what the work came to, and found
that everything HQ measured and showed was activity:

- **201 turns and $8.85 delivered no change the owner accepted.** 362 posts were written in
  threads against 46 acts. Nothing on the screen said so: it counted turns, tokens, send-backs and
  tasks, and none of them is a change in the lab or the fork.
- **The owner's desk got 30 items in 38.6 hours, about 19 a day, and 6 were real decisions or
  approvals.** The rest: stuck work 7, a guard's finding 5, a member out for the day 4, a notice
  3. Each was a card, and each interrupted.
- **The same task was flagged by the stuck guard up to four times**, and **a ceremony finding came
  back three times after the owner marked it resolved**. The code explains both: a stuck finding
  closed itself whenever any turn touched the task, the ladder's own wake included, and a new one
  opened the next time it stopped; a ceremony finding closed when the talk fell under the line for
  a day, and a new one opened when it came back over.

The designs we borrow from keep one finding for a row and a kind, refreshed while the condition
holds and closing itself when it stops, with a watermark for the evidence already seen; they send
a person who has been away one digest of what happened, not a message for each. Ours kept a card
for each detection. ADR 0124 had named the digest as deferred, for ceremony only.

## Decision

### Outcomes, beside the activity

The Team tab's "How the team is doing" card opens with **Outcomes**, over the last day and the last
week (rolling, ending now, the windows the guards look over), and the top bar says how many
changes landed this week. Every number is computed by code from HQ's own record
(`application/scorecard/outcomes.rs`, pure) and what cannot be known is not guessed.

| Measure | Definition |
|---|---|
| **Landed** | Distinct tasks whose branch the node pushed to the fork after the owner approved the push card: the push's own outcome (`branch.pushed`, ok). An approval the node has not yet pushed has not landed, and neither has a push that failed. A branch the owner pushes by hand from a downloaded bundle is not seen. Beside it: branches submitted, checks passed. |
| **Cost of one** | The window's spend (the turns that ended in it) over what landed. With nothing landed it is "none landed", never $0 and never a division by nothing. |
| **Talk : work** | The ceremony guard's own labels (`guards::turn_kind`) over the same turns: coordination turns against turns that changed the product, with the posts members wrote and the acts that succeeded. The standing reviews of a lead (its report and patterns turns) are left out. Measured by the five-minute process-health look and kept (`guards_talk`), so the snapshot stays cheap; "not measured yet" until the first look. |
| **Desk** | Items added in the window that interrupt, a day-rate of them, and those that went to the digest. |
| **Waiting now** | Turns waiting for budget and for the Mac (0131), and how long the oldest has waited. |
| **Memory** | Work sent back as `exists_already`, searches of the record (`ask_record`), and consults asked, answered, and answered from the record. |

A tester or a lead may name `reason: exists_already` on a send-back (`domain::review::SendBackReason`,
a vocabulary of one so far; any other reason is refused, and so is one on a pass). Every send-back
is told in the feed (`task.sent_back`, with the reason or none) and every search of the record
(`record.asked`, with how many passages it found). `consults_shown`, which nothing called, is
removed: the counts replace it.

### A desk that interrupts only for what needs the owner

`domain::desk` says which kinds interrupt: **every kind but three**. A decision, an approval, a
question, a push (and a branch ready to merge), a project's cap or budget, a member out for the
day (money is the owner's), a recall, canon, a goal, a hire, a furlough, work sent back three
times, and a write-up to read all stay on the desk as before. **Stuck work, a guard's finding and
a notice** do not: none needs a decision before the team can go on.

They collect into **one "While you were away" digest** (`snapshot.digest`), the first card of the
Desk tab when it has anything in it, with a count, **newest first**. Each item keeps the choices it
always had, on the endpoints it always had: try again or drop for stuck work; real, false
positive, can't tell or resolved for a finding; seen for a notice. **Acting on an item takes it
out.** The owner's verdict on a finding from the Guards card is the owner acting on it, and puts
its card away; a lead's verdict is not, and leaves the card for the owner. `snapshot.desk`, and the
"N waiting on you" chip and the Desk tab's badge that count it, now hold only what interrupts. A
kind nobody has classified interrupts: a decision is never quietly folded into a digest.

### One finding for a guard and what it is about

A guard's detection is weighed against the newest finding for its key (stuck work: the task; the
ceremony: the project; a carousel: the task; woken for nothing: the task and the member; the same
ask: the card and the task; a fence push: the run, or the tool) by one pure rule,
`guards::reconcile`:

| The newest finding is… | …and the detection rests on… | …so |
|---|---|---|
| none | | a new finding |
| open, not judged | anything | **refreshed**: its evidence and time (no oftener than HQ looks unless the evidence changed), its card reworded |
| open, judged | less than a step more than was judged | **refreshed**, resting still on what was judged |
| open, judged | a step more | it closes, and a new finding opens (**renewed**) |
| cleared, not judged | anything | **opened again** as itself: one more episode, a card of its own |
| cleared, judged | less than a step more | **held**: nothing happens |
| cleared, judged | a step more | a new finding |

What a detection rests on is its **mark**, the evidence watermark, a count that only grows with new
evidence: for stuck work, how many of the task's turns advanced it, so that work which stops
again with nothing having moved it is the same stuck work, and a new episode after progress is
new; for the ceremony, the project's coordination turns in the week; for a carousel, the times the
work was sent back; for woken-for-nothing, the member's turns on the task; for a fence push, the
newest refused act. A judged finding keeps the mark it was judged at. **The step** is one for
all but the ceremony, whose count creeps up with every turn: another `ceremony_least` (12) turns of
talk, so that one more turn does not bring back what the owner resolved, and a dozen more does.
Any verdict counts as judging, "can't tell" included. Stuck work whose finding was judged, and
has not moved since, puts up no new card either.

### Stored

The findings table gains three columns, each added only if missing (`seen_at`, `episodes`, `mark`;
a finding with no time seen reads as seen when it opened, on its first episode, resting on nothing
yet), so the migration is idempotent and no row changes. `desk` gains no column: a card's words
are reworded while it waits. The process-health look keeps talk against work in HQ's settings
(`guards_talk`). Two new events, `task.sent_back` and `record.asked`, which the console's log
does not show. No threshold changes; `Guard::step` and the rule above are the new numbers, in the
domain beside the others.

## Consequences

- The desk and its chip count only what the owner must decide; a returning owner reads one card
  for the rest. Of the thirty items, the fifteen that were stuck work (7), findings (5) and
  notices (3) would have waited in the digest, and the one task flagged four times would have been
  one finding.
- Stuck work no longer interrupts. The ladder, the nudges and the findings are as before and the
  team still climbs; a task nobody can unstick sits in the digest until the owner looks. A matter
  that needs the owner reaches the desk as the decision, approval or question the chain makes of
  it, and money never goes to the digest.
- A finding the owner judged is quiet until its evidence has grown by a step; the ceremony needs
  another dozen turns of talk. The step is the first guess, tuned from the verdicts like every
  threshold, and a finding that was wrong is judged false positive once, not three times.
- "Landed" counts what HQ was told was pushed. It is the number the activity was missing: zero for
  the 39 hours it was written for, and the first thing a returning owner reads.
- Talk against work is as fresh as the last five-minute look, and says so before the first.
- The snapshot's `desk` no longer holds stuck work, findings and notices: a client that wanted
  them reads `digest`. The node reads the desk only for approvals, which stay on it.

## Alternatives considered

- **The digest for ceremony only, as 0124 deferred it.** Rejected: stuck work and notices were
  most of the noise, and the same fix serves all three.
- **Taking stuck work, findings and notices off the desk altogether.** Rejected: each is the
  owner's to act on (try again, drop, say whether a guard was right), and an item with no place
  would be an item nobody acts on.
- **Raising a judged finding on any new evidence.** Rejected for the ceremony: one more turn of
  talk, minutes after the owner resolved it, would bring it back, which is what happened. Where
  each detection is an event of its own, any newer one is new.
- **Counting an approval as a landed change.** Rejected: an approval the node could not push did
  not land, and the feed tells which.
- **Computing talk against work on every snapshot.** Rejected: it reads each turn's receipts,
  citations and thread, and the snapshot is served every few seconds. The look every five minutes
  already does it, and the number is kept.
- **A model that summarises the digest.** Rejected: models propose and code decides, and the digest
  is a list of stored facts the owner can act on, not prose.
