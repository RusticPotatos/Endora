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

Since rungs 1 and 2 are measured together, rung 4, **context sized to the turn**, moved up to be
round 2 (30 Oct; ADR 0142's amendment of that date): a house turn starts with HQ's core and what
its kind needs and finds the rest by name, the tools say their rules in fewer words, and a log is
40 lines and 6 KB on the house's models unless the turn asks for more. Measured offline, a chess
move's three calls went from about 9,700 tokens to 6,200.

## Rounds after the baseline

The bar is round 0's arm A (Claude Code, 10 of 10). Each round re-runs the same ten tasks on HQ
alone, with round 0's exact words, on the fork's `develop` at `ee878f7` (Claude Code's branches,
`exp/a-NN`, were pushed on 30 Oct but are not merged until the rounds are over, so every round
starts from the same code). Judged as round 0 was, each claim re-checked; the same five fields.

### Running a round with the script

`scripts/experiment.py` (Python standard library only) does what round 1 did by hand. It is fed
on stdin to the Python inside the HQ container, so nothing is copied in; its arguments follow
the `-`. It reads the store read-only (`HQ_DB`, default `/data/hq.db`) and, for `start` alone,
calls HQ's API (`HQ_URL`, default `http://127.0.0.1:8790`) with the token in `$HQ_TOKEN_FILE`,
which it never prints.

```sh
run() { docker --context nas exec -i endora-hq python3 - "$@" < scripts/experiment.py; }
run start --round 2 --from-tasks 882-890,903        # a dry run: what it would archive and make
run start --round 2 --from-tasks 882-890,903 --yes  # prints the new ids as JSON
run status --tasks 923-932                          # per task, as JSON
run fields --tasks 923-932 --header                 # the round's table, to paste below
run check --task 929                                # the final post's quotes against its receipts
```

- **`start`** (the only writing subcommand, and only with `--yes`) sets the previous round aside:
  every open task bearing one of round 0's titles (or the tasks given with `--previous`), with
  any open task under it, is moved into a new project "Round N-1 (archived)", led by an existing
  member, which is then dropped, history kept. It then makes each round-0 task again with its
  exact title, description and criteria, in its goal and the project it was made in (from its
  `task.created` event), checks each copy against the store, and refuses before writing anything
  if a project is no longer active. A criterion's wait (`after`) cannot be set through the API:
  it is reported, not copied.
- **The fields:** elapsed is from the task's creation to its first "in review" (or "done"), in
  whole minutes, else "not ready"; cost is the `spent` of every turn on the task and the tasks
  under it; interventions are the question cards on them that the owner answered (marked seen,
  with the owner's words in the thread), not those dismissed. Accepted and rework stay blank for
  the judge.
- **`check`** is mechanical and modest: in the task's last member post, every receipt cited
  exists and is the task's, and every quoted line (`>` lines, fenced lines, long `inline`
  spans) is in a cited receipt's stored result. It exits 1 on a problem. Judging stays the
  owner's (or the judge's).

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
| 3 | 915 | No (never ready: no tool could check a tag on its registry, the round-0 blocker; Ada asked the owner five times and made no branch rather than guess) | $0.02 | not ready at close (14.7 h) | 5 | |
| 4 | 916 | No (never started: the loop held one item at a time, and 915 held it) | $0 | not started at close | 0 | |
| 5 | 917 | No (never started, as 916) | $0 | not started at close | 0 | |
| 6 | 918 | No (never started, as 916) | $0 | not started at close | 0 | |
| 7 | 919 | Yes (TheRARBG, The Pirate Bay, YTS from both apps; Prowlarr's own HTTP/2 and DNS-SSL failures, not a rate limit; DNS/IPv6 change; nothing changed. Dex sent Rae's first cause back) | $0.07 | 341 min | 1 | |
| 8 | 920 | No (blocked honestly: the window's logs had rolled over by the time HQ reached it, a day after the session answered; recommended keep; changed nothing on the lab, where round 0 restarted gluetun) | $0.02 | blocked after ~11 h | 1 | |
| 9 | 921 | No (never started, as 916) | $0 | not started at close | 0 | |
| 10 | 922 | No (never started, as 916) | $0 | not started at close | 0 | |

Round 1 was closed on 30 Oct at 21:30 EDT, 14.7 hours in and before its 24-hour cap, at the owner's
request so that round 2 could run overnight. **Accepted 3 of 10** (round 0: 1 of 10). What held it
back, by the evidence:
- **One item at a time.** A loop worked one item, so five tasks never started while 915 waited
  on a tool it did not have (items side by side, on round 2).
- **No registry lookup.** 915 could not check a tag (the `image_tags` contract, on round 2).
- **The Mac's timeouts** on long prompts, until Rae moved to DeepSeek for the evening (context
  sized to the turn, stubs, a turn budget and gists, on round 2).
- **Slowness let evidence expire.** 920's window had rolled out of the logs before HQ reached it.
  Task 8 cannot be met in any later round either: its window is gone. It stays in the set with
  its words unchanged, and is expected to be not accepted.
- **A blocked item asked the owner again** about every half hour (915 five times, 920 four).

### Round 2 (30–31 Oct): context sized to the turn, items side by side, the registry tool

Released 30 Oct 22:43 EDT as `7c195ac6`, at the owner's word ("do whatever is needed to push and
measure the changes against the first round"). It changes several rungs at once, measured
together:
- **Context:** context sized to the turn; tools on demand; old results read as stubs; a turn
  budget with a fresh continuation; gists of long results; a briefing and a review sized to the
  item (ADR 0142's amendments of 30 Oct).
- **Loops:** items side by side (two in hand per loop); a handover at each change of hands; HQ's
  mechanical checks at review.
- **Tools:** the `image_tags` registry tool.
- **Fixes:** the review's three most serious fixes (HQ decides out-of-room; moved and quoted
  owner's files are seen; quotes are checked only against the item's own receipts).

Rae ran on DeepSeek Flash, with the Mac marked busy (the owner's laptop), as at the end of round 1.
Round 1's open items were set aside first (project 7, "Round 1 (archived)", dropped, history
kept). Started 30 Oct 22:50 EDT as HQ tasks 929 to 938 (task 8 is 938).

