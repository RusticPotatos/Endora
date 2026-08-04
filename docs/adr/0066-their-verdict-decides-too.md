# 0066 — Their verdict decides too

## Status

Accepted (2026-08-04). Narrows [0062](0062-one-permission-surface.md)'s graduation with a
second condition, and applies [0065](0065-a-place-is-not-the-models-to-remember.md)'s lesson
one layer up: a fact the system already holds, narrated to the model instead of deciding
anything.

## Context

[0062](0062-one-permission-surface.md) lets the record graduate a tool: three outcomes where
read-back **saw the world change** earn it the right to act without asking. That threshold is
right about the thing it measures, and it only measures one thing.

**Read-back proves the world changed. It cannot prove the change was wanted.**

A tool can reliably switch a light off, be confirmed three times, graduate, and go on
switching that light off in a house where nobody wanted it off. Every check in the graduation
path would pass, because the only question any of them asks is whether something moved.

The person's own verdict was already being collected, from two directions:

- the console has helped / didn't help / made no difference on every outcome
- **repeating an ask marks the actions before it as unhelpful**, with nobody pressing
  anything — the strongest signal in the system, because it costs the person nothing to give

And it went nowhere. `track_record` folds those counts into a sentence in the prompt — *"skill
X — didn't help 4 time(s)"* — and a 7B model reads it alongside everything else. Meanwhile
`proven_by_the_record`, which decides whether that same tool may act unasked, looks only at
`changed`.

So the person could say *that didn't help* four times and the tool kept its autonomy, because
their verdict was advice to a model and the read-back was policy. That is the mistake
[0065](0065-a-place-is-not-the-models-to-remember.md) named yesterday, wearing different
clothes: the system holds the fact, and hands it to the model rather than acting on it.

## Decision

### Three confirmed changes grant it; three net unhelpful verdicts take it back

A tool stays graduated only while the person has not, on balance, said it does not help. The
test is `did_not_help >= PROVEN_AFTER && did_not_help > helped`.

Symmetric with the grant deliberately. The same count that was enough to trust a tool is
enough to stop trusting it, and there is no second number to explain or tune.

**Net, not merely present.** A tool somebody is mostly happy with keeps acting through a few
marks against it. Requiring the balance to tip means the rule reads the person's opinion
rather than the worst moment they had with it.

**Three, not one**, because the repeat-ask path is coarse: it marks *every* successful action
in its window, so an unrelated tool that happened to run while somebody rephrased a question
collects a mark it did not earn. A single-strike rule hung on that instrument would revoke
working tools for being nearby.

### Made no difference is a third answer, not a middle one

`NoReaction` counts for neither side. *I saw it and it changed nothing for me* is information
about that moment, not about whether the tool works, and averaging it in would put a thumb on
a scale it is not on.

### Derived, never stored

The same property the grant already had. There is no withdrawal to record, no flag to clear
and nothing to administer: delete the marks and the tool is trusted again, purge everything
and every tool goes back to asking.

### The person is told

A tool that quietly stops acting on its own is the kind of change
[0053](0053-honesty-about-what-it-did.md) says somebody hears about rather than notices, so
the scorecard names it: *"X earned acting on its own and you took it back."*

## Consequences

- **Autonomy now answers to the person, not only to a sensor.** The strongest available signal
  about whether an action was wanted finally reaches the decision it is about.
- **A tool can lose autonomy without anybody choosing to revoke it**, by being repeatedly
  present when somebody had to ask twice. That is the cost of the coarse instrument, bounded
  by requiring three and a net balance, and it fails in the safe direction: back to asking.
- **Nothing changes today.** All thirty outcomes on this install carry no reaction at all, so
  no tool is withdrawn yet. The rule exists for when the repeat-ask path starts producing
  marks, which it will without anyone opting in.
- **A person could suppress a working tool** by marking it unhelpful. Intended — that is what
  the buttons are for, and the tool still runs when asked. Only the unasked acting stops.

## Rejected

- **Letting "helped" graduate a tool faster.** The inverse is not symmetric in risk: pressing
  a button should never *lower* the bar for acting without permission. Read-back stays the
  only thing that can grant.
- **Weighting the ratio instead of counting.** A ratio invites gaming by volume and reads as
  tunable — the same reason [0062](0062-one-permission-surface.md) chose a count.
- **Withdrawing on one mark.** Correct if the marks were deliberate, wrong given that most
  will be inferred from a rephrased question.
- **A separate revoked list the person manages.** A stored flag to set, clear, migrate and
  forget, replacing a rule that derives itself from records that already exist.
- **Leaving it in the prompt and asking the model to weigh it.** What is already happening,
  and what produced this record.
