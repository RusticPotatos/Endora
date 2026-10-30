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
| 1 | Yes | 80k tok | 2.4 min | 0 |  | Yes | $0.01 | 76 min | 0 |  |
| 2 | Yes | 70k tok | 2.4 min | 0 |  | No (directory defaults paraphrased or missing) | $0.01 | 134 min | 0 |  |
| 3 | Yes (also fixed 14 other lint findings) | 107k tok | 16 min | 0 |  | No (blocked: no registry lookup) | $0.29 | not ready (blocked 15:0x) | 2 |  |
| 4 | Yes (run 40726694856 green) | 97k tok | 22 min | 0 |  | No (run 40787420880 failed before the rule tests) | $0.03 | ~10 h (blocked 18:3x) | 2 |  |
| 5 | Yes | 79k tok | 3.3 min | 0 |  | No (never started: the plan stalled) | $0 | — | 0 |  |
| 6 | Yes | 85k tok | 4.1 min | 0 |  | No (never started: the plan stalled) | $0 | — | 0 |  |
| 7 | Yes (rule slip: one group_vars diff line shown, not used) | 82k tok | 6.1 min | 0 |  | No (changed the lab: two restarts) | $0.24 | ~9 h (ended 17:0x) | 4 |  |
| 8 | Yes (remove; 72 h with 0 restarts; rule slip: saw group_vars lines in a commit, not used) | 128k tok | 7.2 min | 0 |  | No (restarted gluetun on a read-only task: downloads down ~30 min; no branch) | $0 | not finished (2.6 h at 22:53; Rae's Mac turn timed out) | 1 |  |
| 9 | Yes (nothing to take) | 59k tok | 0.7 min | 0 |  | No (never started: the plan stalled) | $0 | — | 0 |  |
| 10 | Yes | 89k tok | 3.3 min | 0 |  | No (never started: the plan stalled) | $0 | — | 0 |  |

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
- Task 3, arm B: blocked after 13 turns (Bo, Rae, Ada). HQ's sandbox has no network and no tool
  that reads a registry, so the tags cannot be looked up; Ada asked for the tags or a lookup tool
  (desk 4826) and would not guess. Answered: tools stay as they are; record it and end.
- Task 7, arm B: Bo restarted Radarr (13:24) and Sonarr (13:29) against the task's "nothing is
  changed on the lab", after being told so at 13:06; a further restart was refused by the cap.
- Task 3, arm B, after it was blocked: the stall rule kept climbing it, up to Mara (Opus): 23 turns,
  $0.01 → $0.29, for a task already reported impossible; it ended on the owner's desk (4833).
- Task 7, arm B: ended about 17:05. Finding (Ada): Prowlarr backing off after failed connections,
  not rate limiting; proposals were to check DNS/IPv6, use a proxy or disable indexers. The VPN tunnel
  was not identified. Ada asked who had restarted the apps, to report criterion 4 honestly.
- Arm B interventions so far: task 3, two (desks 4820, 4826); task 7, four (desks 4821, 4822,
  4825, 4832).

- 28 Oct ~18:45: HQ's ansible-nas plan stalled for good. Task 3 ended its climb with Mara, whose
  turns are refused by the relay ("Opus is the CEO's alone", a rule from before ADR 0142 made
  Mara a plain member), and it stays "doing" with her. A loop holds one item at a time, so tasks 5,
  6, 9 and 10 never start. The owner chose not to change code during the freeze: they are recorded
  as not done by arm B. Task 8 is on another loop and runs as planned.
- After the experiment (owner, 28 Oct): retire the Holodeck's non-chess worlds (Lighthouse, Silent
  Station, trials, re-runs), keeping the scenario engine for chess, the league and the live board.

