# Agent collaboration: Phase 3 integration map

2026-10-22. Source-grounded implementation notes for Phase 3 of the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md), on
`codex/agent-collaboration` after the validated information-only slice.
This map preceded the assistance implementation. The
[implementation checkpoint](agent-collaboration-assistance-checkpoint-2026-10-22.md)
records actual coverage and remaining gates. Neither document authorizes a pilot.

## Where the current code differs from the new contract

1. `consult_in` in `domains/team/src/application/engine/consult.rs` requires a
   task thread, stores a legacy `Consult`, and wakes the expert with
   `WakeReason::Consulted`. It scans all consults for limits, claims and unread
   answers. It cannot directly represent a taskless conversation request or
   provide indexed work as retained history grows. Preserve its public thread
   response and record fallback; do not relabel a legacy consult as a new
   assistance request.
2. Legacy `answer_consult_in` posts first, updates the consult second, then
   wakes the requester. A crash between those actions can leave an answer and
   state disagreeing. The new request path needs conditional, transactional
   answer acceptance and one durable notification intent before delivery.
3. In `engine.rs`, queued turn start derives the project from `run.task` before
   affordability, placement and reservation. `sqlite.rs` derives the ledger's
   first project from that task, and later use/settlement rows from the
   reservation. A taskless assistance run therefore needs one immutable,
   server-validated project association used by all of those paths. Carrying a
   project in only the prompt or request row would leave charging unscoped.
4. `WakeReason::Consulted` is classified as local work in `turns.rs`; the
   scheduler claims a legacy consult or stops the turn. New delivery must
   identify its specific request and run independently of names, message text
   and the legacy claim scan. Add new wake/turn classifications only with the
   exhaustive `wakes.rs` and `turns.rs` checks, and preserve the local-only
   restricted tool boundary unless the accepted ADR requires a narrower
   explicit extension.
5. Conversation messages from Phase 2 contain original evidence *locators*,
   not permission grants. A new grant must bind one immutable document/receipt
   revision, conversation and authorized assistance run; the actual recipient
   model tier and current resource state must be checked at each dereference.
   A successful peer answer is testimony, not an execution receipt or task
   acceptance.

## Build and verification order

1. Freeze legacy consult behavior with focused tests. Add failing request
   lifecycle tests for taskless identity, fixed project charge, idempotent
   acceptance, concurrent claim, deadline, pause/disable and a restricted
   expert tool list. Add real SQLite reopen and two-connection crash-point
   tests before introducing a scheduler path.
2. Add one bounded request/result/delivery adapter and application port in
   the team context. Accept request plus delivery intent atomically; index
   pending claims and deadline sweeps. Use conditional state transitions for
   claim, run association, answer/decline/cancel/expire and result notification.
   Keep history for research under ADR 0137, and keep each tick/page bounded.
3. Integrate the request with the existing run queue. A leased claim is not an
   execution timeout; a stored live run still owns it after the lease expires.
   Reconcile claims and notifications on restart, fold or deliver an answer
   wake once, and never wake for a retry, read or acknowledgement.
4. Thread the validated project through queue admission, `affordable`, ledger
   reserve/use/settle, budget pause and late-charge handling. Assert that no
   taskless paid call can produce a null project ledger row or use the owner's
   allowance. Run the existing pool/member/free-turn/placement regression
   suites alongside the new project-cap tests.
5. Add the minimum original-evidence read path for the diagnostic scenario.
   Verify document/receipt identity, revision, scope, expiry and actual model
   tier at the handler as well as tool discovery. Keep mutators denied. Present
   fallback search as attributed historical testimony.
6. Add owner Work inspection of request state, actual answer source, attempt
   count, charged project and notification outcome. Only then run the full
   repository gate and the Phase 3 crash, cost and authority acceptance matrix.

No service, pilot, model or production database should be exercised by these
local implementation tests. Deployment remains at the later gate in the
reviewed plan.
