# Research assistant implementation handoff

Accepted and implemented 2026-09-06; AI-assisted (Codex/Astra). Planning baseline:
`6207f9a` on `develop`. No pull request or deployment was created.

Read [ADR 0077](../adr/0077-seeing-something-through.md) and the
[execution checklist](research-assistant-checklist.md). AGENTS.md applies.
The owner accepted ADR 0077 on 2026-09-06 before implementation began.

## Implementation record

The shipped slice takes the shortest complete path through this design. SQLite
stores the explicit commission, parent intention, lifecycle, persisted budgets and
Markdown report. A single node worker notices it within five seconds, shares the
foreground turn lock, and reruns only guarded read operations after restart. Model
and external-read reservations are saved before I/O. Cancellation and final
publication use conditional transactions, so a late result cannot resurrect work
and report/intention/audit/notice cannot split. Supplied URLs are fetched through
the narrowed runner before synthesis; a model cannot skip the evidence step by
answering from prior knowledge.

Exact production files are:

- `domains/understanding/src/domain/{intentions,research}.rs`, `application.rs`
  and `infrastructure.rs`: provenance, lifecycle, limits and canonical migration.
- `crates/endora-application/src/{ports,research,usecases}.rs`: atomic delivery
  port and the narrowed, counted composition of the existing tool turn.
- `crates/endora-infrastructure/src/sqlite.rs`: commission, cancellation,
  publication, export, purge and restart persistence transactions.
- `domains/capabilities/src/infrastructure.rs`: opt-in 12,000-character research
  read window over the existing guarded fetch (ordinary reads remain 2,000).
- `app/node/src/{api,main}.rs` and `web/app.js`: authenticated contracts, worker,
  Research composer, status card, safe report view/download and polling fallback.
- `scripts/check-console.mjs` and
  `crates/endora-infrastructure/tests/research_eval.rs`: offline UI coverage and
  the ignored real-model acceptance harness.

The pre-implementation packet detail below remains useful rationale. Two larger
mechanics were deliberately deferred from this smallest release: operation-level
conversation checkpoints/evidence excerpts, and provider-normalized search result
records. Restart instead reruns policy-cleared reads within the persisted 16/16
budget; only the exact URL of a successful guarded fetch is cited. Escalate to a
follow-up ADR before adding non-read tools, multiple concurrent assignments,
arbitrary output paths, automatic commissioning, or a workflow framework. Escalate
the executor design if live tests show whole-turn latency blocks foreground chat,
or repeated reads consume the budget before useful reports are produced.

## 1. Smallest useful product

The person selects Research in the existing composer, submits an assignment
once, and receives a short sourced Markdown report. Work begins promptly while
the browser is closed, survives restart, and supports cancellation. The UI
discloses this bounded report workflow before submission. Ordinary chat does not
silently become work. No additional confirmation after Research submission.

Acceptance example: "Compare two approaches to backing up a local Rust
application's SQLite database. Recommend one and give an implementation
checklist." All checked-in examples/captures must be synthetic or sanitized.
Personal assignments and live results belong in ignored local storage.

General discovery requires a configured search provider. Built-in `web_search`
was removed; see `nothing_claims_to_search_the_web_any_more` in capabilities
infrastructure. The existing catalogue offers Brave Search. Do not assume it is
connected or resurrect DuckDuckGo Instant Answer as a search engine. Support
source-only research from 1–5 user-supplied URLs when search is unavailable.

Deferred: implicit normal-chat delegation, model-generated commissioning offers,
recurring watches, multiple user assignments, terminal-job resume UI, broader
belief-intention anti-churn changes, PDF/JS browser extraction, arbitrary files,
Word/PDF exports, new integrations/providers, house/mail/calendar access,
self-authored tools, external actions, push notifications and workflow frameworks.

## 2. Exact code ownership

Existing paths/symbols were checked at the baseline; new symbols are proposed.
Reuse current dependencies and inward dependency directions. Pure domain types
must not contain model/vendor/HTTP/JSON dependencies.

