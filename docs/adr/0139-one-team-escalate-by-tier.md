# 0139 - One team, escalate by tier

## Status

Amended by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.

Accepted, 2026-10-25, by the owner. AI-assisted. Amends
[0137](0137-addressed-conversations-and-bounded-assistance.md) and
[0138](0138-one-conversation-and-assistance-lifecycle.md) (who may ask whom), and the
org-chart routing in 0111, 0113, 0120, 0130 and 0131 as described below. Authority
rules in those decisions stand.

## Context

The team's members are one team. The owner, citing Conway's law: if the org chart
decides who may talk, the system mirrors the org chart instead of the work. Today
peer help needs a pair the owner authorized on one project, the colleague answers
on the cheapest model, and stuck work climbs people (lead, huddle, CEO) rather than
models. A stronger model is reached only because leads and the CEO happen to think
with Claude; nothing lets the same member retry a hard problem on a stronger tier.

The vendors' guidance, read 2026-10-25:

- Anthropic routes "easy/common questions to smaller, cost-efficient models" and
  harder ones to capable models ([Building effective agents][bea]); its advisor
  tool lets a cheaper executor consult a stronger model "when stuck — errors
  recurring, approach not converging" ([advisor tool][adv]). Gains appear only
  when the executor actually escalates.
- OpenAI mixes models by task and escalates to a human on "exceeding failure
  thresholds" and "high-risk actions" ([practical guide][oai]).
- Both caution against free-form multi-agent coordination: Anthropic's agents are
  "not yet great at coordinating and delegating to other agents in real time"
  ([multi-agent research][mar]); OpenAI: "maximize a single agent's capabilities
  first".

## Decision

1. **One team.** Any member may message or ask any other member. No owner-made pair
   and no shared project are required. Asks stay bounded (0138: one question to one
   colleague, a deadline, a recorded answer and finding); broadcast chat is not a
   feature. The owner keeps the global switch and may block a pair.
2. **Escalate by tier, then by authority.** A member stuck on a problem first takes
   it to a stronger model, not to a person: the next tier above the turn it is on
   (the house's model, then Claude Haiku, then Claude Sonnet; Opus stays the CEO's).
   The escalation is the same member's judgment turn on that tier, with the stuck
   turn's context. People are asked for decisions only: acceptance, owner approval,
   spending, risky or irreversible acts, which keep their existing authority paths.
3. **Help is charged where the work is.** An ask is charged to the asker's task's
   project, or to the team pool when it has no task. A colleague's answer runs on a
   tier at least the asker's, never forced onto the local model.
4. **Bounds.** Every paid tier stays inside the daily pool, member shares, project
   caps and per-turn maximum. An escalation is capped per task per day so a problem
   cannot bounce between tiers; past the cap, it becomes a decision for the person
   with authority. The Confidential guard, ask limits and deadlines are unchanged.

## Consequences

- `ask_owner` splits: help goes to a peer or a higher tier; a decision goes to the
  authority holder.
- The stuck ladder puts the tier step first, opens the huddle to any member, drops
  the hop to the CEO for an undecided lead, and replaces handing work up to a
  manager with a tier retry.
- Owner-made conversation pairs and project scoping are removed from admission;
  stored conversations stay readable as history.
- Paid model use may rise; it is bounded by existing caps and visible on Health.
- Shipped in steps recorded in the
  [checkpoint](../proposals/agent-collaboration-foundation-checkpoint-2026-10-22.md).

[bea]: https://www.anthropic.com/engineering/building-effective-agents
[adv]: https://platform.claude.com/docs/en/agents-and-tools/tool-use/advisor-tool
[mar]: https://www.anthropic.com/engineering/multi-agent-research-system
[oai]: https://cdn.openai.com/business-guides-and-resources/a-practical-guide-to-building-agents.pdf
