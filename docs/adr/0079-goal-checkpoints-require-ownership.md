# 0079 — Goal checkpoints require current ownership

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Codex).

## Context

The goal foundation gate reproduced stale evidence overwrites, recreation after
purge, and exhausted reservations leaving permanently active work. ADR 0078's
expiry token fenced release only. A long model call could outlive the 30-second
lease; cancellation could not stop the next capability dispatch in that pass.

## Decision

Split `GoalRepository` creation from checkpoint updates. Creation is idempotent
without overwriting evidence; reusing a key with a different objective fails.
Checkpoint updates never insert and require an existing active goal plus its
current unexpired ownership token. Terminal checkpoints are immutable through
this update path. Ownership tokens are random and never reused after purge.

Claim work and spend one attempt in an immediate SQLite transaction, checking
both goal status and lease expiry. After eight attempts, wait for the last owner
to expire or release before recording `Blocked` with a budget-exhausted reason.
The worker obtains the turn lock before claiming, so capacity waits spend no
attempts. Failed context preparation still spends an attempt and is bounded.

Add renewal, ownership checks and token-specific release to the goal port. The
background goal worker renews every ten seconds during its blocking pass. Wrap
its existing reversible policy runner with an ownership guard which checks
durable state immediately before every tool dispatch and preserves the policy
runner's decisions and verification hooks. Cancellation, deletion, replacement,
storage failure or expiry prevents subsequent dispatch and checkpoint writes.
An operation already dispatched keeps its existing transport deadline.

Migrate by adding `owner_token` to `execution_leases`, preserving existing attempt
counters and expiries. Existing leases without a token remain unavailable until
expiry. Legacy lease operations cannot release or revive a token-owned claim.
The research worker retains its existing lifecycle until the planned migration;
this ADR does not claim that research writes are now fenced.

## Consequences and limits

The HTTP protocol and goal checkpoint format remain compatible. Repository-port
implementations and callers must migrate to explicit create/claim/checkpoint
operations. No dependency or separate execution framework is added.

Foreground sports lookup also claims before dispatch and uses fenced updates.
It currently has no periodic renewal inside its synchronous lookup: a result
arriving after expiry is rejected and the goal remains resumable. The shared
request loop planned next must replace this shortcut rather than extending it.

This repairs storage and background dispatch safety. It does not repair source
provenance, formatting-based completion, terminal-result delivery or browser
progress. Those remain release blockers in the foundation gate and handoff plan.

## Verification

`goal_ownership` tests cover competing connections, renewal, expiry, stale release,
deletion and key reuse, cancellation, restart, older-schema migration, budget
exhaustion and policy forwarding. The actual node worker is separately tested
with a 35-second model call; its heartbeat keeps the checkpoint valid beyond the
initial lease. Another worker test verifies that busy foreground chat spends no
reservation. The original foundation persistence counterexamples now pass using
the explicit checkpoint contract.
