# Harness improvement ledger, 3 November 2026

The record of this cycle's corrections, the hypotheses tested and rejected with their evidence, and
set 4's results by kind of outcome. Kept so the next cycle starts from evidence, not memory.

## Corrections

| # | Defect | Evaluator (unchanged) | Baseline | Commit | Result | Decision |
|---|---|---|---|---|---|---|
| 1 | HQ's tick decoded every wake once per task while holding its engine lock (~10 s a tick on the NAS): node calls timed out, turns failed with 502s | new test `held_wakes_of_a_task_do_not_decode_unrelated_retained_history`; bench on a copy of the live store; live CPU and node logs | test failed; tick 2.15 s (Mac copy); live HQ 1.01 cores, 51 node timeouts in the 3 h before release | `38e60960` | test passes; tick 0.12 s (Mac copy); live 0.11 cores, about 1.1 s CPU a tick, snapshot 4.6 s to 0.52 s, 0 timeouts and 0 502s in 18 min after | **Released** 3 Nov 13:52 UTC (NAS and Pi, 0 turns cut) |
| 2 | The model's own `git add -A` committed Python bytecode (set 4, #1059): the fork has no ignore entry, the worker's instructions say add-all | new test `the_models_own_add_all_leaves_pythons_byproducts_out` | test failed: `loose.pyc` and `tests/__pycache__/…pyc` staged | `ecdb11b8` (branch `harness-improve-2`, on `38e60960`) | test passes; worker suite 102 + 62; full gate 2,225 unit, 224 scenarios / 2,201 steps, 0 failed | **Kept for the next ordinary release**; not deployed separately |

Not changed (owner): the privacy scan's policy. Its hold on #1062's push, for a placeholder mailbox
the base commit already carries, stays the owner's decision each time.

## Hypotheses tested and rejected

| Hypothesis | Evidence | Verdict |
|---|---|---|
| The lab check's count comparison hid a new lint finding behind a fixed one (#1062) | `ansible-lint --offline -p nas.yml` on #1062's branch and its base: identical output (14 findings each, the same lines). `nas.yml` does not run `roles/librechat`, so the role is outside the check; the judge's finding came from linting the role directly. | Not reproduced. Masking is possible in principle but never observed; nothing built. Known scope limit: roles `nas.yml` does not run get no lint from the check. |
| Turns refused by the relay ("Opus is the CEO's alone") waste turns | 37 runs, the last on 30 Oct; none since. | Obsolete; no change. |
| Turns that run 30 min with zero model tokens are a separate defect | 12 of 14 timed-out turns this week had 0 tokens; each had a briefing built within a second of its start and no call receipt; all fall in windows when HQ was lock-bound. Container logs that would show the hang were replaced at the release. | Unproven; most likely the turn's end report failing to reach a lock-bound HQ. No change unless one recurs after `38e60960`. |

## Set 4 by kind of outcome (12 unseen tasks, judged blind)

| Kind | Endora | Claude Code |
|---|---|---|
| Task completion: verified, all criteria | 11 | 11 |
| Not completed: the task's own criterion unreachable (homelab-5: `yamllint .` fails on a file the task forbids touching) | 1 (blocked, true cause named) | 1 (reported "not fully met") |
| Artifact quality outside the criteria | 3 notes: a committed `.pyc` (#1059, fixed by `ecdb11b8`); a dropped `---` adding a lint finding while its report claimed none (#1062); a tag pattern that misses `>` inside quoted attributes (offdomain-1) | 1 note: out-of-scope whitespace edits (homelab-5, harmless) |
| Model judgment | 1 unnecessary question to the owner (#1063, answered by its criteria); #1062's false "no new findings" claim | none observed |
| Infrastructure | 3 turns failed on HQ timeouts and 502s, 1 turn timed out with no tokens; all retried, no verdict changed; cause fixed (`38e60960`) | none |

**First consequential demonstrated harness failure:** HQ's lock-bound tick (infrastructure caused
by harness code). It changed no verdict but cost turns and time; it is fixed and live. No other
demonstrated harness failure decided an outcome in set 4. The next cycle starts from the first such
failure in real use.

## Watch list (act only on a reproduced failure)
- A turn that ends "took longer than a turn may" with zero tokens after `38e60960`.
- A branch whose report claims "no new findings" where a reviewer finds one.
- Turns failing on "HQ is not answering" or "conversation was not kept" after `38e60960`.
