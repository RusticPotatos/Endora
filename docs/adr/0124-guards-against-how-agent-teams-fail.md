# 0124 — Guards against how agent teams fail

## Status

Proposed, 2026-10-18. Prepared with AI assistance (Claude). Amends
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (a tester's pass is bound to the
criteria it judged; evidence already enough is submitted, not polished; re-checking it is talk
only), [0118](0118-elastic-staffing-and-notebooks.md) (a note that cites another is built on it;
the owner retracts a note as not true) and [0120](0120-roles-as-an-enforced-raci.md) (only
parked work climbs the ladder; a review carousel goes to a huddle at once; each briefing carries
its goal's "not now"). Built: everything below, and the owner's scenarios in
`domains/team/features/guards.feature`.

## Context

A team of models does not fail by going rogue. It fails the way a busy office does, one locally
sensible step at a time, and no single turn shows it. In our own words, the ways we have seen,
or can see coming in HQ's record:

1. **Talk stands in for work.** Turns that post, re-read what was already read, review, ask
   and huddle, while nothing changes in the lab or the branches. HQ caught the *empty* turn; a
   *busy* one went round worker, tester and lead without ever being called stuck.
2. **Checks pile on checks.** Each check answers a real failure, and none is ever taken away.
   HQ is built of checks (criteria, receipts, a tester, a lead, refusals), with no measure of
   what they cost against what they produce.
3. **Proof polished after it was enough.** A member whose evidence already met the criteria
   kept taking readings. Its posts then became the next member's idea of normal.
4. **Scope grows by defensible steps.** A sub-task, then another, each sensible; nothing compared
   their sum with the ask, and a sub-task's criteria need not serve its parent's.
5. **A wrong claim spreads.** One member notes it; the next reads it and writes it in its own
   words. Correcting one copy fixes one copy, and nothing could list the rest.
6. **The fence is pushed at.** Refused acts are receipts, one at a time; nothing noticed a
   member, or two, trying the same thing again and again: a sign of what they believe they are
   for.
7. **Stopped is not one thing.** Work stopped on purpose (blocked, with a check-back to come)
   and work dropped by accident were both "no turn moved it", and both were nudged and climbed.
8. **The same nudge, again.** A recovery wake could be sent again with nothing new to say, and a
   member woken for nothing three times was woken a fourth.
9. **A pass outlives what it passed.** Nothing tied a tester's pass to the criteria it judged.
10. **The same ask twice.** Live, 18 October: the CEO put two near-identical board decisions on
    the owner's desk, one for each of two tasks with one cause.
11. **"Down" from a slow answer.** A home-lab read that timed out was "unavailable", which is an
    alarm.

A guard that never fires looks the same as one that works, and one that fires on everything is
worse than none (a published sweep elsewhere was half false positives). So every guard here is a
rule over HQ's record (models propose, code decides), its numbers sit in one table, and every
finding takes a verdict, so its precision is measured before anyone tunes it.

## Decision

**Thresholds** live in `domain::guards::THRESHOLDS`, one table, and the Guards card shows them.

| Guard | What | Threshold (start) | Where enforced |
| --- | --- | --- | --- |
| Stopped diagnosis | Each open task, read when asked: **working** (a turn or a held wake), **parked** (its member idle, still owed), **held** (blocked with a named reason and a live check-back or answer to come, or waiting on open work), **indeterminate** (nobody holds it, or its last turn failed for good) | — | `domain::guards::diagnose`; the tick's steps 5 and 5b; `climb_stuck` |
| Receipted nudges | A recovery wake is recorded under task + member + reason + what HQ saw (status, posts not HQ's, citations, newest receipt, newest wake not HQ's own) before it is sent; a key is never sent twice | — | `nudges` table (append-only); `engine::guards::nudge_once` |
| Stuck | Parked work nudged once and still parked: a finding; on the desk once, closing itself when the work moves | — | ladder, `guards_tick` |
| Closure reasons | Done: met criteria, superseded (successor), not needed. Dropped: obsolete, duplicate of #N, won't do, blocked for good, optional successor. A task waiting on a dropped one stays blocked; its holder is told at once (`wait_dropped`) | — | `domain::closure`; `closures` table; `after_closing` |
| Ceremony | Per project, over 24 h and 7 d: coordination turns (talk only, a re-check with no new reading, a review round or huddle view, a question up the chain) against product change (an act, a branch, a criterion met) | 12 coordination turns and 4:1 | `domain::guards::ceremony`; `health_tick`, every 5 min |
| Carousel | Sent back with no new evidence submitted between returns: straight to a huddle; a submission while it decides waits | 3 returns | `after_sent_back`; `review_asked` |
| Redundant wakes | A wake that brings no news, to a member whose last turns on the task showed nothing: refused, the work parked | 3 turns | `wakes::decide` (`Refused::Redundant`) |
| Sub-tasks | At most 5 open under one task, at most 3 deep; a sub-task of a member's task with criteria names which it serves (`serves`); the owner's and a routine's tasks (depth 0) need none | 5 open, depth 3 | `create_task_in` |
| Enough evidence | On each turn on its own doing task, HQ dry-runs the evidence rule on the member's readings since the last send-back; enough, the briefing says submit now; re-checking turns after that (every reading one already taken) are talk only; on the third HQ submits it | 3 re-checks | `engine::discipline` |
| Retract and sweep | The owner's "Not true" on a note, section or story keeps its fingerprint (lowercase, punctuation stripped, word pairs), takes it out, and lists what else holds enough of it to forget at once; a later note, section or story that says it again is refused, with the date; a note citing `n12` is built on it, and forgetting one names its descendants | 60% of the claim's pairs, 2 at least | `domain::retraction`; `retractions` (append-only), `note_parents`; `note_in`, `write_world_in`, `tell_story_in` |
| Fence push | Refused acts in one turn, or the same refused act for different members within a day: a deduped desk notice naming the tool and the members; every briefing carries its goal's "not now" | 3 a turn; 2 members in 24 h | `after_refused_act`; the briefing |
| The same ask once | A question or board decision alike both ways to one waiting from another task attaches to it ("also on #672"); the owner's one answer reaches every attached thread and wakes each asker. Approvals never merge: each runs its own stored action | 60% both ways | `ask_on_the_desk`; `desk_attached` table |
| Pass bound to criteria | A tester's pass keeps a hash of the criteria; changed since, it reads "needs re-check" and does not count | — | `test_in`, `tested_since_submitted` |
| CEO rescues | Per member: the CEO's directions on a task it holds, and its stuck work that climbed to the CEO, on the roster | — | `rescues` |
| Reachability | A read that timed out is `unverified`, never `unavailable`: no alarm, and what was last seen stands. `/health` gives the tick's last finish, its age and whether it is within three periods | — | `capabilities::observe_homelab`, `lab_alarms`; HQ `/health` |

**Findings and verdicts.** Every finding (stuck, ceremony, carousel, redundant wake, fence push,
the same ask) is a row in `findings`: its guard, a key (one open a key), the task or project, the
rule as text, the evidence it fired on, when it opened and cleared, a verdict (real, false
positive, not enough to say, resolved), whether the fix worked, and who judged. The owner judges
from the desk notice or the Guards card (`POST /v1/hq/guards/findings/{id}/verdict` through the
node, recorded in its audit log); the lead of the finding's area with `judge_finding`. A guard's
**precision** is real and resolved over those judged real, resolved or a false positive; "not
enough to say" is no verdict on the guard. Findings clear themselves when their condition no
longer holds; a desk notice clears with its finding.

**Who is woken.** Never a random member: a ceremony finding wakes the project's lead on its
busiest task, with the finding in the thread; indeterminate work wakes its area's lead once; the
ladder (0120) takes parked work as before.

**Stored** in tables made if missing (`nudges`, `findings`, `closures`, `retractions`,
`note_parents`, `desk_attached`) and in HQ's settings (`serves:<task>`, `enough:<task>:<member>`,
`passed:<task>:<run>`, `guards_looked_at`, `guards_ceremony`). No existing row changes.

