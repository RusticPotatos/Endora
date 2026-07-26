# 0040 — Withdrawing a capability that never works

## Status

Accepted (2026-07-26). Extends [0039](0039-capability-repair-proposals.md) with a second
remedy, and completes the "observed" source of [0038](0038-capability-profiles.md).

## Context

[ADR 0039](0039-capability-repair-proposals.md) gave Endora one answer to one question:
*this tool does not work on this thing* → **what is it really called?**, stored as a
`TargetAlias`. That answer is correct for the case it was built from, and it works: the
kitchen table light comes on now.

It is also the *only* answer available, and the live records show a failure it cannot
touch:

```
HassLightSet{"area":"kitchen","domain":["light"],"name":"table"}
→ error: HassLightSet only changes brightness or colour, and none were given,
  so this would have done nothing. Use HassTurnOn or HassTurnOff instead.
```

The model reached for a **brightness setter** to switch a light on. No name fixes that.
The alias retry fired, substituted the confirmed name, and failed identically — because
the target was never the problem. Asking the person "what is the table really called?"
here wastes the one answer they give us.

The obvious fix is to stop offering `HassLightSet`. **That is not a fix, it is a patch** —
one more name in Endora's source, which is precisely the shape of work
[0034](0034-evidence-verifies.md) diagnosed and [0038](0038-capability-profiles.md)
exists to stop. The next server brings its own confusable pair and the count starts at
zero again.

So: what is the *general* version of "that is the wrong tool"?

## Decision

**A capability the person has turned off is not offered to the model at all — for every
kind of capability, from every source — and Endora derives when to propose that from the
width of the failure.**

Two parts, and the first is the one that generalises.

### The remedy follows from how wide the failure is

Both findings come from the same stored outcomes, and are told apart by arithmetic that
knows nothing about any particular server:

| what the history shows | what it means | what is asked |
| --- | --- | --- |
| fails on **one** target, works on others | the name is wrong | *what is it really called?* → alias |
| **refuses** on **several** targets, has **never** worked | the tool is wrong | *stop offering it?* → withdrawal |

That is the whole rule. It reads the same for a calendar, a filesystem or a light, which
is the property that made it worth building instead of hiding `HassLightSet` by name.

Deliberately conservative, because the consequence is a tool disappearing:

- **only an outright refusal counts.** See below — the first version of this rule counted
  "changed nothing" too, and production immediately proposed withdrawing the most useful
  tool in the house.

- **width is counted over targets that are each already a pattern.** One stray failure at
  the garage must not escalate a plain alias question into a withdrawal.
- **any success at all disqualifies it.** Including an *unverified* one: `changed: None`
  on a call that did not error means nobody could check, which is 0038's honest silence
  and not evidence of a problem.
- **it proposes.** The person turns the tool off; Endora never does. Same boundary as
  everywhere else — [models propose, policy authorizes](0005-models-propose-policy-authorizes.md),
  and here the *derivation* proposes too.

### Only unambiguous evidence (corrected 2026-07-26, from production)

The first version of this rule treated the two kinds of failure ADR 0039 counts as
interchangeable: an outright `error:` and a "reported success, changed nothing". Deployed,
it immediately proposed **withdrawing `HassTurnOn`** — the most useful tool in the house —
on 15 attempts.

The records show why:

```
home-assistant.HassTurnOn   ERROR   changed: None    ×9
home-assistant.HassTurnOn   ok      changed: false   ×5
```

Those five worked. One of them is the call that turned on every light in the house. They
read as `changed: false` because the read-back was scoped to something the action did not
touch — so every one was counted as a failure, and none of them registered as the tool
having ever worked.

The two evidence streams are therefore **not** interchangeable, and the asymmetry is the
point:

- **"changed nothing" is ambiguous.** Switching off an already-off light legitimately
  changes nothing, and a mis-scoped read-back reports no change on an action that plainly
  worked. Good enough to ask *what is this really called?* — being wrong costs a sentence.
- **"I cannot do this" is not ambiguous.** It is the capability itself reporting it did
  nothing, via Endora's own marker rather than any server's error format.

So a **withdrawal counts only refusals**, and **any** non-error result disqualifies the
capability entirely — including an unverified one. A no-op history still derives the alias
question, exactly as ADR 0039 built it. The strength of the evidence now matches the
strength of the claim.

