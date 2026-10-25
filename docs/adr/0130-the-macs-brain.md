# 0130 — The Mac's brain

## Status

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Builds on
[0109](0109-a-brain-per-member.md) (a brain per member, and what a cloud turn may read),
[0105](0105-models-in-ordered-lists-by-data-tier.md) (data tiers: nothing here leaves the house),
[0102](0102-endora-hq.md) (the node signs to HQ, and is HQ's only way out) and
[0125](0125-secrets-at-rest-in-the-node.md) (the key and the token are sealed at rest). Built:
everything below, and the owner's scenarios in `domains/team/features/the-macs-brain.feature` and
`the-macs-brain-restart.feature`.

Amended 2026-10-18 by [0131](0131-the-two-kinds-of-turn.md): the Mac is now a member's **work
brain**, no longer its one brain. Its rules here (free, idle, one turn at a time, a turn that
waits where it is and says why, never another model) stand for the turns that need the Mac: code
and branch work, the engineering wing lead's planning, and a tester's work. Reading, status and
drafts may use the Mac when it is free
and the NAS's model when it is not; judgment never runs on it. `start_queued` decides this per
turn, from why the turn was asked for, and the Mac's one-turn rule counts turns started on it,
not members.

Amended 2026-10-19 (hygiene, from a review of the live record): the Mac has limits of its own,
between the NAS's model's and Claude's (*The Mac's limits*, below).

