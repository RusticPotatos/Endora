# 0029 — Delete the goal tracker; understanding is the only model

## Status

Accepted (2026-07-25). **Supersedes [0020](0020-intent-first-understanding-loop.md) §4**
("Goals are demoted, not deleted"), and with it the goal-centric parts of
[0006](0006-first-vertical-slice.md), [0011](0011-review-scheduling-reminders.md),
[0013](0013-rename-goal-to-target.md), [0014](0014-the-butler-conversation-values-attention.md),
[0015](0015-identity-and-values-context.md) and [0016](0016-adaptive-attention.md).
The invariant of [0005](0005-models-propose-policy-authorizes.md) — *models propose,
deterministic policy authorizes* — is reaffirmed, not changed.

## Context

[ADR 0020](0020-intent-first-understanding-loop.md) reset the product: Endora is an
autonomous personal intelligence whose job is to understand a person, not a place for
that person to file goals. But §4 hedged — *"we do not rip it out — the reset is
reversible (this branch), and goals still serve people who want them."*

That hedge was reasonable then. It is not what happened. Three releases later the
Direction/Target/Assumption/Experiment/Observation/Reflection/ProcessChange machinery
was still:

- **12 of 28 tables**, ~**680 lines** of use cases, the whole `domains/direction`
  crate (~2,300 lines), ~**25 of 60** HTTP routes, and five console views;
- the thing `butler_context` was mostly assembled from — it took **four**
  goal-tracker repositories to build one turn's context;
- the source of `nightly_focus`, so Endora's most autonomous behaviour picked its
  overnight research topic from an active North Star;
- the entire content of `ButlerProposal`, so the "butler proposes, you confirm"
  inbox existed only to create goal-tracker rows.

And it had reached the prompt. `BUTLER_SYSTEM_PROMPT` told the model *"you are not a
goal tracker or task manager"* and then appended `What they're working toward (id |
what | status | area | has next step)` and a `create_value` / `create_north_star` /
`create_target` proposal schema for it to fill in. It also told the model never to say
"value", "goal", or "proposal" — words the schema it was being handed is made of.

A weak local model cannot be prompted out of a schema it keeps being given. The
contradiction *was* the defect, and only one side of it could be deleted.

Two things follow from 0020 that make the choice clear. Its own guiding test — *would
Endora still feel like an autonomous personal intelligence without this?* — answers
**yes** for every one of these concepts. And its §3 already drew the real line:
**beliefs** are Endora's model, formed on its own and corrected by the person;
**interventions** are actions in the world, gated by policy. A queue of records for
the person to approve later is neither.

## Decision

**Delete the goal tracker.** Not deprecate, not hide — remove, and record that the
reset is no longer reversible.

Removed: `Value`, `Direction` (North Star), `Target`, `Assumption`, `Experiment`,
`Observation`, `Reflection`, `ProposedProcessChange`, the attention/snooze read model,
and the `Suggestion` inbox with its `ButlerProposal` closed set — across domain,
application, persistence, HTTP, CLI and console. The `domains/direction` crate is
retired, as is the `Proposer` port, whose only job was drafting process changes.

**Understanding is the only model Endora keeps of a person.** Beliefs already carry
what goals were standing in for — what someone is reaching for, with the evidence,
a confidence, and the ability to be corrected or to expire. `ButlerContext` now
carries what Endora understands and the skills it can reach, and nothing else.
`nightly_focus` takes its topic from the **intent Endora is most sure of**;
confidence is the ranking, because spending the night researching a tentative guess
is spending it on something Endora may simply be wrong about.

**The butler no longer files anything for later approval.** It converses, and it acts
through the policy boundary as it goes. This is not a loosening: an action still
passes deterministic authorization before it runs (0005/0024). What is gone is the
intermediate step where the model wrote a *record* proposing to change the person's
profile, which the person then had to review — friction 0020 §3 explicitly set out to
remove.

**Existing databases shed the old tables on open.** Leaving them in place would
strand data the person can no longer see, correct, or delete, which is exactly what
[constitution §6](../../constitution.md) forbids.

## Consequences

- The turn contract simplifies: `ButlerReply` loses `proposals`, and the chat
  use cases lose the `SuggestionRepository` they no longer wrote to.
- The system prompt loses the schema it was contradicting itself over, and with it
  the instructions that existed only to police that contradiction.
- **Breaking:** the `/v1/values`, `/v1/directions`, `/v1/targets`, `/v1/assumptions`,
  `/v1/experiments`, `/v1/reviews`, `/v1/observations`, `/v1/reflections`,
  `/v1/process-changes`, `/v1/suggestions` and `/v1/attention` endpoints are gone,
  along with their CLI commands; `/v1/export` no longer carries those collections.
  Acceptable in `0.x` (see [SECURITY.md](../../../SECURITY.md) on supported versions).
- ~8,600 net lines leave the codebase. The safety machinery — reversibility bands,
  deny-by-default, the egress guard, the autonomy envelope, the audit trail — is
  untouched. This ADR changes what Endora is *for*, not what it is allowed to do.
- **Cost, stated plainly:** a person who genuinely wanted to keep a goal list no
  longer can. That is the trade — Endora is meant to do the thinking rather than give
  the person somewhere to file it, and it cannot credibly claim that while shipping
  the filing cabinet.
- **Risk:** understanding now has no structured fallback. If belief-forming is weak
  on a given model, Endora has less to go on than before, where an explicit North Star
  could carry it. Accepted: that fallback is what let the goal tracker persist, and
  belief quality is measurable by the [0027](0027-self-improving-model-layer.md) eval
  battery, which is where the pressure belongs.

## Alternatives considered

- **Keep it behind a feature flag.** Rejected — the cost was never the runtime, it
  was the concept's presence in the prompt, the context, and every contributor's
  mental model. A flag removes none of that.
- **Keep Value/North Star/Target and cut only the experiment loop.** Tempting: it is
  the smaller diff. Rejected because those three were the part actually wired into
  the prompt and the nightly loop, so the contradiction would have survived the cut
  that was supposed to end it.
- **Re-derive North Stars from high-confidence intent beliefs, and keep the views.**
  Rejected *for now*, not on principle — it is one concept presented two ways, and
  worth revisiting once belief quality is measured. Doing it during the teardown
  would have rebuilt the thing being removed.