The underlying read-back defect is real and separate; this rule means it can no longer
cost anyone a working tool.

### A finding nobody can answer is not a finding

Also live: `{"area":null,"name":null}` derived *"2 attempts aimed at "" didn't work — what
is it actually called?"*. There is no *it*. The failure was the model not saying what it
meant, which the runner already refuses. A finding with no target is dropped rather than
shown as an unanswerable question.

### Withdrawn, not blocked — and why that distinction matters

Endora already has a way to stop a tool being used: **blocked**, the deny-by-default band
from [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md). It is the wrong instrument here, and the reason
is measured rather than aesthetic.

A blocked tool is **still in the catalogue**. The failure being fixed is the model
*choosing* the wrong tool, and it chose `HassLightSet` again after being refused. Refusing
more loudly does not change a choice; the tool has to stop being on the menu.

- **Blocked** is for a tool that works and is consequential. It is safety.
- **Withdrawn** is for a tool that does not work. It is fitness.

Both stay, because they answer different questions.

### One meaning of "off", applied once, above everything

`capability_config.enabled` has existed since [0021](0021-capability-catalog-and-mcp-host.md) and was honoured
only by the built-in registry. For an MCP tool the flag could be set and **nothing
happened** — the tools come from a shared connection built long before the person's config
is read. Turning one off was a control that silently did nothing.

A single decorator now applies it above every source, so `off` means the same thing
whatever the capability is and wherever it came from. It hides the capability from the
catalogue, refuses to run it if named anyway, and refuses to nominate it as anything's
verifier — a withdrawn reader would be asked for a reading it can no longer produce, so
actions fall back to `[unverified]`, which is the honest state
([0034](0034-evidence-verifies.md)).

### Answering retires the finding

Proposals are still **derived, never stored** ([0039](0039-capability-repair-proposals.md)),
which normally means a finding disappears when the world changes. A withdrawn tool can
never produce new evidence, so its finding would sit there forever asking for something
already done. The derivation stays pure and unaware of config; the composition layer,
which knows both, drops findings whose answer has been given. No dismissal, no state —
answering *is* the retirement.

## Consequences

- The wrong-tool failure has a remedy that arrives with any server, not just this one.
  Nothing in the derivation names Home Assistant.
- Turning off an MCP tool works. It previously appeared to and did not.
- **A withdrawal can be wrong.** A tool might have refused twice on names the person could
  have fixed with an alias. Cost: they turn it back on, one click, nothing lost — and the
  card says where. This is why success disqualifies, why width is counted strictly, and
  why only refusals count.
- Withdrawing a tool **removes a capability**. Turning off `HassLightSet` means no
  brightness control, and the proposal has to say so rather than presenting it as free.
- The `reject_no_op_light_set` hardcode **stays**, and is now load-bearing in a second
  way: it is what turns a silent no-op into the error that makes this finding derivable.
  Per-integration knowledge is still allowed where it genuinely cannot generalise
  ([0038](0038-capability-profiles.md)) — what changed is that the *response* to it no
  longer has to be hand-written per integration.
- Two ways to stop a tool is more surface to explain. Accepted: they answer different
  questions, and collapsing them would mean either blocking things that are merely broken
  or hiding things that are merely dangerous.

## Alternatives considered

- **Hide `HassLightSet` by name.** Rejected — the patch this ADR exists to avoid. It also
  removes brightness control silently, with no record of why and no way back.
- **Improve the prompt so the model picks better.** Tried, repeatedly. Three eval
  hypotheses about this exact confusion were refuted by measurement, and this model obeys
  an explicit instruction about verification roughly 1 run in 3
  ([0034](0034-evidence-verifies.md)). Prompting is not where a guarantee goes.
- **Withdraw automatically once the evidence is in.** Rejected. It removes a capability
  from a heuristic over a handful of samples, and "Endora quietly stopped being able to do
  that" is exactly the kind of silent narrowing that erodes trust.
- **Reuse `blocked` instead of adding withdrawal.** Rejected; see above. A blocked tool is
  still offered, and being offered is the problem.
- **Let the model decide which of its tools are useless.** Rejected. It is the component
  whose bad choice created the finding.
- **Store the proposal so it can be dismissed.** Rejected, for [0029](0029-delete-the-goal-tracker.md)'s
  reason. A dismissible record is a queue with extra steps.
