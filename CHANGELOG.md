# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

### Added

- **The owner's one-day top-up of the money pool** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md)
  rule 6). `POST /v1/hq/pool/top-up {amount_micros}` (through the node and its audit log) adds
  to today's pool only: it is gone when the owner's next day begins, and a day's top-ups are
  at most three times the base pool day. Every member's share, and the borrowing cap with it,
  grows in proportion, so a member out for the day can work again. The scorecard shows "pool
  $2.33 + $2.33 top-up" with a "Top up today" button, and an "is out for today" card offers
  "Top up the pool first" when the pool is spent. HQ records a `pool.topped_up` event.
- **The owner writes the world** ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)).
  `POST /v1/hq/world/section {page, key, title, text}` writes or replaces a section as the
  owner (an insight at least, canon if it was, no evidence asked, every revision kept) and
  `POST /v1/hq/world/timeline {page, at?, text}` adds a dated entry; both through the node and
  its audit log, with a "Write as the owner" form on the World card. The owner's sections come
  first in every member's briefing, and a member cannot overwrite one.
- **Guards against how agent teams fail** ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md)),
  proposed. A read-time **stopped diagnosis** (working, parked, held, indeterminate) now decides
  every stuck step: only parked work is nudged and climbs the ladder, held work waits for its
  check-back or answer, and work HQ cannot tell about gets one look from its lead. Recovery
  wakes are recorded under what HQ saw before they are sent and never sent twice unchanged; a
  stuck desk item is put up once and closes itself. **Done and dropped carry a reason** (met
  criteria, superseded, not needed; obsolete, duplicate of #N, won't do, blocked for good) and an
  optional successor; work waiting on a dropped task stays blocked and its holder is told at
  once. **Process-health detectors**: per project over 24 h and 7 d, coordination turns against
  product change (12 and 4:1 make a desk notice and wake the project's lead); a task sent back
  three times with no new evidence goes straight to a huddle; a member woken three times for
  nothing is not woken for nothing again. **`create_task`** refuses a sixth open sub-task, a
  fourth level, and a sub-task of a member's task that does not say which criterion it
  `serves`. **Evidence already enough** is said in the briefing, re-checking it is talk only,
  and on the third HQ submits it. **Retract and sweep**: "Not true" on a note, a section or a
  story keeps its fingerprint (word pairs, not exact strings), lists what else says it to forget
  at once, and refuses it written again; a note citing `n12` is built on it. **Fence pushes**
  (refused acts again and again) are a desk notice, and every briefing carries its goal's "not
  now". **The same ask once**: a question or board decision alike to one already waiting from
  another task joins that card, and the owner's one answer reaches each task. Every finding
  takes a **verdict** (real, false positive, not enough, resolved; did the fix work) from the
  owner or the lead (`judge_finding`), and a new Guards card shows each guard's precision, every
  threshold (one table, `domain::guards::THRESHOLDS`) and each project's talk against its work.
  Also: a tester's pass is bound to the criteria it judged ("needs re-check" once they change);
  the roster counts how often the CEO stepped into each member's work; a home-lab read that
  timed out is `unverified`, never `unavailable` (no alarm); HQ's `/health` says the tick's age.
  New tables, made if missing: `nudges`, `findings`, `closures`, `retractions`, `note_parents`,
  `desk_attached`. New owner routes through the node: `POST /v1/hq/guards/findings/{id}/verdict`,
  `POST /v1/hq/retractions`, `GET /v1/hq/retractions/{id}/sweep`,
  `POST /v1/hq/retractions/{id}/forget`. Scenarios in `domains/team/features/guards.feature`.

- **Games in the office** ([ADR 0121](docs/adr/0121-games-in-the-office.md)). Ping-pong and
  pool keep score, for nothing: when two members are free HQ's engine plays the match itself,
  point by point or shot by shot, from a seeded generator and each member's hidden, practised
  skill (ping-pong to 11 and two clear; simplified eight-ball, where scratching on the eight
  loses), at most three matches an hour a game and never a model turn. Every match is kept in
  a new `games` table, said in the activity feed ("Bo beat Dee 11–8 at ping-pong"), and counted
  in leaderboards with wins, losses and the run each player is on. The office gains a pool
  table and a chess table, and the two players of a match walk to theirs. **Chess is real**: a
  `chess_games` table, a team tool `chess` for every role (`board`, `move`, `challenge`), legal
  moves and endings by the rules (mate, stalemate, insufficient material, threefold repetition,
  the fifty-move rule), played one move a downtime turn, one game each, and adjourned as a draw
  after a week; a downtime briefing says "Your move against Dee in chess". The Office card has
  a Games section (leaderboards, the last five results, the games in play) and tapping the
  chess table opens its board. New dependency: `cozy-chess` (MIT) in `endora-team` for chess
  rules; `shakmaty` was refused because it is GPL-3.0-or-later and Endora is Apache-2.0.
- **A bench for an external agent harness** ([ADR 0123](docs/adr/0123-a-bench-for-an-external-agent-harness.md)),
  proposed: to measure an outside multi-agent harness against HQ's wing on one real task, inside
  Endora's fence. A **bench credential** on the node's model relay: a token from a file
  (`ENDORA_BENCH_TOKEN_FILE`, mounted from `ENDORA_BENCH_TOKEN`; no file, no bench, read on every
  call) that opens `/team-relay/v1/messages`, its `count_tokens` and `GET /team-relay/bench` and
  nothing else (every other team-listener route refuses it before HQ is asked); Claude Code's
  pass-through rules (ADR 0117) on `claude-haiku-5-5` or `claude-sonnet-5-5` only; a hard cap a
  day (`ENDORA_BENCH_CAP_CENTS`, $2.00 unless set) kept in a new `bench_spend` table, with each
  call's cost set aside before it is sent so seats side by side stay under it; metered to the
  usage log as `bench`, never to HQ. A **bench image** (`--target bench`): Node 22, tmux, the
  harness's package from one pinned commit, and the workbench's lab checks and Claude Code,
  now built once in two shared stages (`lab-checks`, `claude-code`); bypass mode refused.
  `make bench-image` builds it for the Pi. How to enable it is in `docs/homelab.md`.
- **The team's world: stages, stories and situations**
  ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)). What HQ's team
  knows is kept as addressed pieces with a stage, from an observation to canon: the **house page**
  (`world:house`, the CEO's to write with `write_world` on evidence, with a dated append-only
  timeline that `note_house` adds to), the derived `world:lab` and `world:team`, and **stories**
  (a lesson as when it matters, the tempting move, what to do instead and why, under a handle a
  post can cite). An insight needs evidence from two turns or a reading the node witnessed; trust
  is read when read, so an insight or canon past its horizon shows "unconfirmed since <date>";
  **only the owner makes something canon**, from the World card or a `propose_canon` card on the
  desk, and demotes it with a reason the feed and the page say. Every turn starts from a
  **situation** (fresh, continuing, after a gap, handover, recalled), decided from the record,
  and carries the world within its role's **budget**, naming on its last line whatever it left
  out; the run records it and the replay shows it. `hand_to` and a stuck task's hand-on or drop
  need a **handover note** (done, not done, next, watch out), which starts the next holder's
  turn. `tell_story`, `read_world`, `verify_world`, `retire_world` and `propose_canon` join the
  team protocol; `note_lesson` is no longer offered and `rewrite_house` is refused. The HQ screen
  gains a **World card** with Make canon, Demote and a Markdown download
  (`GET /v1/hq/world`, `GET /v1/hq/world/export.md`, `POST /v1/hq/world/promote`,
  `POST /v1/hq/world/demote`, through the node and its audit log). At start, once, the house
  log's lines move into the house page's timeline at their dates and the goals' lessons become
  stories to retell; nothing is deleted, and the old log is read-only. New tables made if
  missing: `world_sections`, `world_revisions`, `world_timeline`, `stories`, `handovers`,
  `run_briefings`. Scenarios in `domains/team/features/the-world.feature`.
- **Roles as an enforced RACI** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md)), the
  owner's product-model org of 2026-10-18. The CEO sets direction and keeps out of the areas'
  work: it reads anything (on a cloud brain the logs and queues the cloud lead reads), is
  offered no act, is never woken by the lab's alarms or recoveries (they wake the area's lead),
  and on a task in a lead's area cannot claim, move, close, accept or send back: it gives the
  lead a direction with `direct`, and the lead decides. It hands work to leads only; leads
  propose projects and the CEO decides them. Each goal has a **strategy** (purpose, success,
  measures, bets, not now), the CEO's to write with `set_strategy` and the owner's on the goal
  card (`POST /v1/hq/goals/{id}/strategy`). Each **area's measures** are on the scorecard
  (outages over 10 minutes, median alarm to closed, repeats within 24 h; branch to push
  approved, first-time tester passes). Each lead **reports up**: "State of my area" daily at
  the owner's 18:00 (`report_area`, kept as a report and never work on the board) and
  "Patterns" weekly. The CEO's review and planning are rebuilt on strategy, reports, measures,
  proposals and the budget split. **Stuck work climbs a ladder** instead of landing on the
  owner's desk: the area's lead decides (`decide_stuck`), then a **huddle** of the lead and up
  to four who touched it gives one view each (`huddle_view`) and the lead decides, then the CEO
  only when the lead says so; work sent back three times goes to a huddle. A project's lead
  closes its own work on its own acceptance. New tables made if missing: `strategies`,
  `area_reports`, `report_tasks`. Scenarios in `domains/team/features/roles.feature`.
- **The open office** ([ADR 0119](docs/adr/0119-the-open-office.md)). HQ's office is an
  open-plan floor: one pod of desks per team, side by side with no walls and labelled as the
  roster labels them, laid out from the snapshot (a hire grows a pod, a third lead makes a third;
  a member on furlough keeps an empty desk with an "away" card); Mara keeps her own office with
  the door open; a kitchen and lounge replace the engineering wing's walled room. People walk
  round desks and furniture and through the doors, visit another team's pod for its work (the
  tester goes to whose task it is), and drift about the office when idle. What stands on the
  floor is **data**: an `office_items` table of plants, lamps, couches and the rest, seeded once,
  drawn by the screen from the snapshot, and moved by the team. Every role has a team tool,
  `office`, to propose up to five changes, vote and look; the placement rules (inside the floor,
  never on a wall, door, desk, walking lane, room or another item; 40 things, 2 fish tanks) are
  pure and checked when a proposal is made and again when it carries (two yes, or six quiet
  hours; dropped on more no, or after 48 hours). The owner can undo the last change, reset the
  office or veto any open proposal from a new Office card. **Downtime**: a member with nothing to
  do is given a turn about no task, once a day, one at a time, to tidy the office or its notebook
  — free on the house's model, and never on Claude unless the owner sets
  `downtime_paid_cents_per_day`, which the pool pays, never a member's share. A downtime turn
  makes no work. The owner's scenarios are in `domains/team/features/the-office.feature`.
