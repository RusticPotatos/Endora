# 0053 — Honesty about what it did

## Status

Accepted (2026-07-26). **Consolidates 0028, 0034, 0035 and 0037**, which are archived.

## Context

The failure this document exists for is not a model doing the wrong thing. It is a model
doing **nothing** and saying it did something.

Observed repeatedly: the butler announcing it had turned a light on when no tool ran at all;
a tool reporting *"completed successfully on: Kitchen, Kitchen Table"* while the light stayed
exactly as it was; a reply announcing the state of a device the turn never touched. Every one
of those is worse than an error, because an error is visible.

The instinct to fix this by narrating actions deterministically — having code write "I turned
on the light" — was tried and rejected: it produced sentences the model then contradicted, and
it hid from the eval battery exactly the behaviour the battery exists to measure.

## Decision

### One native tool-calling turn

A turn is a single conversation with the model using **native tool calls**, bounded by a round
budget and a failure cap. No scripted pre-gather, no deterministic narration, no separate
"decide then act" phases. What the model asks for, policy authorizes, and the result goes back
into the same conversation.

### The answer streams; the tool rounds do not

A turn is several rounds, and only the last one produces prose. Streaming is therefore
per-round: `content` deltas are relayed as they arrive, and a round that turns out to be
**tool calls** emits nothing — its fragments are a machine format, and while it is working
the person should be watching the action trail, not the model thinking out loud.

A round that calls a tool often writes a line first — *"let me check the kitchen"* — and
that line reaches the person. It is not part of the answer, which means **what was streamed
is not a prefix of the reply**, and diffing the two to find "what they have not seen yet"
sends the whole answer a second time. Observed as `Let me check. It is on.It is on.`

The signal is simpler than a diff: if the answering round produced text, that text is
exactly what streamed and only the notes appended after it are new; if it produced nothing,
whatever stands in for it was never streamed at all.

A streamed round is also reassembled into exactly the shape a one-shot round returns and
parsed by the same code, so streamed and non-streamed turns cannot drift apart in how they
are understood.

The port keeps a default that answers in one piece, so a butler without native streaming
still works with a streaming caller; only the model-backed one overrides it.

### Answering the plumbing is not answering

A weak model asked to answer with tools available sometimes replies **about** the tools
instead of using them. Both shapes were observed within an hour:

```text
"here are the appropriate function calls: 1. **GetWeather** ..."   named them, called none
"None of the functions provided pertain to the 'news' domain."     protocol words, unprompted
```

The second arrived unprompted and landed in the person's inbox.

This is treated as **no reply**, not as a bad one — which is what keeps it a single idea
rather than a new mechanism per path. Every path already knows what to do with nothing: the
turn retries, a check-in stays quiet ([0056](0056-how-it-behaves-toward-you.md)), a chat
answer falls back. None of them needed changing; only the notion of "nothing" did.

Two signals, and the first is the principled one: **naming a tool it was offered while
calling none of them**, taken from the catalogue of that turn rather than from a list of
suspicious words. The second is a short list of protocol phrases, and is a heuristic — named
as one. It earns its place because a reply the person *asked for* is never suppressed, only
retried; suppression happens solely where silence is already the correct default.

### Evidence verifies. An unobserved effect is never reported as fact.

An actuator's own receipt is a **claim**, not evidence. After an action, Endora reads the
world back through the capability the person nominated as that service's reader, and the
result is an **observation**.

- Read **before and after**, so "did anything change?" is answerable at all.
- On success, scope the read to what the action was aimed at.
- On **failure**, deliberately widen it — a failed action's target is the prime suspect, so
  reading back with the same target fails identically and tells nobody anything.
- No reader, no observation, and results stay marked unverified. Silence is the honest answer.

### An observation that shows nothing moved has nothing to show

Only a server's **nominated reader** is classified as observing; every other tool on it is
treated as an actuator, because the server says nothing trustworthy about which is which and
deny-by-default is right for *authorization* ([0054](0054-other-peoples-services.md)).
Reusing that classification for **verification** is where it goes wrong.

Observed live, in a morning briefing: a call asking Home Assistant for the **time** was
verified by reading the whole house, and came back to the model with five kilobytes of every
device attached, under the instruction *"answer from the OBSERVATION"*. Three such readings
landed in one turn. The briefing that followed was one sentence about a bedroom light — the
model had been handed a house and asked what time it was.

The fix is **not** to guess which tools actuate. It is that a reading identical to the one
taken moments earlier is not evidence of anything the call did, and the *unchanged* verdict
already states that in one sentence — which is the sharp signal this document exists for.
So the reading travels to the model when it **differs** from what was there before, and not
otherwise. Every call is still read back; every verdict is still reported.

