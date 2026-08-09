# 0076 — Standing questions

## Status

Accepted (2026-08-08). Generalizes the routing half of
[0074](0074-the-brief-is-a-standing-order.md) and **retires** its bespoke brief
pre-route into the one mechanism defined here.

## Context

Two asks, measured live, taught the same lesson twice. "Give me my brief" — with
the brief skill sitting in the catalogue — routed to house gossip. "What lights
are on" survived four fixes of everything *around* the model (the refused reader,
the unbounded generation, the null call, the doubled watchers) and still came back
as "several lights are on" prose, because the enumeration itself was left to a
model that follows instructions one run in three.

Both asks share a shape: the person asks them routinely, and their complete answer
is derivable by code from data Endora already holds. For that shape, model routing
is pure downside — it can only add wrongness to an answer policy could compute.

## Decision

**A closed set of standing questions routes deterministically.** One function —
`standing_answer` — is the single routing point, consulted before the model's
turn. Its entries:

1. **The brief** (0074's route, moved here verbatim): determiner+noun "my/the/
   morning brief", "brief me" → the standing-order assembly, deep-worded as the
   scheduled brief is.
2. **Kind status**: "what lights are on?", "any lights offline?", "check the
   lights" → answered from `current_states()` directly. Three gates, all
   required: a *kind the house actually holds* (the first word of the service's
   own keys, plural-trimmed — never an English list), a *question-or-status
   word*, and **no action verb** — "turn off the lights" carries both `off` and
   `lights` and is an instruction, so it falls through to the model's turn where
   policy gates the acting. The answer names what is on and what is not
   answering (the two lists a person acts on), counts the rest, and keeps
   `unknown` out of the problem column (0056's vocabulary).

Every entry falls through to the ordinary turn when it cannot assemble its
answer: broken routing degrades to a conversation, never to silence. A standing
answer files no specimen (0075) — it cannot fail the way a model turn can.

## What this retires

Per the pattern budget: the bespoke brief pre-route added by 0074 (the
`asked_for_their_brief` hook inlined in the chat turn) is gone as a separate
mechanism — it is now the first entry of `standing_answer`, and there is exactly
one place a chat ask can route deterministically. A third standing question is a
new entry in that function and a line in this ADR's list, not a new hook.

## What this is not

- **Not the old `is_brief_request` hack**, for the same reason 0074 wasn't: these
  route into features whose answers are the person's own data, assembled by code
  that is right by construction — not into scripted templates.
- **Not open-ended intent classification.** The set is closed and small, each
  entry's matcher is a named heuristic with its misfire cost written down, and
  anything conversational, ambiguous, or action-shaped still belongs to the
  model's turn. The moment an entry needs a model to decide whether it matches,
  it is not a standing question.

## Consequences

- The two most common asks in this house's live history now have deterministic
  floors end to end: routing, gathering, and wording (or enumeration) all in
  code.
- The eval battery's routing cases for these asks measure a path production no
  longer takes; they remain as model-fitness measurements (the model still routes
  everything outside the closed set).
- Candidate third entries, when the need arrives: "what's on my calendar today"
  (already in `present`), "did you do anything while I was out" (the
  `own_activity` digest). Each is one entry plus its gates, here.
