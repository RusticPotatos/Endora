# Harness improvement ledger, 3 November 2026

The record of this cycle's corrections, the hypotheses tested and rejected with their evidence, and
set 4's results by kind of outcome. Kept so the next cycle starts from evidence, not memory.

## Corrections

| # | Defect | Evaluator (unchanged) | Baseline | Commit | Result | Decision |
|---|---|---|---|---|---|---|
| 1 | HQ's tick decoded every wake once per task while holding its engine lock (~10 s a tick on the NAS): node calls timed out, turns failed with 502s | new test `held_wakes_of_a_task_do_not_decode_unrelated_retained_history`; bench on a copy of the live store; live CPU and node logs | test failed; tick 2.15 s (Mac copy); live HQ 1.01 cores, 51 node timeouts in the 3 h before release | `38e60960` | test passes; tick 0.12 s (Mac copy); live 0.11 cores, about 1.1 s CPU a tick, snapshot 4.6 s to 0.52 s, 0 timeouts and 0 502s in 18 min after | **Released** 3 Nov 13:52 UTC (NAS and Pi, 0 turns cut) |
| 2 | The model's own `git add -A` committed Python bytecode (set 4, #1059): the fork has no ignore entry, the worker's instructions say add-all | new test `the_models_own_add_all_leaves_pythons_byproducts_out` | test failed: `loose.pyc` and `tests/__pycache__/…pyc` staged | `ecdb11b8` (branch `harness-improve-2`, on `38e60960`) | test passes; worker suite 102 + 62; full gate 2,225 unit, 224 scenarios / 2,201 steps, 0 failed | **Kept for the next ordinary release**; not deployed separately |
| 3 | The lab check counted yamllint's errors only, so a warning a branch added was invisible and the receipt said "no new findings against develop: met" (set 4, #1062: a dropped `---`; the member repeated the receipt as its own claim) | new test `a_new_yamllint_warning_is_a_new_finding_against_develop` (the real `yamllint .` pipeline, #1062's six errors and one warning replayed) | test failed with the live receipt's words: `yamllint 6 errors (develop 6): no new findings against develop: met`. Real #1062 trees (`ee878f76`, bundle head `a636acbb`): 6 errors + 0 warnings, then 6 + 1 | `6fd0bbfa` (branch `harness-improve-3`, on `ecdb11b8`) | test passes; real trees counted 6 then 7, so the check now says "not met". Worker suite 103 + 62 (8.8 s); full gate 2,226 unit, 224 scenarios / 2,201 steps, 0 failed (3 min 31 s) | **Kept for the next ordinary release**; not deployed. Summaries now read `yamllint N findings` |

Not changed (owner): the privacy scan's policy. Its hold on #1062's push, for a placeholder mailbox
the base commit already carries, stays the owner's decision each time.

## Hypotheses tested and rejected

| Hypothesis | Evidence | Verdict |
|---|---|---|
| The lab check's count comparison hid a new lint finding behind a fixed one (#1062) | `ansible-lint --offline -p nas.yml` on #1062's branch and its base: identical output (14 findings each, the same lines). `nas.yml` does not run `roles/librechat`, so the role is outside the check; the judge's finding came from linting the role directly. | Not reproduced. Masking is possible in principle but never observed; nothing built. Known scope limit: roles `nas.yml` does not run get no ansible-lint from the check. The real cause of #1062's false claim was found later: yamllint's warnings went uncounted (correction 3). |
| Turns refused by the relay ("Opus is the CEO's alone") waste turns | 37 runs, the last on 30 Oct; none since. | Obsolete; no change. |
| #1063's question to the owner came from a briefing that hid its criteria | Run 5350's stored transcript (not cut) carries all four criteria verbatim, criterion 2 ("nas.yml runs `readarr`…") about 10 KB in, before the `ask_owner` call. | Rejected: model judgment. A guard that refuses questions the criteria answer would have to judge meaning; that is an owner or architecture decision, not a small fix. |
| offdomain-1's tag pattern missing `>` inside quoted attributes is a harness defect | The pattern is the model's own code, outside the task's criteria; the repo's tests pass and nothing in the harness reads or shapes it. | Rejected: model artifact quality; no harness path to fix. |
| Live outcomes after the `38e60960` release show a new defect | Read-only HQ store, 13:52-15:57 UTC: 2 turns, both idle downtime turns (member 6), succeeded; no task turn, closure or desk card; node logs since the release: 0 errors, timeouts or 502s. | Nothing to review yet; no watch-list item recurred. |
| Turns that run 30 min with zero model tokens are a separate defect | 12 of 14 timed-out turns this week had 0 tokens; each had a briefing built within a second of its start and no call receipt; all fall in windows when HQ was lock-bound. Container logs that would show the hang were replaced at the release. | Unproven; most likely the turn's end report failing to reach a lock-bound HQ. No change unless one recurs after `38e60960`. |

## Set 4 by kind of outcome (12 unseen tasks, judged blind)

| Kind | Endora | Claude Code |
|---|---|---|
| Task completion: verified, all criteria | 11 | 11 |
| Not completed: the task's own criterion unreachable (homelab-5: `yamllint .` fails on a file the task forbids touching) | 1 (blocked, true cause named) | 1 (reported "not fully met") |
| Artifact quality outside the criteria | 3 notes: a committed `.pyc` (#1059, fixed by `ecdb11b8`); a dropped `---` adding a lint finding while its report claimed none (#1062: the check's receipt said so, fixed by `6fd0bbfa`); a tag pattern that misses `>` inside quoted attributes (offdomain-1) | 1 note: out-of-scope whitespace edits (homelab-5, harmless) |
| Model judgment | 1 unnecessary question to the owner (#1063, answered by its criteria); #1062's "no new findings" claim, which repeated the check's own receipt (harness, correction 3) | none observed |
| Infrastructure | 3 turns failed on HQ timeouts and 502s, 1 turn timed out with no tokens; all retried, no verdict changed; cause fixed (`38e60960`) | none |

**First consequential demonstrated harness failure:** HQ's lock-bound tick (infrastructure caused
by harness code). It changed no verdict but cost turns and time; it is fixed and live. No other
demonstrated harness failure decided an outcome in set 4. The next cycle starts from the first such
failure in real use.

## Watch list (act only on a reproduced failure)
- A turn that ends "took longer than a turn may" with zero tokens after `38e60960`.
- A branch whose report claims "no new findings" where a reviewer finds one.
- Turns failing on "HQ is not answering" or "conversation was not kept" after `38e60960`.
