# Endora HQ: the design

Agreed with the owner in a design interview on 2026-10-16 (questions Q1–Q34), recorded in
[ADR 0102](../adr/0102-endora-hq.md). Prepared with AI assistance (Claude). This is step 7 of
[the always-on butler plan](always-on-butler.md). Phases 1, 2 and 4 (section 15) are built: the
team core in `domains/team`, the HQ service in `app/hq` and the office screen; real workers
(Endora's own worker, the model relays, the runner on the NAS and its fence); the team's
contracts on the home lab ([ADR 0106](../adr/0106-team-contracts.md)); routines, alarms
and check-backs ([ADR 0107](../adr/0107-routines-alarms-and-check-backs.md)); and the bridge,
approvals, the owner's replies and a goal's notes ([ADR 0108](../adr/0108-the-bridge.md)); and
the team's rings ([ADR 0110](../adr/0110-the-teams-rings.md)): the Pi as a runner of offline
workbenches, reaching HQ through the node's team gateway. Phase 3's pairing is deferred, and so
is the egress proxy. Phases 5 to 7 are not yet.

This document is the specification to build from. It describes behaviour, rules and words,
not code: implementation starts from here and nowhere else.

## 1. What it is

Endora HQ is a separate Endora feature: a **team** of AI workers that takes a **goal** and
works towards it, while the owner watches and decides. It shows as a pixel startup office in
Endora's console: the team at their desks, a whiteboard of sticky notes, a server closet
with the engine, and the owner's desk where everything that needs them lands.

- **Goals** come from the owner, set in HQ, or from Endora, which proposes a goal from what
  it has learned or noticed; a proposed goal waits on the owner's desk.
- **The work** is anything a team can do through pull requests and write-ups: changes to the
  owner's apps and to Endora itself, home-lab changes (as pull requests to the Ansible
  repositories), and research.
- **What the team knows** about the owner arrives only as **briefs** that Endora prepares
  from its brain and the owner releases (section 8).
- **The team** starts as one: a lead, a developer, a researcher and a reviewer. It can propose
  more members (a manager or product owner, say) and a name for itself. The owner decides.

## 2. Words

| Word | Meaning |
| --- | --- |
| Owner | The person Endora works for. In HQ they are also the board: the last word on everything. |
| Desk | The owner's queue in HQ: everything waiting for them to approve, adjust or decline. |
| Team | A group of members with a goal, a budget and a name. One to start. |
| Member | One worker on a team, with a role, a model, a look and a daily budget. |
| Goal | What a team is for, in a sentence. It can have smaller goals under it. |
| Task | One piece of work towards a goal. Every task traces up to its goal. |
| Thread | The conversation on a task: the only way members, Endora and the owner talk about work. |
| Claim | Taking a task so that only one member works it at a time. |
| Wake | A request that a member take a turn: something changed that concerns them. |
| Run | One turn of a member: it starts, does a bounded amount of work, and ends. |
| Tick | The engine's regular pass that starts due work and recovers stuck work. |
| Runner | A host that runs members' turns in containers: the Pi, Docker on the NAS, a spare computer. |
| Brief | What the team is told about the owner for a goal, prepared by Endora and released by the owner. |
| Proposal | Something a member or Endora asks the owner to decide: a goal, a hire, a name, a brief, a budget, an action. |
| Gate | The checks that decide whether work is done: tests, build, review. Never a member's own word. |
| Pool | The team's share of model spend, split per member per day. |
| Kill switch | Stops every run at once and closes Endora's gateway to the team. |

## 3. Architecture

Three parts, loosely coupled, each doing one job well.

```
Owner ── Endora console (one sign-in) ──┐
                                        ▼
  ┌──────────────────────────── Endora node (NAS) ─────────────────────────────┐
  │ trust boundary: gateway (allow / ask / deny), secrets, the brain,           │
  │ append-only audit log, the console and its live updates                     │
  └──────────────▲────────────────────────────────────────────▲────────────────┘
                 │ team API + events (authenticated)           │ privileged actions only
  ┌──────────────┴────────────── Endora HQ (own container) ────┴───────────────┐
  │ teams, goals, tasks, threads, wakes, runs, tick, budget ledger, proposals,  │
  │ event feed; its own SQLite database and settings                            │
  └──────────────▲─────────────────────────────────────────────────────────────┘
                 │ team protocol (MCP), per-run token
  ┌──────────────┴──── Runners: the Pi (fenced), Docker on the NAS (fenced) ───┐
  │ one fresh container per run: the worker, its workspace, nothing else        │
  └─────────────────────────────────────────────────────────────────────────────┘
```

- **The Endora node** stays the trust boundary. It holds the gateway, every secret and
  credential, the brain and the audit log. The owner signs in to Endora only; HQ's screens
  are part of Endora's console.
- **HQ** is its own container with its own settings (also editable from the console) and its
  own SQLite database. It knows the team and the work, and nothing private.
- **Runners** run members' turns. A worker talks only to HQ. Anything that touches a real
  system (a pull request, a deploy, a message) is a request that HQ passes to Endora's
  gateway, which allows, denies or puts it on the owner's desk.

The six invariants of the always-on plan, as they apply here:

1. **Isolation.** Every run is a fresh container on a fenced runner (`docker_egress` on the
   Pi and the NAS) that can reach HQ and the model relays only; on the Pi, both through the
   node's team gateway (ADR 0110).
