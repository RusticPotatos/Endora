# 0056 — How it behaves toward you

## Status

Accepted (2026-07-26). **Consolidates 0017, 0019, 0025 and 0031**, which are archived.

## Context

A butler that only answers when spoken to is a search box. One that interrupts on a timer is
an alarm clock. The difference between an assistant and an intrusion is entirely in when it
speaks and how it sounds, and both were originally decided separately.

## Decision

### The heartbeat evaluates; it does not act

The node runs a background loop. On each tick it does not *do* anything — it **evaluates**
what, if anything, is due, and routes each candidate through the autonomy model
([0051](0051-where-the-boundary-is.md)). Unattended turns are clamped to the reversible
bands: nobody is there to say stop.

The loop also does the unglamorous maintenance that keeps the rest honest — retrying servers
that came up empty, applying findings that have earned it.

### Proactivity is a budget, not a trigger

Deterministic code owns **how often**; the butler owns **whether**.

The schedule is a gate: the feature must be on, the minimum interval must have elapsed, and
the person must have been quiet for a while — if they just spoke, they are present and can
simply ask. Once the gate opens, whether there is anything worth saying is a judgement, and
having nothing to say is a valid outcome.

That split is why there are no scripted check-ins any more. A clock decides *when it may*;
it never decides *that it must*.

### The persona lives in the prompt, and has a floor

Tone is not code. The butler has a default register — candid, warm — and **mirrors the
person's register asymmetrically**: it reflects formality and kindness *upward* and never
mirrors hostility, rudeness or contempt *downward*. It stays even and kind. That floor is the
one part of the persona that is not negotiable by tone-matching.

### It never recites its own vocabulary

Endora's internal taxonomy — beliefs, confidence, intents, bands — is **internal**. It does
not say "I have formed a belief with medium confidence"; it says what it thinks and why. The
words engineers use for a model's insides are not the words a person wants to be spoken to
in, and every time the vocabulary changes, this rule has to be checked again.

### Messages it started are an inbox, not a conversation

What Endora said **unprompted** — check-ins, the brief, what it looked into overnight — is
collected separately from the conversation, newest first, grouped by day, readable or read
aloud. It is derived, not stored: a butler message is unprompted when the message before it
is not the person's, so nothing needs a flag anyone must remember to set.

A failure notice is not an approach and does not land there. An inbox is what Endora *chose*
to say; "I couldn't reach my language model" is what happened when it could not choose
anything.

### Silence is the default, and two paths were bypassing it

An unprompted turn that has nothing to say says nothing. Two paths reached the person's chat
anyway, and between them they were the *entire* proactive output of one day:

```text
00:00  the night loop ran, could not reach the model, and posted
       "Give me a moment and try again" — into an empty room
04:18  the check-in posted "None of the functions listed involve a
       'person' domain, so there's no need to call any."
11:01  the daily brief posted an actual brief          ← the only one that belonged
```

Neither was caught, for different reasons worth keeping apart:

- **The apology was a real answer** by every test available: non-empty, naming no tool, using
  no protocol words. What makes it not an answer is that *the model was never reached* — so
  that is now carried as a **flag on the reply**, not inferred from its words. A phrase to
  match on would mean the same sentence written twice in different crates, drifting apart.
  Asked a question, the person is owed the truth that it failed; unprompted, nobody asked.
- **The protocol prose missed a suppression list by one word.** The list held "functions
  provided"; the leak said "functions listed". A list of exact phrases will always fail this
  way, so the marker is now the bare word — a butler has no reason to say *functions*.

### A butler must be able to answer "what did you do while I was out?"

Asked exactly that, and then "nothing proactive done today?", the butler replied **"No
specific activities were recorded today"** — on a day it had posted three unprompted
messages, one of them a real brief four hours earlier.

Every part of the answer was stored: the messages it sent, the outcomes it recorded, the
schedules that fired. None of it was reachable from a turn, so the most important question a
proactive butler can be asked was the one it could not answer. Being proactive and being
unable to account for it is worse than not being proactive, because the person cannot tell
the difference between a quiet day and a broken one.

**Its own record is a skill.** Endora reads back what it did — messages it started, actions
it took with the verdict it observed, settings it changed, things it noticed stop answering —
and the digest is **assembled deterministically**, because it is a report of stored facts,
which is the one thing Endora is entitled to assert
([0053](0053-honesty-about-what-it-did.md)). The model's job is to say it nicely, not to
remember it.

**It is stated in the turn, not fetched by it.** The skill shipped first and did not work:
asked *"did you do anything while I was out?"*, a 7B model called `HassTurnOn` and tried to
switch on a light. Tool *choice* is the least reliable thing a small model does, and this is
the one question a proactive butler must not get wrong — so the answer goes where it cannot
be missed, exactly as **presence** does. Facts the butler reads, not a call it has to decide
to make.

