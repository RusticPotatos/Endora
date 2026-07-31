# 0052 — What it knows about you

## Status

Accepted (2026-07-26). **Consolidates 0006, 0011, 0013, 0014, 0015, 0016, 0020, 0029, 0032,
0033 and 0036**, which are archived. Includes one large reversal, kept because it is the
most instructive thing in this repository.

## Context

Endora began with a **tree**: Value → North Star → Target → Experiment → Reflection, plus
suggestions awaiting approval. It was coherent, it was implemented, and it was wrong — a
project-management tool wearing a butler's clothes. It asked the person to maintain a
hierarchy, and a queue of proposals accumulated for them to process.

[0029] deleted the entire thing. What replaced it is smaller and does more.

## Decision

### Understanding is the only model

Endora holds **beliefs about the person** — statements with evidence, a kind, and a
confidence — and nothing else. No goals, no targets, no north stars, no experiments, no
approval queue. If something cannot be expressed as "here is what I think is true about you,
and here is why", Endora does not track it.

### Conversation is the interface; the model is internal

The person talks. Understanding is formed from the conversation and is visible and
correctable, but they are never asked to curate it. Every belief can be seen, affirmed or
corrected — memory rights, not a maintenance task.

### What understanding admits

- **Instructions are not beliefs.** "Turn off the light" is a command, not a fact about the
  person; treating it as one filled the model with noise.
- **Contradictions are kept apart, not merged.** Two conflicting statements are two beliefs
  with their own evidence. Averaging them loses the fact that the person changed their mind.
- **Confidence comes from evidence**, not from the model's enthusiasm.

### Beliefs decay and expire

A belief that is never re-evidenced loses confidence over time and eventually expires.
Without this, understanding is a ratchet: everything ever inferred stays true forever, and
the person is permanently characterised by a passing remark. Decay is what makes it a
*model* rather than a transcript.

### One intention at a time — a cursor, not a queue

Endora may hold **one active intention**: work that outlives a turn, traceable to a belief
that motivates it, with a step budget and a staleness horizon so it retires itself. The
person's only verb is *stop*.

There is deliberately **no "add"**. A console that let you file work would rebuild [0029]'s
goal tracker with a new name.

**A night that produced no answer is not a night's work.** The one intention Endora has ever
held — *"you want to run more often"* — reached five of its seven steps with its stored
progress reading:

```text
Sorry — I couldn't reach my language model just now, so I didn't follow that properly.
```

An apology is non-empty, so it counted as a step, was written down as what happened, and was
handed to the next night as the thread to pick up. Two things were being spent on nothing: the
**step budget** that makes an intention retire itself rather than rot, and the **only record**
of what Endora has been doing about the thing it decided to care about.

The same predicate every other path already uses settles it — the notion of "nothing" that
[0053](0053-honesty-about-what-it-did.md) had to get right for chat turns applies unchanged to
a night. It is the third place that notion has had to reach: a chat retry, an unprompted
message, and now progress on work that outlives a turn.

### A settled belief stops asking

Every belief card offered *That's right / Not quite*, forever, regardless of how sure Endora
was or how many times the person had already confirmed it. One screen held a dozen
high-confidence, repeatedly-affirmed cards, all still asking. That is the queue this document
exists to prevent, wearing a different costume: a list of chores that never empties, because
answering an item does not remove it.

A belief that is **held with high confidence and has been reinforced at least once since it
was formed** is settled, and settled beliefs are not asked about.

The asymmetry is the whole rule. **Affirming a settled belief adds nothing** — confidence is
already at the top — so the prompt is pure cost. **Correcting one always matters**, however
sure Endora was, so correction stays available on every belief, always. Settledness is judged
on the *decayed* confidence, so a belief that fades becomes a question again on its own:
what buys silence is being sure now, not having once been sure.

**Contradiction defeats settledness.** Live, immediately after the above shipped: *you
prefer temperatures in Fahrenheit* and *you find it more convenient and accurate to measure
temperature in Celsius* — both high-confidence, both reinforced, both now silent. Endora is
definitely wrong about one of them and only the person knows which, so that is the case
where a prompt has the clearest possible payoff. A belief that disagrees with another active
belief is never settled, and the disagreement is shown on **both** cards so neither is
quietly treated as the winner.

That check cannot live on a belief, because contradiction is not a property of one.