2. **Mediated authority.** Members never act on the owner's systems. They ask HQ, HQ asks the
   gateway, and the gateway decides.
3. **No bypass credentials.** A run holds a token for its own task and run only, which ends
   with the run. No repository, cloud or home credential ever enters a container.
4. **External audit.** Endora's append-only log records every request, decision and action.
   HQ's own history is for display; Endora's log is the record.
5. **Hard limits.** Time, money and tool limits can stop a run that is already going.
6. **Information boundary.** The team sees only released briefs, with names and places
   replaced (section 8). Never mail, chat, notes or the brain itself.

## 4. Goals and tasks

**A goal** has a title, a reason, a status (proposed, active, done, dropped) and optionally a
parent goal. A goal Endora proposes is `proposed` until the owner approves it.

**A task** belongs to a goal (directly, or through its parent task) and has:

- a title and a description;
- one owner among the members, or none yet;
- a status: `todo`, `doing`, `in_review`, `done`, `blocked`, `dropped`;
- a priority: urgent, high, normal, low;
- its parent task, if it is a sub-task, and the tasks it waits on;
- a delegation depth: how many hands it passed through from the goal;
- who pays: the member and goal its spend is charged to;
- its thread.

Rules:

- **Every task can say why it exists**: its chain of parents up to its goal is part of what a
  member is shown when it works on it.
- **One owner at a time.** Taking a task is a single atomic step that succeeds for exactly one
  member; anyone else is refused and stops. A member never works a task it has not claimed.
- **Delegation is a task.** A member hands work on by creating a sub-task for another member.
  By default a member hands work only to members below it in the team.
- **A member may not drop work it doubts.** It hands it back to its lead with a reason.
- **Waiting is explicit.** A task that waits on others says so, and is woken when they finish.
- **Nothing is reassigned silently.** Stuck work goes to the owner's desk, with what was tried.

**Done** means all three: the gate passed on a runner (tests went red then green, lint and
build pass); a *different* member reviewed and approved it; and it is on the owner's desk
as ready to merge. A member saying it is done changes nothing.

## 5. The engine

### Wakes

Everything a member does starts with a **wake**: one entry point, with a reason. Reasons
include: assigned a task, mentioned in a thread, a waited-on task finished, all sub-tasks
finished, a proposal decided, a timer for the lead to look over the team's work, a retry,
a recovery nudge, a check-back the member asked for, and an alarm passed into a routine's
open task ([ADR 0107](../adr/0107-routines-alarms-and-check-backs.md)).

Goals are passive and work always arrives as a task: a member never goes looking for work. A
wake on a clock (a timer, a check-back, an alarm) that names no task, for a member with no open
task, is refused as **nothing to do** and starts no turn. After two turns in a row on a task
that moved nothing forward, a wake for it that brings nothing new (a recovery nudge, a retry)
**backs off**: 2 minutes, doubling, at most 30. Anything new (an owner's word, someone else's
post naming the member, a blocker or sub-tasks finished, a check-back, an alarm) wakes at once
and brings a waiting turn forward.

Every wake leaves a receipt, whatever happens to it: **queued**, **folded** into a turn the
member is already taking on the same task, **held** because another member is working that
task (and released, oldest first, when that turn ends), **refused** (and why: the kill
switch, a paused member, an exhausted budget, a duplicate, nothing to do, in review),
**started**, or **finished**.

**The reviewer** (built 2026-10-17). One path takes work and questions past the team's
reviewer, the first member with the reviewer's role:

