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
