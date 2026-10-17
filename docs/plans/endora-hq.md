# Endora HQ: the design

Agreed with the owner in a design interview on 2026-10-16 (questions Q1–Q34), recorded in
[ADR 0102](../adr/0102-endora-hq.md). Prepared with AI assistance (Claude). This is step 7 of
[the always-on butler plan](always-on-butler.md). Phases 1 and 2 (section 15) are built: the team
core in `domains/team`, the HQ service in `app/hq`, and the office screen; the rest is not yet.

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
  ┌──────────────┴──── Runners: the Pi (VLAN), Docker on the NAS (fenced) ─────┐
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

1. **Isolation.** Every run is a fresh container on a fenced runner (the VLAN on the Pi,
   `docker_egress` on the NAS) that can reach HQ and the model provider only.
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
and a recovery nudge.

Every wake leaves a receipt, whatever happens to it: **queued**, **folded** into a turn the
member is already taking on the same task, **held** because another member is working that
task (and released, oldest first, when that turn ends), **refused** (and why: the kill
switch, a paused member, an exhausted budget, a duplicate), **started**, or **finished**.

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

1. Timers that are due raise their wakes.
2. Runs whose container is gone become `failed`, with one retry.
3. Retries whose time has come are queued again (at most two, a minute apart).
4. Queued runs start where a member has a free slot, budget can be reserved, and a runner is
   available.
5. Tasks that are `doing` with no run and no way forward get one recovery nudge; if that
   fails too, they go to the owner's desk as stuck.
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
| `set_status` | Moves the task on, with a reason; `done` is refused (only the gate sets it) |
| `create_task` | Creates a sub-task for another member, with what it waits on |
| `ask_owner` | Puts a question or a proposal on the owner's desk and ends the turn: HQ ends the run there and refuses any further call from it, and the worker exits 0. One waits at a time per member and task. The owner can answer on the card, into the task's thread, and seeing it wakes the member who asked |
| `run_gate` | Runs the tests and build on a runner and returns the result |
| `request_action` | Asks for a privileged action (open a pull request, for example); the gateway decides |

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

A runner is a small program on a host. It joins by pairing:

1. In HQ, the owner adds a runner and gets a pairing code that lasts ten minutes, with the
   exact command to use: the `team_runner` role in Endora's Ansible collection for the NAS
   and the Pi, or a one-line `docker run` for any other computer.
2. The runner presents the code and declares what it offers: architecture, Docker, GPU,
   test environments, how many containers at once.
3. The owner approves it on their desk.
4. Endora tests it: it can reach HQ and the model provider, and it cannot reach the LAN.

The network fence is never the runner's own: the VLAN rules on the Pi, `docker_egress` on the
NAS. Test environments start as containers; virtual machines come later.

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

**Models by role:** Sonnet for the developer, the reviewer and the lead; Haiku for the
researcher; the local model for small jobs (summaries, naming, tidying), at no cost; Opus only
when the owner approves it for a task.

**The money**, by default (a monthly allowance of API credits that does not roll over; the
amounts are settings the owner changes):

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
- actions the gateway marks Ask first;
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
   fences, on the NAS and the Pi. Needs step 5 (the Pi on its VLAN).
4. **Real workers**: the Claude worker, the team protocol over MCP, and budgets that stop a
   run mid-way. Needs step 4 (the API key).
5. **Real actions**: the gateway's pull requests to Forgejo and GitHub, and the gate.
6. **Briefs**: from the brain, through the owner's desk.
7. **More capacity**: the local, Codex and DeepSeek workers.

### Phase 4, as built (owner's decisions of 2026-10-16)

The owner chose Endora's **own small Rust worker** for now, not the Agent SDK, and that it
thinks with the **local model** first; Claude through the relay comes later, per member. The
team pool stays $70 a month. The pieces and their contracts:

1. **The team protocol** (built): `POST /v1/team/mcp` on HQ, JSON-RPC, opened by a turn's own
   token (`Authorization: Bearer <token>`).
2. **The model relays** in the node, outside `/v1`, opened by the same token. The workers
   reach them on a plain-HTTP listener of their own (`ENDORA_TEAM_ADDR`, `0.0.0.0:8788` in
   compose, not published), which serves the relays and nothing else: the node's main port
   is HTTPS with a self-signed certificate a worker cannot trust.
   - `POST /team-relay/v1/messages`: Anthropic, with the owner's key, capped and charged
     (built). It passes on only what it can meter: the plain message fields, the worker's
     own tools (no server tools), inline content (nothing fetched from a URL or a file),
     and a short list of betas that cost nothing extra; anything else is refused, and other
     betas are dropped. The model must be one the member's role may use (Sonnet or Haiku;
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
   (`http://node:8788/team-relay/local/v1`) and `ENDORA_MOST_ROUNDS` (default 12). It
   initialises MCP, lists the tools, calls `my_turn`, then loops: the model answers or calls
   tools, each tool call goes to `tools/call`, until the model stops calling tools or the
   rounds run out. A last answer with no tool call is posted to its task's thread. An
   answer of 401 or 402 from HQ or the relay means the turn is over: it exits at once. A
   question put on the owner's desk (`ask_owner`) ends the turn too: it stops there. Exit
   0 is a turn taken; anything else failed. Its instructions are written for Endora.
4. **The runner adapter** in HQ (`RemoteRunner`), used when the demo is off. It keeps, in
   memory, the turns waiting to start, the ones running, how each ended, and stops asked
   for. The runner speaks to HQ with its own token (`HQ_RUNNER_TOKEN_FILE`, made like
   HQ's): `POST /v1/runner/next` (a turn to start: run, lease, token, member, or 204),
   `POST /v1/runner/{run}/ended` (`{ "succeeded": bool, "lease": n }`; 409 for a lease that
   is not the run's current one, a report from an earlier attempt), `GET /v1/runner/stops` (the runs
   to stop). A turn the runner never reports is ended by the engine's longest-turn limit.
5. **The runner**, `endora-runner` (crate `app/runner`): asks HQ for work every two seconds
   and starts each turn as a fresh container through Docker's API on its socket: the worker
   image only, `endora-worker`, on the internal `team` network, read-only, no capabilities,
   no new privileges, 256 MB, one CPU, 128 processes, a small `/tmp` and a 1 MB `/data` in
   memory (the image declares `/data` a volume, which would otherwise be an unbounded disk),
   labelled with its run and lease. It reports how each ended, removes it, kills one asked
   to stop or past 30 minutes, and tries a failed kill again every round until it takes.
   HQ cannot choose the image, mounts or network.
6. **The fence** is a Docker network marked internal, with its gateway isolated
   (`com.docker.network.bridge.gateway_mode_ipv4: isolated`, so the bridge has no address on
   the host): worker containers can reach only the node and HQ, which join it too, and
   nothing else (no internet, no LAN, no NAS host). The
   runner, which holds Docker's socket, is its own compose profile, off until the owner
   turns it on.

## 16. Later, and not at all

- **Later:** skills and adapter plugins, including adapters to other systems; more teams; a
  CEO member; virtual-machine test environments; the office on the handheld.
- **Not at all:** members holding any credential; anything merged or deployed without the
  owner; paid billing beyond the credits without the owner; the team reading the brain,
  mail or chat.
