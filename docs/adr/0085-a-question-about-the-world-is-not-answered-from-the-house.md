# 0085 — A question about the world is not answered from the house

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude).

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** The chat's routing of questions about the world is retired. The reasoning below is kept; none of its mechanism runs.

## Context

A conversation on the deployed node gave three poor answers in a row:

1. "what watch events are in new york this month?" was answered "Of your 1 event:
   1 reporting other readings." from "the house's own reading". The standing-question
   route (ADR 0076) matched the noun "events" to a Home Assistant `event` entity and
   gave the house's current count as the answer to a question about a city next
   month. Everyday words such as `event`, `weather` and `calendar` are entity kinds
   too, so the route had no way to tell the world from the house.
2. A later answer ended "…other events or activities, feel free to ask!" and the
   node appended its whole "what I did" record to it, including the notice a goal
   had published that morning. `account_behind` was keyed on words in the model's
   reply (`activity`, `activities`, `proactive`, `recorded`), and a pleasantry
   contained one. A comment next to its call claimed the opposite design ("matched
   against what the person asked") but had been displaced there from the
   neighbouring `note_not_answering` call; the code matched the reply.
3. The model chose `ticketed_events` three times for a question that skill's own
   description rules out ("Ticketed events ONLY"), while Brave Search, connected over
   MCP with web, news, local and place search enabled, went unused.

## Decision

**The kind route gains a fourth gate: about now, and about this house.** A time
such as "tomorrow", "yesterday", "this month", "next week" or "upcoming", or
"in <a place>" where no key the house holds contains that word (and it is not
"the house", "home" or similar), means the person is not asking for the house's
current reading, and the question goes to the model. A wrong route costs trust and
a hand-off costs a model turn, so the gate errs towards hand-off.

**The record answers the question, not the model's wording.** `account_behind` now
takes what the person asked and appends the record when they are asking about
Endora's own doings: "you" with did, done, doing, been or lately, or the words
"proactive" or "unprompted". It no longer looks at the reply at all. "Activities"
is deliberately not a cue, since "find me some activities for the kids" is a request
and the word caused the leak. The displaced comment is replaced with one that
describes the code.

**Not changed by this fix: tool choice.** The third symptom was measured rather than
guessed at, and the measurement does not point at one cause (below). Nothing in the
code was changed for it.

## What was measured about tool choice

`tests/tool_choice_eval.rs` (ignored, live) offers a model the real built-in skills
plus 89 stand-ins named like the node's MCP tools, asks eight fixed questions, and
records the first tool it asks to call. The stand-ins' descriptions are approximations,
so the comparisons matter more than the absolute scores. It is a small sample (two or
three repeats), not a reliability rate.

- **The node's Ollama truncates prompts to 2,050 tokens.** It logged 46 truncations in
  eight hours, every one at that limit, with prompts of 4,400 to 12,300 tokens, some
  from real chat turns. The chat adapter does not ask for a larger window. A truncated
  prompt loses the head of the tool list. This also means the Phase 6 live goal runs
  were made through the same truncation.
- **At the default window, shortlisting looked decisive and was not.** Full catalogue
  (104 tools) against a shortlist (about 25) scored 4 vs 11 of 16 (`qwen2.5:7b`), 6 vs
  13 (`qwen2.5:14b`), 4 vs 9 (`hermes3:8b`) and 6 vs 9 (`qwen3:8b`). With truncation
  removed (a 32k-context copy of `qwen2.5:7b`) both catalogues scored 12 of 24. Much of
  the effect was truncation, and some "successes" were accidents of which tools survived
  at the end of the list. `granite3.3:8b` never called a tool in this setup.
- **The second step is the real gap.** When the first tool returns nothing, a good
  assistant tries another source. After `ticketed_events` returned nothing for "watch
  events", four local models (7B to 14B, nothing truncated) searched the web in 1 of 12
  shortlisted trials; the rest reported the empty result as the answer, as in the
  conversation that prompted this. Rewording the empty result to say what the skill does
  not cover and to search the web did not help (0/3 for `qwen2.5:7b` and `14b`, 1/3 for
  `qwen3:8b`; `hermes3:8b` changed behaviour but chose `news` and `web_fetch`). It was
  not adopted.
- **Whether the larger model has the skills depends on which way it is reached.** There
  are two, and they differ.
  - *The automatic hand-off* (`Deeper::continue_turn`) offers the deep model the same
    skill list the local model had, and what it chooses is returned to the turn and runs
    through the same gate. Tests now prove this end to end.
  - *The "Ask deep" toggle* (`ask_deep_model`) sends the larger model a single bare user
    message: no tools, no system prompt, no history, no place or date. It answers from
    its training data and cannot look anything up.
- **The automatic hand-off still cannot rescue the screenshot's case.** Escalation fires
  only when code detects a failed turn, and a confident wrong answer is not detected. And
  the door (ADR 0064, 0070) refuses any turn that has read a third party's words. A
  third-party skill's result carries the `[from outside]` mark whenever the call
  *succeeded*, **including when it found nothing**; only a failed call is exempt. So an
  empty `ticketed_events` result already shuts the larger model out: it can choose the
  skill, but after a third-party read it may not be the one to answer. A control test
  shows the only difference is that flag.
- **Web search is probably not in front.** Only reads are in front, and for an MCP
  server only the one tool the person nominated as its reader is a read (the nomination
  is deliberately the person's, never the server's own claim). The rest wait behind
  `find_a_way_to_do_it`, which returns every deferred tool at once. The built-in
  `web_search` needs its own key.

**Applied afterwards: the truncation.** The window is Ollama's, not Endora's, so it was
raised where Ollama is configured: the NAS's `ollama` role now sets
`OLLAMA_CONTEXT_LENGTH=16384` and `OLLAMA_NUM_PARALLEL=1` (one request gets the whole
window instead of half). On the plain `qwen2.5:7b` the node uses, the same evaluation
went from 4 of 16 to 13 of 24 on the full catalogue, `ollama ps` reports a 16,384
context, and no truncation was logged afterwards, including for the 104-tool prompt.
The cost is KV-cache memory per loaded model, and a 14B model will spill some layers to
the CPU on a 12 GB GPU.

Not decided here: a deterministic route for questions about the world, where code
chooses the search and the model only summarises, in the manner of ADR 0076; whether an
empty result should still count as having read a stranger's words; a cap on what the
lookup returns.

## Validation and limits

Unit tests pin both gates against the live question and its siblings, and the
controls (rooms and "the house" still route to the reading). Whole-turn tests cover
the city question reaching the model and not the reading, the pleasantry not
attaching the record, and being asked what it did still attaching it whatever the
model says. The leak test was confirmed red before the fix.

Limits: the time and place gates are word lists and will sometimes hand a
legitimate house question to the model, which costs a turn. The doing cues will
sometimes attach the record to a question that did not need it, which is true but
unwanted. Neither has been measured on live traffic. The two code fixes were
deployed as build `c220320+local-2` and have not been exercised on the live node
through a signed-in chat. Tool choice is unaddressed in code.