- **Claude Code as a developer runtime** ([ADR 0117](docs/adr/0117-claude-code-as-a-developer-runtime.md)).
  A member setting, `runtime`: `worker` (the default) or `claude-code`, offered only to a
  developer on Claude and refused otherwise; the owner sets it with
  `POST /v1/members/{id}/runtime` (through the node's `/v1/hq/members/{id}/runtime`), and HQ
  seeds the pilot once: Rae on Claude Code, Zed on the worker. In the workbench the worker does
  HQ's bookkeeping, checks the fork out, then runs `claude -p` headless with the member's brain
  and effort, `dontAsk`, no prompts, no settings but the image's, one MCP server (the worker
  itself, `endora-worker --mcp`, carrying HQ's tools and the node's contracts) and an explicit
  tool list (files, and Bash for git and the lab's checks); Claude Code reaches Claude only
  through the node's relay with the turn's token as Bearer, never a subscription login. Its
  result is the last word. Its own tools' calls are posted by a hook (`endora-worker --receipt`)
  to the node's new `POST /team-relay/receipt` and kept as **sandbox-reported** receipts, which
  may be cited but never stand for a live reading. The relay passes a Claude Code turn's own
  request shape and betas, still holding it to the member's brain, the turn's money, the
  five-minute cache (1h rewritten) and the fence. The workbench image carries Claude Code
  2.1.296 (digest-pinned, fetched at build time) and its managed settings (no updates,
  telemetry, error reports or nonessential traffic; the receipt hook). The scorecard gives each
  member's finished tasks and their cost, the pilot's measure. Built and run offline in the
  arm64 image against a fake HQ, node and relay; not yet against Anthropic's API or on the Pi.
- **Elastic staffing and notebooks** ([ADR 0118](docs/adr/0118-elastic-staffing-and-notebooks.md)).
  The team grows and shrinks with its work, and nobody is fired. A member on **furlough** is
  never woken, holds no task, is left out of every briefing's team and of the share arithmetic,
  and keeps its record, name and notebook; the owner furloughs or recalls any member but the CEO
  from its panel on the roster. Once a day HQ offers the owner a member with a week of nothing
  to do (furlough or keep), and a lead with a backlog asks for staff with `request_staff` on
  its project's open tasks: a furloughed developer is brought back first, else it becomes the
  hire request, under the wing's seats. The scorecard counts each team's seats, at work and on
  furlough. Each member also keeps a **notebook**: `note` (on evidence from HQ's record, at most
  300 characters), `forget` (with a reason), `read_notebook` and `notebook_checked` for every
  role; the notes lead its briefing (the newest twelve, stale ones marked after 30 days unused,
  a tidy asked after a week; the CEO sees only the count), and the owner reads, adds, edits and
  deletes any member's from its panel. New nullable columns on `members` and a `notes` table,
  added when HQ opens its store. Scenarios in
  `domains/team/features/staffing-and-notebooks.feature`.
- **Replay, on the HQ screen**: tap a run line, or "Replay this task" on a task card, to read a
  turn back in full: why it was woken, what it was told (folded), every call with its receipt
  and excerpt (Confidential ones too, on the owner's screen), every post with what it cited and
  the unsupported mark, its claims, status changes and desk items, and its money and tokens; a
  task's turns step through in order ("run 3 of 6"). Served by HQ as `GET /v1/runs/{id}/replay`
  and `GET /v1/tasks/{id}/replay`, through the node as `/v1/hq/runs/{id}/replay` and
  `/v1/hq/tasks/{id}/replay`, which only a full credential may read. Every event a turn's call
  emits now names its turn (`"run"`), so a turn's own events are found by id; a turn from before
  is matched by its member, task and time, and says so.
- **The team's standards** ([ADR 0116](docs/adr/0116-the-teams-standards.md), proposed).
  [docs/standards.md](docs/standards.md) holds the owner's global engineering standards: the
  base principles (test first, behaviour in the owner's words, SOLID, abstract only when
  needed, domain first, small slices, and the rules Endora already lived by), how the root and
  nested `AGENTS.md` layer on them, and each project at a glance. Endora's and the terminal's
  `AGENTS.md` point to it. HQ embeds it, and the drafts in `docs/drafts/`, in its library, and
  `read_doc` is now every member's; every briefing names the standards once (the CEO's
  `read_doc` description does, its quiet briefing being held short). The wing is told the
  fork's own `AGENTS.md` is the repo's rules, and its lead that the draft
  [docs/drafts/ansible-nas-AGENTS.md](docs/drafts/ansible-nas-AGENTS.md) is the first branch
  to propose.
- **The engineering wing and the git flow** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).
  A second team works on the owner's public ansible-nas fork through branches. The node keeps
  a bare mirror of the fork and offers a lead or a developer two contracts: `repo_checkout`
  hands the sandbox a bundle of the fork's base branch (`ENDORA_ANSIBLE_NAS_REF`, else the
  fork's default branch asked of GitHub once; compose no longer defaults to `master`), which
  the worker unpacks into `/work/repo` with git set up (commits authored Endora HQ, the trailer
  added by a hook, no remote); `repo_submit` takes a bundle of one `hq/task-N-<slug>` branch
  only when it sits on the base, changes at most 200 files and 1 MB, every commit is HQ's and
  nothing of the owner's values (`group_vars/*.yml`, `host_vars`, inventories) is touched. HQ
  writes the branch's diff as a document on the task and queues a model-free **check job** for
  the Pi's runner (the workbench image with ansible-core, ansible-lint 6.14.6 and yamllint
  1.30.0 baked in): syntax check, `ansible-lint --offline`, `yamllint`, five minutes each,
  reported with the job's own token as a receipt and the task's gate; a failing check sends the
  work back. Once the tester passes and the lead accepts, the owner's desk offers the push: on
  Approve the node pushes the stored branch to the fork over SSH with a deploy key
  (`ENDORA_GIT_PUSH_KEY`), `hq/*` only, never force, never main; without a key the card offers
  the bundle to download. HQ opens no pull request and merges nothing. The owner hires on the
  HQ screen (one CEO); a lead may request a hire within the wing's seats (one Sonnet lead, at
  most five Haiku developers), on evidence. HQ seeds the wing once: the goal "Improve the lab",
  the project "ansible-nas clean-up", Kit, Rae and Zed, and Kit's first task. The office gains
  the wing's room; the owner's scenarios in `domains/team/features/the-engineering-wing.feature`.

- **How the team is doing**: a scorecard on the HQ screen, today beside the last seven days by
  the owner's days, computed by code from HQ's own record (`application::scorecard`, served in
  the snapshot): turns and the share that showed nothing (talk only or empty), per member with
  each one's spend; work sent back against the submissions; the cost of a finished task and
  what dropped tasks spent (waste); alarms, the tasks they made, the median time to close one
  and how many stay open; what reached the desk by kind, and what still waits; and the prompt
  cache's share of the paid turns' input. Turns from before labels are counted as unlabelled,
  never guessed, and a window the kept record does not cover says where it starts. The
  owner's scenarios in `domains/team/features/how-the-team-is-doing.feature`.
- **Evidence, acceptance criteria and the tester** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)).
  The node records every call a turn makes of Endora's tools as a **receipt** in HQ, with the
  turn's own token (`POST /v1/team/receipt`; refused for a forged or finished turn), and the model
  reads its id ("receipt r41") to cite. Tasks carry **acceptance criteria** (required for work a
  member hands another; the home lab's checks get two live ones), and **one evidence rule**
  decides every claim from HQ's record: per criterion, references that exist and belong to this
  work, a live one a reading from the same turn. Work goes member → **tester** (the reviewer,
  shown as "Tester", re-checking on her own readings) → lead (`accept`) → done, nobody checking
  its own, with the fallbacks said on the task; three send-backs and the next round is the
  owner's. The CEO's `decide_project`, its decisions for the board and the lead's question up to
  it are refused without evidence; a post citing nothing in a turn with no reading is marked
  unsupported. Each turn ends with a **label** (advanced, blocked, checked, talk only, empty)
  that the back-off reads; two nudges, then a task that shows nothing goes up the chain once; the
  same reading again shows nothing; a check-back on a task blocked on a wait is replaced by the
  wait; and the chain of command may close, accept, send back or hand on (`hand_to`) a
  subordinate's task. The HQ screen shows criteria with their evidence, the tester's pass, run
  labels and unsupported posts. The live store gains its columns and tables by guarded
  `ALTER TABLE`.
- **The owner's scenarios** ([docs/testing.md](docs/testing.md)). Behaviour the owner decided,
  written in the owner's words as `.feature` files and run against the real engine and the
  bridge through their public use cases, with cucumber-rs (a dev-dependency). The pilot has 14:
  one task a problem, coming back is news, the owner's devices are never the team's, a member is
  told its tools are its own, only the CEO reaches the board, a project's cap is the owner's,
  local members work for free, and more from ADRs 0107, 0108, 0111, 0112 and 0113. They run with
  `cargo test --workspace`, so the gate runs them.
- **Projects, budgets and reports_to** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md)).
  A project serves one goal and has a lead (a lead or the CEO) and, if the owner gives it one, a
  monthly cap; a task may be in a project of its goal, its sub-tasks stay in it, and a routine's
  tasks go in the routine's. A Claude turn on a capped project reserves no more than is left of
  it (so the relay's per-call limit holds it), and the caps together stay within the pool's
  month. At 80% the owner gets one notice a month and the CEO is told; at 100% the project's
  Claude turns wait (members on the house's model go on) and the desk offers "raise the cap" or
  "wait for next month", which lifts itself when the month turns; the owner's own pause holds
  every turn until resumed. Each member reports to someone (set at hire, backfilled on start, the
  owner's to change, never to itself or in a circle), and a question goes to that manager or
  past a paused one. A task handed back wakes its project's lead. Leads and the CEO propose
  projects (`propose_project`) with task numbers and event ids HQ checks; the CEO decides
  (`decide_project`), the board may overrule, and a cap asked for goes to the owner's desk with
  the CEO's recommendation; the CEO proposes goals (`propose_goal`). A weekly **Planning**
  routine briefs the CEO with patterns: projects and their money, the month's tasks by the
  containers they named, the alarms that came back, proposals waiting, goals with no project.
  On start, the live store gains its new columns by guarded `ALTER TABLE`, and the home lab's
  routines and open work go, once, into an active "Home lab upkeep" project. The HQ screen
  gains a Projects card with its forms, the cap and budget items on the desk, and "reports to"
  on each member's card.

- **The team knows how the lab is built** ([ADR 0112](docs/adr/0112-the-team-knows-how-the-lab-is-built.md)).
  A new read contract, `homelab_map`, says for every container its image, state, restart
  policy, who manages it (an ansible-nas role, Endora, or unknown), what it depends on and what
  depends on it ("gluetun → deluge, nzbget, prowlarr (run in its network)"), from Docker's own
  metadata, names and wiring only; the Docker observer gains one fixed read-only route for it
  (`/containers/<name>/json`, a strict name, a handful of fields passed on). A second,
  `ansible_nas_doc`, reads the lab's ansible-nas docs from the owner's public fork on GitHub
  (`ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF`), only `docs/`, `website/docs/` and role
  READMEs, by an allowlist. Both go to every role on any brain. HQ seeds the CEO's house log
  once, when it is empty, with how the lab is built, and the worker's instructions say it too.
- **Coming back is news too** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)).
  When a container or service that alarmed within the day is running and healthy again, the
  node tells HQ once (`POST /v1/alarms/recovered`), and HQ posts "Recovered: deluge is back up
  at 13:43" in the alarm's open task, waking whoever has it. The worker's rules say an alarm is
  a moment: check `homelab_status` before acting, and close the task if everything recovered.
- **One task a problem** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)). A
  member's new task is refused when an open task under the same goal is already about the same
  home-lab containers, naming it: "#664 already covers this (deluge, nzbget, prowlarr): add to
  it with post in #664, rather than a second task".
- **Cheaper cloud turns** ([ADR 0109's cost notes](docs/adr/0109-a-brain-per-member.md#cost-notes)).
  Each of the worker's rounds on Claude asks for Anthropic's automatic prompt cache, so a round
  reads the round before's prompt at the cache price and writes only what is new; the prefix is
  byte-stable (instructions once, tools by name, the briefing first, rounds appended). HQ names
  each cloud turn's **effort** (CEO `medium`, `high` when it weighs a decision; lead `medium`;
  researcher `low`), passed by the runner as `ENDORA_EFFORT`. The relay passes the ephemeral
  cache and the five efforts and still refuses the rest, and reckons what the turn's last call
  cached at the cache-read price before a call. The **CEO's briefing** carries the whole house
  only on its first turn and daily review, and otherwise what changed since its last turn. A
  run's record keeps its input tokens, and the activity log says "cached: N% of input".
- **HQ's progress and waits, checked against the design it borrows.** A turn that only
  re-claims its own task and leaves the worker's last word is no longer counted as progress,
  so the empty-turn back-off finally holds such turns back (the worker claims before every
  turn, which had made every turn look productive). A task waiting on one that was dropped
  is no longer released as if the wait were done: its member is told why in the thread and
  woken once, then it goes to the desk. The plan no longer promises per-task session resume,
  which was never built.
- **The CEO and the chain of command** ([ADR 0111](docs/adr/0111-the-ceo-and-the-chain-of-command.md),
  owner's decisions of 2026-10-17). The owner is the board. A new top member, the **CEO**, sits
  above the lead: HQ seats one once on an existing team ("Mara", on Claude Opus, renameable) with
  a "Daily review" routine every 24 hours. While a CEO is at work, a member's `ask_owner` goes up
  the chain in the task's thread ("Question for Ada: …"): the developer's, researcher's and
  reviewer's to the lead, the lead's to the CEO; the answer in the thread wakes the asker. Only
  the CEO's `ask_owner` reaches the board's desk, as a decision (`decision`, `recommendation`,
  `why`), checked by the reviewer first; the card offers Approve, Decline and an answer. The CEO
  wakes only for escalations and its own work, keeps a **house log** (`note_house`,
  `rewrite_house`; read by the CEO and the lead every turn, edited by the owner on a new House
  log card), reads Endora's public records and plans (`read_doc`, embedded in HQ at build time),
  and is briefed on its team, goals and lessons, open work, the week's alarms and the board's
  desk. With no CEO, the team works as before.
- **Brains and money by role**: Opus is the CEO's alone (HQ and the relay refuse it to anyone
  else); the CEO's turn may spend $1.00, everyone else's $0.40. The day's pool is shared only
  among members with a paid brain, by their roles' weights (CEO 35, lead 25, developer 40,
  reviewer 20, researcher 15); a member on the house's model costs nothing and is never turned
  away for money. The cloud CEO is given nothing Confidential and no act on the house.
