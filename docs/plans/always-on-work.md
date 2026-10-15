# Always-on work: Endora keeps working when nobody asked

Proposed 2026-10-14; Phases 1–3 accepted by the owner 2026-10-15, recorded in
[ADR 0094](../adr/0094-work-it-finds-for-itself.md). AI-assisted. This plan
sequences the work and says what is proven versus assumed; the ADR is the
decision. Where they disagree, the ADR wins. Phases 4 onward are not accepted
and need their own records.

## The goal, in the owner's words

> Always running and working on something. If there is nothing to do, dig into
> things like email, Home Assistant, whatever it has to, to resolve issues and make
> my life easier. Do the things I would do but do not have the time to get to.

Three separable claims hide in that, and they have different costs:

1. **Always working** — there is rarely a moment with nothing in flight.
2. **Finding its own work** — nobody has to hand it the task.
3. **Doing what the owner would do** — closing things, not just reporting them.

Claims 1 and 2 are mostly wiring. Claim 3 is bounded by the reversibility bands
and is where honesty about limits matters most (see "What this will not do").

## Where things stand (verified in the tree on 2026-10-14)

**The execution engine is built.** `spawn_goal_worker`
(`app/node/src/api/workers.rs`) ticks every 5 s and advances one goal a bounded
pass at a time, under a lease with a 10 s heartbeat, through an
ownership-checking runner clamped to the reversible bands. Every way a goal ends
is announced once (ADR 0082); a console strip shows running work and a Stop
button (ADR 0083); at most three goals are active, enforced transactionally in
SQLite.

**Four lists of real problems are already derived by code** and nothing consumes
them:

| Source | What it knows | Read through |
|---|---|---|
| Standing trouble | house things that stopped answering, with since-when | `StandingTroubleRepository::troubles()` |
| Specimens | asks the butler failed, replayed nightly | `open_specimens()`, `gave_up_specimens()` |
| Repairs | capabilities that claim success and change nothing (ADR 0054) | `usecases::repairs()` |
| Notions | what it is wondering about, with a settling condition | notions store |

`gave_up_specimens()` was added for exactly this and **has no production caller.**

**The connector does not exist, and was deliberately removed.** The only
production path that creates a goal is the person's declared request
(`endora_begin_goal`, ADR 0081). ADR 0081 states it retired *failed-answer goal
creation* because "failed answers could silently become speculative goals." The
owner has since said they want this autonomous. These are in tension and the plan
reconciles them rather than ignoring either (principle 1 below).

**Two things that look like they would protect the person do not:**

- **Nothing caps the paid model.** ADR 0091 *meters* deep-model use; it says in
  its own context that an over-eager loop "would have been invisible until the
  provider's bill." Inside a turn, escalation is one call, `butler.deeper()`
  (`usecases/turn.rs`), and goal passes run through that same turn. A background
  goal can already reach the paid model, whenever escalation is on. (An earlier draft
  said this was observed in the live eval traces. It was not: the "escalating to the
  deep model" line printed whether or not a deep model existed, and the eval has none.
  That line now prints only when escalation is possible. The risk is from reading the
  code, not from a trace.)
- **The worker is not fair.** It claims goals oldest-updated-first
  (`ORDER BY updated_ms ASC`) with no notion of whose they are, and the three-goal
  cap is one shared pool. Self-started work would queue ahead of, and could
  exhaust the slots of, the owner's own requests.

## Constraints, each one earned

These come from things this repository already learned. They are the design.

1. **Work comes from derived records, never from a model's guess.** This is what
   answers ADR 0081's objection. A goal started because *a trouble has stood for
   six days* is arithmetic over stored state; a goal started because *the model
   thought something might be interesting* is the speculation 0081 retired. No
   classifier decides anything here (ADR 0076).
2. **The source is the verifier.** "Done" means the originating condition no longer
   holds, checked by code re-reading the same source: the specimen's replay passes
   the verdict, the trouble clears, the repair stops being derived. A model never
   declares its own work finished (ADR 0053/0077; `verify:*` measured 0/3–1/3).
3. **The owner's work always wins.** Separate lane, separate cap, claimed first,
   and self-started work yields at a pass boundary.
4. **Only when quiet.** Not while the owner is chatting, and not on top of a
   foreground turn. The check-in budget already encodes "never on top of someone
   who just spoke" (ADR 0056); the same rule applies to work.
5. **Local model only.** Self-started work never reaches the paid model. Enforced
   structurally (a butler whose `deeper()` is `None`), not by a setting someone
   must remember.
