# 0086 — Ask deep plans, and the house reads

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude). Not yet deployed.

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** Ask deep is retired. The reasoning below is kept; none of its mechanism runs.

## Context

The console's manual "Ask deep" sent the larger model one bare question: no skills, no
history, no place or date. It could only answer from training data, so a question that
needs a lookup ("what is on this month?") got a guess (ADR 0085). The person asked for it
to use the skills, without giving up the protection against prompt injection.

The rule that makes that hard is ADR 0064/0070: nothing a stranger wrote may reach a model
that could be commanded by it, and nothing a stranger wrote leaves the device. A skill that
reads the web returns a stranger's words, so the obvious design (give the deep model the
skills and show it the results) would send those words off the box and let a page steer a
more capable model.

## Decision

Two models, two jobs. The split is the defence.

- **The deep model plans.** It is shown the person's question (secret-refused and PII
  redacted, as before), the skills on offer and the date. After each call it is told that
  the skill ran and roughly how much came back, in a sentence Endora writes ("returned
  almost nothing (37 characters)", "returned results (412 characters)", "did not run").
  It is never shown what came back: not the text, not the `[from outside]` mark, not an
  error that might echo a stranger. It sees nothing about the person: no understanding,
  recent activity, location, or earlier chat.
- **The local model reads.** It sees what the skills returned, since that never leaves the
  device, and it is given no skills while it does; any call it attempts anyway is dropped.
  An instruction in a page has nothing to call.
- **The loop is unchanged.** Every call the deep model chooses runs through the same
  clearance gate, and once a stranger has spoken in a turn nothing acts without the
  person's confirmation (ADR 0064).
- **Failure falls back, with a disclosure.** If the door refuses (a secret, or a stranger
  already read) or the deep model fails, the turn is answered locally and the reply says
  so. Every reply says where it went: the deep model chose skills and results stayed here;
  or the deep model answered alone and the question left the box.
- Planning is limited to 3 deep calls per turn. The mechanism is `PlannedByTheDeepModel`
  in `endora-application::deep_turn`, a `Butler` wrapper; `deep_ask` uses it with the deep
  model built from configuration, so the button works whether or not automatic fallback is
  on. The response shape is unchanged and now includes `activity`.

## Consequences and limits

- More leaves the device than before: the skill list (names, descriptions) and the outcome
  sentences go to the deep model's provider. Nothing a skill returned does.
- A planner that never sees results cannot judge them. It is told how much came back and
  may pick another source; a thin-but-nonempty result looks like a good one.
- The plan sees only the current question, not earlier chat, so follow-ups ("and next
  week?") lack their context. A deliberate v1 trade for privacy.
- The local reader can still be talked into saying something wrong by a page. The person
  sees it and can correct it; it cannot act.
- Fixed afterwards: `Deeper::continue_turn` did not restore pseudonyms inside tool call
  arguments, so a skill could be asked about `<person 1>`. It now restores them string by string
  inside the JSON (`Pseudonyms::restore_in_json`), so a value holding a quote cannot break it.

## Validation

Seven end-to-end tests with a scripted deep model, a scripted reader and a runner whose web
result carries an injected instruction and a unique marker: the deep model's whole
transcript never contains the marker or the stranger's text; none of the person's private
context appears; an empty first source lets it choose another; a question needing no skill
is answered by it alone; a reader told by a page to act cannot; after a stranger spoke no
actuator runs unprompted; a secret-looking question is not sent and is answered locally.
Unit tests pin the outcome sentences and the trusted view. Workspace: 1,011 passed, 0 failed.
Not run against live models.
