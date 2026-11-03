# Release candidate manifest, 3 November 2026

**Candidate:** branch `rc-2026-11-03` at `9b0e7ce2`, in `~/repos/Endora-premature-close`.
**Live now:** `38668a2b` (the Role retirement), on HQ and the Pi.

- The candidate is a straight line on top of live: `38668a2b` is its ancestor.
- It needs no merge.
- It carries 14 commits: 35 files, +1,340 / −138.
- Nothing is pushed or deployed.

## What is in it

| Batch | Commits | What changes |
|---|---|---|
| Milestone 1 | `ab0fb8fb` | A checkout as large as pyparsing's reaches the sandbox whole: the contract answer limit is now 48 MiB, and an answer over it says so. |
| | `bbf009d3` | A branch outside the lab is refused the lab's check (`NoLabCheck`), instead of a 404 counted as a failure. |
| | `efceca76` | A criterion over 1,000 characters is refused at intake; nothing is cut at 200 characters any more. |
| Integrity audit | `d2a9d08a`, `04c61e38` | A ninth criterion is refused at intake (`TooMany`); before, it was silently dropped. |
| | `f4a325ca` | The worker's instructions no longer mention a tester, a reviewer, a lead or roles. |
| | `9d3b2fc9` | Words cut to make room for the turn's closing line are marked `…[cut]`. |
| | `e1bb5e16` | A turn whose rounds run out with nothing said still posts that it ran out. |
| | `b91129e9` | A thread longer than the 12 posts a turn reads says that older posts were left out. |
| | `a7d42364` | A turn whose node could not list its tools is told so, and told to say so in the thread. |
| | `354409a9` | A bridged tool's answer over 16K says how much was cut. |
| #796 follow-up | `8d340f86` | A log read that reaches its limit says older lines may be left out; Portainer is asked for the same limit. |
| | `a3506081` | `set_status` answers with the item's real status, not the one asked for. |
| | `9b0e7ce2` | A solo item with no branch still closes on its own, recorded as `completed_unverified` and shown as "Completed · Not verified"; the scorecard and `experiment.py` count it apart. |

Each change has its evidence in one of three reports in this folder:
- `mechanism-audit-2026-11-02.md` (Milestone 1's record);
- `harness-integrity-audit-2026-11-03.md`;
- `premature-close-2026-11-03.md`.

## Review

**Overlaps.** Eleven files were changed by more than one commit. They hold together:
- **Criteria in `evidence.rs`.** The length limit (`efceca76`) and the count limit (`d2a9d08a`) both refuse, with one `Refusal::Criterion` path.
- **Instructions in `turn.rs`.** Each of the five commits touches a different function: instructions, `beside`, the rounds-out path and the node listing. The tests for all of them pass together.
- **`protocol.rs`.** The stale wording was removed (`f4a325ca`), and the status answer (`a3506081`, `9b0e7ce2`) builds on it.

**Security and authorization.** The candidate changes none of the following:
- `may_see` or `may_use`, the bridge's stance and `run_as`, or contract tiers;
- the furlough refusal, or the relay's model limits.

It also handles tokens safely:
- The one new error text passed to the model (the node listing failure) has its token removed by `quiet(…, token)`, and a test checks that the briefing holds no token.
- The model cannot pick `completed_unverified` itself: `set_status`'s `why` list does not include it. Only HQ records it.

**Historical data.** No migration is needed.
- `closures.why` has no CHECK constraint.
- Rows already marked `met_criteria` read back unchanged, and a test pins that.
- The new criteria limits apply only at intake (`app/hq/src/api.rs`). Stored tasks are never re-checked against them.

**What it changes for running work.**
- The owner API now refuses (400) a task given more than 8 criteria, or one over 1,000 characters. Before, the extra criteria were dropped or cut without a word.
- Solo, ungated items close as before, but are no longer counted as "met its criteria".

## Test evidence

The gate, `scripts/gate.sh`, was run on `9b0e7ce2` twice:
- by the follow-up session;
- independently by the main session, logged in `rc-gate.log`.

Results: see "Gate" below.

## Known limitations (deferred; not in this batch)

These were found in the audit and the experiments, and recorded for later.

- **Check-box failures blamed on the task.** When the check box itself fails, that counts as a loop failure: 9 of 16 failed checks were the box's own fault. Deciding otherwise is a policy question.
- **Connection failures are final.** A 502, an unreachable node or a timeout is a final run failure, with no retry: 32 runs were lost this way.
- **Stranded tasks.** Tasks 673 and 796 are left as they are.
- **Logs by time.** `container_logs` cannot read by time; a busy container's history past 100 lines is out of reach. The cut is now said, but nothing more was built.
- **Unscoped record search.** Asked without naming a task, the record search ranks recent briefings first, which echo a stale house page.
- **Checkout wording.** The instructions describe `repo_checkout` more narrowly than the tool's own description does.
- **The evidence rule.** It checks that citations exist, not that they settle the goal. This is by design: `completed_unverified` now says so instead.
- **Stale house page.** It still says the gluetun timer runs. The owner keeps that page.
- **Unattributed worker behaviour.** In #796 the worker named the deciding unknown and then asked the owner. Its reasoning is not stored, so the cause cannot be pinned down.

## Rollback

`scripts/release.sh` takes integrity-checked database snapshots before deploying. The previous image is tagged `endora-node:before-<sha>`.

The image rollback is the usual one:
1. Retag `before-9b0e7ce2` as latest.
2. Recreate the containers.
3. Resume HQ.

**One forward-only effect.** Once any task closes as `completed_unverified`, `38668a2b` cannot read that closure. A rollback then needs one of these two:
- Restore the pre-release snapshot. That loses every write since the release.
- Run one owner-approved update on the HQ database: `UPDATE closures SET why = 'met_criteria' WHERE why = 'completed_unverified'`.

Nothing else this candidate writes is new in shape.

## Gate

`scripts/gate.sh` on `9b0e7ce2`, run by the main session, exit 0:

- formatting, clippy (`-D warnings`), tests, console, nothing-personal, collection parity and diff check all pass;
- 2,221 unit tests, 224 scenarios (2,201 steps), 0 failed;
- the follow-up session's own run of the same gate gave the same totals.

Code size, product lines across all areas:
- 102,189 now;
- the team, node and hq areas alone come to 67,778.