Amended 2026-10-19, from the live record (the Phase 0 audit's P5): **a long wait for the Mac
is said.** Work that needs the Mac waits for it, never moved to the NAS's model or to Claude
(0131), and the stuck ladder rightly leaves a waiting turn alone. Live, a tester's turns waited
ten hours overnight while the Mac could not be reached, and nothing reached the owner. Now,
when the oldest turn waiting for the Mac has waited more than three hours
(`mac::LONG_WAIT_MS`), one note goes in the owner's digest, which does not interrupt. It says who
waits and on which tasks, the Mac's reason, and what the owner can do: free the Mac, or change
who works on it from the roster. The note is said once while any turn waits for the Mac, and
again only after none has (`mac_long_wait_said`). The work keeps waiting.

## Context

The owner's laptop (an Apple Silicon Mac) can run a far stronger coding model than the NAS can.
It is also the machine the owner works on all day, so the model cannot simply stay loaded and
answer whenever it is asked: it holds about fifteen gigabytes, and a turn running while the owner
types makes both slow.

**Owner decisions, 2026-10-18.**

- Local models do the work; paid Claude judges. Quality work for a member who thinks with the Mac
  **waits for the Mac**. It does not fall back to Claude, and it does not fall back to the NAS's
  model.
- The model is **loaded only while the Mac is plugged in and the owner has not switched on "Mac
  busy"**. A turn runs on it **only while the owner is idle** (ten minutes without keyboard or
  mouse, the owner's to change). On battery, or with "Mac busy" on, the model is unloaded.
- **One Mac turn at a time**, across the whole team.
- The "Mac busy" switch is in the console, which is made for a phone, so the owner can flip it
  from anywhere.

## Decision

### A brain: `mac`

A fifth brain, `Brain::Mac` (stable name `mac`, shown as "Mac"): the owner's Mac, through the
node; nothing leaves the house; it costs nothing; it runs only while the Mac is free. It is
offered to every role on the roster's brain picker and the hire form, is unmetered like the
house's own model (price class 0, no share of the pool), and has the **house-local data tier**:
`cloud()` is false, so the same data rules as `local` apply (a Mac turn may be given Confidential
results; the node's Anthropic relay refuses it). Where a rule is about *where the model runs*
(the data tier, what costs nothing) it follows `Brain::house()`, which is true of both; where it
is about the size of the model's context (the briefing budget, the map's size) it does not:
*The Mac's limits*, below. In the engineering wing a lead or a developer may sit on the Mac as
on the house's model. Claude Code, which speaks Anthropic's API alone, is not offered for it.

### The Mac's limits

*Amended 2026-10-19.* The first build gave the Mac the NAS's limits on what a turn is told of
the team's world, because both are house models. They are not alike in the one thing those
limits are about, the context. The bundled runtime serves the NAS's model a window of 8,192
tokens (`scripts/pick-a-model.sh`), of which the briefing's 1,800 tokens of the world is a fifth;
the Mac's helper starts its server at `-c 32768` unless the owner says otherwise
(`ENDORA_MAC_LLAMA_ARGS`), and the owner runs it at 65,536, where the same 1,800 is under 3%. So a
model that reads and writes code, and needs what the team knows about the lab to do it, was
being told the least of anyone.

It is not given Claude's either. A turn on the Mac runs on a laptop the owner is not using, and
every token of its briefing is read there at the start of the turn; a local model also holds a
long briefing less steadily than Claude does (the failure study, 0124). So the limits sit between:

| | The NAS's model | **The Mac** | Claude |
|---|---|---|---|
| The world in a briefing (0122), continuing or fresh | 1,800 tokens | **2,800 tokens** (the CEO's 2,500 stays: its briefings are held short whatever it thinks with) | 2,500 / 4,000 / 3,000 by role |
| The same after a gap, a handover or a recall | 3,000 | **4,000** (the CEO's 3,700) | 3,700 / 5,200 / 4,200 |
| The map's lines (0126) | 1,400 characters | **1,900 characters** | 2,400 |

2,800 is 1,000 tokens (about 55%) above what the NAS's model has, 200 short of what a developer
has on Claude and 1,200 short of a lead's; 1,900 characters is about ten map lines, where the
NAS's 1,400 is seven and Claude's 2,400 is twelve. They are a starting point, not a
measurement. Every run keeps the size of its briefing against its budget and whether the core
alone ran over (`run_briefings`, shown in the replay), so a limit that binds on most Mac turns, or
is never reached, shows there, and is tuned from it. The rules are pure functions, one place each
(`situations::budget`, `map::chars_for`), and a model that is neither the NAS's nor the Mac's has
its role's.

### Who knows what

```
Mac helper ── heartbeat ──> node ── POST /v1/mac ──> HQ ── start_queued ──> runner ──> worker
 (launchd,    every 30 s    decides      on change,     holds a Mac turn            │
  pmset,      {plugged,     "free?"      and every      queued until the Mac        │ chat completions
  ioreg)       idle_s,      from the     minute         is free, one at a time      ▼
               model_up}    heartbeat                                       node's local relay
        <── { busy } ──     and the                                           ── brain = mac ──>
                            switch                                         the Mac's llama-server
```

- **The Mac helper** (`scripts/mac-brain/`, a POSIX shell script run by a LaunchAgent every 30
  seconds, installed without sudo by `make mac-brain-install`) reads the power source (`pmset`)
  and the owner's idle time (`ioreg`, `HIDIdleTime`), tells the node (`POST /v1/mac/heartbeat`,
  `{plugged, idle_s, model_up}`), and obeys the node's reply `{busy}`: it keeps `llama-server`
  loaded while it is plugged in and not busy, and stops it otherwise. It starts and stops only the
  server it started itself (its pid is kept, and checked to be a `llama-server` before it is
  signalled) and touches no other process. The server listens on the LAN and asks for a key it
  reads from a file. With the node out of reach it keeps the last busy flag it heard (not busy if
  it never heard one): power is local knowledge, so unplugging still unloads. Its log is
  `~/Library/Logs/endora-mac-brain.log`, rotated by size.
- **The node** keeps the helper's last heartbeat in memory and decides: the Mac is **free** when
  it was heard within 90 seconds, "Mac busy" is off, it is plugged in, its model is up, and the
  owner has been idle for the threshold. The reason, when it is not, is one of `not heard from`,
  `busy`, `on battery`, `model loading`, `owner active` (in that order), else `ready`. It
  answers `GET /v1/mac` for the console and tells HQ (`POST /v1/mac`, signed with HQ's token,
  `{ready, why}`) when the answer changes, when the owner flips the switch, and every minute
  otherwise.
- **HQ** keeps what the node last said in memory and trusts it for **150 seconds**
  (`domain/mac.rs`). It never guesses: a new HQ, or a node gone quiet, is "the Mac is away".
- **The relay** (`/team-relay/local/v1/chat/completions`) already asks HQ whose turn a token is.
  HQ now says `brain: "mac"`; the relay then sends the call to the Mac's address with the Mac's
  key and model name, and to nothing else, and only if the Mac is free *at that call*. If it is
  not, the call is answered `503` with the reason and "the turn waits and is tried again when it
  is": the worker ends the turn, and HQ tries it again later. There is no path from a Mac turn to
  another model: the Mac is not in the Everyday list `local_target` reads, and the Anthropic
  relay refuses a turn that is not cloud.

### The queue

The existing wake machinery decides *whether* a member is woken (the kill switch, a pause,
furlough, a project paused, duplicates). A wake for a member on the Mac then queues a turn as
any other does, and the turn **waits where it is**: `start_queued`, the one place that starts
turns, will not start a Mac member's turn unless `mac::wait_for` says nothing holds it, and then
only if no other Mac turn runs. The turn stays `queued` with its `why` set to *waiting for the
Mac: owner active* (or `on battery`, `busy`, `another turn is using the Mac`, `the node has not
said yet`, `the node has not said lately`), which the HQ screen shows, and `snapshot.mac` counts
the turns waiting. The task's guards see a live turn, so a waiting turn is never "stuck" and never
nudged. A *held wake* in the sense of the Held receipt (a second member waiting for the task's
live turn) is a different thing and is left alone.

This is deliberate: a held wake is released when a turn on its task ends, and nothing ends when
the owner stops typing, so the Mac's gate would have needed a second release path. Gating the
start has one. It also survives an HQ restart for free, because the queued turn is a row in HQ's
store; only the node's word is lost, and HQ waits for the node to say it again.

- **A Mac turn cut short because the Mac went away** (the owner came back; the laptop left its
  charger) is not held against it: the failure is not counted among the two retries a turn gets,
  and it is tried again when the Mac is free.
- **Downtime turns and the nightly quiz do not use the Mac.** They are not quality work, and the
  Mac is the scarce thing.

### The console

The HQ screen gains a **The Mac** card: whether it is free and why, how many turns wait for it,
the helper's last word (plugged in, idle minutes, model up, last heard), the **Mac busy** switch,
and a *Set up the Mac* section: the model server's address (it must be on the house's network;
the node refuses any other), the model's name, the key (kept sealed; never sent back), the idle
minutes, and a button that makes the helper's token and shows it once.

### The helper's token

A `Scope::Mac` credential in the node's gate: a token the node makes (`POST /v1/mac/token`,
console only), kept sealed, that opens `POST /v1/mac/heartbeat` and **nothing else**. It lives in a
file on a laptop, so it can say how the Mac is and read the switch; it can do no other thing. A
new token replaces the old.