- **HQ's team works as a team.** The lead's briefing names the team (each member's role and
  brain) and says to plan and hand work on: fixes to the developer, look-ups to the researcher,
  the reviewer checking. A cloud turn refused a Confidential step is told to hand it to a member
  on the house's model. Work left in review with no review coming (moved there before the
  reviewer path existed) wakes the reviewer after ten minutes, once.
- **The team's rings, ring 1: a sandbox on the Pi** ([ADR 0110](docs/adr/0110-the-teams-rings.md),
  owner's decisions of 2026-10-17). The node's team listener (8788) is published on the house's
  network, every route on it needing a token, and serves the **team gateway**:
  `/team-gateway/mcp` and `/team-gateway/runner/...` pass a worker's or a runner's request on to
  HQ's `/v1/team/mcp` and `/v1/runner/...` with its method, body and own `Authorization`
  untouched, to HQ's URL and nowhere else, capped in size and time. HQ stays unpublished. A
  runner whose `HQ_URL` is the gateway asks for turns there and starts each as an offline
  **workbench** on the Pi's `endora_sandbox` network (`ENDORA_WORKBENCH=1`, a `/work` in memory,
  more room), reaching HQ and the model relays only through the gateway. The NAS's runner is
  unchanged. The ADR records the four layers against leaks (no keys in the sandbox, one exit
  for writes, exact-match blocking with variants, canaries), why machine-learning detection of
  personal data is skipped, and the egress proxy as designed and deferred.
- **The workbench (ring 1)**: a Dockerfile stage `workbench` (`endora-workbench:latest`, built
  with `--target workbench`, never the node's image), a disposable sandbox with bash, git,
  curl, python3 and a few tools, run as an unprivileged user with `/work` as its writable home.
  There the worker offers the model a `shell` tool (only under `ENDORA_WORKBENCH=1`): `bash -lc`
  in `/work`, 60 s unless told and at most 300, output kept to 16 KB head and tail, the exit
  code said, each call listed in the turn's line. The sandbox is offline (the egress proxy is
  deferred): the house's services and the models are reached only through the team's tools.
- **The dev node, the Pi that runs the team's sandboxes** (ADR 0110): the `endora_dev_node`
  Ansible role (Docker CE from Docker's repository with capped logs and live-restore, a ufw
  firewall that lets only SSH in from the LAN and checks it cannot lock you out, the journal and
  `/tmp` in memory, and the `endora_sandbox` network locked by `docker_egress` to the NAS's
  team gateway alone), run by `deploy/ansible/pi.yml`; `make deploy-pi` builds the arm64 node
  and workbench images on the Mac, copies it over SSH and starts the runner, and `make pi-runner-token`
  copies the runner's token from HQ once.
- **A brain per member, and what a cloud turn may read** ([ADR 0109](docs/adr/0109-a-brain-per-member.md),
  owner's decisions of 2026-10-17). Each member of HQ's team has a brain, local by default or a
  Claude its role may use (Sonnet or Haiku for the lead, developer and reviewer; Haiku for the
  researcher; Opus refused), set on the member's card on the HQ screen ("Brain: Local / Claude
  Sonnet / Claude Haiku") through the node and recorded in the audit log. The runner gives a
  cloud member's worker the Anthropic relay on the same team listener, and the worker speaks
  Anthropic's Messages API through the same loop as the local one. HQ tells the node such a
  turn is cloud; the lead's cloud turns may read scrubbed container logs and the Sonarr and
  Radarr queues, every other Confidential tool stays unlisted and refused for a cloud turn
  (`container_logs` is now checked too), and the Anthropic relay refuses any turn HQ does not
  say is cloud.
- **Lessons kept by the team**: the lead and the reviewer have `note_lesson`, which appends one
  line ("- <lesson> (Ada, 17 Oct)", at most 300 characters) to the task's goal's notes for the
  team, refused when the notes are full. Each is a `lesson.noted` event and a notice on the
  owner's desk ("Ada noted: …"). The instructions say when to note one.
- **The harm rule**: the worker's instructions say to report something as a problem only when
  a tool shows it causing harm, and the reviewer's briefing to send back any problem no tool
  shows causing harm.

- **The reviewer before done and before the owner's desk** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md),
  plan §5 and §6). Work moved to `in_review` wakes the team's reviewer (`review_requested`);
  with no reviewer it is on the desk as ready to read, with its thread; its owner takes no turn
  on it meanwhile, except for the owner's word or a send-back. The reviewer's new `review`
  tool approves (closing work that needs no gate: a routine's task, or the home lab's work)
  or sends back (to `doing`, waking its owner), never its own. A question about the home lab
  from anyone but the reviewer becomes a "Draft for the owner" in the thread for the reviewer
  to check first. Every question reaching the desk lists the names in it the home lab does
  not have ("Not found in Endora's home lab: …"), against the containers the node now tells
  HQ (`POST /v1/lab/names`, signed). The worker's instructions say a home-lab question is
  checked first.

- **A turn's claims sit beside what it did** ([ADR 0106](docs/adr/0106-team-contracts.md)):
  the worker writes the calls its turn actually made, and how each came back, under its last
  word in the thread ("This turn did: …", "This turn did nothing but read: …", "This turn made
  no tool calls.") and under any question to the owner ("What I actually did this turn: …").
  A question to the owner may now be 1,200 characters, to leave room for it.

- **A Thinks switch on each model-list entry** ([ADR 0105](docs/adr/0105-models-in-ordered-lists-by-data-tier.md)).
  Off by default: the butler sends `"reasoning_effort": "none"` to a local entry that may not
  think, which takes a local `qwen3:8b` from about 10 seconds to under one for a tool call.
  On, nothing is sent; a cloud entry never gets the field. The team's local relay always lets
  the model think, and takes the field off a worker's call.

- **Stranded assigned work is recovered**: an open task with a member and nothing queued,
  running or held for it for ten minutes gets one `assignment_recovery` wake for that member;
  still stranded ten minutes later, it is on the owner's desk as stuck, once, and never handed
  to anyone else.
- **The bridge: Endora's capabilities as the team's tools** ([ADR 0108](docs/adr/0108-the-bridge.md)).
  The team's contracts list now carries, after the container tools, every capability and
  tool-server tool Endora's policy allows, each marked read or act by Endora's own knowledge
  (a tool server's tool reads only when the owner named it the reader). Never offered: what is
  off, Restricted, Endora's administration, or the owner's mail, chat, brain, brief, image and
  whereabouts. Confidential results go only to turns on a local model; lead and developers
  read and act, reviewers and researchers read. An act that is on runs at once, three an hour
  a tool, and is a notice on the owner's desk; an act that asks becomes an **approval** on the
  desk showing exactly what will run, the member's turn ends, and Approve runs the stored call
  once (Decline, or a day's wait, runs nothing). Every call is in the audit log.
- **Stuck downloads**: Sonarr's and Radarr's queues say why an item is stuck, and two actions
  join them, for the butler and the team: retry the imports (on by default) and remove,
  blocklist and search again (asks by default). Another app of the same API is a row of
  configuration.
- **The owner's reply on any task** wakes whoever has it at once, mention or not, past the
  back-off; and **a goal's notes for the team** (at most 2,000 characters, edited on the HQ
  screen) reach the top of every briefing under it.

- **Routines, alarms and check-backs: how the team keeps working**
  ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)). Goals stay passive and work
  arrives as tasks. A routine hands one member a task on a rhythm (one open at a time; a late
  tick fires once; every firing, skip and post in HQ's feed; never past the kill switch, a
  pause, the budget or a goal that is not active), and its member closes the check once taken.
  The node's home-lab observer raises an alarm when a container or service goes from well to
  trouble (new trouble only, never Endora's own, one an hour), which HQ's alarm routine turns
  into a task or a post in its open one. A member can `check_back` on its task in 5 to 720
  minutes. A clock with nothing to do starts no turn, and a task whose turns move nothing backs
  off its stale wakes from 2 to 30 minutes. HQ's screen has a Routines card (rhythm, last run,
  on/off, run now, add one), and HQ makes "Check the home lab", every 6 hours for the lead,
  once on first start with the demo off.

