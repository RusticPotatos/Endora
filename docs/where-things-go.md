# Where things go

The map of Endora for anyone adding to it, person or agent. Read it **before** adding a screen,
a card, a setting, a menu entry, a file or a crate, and put the new thing where this says. If
nothing here fits, that is a gap in the map: say so, and add the place here in the same change,
rather than inventing a spot.

Things landed in the wrong place because there was no map: the Mac's model setup went first
onto the team's screen, then under Settings beside sign-in, when every other model already
lived in AI. Each placement was reasonable on its own; none asked where models live.

## The console (the web app)

The menu holds the everyday destinations; everything that configures lives in Settings.

| Destination | What belongs there | What does not |
|---|---|---|
| **Home** (chat) | Talking with Endora, the daily brief | Settings of any kind |
| **Mail, Music, Speakers, Smart home, Media** | Using that thing, and its own status | How it is connected (Connections) |
| **Home lab** | The lab's containers, logs, jobs, now | Credentials (Vault), models (AI) |
| **HQ** | The team at work: your desk, giving work, the team, its work, its knowledge, the office. Status and switches you flip often (kill switch, Mac busy) | Setting anything up once: addresses, keys, tokens. Link to its Settings place instead |
| **Activity** | The record of what Endora did | Anything you change |
| **Settings** | Everything you set up once, in the sections below | Day-to-day controls |

**Settings sections:**

| Section | What belongs there |
|---|---|
| Notifications & voice | How and when Endora reaches you, and speaks |
| **AI** | **Every model** Endora or its team uses: which, where it runs (the NAS, the Mac, a cloud), its address and key, what it sees. A new model or model host goes here |
| Connections | Outside services and tool servers Endora talks to (Home Assistant, the lab's services) |
| Skills | What Endora can do in chat |
| Permissions | How far Endora may act on its own |
| What Endora watches | What it does about each kind of thing |
| About you | What you told it and what it knows |
| Your sign-in | Your password, authenticator, script tokens |
| Vault | The house's secrets and the passkeys that unlock them |
| Your data | Export, delete |

**Rules of thumb**

- Set up once → Settings. Looked at or flipped often → the screen it belongs to, with a link to
  its Settings place.
- A model, wherever it runs → **AI**. A secret → **Vault**. An outside service → **Connections**.
- One thing, one home. If two screens need it, one owns it and the other shows a line and a link.
- A new top-level menu entry needs a reason the existing ones cannot hold it, in its ADR.
- Inside HQ, use its tabs: Desk (what waits on you), Team, Work, Knowledge, Office (games, last).
  Fun and decoration go in Office, never above the desk.

## The repository

| Path | What belongs there |
|---|---|
| `app/` | Composition roots: one per program (node, hq, worker, runner, sound, …). Wiring only |
| `domains/<context>/` | One crate per bounded context, each with its own domain / application / infrastructure ([domain-map.md](domain-map.md) says what each owns) |
| `shared/` | Only what is genuinely cross-cutting (`kernel`, `persistence`) |
| `crates/` | The orchestration layer and infrastructure adapters shared by several contexts |
| `clients/` | Programs that talk to the node only over HTTP (the handheld terminal) |
| `app/node/src/web/js/` | The console, one file per screen family; the menu is in `70-dispatch.js`, Settings' list in `20-chat.js` (`UNDER_SETTINGS`), AI in `30-settings.js` |
| `deploy/ansible/collection/` | The `endora.homelab` collection: how the lab is declared. A new variable in `docker-compose.yml` needs a variable in its `endora` role in the same change |
| `scripts/check-collection-parity.sh` | Fails when `docker-compose.yml` reads a variable or profile the `endora` role neither declares nor lists as Mac-only; run by CI |
| `scripts/` | Checks and helpers run by people, CI or the Mac (`scripts/mac-brain/`) |
| `docs/adr/` | One decision each; a new architectural choice needs one |
| `docs/homelab.md` | How to set up and run each part in the house |
| `local.mk` | Your own values (addresses, paths, names). Git-ignored; never in a tracked file |