| File | Change |
| --- | --- |
| `domains/understanding/src/domain/intentions.rs` | Add `IntentionOrigin`; retain belief constructor and legacy state names. |
| `domains/understanding/src/domain/research.rs` (new) | Pure statuses, budgets, evidence/report data, transition and delivery validation. |
| `domains/understanding/src/domain/mod.rs`, `src/lib.rs` | Export new pure types. |
| `domains/understanding/src/application.rs` | Origin-specific lookup and intention-owned research read/storage DTOs; opaque checkpoint string belongs here, not domain. |
| `domains/understanding/src/infrastructure.rs` | Canonical migration, row decoding, transaction helpers and reads. Replace intention `INSERT OR REPLACE` with explicit insert/update. |
| `crates/endora-application/src/ports.rs` | `ResearchWorkStore` orchestration port, application `TurnSession`, research export snapshot. |
| `crates/endora-application/src/research.rs` (new) | Commission/advance/validate/publish use cases, checkpoint JSON codec and terminal wording. |
| `crates/endora-application/src/usecases.rs` | Extract shared resumable primitive from `run_tool_turn`; preserve all existing public turn behavior; nightly uses belief-only lookup. |
| `crates/endora-application/src/lib.rs` | Modules and re-exports. |
| `crates/endora-infrastructure/src/sqlite.rs` | Implement orchestration transactions on existing `SqliteStore`, real startup migration, export/purge. |
| `domains/capabilities/src/infrastructure.rs` | Bounded `web_fetch` windows, narrowed research runner, provider-specific search normalization. |
| `crates/endora-infrastructure/src/butler.rs` | Research-purpose prompt/envelope parsing and actual invocation accounting through existing path; retain generation/transport ceilings. |
| `app/node/src/research_worker.rs` (new) | Single worker, notification/poll/yield scheduling; no domain policy. |
| `app/node/src/api.rs`, `main.rs` | Authenticated endpoints, worker wiring, foreground priority, HTTP export and composition tests. |
| `app/node/src/web/app.js`, `styles.css` | Composer mode, progress/result card, safe render and authenticated download. |
| `scripts/check-console.mjs` | Realistic research states, new controls/routes, existing UI budgets. |
| `crates/endora-infrastructure/tests/research_eval.rs` (new) | Ignored live acceptance harness using the actual API/runner. |

Checkpoint JSON encoding lives in endora-application, which already depends on
serde_json. Understanding persists an opaque versioned string through its
application DTO. No new dependency is planned, and no task-specific policy goes
into shared/kernel. No new context/crate/service is justified by this slice.

## 3. Data and lifecycle contract

`IntentionOrigin = Belief(BeliefId) | UserRequest(MessageId)`. Preserve the old
belief constructor; add a user constructor. Internally motivating belief becomes
optional and every caller must be updated deliberately. No fake/zero belief ID.
Assignment text is immutable; commissioning message and intention commit together.
At most one active intention per origin. No implicit replacement or backlog.

Research is a one-to-one child of that aggregate, with:

- `status`: queued, running, retry_wait, delivered, blocked, budget_exhausted,
  cancelled or failed; `phase`: gathering/synthesizing.
- `reason_code`, bounded `reason_text`; `revision: u64`; `next_run_ms`,
  `expires_ms`, `created_ms`, `updated_ms`, nullable `operation_started_ms`.
- `model_calls_used`, `read_attempts_used`, `operation_attempts`; immutable
  limits saved at creation; model cannot alter counters or limits.
- `checkpoint_version=1`, `checkpoint_json`; nullable exact search capability
  ID, supplied URLs, unique client `request_key`, reserved `terminal_message_id`.

