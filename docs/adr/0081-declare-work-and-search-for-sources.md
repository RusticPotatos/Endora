# 0081 — Declare work and search for sources

## Status

Accepted, 2026-10-12. AI-assisted implementation.

**Superseded 2026-10-16 by [0100](0100-the-clean-slate.md).** The chat's intake tool and
durable goals are removed. Built-in search for research stays.

## Context

Keyword routing special-cased three sports games, while failed answers could
silently become speculative goals. The worker needs the original request and
explicit criteria, and an alternate source when its first provider fails.

## Decision

Offer `endora_begin_goal` through the existing model tool interface in chat.
Bind validated schedule or research criteria to the unchanged request in a
version-4 checkpoint. Criteria cannot change on retry or owned checkpoint.
Optional chat `request_id` keys deduplicate durable work. The browser supplies
fresh keys and reuses its last key for an interrupted-turn retry within that
page session. SQLite enforces the three-active-goal limit transactionally.

Retire the sports keyword pre-route and failed-answer goal creation. Ordinary
standing house/brief questions keep their existing path. Goal execution retains
the existing read-only worker, ownership heartbeat and tool loop; prior actual
receipts and errors inform the next approach.

Add a built-in `web_search` capability backed by the
[Brave Search API](https://api-dashboard.search.brave.com/documentation/services/web-search).
Its required API key uses the existing secret Skills settings. The fixed provider
endpoint receives the key in a header, never a query; redirects are disabled and
responses, queries and result counts are bounded. Existing approved MCP search
remains available. Do not restore the retired Instant Answer pseudo-search.

Search results are leads, not completion evidence. Guarded `web_fetch` retains
bounded JSON-LD SportsEvent objects. A schedule verifier checks the actual fetched
URL, team, UTC dates, requested count and duplicates, excludes past events, sorts
future events and renders canonical results. Unsupported pages remain unfinished.
Completed goals publish their final evidence to chat using the existing atomic
publication transaction.

## Validation and limits

HTTP intake tests cover persisted criteria, duplicate request keys and conflicts.
Storage tests reject changed criteria and over-capacity creation. A mocked HTTP
server verifies search headers and a 429 failure. An actual-worker composition
test covers first-source 403 → search → fetched structured calendar → verified
two-game completion → one delivered chat result. Its providers and planner are
synthetic; it does not establish live-model reliability or live-provider success.

General research still cannot reach independently verified completion. Its
dedicated assignment worker has not migrated. Partial/blocked delivery, durable
browser progress, retry keys across page reloads, timezone offsets, ambiguous
teams, season completeness and the remaining acceptance scenarios are subsequent
work. No new dependency, framework, deployment or provider account is introduced.

## Amended by 0094 (2026-10-15)

[ADR 0094](0094-work-it-finds-for-itself.md) lets a goal start without being declared,
but only from a record that code derived (first: a device in standing trouble) and never
from a failed answer. The retirement of failed-answer goal creation above stands: that
objection was about speculation, and the new trigger is a fact, not a guess.
