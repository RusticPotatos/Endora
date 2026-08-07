# 0074 — The brief is a standing order

## Status

Accepted (2026-08-07). Completes the direction [0056](0056-how-it-behaves-toward-you.md)
set with "a brief is assembled, not requested", and retires the agentic gather that
[0053](0053-honesty-about-what-it-did.md)/0056 left underneath it.

## Context

The brief was one agentic turn: a prompt ("reach for anything else relevant"), the
whole skill catalogue, and six tool rounds. ADR 0056 had already taken the *facts*
away from the model — calendar, presence, standing troubles are assembled and
appended, never rewritten — but the sections a person actually wants each morning
still depended on the model choosing the right tools, in the right order, with the
right arguments, every day.

Measured and lived with, it did not hold. Four days of briefs read like "the kitchen
and garage lights are on". One arrived as a paragraph about calendar integration
errors. The person then asked, in chat, for exactly what they wanted:

> "In my morning brief please provide me three top news stories. Weather conditions.
> Traffic conditions and anything home related I should be aware of. Also any
> calendar events that day."

That request had nowhere to live. Nothing turns a chat message into configuration;
it scrolled out of the recent-message window like any other sentence. And even
stored as a preference it would only ever have been a hint to a model that ignores
stronger hints — "top three headlines" in a prompt is a wish, where `count: 3` in a
call is a fact.

The request *is* a standing order: the sections of a brief are the person's
decision, stable across months. A standing order is configuration, and
configuration is code's to execute — the same split 0056 drew for check-ins: the
clock decides *when it may*, and now the order decides *what it holds*.

## Decision

**Sections are gathered by code.** `gathered_brief_sections` calls each configured
section skill with known-good arguments — weather for the person's place (its
summary now says whether an umbrella or jacket is worth it, from the numbers), the
drive from the house's own travel-time sensors, the top **three** headlines
(enforced by the call), ticketed events and city meetings where configured. The
results are joined to what Endora already holds (calendar, presence, standing
troubles — 0056's assembly, unchanged) by `assembled_brief_facts`.

**The model's only job is wording.** The deep model words the facts when the person
opted in (0055), under append-never-rewrite (0056): whatever the writing leaves out
is appended verbatim. The local model is not asked at all — measured, it wrote "the
lights in your bedroom are off" when handed exactly these facts. With no deep
model, the facts post as themselves: a brief no longer needs a model.

**Asked, not only scheduled.** A `brief` skill (composition root, the
`own_activity` pattern) assembles the same sections fresh at call time, so "give me
my afternoon brief" in chat reads the world in the afternoon and the weather line
carries its own "as of" clock. The skill is third-party — headlines are other
people's words, so a turn that read the brief may not act on its own (0064).

**A failed section is a trail note, not a daily apology.** A section whose skill is
unconfigured contributes nothing — the brief shrinks honestly. One whose fetch
failed is recorded in the activity trail, where the operator looks, rather than
apologised for in the person's morning every day the API is down. A skill with
nothing to report (a house with no travel sensors) reports unavailability once,
in chat, when asked — never as a recurring line.

## What this retires

Named, per the pattern budget:

- **The brief's agentic gather** — the `run_tool_turn` call inside `daily_brief`,
  its prompt, and `BRIEF_TOOL_ROUNDS`. Chat turns, check-ins and the nightly loop
  keep the agentic turn; the brief was the one place the person had already written
  the tool plan themselves.
- The stale doc claims that went with it ("weather/safety/news sweep" wording in
  scheduling; the on-demand brief's empty-case note).

What this is **not**: the scripted floor 0053/0056 deleted. That floor emitted
canned sentences when there was nothing to say; this posts nothing when there is
nothing, and every line it does post is a fact fetched moments earlier from the
person's own services, under their own standing order.

## Consequences

- "Three headlines, weather, traffic, events, calendar" is now a guarantee, not a
  prompt. Changing the order means changing code — acceptable while the order is
  one person's; a person-editable section spec on `BriefSchedule` is the recorded
  next step if a second opinion ever needs it.
- The brief's cost is bounded and knowable: at most one call per configured
  section, no tool rounds, no model except the opted-in wording pass.
- Safety alerts are not a section: severe weather already arrives through the
  weather skill's own warning field, and a daily "no alerts" line is the noise
  0056 forbids. The `safety_alerts` skill remains reachable in chat.