- **Work moved to `in_review`** by a member other than the reviewer wakes the reviewer with
  `review_requested` on that task. With no reviewer (or the reviewer's own work) it is on the
  owner's desk as **ready to read**, with its thread. While a task waits in review its owner
  takes no turn on it (the wake is refused as **in review**), except for the owner's reply,
  the answer to a question, and the reviewer sending it back (`sent_back`).
- **A question to the owner about the home lab** (`ask_owner` on a task under a goal whose
  routine takes the home lab's alarms, or a question naming containers), from anyone but the
  reviewer, does not reach the desk: the task moves to `in_review`, the question is posted in
  its thread as "Draft for the owner: …", the member's turn ends, and the reviewer is woken
  the same way. The reviewer's own `ask_owner` goes to the desk.
- **The reviewer's briefing** says which it is. For work: check every claim in the thread
  against the tools; approve only what holds; send back with exactly what does not. For a
  draft: check every claim; if everything holds, ask the owner with the draft, corrected if
  needed; if not, send it back. Either way: "Send back any problem that no tool shows causing
  harm." (the harm rule, below).
- **The harm rule**, in the worker's instructions with contracts: "Report something as a
  problem only when a tool shows it causing harm: an item stuck in a queue, a container down
  or unhealthy, a failure that repeats. Errors or odd lines in a log are evidence to explain,
  not problems in themselves. Normal operation (e.g. an RSS sync that finds releases and grabs
  none) is not a problem."
- **Its verdict** is the `review` tool (below). The reviewer never gives one on its own work.
- **Before any question reaches the desk**, from anyone, its container-like names (in
  backticks, or shaped like the names Docker makes up, `word_word`) are compared with the
  home lab's containers, which the node tells HQ (`POST /v1/lab/names`, signed, when they
  change and every ten minutes). A name the lab does not have is listed on the card, "Not
  found in Endora's home lab: …", never quietly removed. Live, a lead on a small model put an
  invented container and an invented ping test on the desk; this is why that is now read
  first, and flagged.

### Runs

A run is one turn: `queued` → `running` → `succeeded`, `failed`, `stopped` or `timed_out`,
with `retry_waiting` between attempts. Each run records:

- the wake that started it and a snapshot of what the member was told;
- the steps it took and what each cost;
- the session it can resume next time, kept per member and task;
- why it ended, and a separate verdict on whether it moved the work forward.

A member is told, at the start of a run: its role, the task and its chain of why, what is new
in the thread since its last turn, the tasks it waits on, the brief if one is released, and
the tools it has. It then works, posts to the thread, updates the task, and ends.

### The tick

Every 30 seconds, in this order:

1. Timers that are due raise their wakes; **routines** that are due fire (a task for their
   member, or skipped while their last task is open); **check-backs** that are due wake their
   member on their task, if it is still open.
2. Runs whose container is gone become `failed`, with one retry.
3. Retries whose time has come are queued again (at most two, a minute apart).
4. Queued runs start where a member has a free slot, budget can be reserved, and a runner is
   available.
5. Tasks that are `doing` with no run and no way forward get one recovery nudge; if that
   fails too, they go to the owner's desk as stuck.
   **Stranded assigned work** too: an open task (`todo`, or `blocked` with nothing left to
   wait on) that has an owner or an assignee, with no run queued, running or waiting to retry
   and no wake held for it for ten minutes, gets one wake for that member
   (`assignment_recovery`, which the back-off, a pause, the budget and the kill switch all
   hold as they hold any nudge; a paused member's work is left alone). Still stranded ten
   minutes after that, it goes to the owner's desk as stuck, once. It is never handed to
   anyone else.
6. Tasks whose waits have all finished are woken.

A late tick runs once; missed ticks are not replayed.

### Limits on concurrency

- A task has at most one live run. Two members never work the same task.
- A member may run a few tasks at once (default 2, set per member).
- A runner runs as many containers as it declares it can.

## 6. Workers and runners

### The worker interface

A worker is any agent runtime that can take a turn in a container and speak the team
protocol. HQ starts it through an **adapter**:

```rust
trait WorkerAdapter {
    /// Starts a turn in a fresh container on a runner. The context names the run, the
    /// member, the task and the protocol endpoint; it carries a token for this run only.
    fn start(&self, run: &RunContext) -> Result<Started, StartError>;
    /// Whether the turn is still alive.
    fn status(&self, run: RunId) -> Liveness;
    /// Asks the turn to stop, then ends it after the grace period.
    fn stop(&self, run: RunId, grace: Duration);
}
```

Adapters, in the order they are built:

1. **Claude worker**: the Claude Agent SDK in a container, on an API key paid from the
   owner's Max credits.
2. **Local worker**: a small loop on the local model (Ollama), for cheap jobs.
3. **Codex worker**: the Codex CLI on Sign in with ChatGPT, the owner's ChatGPT plan.
4. **DeepSeek worker**: an API worker on a prepaid, capped balance.

New adapters arrive as plugins later, with skills, under the same rules.

### The team protocol

Every worker, whatever its runtime, uses the same tools, served by HQ over MCP inside the
container:

| Tool | What it does |
| --- | --- |
| `my_turn` | What this turn is for: the task, its why, what is new, the brief if released |
| `claim` | Takes the task; refused if another member has it |
| `post` | Writes to the task's thread, at most 4,000 characters a post |
| `set_status` | Moves the task on, with a reason; `done` is refused (only the gate sets it), except a routine's check, which its member closes once taken |
| `create_task` | Creates a sub-task for another member, with what it waits on |
| `ask_owner` | Puts a question or a proposal on the owner's desk and ends the turn: HQ ends the run there and refuses any further call from it, and the worker exits 0. One waits at a time per member and task. The owner can answer on the card, into the task's thread, and seeing it wakes the member who asked |
| `check_back` | Wakes the member on its task again after 5–720 minutes, once, if it is still open: after a fix, to confirm it held |
| `run_gate` | Runs the tests and build on a runner and returns the result |
| `request_action` | Asks for a privileged action (open a pull request, for example); the gateway decides |
| `note_lesson` | The lead's and the reviewer's: `{ "lesson" }`, at most 300 characters, appended as "- <lesson> (Ada, 17 Oct)" to the task's goal's notes for the team (which every briefing carries). A lesson that would overflow the notes' 2,000 characters is refused: "the notes are full; ask the owner to tidy them". Each is a `lesson.noted` event and a notice on the owner's desk ("Ada noted: …"), so the owner sees and can edit it |
| `review` | The reviewer's only: its verdict on work in review, `approve` or `send_back`, with notes. `send_back` posts the notes, sets the task to `doing` and wakes its owner. `approve` closes work that needs no gate (a routine's task, or work under a goal the home lab's routine serves) as `done`; work that needs the gate stays in review until the gate passes, and the reviewer is told so |

**What a turn may touch** (engine rules, whatever the way in): `claim` and `set_status`
act only on the turn's own task and its sub-tasks, at any depth; `post` reaches those, the
task the turn's own hangs from, and any task whose thread names the member (`@name`, as a
wake reads it). The lead may claim, move and post on any task in a goal it leads (a team
has one lead, which leads every goal). Anything else is refused with a reason the worker can
act on, and only the gate sets `done`, for everyone.

Model use is not a tool: the node's model relay charges HQ for every call a turn makes, as it
makes it, so a worker never reports its own (it would pay twice). The worker's own
instructions (how to take a good turn) are written for Endora from scratch.

### Runners

**Deferred** (phase 3): pairing and the `team_runner` role are not built. The NAS runner went
first, configured by hand and started as its own compose profile (section 15, phase 4 as
built); the Pi followed, configured by its own role (section 15, the rings). What follows is
the design for later.

A runner is a small program on a host. It joins by pairing:

1. In HQ, the owner adds a runner and gets a pairing code that lasts ten minutes, with the
   exact command to use: the `team_runner` role in Endora's Ansible collection for the NAS
   and the Pi, or a one-line `docker run` for any other computer.
2. The runner presents the code and declares what it offers: architecture, Docker, GPU,
   test environments, how many containers at once.
3. The owner approves it on their desk.
4. Endora tests it: it can reach HQ and the model provider, and it cannot reach the LAN.

The network fence is never the runner's own: `docker_egress` on the Pi (ADR 0110: the
`endora_sandbox` network reaches the NAS's team gateway and nothing else) and on the NAS. Test environments start as containers; virtual machines come later.

## 7. The gateway and real actions

- Every privileged action a member asks for goes from HQ to Endora's gateway, which uses the
  existing rules: **Allowed**, **Ask first** or **Off** per action, and remembered approvals
  that match the exact arguments they were given for.
- **Code** goes to pull requests:
  - public repositories: on a mirror in **Forgejo**, the local git server;
  - private GitHub repositories: on GitHub itself, opened by the gateway with a
    fine-grained token that can open pull requests and cannot merge.
  The team never holds either credential, and only the owner merges.
- Everything the gateway allows, refuses or asks about is in Endora's audit log.

## 8. Briefs

The team works better knowing the owner, and must never see their private life.

1. For each goal, Endora picks the relevant pages of its brain and writes a brief: the facts
   the team needs, with names and places replaced by placeholders.
2. Private areas never go into a brief: mail, people, health and money.
3. The brief is a **proposal on the owner's desk**: the exact text, the pages it came from,
   and what was replaced. The owner approves, edits, or declines, and can ask for changes in
   the goal's thread.
4. Only a released brief reaches the team. Every brief ever released stays visible.
5. Later the owner may allow briefs from chosen pages without asking.

## 9. Models and money

**Order of providers** for each member: Claude (the Max credits), then Codex on Sign in with
ChatGPT once the day's credits are spent, then DeepSeek on its capped balance. Work that
carries a brief never goes to DeepSeek.

**Models by role:** Sonnet or Haiku for the developer, the reviewer and the lead; Haiku only
for the researcher; the local model for small jobs (summaries, naming, tidying), at no cost.
Opus is refused until the owner can approve it for a task (later).

**A brain per member** (owner's decision of 2026-10-17, [ADR 0109](../adr/0109-a-brain-per-member.md)):
each member has a `brain`, `local` by default or a Claude model its role may use by the rule
above (HQ refuses anything else). The owner sets it on the member's card on the HQ screen
("Brain: Local / Claude Sonnet / Claude Haiku", only what the role allows), through the node
(`POST /v1/hq/members/{id}/brain` → HQ's `POST /v1/members/{id}/brain`, recorded in the audit
log). The lead may think with Claude Sonnet, metered against the pool and $0.40 a turn; the other
members stay on the local model (qwen3:8b, thinking).

**The money**, by default (a monthly allowance of API credits that does not roll over; making
the pool and the shares settings the owner changes on screen is later: today they are the
defaults below):

| Share | Per month | Per day |
| --- | --- | --- |
| Endora itself (its own big-model decisions) | $30 | about $1.00 |
| The team pool | $70 | about $2.30 |
| Developer (40% of the pool) | | $0.93 |
| Lead (25%) | | $0.58 |
| Reviewer (20%) | | $0.47 |
| Researcher (15%) | | $0.35 |

- Each run may spend at most **$0.40**. Unused daily budget does not carry over.
- **The ledger** is append-only: a reservation is made before a run starts, use is reported
  while it runs, and the charge and release are written when it ends. Crossing a limit
  during a run stops the run then, not afterwards. A charge that arrives after its run has
  ended (a call still running when it was stopped) is still written and settled, on the
  day the run was booked; no one report counts for more than a run may spend.
- When the credits are gone, the team stops. It never moves to paid billing. Raising any
  limit is the owner's decision.

## 10. The owner's desk

Everything that needs the owner lands on their desk in HQ, and Endora's Now line and chat
badge show how many are waiting:

- goals Endora proposes, and briefs;
- hires, role changes and team names;
- budget raises and Opus requests;
- actions the gateway marks Ask first, and the bridge's approvals (ADR 0108): an action a member
  proposed, shown as exactly what will run, with Approve and Decline;
- stuck work;
- finished work, ready to merge;
- new runners.

Later a **CEO** member may take most of these, with the owner as the board. Six things never
leave the owner's desk:

1. changes to Endora's policy, permissions, tripwires or allowlists;
2. raising the pool, or any spend beyond the credits;
3. releasing a brief;
4. merging to main, or deploying to prod;
5. adding a runner;
6. the kill switch.

## 11. The kill switch and tripwires

The kill switch stops every run at once **and** makes Endora's gateway refuse every team
request, so it holds even if HQ itself misbehaves. Tripwires pull it automatically: a worker
trying to gain privileges, reaching anything outside its allowlist, or touching a credential.
Only the owner resumes, from their desk, after seeing what tripped it.

## 12. The event contract

HQ describes itself as a **snapshot** (everything on screen now) followed by a **stream of
events**. The office view, Endora's console, the handheld and the tests all use the same
contract. Every event has an id, a time, a kind and the ids it concerns; the stream can be
resumed from the last id seen.

| Kind | Carries |
| --- | --- |
| `goal.proposed`, `goal.changed` | goal, title, status |
| `task.created`, `task.changed` | task, goal, parent, owner, status, priority |
| `task.claimed`, `task.released` | task, member, run |
| `thread.posted` | task, author, a short preview |
| `wake.queued`, `wake.folded`, `wake.held`, `wake.refused` | member, task, reason |
| `run.started`, `run.step`, `run.ended` | run, member, task, step label, cost, result |
| `gate.result` | task, passed or failed, what failed |
| `desk.added`, `desk.decided` | item, kind, decision |
| `budget.changed`, `budget.crossed` | member or pool, spent, limit |
| `runner.changed` | runner, status, capacity |
| `tick` | number, counts |
| `stopped`, `resumed` | by whom, why |

The office maps these to what the owner sees: a wake is an envelope flying from the server
closet, a claim is a walk to the whiteboard, a run step is a speech bubble, a gate result is
a sticky note changing colour, a desk item is a card on the board-room table, a budget is the
burn gauge, and `stopped` is the kill-switch lever.

Approvals and the kill switch are never sent through the view: they go to Endora's own,
authenticated routes.

## 13. The office view

- A screen in Endora's console, **phone first**: the office scales to the width, people and
  sticky notes are tapped to see their detail and their chain of why, and the desk, activity
  and the team sit below as tabs.
- Each member picks a look from the characters when hired; a team's name hangs on its wall.
- A **simulated runner** (scripted members, no model, no cost) drives the engine's tests and a
  labelled demo mode, so the view is built and tested before real workers exist.
- The art is from [Pixel Agents](https://github.com/pixel-agents-hq/pixel-agents) (MIT) and
  [MetroCity by JIK-A-4](https://jik-a-4.itch.io/metrocity-free-topdown-character-pack) (CC0).
  Its licence and credits travel with every copy of the files. Its pet sprites and its
  bundled font are not used.
- A working prototype, with a simulated engine following these rules, is in
  [endora-hq/prototype](endora-hq/prototype/index.html).

## 14. Storage

HQ's own SQLite database holds: teams, members, goals, tasks, task waits, thread posts,
wakes (as receipts), runs and their steps, sessions per member and task, the budget ledger,
desk items, briefs (the released text and where it came from), runners, settings, and the
event log the stream is served from. Endora's audit log, secrets and brain stay in the node.

## 15. Build order

Each phase is complete, tested and merged with the owner's OK before the next.

1. **The team core**: goals, tasks, claiming, wakes, runs, the tick, the ledger, events, and
   the simulated runner, with tests for folding, holding, claiming, recovery and the budget.
   No models.
2. **HQ and the office**: the HQ service, its container, and the office screen in the
   console, running on the simulated runner.
3. **Runners**: the runner program, pairing, the `team_runner` Ansible role and the network
   fences, on the NAS and the Pi. Needs step 5 (the Pi on its VLAN). **Deferred**: the NAS
   runner and its fence went first, in phase 4; pairing, the role and the Pi come later.
4. **Real workers**: the Claude worker, the team protocol over MCP, and budgets that stop a
   run mid-way. Needs step 4 (the API key).
5. **Real actions**: the gateway's pull requests to Forgejo and GitHub, and the gate.
6. **Briefs**: from the brain, through the owner's desk.
7. **More capacity**: the local, Codex and DeepSeek workers.

### Phase 4, as built (owner's decisions of 2026-10-16)

The owner chose Endora's **own small Rust worker** for now, not the Agent SDK, and that it
thinks with the **local model** first; Claude through the relay, per member, followed on
2026-10-17 ([ADR 0109](../adr/0109-a-brain-per-member.md): the lead may think with Sonnet). The
team pool stays $70 a month. The pieces and their contracts:

1. **The team protocol** (built): `POST /v1/team/mcp` on HQ, JSON-RPC, opened by a turn's own
   token (`Authorization: Bearer <token>`).
2. **The model relays** in the node, outside `/v1`, opened by the same token. The workers
   reach them on a plain-HTTP listener of their own (`ENDORA_TEAM_ADDR`, `0.0.0.0:8788` in
   compose, published on the house's network for the Pi since ADR 0110, every route on it
   needing a token), which serves the relays, the team's contracts (below) and the team
   gateway, and nothing else: the node's main port
   is HTTPS with a self-signed certificate a worker cannot trust.
   - `POST /team-relay/v1/messages`: Anthropic, with the owner's key, capped and charged
     (built). It passes on only what it can meter: the plain message fields, the worker's
     own tools (no server tools), inline content (nothing fetched from a URL or a file),
     and a short list of betas that cost nothing extra; anything else is refused, and other
     betas are dropped. Only a turn HQ says is cloud (`"cloud": true`, ADR 0109) is passed on:
     a turn on the house's model may hold Confidential results, so its calls (and its counting)
     are refused with 403. The model must be one the member's role may use (Sonnet or Haiku;
     Haiku only for the researcher); Opus is refused until the owner can approve it for a
     task. The prompt is counted first (free) and its cost taken off what the turn has left;
     the reply's `max_tokens` is capped by the rest, and a prompt that leaves nothing is
     refused with 402. A turn's calls go one at a time, each after the one before was
     charged. A stream cut before its last usage is charged its whole cap.
   - `POST /team-relay/local/v1/chat/completions`: the local model, OpenAI-compatible, not
     streamed. The relay checks the token with HQ, sends the call to the first enabled
     Everyday entry on the house's network (else the model the node was installed with),
     using that entry's model whatever the worker named, and records the use as `team-local`
     at no cost.
3. **The worker**, `endora-worker` (crate `app/worker`, in the same image). It reads
   `ENDORA_RUN_TOKEN`, `ENDORA_HQ_MCP` (`http://hq:8790/v1/team/mcp`), `ENDORA_MODEL_URL`
   (`http://node:8788/team-relay/local/v1`) and `ENDORA_MOST_ROUNDS` (default 12), and
   `ENDORA_CONTRACTS` (default: `/team-relay/contracts` beside the model relay), and
   `ENDORA_MODEL` (the model it names, default `local`; the local relay uses its own whatever
   the name). A model named `claude-…` is a cloud brain (ADR 0109): the worker speaks
   Anthropic's Messages API to `ENDORA_MODEL_URL` + `/messages` (its token as the bearer,
   `anthropic-version`, its instructions as `system`, the tools as Anthropic's `tools`, each
   call a `tool_use` answered by a `tool_result`, a refusal with `is_error`, `max_tokens`
   4,096, which the relay lowers to what the turn can pay for); otherwise OpenAI's chat
   completions. One loop serves both. It initialises MCP, lists the team's tools and the node's contracts (a node that
   offers none leaves the team's tools), calls `my_turn`, claims its turn's task before the
   model's first round (and tells the model it did), then loops: the model answers or calls tools,
   each tool call goes where its tool came from (`tools/call` on HQ, or the contract on the
   node, whose refusal is a tool error), until the model stops calling tools or the rounds
   run out. The same call (tool and arguments) asked again straight after is answered from
   what the turn has, unsent; asked again later and saying nothing new, it is answered the
   same way. A last answer with no tool call is posted to its task's thread. An
   answer of 401 or 402 from HQ or the relay means the turn is over: it exits at once. A
   question put on the owner's desk (`ask_owner`) ends the turn too: it stops there. Exit
   0 is a turn taken; exit 3 means HQ or the relay said the turn was over; anything else
   failed. Its instructions are written for Endora, and
   with contracts offered say what they may do on the home lab and that anything else is
   the owner's.
4. **The runner adapter** in HQ (`RemoteRunner`), used when the demo is off. It keeps, in
   memory, the turns waiting to start, the ones running, how each ended, and stops asked
   for. The runner speaks to HQ with its own token (`HQ_RUNNER_TOKEN_FILE`, made like
   HQ's): `POST /v1/runner/next` (a turn to start: run, lease, token, member, role, brain, or 204),
   `POST /v1/runner/{run}/ended` (`{ "succeeded": bool, "lease": n }`; 409 for a lease that
   is not the run's current one, a report from an earlier attempt), `GET /v1/runner/stops` (the runs
   to stop). A turn the runner never reports is ended by the engine's longest-turn limit.
5. **The runner**, `endora-runner` (crate `app/runner`): asks HQ for work every two seconds
   and starts each turn as a fresh container through Docker's API on its socket: the worker
   image only, `endora-worker`, on the internal `team` network, read-only, no capabilities,
   no new privileges, 256 MB, one CPU, 128 processes, a small `/tmp` and a 1 MB `/data` in
   memory (the image declares `/data` a volume, which would otherwise be an unbounded disk),
   labelled with its run and lease. The turn's brain picks one of the runner's own two relays:
   `local` gives the worker `WORKER_MODEL_URL` (the local relay); a Claude model's name (only
   `claude-` and lowercase letters, digits and dashes; anything else is refused and reported
   failed) gives it `WORKER_CLOUD_URL` (`http://node:8788/team-relay/v1`, the Anthropic relay
   on the same team listener) and `ENDORA_MODEL`. It reports how each ended, removes it, kills one asked
   to stop or past 30 minutes, and tries a failed kill again every round until it takes.
   HQ cannot choose the image, mounts or network.
6. **The fence** is a Docker network marked internal, with its gateway isolated
   (`com.docker.network.bridge.gateway_mode_ipv4: isolated`, so the bridge has no address on
   the host): worker containers can reach only the node and HQ, which join it too, and
   nothing else (no internet, no LAN, no NAS host). The
   runner, which holds Docker's socket, is its own compose profile, off until the owner
   turns it on.

### Routines, alarms and check-backs (owner's decision of 2026-10-17, [ADR 0107](../adr/0107-routines-alarms-and-check-backs.md))

Built, so the team keeps working on "Keep the home lab healthy" without anyone looking for
work:

- **Routines** in HQ's store (`routines`): title, description, goal, assignee, `every_ms`
  (15 minutes to 30 days) and next due, on/off, and whether it takes the home lab's alarms.
  Firing makes a `todo` task for the assignee with why it fired; one open task a routine (a
  firing on the clock is skipped while it is open); late ticks fire once. A routine's check is
  closed as `done` by its member once taken. Kill switch, a paused member, a spent budget or a
  goal that is not active skip a firing. HQ: `POST /v1/routines`, `POST /v1/routines/{id}`
  (`{ "enabled" }`), `POST /v1/routines/{id}/fire`; listed in the snapshot. The node passes
  them through as `/v1/hq/routines…`, recorded in its audit log; the HQ screen has a Routines
  card (rhythm, last run, on/off, run now, and an add form: title, every N hours, who).
- **Alarms**: the node's observer tells HQ (`POST /v1/alarms`, `{ "text" }`, signed) when a
  container or service goes from well to unhealthy, restarting, exited, dead or unavailable:
  only a change, never what was wrong at the node's start, never Endora's own, at most one a
  container an hour. HQ fires the first routine that is on and takes alarms: a new task, or a
  post in its open task that wakes its member. The demo takes none.
- **Check-backs**: the team tool `check_back { minutes, why }` (5–720) wakes the turn's member
  on its task later, if it is still open; a newer one from the member replaces its older, at
  most three a task; the next briefing says why it came back.
- **First start with the demo off**: with no routine yet, an active goal titled "Keep the home
  lab healthy" and a lead, HQ makes "Check the home lab", every 6 hours, for the lead, taking
  alarms, once, recorded in the feed.

### Contracts (owner's decision of 2026-10-17, [ADR 0106](../adr/0106-team-contracts.md))

No keys and no raw access: the team reaches the home lab only through **contracts**, narrow
abilities the node declares and decides. Reads are free; a safe fix runs on its own within
limits and is told to the owner as a **notice** on the desk (seen or dismissed); anything
riskier will ask first (none yet). On the team listener, opened by the turn's token and
checked with HQ as the relays are:

- `GET /team-relay/contracts`: the contracts as MCP tools; `POST /team-relay/contracts/{name}`:
  one call, arguments as the JSON body, answered `{ "text" }` or `{ "error" }` with a status.
- `homelab_status` (read, Internal): the observer's snapshot, compact, marked stale.
- `container_logs` (read, Confidential): the newest 100 lines, at most 16 KB, scrubbed of
  secrets; never Endora's own containers; a plain refusal without Portainer. Given to turns
  on the local model, and to the lead's cloud turns by the owner's decision (ADR 0109); a
  cloud developer, reviewer or researcher is neither offered it nor given it (403).
- `restart_container` (automatic act): never Endora's own, never what a restart will not fix
  (restarting, out of memory, crash-looping); once a container an hour and three an hour in
  all, kept in the node's database; the state read back; a notice to the owner through
  `POST /v1/team/notice` on HQ.
- Every call, done or refused, is in the node's audit log with the member and the run. The
  owner switches `restart_container`, or every contract, off on the HQ screen
  (`/v1/hq/contracts`).
- HQ started with the demo off puts the demo's goals and their open tasks away (dropped, their
  queued turns ended, a notice on the desk), so none of it becomes a real turn. The owner sets
  a real goal and hands the lead its first task from the HQ screen. The board hides a dropped
  goal's tasks, finished or not, so the retired demo's notes do not bury the owner's own.

### The bridge (owner's decision of 2026-10-17, [ADR 0108](../adr/0108-the-bridge.md))

The three contracts above are the generic container tools. Beside them, the **bridge** offers
every capability Endora has and every tool of its tool servers, decided by Endora's own policy
in `domains/capabilities/src/bridge.rs`, in this order:

- **Never offered**: off; Restricted (and anything unclassified); Endora's administration (the
  settings-only connection entries); the owner's own (`mail`, `own_activity`, `read_page`,
  `brief`, `image_review`, `location`, and tool-server tools named for mail, chat, brain,
  memory, Endora, the team or money); and what is not set up.