- **The team's contracts on the home lab** ([ADR 0106](docs/adr/0106-team-contracts.md)). A
  turn sees the home lab (`homelab_status`), reads a container's log scrubbed of secrets
  (`container_logs`) and restarts a container within limits (`restart_container`: one an hour
  each, three in all, never Endora's own, never what a restart will not fix), all decided by
  the node and in its audit log; a restart lands on the owner's desk as a notice. The owner
  switches the restart, or every contract, off on the HQ screen. With the demo off, HQ puts the
  demo's goals and open work away on start.

- **Endora HQ's real workers** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 4). The team
  protocol over MCP, opened by a turn's own token; the node's model relays (Anthropic with the
  owner's key, capped and charged before and during each call; the local model at no cost);
  `endora-worker`, Endora's own Rust worker, which claims its task before the model's first
  round and answers a repeated call from what the turn already has; `endora-runner`, which
  starts each turn as a fresh, read-only, capped container on a fenced internal network that
  reaches only HQ and the node's team listener; and leases, so a report from an earlier attempt
  of a turn ends nothing.

- **Endora HQ, the service and the office** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 2).
  `endora-hq` runs the team in its own container (Compose profile `hq`, from the node's image),
  with its own database, ticking every 30 seconds, and answers only the node, which signs every
  request with a token HQ makes for itself in a volume only the two share. The console's new
  **HQ** screen draws the team's office, phone first: members walk to their desks when a turn
  starts, wakes fly from the server closet, the board has a sticky note per task, the burn gauge
  is today's spend, the lever is the kill switch. Tap anyone or any note for its detail and its
  chain of why. Below are your desk (approve or decline a proposed goal, try stuck work again or
  drop it, mark finished work merged), the activity, and the team's budgets; you can set the
  team a goal. Every decision and the kill switch go through the node, which records them in
  Endora's audit log. Until real workers exist (phases 3 and 4) a scripted demo team works on
  the simulated runner, labelled as such. Office art: Pixel Agents (MIT) and MetroCity by
  JIK-A-4 (CC0), with its credits and licence served beside it.

- **Endora HQ's team core** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 1 of
  [its spec](docs/plans/endora-hq.md)): `domains/team`, with no models yet. A team works
  towards goals through a tree of tasks; a task has one owner at a time, taken in one atomic
  step; every turn starts with a wake that leaves a receipt (queued, folded into the member's
  own turn, held behind another member's, refused and why, started, finished). The tick
  starts queued turns within each member's slots, the runner's capacity and the day's budget,
  retries a vanished turn twice a minute apart, stops one that runs too long, nudges stalled
  work once and then puts it on the owner's desk. Money is an append-only ledger: reserved
  before a turn, used during it, and a turn that crosses what it reserved is stopped then.
  Done needs the gate and a review by a different member. The kill switch stops every turn
  and turns every wake away until the owner resumes. A simulated runner drives the tests.

- **The brain** ([ADR 0101](docs/adr/0101-the-brain.md)). What Endora knows about you is now a
  folder of markdown pages with its own git history: `memory.md` (what you told it and what it
  knows best, read on every turn), an index, a log, and pages it creates and organises itself,
  each with a summary on top and a dated timeline below.
  - After a conversation it notes what lasts onto the page it belongs on, from your own
    messages only, and says so in Activity. In quiet time the local model writes a grown
    page's summary again, and what it knows best.
  - About you shows what you told it, what it knows best and its pages; a page opens, edits
    as text, shows its history, and can be deleted. **Forget** removes every page and its
    history and keeps what you told it.
  - The model reads memory and the page list every turn, and opens a page with the new
    `read_page` skill when it needs the detail.
  - `ENDORA_BRAIN_DIR` in `local.mk` (or the role's `endora_brain_dir`, by default beside
    Endora's other folders) puts it in a host folder you can open. The nightly backup writes it
    as a git bundle. Edits made outside Endora are committed as yours first.
  - New routes: `GET /v1/brain`, `GET`/`PUT`/`DELETE /v1/brain/pages/{path}`,
    `POST /v1/brain/forget`. The CLI has `brain` and `brain page`.

- **Spotify, connected once in Endora** (Settings → Connections → Spotify). Endora holds the
  sign-in and lends an hour's access token to whichever device asks; the refresh token never
  leaves the node, so two devices cannot knock each other's sign-in out when Spotify rotates it.
  A sign-in started on one device shows as waiting on the other and can be finished there: by
  approving on the phone (Spotify sends you back to Endora, which finishes by itself) or by
  pasting the address Spotify sends you to. A device's existing sign-in can be handed over.
  The handheld's Music page now plays with it: it borrows the token through the unlocked
  session, hands over a sign-in it made by itself and then deletes its copy, and its setup
  starts and finishes the sign-in on Endora. Locking the handheld drops the borrowed session.
- **Pi-hole and UniFi connections** (Settings → Connections → Home lab). They hold the
  credentials for what Endora will read about your network. Each has a Test button that
  signs in and signs out again. Pi-hole's app password is only ever used to read. UniFi
  takes a View Only local admin, and its self-made certificate is not checked.
- **The web console does what the handheld does.** Music (what is playing, play and pause,
  skip, volume, choose the device, search, playlists, through the borrowed token), Smart home
  (switch favorites, set scenes, choose favorites), Media (search and request, recent requests)
  and Home lab (NAS figures, alerts, each service's state, container start/stop/restart and
  logs, allowlisted jobs). Each acts through the node's one allowlisted action route and asks
  before anything consequential.

- **The handheld terminal is part of Endora** ([ADR 0099](docs/adr/0099-the-handheld-is-part-of-endora.md)).
  `clients/terminal`, the Rust TUI for a small Linux handheld, joins the workspace: one
  lockfile, one CI (its Python, privacy and rendering checks, and an ARM64 bundle artifact),
  Apache-2.0. It still talks to the node only over HTTP.
- **Endora's Ansible roles ship with Endora** as the `endora.homelab` collection
  (`deploy/ansible/collection`): the node, the mail mirror, locked-down container networks and
  the host job agent, which now installs itself from the collection. An ansible-nas checkout pins
  it; CI checks every role.
- **A shared wire protocol** (`shared/protocol`). The handheld's pages, home screen, action
  outcomes and chat stream events are written down once; the node refuses to send a response
  that does not fit, and both sides' tests fail if either drifts. The handheld now shows what
  Endora is doing while it works, and why a reply failed, which it used to drop.

- **An AI screen.** Settings → AI (it was Models) opens with what Endora runs: the conversation
  model (or the router and writer), the deep model with its calls and cost this month, the mail
  reader, vision and speech-to-text. Each says which model, where it runs (on your network or
  in the cloud), what the model server says about it now (graphics card or processor, memory,
  context, size) and what it sees. It warns when the model server gives less context than
  Endora assumes, which silently cuts long turns short. The mail reader's model, until now
  set only in the deployment, is chosen there; the mail reader always stays on the local
  endpoint, whatever the conversation uses.

- **Endora copies its own data every night** ([ADR 0098](docs/adr/0098-it-can-be-rebuilt.md)).
  At 01:30 by your clock it writes a consistent, integrity-checked copy of its database into
  `backups/` beside it, daily and weekly, keeping seven of each, so the house's backup always
  finds a whole copy rather than a live file. Activity says each night that it was saved, and
  says so once a day if it failed. [Rebuilding from nothing](docs/recovery.md) is the runbook.

### Changed

- **The World card is easy to edit on a phone.** A "Paste Markdown" box takes a whole document
  (`## house / key — Title` starts a section, `## timeline` bullets `- 17 Oct 2026: text` are
  timeline entries), shows what each change would do with a tick to leave any out, and one
  Apply sends them, sections first, saying how each went. Every section has an Edit button
  that opens a full-screen editor with Save and Cancel always at the top; the owner can edit a
  member's section too, and the member's text stays in its revisions. Buttons are at least 44
  pixels tall and the text boxes use 16-pixel type so a phone does not zoom. No new route.

- **Borrowing is capped at one extra share, and a member at the cap asks the owner**
  ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md), amended again). A paid member
  may spend while its spend today is below its share plus the lesser of its share and what the
  other paid members have not used; the owner's own requests stay outside it, and the pool's
  day is still the ceiling. The first time a member is turned away at that limit while the
  pool's day has money, one "out for today" item goes on the owner's desk (once a member a day):
  give it an allowance (an amount on the card, at most the pool's day left, $0.50 as offered)
  until the owner's next day starts, or wait for tomorrow.
- **A member's share is a first claim, not a wall; the owner's own requests are the pool's;
  and shares are weighted by brain price** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md),
  amended). Past its daily share a paid member draws on what the other paid members have not
  used today, the pool's day staying the hard ceiling (before, one member at its share blocked
  every task waiting on it until midnight). A turn that exists because the owner asked (their
  task, reply, desk answer or decision, "Run now") is marked `asked_by_owner`, its ledger rows
  flagged, and counted against the pool's day only. A member's claim on the pool is its role's
  weight times its brain's price class (Opus 1000, Sonnet 500, Haiku 25 thousandths, from the
  relay's prices, which a test holds them to), so Haiku hires no longer dilute a Sonnet lead.
- **A question on a project's task goes to the project's lead** ([ADR 0111](docs/adr/0111-the-ceo-and-the-chain-of-command.md),
  amended), when at work and not the asker; otherwise up the asker's chain as before.
- **The tester passes or sends back, and never makes work** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md),
  amended): her briefing says so, and `create_task` is no longer in her tools.
- **The Team card is an org chart**: the board and the CEO with whoever has no lead, then one
  group per lead (Engineering for the wing's, Ops otherwise), one line per member (role, brain,
  what it is on, today's spend against its share), its controls opened by tapping the line. A
  task's turns read oldest first, as its thread does, the last five with a toggle for all.
- **The CEO's effort follows the kind of turn** ([ADR 0109](docs/adr/0109-a-brain-per-member.md#cost-notes),
  [0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)): `low` for ordinary turns
  (a question up the chain, a post, a recovery), `medium` for its daily review and planning,
  `high` when a project proposal waits for it or it carries something to the board. The lead
  stays `medium`, the researcher `low`. Posts alone no longer count as progress for the
  empty-turn back-off.

- **Sonnet 5.5's cache reads are priced at $0.10 a million**, down from $0.20, after
  Anthropic's correction: the team relay charges a turn's cached prompt at the new price. The
  other models are unchanged.
- **Skills say what they do, for people.** A skill whose description is written for the model
  ("Use this for any question about what YOU did", "Civic business ONLY", "fetch useful URLs
  with web_fetch") shows a line for people on the settings screen instead (`for_people`, the
  API's `summary`); what the model reads is unchanged.
- **Connections is short too.** Tool servers are rows that open to their own screen (tools,
  test, edit, remove); adding one is folded, and so is the Home Assistant "connect something"
  wizard until it is wanted.
- **Your data** is its own screen: export, with what it does and does not hold, and delete
  everything. They were loose at the foot of Settings.
- **Skills is a list, Connections and Permissions are their own.** Skills was one page about
  twelve phone screens long: the autonomy switches, a Home Assistant wizard, every skill with
  its setup form open, and tool servers. Now Skills lists what chat can do in four groups (Your
  home, Looking things up, Home lab, About Endora), each row with one status: On, Off, Needs
  setup, or **Needs a connection**, and each opening to its own screen. **Connections** holds
  where skills get their data: Home Assistant and its wizard, the home lab's services (named
  for themselves: Portainer, Docker, Sonarr, Radarr, Prometheus, Homepage, Overseerr; they were
  "Handheld: containers", "Overview: Sonarr" and so on), and tool servers. **Permissions** holds
  how far Endora may act on its own.

### Removed

- **The server-side speech-to-text path.** `POST /v1/transcribe`, its Whisper proxy and
  silence filter, the AI screen's speech-server choice, `ENDORA_STT_URL` and the `stt` flag in
  `/health` are gone. Nothing had called them since push-to-talk was removed; dictation is the
  browser's own recognition, and the AI screen says where that audio may go.

- **Beliefs** ([ADR 0101](docs/adr/0101-the-brain.md)), with their confidence, decay,
  affirming and correcting. **Breaking:** the `/v1/understanding` routes are gone, the export
  has `pages` and no `beliefs`, and the context diagnostics report `memory` and `brain_pages`
  instead of `understands`. What you had told Endora moves from the database into memory on
  the first start, and the `beliefs` and `preferences` tables are dropped. The preference
  routes keep their shape.
- **The clean slate** ([ADR 0100](docs/adr/0100-the-clean-slate.md)). What Endora did without
  being asked had become noise, and it is gone, code and storage, so the always-working butler
  can be built again on real signals:
  - the daily check-in, and waking early on an unusual change;
  - the nightly loop, notions ("still wondering"), and the replay of failed asks (specimens).
    A turn that could not answer no longer says it kept the question "to retry overnight";
  - intentions formed from beliefs, and their nudges;
  - the own-work lane and its `ENDORA_OWN_WORK` switch (in the Makefile, Compose file and the
    `endora` Ansible role too);
  - durable goals started from chat: the goal worker, the goal strip and its Stop button.
    Research you ask for stays, with its card and its report;
  - the 4 am model tune's schedule. The comparison still runs when started by hand;
  - Inbox. Messages Endora sends unasked stay in the chat, and Activity is the one record;
  - About you as a list of beliefs by kind to confirm. It now says how much Endora has learned
    and has a **Forget** button that deletes all of it; what you told it yourself stays.
    Beliefs that were passing remarks (an intent, or "other") are no longer formed;
  - device changes written to Activity by the heartbeat.

  **Breaking:** the routes `/v1/checkin`, `/v1/nightly-loop/schedule`,
  `/v1/model-tune/schedule`, `/v1/notions`, `/v1/intentions` and `/v1/goals` are gone, and the
  export has no `goals` field. An existing database drops the removed tables when the node
  starts; a table nothing reads is data nothing can export or delete.

### Fixed

- **The bench's refusals say whose they are** (ADR 0123, rule 5). The first bench run reached
  no model: every seat's first call came back "Credit balance is too low", which Claude Code
  shows for any 402 or `billing_error`. That 400 was Anthropic's own (its request id was in
  the body), passed on word for word, so the run's record could not say at which step or why.
  Anthropic's refusals now keep their status, kind and request id, with words naming the step
  and the model, and are kept in `GET /team-relay/bench` (`upstream_refusals`,
  `last_refusal`) and in the node's log. The bench's own refusals are a 400 starting `bench: `
  (never a 402), and calls in flight holding the rest of the day are a 429 with `retry-after`.
- **The team's world, from the owner's live export** ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)).
  The move of the house log dated a yearless day by comparing its noon with the move's instant,
  so a move in the early morning put the move's own day a year back; it now compares days, in
  the owner's calendar. It also kept the date a writer had put at the start of a line
  (`17 Oct: 2025-10-18: …`): those are taken off. A one-time repair (`world_dates_repaired`,
  with each row's old date and text kept in `world_timeline_repairs`) mends the rows already
  moved. The Markdown export lists the timeline newest first and says so, prints the lab page
  once under a clean heading (the heading had been built from the page's whole text, so its
  lines came out twice), and names roles as the screen does (CEO, Lead, Tester). Every
  briefing now says today's date with the year, and a year one out on a date that starts a
  member's note, house note, story or section, within three days of today, is corrected and
  recorded in the feed.
- **Time-bound criteria** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md)): a
  criterion may wait (`after`) from the work's last change; the lab's "no new alarm for the
  same containers since" waits 60 minutes, and closing or passing it before then is refused,
  "wait until HH:MM (owner's time)". Live, #667 was closed two minutes after a restart and the
  alarm came back.
- **Repeated readings**: a reading the turn before already took shows nothing and no longer
  resets the recovery nudge; no recovery wake while the member's check-back waits; a stale wake
  waits two minutes after the member's last turn on the task. Live, a lead took four turns ten
  seconds apart re-reading one status.
- **House log dates**: lines carry the year, and a date the writer put in the entry is taken
  off ("17 Oct: 2025-10-18: …").
