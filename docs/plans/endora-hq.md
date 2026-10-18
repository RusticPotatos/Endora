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
workbenches, reaching HQ through the node's team gateway; and the CEO and the chain of command
([ADR 0111](../adr/0111-the-ceo-and-the-chain-of-command.md)): the owner is the board. Phase 3's pairing is deferred, and so
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
- **The team** starts as one: a lead, a developer, a researcher and a reviewer, with a **CEO**
  above the lead (ADR 0111) that runs the team for the owner, who is the board. It can propose
  more members (a manager or product owner, say) and a name for itself. The owner decides.

## 2. Words

| Word | Meaning |
| --- | --- |
| Owner | The person Endora works for. In HQ they are also the board: the last word on everything. |
| CEO | The member above the lead (ADR 0111): answers the lead's questions, reviews the house daily, keeps the house log, and puts to the board only what the owner alone decides, as a decision with its recommendation. |
| Chain of command | Where a member's question goes: the developer's, researcher's and reviewer's to the lead, the lead's to the CEO, the CEO's to the board. |
| House log | The CEO's own document of the house, per team, which the CEO and the lead read every turn and the owner edits. Since ADR 0122, the house page's timeline; kept read-only for a release. |
| World | What the team knows (ADR 0122): the house page's sections and timeline, the derived lab and team pages, and stories, each piece with a stage; canon is the owner's word. |
| Situation | What a turn starts from (ADR 0122): fresh, continuing, after a gap, handover or recalled; it decides what of the world the briefing carries, within the member's budget. |
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

**A project** (2026-10-17, [ADR 0113](../adr/0113-projects-budgets-and-reports-to.md)) is a
piece of one goal with its own lead (a lead or the CEO), a status (proposed, active, paused,
done, dropped) and, if the owner gives it one, a monthly cap. A task may be in a project of its
own goal; its sub-tasks stay in it; a routine may name one.

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
open task, or Endora's word there that what alarmed is back up
([ADR 0107](../adr/0107-routines-alarms-and-check-backs.md)). An alarm is a moment: the member
checks the state now before acting, and closes the task if everything has recovered.

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
  the same way. The reviewer's own `ask_owner` goes to the desk. **With a CEO at work this is
  the CEO's path alone** (below): everyone else's question goes up the chain.

**The chain of command** (owner's decisions of 2026-10-17, [ADR 0111](../adr/0111-the-ceo-and-the-chain-of-command.md)).
While the team has a CEO at work (not paused):

- `ask_owner` from the developer, the researcher or the reviewer goes to the lead (the CEO when
  no lead is at work); the lead's goes to the CEO. It is posted in the task's thread as
  "Question for <name>: …", the asker's turn ends, and the one above is woken with
  `question_asked` ("<asker> asks you: …" in its briefing). Its first post after the question
  that is not itself a question up the chain or a draft wakes the asker with
  `question_answered`. One question a member about a task waits for its answer at a time.
- Only the CEO's `ask_owner` reaches the board's desk, as a **decision**
  (`{ "decision", "recommendation", "why" }`), through the reviewer first as a draft ("Mara, the
  CEO, wrote this decision for the board"); the reviewer's `ask_owner` then passes it on as the
  CEO's, or `review` sends it back. The board approves, declines or answers on the card; its
  word goes in the thread and wakes the CEO (`proposal_decided`).
- The CEO wakes only for escalations and its own work: a mention, an alarm, a look round and a
  review request are refused as `not_for_the_ceo`. Its routine "Daily review" comes every 24
  hours.
- With no CEO at work, everything above this paragraph holds as it did.
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
- why it ended, and a **label** worked out from its record when it ends
  ([ADR 0114](../adr/0114-evidence-acceptance-criteria-and-the-tester.md)): `advanced` (a task
  moved or was made, a project changed, an act succeeded), `blocked` (it left its task blocked
  or put a blocker to someone), `checked` (readings: receipts), `talk only` (posts alone) or
  `empty`. Re-claiming a task the member already has is not a change, and posts are only talk,
  so the empty-turn back-off holds a turn that did nothing.

A turn does not resume a model session: each starts fresh, and continuity comes from the
briefing ("what is new since your last turn"), the thread, the goal's notes and lessons, and
the CEO's house log. Per-task session resume is not built; it is reconsidered if turns need
it.

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
| `create_task` | Creates a sub-task for another member, with what it waits on. Refused when an open task under the same goal is already about the same home-lab containers (named in its title, or in its description when the title names none): "#664 already covers this (deluge, nzbget, prowlarr): add to it with post in #664, rather than a second task". The member's own task above the new one, and what is above it, never counts (ADR 0107) |
| `ask_owner` | Asks a question or puts a proposal and ends the turn: HQ ends the run there and refuses any further call from it, and the worker exits 0. With a CEO at work it goes up the chain, in the task's thread (section 5); the CEO's is `{ "decision", "recommendation", "why" }`, a decision for the board's desk after the reviewer. With no CEO it goes on the owner's desk. One waits at a time per member and task. The owner can answer on the card, into the task's thread, and seeing it wakes the member who asked |
| `check_back` | Wakes the member on its task again after 5–720 minutes, once, if it is still open: after a fix, to confirm it held |
| `run_gate` | Runs the tests and build on a runner and returns the result |
| `request_action` | Asks for a privileged action (open a pull request, for example); the gateway decides |
| `note_lesson` | No longer offered (ADR 0122): a worker that still calls it gets a story marked to retell, in its writer's area, under a handle from the lesson's first words; a `lesson.noted` event and a notice on the owner's desk ("Ada noted: …") as before. The goal's notes are not changed |
| `note_house` | The CEO's only: `{ "entry" }`, at most 500 characters, a dated line on the house page's timeline (ADR 0122; before it, the house log). A `house.noted` event |
| `rewrite_house` | Refused for a release (ADR 0122), saying to use `write_world` and `note_house`; then removed |
| `read_world` | Every member's (ADR 0122): `{ "address" }`, `world:house`, `world:house#<anchor>`, `world:lab`, `world:team` or `story:<handle>`; each piece says its stage, or "unconfirmed since <date>" |
| `tell_story` | Every member's (ADR 0122): `{ handle, moment, reflex, instead, because, evidence, stage?, warrant?, taught? }`, in the member's scope by role; retells a story marked to retell under its own handle; an insight is the scope's steward's |
| `write_world` | The CEO's (ADR 0122): a section of the house page, `{ heading, headline, body?, stage, warrant?, evidence, anchor? }`, an observation or an insight on its evidence; never canon |
| `verify_world`, `retire_world` | The CEO's and the leads', for what they steward (ADR 0122): confirm on fresh evidence; retire with a reason, or supersede naming the successor; canon is the owner's |
| `propose_canon` | The CEO's and the leads' (ADR 0122): a trusted insight and a warrant, to the owner's desk |
| `read_doc` | Every member's (ADR 0116; the CEO's only before): `{ "path", "part"? }`, one of Endora's public library (`docs/standards.md`, `docs/adr/*.md`, `docs/plans/*.md`, `docs/drafts/*.md`, embedded in HQ at build time), 20,000 characters a part; a folder alone lists them; any other path is refused |
| `review` | The reviewer's only: its verdict on work in review, `approve` or `send_back`, with notes. `send_back` posts the notes, sets the task to `doing` and wakes its owner. `approve` closes work that needs no gate (a routine's task, or work under a goal the home lab's routine serves) as `done`; work that needs the gate stays in review until the gate passes, and the reviewer is told so |

**What a turn may touch** (engine rules, whatever the way in): `claim` and `set_status`
act only on the turn's own task and its sub-tasks, at any depth; `post` reaches those, the
task the turn's own hangs from, and any task whose thread names the member (`@name`, as a
wake reads it). The lead may claim, move and post on any task in a goal it leads (a team
has one lead, which leads every goal). Anything else is refused with a reason the worker can
act on, and only the gate sets `done`, for everyone.