6. **Gather and reversible only.** The existing reversible-only runner stays. No
   new unattended actuation in this plan; the precedent for acting without being
   asked is `withdraw_what_never_works`, whose own doc lists why it is safe
   (only turns something off; high bar; one click to undo; said out loud), and
   anything new has to meet that bar.
7. **Speak rarely.** Continuous working is not continuous talking. ADR 0056's
   budget governs speech; self-started results go to the activity trail and the
   next brief, and to chat only when they need the person.
8. **Bounded per item.** Attempts per item, a cooldown after it ends, and
   suppression after the owner says "it's fine" or presses Stop. Otherwise a
   permanently broken thing is re-investigated forever.
9. **Nothing to do means doing nothing.** If no source yields work, it idles. It
   does not manufacture an objective to look busy; that is the failure the
   reset plan names.

## Design

### 1. Goals record where they came from

Add provenance to a goal record: `GoalOrigin::Person` (today's behaviour, and what
every existing row is) or `GoalOrigin::Record`.

*Revised 2026-10-15, when Phase 1 was built.* The first draft put the source and
reference in the checkpoint (version 5). Reading the code showed two problems with
that. `Goal::from_checkpoint` reads `criteria` only when the version is exactly 4,
so a version bump would have silently dropped every goal's completion criteria on
load. And origin is an immutable fact about the *record* (like its request key),
not part of the goal's mutable state. So it is a column on the record, additive
(`origin TEXT NOT NULL DEFAULT 'person'`, following the repository's existing
additive-`ALTER` precedent), because the lanes and claim order have to be
expressible in SQL. The checkpoint is untouched. Which record a goal came from
(`trouble`, `specimen`, `repair`, later `mail`) travels with its completion
criterion in Phase 3, where it is first needed; Phase 1 records the lane only. An
origin that cannot be read is treated as `Record`, the lane that never outranks a
person's request.

### 2. Two lanes, owner first

| | Cap | Counts toward | Claimed |
|---|---|---|---|
| Person | 3, unchanged | person goals only | first |
| Record | 1 | record goals only | only if no person goal is claimable |

The cap query changes from "all active" to "active in this lane"; the claim query
orders by lane, then age. This is the riskiest change in the plan (it touches the
shared worker the owner's requests depend on), so it ships first and alone, with no
new behaviour (Phase 1).

### 3. A criterion whose completion code can check

`GoalCriteria::Resolve { source, reference }` beside `Schedule` and `Research`.
Completion re-reads the source:

| Source | Done when |
|---|---|
| Specimen | a replay passes `specimen_verdict` (the rule that filed it) |
| Trouble | the item is gone from `troubles()` |
| Repair | the finding is no longer derived from outcomes |

If the budget runs out unresolved, the goal ends `Partial` with what it found,
which is a useful result rather than a failure: *"I looked into the garage sensor;
it has not reported since the 9th and nothing I can read explains it."*

### 4. The selector: one pure function

`next_work(troubles, specimens, repairs, suppressed, now) -> Option<WorkItem>`.
Priority, then age; skips anything resolved, suppressed or inside its cooldown;
returns `None` when there is nothing, which is a normal outcome. Pure, so every
rule above is a unit test and none needs a model.

Proposed priority: **standing trouble first** (a real thing in the house is
wrong, with a clock), then **specimens that gave up** (a proven gap, ADR 0075),
then **repairs**. Notions are deferred: they are the most speculative source and
the least checkable.

### 5. The gate: when a tick may start work

All must hold, evaluated by code on a slower cadence than the 5 s worker tick
(proposed: every 5 minutes):

- no person-lane goal is claimable;
- the turn lock is free;
- the record lane is empty;
- no chat message in the last *N* minutes (proposed 10);
- *M* minutes since the last self-started goal ended (proposed 30).

*N* and *M* are placeholders to be argued from real use, not constants to ship.

*As built in Phase 2, 2026-10-15.* The gate has two halves, and only the first
exists yet. **Claiming** a self-started goal waits for the quiet window, every
worker tick: a pass on such a goal begins only when the owner has not written in
chat for *N* minutes (`record_lane::quiet_enough`, applied in the worker's claim).
Because the cap is one and a pass is bounded, this is also the pass-boundary yield
S1 asks for. It is checked on the 5-second tick rather than every 5 minutes: it is
a read of the last few minutes of chat, and checking sooner only means stepping
aside sooner. Only the owner's own messages count (Endora's briefs and check-ins
do not), a message dated in the future counts as speech, and a chat that cannot be
read counts as not quiet. **Starting** one (empty lane, cooldown *M*, the 5-minute
cadence) belongs to the selector and arrives with it in Phase 3, because nothing
yet decides to start anything. There is no flag: with nothing able to create a
self-started goal, a flag would guard nothing. The switch that matters is at
creation, and Phase 3 puts the selector behind one.

### 6. The cost clamp

A thin `LocalOnly` wrapper over the butler whose `deeper()` returns `None`, used
for every record-lane pass. The test is a spy: a butler that counts every time
`deeper()` is consulted, driven through a full pass, beside the same spy on a
person's goal, which must reach for it (otherwise the first test proves nothing).

*As built in Phase 2:* the clamp is inside `resume_goal`, the one place a goal pass
runs, and chooses by the record's origin. No caller (the worker, the eval harness,
a future selector) can forget it, and a person's goal is untouched.