## Consequences

- Fewer turns spent on talk, polish and nudges that say nothing new; one more look for the lead
  where HQ cannot tell, and a few more refusals a member must read and act on.
- The detectors run every five minutes over the week's runs; at home-lab size this is small.
- A member's sub-task under its own task with criteria must say which it serves; the owner's and
  a routine's tasks are exempt, so most of today's splitting is unchanged.
- A retracted claim's fingerprint is compared on every note, section and story written; a
  paraphrase that shares too few word pairs passes, which is the price of no model in the loop.
- The first month's precision is expected to be low; the verdicts are how it is learned.

**Deferred**: a screen for the owner to rewrite a task's criteria (the engine has it, and the
pass's binding holds for any edit); the house log's timeline lines and thread posts in the sweep
(only notes, sections and stories now); a "while you were away" digest of ceremony; a ceremony
budget per task that refuses (only once the measure shows it is needed); model-assisted
candidate matching for the sweep; the desk grouping for approvals (by design, not deferred).

## Alternatives considered

- **A model that diagnoses each finding.** Rejected: a model is never the enforcement boundary,
  and its word on a finding is the thing the verdicts measure.
- **Exact strings for retractions and the same ask.** Rejected: model text paraphrases, and an
  exact comparison of model output is a mistake this project has made.
- **Stricter thresholds from the start.** Rejected: a loose start with verdicts learns the real
  numbers; a strict one teaches the team to work around the guard.