- **Tier**: Confidential only to a turn on a local model, but for what the owner allowed the
  lead on Claude (ADR 0109): `container_logs`, `sonarr_queue` and `radarr_queue`. HQ's
  `/v1/team/turn` answer carries `"cloud": true` (and the `brain`) for a member whose brain is
  not local, asked on every call; the node then neither lists nor runs any other Confidential
  tool for it (the download actions included).
- **Profile by role** (a table in code, for a later owner setting): lead and developer read and
  act; reviewer and researcher read.
- **Kind**, from Endora's knowledge: a skill reads when it declares the observation band; a
  tool server's tool reads only when the owner named it the server's reader. Each listed tool
  carries `"kind": "read" | "act"`, and every act requires a `why`.
- **Stance**: a read runs through the explicit-invoke path and keeps a stranger's mark; an act
  that is on runs at once, three an hour a tool across the team (`team_acts`), and is a notice;
  an act that asks, any tool server's act the owner has said nothing about, and an act given
  arguments after the turn read a stranger's words, is stored on the node (`team_approvals`)
  and put on the desk through `POST /v1/team/approval`. The member reads "On the owner's desk
  for approval; you are woken when they decide." with `"asked_owner": true`, and the turn ends.
- **Approval**: HQ's desk item `approval` counts as the member's question (one waits a task).
  The owner's Approve reaches the node, which checks HQ's desk still holds it as asked, has HQ
  decide it, re-checks the stance and role, runs exactly the stored call and posts what came of
  it through `POST /v1/team/approval/{id}/outcome`, which writes it in the thread as Endora's
  and wakes the member. Decline, and a day undecided (HQ's tick), run nothing and say so in
  the thread. The card shows the tool, the arguments one a line and the reason, as text.
- **Sonarr and Radarr**: the queue read says why an item is stuck; `*_retry_import` (on)
  asks the app to check its downloads again; `*_remove_and_search` (ask) removes one item from
  the queue and its client, blocklists the release and searches again. An app is a row of
  `ARR_APPS`.

### The owner's voice (owner's decision of 2026-10-17, [ADR 0108](../adr/0108-the-bridge.md))

- **A reply on any task**: the "Looking at" panel has a reply box. `POST /v1/hq/tasks/{id}/post`
  on the node, `POST /v1/tasks/{id}/post` on HQ: the owner's words in the thread, and whoever
  has the task woken at once (`owner_replied`), mention or not, past the back-off. Any post of
  the owner's wakes the same way; a finished task refuses it.
- **A goal's notes for the team**: at most 2,000 characters, written on the "Notes for the
  team" card (`POST /v1/hq/goals/{id}/note`, `POST /v1/goals/{id}/note`), shown in the
  snapshot's goals, and carried near the top of every briefing for a task under the goal as
  "Notes from the owner for this goal: …". The lead and the reviewer add lessons to them with
  `note_lesson` (section 6); the instructions say "When the owner corrects you, or you learn
  something about this house that the team will need again, note it with note_lesson so nobody
  asks twice."

### The rings (owner's decisions of 2026-10-17, [ADR 0110](../adr/0110-the-teams-rings.md))

- **Three rings**: the sandbox (the Pi, a fresh container a turn), the lab (the Pi, long-lived
  test containers, later), production (the NAS, only through the contracts and the bridge,
  with evidence and undo, later). Agents never hold keys; Endora is the gatekeeper.
- **The team gateway**: the node's team listener, published on 8788 (every route needs a
  token), passes `/team-gateway/mcp` to HQ's `/v1/team/mcp` and `/team-gateway/runner/...` to
  HQ's `/v1/runner/...`, the method, body and caller's `Authorization` untouched, to HQ's URL
  and nowhere else (4 MB in, 8 MB out, ten seconds, no redirects followed). HQ stays
  unpublished.