The skill stays for detail — a wider window, the full list — and a short account of the last
day sits in every turn. Short, because everything in a turn's context is paid for on every
turn, which is the same reason a clock reading must not arrive with five kilobytes of house.

**And it ignored that too.** Asked again on the deployed node, with all eleven of the day's
entries sitting in its context under an instruction to answer from exactly them, the butler
repeated word for word the sentence it had produced the previous day: *"no specific activities
were recorded."* It was copying its own earlier answer out of the transcript.

Three attempts, each a better *place* to put the facts, and all three still asking the model
to use them:

| where the facts went | what happened |
| --- | --- |
| a skill it could call | called `HassTurnOn` and tried to switch on a light |
| stated in the turn's context | ignored; repeated yesterday's answer verbatim |
| **appended to the reply** | the record is there whatever the sentence says |

So the answer is the mechanism this project already uses for exactly this shape: **a claim and
the record disagree, so the record is appended** ([0053](0053-honesty-about-what-it-did.md)).
Nothing is rewritten and nothing is judged. The account goes next to the sentence and the
person can see which to believe — the same move as putting a reading next to a tool's receipt.

The trigger is a claim about activity *in general*, on a deliberately tiny vocabulary —
`activity`, `activities`, `proactive`, `recorded` — words rare in ordinary butler prose and
precisely what appears when this question is answered badly. A heuristic, named as one, and
cheap both ways: a false positive appends true facts, a false negative leaves things as they
were.

This is the pattern, and it is worth stating without hedging: *anything that matters cannot
depend on the model doing something.* Not on it calling the right tool, not on it reading the
context it was given, not on it declining to repeat itself. Three placements failed before the
one that does not ask.

Three details that carry the point:

- **"Nothing" is stated as nothing happening**, never as nothing being *recorded*. Those are
  different claims and the second one was made falsely. The empty answer names what did not
  happen: it did not write, act, change any setting, or notice anything stop answering.
- **Unprompted is read from adjacency** — a butler message whose predecessor is not the
  person's — because nothing stores a "proactive" flag. A reply the person *asked for* is not
  something Endora did on its own and must not be reported as though it were.
- **Whether the model reaches for it is measured, not assumed.** A skill the model never
  calls is worth nothing, so the battery scores a question about what *Endora* did being
  answered from Endora's record rather than from the house's lights — both offered, choosing
  is the test.

### A problem statement, not a status line

The failure this section exists for is a butler that **observes correctly and helps nobody**.
Endora could see that thirteen devices were unavailable and never once asked whether they
were still wanted. Reporting the count would only have added an item to somebody's day.

The difference between a status line and a problem statement is **duration** and **a place
for the answer to go**:

> ~~13 entities unavailable~~
> *Living Room Lamp hasn't answered for 9 days, in home-assistant. Still yours?*

The second is only sayable if Endora noticed nine days ago. It reads state constantly and
stored none of it, so "since when" — the one fact that turns an observation into a problem —
did not exist anywhere.

**What is stored is one row per thing that is wrong right now**, created the first time it is
seen that way and **deleted the moment it answers again**. That is the anti-queue guarantee
from [0052](0052-what-it-knows-about-you.md) made structural rather than promised, by a
different route than the derived findings in [0054](0054-other-peoples-services.md): the
store is bounded by *the state of the world*, never by how long Endora has been running. It
cannot grow into a backlog because recovering removes the row, including any answer the
person had given about it — a device that comes back is a different situation from one that
never did.

Deliberately not a state log. History would answer more questions and would grow forever;
"since when" answers the one question that matters here and costs a row per open problem.

**Nothing is said until it has been wrong for three days**, which is chosen to survive the
ordinary reasons something goes quiet without being broken: a weekend away, a router reboot,
a battery swap, a hub upgrade. Interrupting about a device that was going to come back on its
own is exactly what makes a butler tiring.

**Both answers end it, and one of them acts.** *It's gone* hides the thing in the service that
owns it — never deletes it, logged with its prior value, undoable from the same change log as
every other configuration write ([0054](0054-other-peoples-services.md)). *It's fine* records
that this is the person's business and stops it being raised, while leaving it visible.

There is deliberately **no "remind me later"**. That is how a queue starts.

Whether a value means "I cannot see this" is a **heuristic** over the words services use, and
is named as one. What makes it acceptable is the blast radius: a wrong classification can only
ever produce a *question*, never an action. Getting it wrong costs one tap; getting the
opposite wrong costs a device quietly staying broken.

