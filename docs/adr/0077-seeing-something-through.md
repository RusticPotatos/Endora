# 0077 — Seeing something through

## Status

Accepted and implemented (2026-09-06; proposed 2026-08-30, revised 2026-09-06).
The [implementation handoff](../plans/research-assistant-handoff.md) and
[five-packet checklist](../plans/research-assistant-checklist.md) specify the
accepted first release. The owner accepted this decision on 2026-09-06. This
decision does not amend the constitution.

**Superseded 2026-10-20 by [0136](0136-endora-is-hq.md).** Research the person asked for in the chat is retired, code and storage. The reasoning below is kept; none of its mechanism runs.

**Amended 2026-10-16:** the 2026-10-12 amendment is withdrawn with durable goals
([0100](0100-the-clean-slate.md)). Chat starts nothing on its own; research starts only when
the person commissions it.

Amended 2026-10-12: ordinary chat may now start an objective on its own, from the
deterministic verdict that a turn could not answer. See the amendment in the
Decision section; the clause it supersedes is struck there rather than deleted.

## Context

The current nightly loop forms intentions from beliefs rather than explicit
assignments. Progress overwrites a prose note; `Intention::complete()` has no
production caller. A retired intention can be formed again from the same belief
without its previous findings. The proposed first release addresses commissioned
research: prompt execution, durable evidence and a deliverable the person can use.

## Decision

This section supersedes the conflicting clauses in the August draft preserved
in git history. The linked handoff resolves the data, migration, API, execution and test
contracts. Prepared with AI assistance (Codex/Astra).

- **Explicit commissioning:** Research mode in the existing composer submits the
  person's assignment once. That submission is authorization; no second yes or
  model classifier. ~~Ordinary chat does not silently create work.
  Natural-language commissioning from normal chat and model-generated offers are
  deferred.~~ **Amended 2026-10-12 — see "Unanswered asks pursue themselves".**
- **Unanswered asks pursue themselves** *(amendment, 2026-10-12)*. The two struck
  sentences above are superseded on the maintainer's direction that this be
  autonomous rather than ask. An ask the turn **could not answer** now starts an
  objective from ordinary chat, with nothing asked of the person.

  The deferred-model-offer refusal stands, and so does 0076's: **no classifier
  decides what you commissioned.** None is needed, because the trigger already
  exists and is already deterministic — `specimen_verdict`, the same honesty check
  that gates retries and files specimens
  ([0075](0075-failures-become-specimens.md)), is what says a turn could not
  answer. A turn that could not answer is, by code rather than by any model's
  opinion of itself, an ask that needs more than one turn. A turn that answered
  starts nothing.

  What makes starting unasked affordable is not that the trigger is clever but
  what a goal may do: the worker runs it clamped to the reversible bands, so it
  gathers and **can never actuate**; it is bounded; its evidence is visible; and
  one verb drops it. A wrong objective costs model time, not consequences — and
  that asymmetry is enforced in code, which is the only place this repository
  accepts a guarantee. Weighed against it, requiring a yes charged a cost on every
  ongoing request to avoid a cheap mistake on a few.

  Bounded as the rest of this is: at most three self-started objectives at once,
  and asking the same thing again addresses the same objective rather than opening
  a second — the rule specimens already use, where asking twice is one problem.

  **What this still does not reach,** stated rather than implied: a turn that
  answers starts nothing, so a long-horizon goal like *"help me get fit"* produces
  an answer and no pursuit. The deterministic trigger catches what Endora **could
  not do**, which is the reported complaint, and not what the person is trying to
  achieve. That remains open.
- **Provenance:** one intention aggregate, originating from a belief or a stored
  user commissioning message. At most one active per origin. User research has
  owned checkpoint/evidence/report records, not a competing task store. A second
  assignment gets an occupied-slot response; nothing is queued or replaced.
- **Delivery is distinct from usefulness:** code can mark a report delivered
  after validating its structure and source references and committing it with
  its completion message. This does not verify semantic correctness or achievement
  of the person's goal. Feedback is optional, not a prerequisite to finishing.
