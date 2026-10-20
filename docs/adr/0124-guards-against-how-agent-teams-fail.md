# 0124 — Guards against how agent teams fail

## Status

Accepted, 2026-10-19 (proposed 2026-10-18). Prepared with AI assistance (Claude). Amends
[0114](0114-evidence-acceptance-criteria-and-the-tester.md) (a tester's pass is bound to the
criteria it judged; evidence already enough is submitted, not polished; re-checking it is talk
only), [0118](0118-elastic-staffing-and-notebooks.md) (a note that cites another is built on it;
the owner retracts a note as not true) and [0120](0120-roles-as-an-enforced-raci.md) (only
parked work climbs the ladder; a review carousel goes to a huddle at once; each briefing carries
its goal's "not now"). Built: everything below, and the owner's scenarios in
`domains/team/features/guards.feature`.

Amended 2026-10-19, from the failure study this record began with and two live failures: two more
ways a team fails (12 and 13 below), built under *Amended: criteria first*. The owner's work now
says what done means, the exemption of the owner's and a routine's tasks from naming what a
sub-task serves is gone (a task with no criteria is the one exception, having none to name), a
routine's task carries criteria from its routine, and a request for the owner's yes says which
criterion of its task it serves. Scenarios:
`domains/team/features/what-done-means-comes-first.feature` and
`a-request-says-what-it-serves.feature`.

Amended again 2026-10-19, from a review of the live record and two audits (*hygiene*): a turn the
platform cut short is not its member's failure (*Amended: a deploy under a turn*), and two holes in
the closure contract are shut (*Amended: the closure contract*), and a guard says it cannot tell
when the record does not let it (*Amended: can't tell*). Scenarios:
`domains/team/features/a-deploy-under-a-turn.feature`, and in `guards.feature` and
`the-engineering-wing.feature`.

Amended again 2026-10-19 (durability): the ticks no longer count toward the events the feed keeps
(*Amended: can't tell*, the CEO's rescues), so the feed reaches back past the week the guards read.

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
12. **An ask with no definition of done.** The owner's work needed no criteria, and a sub-task
    under the owner's or a routine's task (depth 0, where a lead first splits work) did not have
    to say which criterion it served. Live, the owner's "no literal passwords in role defaults"
    went to a lead with nothing to check it against and split into three blocked sub-tasks, each
    defensible and none tied to what was asked.
13. **A request that serves nothing.** Live, a developer on the house's own small model on the
    NAS asked the owner to approve a web map search, with coordinates it had made up, "to find
    the NAS backup location", and it reached the owner's desk as an approval. The first half of
    the answer is which tools a model is offered ([0131](0131-the-two-kinds-of-turn.md),
    amended); the second is below.

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
| Closure reasons | Done: met criteria, superseded (*names its successor*, never itself), not needed. Dropped: obsolete, duplicate of #N, won't do, blocked for good, optional successor. A task waiting on a dropped one stays blocked; its holder is told at once (`wait_dropped`); *a task is not made to wait on work already done or dropped* | — | `domain::closure` (`may_close`); `closures` table; `after_closing`; `add_task` |
| Ceremony | Per project, over 24 h and 7 d: coordination turns (talk only, a re-check with no new reading, a review round or huddle view, a question up the chain) against product change (an act, a branch, a criterion met). *Amended*: a turn HQ cannot read (it ended before turns were labelled) is counted apart and said; a window that is over only if such turns talked, or under only if they changed the product, is *can't tell*: no finding opens and none that stands clears | 12 coordination turns and 4:1 | `domain::guards::ceremony_with_unread`, `Ceremony::over` (`Over`); `health_tick`, every 5 min |
| Carousel | Sent back with no new evidence submitted between returns: straight to a huddle; a submission while it decides waits | 3 returns | `after_sent_back`; `review_asked` |
| Redundant wakes | A wake that brings no news, to a member whose last turns on the task showed nothing: refused, the work parked | 3 turns | `wakes::decide` (`Refused::Redundant`) |
| Sub-tasks | At most 5 open under one task, at most 3 deep; a sub-task of any task with criteria names which it serves (`serves`), the owner's and a routine's included (*amended*: depth 0 was exempt, and is where a lead first splits work). A task with no criteria has none to name: work open from before they were asked for, and the plan HQ hands a project's lead | 5 open, depth 3 | `create_task_in`, `subtask_discipline` |
| Criteria first | The owner's work says what done means: at least one criterion, from Give work, a project's hand-it-work button or the API, or it is refused with the way to give one, and nothing is made. A routine's task carries criteria from its routine (the home lab's two live ones; a reading of what it is about, cited, for any other routine; none for a lead's report and patterns and the CEO's review and planning, which close by being filed or decided) | at least 1, at most 8 | `evidence::owner_may_hand_out`, `add_task_with`; `routines::criteria_for`, `fire_held_lock` |
| A request for the owner's yes | A bridged act that asks the owner says which of its task's criteria it serves (`serves`); one that names none, or sits on a task with no criteria, is refused to the member with the reason (the turn goes on) and never reaches the desk; the owner's card shows the criterion it serves, as it was asked | — | `guards::approval_serves`, `ask_approval`; the node's `bridge::call` |
| Enough evidence | On each turn on its own doing task, HQ dry-runs the evidence rule on the member's readings since the last send-back; enough, the briefing says submit now; re-checking turns after that (every reading one already taken) are talk only; on the third HQ submits it | 3 re-checks | `engine::discipline` |
| Retract and sweep | The owner's "Not true" on a note, section or story keeps its fingerprint (lowercase, punctuation stripped, word pairs), takes it out, and lists what else holds enough of it to forget at once; a later note, section or story that says it again is refused, with the date; a note citing `n12` is built on it, and forgetting one names its descendants | 60% of the claim's pairs, 2 at least | `domain::retraction`; `retractions` (append-only), `note_parents`; `note_in`, `write_world_in`, `tell_story_in` |
| Fence push | Refused acts in one turn, or the same refused act for different members within a day: a deduped desk notice naming the tool and the members; every briefing carries its goal's "not now" | 3 a turn; 2 members in 24 h | `after_refused_act`; the briefing |
| The same ask once | A question or board decision alike both ways to one waiting from another task attaches to it ("also on #672"); the owner's one answer reaches every attached thread and wakes each asker. Approvals never merge: each runs its own stored action | 60% both ways | `ask_on_the_desk`; `desk_attached` table |
| Pass bound to criteria | A tester's pass keeps a hash of the criteria; changed since, it reads "needs re-check" and does not count | — | `test_in`, `tested_since_submitted` |
| CEO rescues | Per member, over the last 7 days: the CEO's directions on a task it holds, and its stuck work that climbed to the CEO, on the roster. *Amended*: the climbs are counted from the event feed, which the node prunes to its newest 20,000 (not counting ticks, amended 2026-10-19); when the oldest event kept is newer than the week's start, the count would be too low and the roster says HQ cannot tell | 7 d | `rescues`, `guards::feed_reaches` |
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

**Amended: criteria first.** Four rules, each a line of code with a test, none a model's word:

- *The owner's work says what done means.* `add_task_with`, the owner's front door (Give work,
  the HQ API's `POST /v1/tasks`, a project's hand-it-work button), refuses none with
  `OwnerNeedsCriteria`: "the team starts from what done means: say at least one thing that must
  be true when this is finished". It makes no task and wakes nobody. `add_task`, the primitive
  under it, stays for the tasks HQ makes itself (a routine's, a started project's plan) and for
  work already open. **Work already open with no criteria is untouched**: no migration changes a
  row, and its lead may give it some with `narrow_criteria` (0133), which allows a task with none.
- *A routine's task carries criteria from its routine* (`routines::criteria_for`). The home lab's
  check keeps the two live ones (0114). Any other routine's task gets one: *a reading of "the
  routine's title" from this run, cited*, not live (a routine need not be about the lab), so that
  closing it takes a reading, and a routine that could never be closed would never fire again.
  A lead's standing reviews (its daily report and weekly patterns) and the CEO's (its daily review
  and weekly planning) get none: they are reports and decisions, closed by filing or deciding
  them, no reading exists to cite, and nothing splits one. A criterion there would only make
  closing them ask for evidence they do not have.
- *`serves` everywhere.* The depth-0 exemption is gone from `subtask_discipline`, so a lead's
  split of the owner's task, or of a routine's, names the criterion each part serves, as a split
  of a member's task always did. The one exception is **a task with no criteria**, which has none
  to name: it covers work open from before, and the plan HQ hands a project's lead when the CEO
  accepts the project (made with no criteria, the lead's own to plan; its parts need no `serves`).
  The refusal now says it applies "whoever asked for the task", and that a part which serves none
  is not part of it. The authority checks (who may hand what to whom, and the CEO's) come before
  it, so a member who may not make the task at all is told so rather than asked what it serves.
- *A request for the owner's yes serves a criterion.* The bridge adds `serves` to every act's
  schema, and requires it of one that always asks. The node passes it to HQ with the request, and
  `ask_approval` reads it against the task's criteria by `guards::serves` (its number or its
  words): a request that names none, or is on a task with no criteria, is refused with the
  reason (`ApprovalServesWhich` lists the criteria; `ApprovalNothingToServe` says what a lead and
  anyone else may do), before the member's turn is ended or anything is put on the desk. The
  card's `serves` is the criterion's number and words as they were when asked. Models propose; HQ
  decides.

**Amended: a deploy under a turn.** A deploy restarts HQ, the node and the runner in turn. Live,
one cut a developer's turn short, HQ read the task as stopped by accident (*its last turn failed:
the runner stopped it, not its member*), its lead was asked to look, and the CEO handed the work
to another area's lead ([0120](0120-roles-as-an-enforced-raci.md), amended). So HQ tells whose it
was that a turn did not finish well (`domain::runs::fault_of`), from what it was told and nothing
a model says:

- *The platform's*: HQ lost the turn (`Liveness::Gone`: HQ restarted, and with it the memory of
  what the runner was handed); the runner says the container was gone before it saw it end (no
  exit code, with its words: the runner has always said so, and is unchanged); or the container
  was killed from outside before its full time (exit 137 or 143).
