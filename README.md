# Endora

Endora is a family of self-hosted products for your house. Each runs on **your** hardware, and
each can run on its own. This page lists them, says where to install each, and then describes
the one this repository holds today: Endora Core.

## The Endora products

| Product | What it is | Where it lives | Install |
|---|---|---|---|
| **Endora Core** | The headless engine: the node (holds authority and secrets), HQ's team of agents, and the runner | This repository, [RusticPotatos/Endora](https://github.com/RusticPotatos/Endora) | [Core's Docker Compose file](deploy/compose/core/) and [docs/install.md](docs/install.md) |
| **Endora Console** | The screens for Core: decisions, work, the team, health, the home lab, settings. It holds no authority of its own; you sign in to Core | [RusticPotatos/endora-console](https://github.com/RusticPotatos/endora-console) | Its README, "Run it". Needs a running Core |
| **Endora Audio** | A self-hosted sound hub: radio, Spotify and AirPlay sources into AirPlay speakers and browsers, with a level per source and speaker | [RusticPotatos/endora-audio](https://github.com/RusticPotatos/endora-audio) ([releases](https://github.com/RusticPotatos/endora-audio/releases)) | Its README, "Deploy". Needs no Core |

**Everything on one Linux host:** the [bundle in `deploy/compose/`](deploy/compose/) runs Core,
Console and Audio together, each product's own compose file unchanged
([ADR 0147](docs/adr/0147-full-system-bundle.md)). It needs only Docker Engine 28 or later and
Docker Compose v2.20 or later, not `make` or a checkout-specific config. First start,
prerequisites, ports, backups and upgrades are in [docs/install.md](docs/install.md).

**The split.** Console and Audio already have repositories of their own. Core is still built
and installed from this repository, which also holds the bundle and these docs. A separate
Core repository is not published yet: until this page links one, Core comes from here, and
nothing below should be read as saying otherwise.

## Endora Core: this repository

A local-first team of AI agents for your house: **Endora HQ**. A small team (a CEO, leads,
developers, a reviewer and a tester) takes on work you give it, mostly your home lab's, and you
watch it happen in an office on your phone and decide what it may do from your desk. It runs on
**your** hardware; the house's own model does most of the thinking, and Claude only where you
choose.

No cloud account. The language model *proposes*; deterministic policy decides what actually
happens. Every act a team turn takes crosses the Endora node, and nothing irreversible
happens without you.

## Why

- **Local AI.** The house's model runs on your box via Ollama (or any OpenAI-compatible
  server). A member may think with Claude instead, paid from your own key, within budgets you
  set.
- **Normal hardware.** A 7B model on a consumer GPU is plenty for the house's model — tested on
  an NVIDIA RTX A2000 12 GB with `qwen2.5:7b`. CPU works too, just slower.
- **You decide.** Goals, budgets, merges and deploys are yours. The desk interrupts you only
  for decisions; the kill switch stops everything.
- **Safe by construction.** Each turn runs in a fresh container that reaches only HQ and the
  node. The node's relay meters every model call, its contracts limit what may touch the lab,
  and the bridge offers the house's tools with each one's stance.

## What it does

- **HQ** (ADR [0102](docs/adr/0102-endora-hq.md)): goals, tasks, a team that claims and does
  them, reviews them and tests them against criteria written first; outcomes and a desk that
  interrupts only for decisions; the model chess league; and the Holodeck, where the team acts
  in simulated worlds to compare configurations.
- **The home lab**: the containers, logs, jobs and media of your lab, read through fixed
  routes, and the team's work on it through contracts and pull requests you merge.
- **The house**: smart home, media requests, music (Spotify) and speakers, from the console and
  from a handheld terminal.
- **Your record**: Activity shows everything Endora did and every decision policy made, and you
  can export or delete it.

The butler's chat, its brief, its memory of you and its mail reader were retired in
[ADR 0136](docs/adr/0136-endora-is-hq.md).

## Quick start

**Installing on a Linux host?** Use Docker Compose alone, no `make` and no checkout-specific
config: Core on its own from [`deploy/compose/core/`](deploy/compose/core/), or the full system
(Core, Console in front, and Audio) from [`deploy/compose/`](deploy/compose/). First start,
`endora-core setup`, ports, backups and upgrades: [docs/install.md](docs/install.md). What
follows is the developer's path from a checkout.

**Nothing installed yet?** `make bundled` starts Endora with a model runtime beside it —
it looks at the hardware it can see and serves something that suits it, so there is
nothing to set up first. The first start downloads a model (1–5 GB) once. It is a
starting point, not a recommendation: everything below still applies, and pointing
Endora at your own endpoint is the better end state.

Bring your own model — any OpenAI-compatible endpoint (Ollama, LM Studio, llama.cpp,
vLLM):

```bash
ollama pull qwen2.5:7b
make run-node                 # http://localhost:8787
```

Open **http://localhost:8787** — the node serves the whole UI; there's no separate app
to install. Or run it in a container (data persists in `./endora-data`):

```bash
make docker-build && make docker-run
```

- **Model recommendations, hardware guidance, tested setups, and private-egress /
  proxy notes:** [docs/model-hosting.md](docs/model-hosting.md).
- **Always-on and reaching it from your phone:** [docs/hosting.md](docs/hosting.md).
  The node authenticates `/v1` requests with bearer credentials; keep it on a trusted network — see
  [SECURITY.md](SECURITY.md).

## The safety model

- **Models propose; policy authorizes.** The model is never the enforcement boundary.
- **Reversibility first.** The team acts on its own only within reversible bounds — read,
  a container restart within limits, a branch. Anything irreversible or outbound — merge,
  deploy, spend beyond the pool — lands on your desk. Some things can't be undone; it treats
  them that way.
- **Your data stays home**; Endora's record is exportable and deletable, and egress is guarded
  and minimized.

Full principles: [Constitution](docs/constitution.md). Every design decision is
recorded as an [ADR](docs/adr/README.md).

## How it's built

Rust, a domain-first modular monolith organized **by responsibility**, not by layer —
Responsibility-Oriented Clean Architecture ([ROCA](docs/roca.md); [ADR 0050](docs/adr/0050-the-shape-of-the-system.md)).
One authoritative backend (the **node**) holds the authority and the secrets; HQ is a service
behind it, and clients are thin. The model sits *behind* the policy boundary.

```text
clients ──HTTP/JSON──▶ node ──▶ HQ (the team) ──▶ a turn in a fresh container
                         ▲                              │
                         └── relay · bridge · contracts ◀┘  policy, metering, audit
```

```text
app/      node · hq · worker · runner · sound · cli   ← composition roots
domains/  team · capabilities · platform · sound · mail
          each a crate layered domain / application / infrastructure, inward-pointing
shared/   kernel (ids, time, errors)   persistence (the shared SQLite handle)   protocol
```

See [docs/architecture.md](docs/architecture.md). There's also a CLI (`make run-cli
ARGS="health"`).

## Status

A working personal project, developed against a home server. It runs HQ, the home lab's
pages, the house's tools behind their stances, and the egress guard described above. Interfaces still
change. Tagged releases exist (latest: [v0.12.0](https://github.com/RusticPotatos/Endora/releases/tag/v0.12.0)); see the [releases page](https://github.com/RusticPotatos/Endora/releases) for what each contains. Build one complete slice at a time.

## Contributing & license

Read [CONTRIBUTING.md](CONTRIBUTING.md), the [Code of Conduct](CODE_OF_CONDUCT.md), and
[SECURITY.md](SECURITY.md). **AI-assisted contributions must be flagged as such** in the
PR. Contributors branch from `develop`.

Licensed under [Apache-2.0](LICENSE). Copyright 2026 Endora contributors. See
[NOTICE](NOTICE).
