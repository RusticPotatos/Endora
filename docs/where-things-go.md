# Where things go

The map of Endora for anyone adding to it, person or agent. Read it **before** adding a screen,
a card, a setting, a menu entry, a file or a crate, and put the new thing where this says. If
nothing here fits, that is a gap in the map: say so, and add the place here in the same change,
rather than inventing a spot.

Things landed in the wrong place because there was no map: the Mac's model setup went first
onto the team's screen, then under Settings beside sign-in, when every other model already
lived in AI; its card and busy switch stayed on HQ until the owner asked, on 31 Oct, why the Mac
had its own place there. Each placement was reasonable on its own; none asked where models live.

## The console (the web app)

Endora is HQ ([ADR 0136](adr/0136-endora-is-hq.md)): every device opens on HQ, and the butler's
chat is retired. The menu holds the everyday destinations; everything that configures lives in
Settings.

**Endora Console** (`RusticPotatos/endora-console`, ADR 0145) carries a copy of these screens as
its own deployable, calling only the routes in `docs/api/console-v1.json`. Until the owner's
cutover, HQ screen changes land **here**, in Endora, and Console copies them; the built-in screens
are removed only after the cutover.

| Destination | What belongs there | What does not |
|---|---|---|
| **HQ** (where every device opens, on the agents map) | The team at work: the agents map with your decisions beside it, giving work; then open work, projects and the team, health, history, the chess league and the office. Status and switches you flip often (kill switch) | Setting anything up once: addresses, keys, tokens. Link to its Settings place instead |
| **Home lab** | The lab's containers, logs, jobs, now | Credentials (Connections, AI), models (AI) |
| **Music, Speakers, Smart home, Media** | Using that thing, and its own status | How it is connected (Connections) |
| **Activity** | The record of what Endora did | Anything you change |
| **Settings** | Everything you set up once, in the sections below | Day-to-day controls |

**Settings sections:**

| Section | What belongs there |
|---|---|
| **AI** | **Every model** Endora or its team uses: which, where it runs (the NAS, the Mac, a cloud), its address and key, what it sees, and what is left with each paid provider (Claude's balance as the Console shows it, the thresholds; the balances themselves are chips on HQ). The Mac's card, beside the house's Everyday models: its state, the Mac busy switch and its setup (HQ says only a word of its state where a member's brain is named, "Mac (busy)"). A new model or model host goes here |
| Connections | Outside services and tool servers Endora talks to (Home Assistant, the lab's services) |
| Tools | What the team's turns may use from the house, and which ask you first (each skill's stance, ADR 0108) |
| Your sign-in | Your password, authenticator, script tokens |
| Your data | Export, delete; never what the team runs on |

**Rules of thumb**

- Set up once → Settings. Looked at or flipped often → the screen it belongs to, with a link to
  its Settings place.
- A model, wherever it runs → **AI**. An outside service → **Connections**. A secret Endora
  uses → the place of what uses it (a model's key in **AI**, a service's token in
  **Connections**), sealed in Endora's own store ([ADR 0144](adr/0144-endora-keeps-its-own-secrets.md));
  never a vault screen. OpenBao is the owner's own service, in ansible-nas, not Endora's.
- One thing, one home. If two screens need it, one owns it and the other shows a line and a link.
- A new top-level menu entry needs a reason the existing ones cannot hold it, in its ADR.
- The team is one team of loops ([ADR 0142](adr/0142-one-team-of-loops.md)): members are ranked
  models with no roles, and each loop is a project with a plan, checks, a starting rank and how many items it has in hand at once. A
  loop's plan and progress go in Work; its notes and the map go with what the team knows, in
  Team. Nothing new goes in for roles, reporting lines, conversations or office votes.
- A loop's lessons (ADR 0142, amended 2026-10-31) are kept with the loop in HQ's settings
  (`loop_lessons:<project>`), and their rules (the `Lesson:` mark that starts a line of a
  worker's last word or a reviewer's notes, and the limits) in the team's domain,
  `domains/team/src/domain/lessons.rs`. The owner edits them whole through
  `/v1/hq/projects/{id}/lessons`, beside the loop's rank and items; no screen yet.
- A loop worked by one member alone (ADR 0142, amended 2026-11-01, round 4's solo arm) is a
  setting of the loop's beside its rank and items (`loop_solo:<project>`, set through
  `/v1/projects/{id}/solo`), and its rules are in the loops' engine
  (`domains/team/src/application/engine/loops.rs`); no screen.
