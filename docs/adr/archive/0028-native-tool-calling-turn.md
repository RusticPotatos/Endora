# 0028 — One native tool-calling turn (grounded honesty, no deterministic narration)

## Status

Accepted (2026-07-24; the cutover completed 2026-07-25). Amends the turn contract of
[0014](0014-the-butler-conversation-values-attention.md),
[0019](0019-proactive-self-improving-butler.md), and
[0027](0027-self-improving-model-layer.md). Native tool-calling on the
tool-*selection* pass shipped first (PRs #89/#90/#92); the single-conversation loop
landed on the chat turn, and the proactive flows (check-in, brief, nightly loop)
followed, at which point `gather_with_skills`, `ButlerContext.tool_result` and the
deterministic floors were deleted. Fully in effect.

## Context

The butler turn is a hand-rolled **two-pass** design:

1. **Gather pass** — the model picks tools (now via the endpoint's native
   tool-calling API) and we run them, collecting results into `gathered` *strings*.
2. **Synthesis pass** — a **separate** model call whose system prompt carries those
   results as a `SKILL RESULT` blob, from which the model writes the final prose.

On the synthesis pass the model is **not structurally bound** to what happened: a
tool's error is a paragraph in its system prompt, which a weak local model ignores —
it narrates "the kitchen lights are now on" after `HassLightSet` failed. Verified
live on `qwen2.5:7b` and `:14b`.

The codebase compensates with **deterministic narration on failure**: the
`follow_up_intent` net (hardcoded to specific intents / the old `home_assistant`
id), the canned "I can't check that…" reply, and the `answer_ctx = None` honesty
overrides in `send_to_butler_streaming`. These are band-aids for the two-pass design,
and they are the wrong shape: they replace the model's voice with fixed strings, they
rot (the `home_assistant` hardcode outlived the built-in skill), and they mask model
failure instead of surfacing it. **We would rather the model fail honestly than emit
a deterministic string on failure.**

Note the scope: **"models propose, deterministic policy authorizes"
([0005](0005-models-propose-policy-authorizes.md)) is about which actions _run_ —
that boundary stays.** This ADR is about how an outcome is _narrated_, which is a
different axis: narration should be the model grounded in the real result, not a
canned override.

## Decision

Collapse the two passes into **one native tool-calling conversation**, the shape
these models are trained for:

1. The model emits an assistant turn, optionally with `tool_calls`.
2. Each call is executed **through the policy layer unchanged** (deny-by-default,
   reversibility bands, openers, the autonomy envelope — [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md)).
3. Each result — **success or error** — is appended to the **same** conversation as a
   `role: "tool"` message.
4. The model continues that conversation until it answers with no tool call, bounded
   by the existing round/failure caps.

The final answer is then generated with the real outcome — including a tool error —
**in the model's immediate context as a message it just received**, which is what
keeps it grounded. We then **delete the deterministic narration**: `follow_up_intent`,
the canned honesty strings, and the `answer_ctx`/synthesis split. If the model still
misreports with the error in front of it, that is a model/loop deficiency to fix
(system prompt, a better model, the [0027](0027-self-improving-model-layer.md)
escalation ladder) — never a string to hardcode.

**Loop ownership.** Tool *execution* is a policy concern (application/capabilities);
the tool-message *protocol* is a model concern (infrastructure). The loop is driven
by the application, which owns policy, and passes the growing conversation —
including assistant-tool-call and `tool`-role turns — to the `Butler` port, which
owns their wire format. The `Butler` port and the message model gain tool turns; the
`ButlerContext.tool_result` system-prompt channel is retired.

## Consequences

- Fabrication-on-failure stops being masked and becomes structurally unlikely; when
  it happens it is *visible* (a model bug to fix), matching "let it fail honestly".
- The deterministic honesty nets are removed — less code, no stale hardcodes.
- Streaming is simplified: the final answer is one ordinary streamed assistant
  message, not a grammar-swapped second pass ([0018](0018-streaming-chat-responses.md)).
- The `Butler` port contract changes (tool turns) → an amendment to 0014/0019/0027.
- **Risk:** with the nets gone, a genuinely weak model can misreport until the loop +
  model quality carry it. Accepted deliberately; mitigated by the grounded loop and
  the escalation ladder, not by canned strings.

## Alternatives considered

- **Keep the two-pass design, add another deterministic net for MCP failures.**
  Rejected — more of the exact band-aid we are removing.
- **Infrastructure owns the whole loop with a policy callback.** Cleaner message
  handling, but it moves the agentic loop out of the application and buries policy in
  a closure; rejected to keep the loop and policy visible in the application layer.
- **Constrain the synthesis model with a grammar that forbids success claims on
  failure.** Brittle and still not grounded; rejected.