Evidence fields: scoped source ID (S1, S2...), URL, title, retrieved-at timestamp,
bounded excerpt, originating read operation ID, status (`retrieved` or
`search_snippet`). Only successful page reads qualify as report citations.
Search snippets are leads, not proof a page was read. Publication dates are
optional and only populated from evidence, never inferred from retrieval time.

Report fields: title, answer paragraphs, recommendation/next-step paragraphs,
limitations, source IDs per substantive paragraph, generated Markdown,
created timestamp and `partial` flag. Application parses proposed JSON; pure
validation accepts a report value. Reference URLs are rendered from stored
evidence, never from model-authored citation URLs. No arbitrary output path.

Checkpoint preserves application `TurnMessage` roles, pending tool calls and
completed results, stable operation ID, repeated-call cache and phase. Persist
model proposals BEFORE tool execution and each tool result BEFORE another model
call. Use a versioned application DTO, not Rust debug serialization. Keep model
types out of domain. Validate checkpoint size/version at save and restore.

| Event | Transition |
| --- | --- |
| Explicit valid submission | queued; base intention Active |
| Reserve operation before network I/O | running; counters/revision increment |
| Successful gathering operation | queued with checkpoint and next due time |
| Retryable failure with attempts left | retry_wait; same pending operation |
| Missing/refused required capability or insufficient evidence | blocked with partial result |
| Global cap or expiry | budget_exhausted with partial result |
| Corrupt checkpoint or exhausted model/protocol retries | failed with reason |
| Cancel active work | cancelled; late result cannot commit |
| Valid report saved with completion notice | delivered; base intention Done |

Other research terminal statuses map base intention to Abandoned, with their
specific reason exposed through the research API. Preserve legacy Done rows
without inventing reports. Terminal assignments never auto-restart. A fresh
submission is a new budget authorized by the person, not an automatic renewal.

Delivery requires nonempty answer and next steps, limitations, at least one
successfully retrieved source, and valid source IDs attached to substantive
paragraphs. Aim for 300–500 words under the existing output ceiling. A universal
three-source minimum would wrongly reject narrow primary-source questions.
Code checks structure/linkage, **not semantic support or fulfillment of arbitrary
user constraints**. Model claims and `done` flags do not decide status. The live
acceptance trial checks coverage, citation support and usefulness separately.

Feedback is optional and cannot keep completed work running. The fixed format
is disclosed in Research mode; free-text constraints remain input, not executable
settle predicates. A report with insufficient evidence is partial and terminal
blocked/exhausted, not delivered. Missing evidence does not mean none exists.
Notice wording is "Your research report is ready", never "goal achieved" or
"all claims verified". Deterministic terminal reason appears outside model prose.

## 4. Migration, repositories and crash safety

Canonical intention/research migration belongs in understanding and is invoked
by BOTH its `migrate` and actual `SqliteStore::from_db` startup. Root `SCHEMA`
currently duplicates intentions; root `all_intentions` also decodes export rows.
Retire those duplicates in favor of shared understanding infrastructure helpers
taking a connection/transaction. Never reacquire Db inside a transaction helper.

Use schema inspection (`PRAGMA table_info`) and transactions; propagate errors,
do not ignore every ALTER failure. Existing `motivating_belief TEXT NOT NULL`
requires rebuilding intentions transactionally to make it nullable and add
`origin TEXT` and nullable `request_message_id TEXT`. Copy every legacy row
as origin belief, preserving IDs, states, notes, counters and times. CHECK exactly
one appropriate provenance ID. Unknown/corrupt rows fail visibly; rollback must
leave original data intact. Do not silently sanitize someone's stored history.

Add STRICT tables:

| Table | Schema outline |
| --- | --- |
| `intention_research` | `intention_id TEXT PRIMARY KEY`; research fields from section 3; integer counters/times, opaque JSON TEXT, `request_key TEXT UNIQUE`, terminal message ID TEXT UNIQUE. |
| `intention_evidence` | intention/source IDs, URL/title/excerpt/status/read operation ID TEXT; `retrieved_ms INTEGER`; PK `(intention_id,source_id)`; unique `(intention_id,read_operation_id)`. |
| `intention_reports` | `intention_id TEXT PRIMARY KEY`, validated report JSON TEXT, Markdown TEXT, `created_ms INTEGER`, `partial INTEGER`. |

