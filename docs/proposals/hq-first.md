# Proposal: Endora becomes HQ

Written 2026-10-20 for the owner's decision. Prepared with AI assistance (Claude). The owner
said yes the same day; their answers to the four questions, and what follows for mail, are in
[ADR 0136](../adr/0136-endora-is-hq.md), which this proposal now only explains. The owner's words: the chat "is just kinda bad", and Endora should be
"primarily just the HQ and not the butler chat".

## What is there today, and where each piece goes

Three verdicts: **keep** (it serves HQ and the team), **retire** (it serves only the butler
chat), **shared** (infrastructure HQ needs, kept and possibly trimmed).

### The menu

| Entry | Screen (file) | Behind it in the node | Verdict | Why |
|---|---|---|---|---|
| Home (chat) | `viewChat`, 20-chat.js, 60-chat-actions.js | `/v1/chat*`, `/v1/brief*`, `/v1/research*`, `/v1/deep-ask`, the turn in `endora-application` | **Retire** | The butler chat itself |
| Mail | `viewMail`, 30-settings.js | `/v1/mail`, `domains/mail`, the mail-reader loop | **Ask the owner** (recommend retire) | Its only consumers are the daily brief and the agenda loop, both retiring |
| Music | `viewMusic`, 45-lab.js | `/v1/connections/spotify*` | **Keep** | No chat dependency; the handheld uses it |
| Speakers | `viewSound`, 49-sound.js, 49b | `/v1/sound/*` to `app/sound` | **Keep** | Its own hub (ADR 0104); only its chat actions retire |
| Smart home | `viewHouse`, 45-lab.js | `/v1/terminal/home*` | **Keep** | Handheld and console read it; no chat |
| Media | `viewMedia`, 45-lab.js | `/v1/terminal/media*` | **Keep** | As above |
| Home lab | `viewLab` and its pages, 45-lab.js | `/v1/homelab*`, `/v1/terminal/*`, the home-lab loop | **Keep** | The same observer feeds HQ's alarms (0107) and contracts (0106) |
| HQ | 47-hq.js, 47b, 47c, 47d | `/v1/hq/*`, `/v1/mac*`, team listener on 8788 | **Keep, and land here** | The product |
| Activity | `viewAudit`, 20-chat.js | `/v1/activity*`, `/v1/audit`, `audit_log` | **Shared** | HQ writes every owner action there; constitution section 10 |
| Settings | `viewSettings` | | **Keep, trimmed** | Below |

The pinned "needs you" cards and the Now line on the chat (48-agenda.js) go with the chat.

### Settings

| Section | What it holds today | Verdict | Why |
|---|---|---|---|
| Notifications & voice | Read aloud, show actions, vibrate, Start on, the daily brief's schedule | **Retire** | Every switch is about chat replies or the brief. Start on is moot once HQ is the landing |
| AI | The roles card, providers, the Everyday and Deep lists, the deep model, the Mac's brain, Worth knowing | **Shared, trimmed** | The team relay takes the Anthropic key from the providers and its house model from the first private Everyday entry (`api/relay.rs:754`); the Mac's brain is HQ's. The deep model, Ask deep, the mail reader's model, vision and speech roles are the chat's |
| Connections | Home Assistant, the lab's services, Spotify, tool servers | **Shared** | The bridge (0108) reads Home Assistant, Sonarr, Radarr and the tool servers; contracts (0106) read the lab |
| Skills | Each skill's on / ask / off and settings; recipes | **Shared, trimmed** | The bridge offers team turns the house's tools with exactly these stances (`domains/capabilities/src/bridge.rs:137`). Recipes (0071) are chat-only and retire. Renamed to say what it now is: what the team may use from the house |
| Permissions | The autonomy envelope: read-only skills on their own, consequential acts on their own | **Retire** | Read only by the chat's turn; the bridge uses per-skill stances, not the envelope |
| What Endora watches | The agenda loop's kinds and rules | **Retire** | See the agenda loop below |
| About you | The brain (0101): what you told it, and its pages | **Ask the owner** (recommend retire after export) | The brain learns only from chat turns. HQ's planned owner brief (0102, phase 6) would read it, but is not built |
| Your sign-in | Password, authenticator, script tokens | **Shared** | The console's door |
| Vault | Secrets and passkeys | **Shared** | Sealed keys, the Anthropic key among them |
| Your data | Export, Delete everything | **Shared, reworked** | Today's export holds the chat's data and nothing of HQ's. Today's Delete everything also wipes the model lists and skill settings HQ needs (see "A bug to fix first") |

