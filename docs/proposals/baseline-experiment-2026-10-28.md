# Baseline experiment: HQ against one Claude Code session (2026-10-28)

Status: designed, not yet run. Owner's direction, 2026-10-28: make the system prove it is useful,
not explain why it has not been.

## The two questions

1. **Can HQ do real work more efficiently than one Claude Code session?** Ten real tasks, each
   done both ways against the same criteria, written before either runs.
2. **Does HQ give value a session cannot?** Watching the lab, scheduled work, catching problems.
   HQ does not have to beat Claude Code at coding to earn a place; this is measured separately,
   below.

A possible answer, and an acceptable one: Endora as the assistant and interface, one capable coding
agent for development, and a much smaller HQ for persistent operations.

## The freeze (from 2026-10-28 until the results are recorded)

- **No platform development on Endora:** no new features, mechanisms, roles, guards or tools.
  Allowed: fixes for genuine blockers, security problems and data loss.
- **HQ does not improve itself during the experiment.** A task that fails stays failed. A defect
  found during a run is fixed only if it blocks all work, and is then entered in the defects table
  below with the tasks it affected. Experimental conditions do not change silently.
- **The measurement stays this table.** No dashboard, scorecard or new instrumentation for it.

## The ten tasks

All in the ansible-nas fork (`develop` at `ee878f7`) or read-only on the lab. None touches
inventories, `group_vars`, `host_vars` or vaults, or MongoDB's password (the owner's accepted risk).
Code tasks end as a branch ready for review; the owner merges and runs playbooks. Ops tasks change
nothing on the lab and end as a written finding and proposal.

| # | Size | Task | Acceptance criteria (all must hold) |
|---|---|---|---|
| 1 | S | READMEs for the `gluetun`, `prowlarr` and `nzbget` roles | Each has a README stating what the app is, every variable in its `defaults/main.yml` with its default, and its port; no personal values; `yamllint .` and `ansible-lint nas.yml` unchanged |
| 2 | S | READMEs for the `sonarr`, `radarr` and `deluge` roles | As task 1, for these three roles |
| 3 | S | Pin the `homepage` and `stats` roles off the `latest` image tag | Each tag is a release that exists on its registry, named in the commit; the roles' molecule scenarios (or, if they cannot run, `ansible-lint`) pass |
| 4 | S | Run the fork's custom lint-rule tests in CI | `.github/workflows/ci.yml` runs `tests/test_lint_rules.py`; a run on the branch passes; a deliberately broken rule fails it (shown once, then reverted) |
| 5 | M | `minio`: no literal secret default | The role's secret variables have no literal default; the play fails with a clear message when they are unset; the docs say how to set them through a vault variable; no secret value appears anywhere, including logs; lint passes |
| 6 | M | `nextcloud`: no literal secret default | As task 5, for `nextcloud` |
| 7 | M | Sonarr and Radarr report indexer errors (HTTP 429): why, and the fix | The failing indexers are named with log lines quoted from both apps; the cause is stated (rate limit, dead indexer, or other) with evidence; a proposed change (Prowlarr or app settings) with its expected effect; nothing changed on the lab |
| 8 | M | Is the 2-hourly gluetun stack restart still needed? (after the off-window ends, 29 Oct 20:08 EDT) | VPN and download-stack health across the window without restarts, with log evidence at three or more points; a keep or remove recommendation; if remove, the ansible-nas change proposed as a branch |
| 9 | L | What upstream ansible-nas changed that this fork lacks | A list of upstream changes since the fork point that touch roles the NAS runs, each with whether to take it and why, and a proposed merge order; read-only |
| 10 | L | A lint rule that refuses literal secret defaults in role defaults | A rule in `tests/lint_rules` with tests (a literal default fails; a vault reference or an empty default passes); it reports today's offenders without failing CI on them (they are tasks 5 and 6's work) |

Sizes give a spread: four small, four medium, two large; six code, two operations, two research.

## How each task runs

- **Arm A, one Claude Code session:** a fresh session per task, its default model, in a fresh
  worktree of the fork at `ee878f7`, on branch `exp/a-NN`. The prompt is the task and its criteria
  as written above, nothing else. The branch stays local until arm B is finished.
- **Arm B, HQ:** the same text, put as one item on the matching loop's plan (ansible-nas clean-up
  or Home lab upkeep), with no assignee. HQ runs as it is today, with its own branches.
- Both arms of a task start the same day. Operations tasks are read-only, so neither arm affects
  the other.
- **Cap:** a task fails in an arm if it is not ready for review within 24 hours, or costs more
  than $2 in that arm.
- **Judging:** the owner judges each result against its criteria only. Results are shown as X and
  Y, with the arm hidden where the result does not give it away.

## What is recorded

Five fields per task per arm. Nothing more.

