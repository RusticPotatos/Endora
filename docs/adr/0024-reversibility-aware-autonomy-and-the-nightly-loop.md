# 0024 — Reversibility-aware autonomy and the nightly self-improvement loop

## Status

Accepted (2026). Refines
[ADR 0022](0022-autonomy-envelope-and-self-authored-capabilities.md) (the autonomy
envelope) and builds on [ADR 0019](0019-proactive-self-improving-butler.md) (the
proactive butler and its learning loop), [ADR 0020](0020-intent-first-understanding-loop.md)
(understanding), and [ADR 0005](0005-models-propose-policy-authorizes.md).

**Reversibility bands implemented** (the primary axis): `shared/kernel` owns the
`Reversibility` band (`Observe` / `Reversible` / `OutwardReversible` /
`Irreversible`, deny-by-default) and its `Decision` (`Act` / `Confirm` / `Block`),
with the un-undoable mapping to `Block` — refused outright, not merely confirmed.
Each capability **declares its band natively** (`CapabilityInfo.reversibility`,
which replaced the old `reversible` bool and subsumed the per-capability autonomy
level), and the classifier maps that band + reach + the person's envelope to a
`Decision`: the kernel owns the envelope-independent posture
(`Reversibility::default_decision`) and the envelope only *widens*
(`auto_consequential`) or *narrows* (`auto_external`) it — never past the
irreversible block. The execution path (`RegistryRunner::run`) blocks the
irreversible band deny-by-default, and the skills API surfaces the band
(`reversibility`).

The **per-capability opener** is implemented: the person can deliberately open a
specific capability's irreversible band (`CapabilityConfigRepository::set_open_irreversible`,
`POST /v1/capabilities/{id}/open`, a Skills-view control), which moves it from
`Block` to `Confirm` — **never** to `Act`, so an opened irreversible skill is
confirmed on every use and never runs autonomously. Opening a skill records a line
to the action feed. **Pending**: recording the *classified decision* per action in
the audit trail (beyond the open/close event), and the **nightly self-improvement
loop** below.

## Context

The vision is a butler that works **while the person sleeps** and whenever they want
something — always on, always learning. Two things stand in the way of doing that
safely:

1. **The autonomy envelope is too coarse.** Today it is effectively 0/1
   (`auto_external`, `auto_consequential`). The person's actual wish is finer:
   *let it experiment freely, but never do anything that can't be undone.* "Some
   things can't be undone" — sending, spending, editing or deleting external state.
2. **The always-on loop doesn't exist yet.** The domain already models a learning
   loop (`Experiment → Observation → Reflection`) and there is a heartbeat/check-in
   scheduler, but nothing applies that loop to the butler itself overnight.

## Decision

### Reversibility as the primary axis

Replace the coarse on/off with a **reversibility-first** classification. Every action
a capability can take is graded, from declared metadata (never the model's say-so):

- **Observe** — read and think only.
- **Reversible / internal** — research, draft, form beliefs, run experiments. Has an
  undo or no lasting effect. **Autonomous by default.** This is the *experiment band*.
- **Outward but reversible** — an effect the person can still undo (e.g. a draft
  posted somewhere deletable). **Confirm.**
- **Irreversible / consequential** — send, spend, edit or delete external state.
  **Blocked by default — "never, for now"**, not merely "confirm."

The default posture becomes exactly the person's rule: **experiment freely, never do
the un-undoable.** A capability declares `reversible` (and reach/cost) in its
metadata; the deterministic classifier (ADR 0022) maps that + the person's envelope
to *act / confirm / block*. Unknown or unclassifiable ⇒ treated as irreversible ⇒
blocked. The envelope can widen bands, but the irreversible band stays blocked until
the person explicitly opens it (and even then, per-capability and confirmable).

### The nightly self-improvement loop

A scheduled job (extending the heartbeat scheduler, ADR 0019) that, on a cadence the
person chooses, applies the learning loop to the butler itself:

- **Review** the day's conversations and the Understanding (beliefs) it formed.
- **Experiment** — run **reversible** work: research topics the person cares about,
  form and test hypotheses, prepare drafts (existing `Assumption`/`Experiment`).
- **Reflect** — record what it learned (`Reflection`), refining Understanding.
- **Surface** — post a **briefing** at a chosen time (morning/evening), and log
  everything to the action feed (ADR 0012) so the person sees exactly what it did.

The loop runs **entirely within the reversible band**. It can prepare, research, and
propose; it cannot send, spend, or edit anything.

### Invariants

- Nothing **irreversible or outbound** happens without explicit human authorization —
  the default is *blocked*, not *confirm*.
- Classification is **deterministic** (declared metadata), never model self-report.
- All autonomous activity is **auditable** (action feed) and its learnings are
  **correctable** (beliefs).

## Consequences

- The butler can run all night usefully and safely: it experiments and learns, and
  the person wakes to a briefing — with a hard guarantee it changed nothing it
  couldn't undo.
- New surface: a `reversible` (and cost/reach) axis on capability metadata; the
  classifier gains bands; a scheduler for the nightly loop; a briefing composer.
- Main risk: **mis-classifying** an irreversible action as reversible. Mitigated by
  deny-by-default on unknown, and by the irreversible band being *blocked* rather than
  *confirm* — the failure mode is "it asked / it didn't act," never "it did something
  permanent."

## Alternatives considered

- **Keep the coarse on/off.** Too blunt to express "experiment but never the
  irreversible." Rejected.
- **Let the model judge reversibility.** The model is never the enforcement boundary
  (ADR 0005). Rejected.
- **Allow irreversible actions with a confirm.** Deferred: for now the irreversible
  band is blocked outright, because a mistaken confirm is unrecoverable. It can be
  opened later, per-capability, when the person chooses.
