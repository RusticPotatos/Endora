# Agent collaboration: information-only implementation checkpoint

2026-10-22. AI-assisted work on `codex/agent-collaboration`, branched from
`develop`. This records the first behavioral slice of the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md), under
[ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md).

## Implemented scope

- Default-off information tools for existing authenticated turns. The owner
  creates a fixed active-project conversation with two active non-CEO members.
  Membership conveys no task, tool, acceptance or deployment authority.
  A turn is eligible only when its briefing began under the current enabled
  switch revision. Changing the switch during a turn, even off and back on,
  does not admit an older turn. Legacy turns keep their prior transcript
  classification while the feature is off.
- Immutable messages with stable member/run IDs, bounded text/references, atomic
  retry deduplication and twelve-per-turn/sixty-per-day admission limits. Sends
  and reads create no additional task, wake or model start.
- History remains until explicit owner deletion. Directory, message and operation
  pages return fifty records with one lookahead, using stable ID cursors. Summaries
  are updated at acceptance rather than recounted from history. No new archive
  query runs in snapshots, briefings or the scheduler; the tick only purges a
  bounded batch for a deleted conversation.
- Signed owner HQ routes and authenticated fixed node proxies. HQ Work provides
  enable/disable, authorization, history, page save, operation outcomes and delete.
  Inspection/export/delete remains available with messaging disabled.
- Confidential current tool results or inherited history prevent sending. HQ
  records inherited taint before returning historical excerpts, persists it across
  restart and uses it for subsequent transcript and record-search classification.
- New sends fail closed when legacy briefing text or tool output lacks provable
  input lineage. Typed notebook, section, story and document reads inherit their
  source run's tier. Task threads have no stable post/run relationship, so a task
  turn is currently ineligible to send Internal information; adding that lineage
  is a prerequisite for broadening the pilot. Owner-authored notes and static
  documentation do not inherit a model-run tier.
- Owner deletion denies further reads immediately, then removes at most one
  thousand message/operation rows per tick. Conversation/message IDs are not reused.
  Backups and SQLite free pages remain subject to existing privacy policy.

## Verification record

Targeted checks observed before the final full gate:

- Taskless send/read, authenticated authorship after rename, retries/conflicts,
  outsider access, feature disable, ended turns, forged sender fields, input
  bounds, current member/project eligibility and owner deletion.
- Confidential historical read followed by attempted forwarding: failed before
  the taint fix and passed afterward. The resulting transcript retains Confidential
  classification. Protocol-refusal attribution likewise failed before its fix.
- Real on-disk SQLite reopen, concurrent unseen-key acceptance from independent
  connections, immutable records, quota across runs and free retries.
- Real message-page SQL with 1,000 versus 100,000 older messages, split across
  the same and other conversations: 625 SQLite VM steps at both sizes. Dropping
  the paging index increased work above 500,000 steps, detected by the checker.
  A deletion pass remained capped at 1,000 rows. These are SQL-work measurements,
  not a universal latency guarantee or a full scheduler benchmark.
- Three executable owner-language scenarios cover no additional work on retry,
  retention after thirty-one days and immediate read denial on deletion.
- Signed HQ HTTP route checks and console rendering/escaping/control checks.
  A local synthetic panel preview was inspected in the browser; this does not
  establish production-served behavior.

The final full gate passed from the worktree root: formatting, Clippy with all
targets/features and warnings denied, all workspace tests, console checks,
personal-data check using the ignored local values, collection parity and diff
whitespace. A paused participant's tool-list visibility failed its new red test
and passed after the handler/listing rules were aligned. No live model calls,
installation, production feature activation, push or pull request occurred.

## Remaining boundaries and next steps

The UI saves one JSON page. Whole-history export is available by traversing the
owner's bounded pages; a one-click streaming archive download and broad research
search are not implemented. Archive growth still increases disk usage, index
maintenance and backup/export time. Existing raw model transcript retention is
unchanged; the permanent policy here applies to the new peer conversations.

Admission quotas use independent content-free run and member/day counters,
updated in the same transaction as acceptance. They contain no conversation link
and survive explicit history deletion, so deleting messages cannot reset a limit.
Retries do not increment either counter. Routine checks use point lookups rather
than counting the archive.

Phase 3 must add assistance, authorized original-evidence reads, trusted project
charging and recoverable delivery together, preserving legacy consultation.
Phase 4's resilience/pilot gate and the reviewed deployment procedure still apply.
No automatic deployment follows this checkpoint: first identify the exact clean
candidate, rehearse HQ backup/restore and schema upgrade, then obtain the required
deployment/pilot budget and risk authorization. Use the existing drain and
`make deploy-check` workflow when that gate is reached.