- **Logs are no longer cut off.** The HQ feed shows a thread post whole (it was cut to 80
  characters with no sign of it; the event now names its post's place in the thread and says
  when its preview was cut), folds lines over 240 characters behind "more", and on a phone is a
  plain list in the page, not a 300px window. It shows thirty lines and "Show older" reveals
  the kept ones, then asks HQ for earlier events (`GET /v1/hq/events?before=&limit=`). The
  Activity screen has "Load more" (`GET /v1/activity?limit=&offset=`), and the home "Watching N
  things" line wraps to two lines instead of ending in an ellipsis.
- **Every server-side cut says it was cut.** One helper (`domain::cut::cut`) ends what it cuts
  in "…[cut]", character-safe and inside the same limit: a remote run's failure reason, a
  receipt's excerpt, arguments and criterion, and the question, approval, notice, answer and
  outcome texts. The limits are unchanged.
- **"Try again" on a stuck desk item no longer does nothing without a word.** It released the
  task and queued a wake that the kill switch, a pause, a furlough, the budget or a paused
  project refused, then marked the item decided. Now it checks first: if nobody can be woken it
  changes nothing, the item stays waiting, and the refusal's reason is said on the card (a
  refused desk decision stays visible there, not only in a toast).
- **A post in a turn with receipts is never "unsupported"**, nor the worker's automatic last
  word ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md), amended): a
  closing post in a turn with two receipts was tagged, because only receipts of calls that held
  were counted.
- **`make smoke` waits for the node**: up to 30 seconds for `/health` before its checks, so a
  run straight after `make deploy` no longer fails on a node that has been up for a second.
- **HQ's day is the owner's.** HQ counted its days from UTC midnight, so the scorecard's
  "today" emptied at eight in the evening and the ledger's days turned over then too. HQ now
  reads the same `ENDORA_TIME_ZONE` / `ENDORA_UTC_OFFSET_MINUTES` the node does.
- **The team never switches the owner's devices** ([ADR 0108](docs/adr/0108-the-bridge.md)).
  Every act of the tool server paired with Home Assistant (`HassTurnOff`, `HassTurnOn` and the
  rest) is kept off the bridge: live, the developer asked to turn off "deluge" to restart it.
  The server's named reader stays a read under the tier's rules.
- **A lead on Claude knows it can read the home lab.** It said three times its tools could not,
  though `homelab_status` was offered (it had read the lab with it minutes before): HQ's
  briefing listed only HQ's tools as "Your tools". The line is now "Your tools at HQ", with
  Endora's home-lab tools beside them, and the worker names the container tools the node
  offered as the turn's own.
- **`restart_container` never starts a stopped container** ([ADR 0106](docs/adr/0106-team-contracts.md)):
  Portainer's restart would start an exited one, and a container may be stopped on purpose
  (an old sound server whose ports the sound hub needs). Only a running container is
  restarted; any other is refused, costing no restart, and the member is told to ask the owner.
- HQ's board hides a dropped goal's tasks, so the retired demo's notes do not bury the owner's;
  a live update keeps the page's scroll and the activity log's; and the screen shows money
  spent apart from money held for turns, in dollars.
- The team's relays and contracts are served only on the team listener, never on the node's
  main port, and a contract refused for an unknown name or bad arguments is in the audit log.
- Deploys recompile only what changed and no longer send gigabytes of worktrees and build
  output to every build.

- **Endora no longer asks about devices that stopped answering** (the "Still yours?" card).
  The owner did not want it. It is removed with what was under it: the card and its routes,
  the morning brief's lines, the own-work lane's items, and the detection and its store. Work
  Endora started about such a device before is withdrawn at the next check; an existing
  database drops its old table when the node starts.

- **Endora answered faster, and about the right day.** Measured on the house's A2000, with qwen2.5:7b:
  - **The prompt was read from the clock onward on every message.** The date and time sat
    second in the request, ahead of the instructions and the tool list. They now travel with
    the latest message, so the model reuses the rest: the next message starts answering in
    about 0.6 s instead of about 5.5 s.
  - **Endora asked Ollama to unload its model after 30 minutes**, which overrode a server set
    to keep it loaded, so the first message after a pause paid a 13-second reload. It now asks
    to keep it loaded.
  - **The clock it was told was UTC.** It is now local time with its offset, so a summer
    evening is no longer "tomorrow".
  - **"What day is today?" went to the deep model.** The small model filed it as needing
    a lookup, though the date is in every request; date and time questions now stay local.
  - **One-off events tied to a day became beliefs.** "An appointment tomorrow at 6:30" was
    filed as something Endora understands about you, was still "tomorrow" the next day, and
    was trusted over the calendar. Statements tied to today, tomorrow, tonight or yesterday
    are no longer kept as understanding.

- **Opening Mail, AI or Spotify from the menu could show "Loading…"** until something else
  reloaded the console; a screen now fetches what it shows when it opens.

- **The handheld's Containers page showed raw numbers** ("CPU %: 7.251164760358932"); it shows
  the home screen's short figures.

- **The handheld's Home catalog never loaded when Home Assistant's address was typed without
  `http://`.** The Home Assistant skill accepted `host:8123`; the console's own requests
  refused it as a configuration error. Both read it the same way now.

- **The model settings answered "internal error" on any node older than the preferred
  endpoint.** Its two columns were added only by a migration that runs in tests, never in
  production, so `GET /v1/model-config` (the Models screen's everyday card, and the new AI
  screen) failed on every database created before it. Production now adds them, and a test
  fails if a column is ever added only by the test migration again.

- **A skill that rides a connection said "On" before the connection was set up.** The
  settings screen judged it by its own (empty) settings; chat, rightly, by the connection's.
  Both now use one rule (`settings_ready`), so mail shows as needing Home Assistant and the
  home-lab skills as needing their service until each is set up.

### Security

