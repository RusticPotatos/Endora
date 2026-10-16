# 0102 — Endora HQ

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions Q1–Q34). Prepared
with AI assistance (Claude). Step 7 of [the always-on butler plan](../plans/always-on-butler.md);
the specification is [Endora HQ: the design](../plans/endora-hq.md). Phase 1, the team core,
is built in `domains/team`; the rest is not yet.

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
  protocol, an MCP server HQ provides inside the container. The first is the Claude Agent SDK
  on the owner's Max API credits; then the local model, Codex on Sign in with ChatGPT, and
  DeepSeek.
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