So Settings becomes: **AI, Connections, Tools, Your sign-in, Vault, Your data** (six
sections, down from ten).

### What runs on its own in the node (`app/node/src/main.rs`)

| Loop | Verdict | Why |
|---|---|---|
| Heartbeat: the daily brief, pushing it to the phone, watching Home Assistant for chat | **Retire** the brief and the watching; keep the tool servers' retry and withdrawal | The retry keeps the bridge's tools alive |
| Research worker | **Retire** | Chat-only |
| Brain folding | **Retire** with the brain | |
| Mail reader | **Retire** if Mail goes | |
| Agenda loop (0103) | **Retire** | Its surfaces were the chat's cards. It restarts containers on its own, and so do HQ's contracts: two restarters with two ledgers (`agenda_restarts`, `team_restarts`) on the same containers. HQ's routines and alarms (0107) already watch the lab |
| Home lab, the Mac, nightly snapshots, the team listener | **Keep** | HQ's |

### Elsewhere

- **The handheld terminal** has a chat tab (`clients/terminal/src/console.rs:458`, posting
  to `/v1/chat/stream`). It loses that tab, in its own release, before the routes go. Its
  home, media, lab and music pages stay.
- **Tests and scripts that name the chat**: the sign-in scope tests use `/v1/chat` as their
  example of a private read (`app/node/src/auth.rs:327, 401, 449`), and
  `scripts/smoke-kitchen-light.sh` posts to it. The tests take another private route; the
  smoke script retires.
- **`/health`** reports whether a chat turn is running and which model serves it; it keeps
  the version and build, and drops those two.
- **HQ's build** depends on `endora-infrastructure`, and through it on every butler context,
  for one type (`TimeZone`, `app/hq/src/main.rs:106`). That edge is cut first.

## HQ as the landing screen

Every device opens on HQ. `startOn()` and its `endora.startOn` setting go: there is nothing
else to start on. The crumbs' "Home" becomes HQ. HQ's Desk tab is what opens, as today.

## Removed, not hidden

ADR 0100 settled this: "switching the features off and keeping the code" was rejected by the
owner, because off-but-present code still has to be kept compiling and understood; and a table
nothing reads is data nothing can export or delete, against constitution section 6. So the end
state is removal of code and storage, the same way: the node drops the retired tables when it
starts, and a fresh database never creates them.

What is **hidden first** is only a short, reversible step on the way (step 2 below), so the
owner lives with an HQ-only console for a few days before anything is deleted.

**Removed, code and storage** (once the owner says yes, after a copy is taken):

- The chat: `/v1/chat*`, the turn and its streaming (`endora-application`'s `usecases/turn.rs`
  and the `Butler` port), `domains/conversation`; tables `messages`, `message_actions`,
  `conversation_summary`.
- The daily brief: `/v1/brief*`, `domains/scheduling`; table `brief_schedule`.
- Research: `/v1/research*`, the research worker; tables `intentions`, `intention_research`,
  `execution_leases`.
- Outcomes, repairs and reliability as the chat used them: `/v1/outcomes*`, `/v1/repairs*`,
  `/v1/reliability`; tables `outcomes`, `repairs_left_alone`.
- Preferences and the brain, if the owner agrees (question 2): `/v1/brain*`,
  `/v1/preferences*`, `/v1/context`, brain folding; the brain folder on disk.
- The autonomy envelope: `/v1/autonomy`; table `autonomy_envelope`.
- Recipes: `/v1/recipes*`; table `recipes`.
- The deep model and Ask deep: `/v1/deep-model*`, `/v1/deep-ask`, `/v1/model-layer/run`; tables
  `deep_model`, and `butler_model_config` once the relay's fallback reads the installed model
  directly. The Deep list goes from the AI screen.
- The agenda loop: `/v1/agenda*`, `domains/agenda`; its six tables.
- Mail, if the owner agrees (question 3): `/v1/mail`, `domains/mail`, the mail reader;
  `mail_messages`, `mail_meta`.
