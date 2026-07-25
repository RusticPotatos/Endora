# DRAFT — Proposed amendment to Constitution §9

> **Status: PROPOSAL. Not adopted. Not in force.**
>
> Constitution §11 states: *"Endora may **never** modify this constitution
> autonomously… A model may draft a proposal; only maintainers, acting deliberately,
> may adopt one."*
>
> This file is that draft, and nothing more. It was written with AI assistance and
> deliberately **not** applied to [docs/constitution.md](../constitution.md). Adopting
> or rejecting it is a human decision, taken through
> [GOVERNANCE.md](../../GOVERNANCE.md).

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

## Recommendation to the maintainer

Adopt, reject, or amend — but **do not leave §9 as it stands**. The current text
governs deleted machinery, and that is the worst of the three options.

If adopting: per §11 and GOVERNANCE.md this is a deliberate human act, and the ADR
accompanying it should record that the constitutional layer was changed, not just the
architecture.