- **The Pi's runner**: `HQ_URL=http://<nas>:8788/team-gateway`. Through the gateway its
  defaults are the `endora-workbench` image on `endora_sandbox`, with the team protocol and the
  model relays at the gateway's listener; each turn is a workbench (`ENDORA_WORKBENCH=1`,
  `/work` in memory owned by uid 10001, a gigabyte, two CPUs, 256 processes). `WORKBENCH=0`
  or `1`, and every `WORKER_*` and `TEAM_NETWORK`, override.
- **Ring 1 is offline**: no DNS, no internet, no house network but the gateway. The egress
  proxy (an allowlist grown by the owner's Approve on desk cards, then optionally inspection
  with a team CA), canaries and credential injection are designed and deferred (ADR 0110).

### The workbench (ring 1, owner's decision of 2026-10-17, ADR 0110)

A disposable **sandbox** on the Pi, one container a turn, where a member has a real shell.
Contracts and the bridge stay the only way to act on the home lab (ring 3); the workbench is
for working things out: scripts, parsing what the tools returned, trying commands.

- **The image**: the Dockerfile's `workbench` stage, tagged `endora-workbench:latest`
  (`docker buildx build --platform linux/arm64 --target workbench -t endora-workbench:latest --load .`),
  never the node's image. Debian slim with bash, git, curl, ca-certificates, python3, jq,
  dnsutils, ping, procps and less, and `endora-worker`. It runs as the unprivileged
  `workbench` user (uid 10001) with a read-only root; `/work` is its home and workspace, a
  tmpfs the runner mounts owned by that user (`uid=10001,gid=10001`).