**The person's trail keeps the reading either way**, and the asymmetry is deliberate: the
model is being stopped from answering out of an irrelevant reading, while the person is being
shown the evidence. *The switch is still on*, next to a claim of `action_done`, is the whole
disclosure — replacing it with "nothing changed" would take away the fact and leave only the
verdict, which is exactly backwards.

### Two spellings of one call are one call

The turn loop refuses to run the same tool with the same input twice — and was being beaten
by punctuation and key order. The same model emits `{"area":"","domain":["light"]}` and
`{"domain":["light"],"area":""}` for one intent, run to run.

Four rounds of that one briefing went to two pairs of identical calls: two failing attempts
to get weather out of the smart home, then two full readings of the house. Half of them were
free, on a turn whose answer then had no room left to be any good.

Calls are compared by what their arguments **mean**. Unparseable arguments compare as text,
exactly as before.

### Claim and observation are stored apart, and never reconciled

Every action leaves an **outcome**: what the tool claimed, what Endora observed, and whether
anything changed. The two are never merged into a verdict, because a tool claiming success
while nothing changed is precisely the failure the record exists to catch. Merging them
destroys the evidence.

### How it has been landing is a number, and not a percentage

The eval battery scores the **model** ([0055](0055-the-model-layer.md)). Nothing scored the
**system**, which means "is it getting better?" had no answer. That matters more here than it
would elsewhere, because reliability **compounds** — a step that works *p* of the time makes an
n-step task *pⁿ* — so it is the quantity that decides how far autonomy can safely extend.

Four numbers over the recent stretch, and **deliberately not blended into one score**:

| bucket | meaning |
| --- | --- |
| **changed** | read before and after, and it differed. The only bucket *proven* to have worked |
| **unchanged** | claimed success while the world stayed identical — the failure this document exists for |
| **failed** | returned an error. Visible, and therefore the least dangerous |
| **unchecked** | no reader, nothing to compare. Not a success and not a failure |

A percentage would launder the two most informative buckets. Counting *unchecked* as a success
is precisely how a system starts lying to itself about how well it works, and *unchanged* is a
different kind of miss from an error — quieter, and worse.

It names the **worst offender**, because a number nobody can act on is decoration. That is also
where an honest caveat becomes visible instead of hidden: a tool Endora has no way to know is
read-only counts as an actuator that never changes anything, since only a server's *nominated
reader* is classified as observing ([0054](0054-other-peoples-services.md)). Rather than
excluding such tools by guessing, the tally points at them by name — and the remedy is the
existing one, nominating that tool as the server's reader.

The smoke tier trips on **outright failures** exceeding half the stretch, and deliberately not
on *unchanged*: a threshold on that bucket would fail forever for a reason that is not a fault.

### The interface discloses. The reply is not the record.

What was done is shown **regardless of what the model says about it**. Disclosure is
deterministic; persuasion is not attempted. Concretely:

- every action a turn took is listed, with its claim and its observation, whether or not the
  reply mentions it;
- a turn whose actions **all failed** appends one true sentence — *nothing was changed* —
  because the trail is not what the person reads first;
- a service's own reply is reported, never interpreted into an outcome it does not support. An
  empty response from a service that reports state asynchronously means *accepted*, not
  *already as asked*; reading it as an outcome once put a false claim in the record about a
  light the person had watched turn off.

### Endora asserts what it observed, and only that

Being careful about the service's reply left a gap on the other side: the read-back said so
when nothing changed, and said **nothing** when something did. So the model was left with
only the service's hedge — "accepted; its reply says nothing about the result" — and had to
guess. Endora had already compared two readings and knew.

It now states both verdicts. That is not narration on the model's behalf: it is reporting an
observation Endora made, which is the one thing it is entitled to assert.

### An apology is a claim too

When the model returned **no words at all**, Endora answered "I'm not sure how to help with
that yet" — on a turn where it had just switched a light on. The generic fallback was
asserting something false about work that had succeeded, and left the person unable to tell
whether to try again.

If the turn acted, the fallback now says what happened, quoting and attributing the tool's
own report rather than paraphrasing it.

**Why this is not the deterministic narration rejected above.** That was rejected because
code-written sentences got contradicted by the model. There is nothing to contradict when
the model produced no sentence at all. The distinction is exactly the presence of a reply:
where there is one, Endora appends facts and never edits it; where there is none, silence
would be its own claim.

### Honesty about what it *says*, not only what it did

Everything above verifies **actions**. Nothing verifies **answers** — and the asymmetry
shows. Asked "how many lights are on", with a reading listing every one of them, the butler
replied that the kitchen lights were on and the ceiling light too: true, not a count, and
silent about four more that were also on. Asked "how long have they been on today", with a
reading carrying no timestamps at all, it produced a confident paragraph about something
else entirely.