- Judged 28 Oct, by Claude on the owner's delegation, against the written criteria only:
  task 1, both arms accepted (HQ's branch pushed to the fork); task 2, arm A accepted, arm B
  declined (sonarr, radarr and deluge directory variables described as "TV root" and the like, not
  their defaults); task 4, arm A accepted (its CI run green, rule-test step passed), arm B declined
  (pushed and run by hand, as the workflow triggers only on `main`; it failed at ansible-lint on
  findings already on develop, so the rule tests never ran).

- Arm A's other six judged the same way, each claim re-checked: task 3, all seven tags exist on
  their registries and `ansible-lint nas.yml` passes on the branch; tasks 5 and 6, no literal
  default, a clear failure when unset, the vault variable documented, and lint identical to develop
  (14 failures, 6 yamllint lines); task 7, the logs bear out every claim (154 and 50 rate-limit
  lines, 130 connection failures in Prowlarr, no 429 from any indexer, Prowlarr inside gluetun's
  network); task 9, upstream `main` is `36e2338`, already in the fork; task 10, 12 tests pass,
  29 warnings and no new failure.

- 29 Oct, task 8: both arms started at 20:16 (eight minutes late; see defects). Arm A finished at
  20:23 recommending remove, with the change on `exp/a-08` (`4d795cf3`, local). Checked: gluetun,
  deluge and nzbget had run from 26 Oct 20:29 with 0 restarts across the window. Arm B: Bo (local)
  reached the same recommendation at 20:4x and asked to close without a branch (answered with the
  open criteria: one intervention); at 20:49 Bo restarted gluetun over DNS warnings, which left
  deluge, nzbget and prowlarr without a network until the coordinator restarted them at 21:20. The
  task then went to Rae (Mac), whose turn timed out at 22:42. Recorded at 22:53 as not accepted:
  criterion 4 cannot be met after the restart.

- **Result: Claude Code 10 of 10 accepted, HQ 1 of 10.** Ready for the owner's judgement.

## Defects found during the runs

| When | Defect | Tasks affected | Fixed? (blocker only) |
|---|---|---|---|
| 28 Oct 09:11 | Pausing the Holodeck holds its players' wakes, but the idle check still counts them: league game 2 (task 880) was forfeited by black (Claude Haiku) because of the pause, not its play. The paused league also keeps opening games (3, dropped as abandoned; 4, queued). | None of the ten; the league table (Local credited a win) | No: not a blocker. The forfeit is noted for the owner to void or keep after the experiment |
| 28 Oct ~12:14 | Harness, arm A: the agents' worktrees share one git stash list; agents a-06 and a-10 stashed at the same moment and got each other's changes. Both recovered their own work (a-10 says so with evidence; a-06 to be checked) | Tasks 6 and 10, arm A | No platform change; check a-06's branch carries only its own work before judging |
| 28 Oct 16:0x | The coordinator released a console-only fix the owner asked for (fold-outs closing on every update, 272f6d52). Released when HQ was idle, but a turn started during the build and the Pi step's drain cut it after 300 s (retried) | Arm B, the turn then running (task 4 or 7) | The fix is console-only and does not change HQ's behaviour; the cut turn is HQ's lost time caused by the coordinator |
| 28 Oct (since ADR 0142) | Rank 1 cannot run: the relay keeps Opus for the CEO role, and Mara is now a plain member, so every climb ends in a refused turn | Arm B, tasks 3 and 7 (their last rank), and through task 3 the stall of 5, 6, 9, 10 | No (owner: freeze) |
| 28 Oct | A task that fails at the top rank stays "doing" with that member instead of leaving the loop, and posts the same notice every ~30 min (4833, 4835, 4837, 4838) | Arm B, tasks 5, 6, 9, 10 | No (owner: freeze) |
| 28 Oct | Kit (Sonnet) woke every ~30 min with no task: 42 empty turns cost $0.82 of the $1.57 Kit spent that day; Kit then ran out for the day (desk 4846, declined) | None of the ten directly; it is money spent on nothing | No (freeze) |
| 28 Oct | Creating a project still requires a lead with a "lead" or "CEO" role, which no member has since ADR 0142 | None of the ten (blocked a workaround) | No |
| 29 Oct 20:49 | The restart tool is offered on read-only tasks, and restarting gluetun alone leaves the containers that share its network (deluge, nzbget, prowlarr) with no network: Bo restarted gluetun over DNS warnings, and downloads were down ~30 min until the coordinator restarted the three at 21:2x | Arm B, task 8 (criterion 4 failed; as task 7's restarts) | The lab outage was restored by hand (an operation, not a platform change); the tool defect is not fixed (freeze) |
| 29 Oct | Harness: the coordinator's notice-clearing job stopped at the first failed desk call, so it cleared nothing all day (91 repeated notices piled up) and task 8 started 8 min late (20:16) | Task 8, both arms (start time only) | Fixed in the coordinator's script (skips a failed card) |

## Question 2: what HQ does that a session cannot

Over the same days, one line per event from HQ's standing work (the lab check and other
schedules): what it found, whether it was real, what it fixed or proposed, and whether the owner
had already noticed. Counted at the end: real problems caught first by HQ, false alarms, and cost.

| When | What HQ found | Real? | Fixed or proposed | Owner noticed first? |
|---|---|---|---|---|
| 28 Oct ~16:00 | Kit (lab check #864): 9 finished downloads stuck in Sonarr (6) and Radarr (3), several named as `.exe` releases, likely fake or malware | Likely | Asked the owner what to do; told to leave them for the owner to remove by hand | To ask |

## Next: what limits HQ's speed (after these results)

Written 28 Oct, before running it. On the Mac's own measurements, the model runs close to what the
hardware allows (reading prompts at 95–108 tokens/s, writing at 11–17 tokens/s), and turns are slow
because of the workload: one call sent 16,362 tokens and took 171 s before any answer began, and a
turn makes several calls. So test the workload before the hardware or the engine.

Same tasks and criteria as above, each configuration in the **same seat** (HQ's bottom rank), so
only the brain or the context changes:

| Configuration | What it tests |
|---|---|
| Qwen3.8-27B on the Mac, as now | Baseline |
| Qwen3.8-27B on the Mac, prompt cut to what the turn needs | Context overhead |
| qwen3.5:9b on the A2000 (downloaded 28 Oct) | A smaller local model |
| DeepSeek Flash through the API | Cloud cost against latency |

Recorded per task: time to first token, total time to ready, number of model calls, accepted or
not, and cost. On the Mac, sample `vm_stat` during a turn to see whether it pages while inferring
(28 Oct at idle: no swap-ins or swap-outs; 3.6 GB of swap held from earlier).

A design idea to test, not build, during the freeze: **context sized to the turn.** A turn that
checks whether tests passed needs the results, the criteria and perhaps the diff, not the task's
whole history. If prompt size dominates as above, this helps local and cloud turns alike without
changing any model. MLX against llama.cpp, and newer hardware, come after, and only if measured.

### Queued: benchmark a machine before it takes a seat (owner, 30 Oct)

Round 1 showed the Mac's rank losing hours: its turns read several 16 KB container logs, the
Mac reads prompts at about 100 tokens a second, and its model calls timed out before answering,
so items sat through three failed turns there before climbing. Nothing measured that before
the Mac was given a rank. The owner's idea, for after the rounds: **a machine is benchmarked
before it is added as a brain**, on the team's own workload rather than a synthetic one.

- **Inputs from the record:** the prompt sizes of recent turns (median and 90th percentile, by
  kind of task), and a fixed set of tool calls whose right answers are known.
- **Measured:** prompt reading and writing speed, time to the first token at those sizes, the
  whole time of a turn of the 90th-percentile size, and tool-call correctness.
- **Admitted only if** a 90th-percentile turn finishes inside the worker's time for a call, with
  room to spare, and the tool calls pass; otherwise it is refused, with the numbers.
- **What it sets when admitted:** its rank by measured quality and speed rather than by name,
  the largest context it is given (logs cut to fit), and its call timeout.
- **Re-run** when the machine, its model or the team's prompt sizes change, so a seat that has
  become too slow is noticed rather than discovered in a failed round.
- **Decided by the owner, 30 Oct: the Mac runs a dense 14B model** (in place of the 27B), the
  benchmark only confirming its context cap and timeout. Switched when the laptop is free.
- **First use: a faster model for the Mac.** Its seat is limited by speed, not ability (a 27B
  dense model at IQ3_S, reading about 100 tokens a second). Candidates to benchmark: a
  mixture-of-experts model of about 30B with about 3B active (near the 27B's quality, computed
  like a 3B); a dense 14B at Q6 to Q8 (a 9B would duplicate the NAS's qwen3.5:9b); the same
  27B under MLX. Not measured yet; the benchmark picks. A settings change on the Mac's model
  server and the node's Mac model name, no HQ code.

#### Running the benchmark: the Mac's 27B against a 14B

`scripts/bench-model.py` (Python standard library only) measures any OpenAI-compatible
server on HQ's workload. For each prompt case (small about 2k tokens, typical about 8k, large
about 16k) it streams three runs, each starting with a fresh line so no prompt cache serves it,
and reports the medians of: time to the first token, reading rate (llama.cpp's own `timings`
when the server sends them, else prompt tokens over the time to the first token), writing
rate, the whole call, and **a turn**: the time to the first token plus the case's `max_tokens`
at the writing rate, so two models that answer at different lengths are compared on the same
work. It then puts ten tool-call cases with known answers (temperature 0) and scores the tool
name and each required argument exactly; a task number must be an integer, as HQ requires.
The key, if the server needs one, is read from `--key-file` and is never printed or saved.

With the laptop free, the model server needing a context of at least 18k tokens for the
large case, and nothing else running on the Mac:

```sh
# 1. The current model, as the Mac serves it now.
python3 scripts/bench-model.py --url http://192.168.1.10:8080/v1 --model <27b-name> \
    --prompts scripts/bench-cases.json --out bench-27b.json [--key-file <path>]
# 2. Load the 14B in the same server, with the same context and settings, and repeat.
python3 scripts/bench-model.py --url http://192.168.1.10:8080/v1 --model <14b-name> \
    --prompts scripts/bench-cases.json --out bench-14b.json [--key-file <path>]
# 3. The decision, by the owner's rule.
python3 scripts/bench-model.py --compare bench-27b.json bench-14b.json
```

The compare prints both models' medians per case and `DECISION: SWITCH to B.` only when the
14B's large turn takes **half the 27B's time or less** and it misses **at most one more** of
the ten tool cases; otherwise `KEEP A`, with the reason. It warns when the two runs used
different case files. Keep the result files out of the repository (they name the server).

**Using real HQ prompts.** The sample cases are fictional lab text sized by estimate; each run
prints the server's real prompt token count, so check the sizes and adjust `target_tokens`.
To measure HQ's own turns instead, save a captured request from a turn of each size (the
`messages` list, and `tools` if you want them counted) as JSON beside a copy of the cases
file, and give that prompt case `"messages_file": "<file>.json"` in place of `messages` and
`fill`. The tool cases can be replaced the same way: each is an `instruction`, and an
`expect` of `{"tool": ..., "args": {...}}` (`"*"` for any non-empty value), or `null` where no
tool should be called. A captured prompt holds the lab's real logs: keep it, and the copy of
the cases file that points at it, outside the repository.

## Decision rule

Written now, before the results:

- **Arm A accepts more tasks for less cost and time:** development work moves to Claude Code
  sessions. HQ keeps only the standing work that question 2 shows is worth its cost.
- **Arm B is close on acceptance and cheaper or needs fewer interventions:** HQ keeps development
  work in its loops.
- **Mixed:** split by task kind, using the table.

**Amended 30 Oct by the owner, after the results.** The rule above read the comparison as a
choice between the two arms. The owner's aim is different: Claude Code is the bar HQ is brought
up to, and this baseline is round 0. So the result (10 of 10 against 1 of 10) does not move
development out of HQ; it sets the gap, and each later round makes **one** change and re-runs the
same ten tasks with the same five fields, so any movement can be put down to that change.

The ladder, in order, each rung chosen from round 0's evidence:

| Round | One change | Round-0 evidence it answers |
|---|---|---|
| 1 | Fix the design defects in the table above | Four tasks never started (a top-rank failure stalled the loop); rank 1 could not run; two read-only tasks restarted the lab |
| 2 | A stronger bottom seat (qwen3.5:9b on the A2000, or DeepSeek Flash) | Most failures began in the first, weakest turn and were not recovered by climbing |
| 3 | Tools fitted to the task | Task 3 lacked a registry lookup; the restart tool was on offer where it should not be |
| 4 | Context sized to the turn | The Mac spends minutes reading prompts before answering (see the section above) |
| 5 | Fewer handoffs | Every climb restarts the work in a new member's turn |

A round is kept when it raises acceptance without raising cost or interventions more than the
gain is worth; otherwise it is reverted and the next rung is tried. Rounds 1 and 2 were made
together on 30 Oct at the owner's request, so round 1's re-run measures both.

## Rounds after the baseline

The bar is round 0's arm A (Claude Code, 10 of 10). Each round re-runs the same ten tasks on HQ
alone, with round 0's exact words, on the fork's `develop` at `ee878f7` (Claude Code's branches,
`exp/a-NN`, were pushed on 30 Oct but are not merged until the rounds are over, so every round
starts from the same code). Judged as round 0 was, each claim re-checked; the same five fields.

### Round 1 (30 Oct): the design defects, and a stronger bottom seat

Live from `c72c3abc` (see ADR 0142's amendment of 30 Oct): rank 1 runs, a top-rank failure lets
its loop go on, only finished turns count, read-only tasks are refused the restart and gluetun is
never restarted alone, a chess move gets three turns, owner's projects need no lead; the house
model is qwen3.5:9b and DeepSeek Flash (Dex) holds rank 4. Rounds 1 and 2 of the ladder are
therefore measured together. Round 0's open items were set aside first (moved to project 6,
"Baseline round 0 (archived)", and dropped, history kept). Started 30 Oct 06:50 EDT as HQ tasks
913 to 922.

| # | HQ task | Accepted | Cost | Elapsed | Interventions | Rework |
|---|---|---|---|---|---|---|
| 1 | 913 | Yes (all 46 defaults with values, ports; pushed as `hq/task-913-readmes`) | $0.03 | 82 min | 0 | |
| 2 | 914 | Yes (all 39 defaults with exact values, directories included; ports; pushed as `hq/task-914-readmes`) | $0.23 | 461 min | 0 | |
| 3 | 915 | | | | | |
| 4 | 916 | | | | | |
| 5 | 917 | | | | | |
| 6 | 918 | | | | | |
| 7 | 919 | Yes (TheRARBG, The Pirate Bay, YTS from both apps; Prowlarr's own HTTP/2 and DNS-SSL failures, not a rate limit; DNS/IPv6 change; nothing changed. Dex sent Rae's first cause back) | $0.07 | 341 min | 1 | |
| 8 | 920 | | | | | |
| 9 | 921 | | | | | |
| 10 | 922 | | | | | |
