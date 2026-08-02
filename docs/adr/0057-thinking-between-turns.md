# 0057 — Thinking between turns

## Status

Accepted (2026-08-02). Extends [0052](0052-what-it-knows-about-you.md), whose central rule it
has to survive rather than quietly amend.

Proposed 2026-08-01 and accepted after the two departures below were put to the person
explicitly: that beliefs may now arrive from **observation** as well as conversation, and that
notions are **stored rather than derived**. Both are real widenings of 0052 and 0054, and both
were named as the reason this sat unaccepted rather than being carried by the rest of the
argument.

## Context

Every turn Endora takes starts from zero:

```text
message or timer  →  rebuild context from stored facts  →  one model turn  →  reply  →  forget
```

It holds **beliefs** — settled facts about the person — and **outcomes** — things that
happened. Between those two there is nothing, and the gap is the whole of thought. There is
nowhere to put *"I think something might be going on, and here is what would tell me."*

The consequence is that Endora can only ever report. Asked what it noticed, it re-reads the
same records the person could read themselves. It has never once been *working on* anything
between one conversation and the next, and the single intention it has ever held spent five of
its seven steps writing down apologies for not reaching its model.

The person named the missing thing directly: they are not asking for more sensors or more
actuators, they are asking for a **thought process**. A butler's distinguishing property is
not reach. It is having been paying attention while nobody was looking, and being able to say
why it brought something up.

**This is also the most fabrication-prone thing this repository could build.** A weak local
model invented a nightly hunch is a nonsense generator wearing a butler's clothes, and
[0053](0053-honesty-about-what-it-did.md) already established that this model obeys a direct
instruction about verification roughly one run in three. Anything here that depends on the
model being disciplined is already broken.

And it sits directly on top of the one thing [0052] deleted. A store of tentative statements
about the person, accumulating, waiting to be resolved, is the **shape** of the goal tracker —
Value → North Star → Target → Experiment → Reflection — that was built, shipped and removed
for making the person into the system's administrator. Any design here that cannot articulate
why it is not that is that.

## Decision

### A notion is a belief that has not earned its confidence

Endora may hold **notions**: a statement about the person, the evidence that suggested it, and
what would settle it.

```text
what I think          "the Monday gym block gets cancelled"
what suggested it     ← calendar events, outcomes, messages — by id
what would settle it  ← what to look for next
status                open · matured · died
```

A notion is not a new kind of thing. It is the same statement a belief holds, at the stage
before there is enough evidence to hold it. When one matures it **becomes a belief**, in the
one model that already exists, carrying the chain that produced it as its evidence.

This is why notions need no new vocabulary anywhere else in the system, and — per
[0056](0056-how-it-behaves-toward-you.md) — the word is internal. Endora never says "notion"
to anybody.

### A notion must cite records that exist

**The guarantee is in code, not in the prompt.** A proposed notion is stored only if its
evidence resolves: real entity ids, real outcome rows, real messages, each of which is
fetched and checked. A citation that does not resolve is not a weak notion, it is a
**discarded** one.

This is the mechanism `facts_behind` already uses for the same reason and with the same
result: the model proposes the wording, the record decides what survives. It is the difference
between a system that thinks and one that free-associates about somebody.

### Evidence about the person must be attributable to the person

**Other people live in the house.** That single fact breaks the obvious version of this
design, and it broke the first implementation: within an hour of deploying, the context line
Endora was reading said

```text
morgan is not home; on the Family calendar: Yardwork in the morning - Elise and Michael
```

A notion could cite that reading three times over and mature into a belief that *the person*
does yardwork in the mornings. The same goes for every household record — a light, a door, a
panel disarmed at six. With more than one resident, none of it is about any particular one of
them, and the resulting belief would be confidently wrong *and invisible*, because the card
reads perfectly well.

So the sources are ranked by what they can actually prove, and only two can carry a notion
into belief:

| source | may corroborate | may carry |
| --- | --- | --- |
| what the person **said** | yes | **yes** |
| what Endora already **believes** about them | yes | **yes** |
| a reading from **their own device** | yes | **yes** |
| an **outcome** — what Endora did | yes | no |
| a **reading** — the house | yes | no |

Outcomes are excluded for a reason this repository already learned once: **an outcome is
Endora's own conduct**, and [0052](0052-what-it-knows-about-you.md) found a belief formed from
exactly that at the top of the understanding screen. Letting one carry a notion is that
feedback loop with an extra step in it.

#### The correction: their own devices are not the household

The first version of this rule ruled out **every** reading. That was right about the hallway
light and wrong about their own phone — and it was the reason wearable and health data would
have landed, corroborated nothing, and correctly done nothing at all.

A watch on somebody's wrist is not the household's. So a reading from a thing that belongs to
*them* may carry a notion, and a reading from the house still may not.

**Which entities those are is read from the service, never inferred.** Home Assistant already
holds the mapping because the person set it up — a `person` entity lists the trackers its
presence is computed from. And where there is **more than one person entity, Endora attributes
nothing**: nothing there can say which one it serves, and this record rejects guessing outright
because the cost of being wrong is not a bad reading but a false belief about somebody. That
also fails safely as a household grows — it stops attributing rather than starting to mix
people up.

