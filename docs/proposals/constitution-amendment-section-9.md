# ADOPTED — Amendment to Constitution §9

> **Status: ADOPTED 2026-07-25** by the maintainer (@RusticPotatoes), in force in
> [docs/constitution.md](../constitution.md).
>
> Constitution §11 states: *"Endora may **never** modify this constitution
> autonomously… A model may draft a proposal; only maintainers, acting deliberately,
> may adopt one."* This file was that draft, written with AI assistance and left
> unapplied until the maintainer chose it from the options below.
>
> It is kept as the record of the deliberation — what the alternatives were, and why
> this one was taken. A constitutional change should be readable back years later
> with its reasoning attached, not just as a diff.

## Why an amendment is needed

§9 currently reads:

> **9. Evidence-driven adaptation**
>
> Endora improves through an explicit, inspectable loop:
>
> ```text
> Direction → Assumption → Experiment → Observation → Reflection
>           → Proposed process change → Human approval
> ```

Every named step in that diagram was deleted by
[ADR 0029](../adr/0029-delete-the-goal-tracker.md). `Direction`, `Assumption`,
`Experiment`, `Observation`, `Reflection` and `ProposedProcessChange` no longer exist
in the code, the protocol, or the UI.

This matters more than a stale reference. The constitution is the **outermost limit**
(§7), and a limit that describes machinery nobody can point at cannot be checked
against the system. Either the clause governs something real, or it is decoration —
and a constitution with decorative clauses invites treating the load-bearing ones the
same way.

The *principle* in §9 is intact and worth keeping. What changed is the mechanism.

## What is being proposed

Replace the body of §9 with the loop Endora actually runs, preserving the two
commitments the original clause exists to make: adaptation is **inspectable**, and
process change is **proposed rather than imposed**.

> **9. Evidence-driven adaptation**
>
> Endora improves through an explicit, inspectable loop:
>
> ```text
> Observe → Understand (beliefs, with evidence and confidence)
>         → Act within policy → Observe the outcome
>         → Reflect → Update or let go of the understanding
> ```
>
> - Endora forms and revises its **own model of the person** on its own. That model
>   is not an action, so it is not gated on per-item approval — but it must remain
>   **visible, correctable, and able to expire**. Nothing is held permanently.
> - Adaptation of Endora's **processes** — how it works, what it may do
>   unsupervised, which model reasons on its behalf — is **proposed, not imposed**.
>   The final step is human approval.
> - Endora improves **processes more readily than values.** Changing *how* it works
>   is routine; changing *what it is for* is not something it may do.

## What is preserved, and what changes

**Preserved.** The loop is still explicit and inspectable. Process change still ends
in human approval. The values/processes asymmetry is unchanged. The third bullet is
verbatim from the current text.

**Changed.** The loop names what exists. The first bullet makes explicit something
already true in code but never stated constitutionally: Endora's *understanding* is
formed autonomously and is **not** subject to per-item approval — that friction is
what ADR 0020 §3 removed — while remaining correctable and impermanent.

That first bullet is the substantive change, and the one worth arguing about. It
grants Endora standing authority over its own model of a person. Three existing
guarantees bound it, and a reviewer should check each is genuinely sufficient before
adopting:

1. Every belief is visible and correctable (§6, and the Understanding view).
2. Beliefs decay and expire without reinforcement
   ([ADR 0032](../adr/0032-beliefs-decay-and-expire.md)), so a wrong belief has a
   bounded life even if the person never notices it.
3. Belief quality is measured, not assumed
   ([ADR 0030](../adr/0030-measuring-understanding.md)).

## The options that were on the table

1. **Adopt as drafted** — constitution matches the code; the carve-out is explicit
   and bounded by §4 and §6. **← chosen**
2. **Adopt the loop diagram only**, leaving the bullets untouched. Fixes the dead
   machinery without changing any protection, but leaves an unqualified "adaptation
   is proposed, not imposed" standing against every future autonomy step.
3. **Reject the carve-out and change the code instead** — reintroduce per-item
   approval for beliefs, undoing ADR 0020 §3.

## Why option 1 was taken

The decision turned on a distinction worth keeping in view for every future change
of this kind:

- **What Endora may think** — its model of the person. Free-forming, bounded by
  visibility, correctability, expiry (ADR 0032) and measurement (ADR 0030).
- **What Endora may do** — actions in the world. Still gated by the reversibility
  bands, deterministic policy, and audit. **Untouched by this amendment.**

§9's original bullet was unqualified, so it sat across both — a blanket rule where a
targeted one belongs. This amendment moves the boundary to the right place rather
than removing it. It grants **no** new authority to act.

The test that justified it, and which should be applied to anything similar:
*does this change what Endora may do to the world, or only what it may think?*
Thinking-only changes are the safe class. Widening the autonomy envelope
(ADRs 0022/0024) is not, and should face a far higher bar.

**Noted for the record:** the draft was written by an AI assistant in the same
session that produced much of the code it accommodates. That is a conflict of
interest. It was disclosed at the time, the alternatives above were put alongside the
recommendation, and the maintainer chose deliberately with the conflict stated.
