# Agent collaboration: operational baseline

Status: Phase 0 evidence, 2026-10-21. Prepared with AI assistance. No deployment,
agent wake, model experiment or production database mutation was performed.

## Capture and reproducibility

The live node's public health endpoint reported build `248deb41`, version
`0.12.0`, status `ok`. Node and HQ were confirmed to use the same built image.
This matches the code baseline. A read-only logical capture
of HQ was taken at 2026-10-21 21:24:49 America/New_York, which is
2026-10-22 01:24:49 UTC. The analysis window begins 72 hours earlier.

Capture method: Python's SQLite client inside the existing HQ container, URI
`file:/data/hq.db?mode=ro`, `PRAGMA query_only=ON`, one `BEGIN` transaction for all
selected rows, then rollback/close. No snapshot file or script was created in
the production container. Journal mode was `delete`, so the brief read transaction
could temporarily delay a writer; it made no changes. Credential tables and
unrelated secret settings were excluded. Raw private material is retained only
under the worktree's ignored `.local/agent-collaboration-baseline/`, with directory
mode 0700 and capture mode 0600. No raw transcript, name, address, host, credential
or task description is copied into this document or synthetic fixtures.

Tables captured: members, goals, tasks, task dependencies, posts, wakes, runs,
ledger, events, consults, call receipts, transcripts, projects, documents,
criterion citations, branches, checks and check-backs. Only the coordination
guard, drain and stop-related settings were selected. Run tokens were excluded.
At capture: 748 tasks, 2,051 runs, 2,345 wakes, 5,049 ledger rows, 22,095 retained
events, 482 call receipts, 238 transcripts and 2 consults.

Recompute the bounded aggregate without contacting production:

```sh
python3 scripts/test-agent-collaboration-baseline.py &&
python3 scripts/agent-collaboration-baseline.py \
  .local/agent-collaboration-baseline/capture.local.json --hours 72
```

The [analyzer](../../scripts/agent-collaboration-baseline.py) emits only numeric
fields and whitelisted classifications, never record text. Its
[12 offline tests](../../scripts/test-agent-collaboration-baseline.py) cover
accounting corruption, empty/missing evidence, unfinished work, privacy canaries
and the difference between an answered consult and its turn label. Additional
tests were observed failing before their fixes. The initial test run failed
because the analyzer did not yet exist.

The raw capture is not portable public evidence. A second authorized reviewer
can reproduce these aggregates from that private artifact; public readers can
run the synthetic controls and inspect the accounting method. Capture-specific
claims below are observations, not independently reproduced live trials.

## Cost cohort and evidence coverage

The cohort is **completed runs whose stored latest `started_at` is in the
72-hour window**. Retrying overwrites that field. All recorded usage and charges
of those run IDs are included, even if an earlier attempt preceded the window.
This is not a first-start cohort, calendar-day spending, or the owner's bill.
Failed, timed-out and stopped runs are included. In-flight work is reported
separately; none met this window's started-but-unfinished predicate at capture.

| Measure | Observation |
|---|---|
| Completed run IDs | 310 |
| Recorded starts for those IDs, including retries | 344 |
| Runs with positive recorded cost | 106 |
| Ledger charges | 1,819,208 microdollars ($1.819208) |
| Sum of run `spent` | 1,819,208 microdollars |
| Sum of ledger `use` | 1,819,208 microdollars |
| Accounting mismatch / missing reservation witness | 0 / 0 |
| Initial wake references / retained start events | Present for all 310 run IDs |
| Actual brain missing on the run row | 28 runs, 387,285 microdollars |
| Retained transcript | 223 of 310 runs; none of these marked cut |
| Transcript unavailable | 87 runs |
| Stored start earlier than stored queue time | 100 runs |

Accounting verdict: **pass for this cohort**. Sum `charge` once; compare to `use`
and `spent`; do not add reservations, usage, charges and releases together.
`SqliteTeamStore::settle` omits zero charge rows and releases the whole reserved
amount, not just unused funds. A zero-cost turn therefore requires a reservation
witness and consistent zero usage/spent, not a fictional zero-valued charge row.
See [SQLite settlement](../../domains/team/src/infrastructure/sqlite.rs).

