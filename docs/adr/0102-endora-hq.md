# 0102 — Endora HQ

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions Q1–Q34). Prepared
with AI assistance (Claude). Step 7 of [the always-on butler plan](../plans/always-on-butler.md);
the specification is [Endora HQ: the design](../plans/endora-hq.md). Phases 1 and 2 are built:
the team core in `domains/team`, and the HQ service (`app/hq`) with the office in the console,
on the simulated runner. Phase 4 has begun: the team protocol is served over MCP at
`/v1/team/mcp`, opened only by a turn's own token, which HQ issues when the turn starts and
which opens nothing once it ends. A turn reaches a model only through the node's **model
relay** (`/team-relay/v1/messages`): it presents its token as its key, the relay asks HQ what
the turn may still spend, caps the reply to that, adds the owner's Anthropic key (which never
leaves the node), and charges HQ what the call cost, priced from Anthropic's table; a model
not priced there is not relayed, and a turn past its budget is stopped. The owner chose to
share their default Anthropic project, whose cap is the outer limit, with Endora enforcing the
team's pool itself. The runner and the worker are built too: Endora's own Rust worker
(`app/worker`) takes each turn in a fresh container the runner (`app/runner`) starts on the NAS
through Docker's socket, on a fenced internal network that reaches only HQ and the node's team
listener; HQ hands each turn out once, under a lease. Runner pairing for the Pi is deferred.

Amended 2026-10-17 by [0106](0106-team-contracts.md): a turn reaches the home lab only through
**contracts** the node serves on its team listener beside the relays (`/team-relay/contracts`),
opened by the same token: reads are free, a safe fix (a container restart) runs on its own
within limits and lands on the owner's desk as a notice, and nothing else is offered. HQ started
with the demo off puts the demo's goals and their open tasks away (dropped, with a notice), so
none of it becomes a real turn.

Amended 2026-10-17 by [0107](0107-routines-alarms-and-check-backs.md): goals are passive and
work arrives as tasks. Routines hand a member a task on a rhythm, the node's alarms about the
home lab join a routine's open task, a member can check back on its task, a clock with nothing
to do starts no turn, and a task whose turns move nothing backs off.

Amended 2026-10-17 by [0109](0109-a-brain-per-member.md): each member has a brain the owner
sets on the HQ screen, local by default or a Claude its role may use; the runner hands the
worker the matching relay, and the worker speaks Anthropic's Messages API to Claude. The lead
and the reviewer keep the team's lessons in a goal's notes (`note_lesson`).

Amended 2026-10-17 by [0110](0110-the-teams-rings.md): the Pi is a runner. It reaches HQ, still
unpublished, through the node's team gateway on 8788 (now published on the house's network,
every route needing a token), and starts each turn as an offline workbench that reaches the
gateway and nothing else.

Amended 2026-11-04 (the agentic-harness climb, round 5): **the owner's new work asks for a pass
of the tick at once.** Work was taken up only on the tick, every 30 s on the NAS, so the owner's
new item waited for anything up to the whole interval before any member read it: 8.7 s in the
climb's live paired run, and exactly 30 s less the tick's phase on a disposable HQ (27.96 s,
20.97 s, 13.98 s and 6.97 s at 2, 9, 16 and 23 s after a tick). Now, once the owner's task is in
the store, the engine asks its service for a pass (`Engine::with_pass_requests`), and HQ's
ticker (`app/hq/src/ticker.rs`) runs one: the same `tick()`, with every guard it keeps (the kill
switch, the drain, budgets, capacity, order). Asks are coalesced (asks before a pass starts are
served by it; asks during one bring one more after it), passes never overlap, the clock keeps
its cadence, and an idle HQ passes no more often than before. Only the owner's stored work
asks: a member's own work, and work refused, do not. Scenarios:
`domains/team/features/the-owners-new-work-starts-at-once.feature`.

