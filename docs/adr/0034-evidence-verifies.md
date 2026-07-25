# 0034 — Evidence verifies: an unobserved effect is never reported as fact

## Status

Accepted (2026-07-25). Completes the fourth clause of the architecture principle in
[docs/direction-reset.md](../direction-reset.md). Extends the turn contract of
[0028](0028-native-tool-calling-turn.md) and sits alongside the policy boundary of
[0005](0005-models-propose-policy-authorizes.md) / [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md).

## Context

The direction reset states the architecture in five clauses:

> **Models propose. Policy authorizes. Capabilities execute. Evidence verifies.
> Memory learns.**

Four were built. **"Evidence verifies" had no implementation at all** — not a partial
one, zero lines. The turn proposed, authorized, executed, and then *narrated*, with no
step in between that looked at the world.

That is not an abstract gap. Three production defects in a single day traced to it:

| symptom | what it actually was |
| --- | --- |
| the model put a device *kind* in HA's `name` field | the call did not match the request; nothing checked |
| `HassLightSet` called with no brightness or colour | the actuator reported success having changed nothing |
| Assist JSON read as a search result | the result was illegible, and nothing was observed instead |

Each was fixed individually. That is the wrong shape of work: it implies a permanent
obligation to discover and hand-patch every quirk of every integration, forever, and
it only ever protects against failures that have already happened to someone.

The worst of the three is the second, and it is worth being precise about why. Asked
to turn a light off, the butler called a tool that only sets brightness. Home
Assistant matched the targets, changed nothing, and honestly answered `action_done`
with an empty `failed` list. The butler announced success. **The light stayed on.**

Every honesty guarantee built so far assumes *the tool result is true*.
[ADR 0028](0028-native-tool-calling-turn.md)'s whole argument is that a model grounded
in a real result will report it faithfully — and that reasoning collapses when the
result says everything went fine and nothing did. A false success is **unfalsifiable
from inside the conversation**. It was caught by a person walking over and looking at
the light.

## Decision

**An effect that was not observed is never reported as though it were fact.**

The turn distinguishes two kinds of tool result:

- **An observation.** A capability in the `Reversibility::Observe` band reports state.
  Its result *is* evidence and stands on its own.
- **A receipt.** Anything else returns the actuator's account of its own work — which
  is exactly the thing that can be wrong. Its result is annotated:

  > `[unverified]` This is what the tool reported about its own work. Endora has NOT
  > independently confirmed the effect. Say what was reported and that it is
  > unconfirmed — do not state the world has changed as though you had checked.

The annotation travels in the **tool result**, the channel ADR 0028 established for
grounding the model in what actually happened. It is not deterministic narration:
whether an effect was confirmed is part of the real outcome, and the butler still
writes its own words.

**Failing closed.** A capability whose band is unknown is treated as an actuator.
MCP servers tell us nothing about whether a given tool reads or writes, so every MCP
result is a receipt until an integration says otherwise. The default is honest for
integrations nobody has debugged yet — which is the whole point.

**Bands were made truthful.** Eight built-in read-only skills were declared
`Reversible` with comments saying "read-only lookup". They are now `Observe`. This is
policy-neutral (both bands map to `Decision::Act`) and it is what lets the band carry
the observation/receipt distinction at all.

## Consequences

- The class of bug is closed rather than three instances of it. A future integration
  that lies about its own success produces an *unconfirmed* report, not a false one,
  without anyone having to discover its quirks first.
- The butler will sometimes be less assertive than it could be — saying "Home
  Assistant reported it done, though I haven't confirmed it" where the action did in
  fact work. **That is the correct trade.** Overclaiming is unfalsifiable; hedging is
  merely wordy, and the fix for the wordiness is to *actually verify*, which is the
  next layer.
- **This does not stop the model choosing the wrong tool.** It means the person finds
  out. Wrong-tool selection is a separate axis, addressed by measurement (the
  `select:turn-off-not-light-set` eval cases), a smaller tool surface, or a better
  model — not by more per-tool patches.
- `CapabilitySpec` gains `reversibility`; `run_tool_turn` annotates on the success
  path only, since a failure is already unambiguous.

## What comes next (deliberately not in this ADR)

**Read-back.** A capability declares how to observe its effect, and the turn performs
that read after acting — so the model receives *observed state* rather than the
actuator's claim, and `[unverified]` becomes `[confirmed]` or `[contradicted]`. Home
Assistant already exposes `HassGetState`, so it is the natural first integration. That
is one mapping per integration rather than one patch per tool, and it turns hedging
back into confidence honestly.

This ADR deliberately ships the universal, zero-configuration half first: it is what
makes every integration safe by default, including the ones not written yet.

## Alternatives considered

- **Keep patching individual tools.** Rejected — it is unbounded work, it only ever
  covers failures already suffered, and it leaves every new integration unsafe until
  someone gets burned by it.
- **Validate calls against the tool's JSON Schema before dispatch.** Worth doing as
  hygiene, and it catches malformed arguments — but not this. `HassLightSet` with no
  brightness is schema-valid; brightness is optional.
- **Have the model self-check ("are you sure it worked?").** Rejected on the standing
  ADR 0028/0030 grounds: the model is the component that was wrong, and asking it to
  audit itself is circular.
- **Always read back before reporting anything.** The right end state, but it needs a
  per-capability mapping that does not exist yet, and shipping it first would have
  left every un-mapped integration exactly as unsafe as before.
- **Suppress the annotation when the tool "looks" successful.** That is precisely the
  failure: the tool did look successful.