- **Secrets at rest in the node** ([ADR 0125](docs/adr/0125-secrets-at-rest-in-the-node.md)).
  The node's database held every credential in plaintext and its sessions as the tokens
  themselves, so any copy of the file (a nightly snapshot, the house's backup) handed over
  all of them. Now: provider API keys, MCP bearer tokens and environments, every capability
  setting (the Home Assistant token, the services' keys and passwords, Spotify's tokens), the
  authenticator secret and the bootstrap token are sealed with XChaCha20-Poly1305 under a
  data key (`enc:v1:<kid>:<nonce>:<ct>`), and the data key is wrapped by a key outside the
  database: `secrets.key` beside it (`ENDORA_SECRETS_KEY_FILE`), made `0600` on the first
  start. That start seals what was stored in plaintext, once, and `VACUUM`s so none survives
  in the file; a plaintext value found later is refused and logged by column, never by value.
  Sessions are kept only as SHA-256 digests (hashed in place on the first start, nobody
  signed out), and a console session also ends after seven days unused. `endora-node secrets
  rekey` rotates the data key; `secrets forget --i-have-lost-the-key` is the way out when
  the key is lost. The key-encryption key sits behind a `KeySource` trait, so a Transit
  engine (OpenBao) can take over by re-wrapping the data keys, without touching a value. **Keep
  `secrets.key` apart from the database's backups** (`docs/homelab.md`, `docs/recovery.md`).
  The TLS private key and the database file are now written owner-only too. No new crate:
  `chacha20poly1305`, `base64`, `zeroize` and `sha2` were already in the lock file.

### Changed

- **A shorter Settings menu.** Settings is five entries: Notifications & voice, Models, Skills,
  About you, and Your sign-in. What Endora has done is **Activity** in the main menu, beside
  the Inbox: it is a record, not a setting. What you told Endora and what it has learned are
  one screen, **About you** (they were two, with nothing saying which was which). Recipes are
  reached from the end of Skills. The seven entries that only hold settings for the handheld
  ("Handheld: containers", "Overview: Sonarr" and the rest) say **Set up** once their settings
  are filled, instead of "Needs setup" forever, and lose an on/off switch that did nothing.
  "Uses freely" reads "Runs without asking" and "leaves device" reads "uses the internet". Two
  screens nothing could reach any more are gone.

### Added

- **Your time zone, by name** (`ENDORA_TIME_ZONE`, e.g. `America/New_York`). Daily patterns
  are found on your clock either side of a clock change, and the daily brief and nightly loop,
  stored as UTC hours, are moved when the clocks change so they keep their local hour. The fixed
  `ENDORA_UTC_OFFSET_MINUTES` remains as a fallback.

- **Endora reads your mail, and tells you what matters** ([ADR 0097](docs/adr/0097-it-reads-the-mail.md)).
  With `ENDORA_OWN_WORK=1` and `ENDORA_MAIL_DIR` pointing at the read-only copy the ansible-nas
  `mail_mirror` role keeps, it reads one message at a time in quiet time, newest first, back
  90 days. Code decides what it can: who you write to, which threads are yours, mailing lists,
  invitations. The local model, with no tools and the message marked as data, only describes
  it in a fixed shape that code checks field by field. Each message lands in a bucket
  (People, Money, Security, Appointments, Orders, Your app, Updates, Newsletters, Other), and
  the ones that matter (a reply in your thread, someone you write to waiting three days,
  money due, account security, an appointment, an order problem) go to the activity trail,
  a "Mail" section at the end of your morning brief, and a new **Mail** screen. A bill or a
  security notice from a sender nothing vouches for is marked **may not be genuine** and
  described by code, never in its own words. Nothing is opened, no link is followed, and no
  mail text reaches the model that words the brief.
  Bills are checked against your calendar through Home Assistant ("Follows 'Dental checkup'
  on 2026-10-02", or that there is no such appointment); dates mentioned in mail are listed
  under **Coming up** only when something confirms them; and when someone you write to sends
  fresh mail, chat says so without naming them, at most twice a day.
- **What you set up on the handheld can now be asked about in chat.** The handheld's Docker,
  Portainer, Sonarr, Radarr and NAS-metrics entries were settings-only, so a service you had
  already configured was invisible to the one place you would naturally ask about it. Five
  read-only skills now borrow those settings (nothing is configured twice, and each is offered
  only once its entry is set up). They never start, stop or restart anything, read container
  logs or search media, and take no input, so the model chooses no address. Their results mark
  the turn as outside material, so a turn that read them cannot act and is never escalated to
  the deep model: container names and NAS figures stay on the device. Overseerr and the home
  screen stay handheld-only. ([ADR 0093](docs/adr/0093-the-handheld-is-an-explicit-home-console.md),
  amended.)

- **Goals know whose they are, and Endora's own wait for quiet and never use the paid
  model** ([ADR 0094](docs/adr/0094-work-it-finds-for-itself.md), Phases 1–2 of
  [the always-on plan](docs/plans/always-on-work.md)). Nothing starts work on its own
  yet; this is the ground that makes it safe to. A goal now records whether the
  person asked for it or Endora started it. The person keeps three goals; Endora's
  own lane holds one, counted separately, so neither can use up the other's slots,
  and the worker takes a request before work nobody asked for. A pass on a goal
  Endora started runs on a butler with no deep model to escalate to, decided in the
  one place goal passes run rather than by a setting, and takes no pass within ten
  minutes of the person writing in chat. A database from before this reads every
  goal as the person's, and an origin it cannot read is treated as Endora's own, the
  lane that never outranks a request. The goals API reports each goal's `origin`.

- **Endora can look into a standing trouble on its own, quietly, behind a switch**
  ([ADR 0094](docs/adr/0094-work-it-finds-for-itself.md), Phase 3). Off unless the node
  is started with `ENDORA_OWN_WORK=1`. A device that has not answered for three days
  (and which you have not called fine) can become a goal Endora starts itself, only when
  you have been quiet, nothing you asked for is open, and half an hour has passed since
  the last one. It reads and looks; it cannot change anything; it uses the local model
  only and reads only the house (Home Assistant), not the web or any other skill,
  keeping only what it actually read. It is finished by the record it came from, not by a model: when the device
  answers again the goal completes with no further model call, and if you mark it fine
  the work stops. Its ending goes to the activity trail and posts nothing to chat. An
  item is looked into at most twice, a week apart, and never again once you press Stop.
  The nightly loop's own long-running intention is to be folded into this lane in the
  next phase, with the stumper replay.

- **The lane also asks failed questions again and looks into what Endora understands, and
  the nightly loop hands both over** ([ADR 0095](docs/adr/0095-more-sources-for-work-it-finds.md),
  Phase 4). Still behind `ENDORA_OWN_WORK=1`. With nothing wrong in the house, a question it
  once failed to answer is asked again a day after it failed, judged by the same
  check that filed it and giving up after the same number of tries; and failing that, the
  belief it is most sure of is looked into, with the web and the other read-only skills
  the nightly thread had, on the local model only. The nightly loop stops replaying a
  stumper and keeping a thread of its own **only when the switch is on**; with it off, the
  loop is exactly as it was, so deploying this changes nothing until the switch is
  turned on. Repairs were considered and left out: they already end in a policy action
  or a question only you can answer, so there is nothing for a goal to gather.

- **Endora notices what the house does at the same time most days**
  ([ADR 0096](docs/adr/0096-patterns-it-notices.md)). With `ENDORA_OWN_WORK=1`, it counts
  the changes the house has settled on over the last fortnight and writes a line to the
  activity trail when something happens at about the same time on most days: "the front
  door goes on around 07:30, on 11 of the last 14 days". It is arithmetic with no model,
  it says only what it counted, and it speaks again only when the picture changes (and not
  more than every six hours). Set `ENDORA_TIME_ZONE` for your hours. This is the
  first standing job: a review that keeps running, not a goal that ends. Reading the mail
  into buckets and a safe terminal sandbox are proposed in
  [the standing-jobs plan](docs/plans/standing-jobs.md) and wait on your decisions.

- **The node's data can live outside the container, and be backed up safely.** Everything
  stateful — every setting, API key and sign-in secret — is one SQLite file under `/data`,
  which in the container was a Docker named volume buried in Docker's own storage. Setting
  `ENDORA_DATA_DIR` in `local.mk` to an absolute host path now keeps it in a folder a backup
  tool can reach; unset, nothing changes. `make migrate-data` copies an existing volume across
  once (checksummed file by file, deleting nothing), and `make deploy` **refuses** to start on
  an empty data folder while the volume still holds the node's data, because that would be a
  brand-new node (new token, no settings) that looks exactly like data loss.
  `endora-node backup <file>` (and `make backup`) takes a consistent, integrity-checked,
  owner-only (`0600`) snapshot through SQLite while the node runs. A plain copy of a live
  database can land mid-write and look whole until it will not open. The snapshot never
  overwrites an earlier file, never creates a database from a mistyped path, and removes its
  own output if the check fails. Proven end to end on the real schema: a snapshot taken from a
  running node booted a second node on its own.

- **A specimen records how it ended, and the night says when it gave up**
  ([ADR 0075](docs/adr/0075-failures-become-specimens.md), groundwork for
  [ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)). A failed ask that
  the nightly loop replays to exhaustion is the live record proving the machinery
  cannot answer something — the only trigger ADR 0071 allows for drafting a new
  capability. That population could not be read: retirement was one flag, every
  listing asked for the specimens *not* retired, and a specimen that **passed on
  its last allowed replay** left a row identical to one that ran out — same replay
  count, same flag. Anything inferring "it gave up" from the count would have
  proposed a new capability for a gap that had just closed. A specimen now records
  which ending it reached, at the moment it reaches it, and the morning trail says
  so — *"I've stopped re-asking this one — nothing I have can answer it"* — where
  before it repeated "it still fails" identically on the first failed replay and
  the last. Existing databases are backfilled from what their rows can still prove;
  the one case that cannot be recovered (already retired on exactly the replay
  limit) is read as having given up, which is the likelier of the two and costs at
  most a proposal card the person is asked about.

- **Authoring a recipe is a loop, not a form**
  ([ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)). A new **Try it**
  button fetches the draft once and shows the address it really used, every field
  it can read — each one tappable straight into the sentence — and the sentence as
  it would read. No more hand-writing JSON paths against an API you can't see.
  Sample-value boxes appear per declared input as you type them. The trial runs
  the same validation, the same guarded fetch and the same templating a saved
  recipe uses, so what you see when you try it is what you get when you save it;
  nothing is stored or enabled by trying. Fields inside lists are skipped and said
  to be skipped, because a recipe reads object keys only.

### Fixed

- **The docs called `endora export` "a complete" backup. It covers memory only.** It has
  no settings, keys, stances, MCP servers or sign-in, and nothing can import it, so a node
  rebuilt from an export had lost its whole configuration. `docs/hosting.md` now says so and
  points at the snapshot.

- **Fetching a web page that contains any non-ASCII character crashed the fetch.** The
  page-to-text step stepped through the page a byte at a time and sliced it at each step,
  so the first character outside ASCII (an arrow in a page's navigation, an accented
  letter, a dash) made it panic, and a letter that changes length when lowercased threw
  every later position off. Found by running a goal against the real model and the real
  web. It now walks the page a character at a time and keeps the text it reads.

- **A recipe could reach the house's own network.** The recipe interpreter
  ([ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)) fetched through the
  same helper the constant-host built-in skills use, which does not apply the SSRF
  guard — so a recipe aimed at `169.254.169.254`, a `192.168.x.x` device, or
  `localhost` was simply fetched. A recipe's URL is arbitrary (the person types it,
  and one day the butler drafts it), which is exactly the case the guard exists for
  (ADR 0051). Recipes now fetch through the guarded path, which also re-guards each
  redirect hop. The regression test asserts on the *reason* the fetch was refused,
  because a connection error would otherwise let a guard-less build pass.

## [0.12.0] — 2026-08-08

### Fixed

- **`make deploy` no longer depends on where you run it from.** Compose derives its
  project name from the working directory, so a deploy run from a git worktree
  invented a *new* project — a fresh empty volume and a container-name collision with
  the real deployment, failing only after the whole image had built. The Makefile now
  pins `COMPOSE_PROJECT_NAME=endora`, so every deploy reaches the same containers and
  the same data wherever it runs from.
- **Home Assistant actions failed on the commonest phrasing.** "Turn on the kitchen
  light" put the *kind* word in HA's `name` field — `{name:"light", area:"kitchen"}`
  asks Home Assistant for a device literally called "light" and comes back
  `MatchFailedError(NAME)`. The butler was picking the right tool and the right area
  every time; only this one field was wrong. `Hass*` calls now drop a `name` that is
  a device kind when an area is given. **Only** when an area is given — without one,
  dropping it would widen "turn on the lights" to the whole house, so it is left to
  fail honestly instead (ADR 0051).
- **Belief de-duplication survives rephrasing.** Found by the first live run of the
  new understanding eval: `similar()` only caught near-verbatim repeats, so a
  rephrased belief was filed as a second one and duplicates accumulated into every
  later turn's context. Now stems words and compares by containment.

### Changed

- **Constitution §9 amended** (adopted by the maintainer, 2026-07-25; deliberation
  recorded in [docs/proposals/constitution-amendment-section-9.md](docs/proposals/constitution-amendment-section-9.md)).
  The clause described the `Direction → Assumption → Experiment → Observation →
  Reflection` loop, every step of which was deleted by ADR 0052 — a constitutional
  limit governing machinery that no longer existed. It now describes the loop Endora
  actually runs, and states explicitly what had been true in code since ADR 0052 §3
  but unstated constitutionally: Endora forms and revises **its own model of the
  person** without per-item approval, bounded by remaining visible, correctable and
  able to expire. Adaptation of Endora's **processes** is still proposed, not
  imposed. **No new authority to act** — the reversibility bands, the deterministic
  policy boundary and audit are untouched.

### Added

- **Standing questions** ([ADR 0076](docs/adr/0076-standing-questions.md)).
  A closed set of routine asks whose complete answer is derivable by code routes
  deterministically before the model's turn: the brief (ADR 0074's route, folded
  into the one mechanism), and kind-status questions — "what lights are on?",
  "any lights offline?" — answered from the house's own reading, naming what is
  on and what is not answering. Action phrasings ("turn off the lights") never
  match; anything the set doesn't cover falls through to the ordinary turn.
- **Failures become specimens** ([ADR 0075](docs/adr/0075-failures-become-specimens.md)).
  A chat turn the deterministic checks reject (the honesty valve, not-an-answer)
  files the ask privately in the house's own record; the nightly loop replays one
  per night under the reversible-only runner, judged by the same verdict that
  filed it, and the morning trail says when a question that once stumped the
  butler answers now. Bounded shelf (12), duplicates not refiled, giving up after
  two weeks of failed replays. Never harvested into checked-in fixtures — the
  battery stays hand-authored, per the constitution.
- **The brief is a standing order** ([ADR 0074](docs/adr/0074-the-brief-is-a-standing-order.md)).
  The daily brief's sections are gathered by code with known-good arguments — the
  weather (whose summary now says whether an umbrella or jacket is worth it, from
  the numbers), the drive from the house's own travel-time sensors (a new `traffic`
  skill, the Waze Travel Time shape, no maps credential), the top **three**
  headlines (`count` is now a news input, enforced by the call), ticketed events and
  city meetings where configured — joined to the calendar, presence and
  standing-trouble facts ADR 0056 already assembled. The deep model words it when
  opted in (append, never rewrite); with no model the facts post as themselves. A
  new `brief` skill assembles the same sections fresh at ask time, so an afternoon
  ask reads the world in the afternoon. The brief's agentic gather and its six tool
  rounds are retired.
- **"Evidence verifies" is implemented** ([ADR 0053](docs/adr/0053-honesty-about-what-it-did.md)).
  The architecture principle is *models propose, policy authorizes, capabilities
  execute, **evidence verifies**, memory learns* — and the fourth clause had **zero
  lines of code**. The turn proposed, authorized, executed, and then narrated, with no
  step that looked at the world, so the butler's account of what happened came from
  the actuator's self-report. Three production defects in one day traced to that gap,
  the worst being a light that never turned off while Home Assistant reported success.

  A capability that *reads* state (`Reversibility::Observe`) returns evidence and
  stands on its own. Anything that *acts* returns a receipt, and its result is now
  marked `[unverified]` with an instruction to report what was claimed and that it is
  unconfirmed. Capabilities of unknown band fail closed — every MCP result is a
  receipt until an integration says otherwise, so integrations nobody has debugged are
  honest by default. Eight built-in reads mislabelled `Reversible` are now `Observe`
  (policy-neutral: both map to `Act`).

  This does not stop the model picking the wrong tool — it means you find out.
- **Read-back verification and ambiguity surfacing** (ADR 0053 layer 1). A capability
  can now name the one that *observes what it changes*; after an actuation the turn
  runs it and the model answers from the **observation**, with an explicit instruction
  that the observation wins if it disagrees with the tool's claim. One mapping per
  integration — every Home Assistant `Hass*` action verifies through `GetLiveContext`
  — and unknown servers stay `[unverified]`. **The read-back runs after failures too**,
  because a failed action's most useful output is what actually exists: the live
  `HassTurnOff` failure is far more actionable once the result carries the entities
  that *are* in that area.

  It also flags **ambiguous names**. A live install had two entities both called
  "Kitchen" in one area — a `light` reading `off` and a `switch` reading `on`, the
  switch being the real ceiling light — so "turn off the kitchen light" matched the
  dead entity and every layer faithfully reported success about the wrong device.
  Nothing was broken; the name was ambiguous and each component resolved it silently.
  A reading where one name spans several domains now says so and tells the butler to
  ask rather than guess.
- **The fitness battery is data-driven, and runs repeatedly.** Cases moved out of a
  hardcoded function into `crates/endora-infrastructure/src/eval.rs` as declarative
  data — a name, a tier, a probe, and a check — so adding one is adding a struct
  literal. The battery grew from 24 cases to **34**, including the case that matters
  most for ADR 0053 (`relay:failure-is-honest`: with a tool error in front of it, the
  model must not narrate success), plus candour, register and conversation-ending
  cases, and more understanding cases.
- **`evaluate_repeated` reports the spread instead of hiding it.** A single run is a
  smoke test: two consecutive runs of the same model scored L1 6/8 then 8/8 with
  nothing in the routing path changed. Repeat runs report per-case pass rates, the
  mean, the range, and which cases *flipped* — a case that flips says the model is
  marginal on that behaviour, which is often more useful than the score. The live
  harness now defaults to 3 runs (`ENDORA_EVAL_RUNS`) and asserts on the **worst**
  run, because a butler that is sometimes unusable is unusable. Tier maxima are
  derived from the battery, so growing it cannot desync them.

  **First trustworthy measurement** (`qwen2.5:7b`, 3 runs): mean **29.7/34**, range
  27–31, **spread 4** — the resolution of the instrument, and the reason the earlier
  "20/23 → 23/24" reading was noise rather than improvement. It also surfaced that
  **`relay:failure-is-honest` fails 1 run in 3**: with a tool error in its immediate
  context the model still sometimes narrates success. That is the risk ADR 0053
  accepted when it removed the deterministic honesty nets, now quantified instead of
  assumed.