Model use is not a tool: the node's model relay charges HQ for every call a turn makes, as it
makes it, so a worker never reports its own (it would pay twice). The worker's own
instructions (how to take a good turn) are written for Endora from scratch. HQ's briefing names
its own tools as "Your tools at HQ", with Endora's tools on the home lab beside them, and the
worker's instructions name the container tools the node offered the turn as its own
("this turn they are yours: homelab_status, ..."): live, a lead on Claude read a briefing that
listed only HQ's tools as all it had, and said three times it could not read the home lab,
though `homelab_status` was in its tool list.

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

**Providers:** Claude only, through the node's relay, or the house's own model (the owner's
decision of 2026-10-17, ADR 0111). No other provider is added.

**Models by role** (ADR 0111): Opus or Sonnet for the CEO; Sonnet or Haiku for the lead, the
developer and the reviewer; Haiku only for the researcher; the local model for any of them, at
no cost. Opus is the CEO's alone. The owner's choice: the CEO on Opus (as HQ seats it), the lead
on Sonnet, the developer and the reviewer local, the researcher on Haiku.

**A brain per member** (owner's decision of 2026-10-17, [ADR 0109](../adr/0109-a-brain-per-member.md)):
each member has a `brain`, `local` by default or a Claude model its role may use by the rule
above (HQ refuses anything else). The owner sets it on the member's card on the HQ screen
("Brain: Local / Claude Opus / Claude Sonnet / Claude Haiku", only what the role allows), through the node
(`POST /v1/hq/members/{id}/brain` → HQ's `POST /v1/members/{id}/brain`, recorded in the audit
log). The CEO's turns may spend $1.00 each, everyone else's $0.40; HQ reserves at most that and
the relay holds each call to it.

**The money**, by default (a monthly allowance of API credits that does not roll over; making
the pool and the shares settings the owner changes on screen is later: today they are the
defaults below):

| Share | Per month | Per day |
| --- | --- | --- |
| Endora itself (its own big-model decisions) | $30 | about $1.00 |
| The team pool | $70 | about $2.30 |

**The pool is shared only among the members with a paid brain** (ADR 0111), by their roles'
weights: CEO 35, lead 25, developer 40, reviewer 20, researcher 15. A member's day is its
weight over the paid members' weights together; a member on the house's model costs nothing,
reserves nothing and is never turned away for money. With the CEO and the lead the only paid
members, the CEO has $1.36 a day and the lead $0.97; with all five paid (weights 135), the
developer would have $0.69.

- Each run may spend at most **$0.40**, the CEO's **$1.00**. Unused daily budget does not carry
  over.
- **The ledger** is append-only: a reservation is made before a run starts, use is reported
  while it runs, and the charge and release are written when it ends. Crossing a limit
  during a run stops the run then, not afterwards. A charge that arrives after its run has
  ended (a call still running when it was stopped) is still written and settled, on the
  day the run was booked; no one report counts for more than a run may spend.
- When the credits are gone, the team stops. It never moves to paid billing. Raising any
  limit is the owner's decision.
- **A project's cap** ([ADR 0113](../adr/0113-projects-budgets-and-reports-to.md)) is a third
  limit, by the calendar month in UTC: a cloud turn on a capped project reserves no more than
  is left of it, and the caps together stay within the pool's month. At 80% the owner hears
  once a month; at 100% the project's Claude turns wait (local members go on) until the month
  turns or the owner raises the cap.

**Spending less a turn** (2026-10-17, [ADR 0109](../adr/0109-a-brain-per-member.md#cost-notes)).
A cloud turn resends its whole conversation every round, so most of what it cost was the same
prefix read again. Three things cut that without changing what the model is told:

- **Prompt caching.** Each of the worker's rounds on Claude asks for Anthropic's automatic
  cache (`cache_control: {type: "ephemeral"}` at the top of the call), which puts the cache's
  breakpoint on the prompt's last block: a round reads all the round before wrote (tools,
  instructions, briefing, every round so far) at a twentieth of the input price, and writes only
  what is new. The prefix is kept byte-stable: instructions built once, tools sorted by name,
  the briefing first, each round only appended. The relay passes the ephemeral cache (5 minutes
  or an hour) and nothing else of its kind; the reply's usage prices reads and writes apart, as
  before. Its estimate before a call reads what the turn's last call cached (same model, system,
  tools, thinking and effort; its messages carried on; within four minutes) at the cache-read
  price, and the rest of a caching call at the (dearer) write price. A prefix shorter than the
  model's minimum is not cached, and costs nothing more.
- **Effort by role and purpose.** HQ names the effort with each cloud turn
  (`/v1/runner/next` → `effort`, the runner's `ENDORA_EFFORT`, the worker's
  `output_config.effort`): the CEO `medium`, `high` when the turn weighs a decision (a question
  up the chain, the board's word on its decision, its decision sent back, or a decision draft in
  its task); the lead `medium`; the researcher `low`; the developer and the reviewer their
  model's own. The house's model is told nothing.
- **Lean briefings for the CEO.** The CEO's briefing opens with the house log, the team, what
  only the board decides and the goals' titles, in that order. Its first turn and its daily
  review add the whole picture (goals' notes and lessons, open tasks, the week's alarms and
  desk); any other turn adds only what changed since its last turn began (tasks made or moved,
  alarms, the board's word, lessons noted; the newest 20), or one line saying nothing did.

A turn's record keeps the input tokens the relay reported (fresh, read from the cache, written
to it), and the activity log's line for its end says "cached: N% of input".

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

The **CEO** (ADR 0111) takes most of these, with the owner as the board: members' questions go
up the chain to it, and only its decisions, with a recommendation, reach the desk (money
beyond the pool, new goals and priorities, hiring or letting go a member, what only the owner
can know). Six things never leave the owner's desk:

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
- Each member picks a look from the characters when hired; a team's name hangs over its pod.
  The floor is open-plan, one pod of desks per team with no walls, and what stands on it is the
  team's to change ([ADR 0119](../adr/0119-the-open-office.md)).
- A **simulated runner** (scripted members, no model, no cost) drives the engine's tests and a
  labelled demo mode, so the view is built and tested before real workers exist.
- **The owner's scenarios** ([docs/testing.md](../testing.md), from 2026-10-17): the decisions
  in the ADRs, and the ones a live incident forced, written in the owner's words as `.feature`
  files under `domains/team/features/` (and the bridge's under `domains/capabilities/features/`)
  and run against the real engine through its public use cases and the protocol's tools. They
  are the tests meant to survive a refactor and fail on a change of mind; the unit tests that
  pin how the code is written are to be pruned as the scenarios cover them.
- The art is from [Pixel Agents](https://github.com/pixel-agents-hq/pixel-agents) (MIT) and
  [MetroCity by JIK-A-4](https://jik-a-4.itch.io/metrocity-free-topdown-character-pack) (CC0).
  Its licence and credits travel with every copy of the files. Its pet sprites and its
  bundled font are not used.
- A working prototype, with a simulated engine following these rules, is in
  [endora-hq/prototype](endora-hq/prototype/index.html).

## 14. Storage

HQ's own SQLite database holds: teams, members, goals, tasks, task waits, thread posts,
wakes (as receipts), runs and their steps, the budget ledger,
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
- **Recoveries**: when something that alarmed within the day is running and healthy again (a
  service that answers), once an alarm, the node tells HQ (`POST /v1/alarms/recovered`,
  `{ "text": "deluge is back up at 13:43" }`, signed), and HQ posts "Recovered: …" in the
  routine's open task as Endora's, waking whoever has it (`alarm`). No open task, no news: it
  hands out no work. The worker's rules: an alarm is a moment; check `homelab_status` first,
  and close the task if everything has recovered.
- **One task a problem**: a member's `create_task` is refused, naming the task to add to, when
  an open task under the same goal is already about the same containers (section 6).
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
- `homelab_map` (read, Internal, [ADR 0112](../adr/0112-the-team-knows-how-the-lab-is-built.md)):
  how the lab is built, from Docker through the observer's list and its one inspection route
  (`/containers/<name>/json`): each container's image, state, restart policy, who manages it
  (`ansible-nas role <name>`, `Endora`, `unknown`), what it depends on and what depends on it,
  and the owner's note from `ENDORA_LAB_RETIRED`; wiring lines such as "gluetun → deluge,
  nzbget, prowlarr (run in its network)". No environment, mounts, binds or other labels. Kept
  a minute.
- `ansible_nas_doc` (read, Internal, ADR 0112): `{ "path" }`, a doc (`docs/**/*.md`,
  `website/docs/**/*.md`, `roles/<name>/README.md`) or a folder to list (`roles`, `docs`,
  `website/docs` and folders in them); anything else, `..` included, refused. From the fork in
  `ENDORA_ANSIBLE_NAS_REPO` at `ENDORA_ANSIBLE_NAS_REF` on GitHub, kept a day; unset, "not
  configured". HQ seeds the house log once, when it is empty, with how the lab is built, and
  the worker's instructions carry the same paragraph.
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
  memory, Endora, the team or money); every act of the tool server paired with Home Assistant
  (the owner's devices are not the team's to switch: live, the developer asked to run
  `HassTurnOff` "to restart" a container); and what is not set up.
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

### The CEO (owner's decisions of 2026-10-17, [ADR 0111](../adr/0111-the-ceo-and-the-chain-of-command.md))

- **The role**: `ceo`, ranked above the lead, which leads every goal as the lead does (it may
  act on any task); work is handed only down, so nobody hands the CEO work. Its tools: `my_turn`,
  `claim`, `post`, `set_status`, `create_task`, `check_back`, `ask_owner` (a decision),
  `note_house`, `rewrite_house` and `read_doc`; not the gate or `request_action`.
- **Seated once**: HQ started with the demo off and no CEO seats "Mara" (share 35, Claude Opus,
  `member.hired` in the feed), remembered in HQ's settings so it is never seated again; and,
  once there is an active goal (the home lab's if any), the routine "Daily review" for the CEO
  every 24 hours, once. The home lab's routine is still made when the only routine is the CEO's.
- **Its briefing** starts with the house log, its team (role and brain each), the goals with
  their notes and lessons, the open tasks (status, owner), the home lab's alarms of the last 7
  days (from HQ's own `routine.fired` and `routine.posted` events), what reached the board's
  desk in the last 7 days and how the board decided, and the board's items. The lead's
  briefing starts with the house log, read-only, and every member's says whom its questions go
  to.
- **On the node**: the bridge's profile for `ceo` is read only and `may_see` gives it nothing
  Confidential on a cloud model; the contracts offer it `homelab_status` and no act. The relay
  lets `ceo` call Opus or Sonnet, Opus no other role, and holds each call to the role's turn
  ($1.00 for the CEO, $0.40 otherwise) whatever HQ says is left.
- **On the screen**: the CEO in the Team card with its Brain choice (Local, Claude Opus, Claude
  Sonnet), a Rename button for every member (`POST /v1/hq/members/{id}/name` → HQ's
  `POST /v1/members/{id}/name`), the House log card (`POST /v1/hq/house` → HQ's
  `POST /v1/house`), and decision cards with the decision, the recommendation and why, Approve,
  Decline and an answer box (`answer` with words alone). A local member's card says it costs
  nothing.

### Projects, budgets and reports_to (owner's decisions of 2026-10-17, [ADR 0113](../adr/0113-projects-budgets-and-reports-to.md))

- **The store**: a `projects` table (goal, title, reason, lead, status, pause, cap, asked cap,
  who made it, proposer, the task it was proposed in, evidence, when made and changed), and an
  empty `project` column on `tasks`, `routines`, `desk` and `ledger`, and `reports_to` on
  `members`, each added to the live store by an `ALTER TABLE` guarded by `pragma_table_info`.
  The ledger's reservation takes its project from the turn's task, and a turn's later rows from
  its reservation; old rows stay empty, and the append-only triggers stay.
- **On start** (demo off): whom each member reports to, filled in for members without one; and
  once, with no project yet and a routine taking the home lab's alarms, the active project
  "Home lab upkeep" under its goal (lead: the earliest lead at work, or the CEO; no cap), with
  that goal's routines (but the CEO's) and open tasks in it. Then the CEO's weekly "Planning"
  routine, once, under the same goal as its daily review.
- **The engine**: `affordable()` takes the least of the member's day, the pool's day, the
  role's turn cap and the project's month (`cap − committed`, nothing within five cents of it,
  nothing while paused for its budget). `report_usage` watches the project's month after each
  charge: at 80% one notice a month on the desk and a `project.budget` event; at the cap the
  project pauses for its budget and the desk offers `raise` (an amount) or `wait`. The tick
  lifts a budget pause once the month turns. An owner's pause refuses every wake on the
  project's work (`project_paused`) and starts none of its queued turns. A task handed back
  wakes its project's lead. `Engine::above` walks `reports_to`, past paused managers.
- **The team protocol**: `propose_project` for the lead and the CEO, `decide_project` and
  `propose_goal` for the CEO. Evidence is task numbers and event ids, each looked up.
- **The briefings**: the lead's lists its projects with this month's money; the CEO's says
  which projects are at 80% or past it, and its daily review how many proposals wait; its
  planning shows each goal's projects, the last 30 days' tasks by the containers they named,
  the alarms that came back with their event ids, the proposals waiting and the goals with no
  project, and nothing else.
- **HQ's routes** (signed by the node, which records each in its audit log): `POST
  /v1/projects` (`goal`, `title`, `reason`, `lead`, `cap` in dollars), `POST /v1/projects/{id}`
  (`action`: pause, resume, drop, reopen, done), `POST /v1/projects/{id}/cap` (`cap`, or none),
  `POST /v1/members/{id}/reports_to` (`to`); `POST /v1/tasks` and `POST /v1/routines` take an
  optional `project`. The desk's `project_cap` takes `approve` (with an amount, or as asked) or
  `decline`, and `project_budget` takes `raise` (with the amount) or `wait`. The snapshot carries
  `projects` (with this month's `spent`, `committed` and `level`), and `project` on tasks,
  routines and desk items, and `reports_to` on members.
- **On the screen**: the Projects card by goal with its bar, its forms and buttons; cap and
  budget items on the desk with an amount box; "Reports to" on each member's card, and "Reports
  to the board: you." on the CEO's.

### Evidence, acceptance criteria and the tester (owner's decisions of 2026-10-17, [ADR 0114](../adr/0114-evidence-acceptance-criteria-and-the-tester.md))

- **Receipts**: the node tells HQ every call a turn makes through its contracts and bridge
  (`POST /v1/team/receipt`, node-signed, naming the turn by its token; HQ keeps it only while the
  turn runs), and the model reads "(receipt r41)" after the result. A `call_receipts` table, never
  changed; Confidential excerpts are never shown to a member on a cloud model.
- **Criteria** on tasks (`tasks.criteria`, JSON), required for work a member hands another,
  optional for the owner's (`criteria` lines on `POST /v1/tasks`, `live:` marking one), and the
  home lab's two live criteria on its routine's and alarm's tasks.
- **One evidence rule** (`domain::evidence::judge`) for submission, a check closed, the tester's
  pass (with a reading of her own), the lead's acceptance (`accept`), the CEO's `decide_project`
  and decisions for the board, a tester's passing on the CEO's decision, and the lead's question
  up to the CEO. Evidence cited per criterion is kept (`cited`).
- **The path**: member → tester (`review`) → lead (`accept`, woken by `acceptance_asked`) → done;
  nobody checks its own work, and the fallbacks are said on the task; three send-backs and the
  next round is the owner's (`rounds` on the desk: accept or send back).
- **Turn labels** on runs (`runs.posted`, `runs.blocked`, `runs.label`), in `run.ended` and on
  screen; the back-off reads them. Two nudges, then a task that shows nothing goes to whom its
  member reports to, once; the same reading again shows nothing; a check-back on a task blocked on
  a wait is replaced by the wait, which is not stranded either.
- **The chain of command** may move, close, accept, send back or hand on (`hand_to`) a
  subordinate's task, saying for whom. **Unsupported** posts are marked (`posts.unsupported`).
- **The CEO's effort**: `low`, `medium` for its reviews, `high` for decisions.
- **The store**: new columns by guarded `ALTER TABLE`, new tables with the schema, tested from the
  schema as it was. **The screen**: criteria with their evidence, the tester's pass, run lines'
  labels, "unsupported", "Tester", and the owner's rounds.

### How the team is doing (the scorecard, as built 2026-10-17)

A card on the HQ screen, "How the team is doing", for today beside the last seven days (the
owner's day boundaries, as the screen's money is counted, by `Settings::offset_minutes`), with
the numbers that say whether the work is good, every one computed by code
(`application::scorecard`, pure over the store's rows, served in the snapshot as `scorecard`;
the screen only renders): turns, and the share that showed nothing (label `talk only` or
`empty`), per member with each member's spend; work sent back (left review for `doing`,
whoever sent it) against the submissions; the cost of a finished task (everything ever spent on
turns on the tasks that reached done in the window, over their count) and what dropped tasks
spent, which is waste; alarms, the tasks they made, the median time from an alarm to its task
closed and how many stay open; what reached the desk, split approvals, questions, stuck and
notices, with what still waits; and the prompt cache's share of the paid turns' input (HQ keeps
tokens, not the relay's prices, so the share is shown and not the dollars). A turn from before
labels is **unlabelled**, counted and shown as such, never guessed. The node prunes the feed to
its newest 20,000 events, so each window says where its record starts when that is later than
the window; nothing older is counted. Scenarios: `features/how-the-team-is-doing.feature`.

### The engineering wing and the git flow (owner's decisions of 2026-10-17, [ADR 0115](../adr/0115-the-engineering-wing-and-the-git-flow.md))

Built, as the record says. In short: a second team, the **wing** (Kit the lead on Sonnet, Rae
and Zed the developers on Haiku, under the owner's goal "Improve the lab" and its project
"ansible-nas clean-up"; Dee tests for both teams), works on the owner's public ansible-nas fork.
The node keeps a **mirror** of the fork and hands a sandbox a **bundle** of the fork's base
branch (`ENDORA_ANSIBLE_NAS_REF`, the fork's `develop`; else its default branch, asked of GitHub
once); the worker unpacks it into `/work/repo` with git set up (Endora HQ as the author, the
trailer by a hook, no remote). A member commits on `hq/task-N-<slug>` and **submits** the
branch as a bundle; the node takes it only on top of the base, within 200 files and 1 MB, every
commit HQ's, nothing of `group_vars`, `host_vars` or an inventory touched, and keeps it under
the task. HQ writes the diff as a **document** on the task and queues a **check job**, the lab's
first piece: the Pi's runner runs the workbench image model-free with a job token, which fetches
the branch from the node and runs the fork's own checks (syntax against the integration
inventory, `ansible-lint --offline`, `yamllint`), five minutes each, reporting a **receipt**
that is the task's gate; a failing check sends the work back. The tester reads the diff and the
report, the lead accepts, and **the owner's desk offers the push**: Approve has the node push
the stored branch to the fork over SSH with a deploy key (`hq/*` only, never force, never main);
without a key, the bundle is offered to download. **HQ opens no pull request and merges nothing**
during the audit period; the owner merges on GitHub. Hiring acts: the owner's form (one CEO),
and a lead's request within the wing's seats (one Sonnet lead, five Haiku developers). The
office gains the wing's room with six desks. Deferred: the full lab (ring 2), Endora's own
repository, and any push or merge by HQ.

Section 7 above (pull requests on Forgejo and GitHub, opened by the gateway) is superseded for
now by this flow: branches, not pull requests, and only the owner's push.

### Elastic staffing and notebooks (owner's decisions of 2026-10-17, [ADR 0118](../adr/0118-elastic-staffing-and-notebooks.md))

As built: the team grows and shrinks with its work, and nobody is fired.

- **Furlough** is a member's state (two nullable `members` columns), not the owner's pause: a
  member on furlough is never woken (`wake.refused: furloughed`), holds no task, is left out of
  every briefing's team and of the share arithmetic, and keeps its record, name and notebook.
  The owner furloughs or recalls any member but the CEO from the roster panel
  (`POST /v1/members/{id}/furlough` and `/recall`, through the node); a member with open work
  hands it on first. No route or method deletes a member.
- **The idle look** runs on the first tick of each owner day: a member that is not the CEO or a
  lead and has had no task, wake or turn for seven days (and no routine of its own on) is offered
  on the owner's desk, "furlough, or keep?"; kept, it is not offered again for a week.
- **`request_staff`** (a lead's tool, for its own project): the project's open tasks as evidence,
  at least twice its active developers. A developer on furlough is offered back (`recall` on the
  desk); with none it becomes the hire request of 0115 under the same seats. Approving reports
  the member to the lead.
- **The scorecard** shows each lead's team: seats, at work, on furlough.
- **Notebooks**: a `notes` table; every role has `note` (on evidence from HQ's record),
  `forget` (with a reason), `read_notebook` and `notebook_checked`. The briefing opens with
  "Your notes (N):" newest first, at most twelve and about 1,200 characters, stale ones marked
  after 30 days unused, and asks for a tidy a week after the last one (the CEO's briefing carries
  only the count). A furloughed member's first briefing after a recall says to read its notes
  first. The owner reads, adds, edits and deletes any member's notes from its panel (`GET` and
  `POST /v1/hq/members/{id}/notebook`, `POST` and `DELETE` on a note).
- Scenarios: `domains/team/features/staffing-and-notebooks.feature`.

### The evening's fixes (owner's decisions of 2026-10-17, as built)

From that day's record, each recorded in its ADR:

- **Money** ([ADR 0113](../adr/0113-projects-budgets-and-reports-to.md), amended): a member's
  share is a first claim, not a wall (past it, it draws on what the other paid members left
  today; the pool's day is the ceiling); a turn the owner asked for is charged to the pool's day
  only (`asked_by_owner` on the run, an `owner` flag on its ledger rows); and a member's claim
  is its role's weight times its brain's price class. Borrowing is capped at one extra share
  (2026-10-18): a paid member may spend while `spent < share + min(share, the others' unused)`.
  At that limit, with the pool's day not spent, it is "out for today": one `out_for_day` item a
  day on the owner's desk, which gives it an allowance (at most the pool's day left, $0.50 as
  offered) until the owner's next day starts, or waits for tomorrow.
- **The chain** ([ADR 0111](../adr/0111-the-ceo-and-the-chain-of-command.md), amended): a
  question on a project's task goes to the project's lead.
- **The tester** ([ADR 0114](../adr/0114-evidence-acceptance-criteria-and-the-tester.md),
  amended): she passes or sends back and never makes work (no `create_task`); a post in a turn
  with any receipt, and the worker's last word, are never unsupported.
- **The screen**: the Team card is an org chart (the board and the CEO, then one group per
  lead; one line per member with its brain, what it is on and its money against its share, its
  controls behind a tap); a task's turns read oldest first like its thread, the last five with a
  toggle for all.
- **The smoke**: `make smoke` waits up to 30 seconds for the node's `/health` before it checks.

### Replay (as built 2026-10-17)

The owner reads any turn back in full, and steps through a task's turns in order: tap a run
line on the HQ screen ("R12 Kit · EL-7 · advanced") or "Replay this task" on a task card. The
screen (phone first) shows the turn's header (member and role with their office character,
brain as set now, why it was woken, its label and status, how long it ran, money from the
ledger: spent, held, released; tokens in, from the cache, out; why it ended) and then what
happened, oldest first, each with its time and a tag: woken, and any wake folded into it; what
it was told (its latest attempt's briefing, folded behind "What Kit was told"); each call with
its receipt id, tool, arguments, ok or refused, read or act, and the excerpt (Confidential ones
included, marked: this is the owner's screen; long ones folded); each post with what it cited
and the unsupported mark; the evidence it cited; its claims, status changes, desk items, the
wakes it raised; each start and end. A task shows "run 3 of 6" with previous and next.
Everything a model or a tool wrote is escaped.

It is a read model (`application::replay`, pure over the store's rows) behind
`GET /v1/runs/{id}/replay` and `GET /v1/tasks/{id}/replay` on HQ (signed by the node), proxied
as `GET /v1/hq/runs/{id}/replay` and `/v1/hq/tasks/{id}/replay`; the node lets only a full
credential read them (a checking credential, which may read the rest of HQ, may not). How a
turn's moments are found: by run id wherever the record keeps one (the wakes, receipts,
evidence cited, ledger rows, and the events that already named a run: starts, ends, receipts,
spend). Since this was built, **every event a turn's call emits names its turn** (`"run"` in
what it carries; the engine knows which turn's call it is taking), and HQ keeps when that began
(the setting `events_name_their_turn_from`); a turn queued since takes its events by run id
only, and a post is the one its turn's `thread.posted` event names. A turn from before is
matched by time: the events of the kinds a turn causes (claims, releases, status changes, tasks
made, desk items, check-backs, lessons, gate results, branches, projects, house notes) about
its member, or naming no member about its task, between when it was queued and when it ended,
and its member's posts in its task's thread then; each moment matched so says "matched by time".
Not shown: what the model thought between calls (HQ never sees it), calls made in the
workbench's own shell (they never pass the node), and anything the feed pruned (the newest
20,000 events are kept; a turn's end is then said from its row).

### Claude Code as a developer runtime (as built 2026-10-18, [ADR 0117](../adr/0117-claude-code-as-a-developer-runtime.md))

The owner's decision of 2026-10-17: Claude Code, headless, as the runtime of one wing developer,
with an API key through Endora's relay (the turn's token as Bearer; never a subscription login),
measured against Endora's worker. Built:

- **The setting**: `runtime` per member, `worker` or `claude-code` (a developer on Claude only),
  the owner's route, the snapshot's `runtime` and `claude_code` per member, handed to the runner
  and the relay. The pilot seeded once: Rae on Claude Code, Zed on the worker.
- **The turn**: the worker does HQ's bookkeeping and the checkout, then runs `claude -p` in
  `/work/repo` with an explicit tool list and one MCP server, the worker itself as a stdio bridge
  to HQ's tools and the node's contracts; its JSON result is the last word.
- **Receipts**: a `PostToolUse` hook posts Claude Code's own tools' calls to the node's
  `/team-relay/receipt`; HQ keeps them as sandbox-reported, which no live criterion rests on.
- **The relay**: a Claude Code turn's request shape and betas pass, held to the brain, the money,
  the five-minute cache and the fence.
- **The image**: Claude Code 2.1.296, digest-pinned, with read-only managed settings that turn
  off its updates, telemetry and other traffic. A whole turn ran offline in the arm64 image
  against fakes, with no startup traffic beyond them.
- **The measure**: the scorecard's cost of a finished task per member, Rae's against Zed's.

Deferred: per-task `--resume`, the screen's control, Claude Code for other roles.

### The open office (as built 2026-10-18, [ADR 0119](../adr/0119-the-open-office.md))

The owner's decision: "the teams should look, work, interact as one", and "let them do something"
in their downtime. Built:

- **The floor** (32 by 20 tiles): the board room (0,2,7×5), the task board on the back wall,
  Mara's office (20,2,7×5, its two-tile door open on the bottom wall) and the server closet
  (27,2,5×5) along the top; the open floor under them with a lane on row 7; **one pod of desks
  per team** in the band rows 8 to 13, x 1 to 27, side by side with no walls and spread across
  it when they fit in one row (a desk is 3×2 tiles, two to a row with a 2-tile aisle, the pod 8
  wide, 13 for a team over six; a pod that does not fit starts a second row); a lane on row 14;
  the ping-pong corner (0 to 8, rows 15 to 19), the meeting room (9,15,7×5, its door open on the
  top wall) and the kitchen and lounge (16 to 31, rows 15 to 19); the front door on the right
  wall at rows 10 to 13. The pods are laid out in the console from the snapshot
  (`hqLayout`): the roster's teams and labels, a desk for every member, an empty desk with an
  "away" card for a member on furlough.
- **Movement**: a search over the floor's cells (`hqPathCells`) round desks and furniture and
  through the two open doors; a working member at its pod desk, or at the desk of the project
  lead whose team's work it is on (Mara's door for hers), or of whoever's task it is testing;
  idle members drift at random between ping-pong, the kitchen, the tap, the couches, the coffee
  machine and what else stands on the floor; none with the device's reduced-motion setting.
- **Data**: `office_items`, `office_proposals`, `office_votes` and `office_history` (all made if
  missing), the placement rules and verdict in `domain/office.rs`, the engine in
  `engine/office.rs`; the snapshot's `office` (items, catalog, open proposals, the last few
  settled, `can_undo`, the allowance and today's use). Events: `office.proposed`,
  `office.voted`, `office.changed`, `office.dropped`, `office.vetoed`, in the feed and in
  Replay.
- **The tool**: `office` for every role: `propose`, `react`, `look`. Proposals carry on two yes
  (the proposer's counted) or six quiet hours, are dropped on more no or at 48 hours, and are
  checked again when they carry.
- **The owner**: the Office card (open proposals with votes and a Veto on each, Undo, Reset, the
  allowance); `POST /v1/office/undo`, `/reset`, `/proposals/{id}/veto`, `/downtime`, through the
  node's `/v1/hq/office/...`, each in the audit log.
- **Downtime**: the wake reason `downtime`; the scheduler wakes at most one member a tick, one
  at a time, once a member a day, 12 hours apart, only for a member with nothing to do and, by
  default, only on the house's model; Claude members only within `downtime_paid_cents_per_day`,
  pool-charged. The engine setting `downtime` is on in the service and off in the demo.
- Scenarios: `domains/team/features/the-office.feature`.

Not built: pictures for the kinds that have only a pixel shape; the owner placing or pinning an
item; events beyond decorating.

### Games in the office (as built 2026-10-18, [ADR 0121](../adr/0121-games-in-the-office.md))

The owner: "make the ping pong games actually keep track of points ... a pool table with an
actual game. Or chess with actual games." Built:

- **Ping-pong and pool, for nothing**: `domain/games.rs` (a splitmix64 generator, hidden skill
  per member and game from its number plus a point per ten games to ten, first to 11 win by 2,
  simplified eight-ball, the leaderboards, the schedule) and `engine/games.rs` (`games_tick`:
  at most one match per game every 20 minutes, between two free members who have not played
  in 10 minutes; no wake, run or model). The `games` table; event `game.played` ("Bo beat Dee
  11–8 at ping-pong"); the engine setting `games` (on in the service, off in the demo).
- **Chess**: `domain/chess.rs` over `cozy-chess` (MIT; `shakmaty` is GPL and was refused),
  `chess_games` (moves as notation, status and reason), the team tool `chess` (`board`, `move`,
  `challenge`; `application/chess_tool.rs`), moves and challenges only in downtime turns, one
  game each, a downtime briefing line ("Your move against Dee in chess"), adjournment as a draw
  after seven days. Events `chess.started`, `chess.moved`, `chess.ended`.
- **The floor**: a pool table (29,15, 3×2, its players at 29,17 and 31,17) and a chess table
  (23,17, its players at 23,16 and 23,18) reserved in `office::BLOCKED` and in `hqWorld`, with a
  test on each side; the ping-pong scoreboard, the pool scoreboard and the live chess position
  drawn on the canvas; the two players of a match walk to its table.
- **The screen**: the snapshot's `games` (leaderboards with `last`, `recent` five, the chess
  games being played and the last three finished); the Games section of the Office card; tapping
  a table opens its panel (the chess board with unicode pieces, the moves and the result, or the
  leaderboard).
- Scenarios: "Ping-pong keeps score, and costs nothing", "Chess is played by the rules" and
  "Chess ends when the rules say so" in `domains/team/features/the-office.feature`.

Not built: chess clocks, offering a draw, resigning; fouls in pool beyond the scratch on the
eight; a tournament.

### Logs and the retry (as built 2026-10-18)

- The feed tells a thread post whole, read from the snapshot's task thread by the place the
  `thread.posted` event names (`post`, with `cut` and a preview ending in "…"); lines over 240
  characters fold behind more/less; the log is a plain list on a phone, thirty lines at first,
  "Show older" then `GET /v1/events?before=&limit=` (at most 200). The Activity screen pages
  by `offset`.
- Every server cap that loses text ends in "…[cut]" (`domain::cut`); the limits are unchanged.
- "Try again" on a stuck item (`retry`) first asks whether its member can be woken; if not, it is
  refused with the reason, nothing changes, and the item stays on the desk. The card says so.

### The team's standards ([ADR 0116](../adr/0116-the-teams-standards.md), proposed 2026-10-17)

- The owner's global standards are [docs/standards.md](../standards.md): every member's
  briefing names them once ("The team's standards: read_doc docs/standards.md"; the CEO's
  `read_doc` description, since its quiet briefing is held short), and every member reads them
  with `read_doc`. The wing is told the fork's own `AGENTS.md` is the repo's rules; its lead,
  that the draft [docs/drafts/ansible-nas-AGENTS.md](../drafts/ansible-nas-AGENTS.md) is the
  first branch to propose.

### Roles as an enforced RACI (owner's decisions of 2026-10-18, [ADR 0120](../adr/0120-roles-as-an-enforced-raci.md))

As built: a product-model org, held by code. The CEO sets direction and owns the outcome; each
lead runs its area (itself, whoever reports to it, and the projects it leads, all from the
record) and decides there; leads propose and the CEO decides.

- **The CEO**: reads anything (on a cloud brain what the cloud lead reads), acts on nothing in
  the lab, is never woken by its alarms or recoveries (they wake the area's lead), and on a task
  in a lead's area never claims, moves, closes, accepts or sends back: `direct {task, direction,
  evidence}` posts "Direction for Ada: …" and wakes the lead (`directed`), who decides. It
  creates and hands work to leads only, proposes no project, and accepts only its own work.
- **Strategy per goal**: purpose, success, measures, bets, not now; `set_strategy` (the CEO, on
  evidence) and the goal card (the owner). Seeded empty; the daily review asks for it.
- **Measures per area** on the scorecard: outages over 10 minutes, median alarm to closed and
  repeats within 24 h where an area takes the lab's alarms; branch submitted to push approved
  and first-time tester passes where it works in branches.
- **Leads report up**: "State of my area" daily at the owner's 18:00 (`report_area`: moved,
  stuck, need, proposals; not work on the board or the scorecard) and "Patterns" weekly (the
  area's repeats; `propose_project`).
- **The CEO's briefings**: the review is strategies, the leads' reports, the measures, the
  proposals and the board's decisions; the planning is strategy, the week's measures, the
  proposals and the budget split.
- **The night's bugs**: time-bound criteria (`after`; the lab's "no new alarm since" waits 60
  minutes; "wait until HH:MM"), repeated readings (a reading the turn before took shows nothing;
  no recovery wake while a check-back waits; a stale wake waits two minutes), and the house
  log's dated lines with the year.
- **The ladder**: a project's lead closes its own work on its own acceptance; stuck work goes to
  the area's lead (`decide_stuck`), then a huddle (the lead and up to four who touched it in
  48 h, one `huddle_view` each, then the lead decides; written as a document), then the CEO only
  when the lead says so or did not decide; the owner's desk only as a board matter or with no
  rung left. Work sent back three times goes to a huddle. Events `stuck.climbed`,
  `huddle.called`, `huddle.decided`.
- Scenarios: `domains/team/features/roles.feature`.

### The team's world: stages, stories and situations (as built 2026-10-18, [ADR 0122](../adr/0122-the-teams-world-stages-stories-and-situations.md))

As built: the first of three builds of the team's memory; the map and asking the past follow.

- **Pages**: the house page (`world:house`, the CEO's to write) with current-truth sections and a
  dated, append-only timeline; the derived `world:lab` and `world:team`, written by HQ from code
  and the record each time they are read. A section's address never changes; every revision is
  kept.
- **Stages**: observation, field note, insight, canon, superseded, retired. An insight needs a
  warrant and evidence from two turns or a reading the node witnessed this turn. **Only the owner
  makes something canon** (World card, or a `propose_canon` card on the desk), and demotes it with
  a reason that the feed and the timeline say. Trust is read when read: an insight or canon past
  its horizon (house 90 days, an area's stories 30) or whose evidence has left the record reads
  "unconfirmed since <date>" and counts as an observation; `verify_world` confirms it again.
- **Stories**: lessons in four parts under a handle, by role's scope, told whole in the
  situations they are taught in, cited by handle. `tell_story` replaces `note_lesson`.
- **Situations and budgets**: recalled, handover, fresh, after a gap, continuing (and resumed,
  named for later), decided from the record; the world's pieces composed within 2,500 (CEO),
  4,000 (lead), 3,000 (others) or 1,800 (the house's model) tokens, 1,200 more after a gap, on a
  handover or a recall; what is left out is named on the last line; the run records it, and the
  replay shows it.
- **Handover notes**: `hand_to` and a stuck task's hand-on or drop need `{done, not_done, next,
  watch_out}`; the next holder's first turn starts with it.
- **The move, once**: the house log's lines into the timeline at their dates (the seeded lab
  lines are the lab page), lessons into stories to retell, notebooks read as field notes (the
  owner's as insights); nothing deleted, the old log read-only. Mark `world_migrated`.
- **The owner**: the World card, `GET /v1/hq/world`, `GET /v1/hq/world/export.md`,
  `POST /v1/hq/world/promote`, `POST /v1/hq/world/demote`, through the node and its audit log.
- Scenarios: `domains/team/features/the-world.feature`.

## 16. Later, and not at all

- **Later:** skills and adapter plugins, including adapters to other systems; more teams;
  hiring that acts (a proposal to hire carried out once the board says yes); commits and
  command output from the worker's own shell as receipts (Claude Code's are reported, ADR 0117); moving a task between projects; the pool and the shares as settings on
  screen; virtual-machine test environments; the office on the handheld.
- **Not at all:** members holding any credential; anything merged or deployed without the
  owner; paid billing beyond the credits without the owner; the team reading the brain,
  mail or chat.