- The watching of Home Assistant for the chat: tables `watched_things`, `transitions`.
- The console's chat and agenda files (20-chat.js's chat, 48-agenda.js, 50-views.js,
  60-chat-actions.js), and the parts of 10-core, 30-settings, 40-skills and 70-dispatch that
  serve them.

**Kept:** sign-in, the vault and its secrets, Activity and `audit_log`, `events`, the model
providers and the Everyday list with `model_usage` and `bench_spend`, skills with their
stances and settings, tool servers, `config_writes` (the record of changes made to the owner's
services, which ADR 0054 keeps for undo), the home lab and handheld pages, Spotify, sound, HQ
and the team listener, and `domains/capabilities` whole (the bridge and contracts live there).

**Size, roughly:** `endora-application` (19,190 lines of Rust) loses most of itself;
`domains/conversation` (562), `domains/scheduling` (209) and `domains/agenda` (2,612) go;
`domains/understanding` (4,894) goes or shrinks; `domains/mail` (3,162) if Mail goes; about
2,000 lines of console. On the order of 25,000 lines, with their tests. The exact count goes in
the ADR when the work is done.

## A bug to fix first, whatever is decided

Settings, Your data, Delete everything (`POST /v1/memory/purge`) deletes the rows of
`capability_settings`, `model_providers`, `model_entries` and `model_lists_meta`
(`crates/endora-infrastructure/src/sqlite.rs:1238`). Those hold the team relay's Anthropic key
and house model, and the Home Assistant and lab tokens the bridge uses. So "delete my memory"
today silently stops the team. That is a fix in its own right (step 1), with a test.

## The order of the work

Each step is its own branch off `develop`, tests first, through the full gate, and deployed and
looked at before the next. Steps 1 to 3 are reversible; step 5 is not.

1. **Prepare (no visible change).** Cut HQ's build edge to `endora-infrastructure` (move
   `TimeZone` where HQ can reach it without the butler). Stop Delete everything from wiping
   what HQ needs.
2. **HQ first, the rest hidden (reversible).** HQ is the landing for every device; Start on
   goes. The menu becomes HQ, Home lab, Music, Speakers, Smart home, Media (and Mail, if kept),
   Activity, Settings. Settings becomes the six sections. The butler's own loops (brief,
   research, brain folding, agenda, mail reader if retired) no longer start. The routes still
   answer, so the handheld's chat keeps working and nothing is lost yet. The console check
   holds the new menu and Settings list.
3. **The handheld loses its chat tab** (clients/terminal, with its own checks), deployed to the
   device.
4. **A copy.** The owner takes Your data, Export, and the nightly snapshot of the database and
   brain is confirmed in `backups/`. This is the last point where the chat can come back as it
   was.
5. **Removal (irreversible).** Code, routes and tables as listed, dropped on start like
   `drop_the_clean_slate`; the export loses its chat fields (a breaking change, marked so);
   `/health`, the scope tests and the smoke scripts updated. Each superseded record gets a dated note.
6. **Documents.** `where-things-go.md`, `architecture.md`, the README, the standards file's
   "Projects at a glance" line, and ADR 0136 accepted. The constitution's preamble and section
   9 describe a personal butler that forms beliefs about the person; changing them is a
   governance amendment only the owner makes (section 11), so it is drafted as a proposal, not
   edited.
7. **Later, its own piece:** Your data covering HQ's record (the team's database is in no
   export and no delete today), which constitution sections 6 and 10 ask for.

## What only the owner can answer

1. **Go ahead?** Retire the chat and its data as above (step 5 cannot be undone).
2. **About you.** Delete the brain's pages after the export, or keep them as notes for the
   team's owner brief later? Recommended: delete. Nothing reads them once the chat is gone,
   the brief that would is not built, and the export keeps a copy.
3. **Mail.** Keep the Mail screen with its reader, or retire it? Recommended: retire; it fed
   the brief and the agenda, both going.
4. **The phone.** With the brief and the agenda loop gone, nothing pushes to the phone. Is
   that fine for now? Recommended: yes; a ping when HQ's desk needs a decision would be new
   work, proposed on its own.