Research counter columns are `model_calls_used`, `model_calls_limit`,
`read_attempts_used`, `read_attempts_limit`, `operation_attempts`, all INTEGER
NOT NULL; limit defaults 16, used/attempt defaults 0. Revision defaults 0 and
must monotonically increase. `source_urls_json TEXT NOT NULL` stores the bounded
input list; `search_capability_id TEXT` is nullable. Reason defaults empty, phase
gathering, checkpoint version 1. Populate required clocks/IDs explicitly on
commission, not SQLite wall-clock defaults. Evidence fields are nonnull except
optional publication date. A partial failure report may contain only a
deterministically rendered evidence summary; it never passes delivery validation.

Foreign keys to parent, child-first purge and transactional preconditions apply;
verify FK enforcement rather than assuming it. Unique partial index on origin
WHERE state='active' enforces slots. Multiple legacy active belief rows produce
an actionable migration error, not silent abandonment. Repeated fresh/upgraded
startup must produce the same schema. Do not add a global migration framework.

Implement one orchestration port, `ResearchWorkStore`, on existing SqliteStore:

- `commission(request, generated_ids, now)`: atomically insert commissioning
  chat message, intention and research row. Same key/same payload returns existing
  assignment; changed payload/key or occupied slot is a typed conflict.
- `research(id)`, `recent_research(limit)`, `export_research()`: aggregate reads.
- `reserve_operation(id, expected_revision, now)`: active/due/budget checks,
  durable counters and operation identity before I/O; return reservation.
- `checkpoint(id, reserved_revision, result, now)`: conditional update and
  evidence insertion; never upsert a missing parent.
- `finish(id, expected_revision, terminal_result, notice)`: report if present,
  terminal/base state and reserved-ID chat message in ONE transaction.
- `cancel(id, now)`: conditional terminal transition with reserved-ID cancellation
  notice in the same transaction; same cancellation is idempotent, already
  delivered/otherwise terminal is a conflict; retain evidence.
- `recover_interrupted(now)`: startup only, recover running rows to due retry
  or terminal according to spent attempts/expiry; never refund reservations.

Return typed created/replayed/conflict or committed/stale/absent outcomes, not
booleans hiding failures. These are application-level cross-context transactions:
root adapter coordinates shared Db using context-owned SQL helpers. Understanding
must not depend on conversation. Do not implement publication as independent
save, append and mark-notified calls. One aggregate/store, not a second task ledger.

Single node process per database remains assumed. No expiring lease permitting
two live workers. Reads are at-least-once: after a crash an uncommitted request
may repeat, but every committed result and spent reservation survives. Do not
promise network exactly-once; report and terminal message commit atomically.

Cancel/purge does not wait for the model lock. Revision/state/parent checks
reject late results. Buffer research per-operation audit/events until conditional
commit too, so late completion cannot recreate purged assignment text through
another table. Recheck cancellation immediately before dispatch; acknowledge an
already-sent network request can still finish. Guarantee no subsequent operation
or stale publication, not withdrawal of I/O already sent.

Extend MemorySnapshot, root export and HTTP ExportResponse with the intentions/
outcomes currently omitted by HTTP, plus research/checkpoints/evidence/reports.
Purge new child records before intentions/messages in the same transaction.
Test HTTP export, not just internal snapshot. Markdown lives in SQLite; no
second artifact directory with separate cleanup and privacy obligations.

## 5. One executor, preserved policy

Extract proposed `advance_tool_session` from `run_tool_turn`. It accepts an
application TurnSession and returns response/result/yield/typed failure. Each
research quantum performs at most ONE actual model invocation OR external read.
Existing foreground wrapper loops it with current empty retries, discovery,
verification, repeated-call handling and forced answer at foreground round cap.
Characterize those behaviors before extraction. Keep public entry points.

