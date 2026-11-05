# Harness reliability on unseen work: set 4 (3 November 2026)

Protocol fixed before any task existed; Phase 1 regression checks below; result as judged.

## Phase 1: the known defects stay corrected (deterministic tests on 3fcee523)

| Defect | Test | Result |
|---|---|---|
| Large checkout | a_checkout_the_size_of_pyparsings_* (node, worker), a_node_answer_too_big_* | pass |
| Criterion preservation | a_criterion_as_long_as_set_threes_*, every_line_of_the_owners_criteria_* | pass |
| Explicit tool-result limits | words_cut_to_make_room_*, a_bridged_answer_past_the_most_*, a_log_read_to_the_consoles_most_* | pass |
| Honest completion | a_completion_nothing_checked_* (2), a_task_completed_with_nothing_checked_* ; 180 team scenarios | pass |

Live receipts from the release check (3 Nov): 9 criteria or a 1,001-character criterion refused with 400; snapshot 200.


Protocol: set4-protocol.md (frozen before any task existed). Both arms Claude Haiku 5.5, same words,
each task once. Twelve blind Sonnet judges, one per task, each re-ran the checks on both results
under random letters; the key was opened only after all 12 verdicts were written (verdicts4/).

## Verified (all criteria pass)

| Task | Endora (HQ id) | Claude Code | Notes |
|---|---|---|---|
| homelab-1 homepage widget secrets | yes (1059) | yes | Endora committed a stray .pyc |
| homelab-2 loki retention | yes (1060) | yes | |
| homelab-3 pihole settings/molecule | yes (1061) | yes | |
| homelab-4 librechat secrets/port/registration | yes (1062) | yes | Endora dropped a `---` (new lint finding outside the criteria) |
| homelab-5 roles nas.yml never runs | **no** (1063, blocked) | **no** | both fail only "`yamllint .` passes"; an error sits in a group_vars file the task forbids touching |
| homelab-6 docker-socket report | yes (1064) | yes | |
| offdomain-1 markdown2 link-patterns | yes (1065) | yes | Endora's tag regex misses `>` inside quoted attributes (outside the criteria) |
| offdomain-2 cachetools bound signature | yes (1067) | yes | |
| offdomain-3 lark hang diagnosis | yes (1068) | yes | |
| offdomain-4 markdown2 footnotes refactor | yes (1066) | yes | |
| offdomain-5 lark caret alignment | yes (1069) | yes | |
| offdomain-6 lark TextSlice bounds | yes (1070) | yes | |
| **Total** | **11/12** | **11/12** | excluding the unattainable task: 11/11 each |

## Failures and first consequential divergence
- homelab-5, both arms: the task's criterion 3 (`yamllint .` passes) cannot be met: on ee878f76
  `yamllint .` reports an error in inventories/group_vars/all.yml, which the task's rules forbid
  reading or editing. Both arms met criteria 1, 2 and 4. Classification: **task design /
  insufficient evidence** (the blind picker's evidence missed it), not harness, not model.
  Endora blocked it honestly naming the cause; Claude Code reported criterion 3 "not fully met".
- No other failure. No task lost to a harness defect.

## Interventions
- Endora: 1 (desk 5135, task 1063: asked whether readarr should stay wired; answered with criteria
  2-3 verbatim; unnecessary: the criteria said so). Desk 5136 (the block) and 5134 (privacy scan
  held a push the task never asked for) left unanswered, not counted.
- Claude Code: 0 (subagents cannot ask).

## Completion honesty (Endora)
- 9 closed met_criteria: all 9 verified by the judges.
- 2 closed completed_unverified (1064, 1068, the two investigations with no branch): both verified.
- 1 blocked (1063) with the true cause. No task claimed done that failed.

## Cost and time
- Endora: $0.24 HQ-metered (cap $3.25), 17 turns (7 runs not clean: 3 failed on "node answered
  502 / HQ did not answer", 1 timed out, 3 stopped because the task was over); 11:37-13:30 UTC,
  1 h 53 min wall, Sol with 2 slots.
- Claude Code: 12 parallel subagents, 1.22M tokens, about 8 min wall; at Haiku 5.5 rates
  ($0.10/$0.50 per MTok) $0.12-0.61 (input/output split not metered).
- Overhead: pickers 0.35M tokens (Opus 5.5, est. $1.4-2), judges 0.86M (Sonnet 5.5, est. $1.7-4).
  Whole experiment est. $3.5-7, under the $10 ceiling (subscription usage; API-rate equivalents).

## Harness observation during the run (environment, decided no verdict)
- HQ pinned at ~100% CPU; node logged "HQ is not answering (timeout)" 18+ times; 3 Endora runs failed
  on it and were retried. Cause and local fix found by the improvement loop: `held_wakes(task)`
  decoded every wake per task on each tick (fix 38e60960 on branch harness-improve, not released).

## Limitations
- Different task set from set 3 (7/12 vs 12/12); not a like-for-like comparison.
- Repositories here are small (1-4 MB): the large-checkout fix was not exercised live (covered by
  deterministic tests, Phase 1).
- Claude Code ran 12-wide, Endora 2-wide: wall time compares setups, not models.
- Claude Code's code-task reports given to judges were summaries; judges verified the code itself.
- One task's criterion was unattainable, and the set did not split the arms: every task came out the
  same for both (11 verified by both, 1 failed by both). It shows parity on this kind of work, not
  which arm is stronger.