Full causal/model coverage verdict: **cannot tell**. Retention is incomplete,
older model choices are absent, and queue/start timestamps do not support a
reliable general queue-latency estimate. Do not fill in model identity from a
member's current configuration. The event feed starts at event 28631 and ends
at 62665; absent older events cannot be reconstructed from this capture.

### Communication and execution categories

Classify by each run's original wake, not by a claim about useful work. Folded
wakes can change a run's purpose, so categories below are descriptive and do not
identify the marginal cost of a message.

| Initial wake | Runs | Charges (microdollars) |
|---|---:|---:|
| Escalated question (`question_asked`) | 12 | 724,362 |
| Assigned work | 65 | 495,064 |
| Decision (`decide`) | 12 | 141,489 |
| Review request | 29 | 140,925 |
| Alarm | 8 | 99,830 |
| Acceptance request | 17 | 90,220 |
| Sent back | 6 | 65,559 |
| Guard | 5 | 37,256 |
| Recovery | 30 | 10,591 |
| Directed | 1 | 5,602 |
| Assignment recovery | 3 | 4,318 |
| Huddle | 3 | 3,992 |
| Other initial reasons, including both consults | 119 | 0 |
| Total | 310 | 1,819,208 |

The 12 question-initiated runs account for **39.82% of cohort cost**, under this
specific denominator. This neither reproduces nor disproves the supplied paper's
historical 36% figure. It does not mean 39.82% was unnecessary.

The stored labels classify 224 runs as advanced, 21 checked, 10 blocked, 39 talk
only and 16 empty. Both specialist answering turns are in the empty group even
though their consult records are answered. Do not use `empty` or `talk only` as
an automatic waste measure; keep request outcomes alongside existing labels.

## Five reconstructable workflows

Identifiers below are operational references, not copied private content.

1. **Direct consultation and deferred answer wake.** Consult 1 links requester
   run 1783 on task 687 to expert run 1785 with no task. The consult is answered
   and `asker_woken` is true. Events 39311, 39318, 39321, 39322 and 39325 record
   expert start, record search, answer post, held wake and end. Its stored brain
   is local and its charge is zero. Consult 2 independently links run 1802 on
   task 686 to taskless run 1803; events 40213, 40222, 40228, 40229 and 40234 show
   start, search, answer, folded wake and end. Both transcripts remain available.
   Direct contact already works; task-thread dependence and outcome labels are
   the gaps. [Implementation](../../domains/team/src/application/engine/consult.rs)
2. **Submission, tester and lead acceptance.** Task 687 retains documents 15/16,
   check receipt r228, submissions by runs 1749/1757, tester run 1792 citing its
   own r244, and lead acceptance run 1793. Task 686 separately retains tester
   run 1805 and acceptance run 1807, with r257/r258. These are staged criterion
   records, not a model's unsupported completion message. This trace establishes
   that the evidence workflow ran; it does not prove the underlying lab change
   met every criterion. [Evidence checks](../../domains/team/src/application/engine/evidence.rs)
3. **Expensive responsibility/evidence questions.** Task 687 has five
   question-initiated runs (1759, 1775, 1778, 1781, 1786), totalling 244,272
   microdollars. Retained questions report lifecycle/tool refusals, missing
   readings, lead identification and acceptance routing. They are candidates
   for narrower fixes, not proof that all RACI checks are wrong. The subsequent
   tester/acceptance sequence is retained. Task 746 has question runs 2047 and
   2050, costing 124,697 microdollars; events 62235-62243 and 62296-62303 expose
   the question/answer wake chain. No acceptance was recorded for task 746 at
   capture. [Question path](../../domains/team/src/application/engine.rs)
4. **Long wait and repeated attempts.** Run 1766 on task 682 has an approximately
   10.25-hour difference between queued time and latest start, several retained
   start/end pairs (38981/38983, 39010/39016, 39045/39047, 39193/39356), receipts
   in its successful final attempt and one recorded interruption. Its final
   brain is Mac and its cost zero. This is not proof of 10.25 hours spent only
   waiting for capacity: the stored latest start includes retries. Do not turn
   it into a pure host-wait latency benchmark without attempt-level evidence.