Research uses stricter options: no forced answer on scheduler yield, no
actuation/read-back, durable repeat cache and before-call budget checks. Every
base/deep invocation or empty retry consumes a reservation. If lower-level
fallback hides multiple invocations, expose the yield/accounting through the
same path; never estimate costs only after completion or fork an unchecked loop.

The existing adapter parses a Butler JSON envelope (`reply`, beliefs, etc.); a
prompt requesting naked report JSON would lose the report in that parser. Add
application `TurnPurpose` (default Chat, ResearchGathering, ResearchSynthesis)
to ButlerContext, and `research_report_json: Option<String>` plus
`output_truncated: bool` to ButlerReply. The adapter retains a proposed
`research_report` object from its existing JSON envelope using serde_json.
It is unvalidated model data, not a domain report. Research-purpose prompting
suppresses belief formation/persona instructions that conflict with the assignment;
all other contexts keep their existing prompt and envelope. No second provider
or raw HTTP call. Propagate provider `finish_reason=length` as truncated; never
deliver it. Adapter tests must cover both old envelopes and the new purpose.

Synthesis envelope contract (maximum 12 paragraphs total, 12 citations per
paragraph; title 160 chars, paragraph text 1500 chars, limitations 2000 chars):

```json
{
  "reply": "",
  "research_report": {
    "title": "Comparison",
    "answer": [{"text": "Finding supported by the retrieved page.", "source_ids": ["S1"]}],
    "next_steps": [{"text": "Recommended next step and its basis.", "source_ids": ["S1"]}],
    "limitations": "This review used one supplied source."
  }
}
```

Model can propose content, not `partial`/status/budgets. The application determines
partial from unmet mechanical gates and records that semantic coverage remains
unverified even for a delivered report. A valid research_report with empty reply
is an answer for research purpose; generic empty-reply retries must not discard
it. Invalid/absent/truncated reports use the one synthesis repair attempt. Capture
this distinction in packet 2 adapter tests and packet 3 report tests.

Build on `build_reversible_only_runner`, narrowed to guarded built-in `web_fetch`
and explicitly selected connected search tool with policy Act and band Observe.
Deferred discovery cannot expand the allowlist. Research context contains the
assignment, constraints and evidence, not all personal chat/house state. Never
call `record_formed_beliefs` from research. Saving a local report is application
persistence, not a model-exposed arbitrary file writer.

Restore evidence as untrusted tool material using current source metadata plus
stored provenance. Missing/unknown source remains untrusted. Summarization and
synthesis cannot launder it into User/System material or remove the stranger
boundary. Keep `Deeper` consent, pseudonymization and taint refusal unchanged.
The local model may have to synthesize research because existing policy refuses
deep escalation after reading the web; this is a deliberate first-release limit.

Support the existing curated Brave integration first. Bind exact connected tool
ID at commissioning, validate against curated server identity and supported tool
schema; description text containing "search" is not authority. Sanitize a real
schema/output fixture in packet 3. Provider-specific parsing belongs in
capabilities infrastructure. Refuse incompatible schemas visibly. If no search
is connected, require starting URLs; no model-invented starting URLs or generic
MCP-tool substitution. Do not install/connect services as a side effect.

Fetch only URLs supplied by the person or returned as leads by approved search.
Use existing egress/minimization/secret checks and guarded GET, including redirect
checks. Do not retain URL credentials or secrets in source records; unsafe source
URLs are rejected rather than made into report links. Remote text is evidence,
never new instructions or commissioned work.

Extend `WebFetchCapability` additively: optional `offset_chars`, `max_chars`,
defaults 0/2000; research max 8000 per window. Return offset, nullable next offset,
and truncated flag. Cap extraction at 32000 chars per source and preserve the
512 KiB fetch cap and guarded timeout. Charge each extra window/read attempt.
Disclose retrieval times; windows from a changing page aren't guaranteed an
immutable snapshot. No new browser or PDF parser in this slice.

