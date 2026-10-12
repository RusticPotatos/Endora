# Research assistant live acceptance record

Date: 2026-09-06. AI-assisted (Codex). The run used an isolated temporary
SQLite database and local models. No personal data or production database was
used.

## Goal exercised

> Evaluate whether enabling SQLite WAL is warranted for Endora now that a
> background research worker and foreground HTTP API share one database.
> Identify relevant benefits and constraints, give a recommendation, and name
> the next concrete validation step. Base the answer only on the supplied
> official SQLite source.

Source: `https://sqlite.org/wal.html`.

## Result

- The authenticated HTTP API accepted the assignment and returned `queued`.
- The background worker advanced it to `running` within its polling interval.
- Read and model budgets were persisted before the corresponding operations.
- The first local model produced empty/degraded answers. Endora rejected those
  answers instead of publishing an unsupported report.
- The node was stopped during the active assignment and restarted with a second
  local model against the same database. The same assignment resumed with its
  prior budget usage intact.
- The restarted worker delivered a 2,790-byte Markdown report. It cited only the
  successfully fetched SQLite URL and remained downloadable from the report API.
- Final accounting was seven model attempts and three guarded reads. Re-reading
  after restart is the accepted first-release behavior; reservations were not
  refunded.
- A second API assignment was commissioned and immediately cancelled. It became
  terminal `cancelled` with `can_cancel: false`.

The separate ignored production-composition acceptance test also passed against
the local `qwen3:14b` model after supplied-source collection was made
deterministic.

## Defect found and resolved

The original implementation relied on the model to choose `web_fetch`. A model
could answer from prior knowledge, after which Endora correctly blocked delivery
for insufficient evidence. Supplied URLs are now fetched through the narrowed,
policy-cleared runner before synthesis. Each attempt consumes the persisted read
budget before I/O, and only successful fetches qualify as citations. A focused
test covers multiple supplied URLs and excludes failed reads from evidence.

## Quality assessment

The delivered report was structurally valid, readable, and broadly consistent
with the supplied SQLite documentation. It identified WAL concurrency benefits,
same-host and checkpoint constraints, recommended a development trial, and
proposed monitoring read/write behavior and WAL growth.

Its usefulness is moderate rather than decisive. The recommendation was based on
one documentation page and did not measure Endora's actual contention, latency,
checkpoint behavior, or shutdown/recovery behavior. It also loosely described
the worker and API as separate processes. A decision-grade follow-up should pair
the official source with a repository-specific concurrency benchmark and explicit
acceptance thresholds.

## Current capability boundary

The assistant can now complete durable research from one to five supplied URLs,
including restart and cancellation. Open-ended discovery still requires the
approved Brave Search integration to be connected; this test environment exposed
source-only mode. Local model latency and malformed/empty replies can consume
several attempts, though the 16-call limit and honest terminal states bound that
failure mode.