### 7. Speech

Record-lane goals do not post a chat message when they end. They write to the
activity trail and are folded into the next brief ("while you were out, I looked
into…"). They post to chat only when they need the owner. This amends ADR 0082's
"every goal that ends is announced" for this lane, and the ADR has to say so.

## Phasing

Each phase ships on its own, passes the full gate, and is deployed and observed
before the next. The reset plan's rule applies: a phase is not a capability until
it has intake, execution, visible progress, a result, failure behaviour and
acceptance evidence.

| # | Phase | New behaviour? | Acceptance |
|---|---|---|---|
| 0 | Decide the open questions below; write ADR 0094 | no | owner answers recorded |
| 1 | Provenance, lanes, claim priority | **none** | S1, S6; existing goal suite and `make goal-eval` unchanged |
| 2 | `LocalOnly` clamp and the quiet gate, behind a flag | none yet | S2, S3 |
| 3 | Selector + `Resolve` for **standing trouble only** | yes, first source, off unless `ENDORA_OWN_WORK=1` | S4, S5, S7 (trail), S8, S9 |
| 4 | Specimens, then repairs, and **intentions folded in** (a belief as the last source; the nightly loop's own pursuit and stumper replay retired in the same edit) | yes | S4–S8 per source |
| 5 | Fold results into the brief | yes | S7 against a real brief |
| 6 | Mail as a source (own ADR; prerequisites below) | yes | own scenarios |
| 7 | Acting: drafts first, then anything needing an opened capability | yes | own ADR |

**What Phase 4 retires:** the nightly loop's "replay one stumper" step
(`usecases/notions.rs`), which does the same job once per night, one item. The
same item would otherwise be worked twice. (It moved from Phase 3 when standing
trouble, not specimens, was chosen as the first source: Phase 3 has nothing to
overlap.) No new worker and no new store: the
record lane reuses the existing one. That is the answer to the reset plan's
"do not maintain two autonomous brains."

*Phase 3, as built (2026-10-15).* `WorkSource` (an enum, so specimens, repairs and the
belief source are additions), `GoalCriteria::Resolve`, and `own_work` (the pure start
decision, the source re-read, the trail line). The worker re-reads the source before each
pass and posts a self-started goal's ending to the activity trail, not chat. A starter
loop, run only with `ENDORA_OWN_WORK=1`, makes the goals. The five-minute cadence the
gate asks for is real here, unlike the claim-time check in Phase 2. The brief half of S7
is Phase 5. Not yet observed on a live node.

*What a live model did with it, 2026-10-15.* Three runs of the goal against the local
model, recorded because each changed the design. With the web available it searched for
generic advice and named a radio protocol the device was never said to use. With the web
removed but Wikipedia left, it did the same from an encyclopedia and the invention was
kept as a finding. With only the house allowed (`HOUSE_SKILLS`) and an unread cause
counted as a dead end, it ended Blocked in 22 seconds with "I found nothing usable". What
is **not** known: what it writes when Home Assistant can actually be read, because the
eval composition has no Home Assistant. That is the first thing to look at on the live
node.

## Acceptance scenarios

Synthetic, no personal data, each a test or a live check.

| # | Scenario | Pass condition |
|---|---|---|
| S1 | A person request arrives while a record goal is running | the person goal is claimed next tick; the record goal yields at a pass boundary |
| S2 | The owner chats during idle work | no record pass starts for *N* minutes |
| S3 | A deep model is configured | a record pass never calls `deeper()` (spy) |
| S4 | A specimen's replay passes | the goal completes with no model verdict; one trail line |
| S5 | The same trouble recurs, or was marked "it's fine" | not restarted inside its cooldown; an accepted one is never started |
| S6 | Three person goals are active | the record lane is unaffected by that cap, and holds one |
| S7 | A record goal ends | no chat bubble; present in the trail and the next brief |
| S8 | The owner presses Stop on one | it is cancelled and not recreated until its cooldown passes |
| S9 | No source yields work | nothing is created; the node idles |

S1, S3 and S9 matter most: they are the ones that fail silently if wrong.

## What this will not do

Said plainly, because "do the things I would do" is the part most likely to be
over-promised.

- **It cannot send anything unattended, ever.** Sending, spending and deleting
  external state are `Irreversible`, which is blocked unprompted at any
  confidence (`shared/kernel/src/reversibility.rs`). Email replies are drafts.
- **Sorting mail is not free.** Labelling or archiving reaches outside the box
  (`OutwardReversible`): it needs the capability opened by the owner and the
  autonomy envelope widened. Until then, "sort and see what is important" means
  *rank and surface*, not *file*.
- **Reading mail makes the turn third-party** (ADR 0064): a pass that has read an
  email loses the ability to act in that turn, by design.
- **"Dig into email when idle" needs a source that does not exist yet.** Today the
  mailbox yields a count. Phase 6 prerequisites, from the earlier review:
  1. teach the egress disguise the mail shape *first*, since `personal_values_in`
     recognises only two line shapes and subjects/senders would otherwise reach a
     third party undisguised;
  2. a Home Assistant template sensor exposing `subject`/`sender` (the skill
     already reads those attributes);
  3. a *list*, not only the latest message, which the HA sensor does not give and
     which Endora cannot fetch without holding a mail credential, a real trade;
  4. "important" decided by deterministic signals (sender known, thread replied
     to, unread), not by asking the model which emails matter.
- **A 7b model is the ceiling.** The battery puts it at 38.7/54, and the
  `verify:*` cases, where it must respect a read-back over its own claim, never
  pass more than one run in three (`docs/roadmap.md`). The design routes around
  that (the source is the verifier) but cannot raise it.

## Decisions

Answered by the owner on 2026-10-15, accepting each recommendation; recorded in
[ADR 0094](../adr/0094-work-it-finds-for-itself.md).

1. **The expansion freeze: lifted for Phases 1–3 only.** `make goal-eval` must not
   regress across Phase 1, and the freeze returns after Phase 3 until S1–S9 pass on
   the live node.
2. **First source: standing trouble**, then specimens, then repairs.
3. **Where results appear: the activity trail and the next brief.** Chat only when
   it needs the owner.
4. **Paid model: none.** Self-started work is local-only, enforced in code.
   Revisit a budget only after a month of use shows the local model is the limit.
5. **Starting numbers:** quiet window 10 minutes, cooldown 30 minutes, gate cadence
   5 minutes, tuned after a week watching the trail.

6. **Intentions are folded into the record lane** (answered 2026-10-15, after the five
   above). The nightly loop's own pursuit becomes the lane's lowest-priority source, a
   belief, and the loop stops keeping one of its own. It cannot be verified by its
   source, so it ends `Partial`, never `Complete`. It lands in Phase 4 with specimens,
   in the same edit to the nightly loop, not in Phase 3; the full reasoning and limits
   are in [ADR 0094](../adr/0094-work-it-finds-for-itself.md#intentions-are-folded-in).

## Risks

- **Three durable-work mechanisms already exist** (intentions, goals, research
  assignments). A fourth is the failure the reset plan names. This plan adds a
  *lane*, not a mechanism, and Phase 3 retires the one it overlaps; decision 5
  decides the rest.
- **Phase 1 touches the shared worker.** The owner's requests run on it. It ships
  alone with no new behaviour and a before/after `make goal-eval`.
- **ADR 0081 was a considered reversal.** This plan reinstates automatic goal
  creation under a different trigger. 0094 has to amend 0081 explicitly and say
  why derived records are not the speculation 0081 retired.
- **Unreviewed neighbours.** This worktree has uncommitted changes from other work
  (`homelab`) and the goal runtime itself has a standing review
  (`goal-handoff-review.md`). Neither is touched by this plan.
- **Idle is shared hardware.** One GPU serves chat and background work. The quiet
  gate and one-item lane exist for this; whether they are enough is a Phase 3
  measurement, not an assumption.

## Not in this plan

Notions as a source; unattended actuation of any kind; a new worker, store or
screen; any change to the person-lane goal behaviour; mail beyond its
prerequisites.