| Field | Meaning |
|---|---|
| Accepted | Yes or no, against the written criteria |
| Cost | Dollars: HQ's ledger for the task; the session's reported cost for arm A |
| Elapsed | From start to ready for review, wall clock |
| Interventions | Times the owner had to step in before it was ready (an answer, an approval, a nudge) |
| Rework | Fixes needed after acceptance, checked seven days later |

## Results

| # | A accepted | A cost | A elapsed | A interventions | A rework | B accepted | B cost | B elapsed | B interventions | B rework |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | (to judge) | 80k tok | 2.4 min | 0 |  | (to judge) | $0.01 | 76 min | 0 |  |
| 2 | (to judge) | 70k tok | 2.4 min | 0 |  | (to judge) | $0.01 | 134 min | 0 |  |
| 3 | (to judge) | 107k tok | 16 min | 0 |  |  |  |  |  |  |
| 4 | (to judge) | 97k tok | 22 min | 0 |  |  |  |  |  |  |
| 5 | (to judge) | 79k tok | 3.3 min | 0 |  |  |  |  |  |  |
| 6 | (to judge) | 85k tok | 4.1 min | 0 |  |  |  |  |  |  |
| 7 | (to judge) | 82k tok | 6.1 min | 0 |  |  |  |  |  |  |
| 8 | | | | | | | | | | |
| 9 | (to judge) | 59k tok | 0.7 min | 0 |  |  |  |  |  |  |
| 10 | (to judge) | 89k tok | 3.3 min | 0 |  |  |  |  |  |  |

### Run log

- 28 Oct 08:18: arm B items on HQ's plans as tasks 882–890 (task 8 waits for its window); the
  Holodeck (projects 3 and 5) paused.
- 28 Oct 08:18: arm A could not start. Sessions cannot be opened over Remote Control, and the
  headless CLI is not signed in; waiting on the owner. Its worktrees are ready at `ee878f7`.
- Task 1, arm B: ready 09:34 (branch `hq/task-882-…`, 3 files, checks passed). Nine turns: Bo
  (local) 4, Rae (the Mac) 3, Ada (Haiku) 2.
- Task 2, arm B: ready 10:32 (3 files, checks passed), nine turns.
- Task 3, arm B, 10:5x: Bo asked the owner for the image tags (desk 4820) after cloning an invented URL
  (`github.com/owner/ansible-nas`) instead of using its checkout tool. Answering counts as an
  intervention.
- 28 Oct 12:11: arm A started as nine subagents of the coordinating Claude Code session (the
  owner was away from the Mac, so neither new sessions nor the headless CLI could start). Each is a
  fresh Claude Code agent with its own context, given only the task file, in its own worktree.
  A deviation from "a fresh session"; re-run with sessions if a result is borderline.

- 28 Oct 12:12–12:34: all nine arm A tasks ready, none needing the owner. Arm A's cost is in
  tokens (subagents report tokens, not dollars). Notes for judging: task 3 also fixed 14 lint
  findings in other roles so `ansible-lint` passes (11 files); task 4 pinned CI's tools, fixed the
  same findings and excluded `inventories/` from yamllint (14 files, pushed as the task allowed);
  task 7's agent printed one line of a `group_vars` diff while reading history (a rule slip).
- Arm B interventions so far: task 3 (the owner answered desk 4820), task 7 (desk 4821, Bo asked
  which task it was on).

## Defects found during the runs

| When | Defect | Tasks affected | Fixed? (blocker only) |
|---|---|---|---|
| 28 Oct 09:11 | Pausing the Holodeck holds its players' wakes, but the idle check still counts them: league game 2 (task 880) was forfeited by black (Claude Haiku) because of the pause, not its play. The paused league also keeps opening games (3, dropped as abandoned; 4, queued). | None of the ten; the league table (Local credited a win) | No: not a blocker. The forfeit is noted for the owner to void or keep after the experiment |
| 28 Oct ~12:14 | Harness, arm A: the agents' worktrees share one git stash list; agents a-06 and a-10 stashed at the same moment and got each other's changes. Both recovered their own work (a-10 says so with evidence; a-06 to be checked) | Tasks 6 and 10, arm A | No platform change; check a-06's branch carries only its own work before judging |

## Question 2: what HQ does that a session cannot

Over the same days, one line per event from HQ's standing work (the lab check and other
schedules): what it found, whether it was real, what it fixed or proposed, and whether the owner
had already noticed. Counted at the end: real problems caught first by HQ, false alarms, and cost.

| When | What HQ found | Real? | Fixed or proposed | Owner noticed first? |
|---|---|---|---|---|

## Decision rule

Written now, before the results:

- **Arm A accepts more tasks for less cost and time:** development work moves to Claude Code
  sessions. HQ keeps only the standing work that question 2 shows is worth its cost.
- **Arm B is close on acceptance and cheaper or needs fewer interventions:** HQ keeps development
  work in its loops.
- **Mixed:** split by task kind, using the table.
