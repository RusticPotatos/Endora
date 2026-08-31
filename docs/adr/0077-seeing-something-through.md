# 0077 — Seeing something through

## Status

Proposed (2026-08-30). Design-first, for the same reason
[0071](0071-capabilities-it-writes-itself.md) was: this record lets the person's
own ask become work Endora carries, and
[0029](archive/0029-delete-the-goal-tracker.md) deleted a queue of records the
person had to groom. The distinction has to survive scrutiny before code, not
after. No code until Accepted.

## Context

Observed in use, and the plainest statement of the problem is the person's own:
*a goal set in Endora is not pursued — it just stops.* Read against the tree
(2026-08-30) that is not a feeling. It is three mechanisms, each independently
verifiable.

**The person's ask never becomes something pursued.** The nightly loop's
`take_up_an_intention` calls `nightly_focus`, which picks the highest-confidence
belief — `Intent` kind first, otherwise any — and forms an intention from it.
What the person actually asked for is nowhere in that path. `drop_intention`
states the rule outright: *"there is no path by which the person can create or
edit one."*

**Nothing can finish.** `IntentionState::Done` exists and `Intention::complete()`
exists; their only two callers in the repository are tests. No production path
sets `Done`. Every intention therefore ends `Abandoned` — dropped by the person,
gone stale after a fortnight, or spent, retiring with *"gave it seven nights
without getting anywhere."* The type can express success. The system cannot reach
it.

**Retiring discards the thread.** With nothing active the following night,
`take_up_an_intention` re-picks the strongest belief — very likely the same one,
since confidence does not move quickly — and `Intention::form` starts it with an
empty note. Nothing consults the prior intention on that belief. So a goal does
not continue; it restarts from zero, having forgotten that it already spent a
week.

Together these explain the observation exactly: what the person asked for was
never taken up, what *was* taken up could only ever be given up on, and giving up
erased the record of having tried.

There is a fourth thing, about pace rather than correctness.
[0056](0056-how-it-behaves-toward-you.md) made the check-in schedule a **budget**
so Endora could not nag — right, and a rule about *speaking*. But the nightly
loop then also became when it *works*: one step, once a night, seven nights
total. The work clock and the speech clock are the same clock, and only one of
them was ever argued for.

## Decision (proposed)

### An intention traces to why it is pursued — a belief, or your asking

[0052](0052-what-it-knows-about-you.md) required `motivating_belief` to be a
`BeliefId`, never optional, *"so Endora cannot pursue what it cannot explain."*
Keep the constraint exactly; widen what counts as an explanation. Provenance
becomes either a belief Endora formed or **because you asked** — and "you asked
me to" is an explanation at least as good as a belief, since it is the only one
the person can check without reading the model's mind.

One record, one store, the same bands. Not a second kind of object living beside
the first.

**One active slot per provenance — yours never waits behind Endora's.** The
first draft of this record kept 0052's flat "at most one active at a time" and
called it a virtue. Read against the code that is a defect: `take_up_an_intention`
runs only when `intentions.active()` is `None`, so an ask arriving while Endora
pursues a belief of its own would have had nowhere to go for seven nights — or
fourteen days with the loop off. The complaint this record exists to answer would
have changed from *"I set a goal and it stops"* into *"I set a goal and it never
starts."*

So the bound moves from one to **one of each**: at most one belief-derived
intention and at most one you asked for. Still a cursor, not a backlog — two is
bounded, nothing accumulates, and no one has to groom anything. Where they
compete for a step, **yours goes first**; Endora's own thread keeps its note and
resumes after, which is the continue-rather-than-restart rule already stated
below.

Asking for a second thing while one of yours is running does not queue it. Endora
says what it is on and asks whether to swap. That is one question at the moment
you ask, which is the opposite of a backlog that asks forever.

### An ask becomes work only when you say so

How a message turns into something pursued was missing from the first draft, and
the obvious filling — let the model recognise ongoing work — is precisely what
[0076](0076-standing-questions.md) refused: *"The moment an entry needs a model to
decide whether it matches, it is not a standing question."* A classifier deciding
silently that you commissioned something would manufacture work you never asked
for, and the record would carry your name on it.

