# Where things go

The map of Endora for anyone adding to it, person or agent. Read it **before** adding a screen,
a card, a setting, a menu entry, a file or a crate, and put the new thing where this says. If
nothing here fits, that is a gap in the map: say so, and add the place here in the same change,
rather than inventing a spot.

Things landed in the wrong place because there was no map: the Mac's model setup went first
onto the team's screen, then under Settings beside sign-in, when every other model already
lived in AI. Each placement was reasonable on its own; none asked where models live.

## The console (the web app)

Endora is HQ ([ADR 0136](adr/0136-endora-is-hq.md)): every device opens on HQ, and the butler's
chat is retired. The menu holds the everyday destinations; everything that configures lives in
Settings.

| Destination | What belongs there | What does not |
|---|---|---|
| **HQ** (where every device opens) | The team at work: your desk, giving work, the team, its work, its knowledge, the office. Status and switches you flip often (kill switch, Mac busy) | Setting anything up once: addresses, keys, tokens. Link to its Settings place instead |
| **Home lab** | The lab's containers, logs, jobs, now | Credentials (Vault), models (AI) |
| **Music, Speakers, Smart home, Media** | Using that thing, and its own status | How it is connected (Connections) |
| **Activity** | The record of what Endora did | Anything you change |
| **Settings** | Everything you set up once, in the sections below | Day-to-day controls |

**Settings sections:**

| Section | What belongs there |
|---|---|
| **AI** | **Every model** Endora or its team uses: which, where it runs (the NAS, the Mac, a cloud), its address and key, what it sees. A new model or model host goes here |
| Connections | Outside services and tool servers Endora talks to (Home Assistant, the lab's services) |
| Tools | What the team's turns may use from the house, and which ask you first (each skill's stance, ADR 0108) |
| Your sign-in | Your password, authenticator, script tokens |
| Vault | The house's secrets and the passkeys that unlock them |
| Your data | Export, delete; never what the team runs on |

**Rules of thumb**

- Set up once → Settings. Looked at or flipped often → the screen it belongs to, with a link to
  its Settings place.
- A model, wherever it runs → **AI**. A secret → **Vault**. An outside service → **Connections**.
- One thing, one home. If two screens need it, one owns it and the other shows a line and a link.
- A new top-level menu entry needs a reason the existing ones cannot hold it, in its ADR.
- Inside HQ, use its tabs: Desk (what waits on you, and at its top the digest of what happened
  while you were away), Team (with how the team is doing: outcomes first), Work, Knowledge,
  Office (games, last; a match's step-by-step replay is a tap on its result). The Holodeck's
  scenarios and trials are work, so they are a card in Work, each opened step by step there.
  Something that needs your word goes on the desk; something you may want
  to look at, but nothing waits on, goes in the digest. Fun and decoration go in Office, never
  above the desk. A switch for a guard that is tried in shadow sits on that guard's line of the
  Guards card, in Work, and not in Settings; a turn that waits for tomorrow shows in the top bar
  and in the outcomes, and is never a card. When HQ stops the team itself, that is a card on the
  desk, because it needs your word.

## The repository

| Path | What belongs there |
|---|---|
| `app/` | Composition roots: one per program (node, hq, worker, runner, sound, …). Wiring only |
| `domains/<context>/` | One crate per bounded context, each with its own domain / application / infrastructure ([domain-map.md](domain-map.md) says what each owns) |
| `shared/` | Only what is genuinely cross-cutting (`kernel`, `persistence`) |
| `crates/` | The orchestration layer and infrastructure adapters shared by several contexts |
| `clients/` | Programs that talk to the node only over HTTP (the handheld terminal) |
| `app/node/src/web/js/` | The console, one file per screen family; the menu is in `70-dispatch.js`, Settings' list in `20-common.js` (`UNDER_SETTINGS`), AI in `30-settings.js` |
| `deploy/ansible/collection/` | The `endora.homelab` collection: how the lab is declared. A new variable in `docker-compose.yml` needs a variable in its `endora` role in the same change |
| `scripts/check-collection-parity.sh` | Fails when `docker-compose.yml` reads a variable or profile the `endora` role neither declares nor lists as Mac-only; run by CI |
| `scripts/` | Checks and helpers run by people, CI or the Mac (`scripts/mac-brain/`) |
| `docs/adr/` | One decision each; a new architectural choice needs one |
| `docs/proposals/` | Research and proposed changes, with sources and tradeoffs; accepted architectural decisions go in `docs/adr/` |
| `docs/homelab.md` | How to set up and run each part in the house |
| `local.mk` | Your own values (addresses, paths, names). Git-ignored; never in a tracked file |