- *The worker's*: every other exit (1, 3, none given), and a kill at the turn's full time, which
  is the turn's own timeout.

A turn the platform cut short is tried again a minute later, as the same member on the same run,
and **the first interruption does not count against its retries** (`runs::own_attempts`, one
free): it gets four starts, not three, and only the first loss is free. While it waits, the task
has a live turn, so the stopped diagnosis reads *working* and nothing is found, nudged, climbed
or put on a desk. The run's reason says `interrupted: …`, and the feed's `run.ended` says
`"interrupted": true`. An older runner that says neither why nor how is the worker's, as before.

**Amended: the closure contract.** Two holes in it, found in an audit of the record. *Superseded
named nothing:* work closed as done, superseded, was meant to name the task that carries it on
(the tool's own description said so), and was not made to. `closure::may_close` now refuses it
with no successor, and a successor that is the task itself, and `set_status` checks it, and that
the successor is there, before anything moves, as it checks a reason not in the set; the engine's
`done_because_in` checks it too. *A wait on finished work:* `add_task` accepted any `waits_on`,
and a task waiting on work already dropped was stuck by construction (what it waits for will not
happen: `wait_dropped` told its holder only when the dropping came after). It now refuses a
`waits_on` that holds a task already done (nothing to wait for) or dropped
(`GuardRefusal::WaitsOnFinished`, which says to leave it out or wait on the task that carries the
work on), and a task that is not there; a wait on open work is as ever. Closings already
recorded are read as they were: nothing is migrated.

**Amended: can't tell.** A guard that reads a gap in the record as zero says something false. Two
were found, and now say that they cannot tell:

- *Ceremony, over a turn it cannot read.* The count of product changes comes from each turn's
  label. A turn that ended before turns were labelled ([0114](0114-evidence-acceptance-criteria-and-the-tester.md))
  has none, and was counted as nothing, so a project could be found talking more than working, or
  cleared of it, on turns HQ never read. They are now counted apart (`Ceremony::unread`, said as
  "1 turn it cannot read"), and `Ceremony::over` is three-valued (`Over`): *over* if it is over
  even were every turn it cannot read a product change, *under* if it is under even were every one
  talk, otherwise *can't tell*. This reading of "the product count is missing" is HQ's: a turn
  with no label is the only way a count is missing, and the count is not guessed. A window that
  cannot be told is neither over nor under: no finding is opened, and a finding that stands is not
  cleared (and, standing, not raised again). The Guards card shows it as *cannot tell whether it
  is talking more than working: some turns have no label*; a task tree outside a project is
  shown only when it is over or cannot be told.
- *The CEO's rescues, over a pruned feed.* A climb to the CEO is an event, and the node keeps the
  newest 20,000. A count over a feed whose oldest kept event is newer than the start of the week it
  counts would be too low, and was shown as a plain number. It is now counted over the last week
  only (the longest window the guards use), and when the feed no longer reaches back to its start
  (`guards::feed_reaches`: the first event ever is place 1, so a feed that starts later was
  pruned) the roster's `rescues` is null and the console says *how often the CEO stepped in this
  week: HQ cannot tell, the record has lost part of it*. Wakes are counted over the same week.
  *Amended 2026-10-19 (durability):* the ticks no longer count toward the 20,000. Nine events in
  ten were the tick's (one every 30 s), so the feed held about two days, and the week was never
  whole. HQ keeps the newest 20,000 events that are not ticks, everything older goes, and a tick
  goes too once it is not among the newest 20,000 of all, so the feed holds at most 40,000 rows
  and, at the live rate, weeks. `SqliteTeamStore::prune_events`; nothing is migrated.

The thresholds, the windows and the findings are as they were; nothing is migrated.

**Stored** in tables made if missing (`nudges`, `findings`, `closures`, `retractions`,
`note_parents`, `desk_attached`) and in HQ's settings (`serves:<task>`, `enough:<task>:<member>`,
`passed:<task>:<run>`, `guards_looked_at`, `guards_ceremony`, and, since the amendment,
`approval_serves:<desk item>`). No existing row changes, and no migration; the deploy
amendment adds one column to `runs`, `interrupted`, at none for every turn from before, added only
if missing.