5. **Interrupted run, bounded retry and final settlement.** Run 1951 on task 707
   starts at event 53040, ends interrupted/retry-waiting at 53056, restarts at
   53064, ends retry-waiting at 53079, starts at 53088 and succeeds at 53101.
   Its three attempts, one interruption and 14,954 microdollars are retained.
   Node receipts r390/r391 support the task's resubmission; later tester run
   1955 uses r394/r395 and lead 1962 accepts. The retained interruption does not
   identify a particular drain as its cause. All 29 retained drain-ended events
   report zero cut turns, so attributing this interruption to them is unsupported.

## Failure classification and limits

Reviewing retained question posts yields these provisional categories. Questions
are testimony; a reported refusal needs the actual handler response before it
can be called an enforcement defect. Categories overlap and are not a partition
of costs.

| Category | Evidence targets | Conclusion for the first slice |
|---|---|---|
| Lifecycle/tool refusal and evidence access | Question runs 1759, 1775; task 687 documents and check records | Test document reach and review state separately from message delivery. Do not grant general write rights. |
| Task area, ownership or lead identification | 1768, 1778, 1781, 1786 | Pass immutable IDs and explicit responsibility context; retain actual area authority. |
| Owner-controlled configuration / risk | 1764, 1846 | Preserve escalation for genuine owner decisions. |
| Recurring alarm / unclear criterion | 1829, 1862 | Better transport cannot resolve a contradictory criterion or authority decision by itself. |
| Specialist responsibility / repeated coordination | 2047, 2050 | A bounded diagnostic assistance case is appropriate; no automatic authority grant. |
| Record coverage and classification | 87 absent transcripts; 28 absent brains; answered consults labelled empty | Record actual execution and request outcomes; report unknowns explicitly. |

In the window, wake records include 44 refusals: 20 duplicates, 11 in-review,
9 out-of-budget, 3 kill-switch and 1 redundant. These are scheduling dispositions,
not evidence that the caller lacked document or repository permission.

The capture has 265 recent node call receipts, all successful (256 reads and
9 acts), while some retained posts describe refused HQ protocol calls. Node
receipts do not cover every HQ tool refusal. A new messaging refusal needs its
own attributable operation record; do not treat an absence of node refusals as
an absence of failed coordination. A protocol success still cannot replace
the node's receipt for a consequential action.

## Phase 0 outcome

- Causal reconstruction: **pass for the five bounded workflows above**, with
  their stated missing/corrupted timing and evidence limits.
- Cost categorization and reconciliation: **pass for the declared run cohort**;
  useful versus waste and marginal message cost remain **cannot tell**.
- Benchmark definitions: **pass**; [B1-B8](agent-collaboration-baseline-cases.md)
  freeze synthetic inputs, current controls and future candidate tests.

This baseline supports proceeding to Phase 1's explicit contract and a bounded
diagnostic pilot. It does not support projected savings or broader deployment.
The next design must resolve taskless evidence scope, project charging, outcome
tracking and durable delivery before enabling new execution behavior.

## Checkpoint verification

The 12 analyzer tests, all seven required root checks, document link/ASCII checks
and arithmetic reconciliation passed on the isolated branch. The root suite also
exercised the existing behavior controls referenced by B1-B8. No new taskless
behavior or performance result is claimed. Cargo emits its existing warning
about terminal package profiles being ignored; this checkpoint changes no Rust
configuration. Terminal-specific checks and live smoke were not run because
the terminal was unchanged and nothing was deployed.

The [Phase 1 decision](../adr/0137-addressed-conversations-and-bounded-assistance.md)
now resolves the contract and records the owner's preference for retaining all
conversations until deletion, with indexed bounded routine access. B9 adds the
archive-size regression. The next implementation slice is information-only
messaging with zero automatic tasks/wakes; the production pilot remains gated.