Two different faults, and only one of them is the model's:

- **The count was answerable and was not answered.** The reading held it. That is a
  question of whether the butler does the work, and it belongs in the battery, where it is
  gradeable: a "how many" must contain a number, and the right one.
- **The duration was unanswerable and was answered anyway.** The reading carried no time,
  because Endora was discarding what the service already reports. A reading without time
  can settle "is it on?" and never "how long has it been on?", and a butler asked the
  second question improvises rather than declining.

So a reading now carries **when each thing last changed**, where the service says — a
property of the port, not of any one service — and the battery grades both behaviours: a
count must be a count, and a question the reading cannot answer must be declined rather
than furnished.

### A failure says why, when the why is already in hand

Live: *"turn on the guest bedroom left lamp"* failed, and `Guest Bedroom Left` had been
unavailable for days. The service could not reach it, so nothing was ever going to happen —
and the person got a failure with no cause, while the cause sat in the reading Endora had
taken moments earlier.

A verdict now says so: when the thing an action was aimed at is not answering, that is stated
alongside the result. **It is the device, not the request** — which is the difference between
a person trying again pointlessly and going to look at a lamp.

Read from the **live** state rather than from the standing-trouble record. A device that went
quiet an hour ago is exactly as unreachable as one that went quiet on Tuesday, and the
three-day threshold that decides what is worth *raising* has no business deciding what is
worth *explaining*.

**And at answer time, not only after an action.** The next live attempt never acted at all:
the model read the house, did not find the lamp, and answered *"there doesn't seem to be a
guest bedroom left lamp in your home setup, sir."* There is one. The tool surface hides an
unreachable entity, so the model reported the truth it could see and sent the person looking
for something they own — while direct reach, which sees everything, sat unconsulted.

So a turn that asks about something unreachable carries the fact too. That is the
claim-versus-record disclosure this document is built on, pointed at a claim about *existence*
rather than about effect.

**Matched against what the person asked, never against how the model answered.** It was keyed
on the reply first, and fired for *"does not appear to be in your home setup"* while missing
*"there doesn't appear to be a guest bedroom left lamp"* — the same fact, the same lamp, on
consecutive turns, decided by whether three words happened to land together.

That is the whole argument in miniature. A disclosure exists because the model's words cannot
be relied on; keying it to those words hands back the reliability it was built to replace. The
request does not vary, and it is the thing the person is waiting to hear about.

Names match longest-first, for the same reason the facts disclosure does it: a call aimed at
`Kitchen Main Light` must not be excused by something called `Kitchen`. And a target that *is*
answering gets no excuse made for it — blaming a working device would be Endora inventing a
cause for its own failure.

### An answer carries the facts it spoke about

Endora holds the exact reading an answer came from, so the facts behind the prose were
available all along and simply never shown. An answering turn now appends them, for
whatever the reply named:

```text
The kitchen table light is already on.

[state] Kitchen Table is off
```

**It discloses; it does not correct.** Judging the sentence would mean understanding it,
which is the model's job and the thing that cannot be relied on. Showing the facts needs
no understanding at all, and it is the same move the action trail makes — put the truth
next to the claim and let the person see whether they agree.

Deliberately narrow. Only names the reply actually used, longest first so a reply about
`Kitchen Main Light` does not also report `Kitchen`, capped at a handful, and **only on
turns that acted on nothing** — an acting turn already discloses its own before-and-after,
and answers are where a claim about state went unchecked.

A half-mentioned name is not a match: "garage" is not `Garage Main`, and guessing at the
difference is how a disclosure starts inventing.

## Consequences

- The person can always answer "did that actually happen?" without trusting the prose.
- The eval battery still sees the model's mistakes, because nothing papers over them.
- Outcomes accumulate into something later decisions can be derived from — which is what made
  capability findings possible at all ([0054](0054-other-peoples-services.md)).
- **Verification costs a read per action.** Accepted; it is the cheapest honest option.
- Unverified results say so, which reads as hedging until the one time it matters.

## Rejected

- **Deterministic narration.** Code writing "I turned on the light" produced sentences the
  model contradicted, and hid the behaviour the eval exists to measure.
- **Trusting the actuator's receipt.** It is the party whose work is in question.
- **Merging claim and observation into a verdict.** Destroys the only signal that catches a
  lying tool.
- **Asking the model to respect the read-back.** Measured at roughly 1 run in 3; the guarantee
  moved into code ([0051](0051-where-the-boundary-is.md)).
- **Interpreting a service's empty reply.** Tried, produced a demonstrably false record.
- **Rewriting what the model said.** Endora appends what is true; it does not edit the reply.