## Consequences

- Fewer turns spent on talk, polish and nudges that say nothing new; one more look for the lead
  where HQ cannot tell, and a few more refusals a member must read and act on.
- A deploy that restarts HQ under a turn costs the member nothing the first time: one more start,
  and no finding, recovery wake or look from its lead. A run interrupted at every start is tried
  four times before it fails, not three.
- The detectors run every five minutes over the week's runs; at home-lab size this is small.
- A member's sub-task under any task with criteria must say which it serves, and since the
  amendment that includes the owner's tasks and a routine's: the splitting that was exempt is the
  splitting a lead does first, so it is now the most asked about. A refusal tells the lead which
  criteria there are; a part that serves none of them is not part of the task.
- The owner types one more line with every ask, at least one criterion, and a project's
  hand-it-work button asks for it. An ask with no line is refused with the way to give one, and
  the console keeps what was typed.
- An owner's routine (not the home lab's, not a standing review) is closed on a reading of what it
  is about, cited, where it was closed on a word before. The lead of the task's area may reword
  that criterion for one task with `narrow_criteria`.
- A request for the owner's yes costs the member a clause, and a request that is not tied to its
  task no longer costs the owner a card.
- A retracted claim's fingerprint is compared on every note, section and story written; a
  paraphrase that shares too few word pairs passes, which is the price of no model in the loop.