- **Honest terminal states:** delivered, blocked, budget_exhausted, cancelled and
  failed are distinct. Incomplete results are labelled partial. No model can set
  those states, invent source references or renew a spent budget.
- **Shared execution:** research composes the existing `run_tool_turn` through
  wrappers that narrow capabilities and reserve persisted budgets immediately
  before each model invocation or authorized external read. A restart resumes
  the durable assignment by safely rerunning its read-only bounded turn; spent
  reservations are never refunded and no external action can be duplicated.
- **Bounded work:** five-second worker poll, 16 model invocations, 16 external
  search/fetch attempts and 24-hour expiry per assignment. Existing turn failure,
  round, generation and transport ceilings remain. Transient model failures enter
  `retry_wait`; exhaustion is terminal and visible.
- **Foreground priority:** research and foreground chat share the existing turn
  lock. The worker uses a non-blocking acquisition, so a waiting foreground turn
  wins when the current bounded research turn releases it. Existing scheduled
  model work waits while research is active, but sensing continues and due
  schedules are not consumed without running.
- **No added authority:** research narrows the reversible runner to an explicitly
  selected approved search tool and guarded web fetch. No house/mail/shell access,
  belief writes, arbitrary files or external actions. Evidence stays untrusted
  across checkpoints and synthesis; `Deeper` and its taint refusal stay intact.
  User-supplied URLs are fetched deterministically through that same policy and
  accounting boundary before synthesis, so evidence collection does not depend on
  a model choosing to call the fetch tool.
  Saving a local report is a deterministic application use case, not a tool that
  grants the model arbitrary filesystem writes.
- **Durability:** SQLite owns the assignment, counters, states and Markdown report.
  Only URLs passed to successful guarded fetches become report references; search
  snippets and model-authored URLs do not. Conditional commits reject cancelled or
  purged work. Report, completed intention, buffered audit and terminal chat notice
  commit atomically. External reads may repeat after a crash; spent budgets survive.
  Export and purge cover all data.
- **UI:** composer mode plus one compact result/progress card, Cancel and download.
  No add-task page, backlog, badge or recurring notifications. Completion is a
  requested result, independent of unsolicited check-in settings.

For this slice, belief-derived intentions keep their nightly behavior; broader
anti-churn/resumption changes, free-text settle predicates, recurring watches and
automatic interventions are deferred. The old draft's claim that every intention
will gain a completion path is therefore narrowed to commissioned research.

On acceptance/implementation, amend affected clauses in 0052/0053/0056, the
architecture and roadmap together. Do not change accepted ADRs in this planning
change. This retires the old proposal's second-yes requirement, user-verdict-only
completion, prose-only research continuity and prohibition on owned child records.
It uses existing Facade, Repository, Observer and narrowing Decorator patterns;
no workflow framework or event bus. A cross-context orchestration transaction
replaces separate save/announce calls for research only.

Acceptance needs production-schema migration tests, shared-loop policy regression
tests, restart/cancel/purge races, safe console/download tests, and inspected live
research outputs. Structure/citation counts are not semantic verification. The
budgets are proposed initial engineering limits, not measured model fitness.

## Consequences

The user gains explicit background research without a new authority boundary or
task dashboard. Persistence and shared-loop refactoring are necessary costs of
restart and cancellation guarantees. Research quality still depends on the
configured model and accessible sources; a mechanically valid report can be
unhelpful. Live acceptance, rather than offline test counts, decides usefulness.

## Review history and alternatives

The August drafts established one slot per provenance and explicit commissioning.
They also exposed an invalid precedent: `Notion::settles_when` is stored text,
not a condition evaluated by code. This revision preserves that correction and
confines deterministic completion to a concrete stored-report contract.

Rejected for this slice: simply raising tool-round limits (no restart recovery),
a second research execution path (duplicated policy), treating a model's done
claim as completion, waiting for user feedback to stop, inferring work from every
chat message, and adding external authority or a general task framework.

The prior proposal and its complete reasoning remain in git history. Its
unimplemented offers, settle conditions and broader belief-intention changes
are deferred rather than additional requirements on these five packets.