Gathering prose without tools is a candidate, not success. With retrieved
evidence, move to synthesis with tools disabled and request structured report.
Without evidence, allow one corrective model turn then block insufficient_evidence.
Invalid report structure allows one repair within synthesis reserve; then fail
invalid_report. At gathering cap with evidence, synthesize with explicit limits.
Never match prose "done", accept a degraded reply, or claim verified semantics.

## 6. Scheduler, retries and numeric bounds

| Bound | Initial value |
| --- | --- |
| Active user assignments | 1 |
| Wake/poll | Notify after commission; fallback every 5s, skip missed ticks |
| Successful quantum spacing | 1s, releasing model lock each time |
| Model invocations | 16 including retries/escalation; last 2 reserved for synthesis/repair |
| External read attempts | 16 including retries/page windows |
| Failed operation attempts | 3 total, delay 10s then 30s |
| Expiry | 24h from submission, including offline time |
| Output | Existing 1024-token ceiling per model call |
| Transport | Existing 60s model, 15s fetch, 30s MCP; no unbounded research transport |
| Assignment | 4000 chars; 5 starting URLs, 2048 chars each |
| Evidence | 12 sources, 8000 stored chars per source |
| Context/checkpoint | 64000 UTF-8 bytes each |
| Report | 16000 UTF-8 bytes |

These are conservative testable initial limits, not measured capability or a
currency budget. Maximum requested output is 16384 tokens, not a promise about
provider billing/reasoning tokens. Counters are durable BEFORE I/O and never
reset on restart. Empty/malformed/degraded model responses count. Failed reads
count. MCP retries that re-execute the request count too; bounded connection
setup chatter does not. Attempt reservations cannot be double-spent by workers.

Missing configuration, policy refusal and bad input block immediately. Transport
failures get bounded retries. When an existing runner exposes only a string
error, treat ambiguous errors as bounded unknown_transport, not a growing phrase
classifier. Third failure blocks unavailable evidence or fails model/protocol
work. Global cap/expiry is budget_exhausted. At 14 model calls, only synthesis/
repair remains eligible; without evidence terminate without more model calls.
No seventeenth "final answer" call. Check expiry before dispatch and publication.
Oversize context/checkpoint terminates visibly; no silent tool-pair truncation.

Worker uses `try_lock_owned` and skips if busy; never queue background ahead of
a user. Add RAII foreground-waiter counter to BOTH chat handlers; check before
and after background lock acquisition. Release after each quantum. An in-flight
operation can still delay chat up to its transport timeout. Dropping/aborting
spawn_blocking does not stop its network operation; do not launch overlapping
replacement work or promise instantaneous cancellation/preemption.

While research active, heartbeat defers check-in/brief/nightly MODEL work but
keeps sensing/reconnection. Due schedules remain due, not consumed without
running. After research terminates reconsider them through existing cadence
rules. Nightly retirement/selection calls `active_belief()` explicitly. Research
is independent of check-in/nightly enabled flags. Single terminal notice is a
requested result, not unsolicited outreach; no intermediate chat spam.

## 7. Additive API and UI contract

All routes use existing authentication. Checking credentials cannot create,
cancel or export; permitted status/report reads follow existing read access.
Extend authorization tests. No token in report URL/query: authenticated fetch
to Blob download, revoke object URL. Escape model/source content, disallow raw
HTML and unsafe link schemes; don't put model text into raw innerHTML.

`GET /v1/research/readiness`: `{ready,modes,search_tools,reason_code}`. `sources`
mode requires web_fetch can Act; `search` also requires compatible connected
search can Act. Search entries contain exact ID/name, no credentials. POST uses
the same eligibility predicate; races with changed configuration are visible.

`POST /v1/research`:

