# 0109 — A brain per member, and what a cloud turn may read

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0102](0102-endora-hq.md) (Claude through the relay, per member, now exists),
[0106](0106-team-contracts.md) (the log rule owed before the first Claude member) and
[0108](0108-the-bridge.md) (the Confidential tier for cloud turns). Built: the member's brain
in HQ's store and on its screen, the runner pointing the worker at the matching relay, the
worker speaking Anthropic's Messages API, the node's cloud check point fed by HQ, the
contracts and the bridge applying the rule below, and the Anthropic relay refusing a turn on
the house's model. Lessons kept by the team and the harm rule were built with it.

Amended 2026-10-17 by [0111](0111-the-ceo-and-the-chain-of-command.md), the owner's decisions:
a **CEO** member above the lead, which may think with **Claude Opus** (Opus is the CEO's alone),
Sonnet or the house's model, and whose turns may spend **$1.00** (everyone else's $0.40). The
brains by role are now: CEO Opus, Sonnet or Local; lead, developer and reviewer Sonnet, Haiku or
Local; researcher Haiku or Local; Claude only, with no other provider. **The pool is shared only
among the members with a paid brain**, by their roles' weights (CEO 35, lead 25, developer 40,
reviewer 20, researcher 15): a member on the house's model costs nothing, reserves nothing and is
never turned away for money. The CEO on Claude is a cloud member that is not the lead: it is
given nothing Confidential, and reads its members' findings in their threads.

Cost notes added 2026-10-17 (see [Cost notes](#cost-notes)): prompt caching, effort by role and
purpose, and lean briefings for the CEO. Amended 2026-10-17 by
[0114](0114-evidence-acceptance-criteria-and-the-tester.md): the CEO's effort follows the kind of
turn, `low`, `medium` or `high`.

Amended 2026-10-18 by [0131](0131-the-two-kinds-of-turn.md): a member's brain is now two, a
**work brain** (the Mac or the house's model) and a **judgment brain** (a Claude its role may
use, if the owner gave it one); the brain a turn runs on is decided per turn by why it was asked
for, and the paid share above is reckoned on the judgment brain alone. A wake refused for budget
is no longer thrown away: its turn waits for budget.

## Context

Every member thought with the house's model (qwen3:8b, thinking). The lead coordinates the
team and decides what reaches the owner's desk; on a small model it put invented containers
and invented problems there, which the reviewer and the name check now catch, but only after
the fact. The owner chose to let the lead think with Claude Sonnet, paid from the team's pool.

A cloud model must not see what stays home. Container logs and the download queues are
Confidential (0105, 0106, 0108), and they are exactly what the lead needs to judge whether
something is wrong. 0106 recorded that the node must refuse logs to a cloud member, or the
owner must decide otherwise, before any member thought with Claude.

## Decision

**Owner decisions, 2026-10-17.**

1. **The lead may think with Claude Sonnet** (`claude-sonnet-5-5`) through the node's
   Anthropic relay, metered against the team's $70 pool and at most $0.40 a turn, as every
   relayed call already is. The other members stay on the house's model.
2. **The lead's cloud turns may receive two kinds of Confidential data**: **scrubbed**
   container logs (`container_logs`), and the download-queue reads with their stuck reasons
   (`sonarr_queue`, `radarr_queue`). Every other Confidential result (Home Assistant's state,
   the download actions, a paired tool server's tools) stays with turns on the house's model.
   Restricted stays home, as always. No other role's cloud turn receives any Confidential
   result.

**A brain per member.** Each member has a `brain`: `local` (the default) or a Claude model.
Which Claude a role may have is the plan's models by role, the rule the relay already enforces
on every call: Sonnet or Haiku for the lead, the developer and the reviewer; Haiku only for the
researcher; Opus for nobody until the owner can approve it for a task. HQ refuses anything else.
The owner sets it on the member's card on the HQ screen ("Brain: Local / Claude Sonnet / Claude
Haiku", only what the role allows), through the node, which records it in the audit log.

**How a cloud turn runs.** HQ hands the runner the member's brain with the turn. The runner
accepts only `local` or a plain model name, and gives the worker either the local relay, or the
Anthropic relay (`/team-relay/v1`, on the same team listener, so the fence is unchanged) and the
model's name. The worker speaks Anthropic's Messages API to it, through the same loop as the
local one: the same rounds, repeat guard, claim before the first round, line saying what the
turn did, and ends of a turn (401 or 402, a question or an approval on the desk).

**The check point.** HQ's answer to the node about a turn (`/v1/team/turn`) says `"cloud":
true` whenever the member's brain is not local, every call, so a switch takes effect at once.
The node then:

- lists and runs Confidential tools for a cloud turn only as decided above (one rule,
  `may_see` in `domains/capabilities/src/bridge.rs`, which the contracts use too);
- refuses, at the Anthropic relay, any call from a turn HQ does not say is cloud. A local turn
  may have been given Confidential results a cloud turn may not see; this keeps all of it home,
  whatever a worker sends.

## Consequences

- The lead judges from real logs and queues with a stronger model; its turns cost money from
  the pool, and the relay stops a turn at its budget as before.
- A cloud lead cannot run the download actions (retry the imports, remove and search again):
  they are Confidential and not among what the owner allowed. It can read the queue and ask the
  owner, or hand the fix to a member on the house's model.
- A member switched mid-turn is held to the stricter side at once: switched to the cloud, it is
  given only what a cloud turn may see; switched home, the Anthropic relay refuses its calls.
- Logs leave the house scrubbed, not disguised: the scrubbing removes secrets, not names. That
  is the owner's accepted trade for the lead.

## Cost notes

Added 2026-10-17, prepared with AI assistance (Claude), to spend less a cloud turn without
changing what the model is told (the plan's section 9, "Spending less a turn"):

- **Prompt caching.** The worker asks for Anthropic's automatic cache on every round, so each
  round reads at the cache price what the round before wrote. HQ's ledger prices a cache write
  at the one-hour price, the dearer, while the worker asks for the five-minute cache: the ledger
  overstates writes, and errs safe. The relay passes only the ephemeral cache, with a `ttl` of
  `5m` or `1h`, at the top of a call or on its blocks.
- **Effort.** Opus 5.5 cannot turn thinking off, and Sonnet 5.5 cannot either the way older
  models did; effort (`output_config.effort`) is the dial. HQ sets it per turn: the CEO
  `low` on an ordinary turn (a question up the chain, a post, a recovery, a routine wake that is
  not a review), `medium` on its daily review and its planning, `high` when a project proposal
  waits for its decision or it carries something to the board (the owner's decision of
  2026-10-17, [0114](0114-evidence-acceptance-criteria-and-the-tester.md)); the lead `medium`
  (Sonnet's own default is `high`); the researcher `low`. The relay passes only the five efforts, and no other part of
  `output_config`. The worker never forces a tool (`tool_choice` is left out, which is `auto`).
- **Lean briefings.** Only the CEO's first turn and daily review carry the whole house; other
  turns carry what changed since the last.
- **Measured.** Each run's record keeps the input tokens fresh, read and written, and its end
  says how much of its input the cache gave.

## Alternatives considered

- **Disguising logs through the door instead.** Rejected for now: the lead needs container names
  to act, and the door's disguise would rename them.
- **Letting every cloud member read logs.** Rejected by the owner: only the lead decides what
  goes on the desk, and the reviewer checks it against tools the lead's claims rest on.
- **The worker choosing its own model.** Rejected: the model is the owner's setting, carried
  by HQ and the runner; the relay checks the role's models on every call.