### Settings, and the migration

The Mac's settings (address, key, model name, the switch, the idle minutes, the token) are rows of
the node's existing sealed `capability_settings` table under the id `mac-brain`, written with the
existing idempotent upsert (`INSERT OR REPLACE`). **There is no schema change and no migration**:
nothing is created, nothing existing is altered, and a second start changes nothing. HQ's schema is
unchanged: the new brain is a new value of the existing text column (`brain`), and a database that
holds an older value reads as before. What the node heard from the Mac, and what HQ was told, are
in memory.

## Consequences

- A member on the Mac costs nothing and shares no pool; its work waits for the owner to step away,
  which on a working day may be long. That is the choice: the owner wants quality work from the
  stronger model, not fast work from a weaker one, and flips "Mac busy" on from a phone to keep the
  Mac whole.
- The owner must keep the helper installed, and the model's key and the helper's token in files on
  the Mac. Both are the owner's, in git-ignored `local.mk` and the Mac's home; nothing of them is in
  the repository.
- A Mac turn has the same data tier as the house's own model, so it is the right place for
  Confidential results, and the Mac is a place they now reach: on a laptop, on the LAN, behind a
  key. The address is refused unless it is private.
- The first `ready` after a node restart takes up to a heartbeat (30 s) and a tell (20 s).
- `llama-server`'s `--api-key-file` and `/health` are relied on; an old build lacking them will not
  start the server, and the helper's log says so.

## Amendment (2026-10-25): the owner at the Mac does not hold it

The owner's word: being at the Mac should not hold its model. Every team turn on the Mac waited
for the owner to have been idle ten minutes, and with nearly all of the team's work on the Mac,
the whole team stopped whenever the owner used it. The idle wait now defaults to none (0) and may
be set from 0 to a day; a wait the owner sets still holds as before. On battery and "Mac busy"
still hold the Mac, and one turn runs on it at a time.
