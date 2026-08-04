# 0065 — A place is not the model's to remember

## Status

Accepted (2026-08-04). Applies [0053](0053-honesty-about-what-it-did.md)'s rule — the
guarantee goes in code, not in a prompt asking the model to be careful — to a fact that had
been left to the model four times.

## Context

Four daily briefs opened with

```text
Good morning! Here's your daily brief for New York, United States.
```

for somebody who does not live in New York, and reported that city's temperature underneath.
Three fixes came before this record. All three shipped, all three were verified, and each was
an attempt to make the model **more likely** to recall the right place:

1. Remove the example cities from the prompt, on the theory that an illustrative
   `"cardiologists in New York"` was acting as a magnet.
2. State the place as ground truth — *they are based in X; that is what "home" and "here"
   mean; never a place you saw in an earlier reply.*
3. Fix the parser behind that statement, which matched `based in ` while storage wrote
   `Based in: `, so the sentence in step 2 had been rendering empty the whole time.

Step 3 was a real bug and fixing it was right. It also revealed the shape of the mistake,
because after it the statement *was* present, correct, and reaching the turn —
`build_turn_request` reuses `build_butler_request`'s system message, so this was checked
rather than assumed — and the fourth brief was still wrong.

At that point the pattern is not three bugs. It is one bug three times: **each fix improved
the odds that a language model would recall a fact, and none of them removed the model from
the recalling.** A 7B model reading a dozen messages, three of which name a city it said last
week, will say it again, and the correcting sentence is one line against a weight of history.

### The test was green through all four

`the_butler_is_told_where_the_person_is` asserted that `/v1/context` returned a non-empty
`right_now`. It tested that the *input existed* and never that the *answer used it*. That is
why nothing caught this: the assertion was one layer above the join where the failure lived,
which is where nine of ten bugs in this repository have lived: they joined two parts
that both worked.

## Decision

### A skill declares that it needs a place; the turn supplies it

`CapabilityInfo` and `CapabilitySpec` gain `wants_place`. Weather, news and safety alerts say
`true`. When such a skill is called and the arguments name no place, the turn substitutes
the person's — the value Endora was told and holds — before the call goes out.

The model is not asked, so it cannot answer wrong. This is the same instrument as
[0064](0064-what-a-stranger-said.md)'s taint rule and [0051](0051-where-the-boundary-is.md)'s
runner: the turn narrows what actually goes out, rather than hoping the proposal was good.

Declared, never inferred from an id, for the reason [0054](0054-other-peoples-services.md)
gives every time: a name-based rule needs a new branch for the twelfth integration. The
default is `false`, so a skill that genuinely has no place behaves exactly as before.

### A place the person *did* name is never overwritten

"The weather in Boston" is a real request and only the person can mean it. If the arguments
carry a location, city, query or coordinates, they are left alone.

The one wrinkle worth naming: **a key present but empty is not an answer.** A model that
emits `"location": ""` has declined to say, and treating that as a named place is how a blank
reaches a geocoder — which resolves it, confidently, to somewhere famous. Empty, blank and
null all count as unnamed.

### The check moves to what the skill was called with

The new test asserts the argument the weather skill actually received, not the prompt the
model was handed. It was confirmed to fail without the fix before being kept, because this
repository has already shipped a checker that reported all-clear on a broken file and a
layout test that produced three false positives, and an assertion nobody has watched fail is
a wish.

## Consequences

- **Home cannot be got wrong**, including in a turn whose history is full of a different
  city, and including for a skill written next year that sets one flag.
- **The prose can still drift.** Nothing here stops a model writing "your brief for New York"
  over Springfield's numbers. That is a visible, correctable embarrassment rather than wrong
  data, and it is the same trade [0064](0064-what-a-stranger-said.md) accepted: an attacker
  can make the butler *say* something wrong and cannot make it *act* on it.
- **A place is now sent to sources that previously got none.** Intended — a weather call with
  no location was never going to be right — and it means the person's town leaves the node in
  more requests than before. It already did for every call the model got right.
- **One flag per skill is a thing to forget.** The default is the old behaviour, so forgetting
  it degrades to today rather than breaking anything.

## Rejected

- **A fourth prompt fix.** Three have been tried. The next one would also mostly work.
- **Overriding a place the model *did* name when the person's message names none.** Tempting,
  and it would have caught this instance. It also breaks "what's the weather" asked after
  "I land in Boston tomorrow", where using the context is correct. Filling a blank is
  unambiguous; overriding a stated value requires knowing what the person meant.
- **An eval case for it.** The battery measures whether the model gets things right, and the
  point of this record is that the model is no longer the one deciding. A case there would
  measure something that has stopped being load-bearing, and would carry the battery's noise
  while doing it.
- **Detecting a wrong city in the reply.** Requires knowing which place names are wrong in
  prose that may legitimately mention any of them. Unsound in the same way the deleted layout
  check was unsound.