**The first list was too wide, and the first live reading said so.** It included `unknown`,
`none`, `null`, `error` and an empty reading, and flagged **28 things against 7 real ones** —
every false positive a *scene*, whose state in Home Assistant is when it was last activated.
`unknown` there means "not since the last restart", which is the healthiest answer available.
Three days later the person would have been asked about 28 working things: the exact pile of
chores this section exists to prevent, at scale, and produced by the mechanism meant to
prevent it.

So the rule tightened to words that **can mean nothing else** — `unavailable`, `offline`,
`disconnected`, `unreachable`. A word that means "hasn't happened yet" as often as it means
"cannot be reached" is not evidence. `error` went too: a thing reporting an error *is*
answering, which is a different problem with a different remedy.

The cost is missing a device that only ever says `unknown`. Accepted: those almost always say
`unavailable` too, and a missed problem is recoverable where a butler that cries wolf 28 times
is not.

### Saying how it landed has to be asked where the person is

Endora has recorded **109 outcomes and zero reactions**. The machinery was complete — a
reaction type, an endpoint, a use case, buttons, and a feedback path that ranks skills by
helped-versus-missed and puts that in the prompt. It had simply never had an input.

The cause was a rule applied too literally. The ask lived on a screen the person opens
occasionally, below several other sections, under a comment reading *"never solicited: no
badge, no counter"*. That instinct is right — it is [0052](0052-what-it-knows-about-you.md)'s
anti-queue rule — but taken far enough that the signal could never arrive. **A loop with no
input is not a loop.**

So the one question worth asking — *did that help?* — now sits on the action trail in the
chat, on the **newest turn only**. Still no badge and nothing that accumulates: the ask is
gone by the next turn whether or not it was answered, so ignoring it stays free. The rule is
kept; it is asked exactly once, where the person already is.

### Reaching someone who is not looking

Being proactive is worth nothing if the person has to open the app to discover it. But a push
stack of Endora's own would mean certificates, a service worker, a subscription store and a
third-party relay — all to rebuild something already working on their phone.

So it is the **nomination** pattern again, the same one that settled which tool reads a
server's state: the person names a service of their own as how they want to be reached, and
Endora uses it. Nothing is assumed and nothing is hosted here — the same line
[0055](0055-the-model-layer.md) draws around the model, drawn around notifications.

The trigger is every message Endora **started itself**, and deliberately nothing cleverer.
The rate limit is already the schedule the person set, so it cannot become a firehose unless
they widen it.

It is **not** gated on presence, though the signal exists. That would mean parsing free text
a service wrote — *"rustic is not home"* — to decide whether to interrupt someone, and being
wrong either wakes them or silently swallows the alert they wanted. The schedule is the
honest limit.

Only the first sentence travels. The point is to say *there is something*; a notification
long enough to be the message is one people stop reading.

## Consequences

- Endora can be quiet for a day without that being a bug, and can speak without that being an
  interruption.
- Removing scripted check-ins made proactivity worse before it made it better — the honest
  version needs the model to have something to say.
- Hospitality is measurable in a small way: the person can see everything it said on its own,
  in one place, and stop it in one click.
- **The persona is prompt-shaped, so it inherits the model's reliability.** Anything that must
  be true is not left to tone ([0053](0053-honesty-about-what-it-did.md)).

- Endora can now say *since when*, which is the difference between noticing and helping.
- **It stores something about the world for the first time.** Small, bounded by what is
  currently wrong, and deleted on recovery — but it is state, and state is what the rest of
  this architecture works hard to avoid.
- A device the person hides is hidden in **their** service, for every client, not just for
  Endora — and is undone from the same place as every other configuration change.

## Rejected

- **Reporting the count.** "13 entities unavailable" is a chore with no duration and no
  remedy, which is the shape of every notification nobody acts on.
- **Keeping a state log.** It answers more questions and grows forever; "since when" answers
  the one that matters and costs a row per open problem.
- **Deleting what the person says is gone.** Destructive, irreversible from Endora's side,
  and a far larger grant than one tap licenses. Hiding is the smallest change that makes the
  catalogue true.
- **"Remind me later."** A deferral is a queue with a timer.
- **Trusting a service to declare which of its values mean "absent".**

- **Scripted check-ins and briefs.** Deleted; they said the same thing forever.
- **A fixed interruption schedule.** A clock may decide when it *may* speak, never that it
  must.
- **Persona in code.** Tone is the one thing a model is genuinely good at.
- **Mirroring the person's register in both directions.** Matching hostility is not warmth,
  and a butler that can be goaded is not a butler.
- **Surfacing internal vocabulary as a feature** ("I'm 70% confident"). It reads as a machine
  explaining itself instead of a butler answering.