So nothing is created by classification. **The butler may offer; only your yes
creates.** When a turn looks like ongoing work, it asks — *"shall I keep on
this?"* — and an intention exists only after you agree. This keeps the ordinary
spine (models propose, policy authorizes) and makes the failure cheap in the
right direction: a wrong offer costs one sentence you ignore, where a wrong
classification costs a week of misdirected nights.

No closed phrase list, no matcher, no second routing surface. One rule, and it is
checkable by reading it: if an intention with your provenance can come into
existence without an answer from you, this is not what shipped.

### Finishing is a state it can reach, and settling is stated in advance

**Settling is your verdict** ([0066](0066-their-verdict-decides-too.md)), with the
step budget as the backstop that stops anything waiting on you forever. That is
the whole of it, and the first draft of this record claimed more than that.

It said an intention would carry what would settle it *"in the shape notions
already use (`settles_when`)"*, and that *"where the settle condition is derivable
by code, code decides."* Checked: `Notion::settles_when` is a `String`, written at
formation, stored, read back, and **evaluated by nothing**. There is no machinery
in this repository that checks a settle condition, so citing it as precedent
described a mechanism that does not exist and this record does not design. A
guarantee resting on a mechanism nobody built is the failure mode
[0053](0053-honesty-about-what-it-did.md) is about, and it does not get to appear
in the record that cites 0053.

An intention may still carry a settling condition, with one restriction that
closes the loophole the first draft left open: **the condition is your words, or
there is none.** Where the intention came from your ask, it is captured from how
you put it — *keep on this until the council publishes the minutes.* Where it came
from a belief, there is no condition and settling is your verdict alone.

The model never authors it. A model that writes the bar it will later be measured
against is marking its own homework one step removed, and the ordering safeguard —
condition before work — does nothing about that. This is the same line 0053 drew,
against the same measurement: `verify:*` at 0/3 and 1/3 on respecting an explicit
instruction about verification.

The retirement message stops asserting a conclusion it does not hold. *"Gave it
seven nights without getting anywhere"* is a claim about the world; what the
system actually knows is that a budget ran out.

### Running out of budget continues rather than restarts

Work taken up again inherits what the last attempt learned, and a motivation just
retired is not immediately re-taken — a guard against the churn today's code
allows, where the strongest belief can be re-formed into a fresh empty intention
every night forever.

### Work may progress more often than Endora speaks

Split the clocks. 0056's budget keeps governing **speech** and is untouched.
Progress becomes something that may happen when there is an opportunity —
including inside a chat turn, while the person is there. When something finishes
and the person is present, it says so into the same conversation rather than
waiting for the morning note.

**This grants no new authority.** Continuity is not permission. Unattended
progress stays clamped by `ReversibleOnlyRunner`, the reversibility bands do not
move, irreversible stays blocked unprompted at any confidence, and nothing here
touches the roadmap's step D. The claim being made is narrow and worth stating
plainly: most of what reads as presence is *being mid-task*, not *being allowed
more*.

## What this retires

Per the pattern budget's rule that a mechanism-adding record names what it
supersedes:

- **0052's belief-only provenance** for intentions — superseded by provenance
  that keeps the must-be-explicable constraint and widens what explains.
- **0052's flat "at most one active at a time"** — superseded by one slot per
  provenance, for the reason argued above: applied flatly it made the person's own
  ask wait behind Endora's.
- **The blanket "seven nights means failure" retirement** — replaced by your
  verdict plus a budget that reports only what it knows.
- No new record type, no new store, no new screen. The console's existing
  intention view gains a provenance line and, where there is one, a settle line.

0052 is amended in place in the same change, per the practice this repository
already follows: a superseded clause that still reads as current in its own record
is how a rule gets cited against itself later.

## What this is not

- **Not the queue [0029](archive/0029-delete-the-goal-tracker.md) deleted.**
  That queue held the model's *guesses about the person* — items only the person
  could adjudicate, so it nagged by construction. This holds work the person
  asked for in their own words: they authored it, and there is nothing to groom.
  It is the same distinction 0071 drew for the proposal card, and it is
  checkable. **If the console grows an "add task" form or a count badge, this
  record failed.**