Two things follow, both accepted deliberately:

- **A pattern purely in household rhythm can never mature.** Bedtimes and departures cannot
  become beliefs while nothing records *who*. That is the honest position, not a limitation to
  be worked around — and per-person attribution is recoverable later (an alarm panel knows
  which user disarmed it) as a deliberate step rather than an assumption.
- **Endora does not accumulate a model of people who are not its person.** The housemates
  never asked to be modelled, and this rule is what stops it happening as a side effect.

### Notions die by themselves

A notion that goes a horizon without new supporting evidence **expires**, exactly as a belief
does under [0052]. Nothing is dismissed, nothing is groomed, nothing needs clearing.

This is load-bearing rather than tidy. The failure mode of every hypothesis store is that
speculation accumulates faster than it resolves, and the honest reading of an old unresolved
notion is not *"still open"* but *"nothing has supported this in three weeks"*.

### A small window, not a queue

Notions are capped at a handful open at once. At the cap, forming a new one **displaces the
weakest**, and the displaced one is gone.

The cap is what makes this a cursor rather than a queue, and it is the same instrument as
*one intention at a time*. A bound that can be raised by the person is not a bound; this one
is a constant in code, and raising it is a code change somebody has to argue for.

### Every rule understanding applies, applies here

A notion is subject to the rules [0052] established for beliefs — at formation *and*
backwards:

- instructions are not notions;
- **Endora's own conduct is not evidence about the person**, judged by the subject of the
  evidence;
- a notion that says what an existing belief already says is not a discovery;
- contradiction is kept, not merged.

**Reusing those rules is a requirement of this record, not an implementation note.** Three
separate times in one week a rule was found implemented twice and drifting; the settings
completeness check existed in two places, and a smoke invariant that re-implemented a rule was
testing its own copy. A parallel rulebook for notions would be the fourth and the worst,
because it would govern what Endora believes about a person.

### The night pass is where thinking happens

The nightly pass today tidies old beliefs. It gains a second job: read the day's
observations, advance or kill open notions, and form at most a small number of new ones.

Thinking happens on a clock because everything here does, and that is a real limitation
awaiting its own record: nothing in this system is yet triggered by the world changing. The
point of *this* one is only that there is now something to think **with**.

### Visible, with no verb

The person can see what Endora is chewing on. They cannot act on it: no approve, no dismiss,
no snooze, and — per [0052] — **no count and no badge anywhere**.

This is the line the goal tracker crossed. Looking is a memory right, and speculation about
somebody that they are structurally forbidden from seeing is not something this repository
should hold. Maintaining is a chore, and a screen with a verb on it becomes a chore list
whatever the cards say. So: readable, never actionable, cleared by *forget everything*.

The reasoning becomes visible where it pays off anyway — on the belief a matured notion turns
into, whose evidence *is* the chain.

## Consequences

- Endora can be **working on something** between conversations, and can say why it raised a
  thing. This is the property the person asked for and the system has never had.
- Understanding gains a source that is not the conversation. Beliefs may now arrive from
  observation, which is a genuine widening of [0052] and the main risk this record accepts.
- Storage grows by a bounded handful of rows. Notions are stored rather than derived, which
  departs from *derived, never stored* ([0054](0054-other-peoples-services.md)) —
  deliberately, and for one reason: a hypothesis is precisely the thing that cannot be
  recomputed on read. Its **statement** must be pinned so that later evidence tests the same
  claim, instead of the model inventing a fresh wording each night and never accumulating
  anything.
- A new way to be wrong about the person, in public, over time. Mitigated by expiry, by the
  cap, and by the fact that a matured notion lands somewhere they can correct it — which is
  better than the same inference happening invisibly inside a prompt, which is where it
  happens today.
- **Little of value until there is something to notice.** Notions over a house that reports
  only light states are notions about lights. This record is worth building after the
  sensing it feeds on, not before.

## Rejected

- **Showing open notions in the conversation.** The place a question goes ([0052]) is for
  things Endora needs an answer to. A half-formed thought is not that, and putting it in the
  chat makes the person resolve it — which is the goal tracker, in the one place they cannot
  avoid it.
- **Asking the person to confirm a notion.** Same fault, with a button.
- **Letting the model decide when a notion has matured.** Maturity is a count of resolving
  citations, in code. The model proposes; policy decides ([0051](0051-where-the-boundary-is.md)).
- **Unbounded notions with ranking instead of a cap.** Ranking a hundred speculations is a
  better queue, not the absence of one.
- **A confidence score on a notion.** Confidence comes from evidence ([0052]); a second
  numeric axis invites the model to express enthusiasm, and there is nothing it could mean
  that the citation count does not already say.
- **Hiding notions entirely.** Defensible on noise grounds, rejected on memory rights: this
  system does not hold private theories about the person it serves.
- **Treating the household as one person.** The tempting shortcut, and the reason it is
  rejected is not tidiness: it would have Endora forming beliefs about its person out of
  somebody else's morning, and quietly building a model of housemates who never asked for one.
- **Attributing a household event by guessing** — inferring who from the time of day, or from
  who was last seen on the network. A wrong guess here does not produce a wrong reading, it
  produces a wrong *belief about somebody*, which is the most expensive kind of error this
  system can make.