- **No personal data in the battery.** The cases are synthetic, modelled on observed
  failure *shapes* rather than harvested content — putting a real conversation in git
  would breach §5/§6, and a battery that cannot be shared cannot compare models with
  anyone else.
- **Contradictions are surfaced rather than resolved.** When a new belief disagrees
  with one Endora already holds, both are kept and the conflict is written to the
  activity trail. Which one is true is the person's judgement, not the butler's —
  auto-resolving by confidence or recency would have Endora deciding which of your
  stated preferences is real. A ninth eval case, `command-not-belief`, scores whether
  the model produces the defect in the first place.
- **Per-machine Make settings via a git-ignored `local.mk`.** Deployment hosts are a
  property of your machine, not of the project: `make deploy` still targets the local
  Docker daemon out of the box, so a fresh clone works with no setup, and putting
  `DOCKER_CONTEXT = nas` in `local.mk` makes an always-on box your personal default.
- **Beliefs decay and expire** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)).
  `BeliefStatus::Expired` existed, round-tripped through storage, and was **never set
  by any code path** — the direction reset's "nothing is assumed permanently" was a
  promise nothing kept. Confidence now steps down one level per elapsed half-life
  since a belief was last affirmed, and fades out entirely below `low`. Half-lives
  are per kind and encode ADR 0052's own thesis: intent and values change slowly (365
  days), a frustration or stressor usually does not (45). Affirming resets the clock.
  Decay is derived from the stored timestamp, so understanding is honest the moment
  it is read; the nightly loop additionally persists expiry and reports each let-go
  belief to the activity trail, so the forgetting is visible. The export keeps
  reporting **stored** confidence — the memory right is to see what Endora actually
  holds — while the console shows the current reading.
- **An L3 "understanding" tier in the model fitness battery**
  ([ADR 0055](docs/adr/0055-the-model-layer.md)) — does the butler form
  beliefs from real evidence, stay quiet when a turn reveals nothing, avoid re-filing
  what it knows, and refrain from overclaiming confidence. Scoring is lexical and
  unit-tested rather than model-judged, which would be circular and unauditable.
  Baseline for `qwen2.5:7b`: **L1 6/8, L2 7/7, L3 7/8, 20/23** — understanding is not
  the weak axis; routing is.
- **An adoption floor**: the model layer never auto-adopts a candidate that wins on
  total while scoring lower on understanding — it proposes it instead. Understanding
  is a veto on automatic adoption, not a way to win.

### Removed

- **The goal tracker is gone** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md),
  superseding [ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §4).
  ADR 0052 reset the direction to an autonomous personal intelligence but chose to
  leave the goal machinery in place "as an optional expression of intent". It never
  became optional: it stayed 12 of 28 tables, ~25 of 60 routes, and the bulk of what
  the butler's context was assembled from — and the system prompt ended up telling
  the model "you are not a goal tracker" while handing it a goal-tracker schema to
  fill in. Removed: `Value`, North Star (`Direction`), `Target`, `Assumption`,
  `Experiment`, `Observation`, `Reflection`, `ProposedProcessChange`, the
  attention/snooze read model, and the suggestions inbox with its `ButlerProposal`
  set — across the domain, storage, HTTP, CLI and console. The `domains/direction`
  crate and the `Proposer` port are retired. **~8,600 net lines deleted.**
- **BREAKING:** the `/v1/values`, `/v1/directions`, `/v1/targets`,
  `/v1/assumptions`, `/v1/experiments`, `/v1/reviews`, `/v1/observations`,
  `/v1/reflections`, `/v1/process-changes`, `/v1/suggestions` and `/v1/attention`
  endpoints and their CLI commands are removed; `/v1/export` no longer carries those
  collections. Existing databases **drop the old tables on open**, so no one is left
  holding data nothing can read, correct, or delete (constitution §6).

### Changed

- **Proactive check-ins are agentic** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
  The schedule is now a **budget, not a trigger**: deterministic code owns how often
  the butler may speak (minimum interval, and never on top of someone who just
  spoke), and the butler decides whether it has anything worth saying — with the
  reason recorded to the activity trail. The budget is spent whether or not it
  speaks, so "nothing to say" cannot become a retry loop. Silence is the default on
  every failure path. The daily brief and nightly loop stay time-anchored on purpose.
  `run_due_checkin` → `consider_reaching_out`; `CheckinSchedule::is_due` →
  `may_reach_out`.
- **Understanding is now the only model Endora keeps of a person.** Beliefs already
  carried what goals stood in for — what someone is reaching for, with evidence, a
  confidence, and the ability to be corrected or to expire. `ButlerContext` carries
  that plus the skills the butler can reach, and nothing else.
- **The nightly loop researches what Endora is most sure of.** `nightly_focus` picks
  the highest-confidence *intent* belief instead of an active North Star; confidence
  is the ranking, so the night is not spent on a tentative guess.
- **The butler no longer files anything for later approval.** It converses and acts
  through the policy boundary as it goes. Not a loosening — every action still passes
  deterministic authorization — but the intermediate "propose a record, review it
  later" step is gone, which is the friction ADR 0052 §3 set out to remove.
- **The system prompt is rewritten** now the contradiction it was policing is gone:
  the proposal schema and the instructions that existed only to fight it are deleted,
  and an explicit rule to report a failed or blocked skill plainly takes their place.
- **The ADR 0053 cutover is complete.** `daily_brief`, `run_due_checkin` and
  `run_due_nightly_loop` moved onto the single tool-calling conversation; the old
  two-pass `gather_with_skills` engine, `ButlerContext::tool_result` and
  `ButlerContext::synthesize` are deleted. Tool results now ride in the conversation
  for every butler, including those without native tool-calling.
- **No more scripted floors.** The daily brief's hardcoded weather → safety → news
  sweep, its `has_real` substring-sniffing of model prose, and the templated check-in
  are gone. When the butler has nothing real to say — or no model to think with — it
  stays quiet rather than emitting a canned string in Endora's voice.
- Docs reconciled with the code: `docs/roadmap.md` rewritten off the goal-tracker arc,
  `docs/domain-map.md` rewritten around the five surviving contexts, and
  `docs/architecture.md`'s code map corrected (it still showed the pre-ROCA layout).

## [0.11.0] — 2026-07-21

The butler becomes real: it understands you, uses skills to act on real data, guards
what leaves the machine, and holds a hard line against doing anything it can't undo on
its own. Highlights — a capability catalog with settings/secrets (weather, news, safety
alerts, web search, Wikipedia, image review, Home Assistant); the interventions loop
(it actually uses skills); an autonomy envelope with **reversibility-first** levels
(never runs the un-undoable on its own); the egress guard (SSRF block, secret tripwire,
query minimization, optional proxy); date/time awareness and a persistent action log; a
daily brief; an evolving, grounded persona and hospitality; the model upgraded to
`qwen2.5:7b`; and a bring-your-own-model hosting guide. See the entries below and
[ADRs 0019–0025](docs/adr/README.md).

### Changed

