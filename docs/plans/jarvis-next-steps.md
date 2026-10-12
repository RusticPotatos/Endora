# Endora: from conversation to dependable delegation

Superseded on 2026-09-07 by [the foundation reset plan](assistant-foundation-reset.md).
The earlier Packet 1 completion claim below is withdrawn: shared leases did not
provide adaptive execution, fenced checkpoint writes, or step-level scheduling.
Retained as historical requirements; use the replacement plan's gates.

Status: proposed implementation sequence, 2026-09-06. Prepared with AI assistance
(Codex). Requested after a review of the current working tree, including the
uncommitted commissioned-research slice. This plan does not amend the constitution,
adopt new architectural decisions, or authorize deployment or a pull request.

## Product outcome

Endora takes responsibility for an assignment, remembers its progress, checks the
result against the request, and returns with a useful deliverable or a specific
decision it needs from the person. Understanding the person improves that service;
an explicit request is sufficient reason to undertake permitted work.

**First milestone:** give Endora a research assignment, close the browser, restart
the node, and receive a supported, useful saved report without repeating the request.
Chat remains usable while research runs. Cancellation and revoked permissions hold.

## The missing primitive: a durable goal loop

The current chat path has a bounded tool loop, but it lets the model decide that
it is finished when it emits no more calls. That is not enough for an assignment:
"next three Panthers games" has a clear completion condition that must be checked
against live evidence. Endora needs one reusable goal runner underneath research,
schedule lookup, and later autonomous work.

Represent each assignment with an immutable objective and a mutable execution
record. The objective contains the user's words, normalized constraints, time
window, requested count, ambiguity policy, and required source quality. The
execution record contains a versioned phase, plan steps, pending step, committed
evidence, attempts, last error, and terminal verdict. Keep the objective separate
from model prose and fetched text; neither may silently become user authority.

The loop is:

1. Parse the request into a goal and deterministic success criteria.
2. Let the model propose a bounded plan when the goal is not covered by a direct
   application route.
3. Validate the plan against capability schemas and policy before dispatch.
4. Execute one step, commit its result and provenance, then release the model lock.
5. Verify the accumulated evidence against the success criteria.
6. Continue, retry, ask one materially necessary question, or finish with
   `complete`, `partial`, `blocked`, `failed`, or `cancelled`.

The verifier owns completion. For a schedule ask it would require a resolved team,
the current season, dates on or after the clock's current date, exactly the requested
number when available, chronological order, and at least one source link. A prose
answer cannot mark the goal complete. This same contract lets a restart, duplicate
submission, or late result resume safely and prevents a scheduled brief from being
mistaken for an answer to the foreground question.

Build this as an application orchestration service over existing capability and
repository ports. Do not add a second agent runtime or make the pure domain depend
on a model provider. The first implementation can support one active goal and a
small fixed plan; its value is the explicit objective, checkpoint, verifier, and
terminal state, not a general workflow editor.

Use the existing Rust modular monolith, SQLite, MCP integrations, console and
policy-gated execution path. No framework replacement, microservices, second agent
runtime, or new client is required for this milestone.

## Baseline and order of work

The reviewed tree already has action outcomes, belief-origin intentions, tool
discovery, browser speech, and commissioned research. Do not rebuild these from
the roadmap's older descriptions. Research currently persists assignment state,
budgets and final output, but reruns a read-only turn after restart. Evidence and
tool history are rebuilt; the shared chat lock covers a multi-round run.

The review ran all six repository checks successfully: 914 tests passed and 14
live/model tests were ignored. It did not run a new live model evaluation.
The existing research checklist records a separate qwen3:14b smoke success as well
as older statements that live checks were skipped. Reconcile those records against
actual evidence; a single source-page smoke test is not research-quality acceptance.

Implement sequentially. Preserve the existing dirty working tree and branch history.
Any new implementation branch must start from develop and carry required research
changes deliberately; do not reset, stash away, or duplicate the current work blindly.

| Packet | Deliverable | Depends on |
| --- | --- | --- |
| 0 | Reconciled contract and frozen acceptance scenarios | Current review |
| 1 | Durable goal execution that yields and resumes | 0 |
| 2 | Supported reports with honest completion states | 1 |
| 3 | Isolated live acceptance of the first milestone | 2 |
| 4 | Research delegation through ordinary chat | 3; applicable ADR update |
| 5 | Reuse of relevant previous work | 4 |
| 6 | One event-driven, preauthorized action workflow | 3; separate authority design |

Implementation note (2026-09-06): the bounded `Goal` state machine, SQLite
`GoalRepository`, foreground checkpointing, and one restart-resumable worker are
implemented and deployed. Research retains its domain-specific report record,
but now uses the same durable execution lease and idempotent completion boundary.
Correction (2026-09-07): Packet 1 is incomplete. See the foundation reset audit
before building further on this implementation.

