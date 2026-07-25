# 0038 — Capability profiles: learning what a tool does, instead of patching each one

## Status

**Proposed** (2026-07-25). Amends the "one mapping per integration" of
[0034](0034-evidence-verifies.md). Not adopted — held for a decision, since it changes
how Endora acquires knowledge about third-party software.

## Context

Endora has two learning loops and is missing the third.

- About the **person**: beliefs carrying evidence and confidence, affirmed, corrected,
  decayed ([0020](0020-intent-first-understanding-loop.md),
  [0032](0032-beliefs-decay-and-expire.md)).
- About **models**: a fitness battery and a deterministic adoption policy
  ([0027](0027-self-improving-model-layer.md), [0030](0030-measuring-understanding.md)).
- About its **own tools**: nothing. Every fact Endora knows about a capability is a
  string literal in its source, added after someone was burned once.

The inventory, today:

| where | what it hardcodes |
| --- | --- |
| `McpRunner::is_state_reader` | `"GetLiveContext"` + `Hass*` |
| `McpRunner::verifier` | `Hass*` → `{server}.GetLiveContext` |
| `drop_domain_word_name` | `Hass*` |
| `reject_no_op_light_set` | `HassLightSet` |
| `flag_ambiguous_names` | Home Assistant's `names:`/`domain:` text format |
| `mcp_stdio` — the **transport** | Home Assistant's `action_done` envelope |

The last is the worst: one integration's response format is parsed inside the generic
MCP stdio client, below the layer that is supposed to know what an integration is.

[ADR 0034](0034-evidence-verifies.md) named this failure mode precisely, and then did it
again:

> Each was fixed individually. That is the wrong shape of work: it implies a permanent
> obligation to discover and hand-patch every quirk of every integration, forever, and it
> only ever protects against failures that have already happened to someone.

That ADR then added the `Hass*` verifier mapping; a later fix added `is_state_reader`.
Both were correct in isolation. Both are the pattern.

**The cost is not tidiness.** Every non-Home-Assistant server is permanently
unverifiable. Not unsafe — deny-by-default holds and results stay `[unverified]`, which
is honest — but a person who connects a calendar or a filesystem server can never get
read-back, can never have a read classified as evidence, and no amount of using it will
change that, because the mapping that would do so is a name in Endora's source.

Meanwhile the protocol already carries part of the answer and Endora throws it away:
`tools/list` may return `annotations` (`readOnlyHint`, `destructiveHint`,
`idempotentHint`). The client parses `name`, `description` and `inputSchema`, and drops
annotations entirely.

## Decision

**Endora keeps a profile for each capability, built the way it builds understanding of a
person: facts with provenance, that can be proposed, confirmed, corrected and
re-derived.**

Three sources, ranked by trust. The ranking *is* the design:

1. **Declared** — what the server says about itself: annotations, description, schema.
   A **proposal**, never authoritative. A server announcing "I only read" is not evidence
   of anything; a careless or hostile one says exactly the same.
2. **Observed** — what Endora saw happen. Outcomes ([0035](0035-outcomes-what-happened-after-acting.md))
   already record, per run, what a capability claimed and what the read-back showed. That
   stream is evidence, and it accrues whether or not anyone is paying attention.
3. **Confirmed** — what the person said. Authoritative.

**Deterministic policy consumes confirmed facts. Declared facts are surfaced, never
obeyed.** This is *models propose, policy authorizes* ([0005](0005-models-propose-policy-authorizes.md))
applied to third-party servers instead of the language model — the same reasoning, the
same boundary, a different untrusted party.

The facts worth holding **now**, deliberately few:

- **`reads_only`** — its result is evidence rather than a receipt. Replaces
  `is_state_reader`.
- **`verified_by`** — which capability observes what this one changes. Replaces
  `verifier`.

Both are name-matched strings today. Both become data, supplied once per server.

### What observation derives on its own

This is what makes the profile *accumulate* rather than be one more settings screen. Two
derivations are available from data already stored, for any server, with no knowledge of
what the server is:

- **A capability that claims success while the read-back shows nothing changed** is
  either lying or being misused. Once is an accident; a pattern across runs is a finding.
  That is exactly the `HassLightSet` defect — derivable without knowing what Home
  Assistant is.
- **A capability whose result never coincides with a changed observation** is very
  likely a read, which is evidence for `reads_only`.

**Neither derivation acts.** Both surface, as proposals, alongside what the server
declared. Policy still waits for the person.

### Where integration quirks go

Some knowledge genuinely cannot generalise: a server whose `name` field rejects domain
words, a call shape that is a no-op, a bespoke response envelope. That knowledge stays
**code** — but in a named per-integration adapter, not scattered through the shared
runner and never in the transport.

The test for which side something belongs on: *could another server reasonably need the
opposite behaviour?* If yes, it is a quirk, and it lives behind that integration's own
boundary.

## Consequences

- Any MCP server can get read-back, evidence classification, and eventually ambiguity
  flagging, without Endora shipping a line of code about it.
- Onboarding a new integration needs a person answering one question — *which tool reads
  this server's state?* — not a patch release.
- Endora's knowledge of its own tools becomes visible and correctable, exactly as its
  knowledge of the person is. Memory rights extend to it.
- The six hardcodes reduce to: two become data, three move behind a Home Assistant
  adapter, one leaves the transport entirely.
- More surface to store, show and export. Accepted.
- **The risk worth naming: this could become an approval queue.** A stream of "confirm
  what this server claims" prompts is [ADR 0029](0029-delete-the-goal-tracker.md)'s
  mistake wearing new clothes. Mitigations, which are load-bearing rather than
  aspirational: nothing is ever *blocked* on a profile being confirmed — an unconfirmed
  capability simply keeps today's deny-by-default behaviour, which already works; and a
  proposal appears only where the person is already looking at that skill, never as a
  queue, badge or count.

## Alternatives considered

- **Trust MCP annotations directly.** Rejected. Policy would be taking an unvetted third
  party's word for whether something is safe, which is the whole thing
  [0005](0005-models-propose-policy-authorizes.md) exists to prevent. Annotations are
  worth *capturing* — they make a good proposal — but not obeying.
- **Infer read-only from the tool's name** (`Get*`, `List*`, `Read*`). Rejected. A
  heuristic that is usually right is the worst kind at a policy boundary, and
  `GetAndClear` is a real API shape.
- **Ask the model to classify tools.** Rejected. The model is measured at 1/3 on obeying
  an explicit, direct instruction about verification
  ([0034](0034-evidence-verifies.md)); it is not the enforcement boundary, and this is a
  policy input.
- **Keep patching per integration.** Rejected — it is what 0034 diagnosed, and it has
  since produced six hardcodes and a leak into the protocol adapter. The next integration
  starts the count again from zero.
- **Do nothing until a second integration exists.** Tempting, and it is the "avoid
  speculative abstractions" reading. Rejected because the abstraction is not speculative:
  six concrete instances exist, the failure they cause has been observed live, and the
  data the profile would consume is already being recorded.