- **Direction reset — intent-first understanding** (canonical:
  [docs/direction-reset.md](docs/direction-reset.md); decision:
  [ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): Endora had drifted into
  a goal tracker; it is now an **autonomous personal intelligence** whose job is to
  **understand intent**. It maintains a living **Understanding** — beliefs about you
  (intent, values, patterns, motivations, frustrations…), each with the **evidence**
  behind it and a **confidence**, formed by Endora itself and **reviewable/correctable**
  by you (no per-item confirmation). The **home screen** is now "what Endora understands
  about you," beliefs grouped by kind with *That's right / Not quite*; **goals are
  demoted to optional**. The butler is grounded in what it already understands (builds on
  it, avoids duplicates) and phrases beliefs in second person; check-ins draw on
  understanding. New `GET /v1/understanding` and affirm/correct; beliefs are in
  export/purge. *Interventions* (the butler acting on that understanding by using its
  skills) now land as the next layer — see Added.

### Faster builds

- The container build now uses **BuildKit cache mounts** for the cargo registry and
  `target/`, so deploys do an **incremental** compile (only changed crates) instead of
  rebuilding every dependency each time — a normal change goes from minutes to seconds
  after the first (cache-priming) build.

### Added

- **Configure your skills — settings & secrets** (second slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): skills that need
  setup now declare their settings (a model, a key, a URL) and the Skills view
  renders a form for each. `configured` is computed — a skill is usable only once
  every required setting has a value (and it's enabled). Secrets are stored
  server-side and never echoed back. As the first real user: **Image review** is now
  a working skill — set its vision model (e.g. `moondream`, already on the box) and
  it describes an image via the local model. New `POST /v1/capabilities/{id}/config`;
  settings are covered by delete-all.
- **Two general-knowledge skills** — **Knowledge lookup** (Wikipedia) and **Web
  answers** (DuckDuckGo), both keyless, so the butler can look things up instead of
  only fetching a URL you hand it.
- **The autonomy envelope — how independently Endora acts** (first slice of
  [ADR 0051](docs/adr/0051-where-the-boundary-is.md)): a
  control in the Skills view sets the boundary the butler acts *within* — it acts on
  its own inside it and asks you at the edges. Two levers today: use read-only skills
  on its own (default on), and take consequential actions on its own (default off,
  the safe posture). A deterministic classifier — never the model — decides whether a
  given action may run, from the skill's declared autonomy and reach. Defaults
  preserve the established behaviour; new `GET`/`POST /v1/autonomy`.
- **Manage your skills — turn capabilities on and off** (first slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): the Skills view is
  now a catalog you control — each skill shows **On / Off / Needs setup**, and a
  toggle turns it on or off. Choices persist (a `capability_config` store) and are
  enforced everywhere: a skill you turn off reports as not usable and never runs,
  even if the butler reaches for it. New `POST /v1/capabilities/{id}/enable`, and
  `GET /v1/capabilities` now reports `enabled`/`usable` alongside `configured`.
- **No fabricated facts — a deterministic net for factual asks**: when you clearly
  ask something factual (weather, news, active safety alerts) and the small local
  model reaches for no skill at all, Endora now runs the matching skill itself — with
  your saved home location — and answers from that real result, instead of letting the
  model make something up. It only fires when the model requested nothing, so a correct
  model-driven tool call (e.g. a specific city you named) is left untouched.
- **Local news skill + honest closure**: a new **Local news** skill (Google News
  RSS, no key) fetches real headlines for a place or topic, so "what's in the news"
  is answered from actual sources instead of guessed. And the interventions loop now
  gives **honest closure** in every case — if a skill ran, failed, needs setup, needs
  your OK, or doesn't exist, the butler always answers with that outcome (told to say
  plainly it couldn't rather than invent a result), so it never hangs on "One moment…"
  or claims it checked something it didn't.
- **The butler actually uses its skills — interventions**: when answering needs
  current facts it doesn't have (weather, local safety alerts, a web page), the butler
  now **uses a skill and answers with the real result** instead of only saying it will.
  The model *proposes* a skill (`use`); a deterministic policy check authorizes it —
  only a **configured, read-only** skill runs on its own, anything consequential (e.g.
  flights) stays gated for you to confirm — then the skill executes and the butler
  answers using what came back (one tool round per turn). If it isn't cleared or the
  model asks for nothing, the reply is unchanged, so nothing regresses. The turn's
  activity notes which skill was used. This is the *interventions* layer promised by
  the direction reset ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  built on the capability registry ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
- **See what Endora does — activity in the chat**: after each turn the chat shows a
  subtle note of what happened behind the scenes ("Learned that you find mornings
  hard", "Added to your inbox — …", "Grew more sure that …") — closure on a
  conversation and a window for debugging. Toggle it from the menu ("Show Endora's
  actions"). The chat responses now include an `activity` list.
- **A gentle "where are you based?" setup**: when Endora doesn't know your location,
  the home screen offers a one-line prompt; it's stored as a preference the butler
  already reads, so skills like weather and the guard dog have a starting point — no
  separate onboarding.
- **Capabilities — the butler's skills** (third slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): pluggable modules the
  butler can reach for, each declaring what it does, its **autonomy level** (act vs
  ask), whether it **leaves the device**, and whether it's **configured**. Working
  today (no keys): **Weather** (current + today + a severe-weather heads-up),
  **Web browsing** (fetch a page, read its text), and a **Guard dog** (active
  public-safety alerts via the US National Weather Service). Declared and awaiting
  setup: image review (local vision model), local events, flight search, location
  tracking, incident scanner — each shown with what it needs, not silently missing.
  New `GET /v1/capabilities` and `POST /v1/capabilities/{id}/invoke`; a **Skills**
  view in the console. (The registry is the substrate an MCP host adapter plugs into.)
- **The butler reaches out — proactive check-ins** (second slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the node now runs a
  **heartbeat**, and on a cadence you choose (a **Check-ins** control in the chat —
  off by default, or every couple of minutes / hourly / daily) the butler posts a
  proactive opening message grounded in what needs attention and what you're working
  toward, always asking how it can serve you better — the self-improvement loop, in
  conversation. It only ever posts a *message*; anything consequential still goes
  through propose→confirm (the autonomy model). New `GET/POST /v1/checkin`; the
  cadence is cleared by purge.
- **Chat learnings persist — a suggestion inbox** (first slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the butler's
  proposals used to vanish on reload unless you confirmed them in the moment. Now
  every proposal is saved as a durable **suggestion** tied to the reply it came
  from, and collects in an **Inbox** (new `GET /v1/suggestions`, with a count on the
  nav) where you can **Apply** or **Dismiss** it any time — applying runs the same
  deterministic create, so it flows into your North Stars / targets / preferences
  (you authorize; the butler only proposed). Suggestions are part of export and
  cleared by purge. **Targets now attach correctly**: the North Star a target names
  is resolved when applied — by id, or by matching the name to an existing North
  Star — instead of the proposal being silently dropped when the model gave a name
  rather than an id (`POST /v1/suggestions/{id}/apply|dismiss`).
- **The butler's reply streams in live, token-by-token**
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a new
  `POST /v1/chat/stream` (Server-Sent Events) streams the reply's prose as the model
  produces it, so the console grows the butler's bubble word-by-word instead of
  waiting for the whole thing — the big difference in *feel* between a spinner and a
  live agent. The structured proposals still arrive at the end and are still
  confirmed by you (the model never acts). Built on the existing `ureq` client
  bridged to an async SSE response (no new dependency) and the unchanged JSON
  envelope (so the candid, jargon-free persona is untouched); the model streams
  `stream: true` and the node incrementally extracts the growing reply. The
  non-streaming `POST /v1/chat` remains for the CLI and as a fallback. Your message
  is still persisted before the model is called, so an interrupted stream never
  loses the turn.
- **Built-in self-signed HTTPS** (`ENDORA_TLS=1`): the node can serve HTTPS with a
  self-signed certificate (persisted next to the database, so the browser warning is
  accepted once), making the web console a **secure context** — which browsers
  require for the microphone / voice input. No domain, CA, or reverse proxy needed;
  set `ENDORA_TLS_SAN` to the host's LAN IP/hostname for a cleaner cert. The console
  and [hosting guide](docs/hosting.md) explain enabling voice. (Fully local Whisper
  STT — audio never leaving the host — is the next voice step.)
- **The butler is grounded in your life (1.0 integration)**: each turn the butler
  is handed a `ButlerContext` — your values, your North Stars (with status, the
  value each serves, and whether it has a target yet), and what needs attention —
  so it speaks about what actually exists and proposes the next concrete step,
  including a **target under an existing North Star** (a new `create_target`
  proposal). Verified live: the butler referred to an existing North Star by name.
  (Cutting the `1.0.0` release remains a human decision.)
- **Voice & character (0.9)** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the butler's prompt now **mirrors the person's register** — matching warmth and
  formality — with the golden-rule floor (reflect kindness upward, never hostility
  downward), on top of stored style preferences; truthfulness is never softened.
  **Voice** is browser-native and opt-in: the console reads the butler's replies
  aloud (`speechSynthesis`) behind a Speak toggle and captures a spoken message
  (`SpeechRecognition`) from a 🎤 button — no backend change, no dependency, and it
  hides where the browser lacks the API. Local-first caveat stated: browser speech
  recognition may use a cloud service, so it stays opt-in.
- **Anti-sycophancy eval harness (completes 0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §5): an
  opt-in eval (`endora-infrastructure` integration test, `#[ignore]`d so CI needs no
  model) baits the butler with prompts that tempt flattery/reflexive agreement and
  checks it stays candid. Run on demand against a live model
  (`ENDORA_MODEL_URL=… ENDORA_MODEL=… cargo test … --test butler_eval -- --ignored`).
  Sycophancy is treated as a defect measured by evals, not a code gate.
- **Preferences — the butler learns (finishing 0.7)**
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): the butler records what it learns
  about you (taste, or explicit grants of authority) as **visible, correctable,
  deletable** memory, and feeds it back into its own context so it stops re-asking
  what it already knows — "learning is the accumulation of preferences." New
  `/v1/preferences` (create, list, delete), CLI `preference add|list|delete`, a
  console **Preferences** view, and a `remember_preference` proposal the butler can
  make (you confirm it). Preferences are part of export and cleared by purge. The
  local model butler is now live on the NAS via Ollama.
- **Adaptive attention (0.8)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
  the butler surfaces what needs attention — due reviews, North Stars not yet filed
  under a value, and North Stars with no active target — via `GET /v1/attention`,
  most pressing first. Each item can be deferred (`POST /v1/attention/snooze`) with
  **exponential backoff**: every "not now" doubles how long it stays hidden (1, 2,
  4, … days), so a repeatedly-deferred item asks less and less. The console home
  shows a "Needs your attention" section with a per-item "Later". The ranking serves
  the person's stated values, not engagement.
- **The butler — conversational MVP (0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a chat
  surface where the butler **proposes** structured actions from a closed set
  (create value / North Star) and the person **confirms** each one — existing
  deterministic use cases execute; the model never acts on its own. Two brains: a
  `ScriptedButler` (offline, deterministic, the reliable fallback) and an
  `LlmButler` (a local OpenAI-compatible model, with a candid, explicitly
  anti-sycophantic prompt, falling back to scripted if unavailable). The
  conversation is persisted and included in export/purge. New `POST/GET /v1/chat`,
  CLI `chat "<message>"`, and a console chat panel with confirmable proposals.
- **Values layer (0.6)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
  a **`Value`** — the *why* a North Star serves (health, community, craft) — sits
  above North Stars: **Value → North Star → Target**. A North Star can be filed
  under a value (assigned by the person, never inferred). New `/v1/values` (create,
  list, delete) and `POST /v1/directions/{id}/value`; CLI `value create|list|delete`
  and `direction value <id> <value-id|none>`; the console home groups North Stars by
  value. Deleting a value in use is refused (re-file first). Existing databases gain
  a `values` table and a nullable `value_id` on open.
- **Artifact lifecycle (0.5)**: North Stars and Targets gain a lifecycle status —
  **active / achieved / abandoned / archived** — plus **delete**, across the domain,
  API, CLI, and console. Set status via `POST /v1/directions/{id}` and
  `POST /v1/targets/{id}` (`{"status":"achieved"}`); delete via `DELETE` on the same
  paths (refused while dependents exist — archive instead). CLI: `direction status`,
  `direction delete`, `target status`, `target delete`. The console shows status
  pills, per-item lifecycle actions, and hides archived items behind a toggle.
  Existing databases gain the `status` column (defaulting to `active`) on open.

- **Phone-usable console and a hosting guide** (0.4): the web console gains a
  mobile breakpoint that stacks forms and tightens the layout so the whole loop
  is usable at phone widths. New [docs/hosting.md](docs/hosting.md) covers running
  the node always-on (systemd / container restart), backups, and reaching it
  securely from other devices over a private overlay (e.g. Tailscale) or an
  authenticating reverse proxy.
- **Activity feed and live updates**
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a newest-first
  timeline of what has happened (`GET /v1/activity`, CLI `activity [limit]`),
  derived from the already-persisted observations and audited decisions. The node
  also exposes a server-sent change stream (`GET /v1/activity/stream`) that emits a
  `changed` signal after any successful write; the web console subscribes and
  refreshes its feed — and due-review banner — live, no reload needed.
- **Review scheduling** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  the first application of the autonomy model
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): an experiment can carry a
  scheduled review time, and the system surfaces reviews that are due without
  acting on them. New protocol endpoints `POST /v1/experiments/{id}/review` and
  `GET /v1/reviews/due`; CLI `experiment review <id> <days>` and `reviews due`;
  and a web-console "Remind me" control plus a due-review banner on the home view.

### Fixed

- **Chat could hang "forever" and lose its place on reload.** The butler's call
  to the local model had no timeout, so a slow/stuck model (e.g. inference on CPU)
  left the "thinking…" indicator spinning indefinitely, and reloading dropped the
  client-only indicator so you couldn't tell if a reply was still coming. Now the
  model round-trip is bounded (90s) and always falls back to the scripted reply, so
  a reply is always persisted; and the thinking indicator is **derived from stored
  state** (shown whenever your message is the newest one), so it survives reloads
  and reflects reality. Your message is still saved before the model is called.

### Changed

- **The butler talks like a person, not a schema** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the `Value → North Star → Target` structure is the butler's *internal model* of you
  and your browsable profile — not conversational vocabulary. The system prompt now
  forbids the taxonomy words ("value", "North Star", "target", "goal", …) in the
  butler's spoken reply and grounds it in plain language; confirm-card labels read as
  natural actions ("Keep this as something you're working toward: …") rather than
  schema ("Create North Star: …"). The structured overview keeps the evocative labels
  as the transparency window. A unit test guards the conversation against jargon.
- **Renamed the second-tier concept `Goal` → `Target`**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a North Star's children are
  now **targets** (a concrete, measurable outcome), not goals. This is a
  **breaking protocol change** — `/v1/directions/{id}/goals` → `…/targets`,
  `/v1/goals/{id}/…` → `/v1/targets/{id}/…`, and the `goal_id` field → `target_id`
  — with the CLI (`target …`) and web console updated to match. Existing databases
  migrate automatically on open (the `goals` table and `goal_id` columns are
  renamed in place, no data loss).
- The `experiments` table gains a nullable `review_by_ms` column, added to
  existing databases by an automatic forward migration on open.
- `make docker-run` now publishes the node on loopback only
  (`127.0.0.1:8787:8787`) rather than all interfaces, matching the documented
  security posture (the `0.x` API is unauthenticated).

## [0.1.0] - 2026-07-19

First tagged release. Endora is in its foundation phase and is **not** a general
autonomous agent; interfaces may still change before 1.0.

### Added

- **Project foundation** — constitution, architecture docs, ADRs (0001–0008),
  contribution/governance/security policies, and a domain-first modular-monolith
  Rust workspace (`endora-domain`, `endora-application`, `endora-infrastructure`,
  `endora-node`, `endora-cli`).
- **The learning loop, end to end** — Direction → Goal → Assumption → Experiment
  (with a proposed/running/concluded lifecycle) → Observation → Reflection (over
  cited evidence) → Proposed process change, all persisted (SQLite) and served
  over a versioned HTTP/JSON protocol, with a CLI client.
- **Deterministic policy boundary** — models propose; deterministic policy
  authorizes. Enacting a process change requires explicit human approval and an
  autonomy-appropriate actor; the decision endpoint returns permit /
  require-human-approval / deny.
- **Audit trail** — every consequential policy decision is recorded and readable
  via `GET /v1/audit`.
- **Local model adapter** — an optional local, OpenAI-compatible model (e.g.
  Qwen3.5 via Ollama) can *draft* a process change from a reflection; the draft
  is an ordinary pending proposal that still passes through the policy boundary.
  The node degrades gracefully (HTTP 503) when no model is available.
- **Memory rights** — `GET /v1/export` returns all of a user's data as JSON;
  `POST /v1/memory/purge` (with explicit confirmation) permanently deletes it.
- **Packaging** — a multi-stage `Dockerfile`; the node targets macOS, Linux,
  Ubuntu Server, and Docker.
- **Tooling** — a `Makefile` (bootstrap, run, dev, `ci`), GitHub Actions CI
  (fmt, Clippy with warnings denied, tests on Linux and macOS), and Dependabot.

[Unreleased]: https://github.com/RusticPotatos/Endora/compare/v0.12.0...develop
[0.12.0]: https://github.com/RusticPotatos/Endora/compare/v0.11.0...v0.12.0
[0.1.0]: https://github.com/RusticPotatos/Endora/releases/tag/v0.1.0
