# 0080 — Completion uses actual tool observations

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Codex).

**Superseded 2026-10-16 by [0100](0100-the-clean-slate.md).** Durable goals and the schedule
verifier are removed. Completion from tool observations is a rule the agenda loop is to keep.

## Context

The foundation counterexample completed a sports goal from invented numbered
prose without invoking a tool. The sports adapter already produced structured
data, but its registry reduced it to a readable summary. The completion check
then treated formatting as evidence. Model-written findings and actual retrieval
receipts were indistinguishable in persisted goals.

## Decision

Add an additive `CapabilityRunner::run_observed` port. It executes once and
returns readable text, optional raw adapter data, and effective arguments. Legacy
integrations default to readable output without structured evidence. The built-in
registry preserves its actual invocation result. Composite and policy wrappers
forward observations while retaining their authorization, revocation and recovery
rules. Native target recovery conservatively retains unstructured receipts.

The goal execution adapter records actual successful and failed calls, arguments,
results and retrieval times separately from model-written findings. Version 3
goal checkpoints preserve these observations. Versions 1 and 2 remain readable
with no observations; their prose is not promoted to source evidence. The API
exposes observations additively. Receipts are checkpointed at the end of each
bounded pass; operation-by-operation persistence belongs to the shared-loop phase.

Replace sports formatting checks with deterministic validation of successful,
recent `sports_schedule` observations: selected team, team ID, public source
shape, exact requested count, event IDs, opponents, calendar-valid UTC start times,
ordering, future eligibility, duplicate events, and the adapter's actual cutoff.
A future query cutoff cannot prove the next games from now. The provider filters
games that already started and sorts before selecting. Missing opponents remain
missing rather than becoming a placeholder presented as a verified opponent.

Render the verified answer from those event fields. Generated summaries or
conclusions cannot change the returned opponents, dates or source. The same
verification applies to foreground lookup and background goal passes. A recent
persisted observation can establish completion after restart without another read.
Repeated prose cannot override an independent positive verification verdict.

## Consequences and limits

No dependency, model-specific rule or second execution framework is introduced.
The structured JSON remains infrastructure/application data; domains acquire no
HTTP, database or model dependency. Existing readable tool output is preserved.

This is a supported source adapter, not general semantic verification. Web search
and fetched pages still need an evidence extraction and verification path before
they can complete a sports goal. General research continues to end partial until
its completion contract is implemented. Terminal delivery and browser progress
remain subsequent phases. This does not claim the real sports source's 403 is fixed.

Pre-existing terminal records remain readable with their historical status; they
are not retroactively audited or relabeled. A rollback writer must understand
version 3 checkpoints or use a compatible pre-migration backup.

## Verification

The original invented-answer counterexample now passes. Negative tests reject
wrong teams and counts, missing fields, duplicate IDs, past/unordered events,
invalid calendar dates, future cutoffs and stale/failed/unstructured receipts.
The actual worker and production wrapper stack complete a synthetic schedule
from structured tool data while ignoring deliberately incorrect model prose and
tool summaries. The persisted receipt survives checkpoint round-trip. The adapter
is tested against an unsorted fixture containing a game already started today.