- **Offline**: ring 1 reaches nothing but the team gateway (the egress proxy is designed and
  deferred, ADR 0110), so no proxy variables are set and no extra CA is trusted.
- **The `shell` tool**: offered only when `ENDORA_WORKBENCH=1`. `{ "command", "why",
  "seconds"? }` runs `bash -lc` in `/work`, for 60 s unless told, at most 300, with its whole
  process group killed when out of time; stdout and stderr together, kept to 16 KB (the first
  and last 8 KB, with how much was cut between), and the exit code said. It goes to neither HQ
  nor the node. The command's environment has no `ENDORA_RUN_TOKEN`, and the token is replaced
  wherever it appears in the output. A call without `why` is refused. Each call is in the
  turn's line, the command cut to 60 characters: "ran `…` (exit 2)", never as a read.
- **The instructions** gain a paragraph: a real shell in a disposable, offline sandbox; the
  house's services and the internet are not reachable from it, and the house is used through
  the team's tools (the bridge and the contracts); `/work` is gone when the turn ends, so
  durable findings go in the task's thread (or with `note_lesson`, for a member who has it).

## 16. Later, and not at all

- **Later:** skills and adapter plugins, including adapters to other systems; more teams; a
  CEO member; virtual-machine test environments; the office on the handheld.
- **Not at all:** members holding any credential; anything merged or deployed without the
  owner; paid billing beyond the credits without the owner; the team reading the brain,
  mail or chat.
