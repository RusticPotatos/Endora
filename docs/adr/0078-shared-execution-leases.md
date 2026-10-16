# 0078 — One lease boundary for resumable execution

## Status

Accepted and implemented (2026-09-06; prepared with AI assistance (Codex)).

**Amended 2026-10-16:** durable goals are removed ([0100](0100-the-clean-slate.md)), so
research is the only worker on the lease table; their leases are deleted on start.

## Context

Goals and commissioned research have different domain records. Goals store a
bounded plan and evidence; research stores source evidence, budgets and a
Markdown report. Their workers nevertheless need the same operational guarantee:
one attempt owner, a bounded retry count, recovery after a crashed process, and
protection against a late worker clearing a newer reservation.

Keeping those mechanics in separate tables makes restart behavior diverge and
invites one worker to bypass the contract. Copying the research record into a
goal row would create two sources of truth for evidence and lifecycle.

## Decision

Add a shared `execution_leases` persistence port and SQLite table. Execution keys
are namespaced (`goal:<request-key>` and `research:<request-key>`), while each
domain keeps its own authoritative checkpoint and terminal state. Reservations
increment a bounded attempt counter and carry a fixed 30-second lease. A worker
may release only the exact lease expiry it received; a late release therefore
cannot unlock a newer worker. An expired lease is reclaimable after restart.

Goal and research workers both reserve before entering their bounded turn and
release after checkpoint/publication. Domain-specific cancellation, evidence and
completion publication remain unchanged.

## Consequences

The two workers now share crash recovery and concurrency behavior without a new
workflow engine or duplicate domain aggregate. A process crash can still cause
an at-least-once read; the lease bounds the duplicate window but cannot make an
external service exactly once. Existing goal lease columns remain for compatible
database migration and are copied into the shared table on open.