## Packet 0 — Establish what passing means

- Reconcile ADR 0077, its handoff, checklist and actual implementation. Label
  implemented mechanics, missing contract items and live evidence separately.
  Preserve valid code; do not label an unchecked packet complete because CI passes.
- Keep the first scope to source-backed research and a saved Markdown report.
  Use the existing Research composer until packet 4. One active research job is
  sufficient for this milestone.
- Freeze 20 synthetic acceptance scenarios before changing execution: 6 research
  quality cases, 6 interruption/recovery cases, 4 authority/provenance cases, and
  4 interaction cases. Record assignment, expected artifact or terminal state,
  allowed tools, failure injection and scoring rubric for each.
- Include comparison against explicit constraints, conflicting sources, insufficient
  evidence, interrupted fetch, interrupted publication, cancellation, purge,
  permission revocation, source prompt injection, chat during research, browser
  reconnect and duplicate submission. Use fixtures without personal data.
- Characterize existing chat execution: retries, repeated calls, discovery,
  escalation, untrusted results, policy refusal and action disclosure.

Exit: an implementation checklist maps each missing behavior to a test and the
existing code that owns it. Architectural departures become explicit ADR proposals.

## Packet 1 — Finish durable goal execution

Build on ADR 0077 and the existing orchestration facade. Extract a resumable step
from the shared tool loop while preserving ordinary chat behavior.

Start with the goal record and runner above. Research is the first concrete goal
kind, and sports schedule lookup is the smallest foreground proving case: it can
be completed by a live read plus a deterministic verifier without introducing a
new task UI.

- Persist pending calls, committed tool results, source evidence, repeat-call
  history, phase and versioned checkpoint. Keep orchestration state out of the
  pure domain's model-provider concepts.
- Reserve model/read attempts before I/O through conditional database updates.
  A restart cannot refund spent attempts or overwrite cancellation/purge.
- Recover the next step from committed state. A crash after dispatch but before
  saving a read result may require a bounded repeated read; do not claim exactly-once
  external execution. Completion publication must remain atomic and idempotent.
- Release the model lock between bounded operations, prioritize waiting foreground
  turns, and prevent overlapping replacement workers. An in-flight blocking request
  remains limited by its transport timeout; do not promise instant preemption.
- Implement the accepted backoff, expiry, synthesis reserve and size budgets.
  Account for lower-level retries. Unknown checkpoint versions fail visibly.
- Store and restore external evidence as untrusted tool material. Never promote
  fetched text into user authority by embedding it in a commissioning message.
- Exercise fresh and legacy SQLite startup, repeated reopen, rollback, export and
  purge using the production composition.

Exit: scripted interruption tests show committed evidence survives, budgets remain
monotonic, late results cannot resurrect work, publication happens once, and chat
gets the next available execution slot. Existing policy characterization tests pass.

## Packet 2 — Deliver a result that answers the assignment

- Separate gathering from synthesis. Use the structured report envelope and
  application-owned source IDs specified in the research handoff. Validate shape,
  citation references, truncation and mandatory sections before publication.
- Persist supporting evidence with retrieval metadata. Search snippets are leads;
  only successfully retrieved sources support report references. Validate the exact
  supported search integration schema; retain source-only mode when unavailable.
- Make answer coverage, recommendation, next steps and limitations reviewable.
  Structural validation proves format and reference integrity, not factual truth.
  Test semantic support through inspected fixtures and live report review.
- Deliver partial work with explicit gaps when useful evidence exists but coverage
  or budgets are insufficient. Otherwise terminate with a precise blocked/failed
  reason. Never convert nonempty prose plus an appended source list into full success.
- Measure the configured model on these assignments. Keep existing authority and
  egress rules. If the model fails, compare supported model/configuration options
  using identical scenarios, latency and repeated trials before adding heuristics.
- If measured failures require different output/call budgets, propose task-specific
  limits with evidence. Do not silently raise the accepted caps or route around the
  current restriction on deeper-model use after external text.

Exit: malformed or truncated reports, fabricated references and unsupported delivery
states are rejected; supported partial and complete artifacts remain readable after
restart and download without regeneration.

## Packet 3 — Prove the first milestone live

Use an isolated test node and test database. Never exercise restart, purge or fault
injection against the person's production data.

- Run the frozen scenarios with the configured model/provider and realistic guarded
  reads. Repeat quality-sensitive scenarios at least three times; retain per-case
  outcomes and variation instead of reporting the best run.
- Inspect reports against retrieved pages for requested coverage, citation support,
  useful recommendations and honest limitations. A pinned independent judge may
  assist later, but cannot replace factual checks or authorize actions.
- Record completion status, unnecessary user questions, model/read attempts,
  end-to-end latency and foreground waiting time. Use sanitized evidence in git.