| # | HQ task | Accepted | Cost | Elapsed | Interventions | Rework |
|---|---|---|---|---|---|---|
| 1 | 929 | Yes (all 46 defaults with exact values, rechecked; ports; Markdown only; pushed as `hq/task-929-readmes`. Bo's three budget-length turns, then Rae, then Ada; reviewed by Kit. One setup nudge after Rae's failed starts, not counted) | $0.26 | 142 min | 0 | |
| 2 | 930 | Yes (all 39 defaults with exact values, rechecked against `defaults/main.yml`; ports; Markdown only; pushed as `hq/task-930-readmes`. Bo, then Rae, then Ada; reviewed by Kit) | $0.22 | 142 min | 0 | |
| 3 | 931 | Yes (all seven `latest` tags pinned, each named in the commit message; each rechecked on its registry; no new ansible-lint or yamllint findings against develop; pushed as `hq/task-931-pin-homepage-stats`. Found `image_tags` once it was in the loop's tools; reviewed by Mara) | $1.12 | 231 min | 1 | |
| 4 | 932 | No (criterion 2: no CI run on the branch. The step was added and runs `tests/test_lint_rules.py` by name; the broken rule is in the history and reverted; rechecked locally, the tests pass at the tip and fail at the broken commit. Pushed as `hq/task-932-ci-lint-rules`, but the workflow runs on push only for main and develop, so no run came, and HQ never asked for one) | $0.98 | 207 min | 0 | |
| 5 | 933 | Yes (the admin password's literal default is gone, read from `vault_minio_admin_password`; an assert stops the play with a clear message when it is empty; the docs say how to set it in the vault; no new lint findings; pushed as `hq/task-933-minio-no-literal-secret`) | $0.25 | 492 min (about four of them waiting on the day's pool) | 0 | |
| 6 | 934 | Yes (both database passwords' literal defaults gone, read from vault variables; an assert stops the play with a clear message when either is empty; the docs say how to set them in the vault; molecule gets marked test-only values; no new lint findings; pushed as `hq/task-934-nextcloud-no-secret-default`. Reviewed by Mara) | $0.78 | 506 min (about four of them waiting on the day's pool) | 0 | |
| 7 | 935 | Yes (TheRARBG and The Pirate Bay, quoted from both apps and rechecked in their logs; the 429s are Prowlarr's own disabling after HTTP/2 connection failures to the upstream, also rechecked, with the DNS link labelled a hypothesis; a gluetun DNS change with its expected effect and a check, and disabling the two indexers in Prowlarr as the fallback; nothing changed by HQ, no restart in its record. Bo, Rae, Ada; reviewed by Kit) | $0.44 | 64 min to first ready, passed review at 08:18 | 2 | |
| 8 | 938 | No (blocked honestly: the window's logs are gone, as expected after round 1; made no lab change) | $0.38 | blocked after ~4 h | 2 | |
| 9 | 936 | No (blocked honestly: the sandbox is offline and `repo_checkout` refuses `main`, so upstream could not be read (D13); nothing pushed or merged) | $0.16 | blocked at 09:31 | 1 | |
| 10 | 937 | Yes (rule `nas-secret-default` in `tests/lint_rules` with fixtures and tests: a literal fails, a vault reference and an empty default pass; rechecked, 12 tests pass; on `warn_list`, so it reports today's 29 offenders as warnings while ansible-lint's failures stay develop's 14; no new yamllint findings; pushed as `hq/task-937-secret-defaults`. Over twelve turns lost to the budget first (D12); finished by Mara) | $3.34 | 651 min (about four of them waiting on the day's pool) | 1 | |

**Round 2's result (31 Oct, 10:00 EDT): 7 of 10 accepted** (round 1: 3; round 0: 1; Claude
Code: 10). 7 interventions, $7.93 on the ten tasks; the owner topped up the day's pool twice
($6.66). The three not accepted: 932 lacked a CI run it could not trigger (D6); 938's window has
left the logs and cannot be met by any later round; 936 could not read upstream (D13). The
paragraphs below were written at 03:30, part-way:

**Where round 2 stood (31 Oct, 03:30 EDT, 4 h 40 min in): 2 accepted, 2 not accepted, 6 open,
and stopped on money.** The day's pool (about $3.33, the $100 a month over 30 days) was spent by
02:44: $2.97 on round 2 (Kit's reviews $1.30, Rae $0.85, Mara's reviews $0.60, Ada $0.22; Bo free)
and $0.19 on league chess. Every open item's next turn waits for the budget, which comes back at
the owner's midnight. Raising it is the owner's decision; it was not raised overnight.

Two setup faults in the release cost the first hours, both fixed during the round and neither
counted as an intervention:
- **Rae's runtime.** The evening's switch to DeepSeek had left Rae's runtime on Claude Code, so
  none of her turns could start from 21:18 until 00:2x. Her items then sat un-woken (a failed
  start does not wake the holder again) until a note on each thread at 00:47.
- **The registry tool.** `image_tags` was released but not added to the ansible-nas loop's tools
  until 02:2x, so 931 at first asked for tags as round 1's 915 had.

**Rounds side by side, on one measure** (1 Nov; round 3's column final at its close, all ten
tasks). **Reported** is what HQ recorded done; **verified** is what the judge found the work
meets, whatever HQ recorded. Rounds 0–2 recorded one "accepted" column; it is re-derived here
from their judged rows. There, every task HQ did not mark done was either never ready, never
started or blocked on an unmet criterion, so verified equals accepted; what differs is that
HQ marked some work done that the judge refused (883 in round 0, 932 in round 2). So round 2's
"7 accepted" is a verified count, set beside round 3's 8 verified, not its 4 reported. The
piping numbers are from HQ's store for each round's task range (rounds 0: 882–890 and 903 with
its one subtask; 1: 913–922; 2: 929–938; 3: 947–955), read-only.

| | Claude Code | Round 0 | Round 1 | Round 2 | Round 3 |
|---|---|---|---|---|---|
| Reported (HQ done) | — | 2 | 3 | 8 | 4 |
| Verified (judge) | 10 | 1 | 3 | 7 | 8 |
| Interventions | 0 | 9 | 7 | 7 | 10 |
| Spend (HQ's runs) | tokens only | $0.81 | $1.67 | $8.35 | $8.68 |
| Turns | 10 sessions | 112 | 92 | 320 | 347 |
| Turns that changed nothing (HQ's own flag) | — | 84 | 65 | 267 | 293 |
| Fork checkouts | 10 | 28 | 50 | 258 | 280 |
| Wakes: continued / recovery / review asked / assigned (refused wakes of any reason) | — | 0 / 373 (335) / 13 / 24 | 0 / 51 (3) / 9 / 21 | 225 / 18 (3) / 25 / 34 | 248 / 42 (23) / 33 / 35 |
| Handovers / climbs one rank up | — | 0 / 14 | 0 / 10 | 249 / 22 | 274 / 24 |
| Questions on the desk | — | 121 | 12 | 10 | 14 |

Where the record does not allow a re-derivation: round 0's arm B judged only what reached it
(five tasks never started) and recorded no verified reading of blocked work, so its 1 is
accepted-as-judged; reviews are not counted before round 3 (the "reviews this" post is new);
round 2's spend here ($8.35) includes runs after its close, beside the $7.93 it recorded.
Corrected on 1 Nov from `experiment.py metrics` (round4 branch), which reproduced these numbers from
the store: round 3's interventions are 8 by its own rows, not 7; rounds 0–2's climbs had counted
"reviews this, one rank up" posts (reviews), the real climbs are 14, 10 and 22; the bracketed
refused count is refused wakes of any reason. What the piping numbers say so far: since round 2, eight or nine of ten turns change nothing
by HQ's own flag, and nearly every turn checks the fork out again (D27): the cost of the
handover-and-continue design, not of the models (the piping audit, round 4's item E).

What round 2 shows already, by the evidence:
- **Side by side works.** Six of ten tasks were worked within four hours; round 1 started five
  in fourteen.
- **The turn budget bites on the NAS's model.** Bo's turns ended at the budget after 7 or 8 calls
  each (about 30 KB a call), and after three such turns the item climbed to Rae. The continuation
  counted against the rank as the fourth-time rule meant, but three budget passes with nothing
  to show is the 9B model's limit, not the budget's.
- **The paid reviews are the cost.** Sonnet and Opus reviews were $1.90 of $2.97, for 15 turns.
- **CI needs the owner's push and a trigger HQ cannot pull.** 932 did everything else; the
  workflow runs on push only for main and develop, so no run came, and HQ never noticed.
- **Questions are still the house model's habit.** Bo put its finished 935 finding on the desk
  as a question twice instead of asking for review.

### Round 3, proposed (31 Oct, for the owner)

Round 2 reached 7 of 10. Of the three left, one can no longer be met by any round, so 10 of 10
needs a decision about task 8 first.

**Built and gated on the local branch `round3-local`** (2,171 unit tests, 110 scenarios):
- D1, D4: a model change keeps a member able to start; a checkout brings `image_tags`.
- D2, D3, D10: what wakes an item (a never-started turn is retried, a blocked item wakes nobody,
  one question per item).
- D9: the day's pool sized from the Claude credit left and the days to its renewal.
- D12, in progress: a long item reads the newest handover and keeps its work across turns.

**For the owner to decide:**
1. **Task 8 (gluetun's restart).** Its window ended 29 Oct and has left the logs, so no round can
   pass it. Approved 31 Oct: a fresh window is already running, since the restart timer is off
   (gluetun, prowlarr and deluge up since 30 Oct 03:09 EDT with no restart). It closes 2 Nov
   03:09 EDT, 72 hours like the first; task 8 then runs in both arms at once with the new dates,
   before the logs roll over.
2. **D7, reviews.** Sonnet and Opus reviews were most of the cost. Proposed: the member one rank
   up reviews, but never above Haiku unless the work came from Haiku or above; HQ's mechanical
   checks carry the rest.
3. **D6, CI.** Proposed: the fork's workflow also runs on `hq/*` branches (a fork change, made
   when the rounds end and develop may move), and until then HQ says when a CI criterion cannot
   be met rather than finishing without it.
4. **D13, upstream.** Proposed: a read-only mirror of upstream ansible-nas on the node that
   `repo_checkout` may check out, so 936 can be done.
5. **The rest of round 2's review fixes, the stable prompt opening with a lessons list, and the
   owner's scenarios for round 2's changes** were left part-done by their agents; they are
   restarted and join `round3-local` before release.

### Round 3 (31 Oct –): the round-2 defects fixed, the long item kept, reviews at Haiku

Released 31 Oct 12:52 EDT as `8f138b2d`. What changed since round 2, each from its evidence:
- **Setup and wakes:** a model change keeps a member able to start (D1); a released tool reaches
  the loops that check out the fork (D4); a never-started turn is retried, a blocked item wakes
  nobody, one question an item (D2, D3, D10).
- **Long items:** a turn reads the newest handover only and keeps its work in progress on a
  branch; running out of room while the work moves is no failure (D12).
- **Reviews and CI:** no review above Haiku's rank (D7); HQ refuses a pass on a CI criterion no
  run can meet (D6).
- **Reading upstream:** a read-only mirror of upstream ansible-nas in every checkout (D13).
- **Money:** the day's pool sized from the Claude credit left and the days to its renewal (D9).
- **Smaller prompts:** a stable opening for the provider's cache, and a lessons list per loop.
- **Delivery:** accepted work is listed, pushed after a code privacy scan, and merged into
  develop only when the owner turns it on after the rounds; no merge or push cards.
- **Console and code:** Activity and Chess tabs, a cost-by-model table, the Mac in Settings,
  "what it was given" on each turn; the one-team clean-up; one consolidation pass.

The setup was checked before the start (the playbook `.claude/skills/hq-round`): every member on
the worker with a model it can run, `image_tags` in the ansible-nas loop's tools, the pool sized
from $67.75 of credit. Started 31 Oct 12:58 EDT as HQ tasks 947 to 955. **Task 8 is not in it
yet**: its window starts when the owner runs the fork's playbook (D11), and it joins then.

**One release mid-round, by the owner's word (31 Oct 17:47 EDT, `9ed87cb2`).** The day's top-ups
had reached the limit of three pool days ($9.49 of $9.50) with four turns queued on live tasks.
The owner raised the limit to five pool days ($15.83): it is there to catch a leak or a
run-away spend, not to hold back credit that expires on the 18th (ADR 0113 rule 6, amended). The
release changed that one number, its refusal's words and their tests; nothing in how HQ works.
No turn was running or cut. Then $3 was topped up (the day's total $12.49), after checking the
queued turns were on live tasks and none on a judged item.

**A second release mid-round, by the owner's word (31 Oct 18:29 EDT, `1340e5b3`): D19.** A
reviewer on Claude never had its verdict tool loaded, so 947's four approvals were refused and
949 waited on the same review; left alone, the round would have measured the bug, not the
team's work. The release changes the worker's opening only (what HQ marks for the turn is
loaded on Claude too). One turn ended before the restart; none was cut.

**The D19 fix held on its first live review (1 Nov 00:42 UTC).** Ada's review of 949 (run 4909)
had `review` among its loaded tools, not held back, and her verdict landed: she sent it back on
criterion 2 (`ansible-lint nas.yml` gives develop's 14 findings, not a pass), with criteria 1 and
3 holding on her own readings. Kit then blocked 949 on a question (card 5055: does "no worse than
develop" meet criterion 2?). Answered at 00:57 UTC with the task's own words and "If your tools
cannot meet these, record that and end it as blocked", as round 2's task 3 was (931, card 5031):
one intervention on task 3.

**Timing.** The 24-hour box ends 1 Nov 16:58 UTC for the nine tasks started at 12:58 EDT; task 8's
window ends 21:00 UTC, so it runs in both arms after that, on the owner's approval of 31 Oct, and
its row is filled when it is judged.

**Task 8's window cut short, by the owner (1 Nov, mid-round).** The owner's word: don't wait for
21:00 UTC; "we don't even know if 24 hours is valuable against a shorter test". The window was
closed at 13:30 UTC, about 16.5 hours from 31 Oct 21:00 UTC, not 24. Task 8 was made at 13:25 UTC
as HQ task 956 from source 909, its words unchanged but for the window ("31 Oct 21:00 UTC to
1 Nov 13:30 UTC"), in 909's project (the gluetun timer, project 4) and goal; Claude Code's arm
started on the same words at 13:3x UTC, a fresh agent in its own worktree of the fork at
ee878f7 (`exp/a-08-r3`, local), as in round 0. **Arm A accepted** (round judge, 13:4x UTC): remove; the four
containers' RestartCount 0 since 31 Oct 21:00:41–21:02:08 UTC, the timer inactive since 27 Oct
00:10 UTC, sonarr's 64 and radarr's 32 RSS syncs through Prowlarr, five sampled points in the live
logs, and one gluetun tunnel outage (08:53–09:03 UTC, inside the NAS's memory stall, recovered
inside the container); the change on `exp/a-08-r3` (ae290f80, one commit off ee878f7) retires
the timer and removes its units and script, a new test fails on develop and passes at the tip
(13 pass), lint identical to develop, nothing personal, no vars or inventory touched; nothing
changed on the lab. 152k tokens, about 7.6 minutes, no intervention. A rule slip, as round 0's
arm A: it read 20 lines of `group_vars/nas.yml` while finding the timer; nothing of it is in the
branch. For round 4's item 8: only the outage needed more than the last few hours (4.5 hours
before the close), and it did not change the verdict; the night's Watchtower run with no recreate
of the stack is the other thing a short afternoon window would not show. **HQ's arm so far:** Bo
(house model) read logs only, recommended remove, and twice asked the owner to approve instead of
making the branch (cards 5064 at 14:15 and 5065 at 14:4x UTC, a second question on one item
despite D10); both answered with the task's own words (two interventions). It climbed to Rae,
whose turns each ran out of room after three to five log reads (D20); at 15:01 she was out for
the day (card 5066: $0.45 spent, the pool $2.46 left) and was given $0.50 within the pool at
15:2x UTC (a money decision, not an intervention). Rae's turns kept running out of room on log reads; at 15:39 it
climbed to Ada (Haiku), whose turns did the same (352 KB over 3 calls; 108 `container_logs`
reads on the task by then), and who was out for the day at $0.08 (card 5067, a share of $0.05):
given $0.50 within the pool ($2.36 left) at 15:4x UTC. Owner's minutes on 956's two answers
(round 4 counts human attention in minutes): about one each, restating the task's words; each
question waited 8 and about 15 minutes for its answer. Ada's turns ran out of room too, and at 15:56 it climbed to Kit
(Sonnet), whose three turns read 330–478 KB of logs each and ended the same way (156
`container_logs` calls on the task by 16:08, $1.22 spent, nothing kept). Kit was out for the day
at $1.80 (card 5068); given $0.50 within the pool ($1.42 left) at 16:0x UTC, under the task's $2
cap, though the spend is a loop of re-reading: each turn reads whole logs and none keeps what it
found. D20 on a read-only task: whole logs read
in each turn, nothing kept between them.

**A lab fault outside the round (1 Nov, from the coordinator).** The NAS stalled from about
08:43 UTC (HQ's status call hung for minutes; 335 MB of 16 GB available): Ollama's llama-server
host prompt cache (llama.cpp's `--cache-ram`, 8192 MiB by default, which Ollama leaves unset)
filled to 8.2 GB, and the kernel's OOM killer killed the runner at 09:07 UTC (the same on
31 Oct 10:40). At the owner's request Ollama's container limit was lowered from 12g to 8g at
about 10:50 UTC, so a repeat stays inside Ollama (the coordinator's note said `docker update`,
no restart; the container's own record shows it recreated at 11:02:10 UTC, RestartCount 0, now
limited to 8 GiB); the full fix
(the cache capped at 1 GB) is on the fork's branch `fix/ollama-prompt-cache`, for after the
round. Not HQ's doing, and not the VPN stack: task 8's "nothing changed on the lab" is judged
on gluetun, prowlarr, deluge and nzbget, unrestarted through it.

**Round 3 is a mixed build, and reports two acceptance rates.** Tasks judged before 22:35 UTC ran
without the D19 fix; the rest with it. And D17 and D19 affect how outcomes are recorded, not
only what the team did. So the round's result gives both: **reported** (what HQ recorded: done
and delivered) and **verified** (what the judge finds the work meets on its own reading, whatever
HQ recorded; 947, blocked by D19 and D17, is checked as well). Round 4's gain is then read
against the verified rate, so fixing the measurement is not counted as better work.

| # | HQ task | Accepted | Cost | Elapsed | Interventions | Rework |
|---|---|---|---|---|---|---|
| 1 | 947 | No: blocked by HQ at 22:38 UTC, "failed at every rank". The work was ready at 19:36 and Ada approved it four times on her own readings (all 46 defaults, ports, lint same as develop), but no verdict could land (D19), and wakes refused while it waited in review counted as Mara's failures (D17). Never delivered. **Verified yes** (round judge, 1 Nov, at head c858650e0b18 from the mirror's `refs/submissions`): all 46 defaults match `defaults/main.yml` value for value with no extra rows; ports 8000, 6789 and 9696 match the tasks; nothing personal in the three READMEs or commit messages; lint identical to develop (14 and 6); one branch off ee878f7 | $0.50 | 158 min to first ready; never accepted | 1 | |
| 2 | 948 | Yes (all 39 defaults with exact values against `defaults/main.yml`; ports and the VPN rule match the tasks; lint the same as develop, 14 and 6; pushed by delivery as `hq/task-948-role-readmes`, its privacy scan clean; rechecked by the round judge) | $0.36 (two thirds after it was ready, see D17) | 32 min | 0 | |
| 3 | 949 | Reported no: blocked by Kit at 00:56 UTC on criterion 2 read literally, never delivered (D22). Verified yes: all seven tags real on their registries (Docker Hub, ghcr) and named in the commit; lint the same as develop (14 and 14, 6 and 6, findings identical); one commit off ee878f7 touching only the two defaults files; nothing personal; rechecked by the round judge from the node mirror's `refs/submissions` | $1.71 (Ada $1.24; 360 `image_tags` calls from re-reading, D20) | 322 min to first ready; never accepted | 1 | |
| 4 | 950 | No, reported and verified: blocked by Bo at 05:07 UTC; his kept heads add only two `.pyc` files (D28); no branch pushed and no CI run (and `ci.yml` runs on a push to main only, D6); no broken rule shown. `ci.yml` already ran `tests/test_lint_rules.py` on develop. Bo checked out the fork six times, then in fresh sandboxes took the empty checkout for "offline" (D27); rechecked by the round judge | $0.00 (house model) | none ready; about 65 min of work (first turn 04:02 after D15's wait) | 2 | |
| 5 | 951 | Reported no: Ada's approvals at 01:26 and 02:10 refused by a gate shut by D23, then blocked by HQ as "failed at every rank" before 03:50 UTC (its card 5058 closed unanswered, not counted); never delivered. Verified yes: the admin password defaults to `vault_minio_admin_password`, the old literal gone from the role and its docs; the judge played the role: unset or empty fails with a clear message, set passes, disabled skips; README and website page say to set it in the vault; no secret in patches or commit messages (the removed line is upstream's public default); lint the same as develop (14 and 14, 6 and 6, identical); 8 commits off ee878f7; rechecked by the round judge from the mirror's `refs/submissions`. Outside the criteria: loki's template reads the same variable, and miniflux keeps the literal | $0.54 (71% after first ready; Mara $0.19 from the D17 climb) | 460 min to first ready; never accepted | 0 | |
| 6 | 952 | Yes, reported and verified: both SQL passwords default to vault variables, no literal left; the assert fails with a clear message when either is unset and passes when set (run by the judge, unset and set); docs say how to set them in the vault; lint the same as develop (14 and 14, 6 and 6, identical); one commit off ee878f7, pushed by delivery as `hq/task-952-nextcloud-secrets`, privacy scan clean; rechecked by the round judge. Ada reviewed work she had written before Kit squashed it (D24) | $0.25 (Kit's squash 37%, after D17 refusals) | 488 min (462 waiting in the loop's queue; about 26 min of work) | 0 | |
| 7 | 953 | Yes, reported and verified (as written): the indexers named (TheRARBG and The Pirate Bay through Prowlarr) with lines from both apps, all in the live logs; the cause shown: the 429 is Prowlarr's own "disabled after failures" reply, each following a failed connection to the indexer's site from inside gluetun's network, beside gluetun's DoH errors; a change with its expected effect and a 24-hour check; nothing changed on the lab by HQ (every receipt a read). The fix is weak: the failures come after names resolve, so switching gluetun's DNS likely leaves the 429s, and DoT is what the role's own comment says it moved off; Kit also said a restart timer is on when it is off. Rechecked by the round judge | $0.99 (78% after first ready: Rae's three reviews with no verdict, D26, then Kit after D17) | 722 min (Bo stopped 18:41–04:00 by D15) | 3 | |
| 8 | 956 (window 31 Oct 21:00 UTC to 1 Nov 13:30 UTC, cut by the owner) | No, reported and verified: past the arm's $2 cap at 16:2x UTC with no branch and nothing kept, after five ranks (Bo, Rae, Ada, Kit, Mara); 52 turns, 5 moving forward, each reading whole logs (190 `container_logs` calls) and running out of room (D20). Bo had recommended remove with log points at 21:01, 03:46 and 10:36, but asked the owner twice rather than make the branch (criterion 3). Nothing changed on the lab: every call a read, the four containers' RestartCount 0. Set aside at 16:3x UTC (project 10, dropped) so it stops spending. Claude Code's arm: accepted (see above) | $2.54 (Kit $1.46, Mara $0.77, Rae $0.21, Ada $0.09, Bo $0) | none ready; about 3 h of turns | 2 (about a minute each) | |
| 9 | 954 | Yes, reported and verified: the fork holds all of upstream's main (merge-base is upstream's head 36e2338; 137 ahead, 0 behind; GitHub's compare agrees; no upstream branch or merged PR since), so the list is rightly empty and no merge order is needed, as Claude Code's arm found; nothing pushed, develop still ee878f7; rechecked by the round judge. Rae ran out of room 12 times on two commands before Ada answered in one turn (D20); the counts rest on unreceipted shell readings | $0.18 ($0.16 of it Rae's 12 cut turns) | 512 min (not picked up until 01:07 UTC) | 0 | |
| 10 | 955 | Reported no: blocked by Kit at 04:10 UTC on criterion 2, since no CI run can be had on the branch (D25); before that its gate was shut (D23) and Rae's review gave no verdict (D26); never delivered. Verified yes at its final head a41a314f2e58: the rule's 11 tests pass as CI runs them; the judge's own literals fail and vault, empty and `!vault` values pass, and a broken rule fails 2 tests; today's 30 offenders are warnings, guarded by a test, and the 14 lint failures are the same as develop; yamllint identical to develop (the fixture's missing `---` fixed); one commit off ee878f7; rechecked twice by the round judge from the mirror's `refs/submissions`. New literals too are only warnings | $1.60 (Kit $1.41 after first ready, the `---` the only change kept) | 548 min to first ready (about 36 min of work) | 1 | |

### Round 3's result (closed 1 Nov, 16:4x UTC)

**Reported 4 of 10, verified 8 of 10** (round 2: 7 verified; Claude Code: 10, task 8 included,
run today on the same words). Reported: 948, 952, 953, 954. Verified as well: 947, 949, 951 and
955, done to their criteria and lost by HQ's own process. Not accepted in either reading: 950
and 956 (task 8). 10 interventions (four tasks' questions answered with their own words; the
minutes counted from 956 on, about one each). $8.68 on the ten tasks; the day's top-ups were
$12.49 on 31 Oct and none on 1 Nov, where four members' days were given within the pool
($2.00, then $0.50 three times).

**What held it back, by the evidence:**
- **The machinery lost four finished tasks** (947, 949, 951, 955): a verdict that could not land
  (D19), refused wakes counted as failures (D17), a literal reading of a lint criterion HQ's own
  check had measured as met (D22), a gate shut by keeping the head that passed (D23), a CI
  criterion read as needing a run (D25), reviews that gave no verdict (D26). Reported and
  verified differ by exactly these.
- **Two tasks the team did not do.** 950: each turn's empty sandbox read as "offline" (D27), and
  the branch carried only `.pyc` files (D28); its CI criterion cannot be met while the fork's
  workflow runs on main only (D6). 956: every turn at five ranks read whole logs and ran out of
  room, nothing kept between turns (D20), until the $2 cap.
- **The piping.** 293 of 347 turns changed nothing by HQ's own flag; 280 fork checkouts; 274
  handovers. Waiting outweighed working: 952 waited 462 of its 488 minutes in the loop's queue,
  Bo was stopped nine hours by the turn cap (D15), and Ada stalled with money in the pool (D21).

**Cost by model** (HQ's runs on 947–956):

| Model | Turns | Spend | Share | Per turn |
|---|---|---|---|---|
| Claude Sonnet | 55 | $5.30 | 61% | $0.096 |
| DeepSeek Flash | 134 | $1.49 | 17% | $0.011 |
| Claude Opus | 7 | $1.24 | 14% | $0.178 |
| Claude Haiku | 107 | $0.65 | 7% | $0.006 |
| The house model | 44 | $0 | 0% | $0 |

The owner's word was that most money should go to Haiku; three fifths went to Sonnet, reached by
climbs (D17's refused wakes, D20's runs out of room), not chosen for the work.

**The code.** 101,368 product lines (`scripts/code-size.sh`; 32,338 of tests; 41 scenario
features). The round's own code, `8f138b2d..1340e5b3` (12 commits, 52 lines added in Rust), was
reviewed by `duplication-reviewer`; nothing skips a safety check, and five findings are for the
round-4 consolidation, in order of what each removes: the worker decides "loaded up front" two
ways (`up_front()` and `CLAUDE_LOADS`, the mark checked twice in `app/worker/src/turn.rs`), which
also takes `chess`'s second record; a scenario step converts dollars by hand beside `micros()`
(three copies); the top-up limit's "five" is written as a number and a word (`money.rs`,
`work.rs`); the `endora/up_front` mark is defined in the worker and in HQ.

### Round 4, as decided (owner, 1 Nov): a product decision, not a benchmark

**Supersedes the plan below**, which is kept as history. After an outside review of the cheat
sheet and field notes ("you have not yet established that the orchestration system is worth
operating"), the owner reshaped round 4 into one decision: does HQ deserve more of the owner's
time? No additional tooling, no scope expansion, no architectural rescue, and no round 5 to
rescue it.

**The finding before it runs.** HQ's sandbox has Python, git and yamllint only (no `cargo`,
`node` or Swift; `ansible-lint` fails in it). HQ cannot do the main development work for Endora
(Rust, JS) or the iOS apps. That is a product limitation, recorded as such, and left as it is for
round 4 (option A): a good round 4 can justify HQ only within what it can do, such as unattended
infrastructure work, never a wider development harness.

**Two arms, the same words.** Claude Code as the single-agent baseline; HQ as one member alone
(`solo`, ADR 0142 amended), with its task's sandbox kept between turns, its deterministic checks
and its safety rules. No team arm, reviewers, new guards or orchestration. The same pinned base
commit for both; local branches only; neither arm sees the other's work.

**The tasks: real ones only, up to five, not a quota.** Fewer than five means limited coverage,
reported as such; it may support narrowing HQ, never a claim that it is generally competitive.

| # | Task | Acceptance criteria (written before either arm runs) |
|---|---|---|
| R1 | The miniflux role ships a literal default password (found by the round judge on 951, 1 Nov) | The literal default is identified; the insecure fallback is removed using the fork's established credential approach (a vault variable, as 951 and 952 did); the relevant configuration is verified without exposing any secret; a branch off `develop` at `ee878f7`, nothing pushed |
| R2 | Sonarr writes Prowlarr's API key into its logs in plain text (found 1 Nov) | The real source of the plaintext logging is identified with cited evidence, and a narrowly scoped remediation proposed. Read-only throughout: no restart, no configuration change, no reproduction that exposes the key further |
| R3 | The `jellyfin` and `overseerr` roles run the `latest` image tag (a gap in the fork, F1 of the earlier plan) | Each pinned to a release that exists on its registry, named in the commit; `yamllint .` no worse than `develop`; a branch off `develop` at `ee878f7`, nothing pushed |

Not used, and why: F2 needs molecule, which the sandbox lacks; F3 (the NAS's memory) was largely
answered on 1 Nov (the Ollama cache), so its answer is known; F4 is not the owner's work.

**Measured, from the existing records and one comparison table.** Verified (the round judge,
against the criteria above); elapsed time; cost (HQ hard-capped at $15 for the round; Claude
Code's tokens recorded); interventions as a **count and as minutes of human attention** (each
logged with the minutes spent reading and answering it); rework found seven days later.

**The decision rule, set before running.** Continue HQ, within what it showed it can do, only if
all four hold:
1. HQ verifies at least Claude Code's verified count minus one;
2. HQ takes no more minutes of human attention than Claude Code;
3. HQ finishes within twice Claude Code's elapsed time, or shows a concrete unattended-work
   advantage;
4. no unresolved security or correctness regression.

A failure with no intervention is not autonomy. Otherwise narrow HQ to unattended
infrastructure operations, or retire its development side. With two to five varied tasks this is
a decision test, not a statistical benchmark: passing it shows no general superiority.

**Engineering effort.** Already authorised and kept: the task sandbox kept between turns, and no
protocol moves (D30). The metrics and replay scripts stop at their first usable point. During the
round: blockers only, each logged with the run it affected.

### Round 4 (1 Nov, from 16:45 UTC): the run

**Build** `round4` (81f49915), released before the round. **Started 16:45 UTC** with
`experiment.py start --round 4 --from-tasks 957-959 --solo Sol --yes` after a clean dry run.

**HQ's arm.** Sol (member 10, `claude-haiku-5-5`, developer, worker runtime) hired through
`POST /v1/members` so the team keeps Ada. The words of R1–R3 were made as tasks 957–959 in a
project paused at once and then dropped (project 11, never worked), and copied by the start into
project 12, "Round 4 solo (Sol)": tasks **960 (R1), 961 (R2), 962 (R3)**, no mismatches. Cap
$15 on project 12 (`POST /v1/projects/12/cap`, ADR 0113); `GET /v1/round` holds 960–962,
`running: true`. Each task's description is the fork's rules line plus "Work on the ansible-nas
fork's develop at ee878f7"; its criteria are the table's, a line each, none live. Sol works the
loop's items one at a time, so HQ's elapsed times run in sequence.

**Claude Code's arm.** One fresh general-purpose agent per task, started 16:46 UTC, in local
branches `exp/a-r4-1`…`3` of the fork cut from `ee878f7` (worktrees beside the fork), nothing
pushed, told the fork's rules (no `group_vars`, `host_vars`, inventories or vaults; no fetch; no
other branch or worktree, `hq/*` included). R2 strictly read-only on the lab, with a redaction
filter for its own output.

**Disclosed before judging: where the arms' words differ.** Both got the same title, description
and criteria. Claude Code's briefs added, beyond the fork's rules: for R1, "look at how other
roles default secrets to `vault_*` variables and assert they are set" (it cannot know what "as
951 and 952 did" points to; HQ's arm can, from its own store); for R3, "verify each tag on its
registry; a current stable release"; for R2, a redaction filter and, as an example of the shape
of an answer, "e.g. URL query-string logging, log level". The last is a hint HQ's arm did not
get, and is weighed against Claude Code's R2 when judged.

**Known deviation: "nothing pushed".** HQ pushes an accepted branch to the fork as `hq/task-N`
after its privacy scan, by the owner's rule of 31 Oct (ADR 0115, "the push is HQ's, by rule");
there is no switch to hold it and building one is out of scope. Judged as: the arm itself pushed
nothing to `develop` or elsewhere; a delivery push is noted, not failed.

Elapsed is recorded two ways, since Sol works one item at a time while Claude Code ran three
agents at once: **first turn to done** (the like-for-like figure, used by the decision rule's
part 3) and **wall clock from 16:45**, the queue included.

| # | Arm | Task | Verified | Elapsed: first turn to done | Elapsed: wall clock | Cost | Interventions (count, minutes) | Rework (8 Nov) |
|---|---|---|---|---|---|---|---|---|
| R1 | HQ | 960 | **No** (round judge, re-checked against 951's, 952's and Claude Code's standard: criterion 2 not met. Unset, the role deploys `POSTGRES_PASSWORD: ""` and failed=0, and the pinned `postgres:11.1` entrypoint then writes `host all all all trust`: a database open without a password, an insecure fallback, unresolved on the delivered branch. First reading, at 217446b from the public fork: the literal at all four places identified (post 8193) and none left in the role or docs, the Postgres password included; `vault_miniflux_*` defaults; but a `debug` reminder in cloudflared's style, not 952's assert: the judge played the role offline and an unset value deploys empty passwords with failed=0; yamllint 6/6 identical, ansible-lint on the role 3/3 identical; two commits off ee878f7, the first adding a `.pyc` the second removes (left in public history); develop untouched; delivered by HQ. **Rule breach, recorded as such whatever the folder held** (disclosed by Sol, post 8191): read `tests/inventories/integration_testing/group_vars`; HQ's scope check sees changed files only. Criteria 1, 3 and 4 met. Existing installs keep the old database password, not documented) | 8 min | 8 min | $0.014 | 0 | |
| R1 | Claude Code | `exp/a-r4-1` | **Yes** (round judge: the literal in the defaults, the tasks' `POSTGRES_PASSWORD` and `DATABASE_URL`, and the docs; none left in the role or docs; `vault_miniflux_*` defaults; the judge played the assert offline: unset or empty fails with a clear message, set passes, a special-character password encodes rightly; yamllint 6/6 and ansible-lint 14/14 identical; one commit `53783e00` off ee878f7, not on the remote. Owner notes: the molecule scenario now stops at the assert, as librechat's already does; an existing install keeps its old database password) | 3 min | 4 min | 83.6k tokens | 0 | |
| R2 | HQ | 961 | **No** (round judge: criterion 3 met, every receipt a read and the four containers unrestarted; 1 and 2 not met. Sol found the leaking `HttpClient` 429 lines (r3594) but said the key is also in Sonarr's own log file, which holds 0 raw keys (244 and 266 `(removed)`), and missed why only the console is plaintext (Sonarr 4.0.20 lacks the cleansing console layout Radarr and Prowlarr have, whose consoles scrub the same 429s); its fix, a promtail redaction plus rotation, stops one copy reaching Loki and leaves the source; "no role-level switch" rests on not looking (`ConsoleLevel`, `ConsoleFormat` in the DLL); its repository claims are true but cite no read receipt) | 2 min | 10 min | $0.006 | 0 | |
| R2 | Claude Code | `exp/a-r4-2` (report) | **Yes** (round judge: 29 unscrubbed `HttpClient` 429 lines in Sonarr's console, 0 raw keys in its file logs, rotated files and `logs.db`; `CleansingConsoleLogLayout` 0 in Sonarr's DLL, 1 in Radarr's and Prowlarr's, whose consoles scrub the same 429s; not the log level; `SONARR__LOG__CONSOLEFORMAT` in the DLL makes the proposed variable plausible, untested as said; sonarr, radarr, prowlarr and promtail started before the round with 0 restarts and no container events but execs. Goes well past the brief's hint. Open for the owner: the key is still leaking until fixed, then rotate) | 3 min | 4 min | 86.4k tokens | 0 | |
| R3 | HQ | 962 | **No** (round judge, criteria 1–3 met but not verified under the decision rule's part 4: the jellyfin pin is two majors behind develop's effective `latest` (12.4), and Jellyfin's 12.0 and 10.11.0 release posts say a backup is the only way back, so any default install since September would break on deploying it; unresolved on the delivered branch, no warning in commit or thread; Sol's own receipt r3603 listed 12.4 as newest. First reading, at a555cb8 from the public fork: one commit, two one-line defaults changes, nothing stray; `linuxserver/jellyfin:10.11.11` (stable, pushed 2026-09-01) and `sctx/overseerr:1.35.0` (= `latest`) exist, named in the commit; yamllint 6/6 and ansible-lint 14/14 identical; develop untouched; delivered by HQ. Note: jellyfin's pin is two majors behind linuxserver's `12.4ubu2604-ls57`, since `image_tags` with `semver_only` hides linuxserver's versioned tags and Sol did not explain the choice; jellyfin does not run on the lab, so no downgrade today) | 4 min | 14 min | $0.008 | 0 | |
| R3 | Claude Code | `exp/a-r4-3` | **Yes** (round judge: one commit `2aea09fa`, two one-line defaults changes; both tags active on Docker Hub, stable, digests equal to today's `latest`, named in the commit; yamllint 7 and 7 identical as the judge ran it, ansible-lint identical; merge-base ee878f7; not on the remote) | 1 min (53 s) | 2 min | 67.5k tokens | 0 | |

**Log.**
- 16:45 UTC: Sol's first turn on 960; 961 and 962 waiting in the loop.
- 16:47 UTC: Claude Code's R3 reported done in 53 s (67.5k tokens, 9 tool calls): commit
  `2aea09fa`, jellyfin `linuxserver/jellyfin:12.4ubu2604-ls57`, overseerr `sctx/overseerr:1.35.0`,
  yamllint 6 and 6, identical. Sent to the judge.
- 16:49 UTC: Claude Code's R1 reported done in 3 min 11 s (83.6k tokens, 26 tool calls): commit
  `53783e00`; admin and database passwords from `vault_miniflux_*` with an assert; the literal
  also in the tasks (Postgres and `DATABASE_URL`) removed; yamllint 6/6 and ansible-lint 14/14
  identical. Its own notes: existing installs must set the database password to the current one,
  and the molecule scenario will stop at the new assert. Sent to the judge.
- 16:49 UTC: Claude Code's R2 reported done in 3 min 9 s (86.4k tokens, 13 tool calls): the
  key reaches Sonarr's console (stdout, so `docker logs` and very likely Loki) in `HttpClient`
  warnings that print the whole Torznab URL on Prowlarr's 429s; Sonarr's file and database logs
  are scrubbed; Sonarr 4.0.20's console layout does not cleanse where Radarr's does. Proposed:
  Sonarr's console level to Error in its role, then an upgrade to a cleansing build, then rotate
  the key. Sent to the judge.
- 16:53 UTC: HQ's R1 (960) done after 2 turns, $0.014; delivered by HQ as
  `hq/task-960-miniflux-vault-password` (217446b3cd0e, 4 files, +19 −5), privacy scan clean
  (desk 5069, a notice). Desk 5070 (`needs_owner`): set `vault_miniflux_admin_password` and
  `vault_miniflux_db_password` before deploying; left for the owner, not a question, not counted.
  Sol moved to 961. Sent to the judge.
- 16:59 UTC: the judge first accepted 960; on the coordinator's word that criterion 2 is held to
  the standard used for 951, 952 and Claude Code's R1, re-checked and changed it to **not
  accepted** (empty-password fallback; the old `postgres:11.1` trusts every connection then).
- 16:55 UTC: HQ's R2 (961) done in one turn, $0.006, report in post 8202 (no key-shaped string in
  any of Sol's posts, checked by count). 16:59 UTC: HQ's R3 (962) done in 2 turns, $0.008,
  jellyfin `10.11.11`, overseerr `1.35.0`, check c64 no new findings; delivered as
  `hq/task-962-pin-jellyfin-overseerr` (a555cb86e98e), privacy scan clean. `GET /v1/round`:
  `running: false`, the round over on its own. Both sent to the judge. No question reached the
  desk in either arm; no money approved; no blocker fixed.
- 17:00 UTC: 962 verified by the judge on the criteria as written. On the coordinator's word,
  sent back on part 4 of the decision rule: pinning jellyfin to 10.11.11 when the role's default
  `latest` is 12.4 is a two-major downgrade for any default install, and Jellyfin's migrations do
  not run backwards (the lab has no jellyfin container, so nothing breaks here today). **Cause, a
  round finding and not a fix to build now:** HQ's `image_tags` lookup with `semver_only` hid
  linuxserver's versioned `12.xubu2604-lsNN` tags, so it showed 10.11.11 as the newest.
- 17:04 UTC: the judge changed 962 to **not verified** under part 4 (open correctness
  regression), and held Claude Code's R3 verified under the same reading: both its pins equal
  today's `latest` digests (its first judge checked them on Docker Hub), so no version moves.

### Round 4's result (closed 1 Nov, 17:05 UTC): the decision

| Measure | HQ (Sol alone, Haiku) | Claude Code (one agent a task) |
|---|---|---|
| Verified (round judge, criteria written before the run) | **0 of 3** (R1 not met; R2 not met; R3 met as written, not verified under part 4) | **3 of 3** |
| Reported done | 3 of 3 | 3 of 3 |
| Elapsed, first turn to done (sum; used by part 3) | 14.0 min (8.0 + 2.3 + 3.7) | 7.2 min (3.2 + 3.2 + 0.9) |
| Elapsed, wall clock from 16:45 (queue included) | 14 min (one item at a time) | 4 min (three agents at once) |
| Cost | $0.028 (`experiment.py metrics`: 5 turns, 0 that changed nothing, 794k tokens in, 713k of them cached, 22k out) | 237.5k tokens (83.6k + 86.4k + 67.5k), judges excluded |
| Interventions: count, minutes of attention | 0, 0 min (no desk question; one `needs_owner` notice, 5070, to clear before any deploy) | 0, 0 min |
| Security or correctness regression left open | **Two**, both on branches HQ pushed to the public fork: R1 deploys an empty Postgres password that the pinned `postgres:11.1` turns into trust-all; R3 downgrades jellyfin two majors. And one rule breach: R1 read a test inventory's `group_vars` (disclosed by Sol) | None found |
| Rework | checked on 8 Nov | checked on 8 Nov |

**The decision rule, applied.**
1. HQ verifies at least Claude Code's count minus one (2): **fails**, 0 against 3.
2. HQ takes no more minutes of human attention: **holds**, 0 and 0.
3. HQ within twice Claude Code's elapsed time: **holds narrowly** on first turn to done (14.0 against
   a limit of 14.4 min); it would fail on wall clock (14 against 4). No unattended-work advantage
   was shown: both arms ran without attention, and Claude Code's did so correctly.
4. No unresolved security or correctness regression: **fails**, two open, on public branches.

**Result: retire HQ's development side.** Two of the four parts fail, so HQ does not continue as
it is. Of the two other outcomes, narrowing to unattended infrastructure operations is not
supported either: the round's one operations-shaped task (R2, read-only diagnosis of the lab) also
failed, on finding the real source. HQ was fast and cheap here (5 turns, none wasted, $0.03,
after round 3's 293 of 347 turns that changed nothing), so the solo arm fixed the waste; what
stayed short is the judgement in the work: a weaker guard than the fork's own pattern, a
downgrade its own receipt showed, a cause asserted without reading. Coverage is three tasks: a
decision test, not a benchmark, and it shows no general claim either way.

**What the owner may want to act on, apart from the decision.**
- Prowlarr's API key is still being written to Sonarr's console log (29 lines by 16:41 UTC), so
  `docker logs` and very likely Loki hold it. Claude Code's R2 has the narrow fix (Sonarr's console
  level in its role, then a cleansing build); rotate the key after the leak stops.
- HQ's two pushed branches, `hq/task-960-miniflux-vault-password` and
  `hq/task-962-pin-jellyfin-overseerr`, should not be merged as they are (the first also carries
  a `.pyc` in its history). Claude Code's branches `exp/a-r4-1` and `exp/a-r4-3` are local.
- Round findings, not fixes to build: HQ's scope check sees changed files only, so a forbidden
  read passes; `image_tags` with `semver_only` hides linuxserver's versioned tags; a solo item is
  marked done on mechanical checks with nobody reading the work, and R2's repository claims cited
  no read receipt.

### The experiment as a whole (1 Nov, after round 4): full analysis

Five rounds in five days (28 Oct – 1 Nov), from HQ's own store, the rows above and the
repository's history. Read-only throughout; nothing here reopens round 4's decision.

| | Claude Code | Round 0 | Round 1 | Round 2 | Round 3 | Round 4 (fresh, solo) |
|---|---|---|---|---|---|---|
| Tasks | 10 + 3 | 10 | 10 | 10 | 10 | 3 |
| Verified | 10 of 10, 3 of 3 | 1 | 3 | 7 | 8 | **0** |
| Interventions | 0 | 9 | 7 | 7 | 10 | 0 |
| Spend on the tasks | tokens | $0.81 | $1.67 | $8.35 | $8.68 | $0.03 |
| Turns (that changed nothing) | one agent a task | 112 (84) | 92 (65) | 320 (267) | 347 (293) | 5 (0) |
| Verified per dollar | — | 1.2 | 1.8 | 0.8 | 0.9 | 0 |

**1. The gains did not carry over.** Rounds 1–3 rose from 1 to 8 verified on the *same* ten
tasks, each round fixing what the last one failed on: a registry tool for task 3, budgets and
kept work for the long tasks, reviews capped for cost. On three tasks HQ had never seen, with
the piping removed, it verified none. What the rounds improved was HQ's fit to the ten tasks and
to its own process, not the judgement in the work. The playbook named this risk ("a gain that
does not show outside the homelab did not carry over"); round 4 is where it showed.

**2. The piping was the cost, and removing it fixed the cost only.** In rounds 2 and 3, 83–84%
of turns changed nothing by HQ's own flag; handovers (249, 274) and climbs (22, 24) re-read the
work each time, and three fifths of round 3's money reached Sonnet through climbs, not choice.
The solo arm cut that to 5 turns, none wasted, $0.03 for three tasks. So the company structure
(ranks, reviews, handovers, the desk) cost money and hours and did not buy quality: round 3's
reviews passed work that still had to be re-verified by an outside judge, and lost four finished
tasks of their own (D19, D22, D23, D25).

**3. A confound to keep in view: the models differed.** Claude Code's arm ran as general-purpose
agents of an Opus 5.5 session (the driver's model, which subagents inherit); HQ's arm was Sol on
Haiku 5.5, by the owner's money-to-Haiku word. Round 4's failures are all judgement (a weaker
guard than the fork's own assert, a downgrade its own receipt showed, a cause asserted without
reading), which is where model strength tells most. This does not change the decision (the rule
was set on HQ as it would be run, and no round 5 rescues it), but it means round 4 did not
measure HQ's *harness* against Claude Code's at equal models. Any later comparison of runtimes
must hold the model fixed. Claude Code's 237.5k tokens are not priced here.

**4. What HQ's design still gets right, by the record.** No arm of HQ broke the lab after round 0
(read-only tasks refused restarts; every receipt in rounds 3–4 a read); every delivered branch
passed a privacy scan; the kept sandbox and D30 made a solo turn efficient. Its weak points were
also mechanical, and they are findings for whatever comes next, not fixes to build: the scope
check sees changed files only, so a forbidden *read* passes (R1); `image_tags` with `semver_only`
hides versioned tags (R3); a solo item is closed on mechanical checks, with nobody reading the
work (R1–R3); claims about repositories cite no read receipt (R2).

**5. What it cost to build.** Since 28 Oct: 274 commits on this branch, 51 of them touching ADRs;
the product grew to 103,464 lines (round 4's gate; 33,837 of tests), against the owner's figure
of about 62k for team, node and HQ before the rounds. Task spend across all five rounds was
$19.54; HQ's store holds $44.25 of runs in all. The cost that matters was the owner's and the
agents' time on the machinery, which no round's task spend shows.

**6. Where that leaves Endora.** By the rule: HQ's development side retires. Its unattended side
was not shown to have an advantage either (R2, the only operations task, failed), so it stays as
it is, without new investment, and is judged by whether the owner uses it. HQ is idle now: empty
wakes every half hour or so at about $0.0006 each. What the parked proposal below calls Endora's
distinct value (the house and lab integrations, local models, one spending limit, the
private-data rules) was not what failed; the coordination and the judgement in the work were.
The parked proposal's step 3 (a bounded proof of concept, an existing agent runtime with
Endora's tools against the extracted worker, on R1–R3 at a fixed effort budget) is now the next
step in its own sequence, only if the owner wants to put more time into Endora; it must hold the
model fixed (point 3). Its stop condition stands: if an existing runtime meets the need, no
engine is built.

**Open for the owner** (none blocks anything): whether to start that proof of concept or put
Endora's development down; the Sonarr leak fix (a local fork branch,
`fix/sonarr-console-key-leak`, ready to apply), then rotating Prowlarr's key; not merging HQ's
`hq/task-960-…` and `hq/task-962-…`; the rework check on 8 Nov.

### Why HQ failed round 4: the same model outside HQ (1 Nov, a diagnostic, not a round)

Asked for by the owner to learn from the failure, not to rescue HQ; the decision stands. To
separate the model from the harness (point 3 above), R1–R3 were run a third time by Claude Code
on **Haiku 5.5, Sol's model**, given **exactly HQ's words** (title, description, criteria) and no
brief of its own beyond where to work, the read-only lab access HQ's tools also allow, and a key
redaction filter. Branches `exp/h-r4-1` and `exp/h-r4-3` of the fork (local), and a report for
R2. Judged by a fresh round judge on the standard the round applied to both other arms.

| # | HQ, Haiku (Sol) | Claude Code, Haiku | Claude Code, Opus |
|---|---|---|---|
| R1 | No: unset deploys an empty password | **Yes**: an assert fails unset, empty, or half set; lint identical (judge played it). Outside the criteria: the database password is not URL-encoded | Yes |
| R2 | No: wrong about the file log; no why; a fix that leaves the source | No: right about where (35 raw-key lines in the console, 0 in every file log), but no why, and "no fix needed in the role" without looking | Yes |
| R3 | No: jellyfin two majors back | **Yes**: `12.4ubu2604-ls57` and `1.35.0`, both digests equal to today's `latest` | Yes |
| Verified | 0 of 3 | **2 of 3** | 3 of 3 |
| Tool calls, time | 5 turns (the first cut at 12 rounds), 14 min | 32 + 17 + 14 calls, 4 + 3 + 1 min | 26 + 13 + 9 calls, 3 + 3 + 1 min |

**What it shows.** With the same model, Claude Code's harness verified two tasks HQ's did not,
and would have met the decision rule's part 1 (Claude Code's count minus one). So most of round
4's gap was HQ's harness, and some of it the model: R2's depth (finding *why* only the console
leaks, and the setting that stops it) needed the larger model in both harnesses.

**Where HQ's harness lost each task, by its own record** (HQ's runs 5120–5124, posts 8190–8215,
receipts 3591–3607):
- **R1, a turn budget and no test of the failure case.** Sol's first turn ended at its 12-call
  budget mid-edit; the work went on in a second turn from a summary. It copied cloudflared's
  "print a reminder" pattern, not 951's and 952's assert, and its own check measured "unset, both
  are empty" and moved on: nothing in the loop asks a worker to run the criterion's failure case,
  and the item closed on lint and syntax alone. Claude Code on Haiku spent 32 calls in one sitting
  and played the assert unset and set.
- **R2, the tools' reach.** Sol had `container_logs` (60 lines, already redacted) and
  `homelab_status`; it could not look inside a container, so it could not read Sonarr's file logs
  or compare Radarr's, and it filled the gap with a claim (the file log leaks too). Claude Code on
  Haiku used read-only `docker exec` and got the where right. Neither Haiku found the why.
- **R3, a tool's filter.** Sol asked `image_tags` for plain version numbers only, which hid
  linuxserver's `12.x` tags; its next call listed `12.4ubu2604-ls57` first, and it pinned
  `10.11.11` anyway. Claude Code on Haiku read Docker Hub's tag list whole with `curl` and chose
  the current release.
- **Common to all three:** a solo item is done when HQ's mechanical checks pass, and none of them
  tests what the criterion is about. The same model, left to check its own work in a plain loop,
  checked the failure case on R1 and the newest release on R3.

**What carries forward** (findings, not fixes to build now): give an agent the whole working
session a task needs rather than a call budget split across turns; give read-only tools their
full reach (inside the container, the registry unfiltered) rather than a pre-chewed slice;
make "done" depend on a check of the criterion's failure case; compare runtimes on one model.
These are what an existing agent runtime already gives, which is the parked proposal's question.

### Runtime proof of concept (1 Nov, 21:30–22:00 UTC): HQ's worker against Claude Code, one model

Step 3 of the parked proposal below, started on the owner's word the same evening. The bridge it
needed already exists: ADR 0117 built Claude Code as a member's runtime, with `endora-worker
--mcp` offering HQ's tools and the node's contracts, receipts posted by a hook, and the relay
in between; it had run one turn since 19 Oct. So the cleanest comparison holds everything else
still: **the same member (Sol), model (Haiku 5.5), words (R1–R3), tools, loop, sandbox and
checks, and only the agent loop changed.** Two solo runs with `experiment.py start --solo Sol`:
labels 401 (worker, tasks 963–965, project 13) and 402 (Sol's runtime set to `claude-code`,
tasks 966–968, project 14), each capped at $3; Sol put back on the worker after. In each arm
the first turn was killed on the Pi (`docker kill`, exit 137) about a minute in, to test
recovery. Both judged by a fresh round judge on round 4's standard.

| | Worker, round 4 (960–962) | Worker, again (963–965) | Claude Code runtime (966–968) | Claude Code alone, Haiku | Claude Code alone, Opus |
|---|---|---|---|---|---|
| R1 | No | **Yes** (assert) | **Yes** (assert; ran the unset case itself) | Yes | Yes |
| R2 | No | No (no where-not, no why; promtail fix) | No (says the file logs leak; no why; "no role setting" unlooked) | No | Yes |
| R3 | No (10.11.11) | No (10.11.11 again, having seen 12.x) | **Yes** (`12.4ubu2604-ls57`, = `latest`) | Yes | Yes |
| Verified | 0 of 3 | 1 of 3 | **2 of 3** | 2 of 3 | 3 of 3 |
| Cost | $0.028 | $0.030 | $0.054 | tokens | tokens |
| Per verified task | — | $0.030 | **$0.027** | | |
| First turn to done | 14 min | 13 min | 12 min (with ~1 min lost to the kill) | 8 min | 7 min |

**The proposal's five acceptance points, per arm.**

| Point | Worker | Claude Code runtime |
|---|---|---|
| 1. A real task from a clean install | Not tested from clean: both ran on the installed build 81f49915 | The same; but the runtime needed no change, after 13 days unused |
| 2. Workspace kept across an interruption | Yes: the kept sandbox; the next turn removed a stray `__pycache__` before its one clean commit | Yes: the next turn found itself on its branch, made at the kill's second; no second checkout |
| 3. Result verified independently | 1 of 3 | 2 of 3 |
| 4. Receipts show what it ran | No: runs 5133–5136 recorded no receipt for their sandbox shell calls | Partly: 38 sandbox-reported receipts (the agent's word, not the node's), arguments cut at about 120 characters |
| 5. Recovers from an interruption | Yes, retried (`interrupted: killed (exit 137)`), but the run then reads "succeeded, ended its turn" and Sol's post blamed its 12-round budget | Yes, retried; the fresh session was not told of the interruption and did not need to be |

**What it shows.**
- **The agent loop was most of the gap, and an existing one closes it inside Endora.** With
  member, model, words, tools and checks unchanged, Claude Code's loop verified 2 of 3 to the
  worker's 1 and 0, the same as Claude Code on its own. Its own record says why: it worked R1
  through to running the unset case (27 receipts in two turns, where the worker's first turn
  stopped at 12 rounds), and on R3 it read past the `semver_only` filter to the current release
  with the same tool that misled the worker twice.
- **Run-to-run variance is real.** The worker went from 0 to 1 of 3 on identical inputs (R1).
  Three-task samples rank arms only roughly; R3 (the worker twice wrong, the same way) and R2
  (every Haiku arm wrong) are the steadier signals.
- **R2 is the model, not the loop.** Four Haiku arms in two harnesses all missed why only the
  console leaks; only Opus found it. A diagnosis that has to look inside a program needs the
  larger model, whatever the loop.
- **Cost.** Claude Code's loop spent more per run ($0.054 to $0.030) and less per verified task
  ($0.027 to $0.030): ADR 0117's own widening test ("finishes tasks for less, or for about the
  same with less sent back") is met on this sample.
- **Its weak points, by the judge:** receipts are the sandbox's word and cut short, so a
  forbidden read could not be ruled out from them; a thread post quoted a private network address
  from a log line (in HQ's store only); HQ's mirror kept 968 under `refs/submissions` only.

**By the stop condition: an existing runtime meets the need, so no engine is built.** The
proposal's direction (an engine of Endora's own, with a CLI) is not supported; what is, is the
runtime Endora already has: Claude Code, carrying Endora's tools and rules over the MCP bridge
it was given in October. What the rounds' coordination machinery added (ranks, reviews,
handovers) showed no value at any point; the solo loop with its kept sandbox and mechanical
checks is enough scaffolding around it. Not settled here: a clean install, a forbidden-read
check from the agent's own receipts, and whether a Claude Code member on a larger model clears
R2 inside HQ.

**For the owner.** The fork now carries HQ branches 960, 962, 963, 965, 966 and 968, all pushed
by HQ's delivery; only 963, 966 and 968 are judged acceptable, and 962 and 965 carry the jellyfin
downgrade. Prowlarr's key is still leaking (37 console lines at 21:53 UTC); the fix is the local
branch `fix/sonarr-console-key-leak`.

### The worker brought up to Claude Code's level (1–2 Nov, head to heads)

The owner's direction after the proof of concept: keep Endora's own worker rather than build on
another vendor's CLI, find what its loop lacks, and close the gap by targeted head to heads. A
research pass compared the worker's loop (`app/worker/src/turn.rs`) with Claude Code's
published design and with the stored transcripts of both arms, and ranked the gaps: the round
budget (12 against 30), instructions that reward closing over checking ("look, post, end the
turn with set_status"), no carry-over at a cut, little reply room, 6 KB shell results, no
retry. Two were built test-first on branch `worker-parity`, each released alone and run twice
on R1–R3 (Sol, Haiku 5.5, the same words, $3 cap a run), each judged by a fresh round judge on
round 4's standard:

- **H1** (`4d824793`): a turn on Claude gets 30 rounds, as Claude Code does; the house's model
  keeps 12.
- **H2** (`fa5cfe15`, on H1): one paragraph in the instructions: before saying done, try the
  failure each criterion guards against (the value missing, empty or wrong), compare the choice
  with what is current, read rather than infer, and never set in_review while a check is
  pending.

| Worker | Samples | R1 | R2 | R3 | Verified | Cost per verified |
|---|---|---|---|---|---|---|
| As released (12 rounds) | 960–962, 963–965 | 0 of 2 → 1 of 2 | 0 of 2 | 0 of 2 | 0, 1 of 3 | $0.058 |
| H1, 30 rounds | 969–971, 972–974 | 0 of 2 | 0 of 2 | 0 of 2 | 0, 0 of 3 | — |
| **H2, 30 rounds and the check** | 975–977, 978–980 | **2 of 2** | 0 of 2 | **2 of 2** | **2, 2 of 3** | **$0.015** |
| Claude Code runtime, same setup | 966–968 | 1 | 0 | 1 | 2 of 3 | $0.027 |
| Claude Code alone, Haiku | exp/h-r4-* | 1 | 0 | 1 | 2 of 3 | tokens |

**What each showed.**
- **H1 alone changed nothing.** No turn came near 30 rounds (22 at most), and both samples
  failed as before: R1 without an assert, R3 two majors back (once to 10.10.7, with overseerr
  downgraded too). The budget was not what held the worker back.
- **H2 fixed the two judgement failures, four times out of four.** On R1 Sol played the role
  unset and set and kept the assert; on R3 one run first pinned 10.11.11 from the filtered list,
  then, comparing with `latest`, found 12.4 and corrected it before committing; the other
  compared first. Both waited for their checks. Its turns ran 15–18 calls, past the old 12, so
  H2 needs H1's room: the two together, not either alone.
- **R2 stayed failed in every Haiku arm** (now eight runs in three harnesses): the right where,
  no where-not, no why. That is the model's depth on a diagnosis inside a program, not the loop.
- **Cost.** H2's six tasks cost $0.059: $0.015 a verified task, under half the Claude Code
  runtime's $0.027 on the same setup.

**So the worker is at Claude Code's level on these tasks, on the same model, at less cost.**
Limits, honestly: three tasks the worker has now seen many times (the instruction is general and
names none of them, but only fresh tasks show it carries over); two samples an arm; the gaps not
tried (carry-over at a cut, reply room, result size, retries) are untested, not ruled out. No
scenario was added: the change is in the worker's own instructions and setup, which the domain's
cucumber suite does not drive; its unit tests pin the text and the rounds.

**Still open.** HQ's delivery pushed every arm's branches to the public fork (`hq/task-960`
through `-980`); only 963, 966, 968, 975, 977, 978 and 980 are judged acceptable, and 975 and
978 carry a `.pyc` in history. Two posts (8357, 8362) quote a private address from a log line,
in HQ's store only. Prowlarr's key still leaks (41 console lines at 23:00 UTC).

### Held-out test of H2, set before any run (2 Nov)

The owner, on the result above: "I'm worried about over fitting again." The worry is sound. H2
was written after reading how R1 and R3 failed and then scored on R1 and R3; its two examples
(the value missing; compare with what is current) are those two failures in general words. A
gain counts only when it shows on tasks it was not written from. Fixed before any task was
seen or run:

- **Tasks.** Six fresh tasks on the fork at `ee878f7`, picked by an agent told nothing of H1,
  H2, the arms or round 4's failures, of at least four kinds, none a repeat of an earlier task.
  It writes each task's criteria and a judge's note (hidden from the arms); neither is edited
  after.
- **Arms**, all on Claude Haiku 5.5, with the same words, two samples each (12 attempts an arm):
  A, the worker as released for round 4 (`81f49915`: 12 rounds, no H2); B, the worker with H1
  and H2 (`fa5cfe15`); C, Claude Code alone in a worktree. A and B run as Sol, solo, at a $3
  cap a run, released one after the other on the same day.
- **Judging is blind.** Each result is relabelled and shuffled; the judge sees the task, its
  criteria and note, and the branch or report, never the arm. Round 4's standard: every claim
  re-checked read-only.
- **The bar.** H2 holds up if B verifies at least 3 more of 12 than A, and no more than 2 fewer
  than C. Anything less, and the R1–R3 result is recorded as not shown to carry over. Harm
  counts against B: a task B leaves unfinished or blocked where A finished it.
- **No tuning on these six.** If H2 fails here, these tasks become ones it has seen; a further
  attempt needs a new set picked the same way.
- **A second set, off the domain** (added the same day, before any of its tasks was seen). The
  owner, on the first set's titles: all six are container roles in the same fork, so H2 could
  still be fitted to the homelab rather than to R1–R3. A second blind pick: six tasks in two or
  three public open-source repositories of ordinary software (libraries, parsers, CLIs), far
  from containers and infrastructure, checkable with Python's standard library alone, as the
  sandbox has. The same three arms (A and B through the outside-repository mirror), the same
  blind judging, and the same bar on its own 12 attempts. H2 counts as holding up only if it
  clears the bar on both sets; on one alone, it is recorded as fitted to that domain.

### What the worker's gaps cost, measured before building (2 Nov)

The owner, on the list of what Claude Code has and the worker lacks: build nothing unless
measurement shows it is needed. A read-only pass over the 496 stored Haiku worker turns
(10-19 to 11-02, 2,517 rounds; only 53% of Haiku runs have a transcript, and thinking is stored
empty) counted, for each gap, the turns where it caused harm, not where its marker merely shows:

| Gap | Turns affected | Harm | Verdict |
|---|---|---|---|
| Reply cap, 4,096 tokens | 7 of 463 runs since 10-29 (1.5%), 13 attempts | each hit fails the turn and HQ redoes it from zero; one cut a shell edit mid-call; one task failed nine times running | needed: rare, costly each time |
| Prompt budget cuts | 43 turns (8.7%), 10-31 to 11-01; none since | the next turn spent 16% of its rounds re-reading (1–4% after a turn that ended itself); one task dropped after five | needed then; re-measure on today's budget |
| Held-back tools called with guessed arguments | 44 of 134 calls (33%), against 0 of 38 for a tool up front | about 0.1 round a turn | small; deferred |
| Results cut at 6 KB | 21 of 53 turns since 1 Nov 16:55 | about one round of re-reading a cut; whether a turn acted on what was cut cannot be told | not shown; instrument |
| API failures, no retry | 0 model API failures on Haiku in 463 runs | — | not shown: no retry built |
| Edits through the shell | 3 of 105 failed, each caught by the turn's own check | about two rounds | not shown: no edit tool built |
| Twelve rounds | 21 turns; the next one usually finished | no work lost | weak; 30 stays as built |
| Unrelated tools (chess, quiz) | 0 calls on work tasks | — | not shown |

**Next, in this order, each set before it runs:**

1. **Finish the held-out test** above, with every arm unchanged. Every arm also reports its cost
   per verified task beside its completion.
2. **Logging only.** The worker writes, against the run and task it already has: why the turn
   ended (its own word, out of rounds, budget, reply cap, failure), each reply's stop reason and
   output tokens, a cut result's full size, and a shell result flagged when it holds a traceback
   or error under exit 0. No new subsystem. Accepted only if a test shows the model's requests
   unchanged, byte for byte, with the same tools and the same endings; then a baseline is taken.
3. **Reply cap 4,096 to 8,192**, alone, on a fresh blind set: control and treatment on the same
   tasks, model and day, scored per task, not per turn (nine failures on one task are one task
   badly hurt). Cap hits are rare (1.5% of turns), so twelve tasks cannot show the gain; the set
   checks for harm instead: the treatment must verify no fewer tasks and use no more than 25% more
   tokens or rounds per verified task. The gain is read from step 2's logs: cap failures should
   go to none. Not raised to the model's maximum: output length is not progress.
4. **Re-measure the budget cuts** from step 2's logs before any change to the budget.

### Parked proposal: Endora runtime simplification (owner, 1 Nov)

**Status:** deferred until round 4's decision. Research and implementation are not authorised: no
ADRs, crates, interfaces or SDK investigation. This is a decision bookmark, not a project.

**Problem.** HQ's coordination costs more than the work: round 3 recorded 246 of 295 turns that
changed nothing, while the single-agent baseline did all ten tasks. Most of the execution needed
already exists: `app/worker`, the sandbox tooling and the task's kept workspace.

**Direction.** The engine owns execution and task state; a CLI is the main control; a UI is an
optional client. Keep deterministic verification, permissions, budgets, receipts and kept
workspaces; remove the coordination and organisational machinery.

**Endora's distinct value.** The house and lab integrations, unattended runs, local models, one
spending limit across providers, and the private-data safety rules.

**Decision sequence.** (1) Round 4 completes without changing scope. (2) HQ continues, narrows or
redirects. (3) Only if it narrows or redirects: a bounded proof of concept, on R1–R3 with a
fixed effort budget, comparing an existing agent runtime with Endora's tools against the
extracted HQ worker. (4) The smallest that meets the need wins.

**Proof-of-concept acceptance.** From a clean install: run a real task, keep its workspace across
turns, verify its result independently, inspect its receipts, recover from an interruption.

**Stop condition.** If an existing runtime meets the requirements, no replacement engine is
built.

**Next action:** run (1 Nov): see "Runtime proof of concept" above. An existing runtime met
the need, so by the stop condition no engine is built.

### Round 4, planned (owner's review of 31 Oct; superseded on 1 Nov by the section above)

The purpose of harness engineering is not to make the harness more sophisticated. It is to make
valuable work more reliable and economical. Round 3's failures so far were in the machinery
around the reasoning (D17, D19, D20), not in the reasoning: Ada judged 947 right four times and
the verdict could not land. So round 4 fixes that machinery before it adds anything, and adds no
new agent, orchestration layer or scoring framework.

**The changes, in order:**
1. **The three infrastructure defects:** D17 (a refused wake counts as a failure and climbs the
   item), D19 (released mid-round 3; round 4 is its first full measure), D20 (a turn's results
   fill a Haiku prompt and the item climbs).
2. **Architecture: limits that stop working members (owner, 1 Nov).** A limit should stop
   waste, not work that is moving. Three in round 3 stopped members while the pool had money:
   - **Shares follow price, not work.** *(Built on `round4` as claim classes Haiku 8, Sonnet and
     DeepSeek 2, Opus 1, house models 0; the owner kept that split on 1 Nov.)* A member's claim is its role's weight times its model's
     price class (`money::claim`, ADR 0113), so Opus has the largest share and Haiku the
     smallest, though Haiku does most of the work and is the cheapest paid model. Ada's share
     was $0.19 of a $16.60 day; she stopped at $0.88 with $3.77 left (D21). The owner's
     direction: **most of the money goes to Haiku.** Size shares from the work each rank does
     (or give the cheapest paid rank the largest claim), and let the climbs to Sonnet and Opus
     draw on what is left; an ADR 0113 amendment.
   - **Asked once a day** (D21): a member capped a second time stalls with no card.
   - **A count of turns for a free model** (D15): Bo stops at 60 turns whatever they achieve.
   Each is replaced by a check on waste (no progress, repeated failure, a judged item) rather
   than a fixed count, and each gets a scenario in the owner's words.
3. **A. A fresh set beside the fixed ten.** The ten stay unchanged as the regression benchmark.
   Each round adds 2–3 tasks HQ has never seen, drawn from the real backlog and written the same
   way, scored apart: does an improvement carry over, or does HQ only get good at the ten?

   **Round 4's fresh set** (drafted 1 Nov from the fork's own gaps, approved by the owner the same day; same rules as the ten: the
   fork's `develop` at `ee878f7` or read-only on the lab, nothing in inventories, `group_vars`,
   `host_vars` or vaults; code tasks end as a branch ready for review). One repeats a kind of the
   ten on roles HQ has not seen, to test carry-over; two are kinds the ten do not have.

   | # | Size | Task | Acceptance criteria (all must hold) |
   |---|---|---|---|
   | F1 | S | Pin the `jellyfin` and `overseerr` roles off the `latest` image tag | Each tag is a release that exists on its registry, named in the commit; `jellyfin`'s molecule scenario (or, if it cannot run, `ansible-lint`) passes; `yamllint .` and `ansible-lint nas.yml` no worse than `develop` |
   | F2 | M | Molecule scenarios for the `gatus` and `ddns_updater` roles, which have none | Each role gains a `molecule/default` scenario following the fork's existing ones; both run and pass on the branch (the output quoted); a deliberately broken task makes one fail (shown once, then reverted); lint no worse than `develop` |
   | F3 | M | The NAS runs short of memory (its swap was nearly full on 31 Oct): which containers use it, and what to cap | The ten largest containers by memory, measured at two or more times, with the figures; which roles already set a `*_memory` limit and which do not; a proposed limit for each of the largest, with the reason; nothing changed on the lab |
   | F4 | S | Outside the homelab: fix python-pathspec issue #176 (https://github.com/cpburnz/python-pathspec/issues/176, chosen 1 Nov): with the rule `/data/private.txt`, `GitIgnoreSpec` misses `data//private.txt` and `data/./private.txt`, which `git check-ignore` matches. The project's repository is set to https://github.com/cpburnz/python-pathspec (base `47ea6a5`) | A new `unittest` test over the three paths (the sandbox has no pytest) fails on `47ea6a5` and passes with the fix; paths holding `..` are left as they are; `python3 -m unittest discover -s tests -t .` passes offline; only `pathspec/` and `tests/` change; a local branch on top of `47ea6a5`, nothing pushed anywhere |

   They are not run in round 3. A task is retired from the fresh set once it has been run, and
   new ones are drawn each round, so the set stays unseen.

   **Not only the homelab (the owner, 1 Nov).** F1–F3 guard against getting good at the ten,
   not against getting good only at the house's homelab: the same fork, tools and kind of
   problem. So:
   - **One fresh task each round is outside the homelab**: F4, a small task in a public
     open-source repository HQ has never seen (a bug with a failing test to write and fix, or a
     documented feature gap), cloned into the sandbox. Its branch stays local and nothing is
     pushed anywhere. Its criteria are written before either arm runs, and Claude Code does it
     too, so it has a bar. The round's start chooses it (`hq-round` step 1).
   - **Every change is labelled general or homelab-specific** when it is planned. General: it
     helps any task (D17, D19, D20, D21, the money split). Homelab-specific: it helps only the
     lab's work (a better `image_tags`, a lab tool). A homelab-specific change needs a stated
     reason, and the round's report says how much of the gain came from each kind.
   - **The result is reported three ways**: the fixed ten, the homelab fresh tasks, and the
     task outside the homelab. A gain on the ten that does not show outside the homelab did not
     carry over.

4. **B. Cost and complexity, not reliability alone.** Each round's report adds the cost of
   building the round (the assistant's sessions, not only HQ's turns), the mechanisms it added
   (handoffs, recovery paths, model calls, settings a task depends on) and the code size. A round
   that gains a task by doubling cost or adding coordinating agents may not be worth keeping.
5. **C. The playbook kept small.** `hq-round` holds its standing rules, its procedure and its
   lessons as three recognisable parts. When a defect is fixed and has held, the extra
   instruction it needed is retired; its history (the defect, its evidence, its fix) stays in
   this document, so a later regression can be understood. Not automatically after one round.
6. **D. The judge tested, on two counts.** A few known-good and known-bad branches, run before a
   round: **judgment correctness** (does the judge accept and refuse rightly?) and **judgment
   delivery** (is the verdict recorded and acted on?). D19 was a delivery failure; a test of the
   reasoning alone would have passed it.
7. **E. The piping audit, before round 4's changes are final (owner, 1 Nov: "let the agents be
   agents").** The question is whether HQ's own process, not the models, causes the failures.
   Round 3's record so far: 296 turns for nine tasks (Claude Code took one session each); 209
   of the wakes were a loop's "continued" and 35 were recoveries (22 of them refused); 255
   `repo_checkout` calls, because every turn starts in an empty sandbox (D27); the four tasks
   done but never reported (947, 949, 951, 955) were each lost to HQ's process (D19, D22, D23,
   D25), not to the work. The audit, from the store, per task: turns that changed nothing
   (re-reading the briefing, re-checking out, waiting); every handoff (climb, review, handover,
   desk question) and whether it changed the outcome; time working against time waiting; tokens
   spent on protocol (briefing, `my_turn`, `claim`, re-checkouts) against tokens spent on the
   work. Each step of the pipeline is kept only where it changed an outcome; the rest is cut
   or merged. And one arm settles the question directly: **one member, one long turn, no
   ladder**, the fresh set worked by a single Haiku member end to end (it checks out, works,
   checks its own criteria, submits), beside the team's arm. If the single member matches or
   beats the team, the team's machinery is cut back to what that arm lacked.
   *Built on `round4` (1 Nov): a project setting `solo: <member>` (ADR 0142, amended); started with
   `experiment.py start --round 4 --from-tasks <ids> --solo <Name> [--yes]`, in a project of its own, by a
   Haiku member hired for it so the team keeps Ada. Its item is done when HQ's checks pass and the member
   sets it ready; nine failures block it.*
8. **Task 8's window becomes short (a few hours)** unless a round shows the longer window finds
   something the short one misses (the owner, 1 Nov: "we don't even know if 24 hours is
   valuable against a shorter test"). The judge of task 8 notes whether anything either arm
   found needed more than the window's last few hours.

8. **F. Faster rounds, and no fitting to the lab (owner, 1 Nov: "drastically reduce the time
   wasters", "ensure we don't hyper fit the system around lab health").** Built on `round4`
   before its release: an item keeps its sandbox between its turns (the 255 re-checkouts); no
   `my_turn`/`claim` moves on a turn already holding the item (D30); downtime and chess pause
   while a round runs (67 such turns cost $0.58 in round 3's 48 hours); `experiment.py metrics`
   scores a round from the store in one command, three ways; a replay bench
   (`scripts/replay-bench/`) tests a change to prompts, tools or models on real decision points
   before a live round. A round runs 8 hours at most, judged as each task ends. Against fitting
   to the lab: every change is labelled general or lab; no new lab tool or lab-only prompt while
   the outside-the-homelab score trails the others; a gain on the ten that does not show on F4
   does not count. Off the critical path until the investment gate: sound, the office and game
   view, chess (kept, costing the rounds nothing).

**Two scorecards, because they are two products.** HQ's development work and its standing,
unattended work have different measures. HQ could fall short of Claude Code at development and
still be worth more for the house's running; that would be a result, not a failed experiment.

| Development | Unattended operations |
|---|---|
| Accepted tasks (fixed ten; fresh set apart) | Successful scheduled jobs |
| Cost per accepted task | Cost per successful run |
| Human interventions | Unplanned human interventions |
| Rework | Recovery rate |
| Time to an accepted result | Time to detect and recover |

**An investment gate after round 4, and after each round from then.** A small section of the
round's report, answered from its numbers:
- Did acceptance improve (fixed ten; fresh set)?
- Did cost per accepted task fall?
- Did interventions fall?
- Did the gain carry over to the fresh tasks?
- What did it take to build (cost, mechanisms added)?
- For a change that saves money per task: break-even tasks = its build cost ÷ the saving per
  accepted task (a $200 change saving $2 a task pays back after 100 tasks). If HQ still costs
  more per accepted task than Claude Code, it may never break even on cost alone, so the
  projected workload and the owner's time saved count too.

If none improves across two rounds, development-path investment pauses for a decision. The
gate allows three: **continue** the general development harness, **narrow** its scope to the
task kinds where it pays, or **redirect** investment into unattended operations. The decision is
the owner's (money and direction); the report gives the evidence for it.

### The house model: qwen3.5:9b against qwen3:8b, replayed (1 Nov)

The switch on 30 Oct went live with round 1's design changes, so the rounds cannot separate the
model's effect. A replay can: 30 decision points from Bo's stored transcripts (15 from turns on
each model; every point the conversation up to a call Bo made, with the tool schemas HQ, the node
and the worker give a turn), sent once to each model through Ollama as the local relay sends a
team call (thinking on, 8,192 output tokens), then judged blind, A and B shuffled per point.

| | qwen3.5:9b | qwen3:8b |
|---|---|---|
| Made a tool call | 29 / 30 (one cut at the output limit) | 28 / 30 (two ended with nothing) |
| Arguments fit the schema | 29 / 29 | 25 / 28 (`set_status` without its task, three times) |
| Receipts cited that the turn never saw | 0 | 2 (one `done` on invented evidence) |
| Blind judge: better move | 14 (8 ties) | 8 |
| Judge's mean score (1–5) / moves scored 1 | 2.93 / 3 | 2.73 / 13 |
| Median seconds, output tokens, thinking characters | 17, 379, 1,277 | 22, 540, 2,172 |

On points from current turns (the 9B's era) the two are even (7 wins each, means 3.1 and 3.3);
on the older points the 8B made most of the harmful moves (8 of its 13): early `done` or
`in_review` with invented evidence, a turn ended with nothing. So the 9B is the safer and
faster model, not the smarter one: both are at "reasonable, not the best" on average, and both
spend moves on `my_turn` (which repeats the briefing they already have) and on `find_tools` for a
tool already offered, which is the harness's to remove (item E). One sample per point, one
judge: a direction, not a measurement to the decimal. The replay's scripts were kept outside the
repository.

## Open issues and next steps (kept current)

The one list of what is open, kept up to date as rounds run. Ticked items say where they were
done. Defects are numbered so the next round's changes can name them.

### Defects found in round 2, not yet fixed

- [x] **D1. A model change can leave a member unable to start.** Rae was moved to DeepSeek with
  the runtime still Claude Code; every turn failed from 21:18 to 00:2x. HQ should refuse a model
  whose runtime the runner cannot start, or set the runtime with it. *Fixed on `r3-setup-safety`:
  Claude Code is a developer's on Claude alone (not DeepSeek), a model change it cannot run moves
  the member to the worker and says so, and Claude Code is refused for such a model (ADR 0117,
  amended 2026-10-31).*
- [x] **D2. A failed start does not wake the item's holder again.** Rae's items sat until a note
  on each thread at 00:47. Fixed on `r3-wakes` (ADR 0142, amended 2026-10-31): woken again after
  a wait from 5 minutes to an hour, and no failure.
- [x] **D3. A blocked item at the top rank keeps waking its member.** Kit was woken on 938 about
  every 11 minutes after it was blocked and judged, about $0.39; set aside by hand. The stranded-work
  path nudged and failed it. Fixed on `r3-wakes`: a blocked loop item wakes nobody until the
  owner answers in words or moves it back to work.
- [x] **D4. A released tool is not offered until a loop lists it.** `image_tags` shipped but was
  not in the ansible-nas loop's tools until 02:2x. A new contract should reach the loops that
  can use it, or the release check should say which loops lack it. *Fixed on `r3-setup-safety`:
  a listed `repo_checkout` brings the read-only `image_tags` with it (ADR 0113, amended
  2026-10-31).*
- [ ] **D5. The NAS's model posts finished work as a desk question** instead of asking for
  review (935, twice; 938, once).
- [x] **D6. HQ cannot get a CI run on its branches and does not notice.** The fork's workflow
  runs on push only for main and develop. Either the workflow runs on `hq/*` branches, or HQ
  reports that a CI criterion cannot be met. *Fixed on `r3-reviews-ci`: the node hands HQ the
  workflows at the branch's head; when a criterion asks for CI and no workflow's `on.push` runs
  for the branch, HQ's checks at review say the criterion cannot be met and HQ refuses a pass
  (ADR 0142, amended 2026-10-31). The fork's own workflow change waits for the rounds to end.*
- [x] **D7. Paid reviews are most of the cost.** $1.90 of round 2's first $2.97 was Sonnet and
  Opus reviewing; Kit spent $3.00 of a day's pool by 08:15. *Fixed on `r3-reviews-ci` for work
  below Haiku's rank, then on `r3-review-cap` for all work (owner, 31 Oct): no review above
  Haiku's rank (ADR 0142, amended 2026-10-31). Work done below Haiku's rank is reviewed one rank
  up within the cap; work done at Haiku's rank or above (Ada's, Kit's, Mara's) is reviewed by
  another member at Haiku's rank, else the nearest below it, else, last, the nearest above.
  Escalation by failed checks and send-backs is unchanged, and HQ's checks, CI included, still
  run at review. Round 3 records each reviewer's verdict against the owner's re-checked
  judgment; past one disagreement in ten from Haiku for a kind of work, that kind goes back to
  one rank up.*
- [ ] **D8. The house model's turns end at the budget with little to show.** Bo's 929 turns hit
  the 240 KB budget in 7 or 8 calls each; after three such passes the item climbed.
- [x] **D9. The pool is a constant.** $95 a month in code; the Claude credit that expires on the
  18th cannot be spent without daily top-ups (see Next steps). Built on the `r3-credit-pool`
  branch (31 Oct): the day's pool is sized from the grant left and the days to its renewal,
  between the floor and three floors (ADR 0113, amended 2026-10-31); not yet merged or released.
- [x] **D10. A blocked item asks the owner again** about every half hour (round 1: 915 five
  times, 920 four). Fixed on `r3-wakes`: one question per item waits on the desk; another is
  refused with the reason, HQ's own is added to the waiting card. Round 1's repeats each came
  after an answer, which this does not stop.
  *It came back in round 3 that way: 950 (cards 5061, then 5062 50 s after the answer) and 956
  (5064, then 5065). Fixed on `round4` (c759e60e): once the owner has answered an item's question
  in words, a later question on it is refused with that answer quoted (act on it, or block with
  the reason); new decisions reach the owner by a block, an approval or a money or key card.*
- [ ] **D11. Lab: containers are recreated every night by Watchtower.** Diagnosed 31 Oct (read-only):
  Watchtower runs at 03:00 (`--schedule 0 0 3 * * * --cleanup`) and recreates every container on a
  `:latest` tag with a newer image; on 31 Oct it recreated six, sonarr and nzbget among them, at
  03:24. It also recreated gluetun on 19, 20 and 30 Oct, and on 15 and 16 Oct failed with "joining
  network namespace of container: No such container": prowlarr, deluge and nzbget share gluetun's
  network by container id. Likely the cause of the VPN stalls the 2-hourly restart works around,
  and it puts restarts into task 8's window. *Endora's parts done on `r3-watchtower` (released
  with round 3): every Endora container is labelled out of Watchtower, and `homelab_status` says
  when each container was last recreated on a new image ("Updated last night by Watchtower:
  sonarr, nzbget (03:24)"; ADR 0106, amended 2026-10-31). The fork's part is the branch
  `fix/vpn-stack-out-of-watchtower` (the VPN stack labelled out, pinned, updated together by the
  playbook, gluetun first). *Applied by the owner on 31 Oct at 21:00 UTC (17:00 EDT): gluetun on
  v3.41.3, healthy, WireGuard up; prowlarr, deluge, nzbget and flaresolverr on the new gluetun and
  out of Watchtower, all on pinned images. Task 8's clean window runs from then to 1 Nov 21:00 UTC
  (16:00 EST; the clocks change on 1 Nov): 24 hours, twelve of the restart's 2-hour cycles, more
  than the criteria's three points in time, and short enough that the logs still hold it when it
  is judged (round 1 lost this task to logs that had rolled over). Both arms get the same window.*
- [x] **D12. A long item's thread eats every turn's budget, and nothing is kept.** 937 failed
  more than twelve turns across Rae, Ada and Kit: each re-reads the thread, now dozens of handover
  blocks long, and reaches the budget after 5 to 8 calls, before anything is committed and
  submitted. $2.14 spent with no branch kept. A turn should read the newest handover, not every
  one, and work in progress should be kept (a work branch committed before the turn ends).
  *Fixed on `r3-long-items` (ADR 0142, amended 2026-10-31): older handovers and out-of-room last
  words are left out of the briefing and `my_turn` (a climb's briefing 28,151 to 4,129 bytes); the
  worker commits the sandbox to the task's branch as the turn runs out of room and the node keeps
  it unchecked for the next turn, which the handover tells to check it out; a turn that kept new
  work is no failure, so 937 would not have climbed. Not yet merged or released.*
- [x] **D13. The team cannot read upstream ansible-nas.** The sandbox is offline and
  `repo_checkout` refuses `main`, so 936 (what upstream changed) cannot be done. A read-only
  upstream checkout, or a mirror of upstream on the node, would let it. *Built on `r3-upstream`
  (ADR 0106, amended 2026-10-31): the node's mirror fetches upstream's default branch on the
  fork's schedule, every checkout carries it as `upstream/<branch>` to read offline
  (`git log --oneline develop..upstream/master`), and `repo_submit` and the push refuse anything
  named for upstream or carrying its commits; not yet merged or released.*

- [ ] **D14. The house model calls Endora's tools from the shell.** Round 3, 947: Bo ran
  `repo_checkout develop` in the shell (exit 127) instead of calling the tool, then asked the
  owner whether to go on. The shell's "command not found" could say which Endora tool has that
  name and how to call it.
  *Partly met on `round4` by D27 (85436f8e): the fork's tools are up front and the shell tool
  says the fork comes in with `repo_checkout`; the shell's own "command not found" still names nothing.*

- [x] **D15. The house's own model is held to 60 turns a day.** Round 3, 31 Oct 15:0x: Bo (the
  NAS's model, which costs nothing) reached "60 turns a day is the most a member takes" and
  stopped until tomorrow, partly from looping on 953's questions. The cap guards against a
  runaway; for a free model a loop that keeps failing is what should stop it, not the count.
  *Fixed on `round4` (7aa23595: a waste check (looping, re-reviews, rooms that keep nothing) replaces the 60-turn count), to release with round 4.*
- [ ] **D16. The top-up refusal names a figure it then refuses.** HQ said "$2.84 may still be
  added today" and refused $2.84; $2.83 went through. The room is rounded up when said.

- [x] **D17. Refused wakes during review count as failures, and climb the item.** Round 3, 948:
  after it was ready at 13:30, wakes HQ refused because the item was in review were counted as
  failures 1 to 3, so the item climbed to Kit (Sonnet) and then Mara (Opus), who only re-checked
  the same head. $0.24 of its $0.36 came after it was ready, and the reviews went above Haiku's
  rank despite the cap (D7).
  *Fixed on `round4` (0345027c: a wake refused while the item waits in review is not a failure), to release with round 4.*
- [x] **D18. DeepSeek's turns ran out of room eleven times in a row and kept nothing.** Round 3,
  948: Rae's turns ended at the budget without keeping work in progress (D12's fix), and the item
  moved only once it reached Ada. Check why the worker kept no work on these turns.
  *Fixed on `round4` (0e2cdda8: on a cached model the turn's budget counts its conversation once (12 KB opening, ~50 KB conversation was counted as 238 KB)), to release with round 4.*
- [x] **D19. A Claude reviewer never had its verdict tool.** Round 3, 947: Mara (Opus) put the
  work in review at 21:10 UTC; Ada (Haiku) reviewed it four times, approving each, and every
  verdict went to `set_status`, refused ("this task is not yours", then a `d86` citation). Her
  turns listed `review` under "held back": the worker loads a fixed list on Claude and left the
  rest to Anthropic's tool search, honouring HQ's up-front mark only on the house models' path.
  She never searched. Every review by a member on Claude this round was exposed to it. *Fixed on
  `codex/agent-collaboration` (`app/worker/src/turn.rs`, test
  `a_claude_turn_loads_what_hq_marks_for_its_kind`); the worker has no scenario suite, so its
  end-to-end test stands for one. Released in `1340e5b3` mid-round, by the owner's word.*
  *Released (1340e5b3, released mid-round on the owner's word: HQ's up-front mark is honoured on the Claude path).*
- [x] **D20. Haiku's turns on 949 filled their prompt budget on every turn, and the item climbed
  to Sonnet.** Ada read 400–460 KB over 6–7 calls a turn (mostly `image_tags` and the
  checkout); after 12 continuations HQ counted three failures and handed it to Kit, who finished
  it in 10 turns for $0.94 (22:02–22:20 UTC), of the task's $1.38. The cap on results a turn
  reads, or a smaller `image_tags` answer, would keep it at Haiku's price. Related to D12 and D17.
  *Fixed on `round4` (4117683b and 92c51ac7: every lab or shell result held to 6 KB on every API, unless asked raw), to release with round 4.*
- [x] **D21. A member capped a second time in a day stalls in silence while the pool has money.**
  Round 3, 31 Oct: Ada had today's $0.50 allowance (card 5025, approved) and reached her limit
  ($0.89 of $0.88: twice her $0.19 share plus the allowance) at about 22:42 UTC. HQ asks about a
  member once a day, so no card came; her turns on 949 (a review) and 951 stayed queued with
  $3.78 left in the pool, until the owner's next day. The owner's top-ups raise the pool, but a
  share of $0.19 a day is small beside a $0.50 allowance. Either size the allowance from what
  the pool has left, or ask again when the pool has money and work waits.
  *Worked round at 00:0x UTC 1 Nov, on the owner's word ("I don't want phase 3 to stop for the 80
  something cents"): the once-a-day mark for Ada was cleared, HQ raised card 5054 itself, and it
  was approved at $2.00 for the day (in place of $0.50; the pool had $3.77). A money decision
  and a setup fault, not counted as an intervention on a task.*
  *Fixed on `round4` (2a4e60df: a member capped again after an allowance is asked about again), to release with round 4.*
- [x] **D22. Work that meets the bar's reading of a criterion ends blocked, never delivered.**
  Round 3, 949: the branch pins all seven tags, each real on its registry, and lints exactly as
  develop does (`ansible-lint nas.yml` 14 and 14, `yamllint .` 6 and 6, sorted findings the
  same), which is how Claude Code's arm and round 2's 931 were accepted. HQ's own check measured
  "no worse than develop", yet Ada sent it back on "passes" read literally, Kit asked the owner
  (card 5055), and on the task's own words ended it blocked at 00:56 UTC: the round judge
  verifies it, HQ reports it not done. Also seen: receipt r2936's bundle is cut in the store, so
  the work survives only in the node mirror's `refs/submissions`. Fix proposed: HQ's lint check
  states its comparison as the measure ("no new findings against develop: met") and the
  reviewer's brief takes the check's verdict for a lint criterion, as it already does for CI;
  keep a blocked item's submission where delivery can reach it.
  *Fixed on `round4` (0ce92a48), to release with round 4.*
- [x] **D23. Work kept in progress at the head that passed its check shuts the gate for good.**
  Round 3, 951: submitted at fc36418e20c2 (00:36 UTC), check c55 passed (no worse than develop).
  At 01:22 a turn of Kit's ran out of room and the worker kept the same head as work in
  progress; `keep_work_in_progress` sets the gate shut and queues no check, so Ada's approvals at
  01:26 and 02:10 were both refused ("needs the gate… which has not passed"), and wakes while it
  sat in review counted three failures at Kit's rank and climbed it to Mara (Opus) (D17). 955
  is in the same state (head e14515374e33 checked by c61, then kept at 02:05). No route reopens
  it but a developer's `recheck`, which a reviewer's turn does not have, and the owner has none.
  Fix proposed: keeping a head the gate already passed leaves the gate as it is (the gate
  belongs to the head, not to the last write); a scenario of exactly this sequence first.
  *Fixed on `round4` (e27c426c), to release with round 4.*
- [x] **D24. A reviewer passed work she wrote.** Round 3, 952: Ada wrote its work-in-progress
  commits (5f3db92, bfec3e4, ad586b7), Kit squashed them into 38aa78a after D17 moved it to him,
  and Ada then reviewed and closed it ("Tested and passed"). The own-work check reads who
  submitted the head, not who wrote the work. The work held on the judge's own readings, so no
  harm this time. Fix proposed: a reviewer is refused for any item whose branch carries her
  commits or whose thread shows her doing turns.
  *Fixed on `round4` (d412ac52), to release with round 4.*
- [x] **D25. HQ's CI check reads a criterion about CI as needing a run.** Round 3, 955:
  criterion 2 is "It reports today's offenders without failing CI on them"; HQ's mechanical check
  (post 7916) said no workflow runs on a push to the branch, so the criterion "cannot be met as
  the fork stands" and HQ will not pass it. The criterion asks what CI does with the rule's
  findings, which running CI's own steps shows (the judge did: 14 failures as develop, the 30
  findings as warnings). D6's refusal is right for "CI passes on the branch", wrong here. Fix
  proposed: the refusal applies only to a criterion that asks for a run's result; otherwise the
  check reports what CI's steps give on the head.
  *Fixed on `round4` (04ea0d74), to release with round 4.*
- [x] **D26. A review turn ends with no verdict, and the item sits in review.** Round 3: Rae
  (DeepSeek Flash) reviewed 955 (run 4978, 02:07 UTC), 952 (runs 4932, 4941) and 953 (runs
  5029, 5033, 5034) with one
  `repo_checkout` each and ended, no verdict and no post. Fix proposed: a review turn that ends
  with neither verdict is recorded as not done and the review goes to the next reviewer.
  *Fixed on `round4` (66225c7e (side effect: a refused verdict also passes the review on)), to release with round 4.*
- [x] **D27. On the house model, each turn's empty sandbox reads as "offline", and Bo gives up.**
  Round 3, 950, 1 Nov 04:02–05:07 UTC: Bo (the NAS's model) did use `repo_checkout`, six times,
  every one working (runs 5003–5009, 5028), after finding it behind `find_tools` (his first try
  ran it in the shell, exit 127). But in the turns that asked the owner (cards 5061 at 04:24 and
  5062 at 04:29, a second question on one item despite D10) and the one that blocked (5031), he
  did not call it: he met a fresh, empty sandbox, ran `git checkout`, then `git init` with a
  made-up remote, and took the failures for "the sandbox is offline, checkout needs github.com".
  Both cards were answered with the task's own words (two interventions); he ended it blocked
  at 05:07. On the house model `repo_checkout` and `repo_submit` stay held back beside weather
  and news tools. Fix proposed: an item of a loop that checks out the fork marks `repo_checkout`
  and `repo_submit` for the turn, so they load up front on every runtime (as D19 did for a
  verdict), and the handover's first line says the sandbox starts empty: check out the kept
  branch first. (Corrected on the judge's readings; it first said he never called it.)
  *Fixed on `round4` (85436f8e: HQ names the fork's tools up front for a loop's item on every runtime; the handover and the shell tool say the sandbox starts empty, not offline), to release with round 4.*
- [x] **D28. A turn's kept work in progress is Python byproducts, called new work.** Round 3, 950:
  all three heads kept on `hq/task-950-work` add only two `__pycache__/*.pyc` files (the fork has
  no ignore entry for them); 947, 951 carried the same file, added and removed. The handover
  says the turn "kept new work". Fix proposed: the keep leaves out what `git status` would show
  only as byproducts (`__pycache__`, `*.pyc`), and a head with no other change is not kept.
  *Fixed on `round4` (ba135b8b), to release with round 4.*

- [ ] **D29. Nothing stops an arm reading the owner's values.** Round 3, task 8: Claude Code's arm
  read 20 lines of `group_vars/nas.yml` (round 0's did the same); nothing of it reached the branch.
  The guard hook (`.claude/hooks/guard-paths.sh`) blocks edits to `group_vars`, `host_vars`,
  inventories and vaults, not reads. Fix proposed: the same paths refused to `Read` and to the
  shell's `cat`/`sed`/`grep` in the arms' sessions, as HQ's sandbox already refuses them.
- [x] **D30. Turns spend moves on the protocol.** The house-model replay (1 Nov): both models call
  `my_turn`, which repeats the briefing the first message already holds, and `find_tools` for a
  tool already offered; round 3's store shows the same pattern in `claim`/`my_turn` openings. Fix
  proposed (with item E): `my_turn` and `claim` are not offered to a turn whose briefing already
  says the item is theirs, and `find_tools` answers a request for an offered tool by saying so.
  *Fixed on `round4` (faf440fc, merged in 81f49915): a turn on its own item is not offered
  `my_turn` or `claim`; `find_tools` for a tool already offered says so at no cost. With it, an
  item keeps its sandbox between turns on the Pi (c22d9658) and downtime waits while a round runs
  (301ce505); not yet tried on the Pi.*
- [x] **D31. A finished turn is replayed, a running one is not.** The owner, 1 Nov: "watch them
  as they work". *Fixed on `round4` (`round4-live`, merged in 9fc92ceb): the worker tells its
  conversation after every round, Replay refreshes a running turn, the reasoning is kept and
  shown folded, and Activity's lines open what they name.*

### From the review of round 2's code (31 Oct), not yet fixed

Fixed and released in `7c195ac6`: HQ decides out of room itself; moved and quoted owner's files
are seen; quotes count only against the item's own receipts. The other six are fixed on the
`r3-review-fixes` branch (from `round3-local`), not yet merged or released (ADR 0113 and ADR
0142, amended 2026-10-31):
- [x] Credits: a renewal from the console view runs outside the ledger's lock.
- [x] Credits: a first grant entered with only a balance counts it all as grant.
- [x] Stubbed results are keyed by the model server's call id, which a server may reuse.
- [x] A Mac away for a while can block an item for good when it is the only rank above.
- [x] A reviewer's turn that runs out of room is never continued.
- [x] Minor: a balance read inside the ledger lock; a blocking lock on the async thread; an
  unbounded list of continued runs.

### Next steps, in order

1. [ ] Finish round 2 (936 and 937 open) and judge them.
2. [ ] Each morning while work is queued, top up the day's pool by about $4 (owner, 31 Oct),
   until the pool follows the credit.
3. [x] Build HQ's daily pool from the Claude credit left and the days to its renewal (owner,
   31 Oct; D9): on the `r3-credit-pool` branch, to merge and release with step 6.
4. [x] A stable prompt opening and a lessons list per loop, after Ralph (ADR 0142, amended
   31 Oct): on the `r3-prefix` branch, to merge and release with step 6.
5. [x] The one-team clean-up (ADR 0142's "What goes"), brought onto round 3: 14 commits on
   the `r3-one-team-cleanup` branch, to merge and release with step 6. Product code (cloc,
   tests excluded): `domains/team` 37,094 to 33,901, `app/node` 25,769 to 25,510, `app/hq`
   6,212 to 5,856; 69,075 to 65,267 in all (-3,808). Left of "What goes": the sections' stages
   and canon.
6. [ ] Merge, gate and release the waiting local work: the rest of the review fixes; a stable
   prompt opening and a lessons list per loop; the owner's scenarios for round 2's changes; the
   one-team clean-up (ADR 0142's "What goes").
7. [ ] Pick round 3's changes from D1 to D10 by cost and effect (D7, D6, D3 and D2 first), then
   run round 3 with the same ten tasks.
8. [ ] Re-run Claude Code's ten tasks as ten fresh sessions, as the method says; needs the owner
   to start them from the desktop app.
9. [ ] Office scene leftovers: a bubble covers the TEAM label; a name tag floats above its
   member; HEADS DOWN overlaps a plant; the chess table's lower seat covers the board.
10. [ ] Benchmark the Mac's 14B against the 27B (`scripts/bench-model.py`) when the laptop is
   free; switch only if about twice as fast on a large turn.
10. [x] What each model cost, compared over time (owner, 31 Oct): on the `r3-cost-by-model`
   branch, a fold on the Money card (Manage) with a row per brain a turn ran on, its turns,
   spend, cost a turn and share for the last 7 days and the 7 before, and which way it went;
   computed in HQ's scorecard (`by_model`). Not yet merged or released.
11. [x] Accepted work reaches the fork without a card (owner, 31 Oct: merging accepted work "is
    just not something I need to review"; ADR 0115, amended 2026-10-31): no "Ready to merge" or
    "Push" cards, a list on Manage, a push after a code privacy scan, merging develop behind a
    setting that stays off while the rounds run. On the `r3-auto-merge` branch, to merge and
    release with step 5. When the rounds end: turn "Merge into develop" on, and set
    `ENDORA_PERSONAL_VALUES` for the node (in `local.mk`, or the role's `endora_personal_values`
    from the vault).

### Code size (kept current)

The owner asked on 31 Oct to keep the code base manageable: one pattern per job and no growth
without a reason. `sh scripts/code-size.sh` gives product and test lines per area (cloc's code
lines, test files apart); the figure is recorded here at each release.

| When | domains/team | app/node | app/hq | The three | All product | Feature files |
|---|---|---|---|---|---|---|
| Released 30 Oct (`4b7d5766`) | 32.2k | 24.5k | 5.9k | 62.6k | 114k | 21 |
| round3-local, 31 Oct (`f8925869`) | 37.1k | 25.8k | 6.2k | 69.1k | 122k | 41 |

Round 3's work added 10% to the three in two days. Before it is released: the one-team clean-up
(ADR 0142's "What goes", about 7,800 lines out and 1,100 in on its first try) lands on top of it,
and a review of round 3's code for duplicated patterns is acted on. Merging round 3 already found
three duplicates made by work done in parallel: two `Brain::claude()`, two `ends_turn` scenario
helpers and two "woken to review" steps.

### Round 3's code: what to consolidate before release (review of 31 Oct)

A read-only review of `7c195ac6..round3-local` found work done in parallel doing one job several
ways. To fix after the one-team clean-up lands, in this order:
- [x] **Secret scanning:** two scanners; the loop lessons use the narrow one, so `NAME=value` or
  a `Bearer` token would be kept. One scanner, in `shared/kernel/src/secrets.rs`. *Done on
  `r3-consolidate` (44b3b83b): `scrub_secrets` moved into the kernel beside the shapes, with the
  link-password and end-of-sentence handling; `secret_in` is what the lessons check, and a
  lesson holding `DB_PASSWORD=hunter2`, a Bearer token or a link's password is refused.*
- [x] **Out of room:** recorded by a flag, by matching the worker's wording, and by a handover kept
  both as a setting and as a post. One record, read by the briefing and the thread view. *Done on
  `r3-consolidate` (df235928): a post is of a turn out of room by its run being in the item's
  `loop_continued` runs (set by `out_of_room_in`), never by its words; the newest handover is
  read from the thread (`loop_handover` is no longer written; old rows are left unread); the
  kept-work line is said by the handover alone, the worker's last word no longer repeats it.*
- [x] **The prompt budget:** bytes in the worker, tokens in HQ, and `ENDORA_PROMPT_BUDGET` moves
  only the worker's. One figure by `Brain`, in the team domain, passed to the worker. *Done on
  `r3-consolidate` (af4dddca): the byte figures in `shared/kernel/src/prompt_budget.rs`,
  `handover::prompt_budget(brain)` picks one by `Brain::claude()`, HQ hands it out with the turn
  and the runner sets `ENDORA_PROMPT_BUDGET` from it, so the worker ends a turn at what HQ judges
  by. The worker's figure by its API stays only for an older runner.*
- [x] **Verdicts:** `review_in` and `test_in` repeat the reviewer, own-work, CI and lessons checks,
  and both still say "one rank up". One gate and one after-verdict step. *Done on `r3-consolidate`
  (94e697d7): `verdict_gate` and `after_verdict`; `review_in`'s approve path, reached by nothing,
  went with `review::may_approve`. One order changed: a draft is refused after the own-work
  check, not before it.*
- [x] **Scenario steps:** ending a turn, running out of room, asking for review, sending back and
  setting up a loop are each written two to five times. One step each, on `Office`. *Done on
  `r3-consolidate` (738d36ea): one helper each on `Office` in `scenarios.rs` (also a reading, a
  loop item, the team, failures and the holder); the capabilities scenarios read the clock through
  `Credit::at` and dollars through the kernel's `read_dollars`. Feature wording unchanged.*
- [x] **Continuation and failed-start rules** sit in the engine; move them to pure domain code.
  *Done on `r3-consolidate` (36999609): `handover::Continued` (`after_work`, `after_review`,
  `kept_new_work`) and `runs::woken_again_after_failed_starts`; the engine loads and saves.*
- [x] **Smaller:** lessons cleaned by a weaker copy of `record::clean`; work in progress repeats
  `record_branch`; three desk de-duplications; a blocked item checked in four places; YAML and
  glob helpers twice in `mechanical.rs` (one shared with the node's `forbidden_path`); the node's
  background loop written a third time; `task.owner.or(task.assignee)` 28 times (`Task::holder`);
  the pool sent twice per snapshot; two `dollars()` that can differ by a cent; the per-item
  `loop_*` settings overlapping. *Done on `r3-consolidate`: lessons and kept branches (3a568dc0),
  `desk::placing` and `waits_for_the_owner` (5972362a), mechanical helpers and `forbidden_path`
  in the kernel's `lab` (ac3cb2a9), `api::every` for credits, the Mac and delivery, and
  `Task::holder()` in 26 places (c2192cbb), the pool sized once per snapshot (42ead635), one
  `dollars()`/`read_dollars()` in the kernel's `money` (738d36ea; a negative amount now reads
  `-$0.50`). Left, and why: the homelab loop keeps its own (it carries state between ticks and
  runs two steps); `role_readmes` and the node's `read_for_checks` stay apart (two contexts, and
  the node's also drops forbidden paths); the `loop_*` settings other than `loop_handover` stay
  as they are, since `loop_held`, `loop_continued.member` and `loop_failures` hold live items'
  counts and merging them needs a migration of existing rows.*
- [x] **A likely bug:** the runner counts an exit code it cannot read as "never started" (-1).
  *Fixed on `r3-consolidate` (bbd81629): `UNREADABLE_EXIT` (-2) beside `NEVER_STARTED_EXIT` in
  `shared/kernel/src/exit_code.rs`; HQ counts it as the worker's, as a turn with no code.*
- [x] **The clean-up's leftovers from the retired deploy check.** *Done on `r3-consolidate`
  (f77df4ee): the `deploy` and `deploy_check` desk kinds, `deploy_check`'s dismissal (a waiting
  card of that kind, if any is left, can no longer be dismissed), `EventKind::BranchDeployed`
  (stored events keep their kind as a name, so old rows still read), the console's `hqLeadOf`,
  and the Deployed card in ADR 0115. The node's test of a narrowed turn is kept and renamed: a
  project's tool list still narrows turns.*

Code size (`sh scripts/code-size.sh`), product/tests: before 101,110/32,427, after
101,368/32,313. The kernel grew by what moved into it from capabilities and the team; the team's
product grew by the domain rules now unit-tested in their own files.

### HQ console, the owner's notes of 31 Oct (one branch, after the clean-up)

- [x] Activity on a tab of its own (Work, Manage, Health, Activity): the latest 20, and "Show
  more" opens a full-screen, paged view; never an endless scroll. On the `r3-ui-activity-tab`
  branch: fifty a page through the existing events route, no API change. Not yet merged.
- [x] On a task, each turn shows what the model was given: its instructions, tools and briefing,
  secrets redacted (HQ keeps every turn's conversation, ADR 0129). Done on `r3-ui-turn-prompts`:
  a folded "What it was given" card in each turn's replay, opened from the task's turn lines;
  the worker now keeps the tools by name, the node scrubs the replay on its way out. Not kept,
  and said so: schemas, Claude Code's own instructions, and the tools of turns before 31 Oct.
- [x] The team board takes a line a member; the rest on a tap. *Done on `r3-ui-manage-layout`:
  name, model, what it is doing and today's money on one line, no groups; role, brains, allowance
  and every control on a tap.*
- [x] Routines join Projects as a kind of project; the ones turned off fold away. *Done on
  `r3-ui-manage-layout`: rows of the Projects card marked "routine", with on/off, run now and
  the schedule on a tap; the ones off behind "Show the N that are off". Display only: routines
  stay their own record (ADR 0107; ADR 0142 does not fold them into loops).*
- [x] Manage in this order: money first, then goals, then projects. *Done on
  `r3-ui-manage-layout`.*
- [x] The Mac's card leaves HQ for Settings, AI, beside the local model: on the
  `r3-ui-mac-to-settings` branch. HQ says only "Mac (busy)" where a member's brain is named.
- [x] A Chess tab: every league game, live boards for those in play, and the month's bracket
  (who has played whom, results and standings), with each game's PGN. Done on `r3-ui-chess-tab`:
  the league is a round robin, so the bracket is the current round's ("Round N, since <reset
  day>"); `GET /v1/league/games?page=N` lists every finished game (ADR 0135, amended 31 Oct).
- [ ] **Activity lines open what they name** (the owner, 1 Nov; after round 3). Every item,
  turn and member a line names is a link: `EL-955` opens the item as Work's "Looking at" does
  (its thread, criteria, branch and turns), `R4952` opens that turn's replay with "What it was
  given", a member's name opens their line on the team board, and "more" opens the whole post.
  Closing it returns to the same place in Activity, so one can step in and out. One view per
  thing, reused from where it already lives; no new screen.

### After round 3: what the second check of round 3's code found (31 Oct)

To fix on one branch, released after round 3 so the round measures one build:
- [ ] **Pushing is done two ways.** With "Cards on my desk" turned on, approving a card pushes
  through the old path, which skips the privacy scan and the owner's-needs check (off by
  default, so not live). One path: a card only marks the branch due; the node's delivery loop
  pushes. Removes about 250 lines.
- [ ] The console reads rules out of card wording (`hqCanPush`, `hqBundleOf`); HQ sends fields.
- [ ] The privacy scan's rules are in Rust and in `check-nothing-personal.mjs` with nothing
  keeping them alike: one set of cases both must pass.
- [ ] Dead pool and ping-pong leaderboard UI; unused `hqSeats` and `presetFor`.
- [ ] The Mac and credits cards' code still sits in `47-hq.js` though it shows in Settings, and
  misses the snapshot there; move it to `30-settings.js`.
- [ ] One pager (Activity and Chess), one chess-result wording, the small formatter copies, and a
  shared commit-shortening helper in Rust.
- [ ] Per-branch delivery state kept as `delivery:{id}` settings; fields on `Branch` instead.

### The owner's

- [ ] Decide on round 3's changes once round 2 is judged.
- [ ] Merge the ansible-nas branches HQ and the sessions pushed, after setting the vault values
  they now require (minio, nextcloud).
- [ ] Restore Rae to the Mac and turn its busy switch off when the laptop is free again.