Amended 2026-11-04 (the agentic-harness climb, round 7): **a turn's record keeps which model
answered, as the provider named it.** HQ kept only the alias a turn asked for (its brain), so a
comparison could not show which model actually answered. The node's relay now reads the model a
reply names (a whole reply's `model`, a stream's `message_start`) and sends it with that call's
charge, as `served_model` on `/v1/team/usage`, or null when the reply named none; the field is
optional, so an older node is still understood, as naming none. HQ counts each call under the
name it was told, beside the turn (`run_served_models`, `Engine::note_served_model`), every name
kept and none overwritten; a call whose answer named no model is counted as not known, and the
alias never stands in for it. Charges, budgets, routing and what is sent are unchanged. The
house's free relay (`/v1/team/tokens`) keeps no name yet. Scenarios:
`domains/team/features/a-turns-model-is-kept-as-the-provider-named-it.feature`.

Amended 2026-11-04 (the agentic-harness climb, round 9): **the node keeps its own count of the
model requests its relay passes on.** HQ's only record of a call was the usage report the node
sends it, so a lost report hid that the call happened, and the comparison could not prove every
request was accounted for. The node's store now has `relay_requests` (`SqliteStore`, behind the
`RelayCensus` port): one row per request passed to the provider, with the run, when it began and
ended, the model asked for, the model the provider named (or none), how it ended (`unfinished`,
`answered`, `interrupted`, `failed`), the HTTP status when known, whether its usage was fully
reported, and whether HQ accepted the usage post (`hq_told`, empty until the charge is tried).
A row begun and never ended stays `unfinished`, never complete. The Anthropic relay counts a
call just before it leaves for the provider, so a call refused earlier (not authorized, over
budget, no key) is not counted, and `count_tokens` is not a model request; the house's and
DeepSeek's chat path is counted the same way, as answered or failed (an unreadable 2xx body as
interrupted). Kept: ids, times, model names, outcome, status, flags. Not kept: any prompt, reply,
body, key, tool argument or token count of the turn. Charges, budgets, routing and what is sent
are unchanged. The count is kept 90 days (let go of at each start of the node) and indexed by
run. Scenarios: `app/node/features/the-relay-keeps-its-own-count-of-model-requests.feature`.

Amended 2026-11-04 (the agentic-harness climb, round 17): **a turn that can go on again is
taken up at once.** HQ settled a turn the runner said had ended, and started a turn the owner's
reply had brought forward, only on its tick. In the climb's interrupted run the owner's reply
brought the stopped turn forward at once, and it then sat 8.6 s for the next tick (every 10 s on
the NAS) before it ran. Now the runner's accepted report of an end (`/v1/runner/{run}/ended`;
`Engine::a_turn_has_ended`) and the owner's reply that wakes someone (a turn queued, or folded,
which brings a waiting one forward) ask for a pass, through the same coalesced asks as round 5:
the ordinary `tick()`, with every guard it keeps. A report under another attempt's lease, and a
reply refused, ask for none. How long a failed turn waits before it is tried again (a minute),
and how many tries it gets, are unchanged; a pass never starts a waiting turn before its time.
Scenarios: `domains/team/features/a-turn-that-can-go-on-starts-at-once.feature`.