- Require every mechanical authority, restart, cancellation and publication invariant
  to pass. Proposed quality gate: at least 90% of feasible research trials are judged
  useful, with zero fabricated sources or false completion claims in the evaluated
  trials. Report failures and sample size; this is a release target, not a guarantee.
- Report unconfigured integrations as untested. Do not equate a passing source-only
  test with a validated search workflow.

Exit: the first milestone can be demonstrated, and acceptance evidence identifies
remaining limitations. If it fails, fix the failing behavior before adding features.

## Packet 4 — Delegate through ordinary chat

Propose an update to ADR 0077's explicit deferral of chat commissioning. Retain the
Research control as an optional explicit shortcut and preserve old API behavior.

- Support clear requests such as "Compare these options and get back to me" through
  the same durable commissioning use case. Persist the originating user message and
  an immutable assignment. Do not require a motivating belief or a second yes for
  already-authorized read-only research.
- The model may propose commissioning; deterministic code binds the work to the
  user's request and existing allowed research capabilities. Tool output, assistant
  prose and inferred interests cannot supply user authority.
- Ask only when a missing decision materially prevents execution. Report unavailable
  setup once rather than silently downgrading the request to ordinary chat.
- Support status and cancellation conversationally with unambiguous assignment
  identity. Do not reinterpret a follow-up as a replacement without evidence of that
  intent. Keep one active slot initially; explain occupancy without losing the draft.

Exit: clear delegation creates one job, ordinary questions create none, retries do
not duplicate jobs, and source instructions cannot commission work.

## Packet 5 — Make previous work useful

- Retrieve relevant completed reports, decisions, evidence and unresolved questions
  into new assignments within a measured context budget. Reuse the existing stores
  before introducing a new memory service or dependency.
- Keep explicit preferences, inferred beliefs and task evidence distinguishable.
  Preserve provenance, correction, export and deletion rights.
- Allow a follow-up to build on a saved result without repeating completed research.
  Refresh time-sensitive evidence and show when a conclusion changed.
- Add cross-session tests: retained constraints, corrected preferences, stale sources,
  contradictory evidence and deleted material no longer entering context.

Exit: a follow-up needs less repetition and fewer redundant reads while retaining
equal or better answer quality on the same scenarios.

## Packet 6 — One useful autonomous action

Start with the existing Home Assistant integration and one named lighting workflow.
Propose a separate ADR before widening unattended authority. Keep constitutional
changes, if any become necessary, as human decisions rather than implicit plan edits.

- Let the person grant a revocable standing instruction for specific targets,
  conditions and permitted actions. Compile it into validated policy constraints;
  model confidence cannot grant authority. Reject unsupported conditions explicitly.
- React to the relevant state event, deduplicate it, check freshness and authority,
  execute through the existing runner, and read the target state back.
- Judge the requested postcondition: already satisfied, achieved, failed or unknown.
  A changed response string is insufficient proof; an unchanged correct state is
  not a failure. Preserve older outcome/API representations additively.
- Record triggering evidence and result. Notify on useful completion or exceptions
  under the person's chosen notification policy. Provide revocation and applicable
  undo through the existing interface.
- Test repeated events, stale state, wrong target, already-satisfied state, failed
  actuation and revocation between planning and dispatch before isolated live trials.

Exit: one standing instruction works repeatedly without per-step supervision and
without acting outside its granted scope.

## Stop, defer and reconsider

Freeze expansion of unsolicited reflection, model auto-adoption, recipe editors,
configuration screens, native clients and additional integrations until packet 3.
Retain working features unless removing them demonstrably reduces maintenance or
user effort; migrate or preserve stored data when retiring anything.

- Disable repetitive nightly notes by preference while measuring whether they help.
  After packet 5, retire the prose-only nightly research loop if durable assignments
  cover its useful behavior. Preserve valuable belief maintenance independently.
- Retire obsolete documentation claims as each replacement ships. Do not impose
  cosmetic code decomposition or a repository-wide refactor on this work.
- Keep memory rights, auditability, guarded egress and deterministic authorization.
- Defer multiple queued assignments until one job is dependable. Then allow a small
  bounded queue without requiring a task-management dashboard or parallel models.
- Defer general shell/browser execution, self-authored executable tools, richer voice
  and new messaging channels until a demonstrated assignment needs them.

## Implementation and handoff discipline

Write failing behavioral tests before implementation. Use targeted tests during a
packet, then run all six AGENTS.md checks before handoff. Record actual red-to-green
evidence, live results, deviations and the next incomplete item. Do not count mock
success as model quality. New dependencies need justification; architecture changes
need ADRs. No pull request or production deployment is part of this plan.

**Next action:** implement packet 0, then close the missing durable-execution items
in the existing research checklist. Do not begin by building a second task system.