- A round of the experiment under way (ADR 0142, amended 2026-11-01) is a setting of HQ's
  (`round_running`, its items), set by `scripts/experiment.py start` through `/v1/round`, and its
  rule (no downtime while an item is open) is the office's, in
  `domains/team/src/application/engine/office.rs`; no screen.
- A loop's repository outside the lab (ADR 0115, amended 2026-11-01: round 4's F4) is a setting
  of its project (`project_repo:<project>`), set by the owner through
  `/v1/hq/projects/{id}/repo`; the rule of which addresses may be named is the kernel's
  (`shared/kernel/src/lab.rs`, `outside_repo`), and the node's mirrors of such repositories sit
  in its data folder under `outside-repos/`, beside the fork's. No screen yet.
- HQ opens on the **agents map** ([ADR 0143](adr/0143-hq-opens-on-the-agents-map.md), the
  owner's decision M6, 7 Nov 2026) on every device, after a reload, from the menu and from any
  old route; nothing remembers another landing. The map is the shell: the bar (Give work, the kill
  switch) and the chips on every place; then the map (what runs, what waits to start, what is
  blocked; you with your desk by project; each member with the open items it holds, grouped by
  project; the items nobody holds; who is away), read-only from HQ's `/v1/hq/team-map`
  (`domains/team/src/domain/team_map.rs` decides each state; `47h-hq-agents.js` draws it) with
  HQ's own snapshot and poll. A line moves only while a turn runs and was heard from. An item
  opens its Live Work map, a member its Looking at page. **Your decisions** stand beside it (under
  it on a phone): the desk, its first five whole and the rest one line each, then *While you were
  away*. Something that needs your word goes on the desk; something you may want to look at, but
  nothing waits on, goes in the digest. When HQ stops the team itself, that is a card on the desk.
- Everything else in HQ is a **secondary place**, reached from one quiet row ("More of HQ") and
  left by "◀ Agents map" or Escape; never a tab strip, and add no new place without a reason none
  of them can hold. **Open work**: the open tasks, finding a task, and the Holodeck's chess
  matches, folded (ADR 0135). **Projects & team** (in this order, the owner's notes of 31 Oct): the
  money and credits (with what each model cost by week, a fold on the Money card), goals and their
  notes, projects and their loops with the routines among them (ADR 0107; adding one at the foot of
  the Projects card); then accepted work with its two switches (ADR 0115, amended 2026-10-31), what
  the team knows (the world's pages and the house), the team board (a line a member, the rest on a
  tap) with hiring, and what the team may do on the home lab. **Health**: HQ's counters, the
  scorecard and outcomes, the turns that failed with a link to the Home lab. A switch for a guard
  tried in shadow sits on that guard's line here, not in Settings. **Activity**: a "Running now"
  strip with a Watch link each, the latest twenty lines and "Show more" for a full-screen view a
  page at a time; what a line names opens where it already lives, with a way back. **Chess
  league**: the model league's games in play, bracket and standings, every finished game with
  its PGN. **Office**: the studio (`47f-hq-studio.js`), drawn and animated only while the Office
  shows, with the office's games beside it (the office chess board, ADR 0121). The studio's
  furniture, desks lit only while a turn runs there, the owner's corner, the alcove and the lounge
  are as before, and a tap opens Looking at. **Looking at** is the page a member, an item or a
  studio tap opens, with its way back. Each place shows one line per thing at rest and the rest on
  a tap; the console check holds each to a budget, so fold something before raising one. Fun and
  decoration go in the Office, never on the map. A turn that waits for tomorrow shows in the top
  bar and in the outcomes, and is never a card. Explicit assistance requests and conversations add
  no place of their own (ADR 0139). Every owner action and its place is listed in
  [the inventory](proposals/hq-map-first-action-inventory-2026-11-07.md).
- One item's **Live Work map** (the owner's decision M3, 7 Nov 2026) is a screen of its own, like
  Replay, opened from that item on the map or under Looking at ("Live work map", beside "Replay
  this task"): read-only, one item only, from HQ's `/v1/tasks/{id}/live` read
  (`domains/team/src/domain/live_work.rs` decides what each record means; `47g-hq-live.js` draws
  it). No other items, no controls. The research team's map is not in Core: its records are the
  Mac's, so it is a checkpoint page drawn by `scripts/research-map/` and published as an artifact,
  stamped with its snapshot's time; never a live feed.

## The repository

| Path | What belongs there |
|---|---|
| `app/` | Composition roots: one per program (node, hq, worker, runner, sound, …). Wiring only |
| `domains/<context>/` | One crate per bounded context, each with its own domain / application / infrastructure ([domain-map.md](domain-map.md) says what each owns) |
| `shared/` | Only what is genuinely cross-cutting (`kernel`, `persistence`) |
| `crates/` | The orchestration layer and infrastructure adapters shared by several contexts |
| `clients/` | Programs that talk to the node only over HTTP (the handheld terminal) |
| `app/node/src/web/js/` | The console, one file per screen family; the menu is in `70-dispatch.js`, Settings' list in `20-common.js` (`UNDER_SETTINGS`), AI in `30-settings.js`; HQ's places in `47-hq.js` (`HQ_PLACES`), the agents map in `47h-hq-agents.js`, the studio in `47f-hq-studio.js`, the Holodeck card in `47d-hq-holodeck.js` and the Chess league in `47e-hq-chess.js`; the console's own scenarios in `app/node/src/web/features/`, run by `scripts/check-console.mjs` |
| `domains/team/src/{domain,application,infrastructure/sqlite}/` | Conversation records and policy belong to the team context alongside history; owner HTTP wiring stays in `app/hq/src/api/conversations.rs` and its fixed node proxy |
| `deploy/compose/` | How anyone else installs Endora (ADR 0147, [install.md](install.md)): `core/` is Core's own compose file and documented `.env`; the bundle's `compose.yaml` includes it with verbatim copies of Console's and Audio's compose files, pinned in `SOURCES.json`; `ci/` and `test-bundle.sh` are its CI test, each check a step of `features/`. A new Core setting goes in `core/compose.yaml` and `core/core.env.example` together; a product's file here is only ever replaced by its release's |
| `deploy/ansible/collection/` | The `endora.homelab` collection: how Endora's part of the lab is declared. A new variable in `docker-compose.yml` needs a variable in its `endora` role in the same change. Services that are not Endora's live in ansible-nas, not here: OpenBao, the owner's own vault, is its `openbao` role, and Endora does not use it (ADR 0144) |
| `scripts/check-collection-parity.sh` | Fails when `docker-compose.yml` reads a variable or profile the `endora` role neither declares nor lists as Mac-only; run by CI |
| `scripts/` | Checks and helpers run by people, CI or the Mac (`scripts/mac-brain/`); `code-size.sh` and `prune-builds.sh` for keeping the code base and the disk in hand; `gate.sh` (the full handoff gate; it runs the checks listed in `required-checks.txt` through `check-required.sh`, which fails for a required check that is missing, stubbed or failing, and `test-check-required.sh` tests that) and `release.sh` (the release to the NAS and the Pi, which stops at any failing stage; `test-release.sh` holds it to that against fakes); `replay-bench/` (replaying stored turns on another model; its data stays outside the repo); `harness-probes/` (the agentic-harness climb's rig: seeded probe families, their graders, both arms' runners and accounting; see its `PROTOCOL.md`; its runs' data stays outside the repo); `research-map/` (the research team's checkpoint map: `org.json` declares the roles and who sends what to whom, `render.mjs` draws a still page from a snapshot the steward writes; tests run with `node --test scripts/research-map`; the snapshots stay outside the repo) |
| `.claude/` | How an assistant runs the baseline experiment the same way each round: the skill `skills/hq-round/` (the playbook), the agents `agents/round-judge.md` and `agents/duplication-reviewer.md`, the workflow `workflows/end-of-round.js`, and the guard hooks `hooks/` wired in `settings.json` (no edits to the lab's secrets, the nothing-personal check before a push). Nothing personal: the repository is public |
| `docs/api/` | Core's published contracts: `console-v1.md` (what Endora Console may call, and how Core answers) and its route list `console-v1.json`, which Core's tests and Console's proxy both read (ADR 0145). A route Console needs is added here, additively, in the same change as the route |
| `app/node/src/api/console.rs` | The routes the Console contract adds: session end, `/v1/decisions`, `/v1/events` and its stream; held by `console_tests.rs` |
| `docs/adr/` | One decision each; a new architectural choice needs one |
| `docs/proposals/` | Research and proposed changes, with sources and tradeoffs; accepted architectural decisions go in `docs/adr/` |
| `docs/homelab.md` | How to set up and run each part in the house |
| `local.mk` | Your own values (addresses, paths, names). Git-ignored; never in a tracked file |
