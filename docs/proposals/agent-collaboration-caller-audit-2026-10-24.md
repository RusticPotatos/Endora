# Agent collaboration: caller audit for ADR 0138

2026-10-24. AI-assisted, read-only audit of `codex/agent-collaboration` at
`d67dbc7e`, for [ADR 0138](../adr/0138-one-conversation-and-assistance-lifecycle.md)
definition of done items 1 and 2: every peer-help entry point uses the canonical
lifecycle, and "a repository/caller audit proves this". Every entry point that can
start, answer, deliver, recover, cancel or schedule peer help was classified.
Line references are at `d67dbc7e`.

## Entry points

| Entry point | Where | Classification |
|---|---|---|
| `consult` tool (v2) | `protocol.rs`, `protocol_history.rs` → `assistance.rs` `consult_peer_in` | Thin adapter: calls `request_assistance_in`, returns its id and state; no record fallback. |
| `request_assistance`, `answer_assistance`, `record_assistance_finding`, `cancel_assistance` | `engine/assistance.rs` | Canonical. |
| `read_messages`, `send_message` | `engine/conversations.rs`, `protocol_conversations.rs` | Canonical; information only, wakes no one. |
| `answer_consult` | `protocol.rs`, `protocol_history.rs` | Retired: refused and not advertised. |
| `ask_record` | `engine/record_search.rs` | Read-only, labelled historical testimony; never substitutes for a live answer. |
| `post` (`last_word`, agent @mentions) | `engine.rs` | Not peer help: wakes no askers; only owner-authored mentions wake. |
| `ask_owner` up the chain, and its reply | `engine.rs`, `domain/chain.rs` | Kept as management escalation (see below). |
| `Consulted` wakes and queued legacy turns | `engine.rs`, `engine/consult.rs` `retired_peer_turn` | Retired: refused, or stopped with "legacy peer help retired". |
| Scheduler | `assistance_tick`, `assistance_notice_tick` | Canonical: the only peer-help ticks. |
| HQ `/v1/team/mcp`, node `/team-gateway/mcp` | `app/hq` `team.rs`, `app/node` gateway | Retired: authenticated v1 calls get HTTP 410. Kept so an old worker gets an explicit answer. |
| HQ `/v2/team/mcp`, node `/team-gateway/v2/mcp` | same | Canonical door (`tools_in_v2`, `call_v2`). |
| Worker and runner endpoints | `app/worker`, `app/runner` | Canonical: default to v2; a v1 override is refused. |
| Owner conversation API and Work UI | `app/hq/src/api/conversations.rs`, `47e-hq-conversations.js` | Canonical owner scope, grants, cancel and delete; history readers. |
| `/v1/tasks/{id}/consultations` | `app/hq`, `engine/history.rs` | Read-only history, pages of 50. |
| HQ startup cancellation | `app/hq` `main.rs` → `engine/history.rs` | Cutover drain: cancels queued legacy rows in batches of 50 before HTTP opens. The only production write to the legacy tables. |
| Legacy rows in quotas and attribution | `sqlite/assistance.rs`, `sqlite/conversations.rs` | Read only: old usage still counts toward the shared allowance. |
| Tool advertising and configuration | briefing in `engine.rs`, `tools_in_v2` | Clean: no retired tool or flag is advertised. |

No violation was found: nothing outside `engine/assistance.rs` and the
conversation store admits, claims, answers, falls back to a record answer,
delivers, recovers or schedules peer help.

## Settled in this change

- **Management escalation.** `ask_owner`'s question to the lead or CEO is the
  escalation ADR 0138 keeps "for authority decisions". Its description invited any
  "question or proposal about your task", which reads as ordinary peer help. It now
  says it is for a decision the task needs, or a proposal, and points a colleague's
  knowledge or help to `consult`. A protocol test fails on the old wording.
- **Dead adapter and refusals.** `Engine::consult_in` had no production caller; its
  one test now calls `consult_peer_in`, which it wrapped. Eight `HistoryRefusal`
  variants of the old consultation engine were never constructed; they and their
  messages are removed. A stale test comment about `answer_consult` is corrected.
- **No test requires retired behavior.** Tests seed legacy rows only as history,
  and HQ API tests assert that v1 is refused and the legacy table stays empty.

## Accepted, with its reason

The scorecard's outcome counts read every legacy `consults` row on each HQ
snapshot. ADR 0138 forbids whole-history reads in routine refresh because archives
grow. This table no longer grows: the startup cancellation's bounded update is its
only production write, and production holds two rows. The read is therefore
bounded by a frozen size. If legacy rows are ever imported, bound it first.

## Verdict

Item 1 holds: task-linked and taskless help both enter through
`request_assistance_in`, task-linked help is limited to the authorized project,
and the only peer-help scheduling is the canonical ticks. Item 2 holds after this
change: retired execution, storage writers and tool advertising are gone, the
surviving `consult` adapter only calls the canonical use case, and no test requires
retired behavior. Production evidence for items 3 and 4 (a real work-triggered
request and its history in Work) is tracked in the
[checkpoint](agent-collaboration-foundation-checkpoint-2026-10-22.md).