- The first month's precision is expected to be low; the verdicts are how it is learned.

**Deferred**: a screen for the owner to rewrite a task's criteria (the engine has it, and the
pass's binding holds for any edit); the house log's timeline lines and thread posts in the sweep
(only notes, sections and stories now); a "while you were away" digest of ceremony; a ceremony
budget per task that refuses (only once the measure shows it is needed); model-assisted
candidate matching for the sweep; the desk grouping for approvals (by design, not deferred); a
routine's own criteria, set by the owner when they make it (today it has its kind's: the form for
a routine does not ask, and the lead may reword one task's).

**Since built**: the "while you were away" digest ([0132](0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md)),
and the ceremony budget per task, tried in shadow first, with a tripwire that stops a runaway
team itself and a lead's tool to narrow criteria ([0133](0133-caps-on-talk-and-runaway.md)); and,
in the amendment above, criteria first.

## Alternatives considered

- **A model that diagnoses each finding.** Rejected: a model is never the enforcement boundary,
  and its word on a finding is the thing the verdicts measure.
- **Exact strings for retractions and the same ask.** Rejected: model text paraphrases, and an
  exact comparison of model output is a mistake this project has made.
- **Stricter thresholds from the start.** Rejected: a loose start with verdicts learns the real
  numbers; a strict one teaches the team to work around the guard.
- **A criterion for every routine, standing reviews too.** Rejected: closing a routine's task that
  has criteria asks for evidence per criterion, and a lead's report or the CEO's review is closed
  by filing it or deciding it, with no reading to cite; the criterion would be one more refusal for
  every daily turn and no more check on the work. Nothing splits one either.
- **A live default criterion for the owner's own routines**, "a fresh reading of the lab". Rejected:
  a routine need not be about the lab, and a task that cannot be closed keeps its routine from
  firing again (it has one open task at a time).
- **Criteria required of `add_task` itself, for the owner.** Rejected: it is the primitive HQ makes
  its own tasks with, and work already open is to stay as it is. The owner's front door is
  `add_task_with`, the one Give work and the API use, and that is where the rule lives.
- **The node checking `serves` itself.** Rejected: HQ holds the task's criteria and the node would
  have to ask for them to answer; HQ already refuses an approval it cannot take, and the node
  passes the reason on. One place decides.
- **Leaving the depth-0 exemption and asking leads to be careful.** Rejected: it was exactly the
  split that went wrong, and "be careful" is a request to a model, not a rule.