```json
{
  "request_key": "client-generated-unique-key",
  "assignment": "Compare these approaches and recommend next steps.",
  "source_urls": ["https://example.com/reference"],
  "search_capability_id": null
}
```

The example is a fixture, not an actual research source. IDs are actual connected
IDs when search mode is used. Return 202 with resource for creation/idempotent
replay; 409 occupied_slot or changed_payload; 400 invalid input; 409 setup_required
if no valid mode. Retain request key across network retry. Persist commissioning
message atomically; don't ALSO send to `/v1/chat`. Revalidate policy per dispatch.

`GET /v1/research`: `{items:[...]}`, latest 20, active first.
`GET /v1/research/{id}`: full resource including evidence/partial report.
Common fields: `id,assignment,origin_message_id,status,phase,reason_code,reason,
created_ms,updated_ms,next_run_ms,expires_ms,budget,can_cancel,report_available,
partial,report_path`. Budget keys are `model_calls_used`, `model_calls_limit`,
`read_attempts_used`, `read_attempts_limit`. String IDs, integer UTC milliseconds,
optional fields null. Ordinary GET
does not expose checkpoint; full memory export does.

`POST /v1/research/{id}/cancel` accepts `{}`: 200 cancelled/idempotent cancelled;
409 with current resource if already terminal; 404 absent. `GET .../{id}/report`
returns saved Markdown (`text/markdown; charset=utf-8`) with safe server-generated
filename; 409 report_not_ready, 404 absent. Never regenerate at download time.

New errors preserve existing `error` text and add `code` and optional current/
active_id. Extend AppError/ApiError or use typed research route outcomes; no
conflict mapped to generic 500. Keep existing `/v1/intentions` belief-only and
its nonnull motivating_belief/legacy states. Old drop route remains belief-only;
new user population uses cancel. Internal export has both origins. This avoids
breaking old clients with null belief IDs. API tests must enforce the distinction.
No checked-in OpenAPI file exists at baseline; these contracts and API tests are
the specification, not a new invented schema-generation toolchain.

Composer uses Chat/Research mode and same textarea, "Start research" label;
optional starting URLs/search choice inside details. One compact card shows
phase/reason, Cancel, View/download. No add-task page, badges or backlog. Existing
SSE/broadcast triggers refresh; visible active card polls every 5s as fallback.
Reload finds durable terminal message/report even if SSE was missed. Do not
emit repeated completion messages or routine progress into chat.

## 8. Validation and escalation

Write behavior tests first. Use fake clocks/barriers, not sleeps, for worker
correctness. Compose real runner stack and SqliteStore startup. Fake Butler
establishes mechanics, not live model research quality. Acceptance scenarios are
allocated in the checklist. Before each handoff run all six AGENTS.md commands
and report actual results. Target affected tests during implementation first.

Final live trial: comparison, primary-source and source-only assignments, plus
forced restart and cancellation against an isolated test node. Inspect each
report against pages for coverage, citation support, useful next steps and
honest limitations. Capture model/provider/timings locally; sanitize git fixtures.
Offline checks cannot replace this. Do not run against personal production state
or deploy without authorization. Record unconfigured live integrations as blocked.

Proceed independently on naming/helpers/SQL formatting/fixtures/compiler fixes.
Escalate a concrete reproduction and proposed choice when:

- ADR is not accepted before production code.
- Design needs new authority, second execution/egress path, new dependency/service
  or breaking API change.
- Real provider schema can't support scoped adapter; no arbitrary tool fallback.
- Conditional transactions cannot preserve restart/cancel/purge safety.
- Shared-loop extraction changes existing policy/tool behavior.
- Deployment has multiple live processes on the database.
- Live model cannot produce useful supported reports within bounds; do not
  quietly raise caps or weaken validation.

After two failed fixes of the same defect, record failing test, observed behavior,
relevant diff and best hypothesis. Escalate that focused packet to Sol/Astra,
not another repository-wide review.