**And a contradiction requires a shared subject.** Putting disagreement on screen immediately
showed that it was decided from negation and antonyms alone, with nothing asking whether the
two statements were even on the same topic: the Celsius belief was reported as contradicting
*every* other belief Endora held, including one about where it found some event listings.
That fault had existed all along and was invisible, because the answer only ever fed
duplicate detection — which independently required word overlap, and so masked it.

The bar for "about the same thing" is deliberately much weaker than the bar for "the same
belief": *you like running* and *you hate running* are about the same thing without being
the same belief, and that gap is exactly where contradiction lives.

### Its own behaviour is not evidence about the person

The sharpest violation of *"only facts about the person"* found so far, sitting at the top of
the understanding screen:

```text
you find it more convenient for the assistant to wait for instructions
  because I didn't do anything proactive since I rely on your instructions.
```

Endora had been passive because its direct reach into the house was broken, and concluded from
its own conduct that the person prefers passivity — then carried that into every later turn as
something it knew about them.

**A butler that mistakes its own behaviour for evidence reinforces whatever it happens to be
doing, including its own faults.** That is a feedback loop with no input from the person at
all, which is the opposite of understanding them.

The discriminator is the **subject** of the evidence, not the presence of a pronoun: evidence
beginning with *"I"* is Endora talking about itself. *"You asked 'where did you find this?'
after I listed some events"* keeps its belief, because the person is the subject and Endora
appears only in passing.

Applied at formation and by the backward sweep below, so the one already stored retires
itself.

### Rules apply backwards, not only forwards

Every guard on understanding ran **at formation only** — which quietly means the model of the
person is frozen at the quality of the rules on the day each row was written. A card reading
*you want me to turn off the kitchen light — because turn off the kitchen light entity* sat
there long after the rule rejecting exactly that shape shipped, and was never going to leave.

So the rules are re-applied to what is already stored: an instruction-shaped statement is
retired, and a statement that repeats an older one is retired while the older one is
**affirmed** — the same thought arriving twice is evidence for it, not litter.

Retired as **expired**, never as *corrected*: the person did not say it was wrong, and
recording that they did would put words in their mouth.

This is the same lesson [0054](0054-other-peoples-services.md) drew about inherited patches,
pointed at data instead of code: *anything that survives long enough to be inherited should
be re-read against the rules that arrived after it.*

Two matching faults were fixed alongside, both of which had put three cards saying the same
thing on one screen:

- the batch of beliefs formed in a single turn did not see **its own writes**, so two
  paraphrases of one thought both landed;
- duplicate detection compared **whole statements**, so a wordier paraphrase — *finds it more
  convenient and accurate to measure temperature in Fahrenheit* versus *prefers temperatures
  in Fahrenheit* — looked like a different belief. It now compares what a statement is
  **about**, with stance words removed. Polarity words are stripped too, which is safe only
  because disagreement is checked first: subject decides whether two statements concern the
  same thing, disagreement decides whether they say opposite things about it.

### Attention is computed, never pushed

What deserves attention is a **read projection** over facts already stored, plus a little
deferral state. Nothing accumulates; nothing needs clearing. The same instinct produced
"derived, never stored" for capability findings
([0054](0054-other-peoples-services.md)) and is why this system has no inbox of chores.

## Consequences

- The person maintains nothing. That is the entire point, and it is what the tree got wrong.
- Understanding is small enough to read in one screen and correct in one click.
- Endora can be wrong about the person, visibly, and be told so — which is better than being
  wrong invisibly.
- **A real loss from the deletion**: explicit long-term goals are no longer tracked as
  first-class things. Accepted, because they were tracked and not used, and the honest
  version of that feature is a belief that says the person cares about something.

## Rejected

- **The Value → North Star → Target tree.** Built, shipped, deleted. It made the person the
  system's administrator.
- **An approval queue for proposals.** [0029]'s specific mistake; anything with a badge and
  a count is the same mistake returning.
- **Asking about everything, in case.** A confirmation prompt on a settled belief is a chore
  with no possible payoff, and a screen of them is the queue by another name.
- **Deleting beliefs the rules would no longer form.** Expiring them keeps what Endora
  dropped on its own visible, which is the difference between a model and a black box.
- **Letting the person edit beliefs freely as a primary flow.** Correction, yes. Curation as
  a chore, no.
- **Merging contradictory beliefs** into one averaged statement.
- **Keeping beliefs forever.** A model that only accumulates is a transcript.