- **Not a task manager.** Two slots, both self-retiring, and no third. The
  person's verbs stay what they were plus one: ask, and drop.
- **Not more autonomy.** The clocks split; the bands do not move.

## Consequences

- Endora can say *done*, which today it cannot, at all, by any path. That is the
  difference between a colleague and a diary.
- A goal the person sets and then forgets still retires itself. Nothing waits for
  them, which is the property 0029 was protecting.
- **The risk, named honestly:** person-authored work is the closest this project
  has come to a task list since 0029, and drift toward one is the failure mode.
  The tripwire above (add-form, count badge) is the thing to check at review.
- The eval battery gains cases for settling: a model that declares its own work
  done is precisely what to measure, and the `verify:*` results say to expect it
  to try. Add one for the offer, too — a butler that offers to keep on everything
  is as useless as one that never offers.
- **Two constants stop meaning what they say.** `STEP_BUDGET = 7` is documented as
  *"Nights of work"*, and `STALE_AFTER_MS` is a fortnight; both were sized against
  a loop that ran once a night. Split the clocks and seven steps could be seven
  hours. Neither number survives the change on its own, and the cadence work below
  has to re-derive both rather than inherit them.
- **No new authority is not no new cost.** Progressing more often is more model
  calls on hardware that has one card, and in-turn progress competes with the
  person's own latency on a 7b model. So progress inside a chat turn happens
  *after* their reply is delivered, never before it — the person's turn is not the
  place to spend a budget on Endora's own work.

## What acceptance needs

Grounded against the live record, not invented — the standard
[0061](0061-answers-worth-keeping.md) set, and the one 0071 was held to:

1. The **console surface**, designed as lines on the existing intention view and
   checked against the nothing-to-groom tripwire.
2. The **progress cadence**, argued as a number: what "an opportunity" is, so
   splitting the clocks does not quietly become a busy loop — and with
   `STEP_BUDGET` and `STALE_AFTER_MS` re-derived against whatever it turns out to
   be, since both are nightly numbers today.
3. The **offer's wording and its restraint**, against real turns from the live
   record: which asks should draw *"shall I keep on this?"* and, more importantly,
   which should not. This is the one place a model still judges, and the thing to
   check is that a wrong judgement stays one ignorable sentence.

## Rejected

- **A todo list or project tracker.** The thing 0029 deleted, and the failure
  mode this record is most likely to decay into.
- **Letting the model mark its own work done.** Measured at 0/3 and 1/3 on
  respecting an explicit verification instruction. Rejected in both its obvious
  form and its disguised one: a model that authors the settle condition has
  chosen the bar it will be judged against, which is the same thing wearing the
  ordering safeguard as a hat.
- **Letting a classifier decide you commissioned something.** The filling this
  record's first draft left blank, and the thing 0076 refused. Work with your name
  on it that you never agreed to is worse than no work at all.
- **Answering "it doesn't feel agentic" with more authority.** Measured against
  the actual defect, it cannot *finish* and cannot *continue* — neither is a
  permission problem, and widening the bands would have fixed neither.
- **Several goals at once.** Two bounded slots keep this a cursor rather than a
  backlog. A third would be a queue with better manners.

## What review changed

Kept because the reasoning that produced a rule is the reason it survives an
argument later. This record was reviewed against the code before acceptance, and
five things in the first draft did not survive it:

1. **The flat one-active rule made the person's ask wait** up to seven nights
   behind Endora's own — the record would have failed at the thing it exists to
   fix. Now one slot per provenance.
2. **How an ask becomes work was simply missing**, and the obvious filling was
   open-ended intent classification, which 0076 had already refused. Now the
   butler offers and only a yes creates.
3. **The `settles_when` precedent was overstated.** It is a `String` nothing
   evaluates; the draft cited it for a code-decides guarantee that has no
   machinery behind it. Now settling is the person's verdict, stated as such.
4. **The model could have authored its own bar**, which the ordering safeguard
   did nothing about. Now the condition is the person's words or absent.
5. **Two nightly constants and the cost of splitting the clocks** were unnamed.
   Now both are consequences, and in-turn progress is placed after the reply.