Amended 2026-11-04 (the agentic-harness climb, round 18): **a probe's side effects can be
recorded outside the agent's reach, by an observer.** The `tool_failure` benchmark probe asks
whether a tool that failed was tried again, and the agent's own word is no evidence of what its
tool did. The runner can now start a small observer container for an item, before the item's
first turn, and start the turn's workbench inside the observer's network namespace, so the
agent's tool reaches the observer on `127.0.0.1:8799` while the observer's records stay in a
container the agent has no mount, process or IPC namespace, token or Docker access to: the
inverse of a sidecar the agent could reach into. It is off by default and the runner's own:
`OBSERVER_PROJECTS` lists the project numbers that get one (empty or unset, none; anything but
numbers stops the runner), and HQ only says which project a turn is in (`project` in
`/v1/runner/next`), so HQ can never turn it on. The image is the runner's own workbench image;
the command (the probe's publish service, compiled into the runner), the limits (64 MiB, 32
processes, half a CPU), the fence (read-only, no capabilities, no new privileges, the workbench's
unanswering resolver, a megabyte in memory for the image's `/data` and no mount) and the network
are fixed in the runner, never taken from HQ, and the
observer has its own random token that the turn never sees. The workbench's fence is unchanged
but for the network (and the DNS setting Docker refuses on a joined network). There is no new
egress and no new endpoint: nothing leaves the host, and the records are read only through the
runner host's own Docker access. The observer lives with its item: a later turn of the item
(another member's, a retry) finds the same observer and its history. When the item is let go the
observer is kept another 30 minutes so the benchmark rig can export its records, then removed at
the sweep. One that is lost (stopped, or removed) is never replaced by a fresh empty one, which
would read as a tool that had no side effects: the item's further turns are refused as never
run, with "evidence is unavailable". An observer that does not say it is serving within ten
seconds is taken down and the turn refused the same way. A runner that restarts takes back the
observers it finds by their label. Scenarios:
`app/runner/features/a-probe-observer-starts-before-the-workbench.feature`.

## Context

The always-on plan wants Endora to do real technical work for its owner (changes to their
apps, to Endora itself and to the home lab, and research) without ever being able to do
harm on its own. A single assistant turn cannot carry that: the work spans days, needs more
than one kind of judgement, has to be checked by something other than the thing that did it,
and has to stay inside a budget. The owner also wants to see it happening, the way you would
see a small company at work, and to stay the one who decides.

## Decision

Endora gets a **team**: a separate feature, Endora HQ, in its own container.

- **A team works towards a goal.** The owner sets goals, or Endora proposes them for approval.
  Work is a tree of tasks under the goal; every task can say why it exists; a task has one
  owner at a time, taken in a single atomic step; members talk only in the task's thread.
- **Three parts.** The Endora node stays the trust boundary (gateway, secrets, brain, audit).
  HQ holds the team and the work, with its own database and settings. Runners (the Pi,
  Docker on the NAS) run each turn in a fresh container that can reach only HQ and the
  model provider.
- **One protocol for every worker.** A worker is any agent runtime that speaks the team
  protocol, an MCP server HQ provides inside the container. The first is Endora's own small
  Rust worker, thinking with the local model (amended 2026-10-16, the owner's decision; it
  replaced the Claude Agent SDK first planned here); Claude, through the node's relay, comes
  later, per member; then Codex on Sign in with ChatGPT, and DeepSeek.
- **Real actions go through Endora.** A member asks; HQ passes it to Endora's gateway, which
  allows, refuses or asks the owner. Code goes to pull requests (Forgejo for public
  repositories, GitHub for private ones, opened by the gateway); only the owner merges.
- **The owner's desk** collects every decision. Six things stay the owner's even when a CEO
  member exists later: Endora's own policy and permissions, spend beyond the credits,
  releasing a brief, merging and deploying, adding a runner, and the kill switch.
- **Briefs** carry what the team may know about the owner: prepared by Endora from the brain
  with names and places replaced, and released only from the owner's desk.
- **Money** is a ledger: reserved before a run, reported during it, and enforced then. The
  team never moves to paid billing.
- **Done** is decided by a gate (tests red then green, build, a review by a different
  member), never by the member that did the work.
- **The office** is a view over a snapshot-and-events contract, phone first, driven in tests
  and demo mode by a simulated runner.
- **Built in Rust in this repository**, from Endora's own specification, in seven phases.

## Consequences

- Endora can take on long, multi-step technical work without holding more authority than
  before: every privileged act still crosses the gateway and lands in the audit log.
- The owner gets a new place to decide things, the desk, and a visible picture of the work.
- A second service to deploy and keep healthy, and a runner on each host that runs work.
- Phases 3 and 4 depend on steps 4 and 5 of the plan (the API key, the Pi on its own VLAN);
  phases 1 and 2 do not.

## Alternatives considered

- **Running an existing agent-orchestration product.** Rejected: the ones reviewed run agents
  with broad local access by default, keep a deletable activity log, charge budgets mostly
  after a run ends, and hold credentials that would go around Endora.
- **Wrapping coding CLIs directly, with their own tools acting on real systems.** Rejected for
  the first version; a CLI may later be an adapter, still inside the container and with no
  credentials.
- **Putting the team inside the Endora node.** Rejected: a separate container keeps the node
  small and lets HQ restart, upgrade or be switched off without touching Endora.
