# Next major: durable goal execution

Status: foundation gate reopened, 2026-09-07. Prepared with AI assistance (Codex).
Superseded by [the foundation reset plan](assistant-foundation-reset.md).
The earlier implementation-complete claim is withdrawn. The ten read-only smoke
checks passed previously but did not establish adaptive execution or safe recovery.

The goal state machine and bounded runner now exist in
`crates/endora-application/src/goal.rs`, and the sports schedule lookup uses
them. This is a foundation, not the finished Jarvis behavior. The first
persistence slice is now present: `GoalRepository` stores versioned checkpoints
in SQLite, exports and purges them, refuses stale writes after cancellation, and
reserves bounded attempts before external I/O. The sports route writes those
checkpoints, and a single worker resumes stale goals. Research keeps its report
and evidence aggregate, while both workers share the same durable lease/reclaim
contract and each publishes completion atomically. Read/status/cancel routes
expose both lifecycles.

## Outcome

When a person asks for work, Endora records what “done” means, performs bounded
steps through policy-approved capabilities, saves evidence after each step, and
returns later with a complete result or a precise blocked/partial/failed state.
The model can propose an interpretation or plan; deterministic code validates
the plan, authorizes each step, and decides completion.

## Scope for this major

Keep one active goal, one worker, and a small fixed plan vocabulary. Do not build
a workflow editor, parallel agent runtime, queue dashboard, or new client. Start
with two goal kinds:

1. **Schedule lookup** — resolve a team, fetch the current season, filter from
   today, verify the requested count and source, then answer.
2. **Research assignment** — reuse the existing commissioned research path,
   replacing restart-time replay with the same checkpoint and verifier contract.

## Implementation packets

### 1. Persist the goal contract

- Add a `goal` repository port in the application layer and a SQLite adapter.
- Store immutable user objective and normalized constraints separately from
  mutable status, phase, plan, evidence, attempts, and terminal reason.
- Add a unique request key so retries cannot create duplicate goals.
- Version checkpoints. Unknown versions fail visibly; they never restart from an
  empty plan.
- Preserve purge, export, cancellation, and late-result protections.

### 2. Separate planning from execution

- Define a planner port that returns a bounded list of typed steps.
- Validate every step against the capability catalog, input schema, policy
  decision, external-data rules, and an explicit step budget.
- Keep deterministic routes for asks with code-complete answers, such as the
  current sports lookup.
- Ask the person only when an unresolved ambiguity blocks safe execution, such as
  two teams sharing the same nickname.

### 3. Add the worker and resume protocol

- Reserve an attempt before I/O with a conditional update.
- Commit the result and checkpoint before selecting the next step.
- On restart, recover only queued or running non-terminal goals and resume from
  the committed step.
- Treat a crash after dispatch as an at-least-once external read; never claim
  exactly-once execution.
- Release the shared model lock between steps so foreground chat remains usable.
- Make completion publication idempotent and prevent overlapping replacement
  workers.

### 4. Make verification explicit

- Give each goal kind a verifier that receives only committed evidence and the
  immutable objective.
- Schedule verification must check team identity, season, date boundary,
  chronological order, requested count, and a source URL.
- Research verification must check source retrieval, citation references,
  coverage, and honest partial/blocked states.
- A nonempty model answer never marks a goal complete by itself.

### 5. Expose the lifecycle

- Add read-only status, evidence, and cancellation routes using the existing
  authentication and API style.
- Show phase, next step, last update, and terminal reason in the console.
- Add conversational status/cancel handling only after route semantics are
  unambiguous; do not let a follow-up silently replace an active goal.

## Acceptance gate

- A schedule request runs without model routing and returns exactly the next
  three verified games when three exist.
- An incomplete result is partial, not complete.
- A restart resumes from the last committed step without repeating completed
  reads unnecessarily.
- Cancellation and purge win against late worker results.
- Duplicate submission returns the existing goal.
- Revoking a capability prevents the next step from dispatching.
- A source page cannot authorize an actuator or alter the user objective.
- Foreground chat remains available between goal steps.
- The 10 existing live smoke checks continue to pass.

## Deliberate removals and freezes

Until this gate passes, freeze new integrations, recipe-authoring expansion,
unsolicited nightly prose, automatic model adoption, and general browser/shell
execution. Retire a feature only when its useful behavior is covered by a goal
and its stored data/API can be migrated or preserved.

## Operational note

The live smoke test requires a read-only checking credential matching the deployed
node. Keep that credential in ignored local configuration or retrieve it through
the signed-in operator flow; never commit it or print it in build output.
