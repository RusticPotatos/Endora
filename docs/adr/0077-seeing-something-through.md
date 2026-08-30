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

One record, one store, one active at a time, the same bands. Not a second kind of
object living beside the first.

### Finishing is a state it can reach, and settling is stated in advance

An intention carries **what would settle it**, in the shape notions already use
(`settles_when`), written down when it is formed. That ordering is the whole
safeguard: "finished" is checked against a condition stated *before* the work,
never adjudicated afterwards by the model that did the work.

Where the settle condition is derivable by code, code decides. Where it is not,
the person's word decides ([0066](0066-their-verdict-decides-too.md)), and until
one of those happens it stays active. **The model does not get to declare its own
work done** — the same line [0053](0053-honesty-about-what-it-did.md) drew about
honesty, for the same reason and against the same measurement.

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
- **The blanket "seven nights means failure" retirement** — replaced by a settle
  condition plus a budget that reports only what it knows.
- No new record type, no new store, no new screen. The console's existing
  intention view gains a provenance line and a settle line.

## What this is not

- **Not the queue [0029](archive/0029-delete-the-goal-tracker.md) deleted.**
  That queue held the model's *guesses about the person* — items only the person
  could adjudicate, so it nagged by construction. This holds work the person
  asked for in their own words: they authored it, and there is nothing to groom.
  It is the same distinction 0071 drew for the proposal card, and it is
  checkable. **If the console grows an "add task" form or a count badge, this
  record failed.**
- **Not a task manager.** Still at most one active, still self-retiring. The
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
  to try.

## What acceptance needs

Grounded against the live record, not invented — the standard
[0061](0061-answers-worth-keeping.md) set, and the one 0071 was held to:

1. The **settle-condition vocabulary**, designed against real examples of things
   this house has actually asked for.
2. The **console surface**, designed as lines on the existing intention view and
   checked against the nothing-to-groom tripwire.
3. The **progress cadence**, argued as a number: what "an opportunity" is, so
   splitting the clocks does not quietly become a busy loop.

## Rejected

- **A todo list or project tracker.** The thing 0029 deleted, and the failure
  mode this record is most likely to decay into.
- **Letting the model mark its own work done.** Measured at 0/3 and 1/3 on
  respecting an explicit verification instruction; a settle condition stated in
  advance exists because that number does.
- **Answering "it doesn't feel agentic" with more authority.** Measured against
  the actual defect, it cannot *finish* and cannot *continue* — neither is a
  permission problem, and widening the bands would have fixed neither.
- **Several goals at once.** The one-at-a-time rule is what keeps this a cursor
  rather than a backlog.
