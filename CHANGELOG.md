# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

### Removed

- **Two tools that were never built are gone: `run_gate` and `request_action`**
  ([ADR 0106](docs/adr/0106-team-contracts.md), amended). Every member was offered both on every
  turn since the first build, and every call answered "not available yet". They are no longer
  listed (a developer's list goes from 25 to 23 tools, the tester's from 24 to 22, a lead's from
  40 to 38), no longer named in what a member is told it can use, and a call to either is a call
  to a tool that is not there. Nothing used them. No migration.

### Changed

- **Compiled Python is no longer tracked.** Four `__pycache__/*.pyc` files had been committed
  under the `endora.homelab` collection's roles, and the repository ignored none. They are
  untracked, `__pycache__/` and `*.pyc` are ignored, and the collection's own `build_ignore`
  leaves them out of a built collection (a build with the files on disk holds none).
- **A guard that cannot tell says so** ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md),
  amended). **Talk against work:** a turn that ended before turns were labelled has no label, and
  was counted as nothing; such turns are now counted apart ("1 turn it cannot read"), and a window
  that is over only if they talked, or under only if they changed the product, is neither: no
  finding is opened, none that stands is cleared, and the Guards card says it cannot tell.
  **The CEO's stepping in:** it was counted over the whole event feed, which the node prunes to
  its newest 20,000, and a count over a pruned feed was shown as if whole; it is now counted over
  the last week, and when the feed no longer reaches back that far the roster says HQ cannot tell,
  and why. No migration. Scenario: `domains/team/features/guards.feature`.
- **Two holes in the closure contract are shut** ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md),
  amended). **Work closed as done, superseded, must name the task that carries it on:** it was
  meant to (the tool says so) and was not made to; it is refused with no successor, with the
  task itself, or with a task that is not there, and refused before anything moves. **A task is
  not made to wait on work already done or dropped:** a wait on done work has nothing to wait
  for, and a wait on dropped work was stuck by construction; the refusal says to leave it out, or
  to wait on the task that carries the work on. Closings already recorded are read as they were.
  No migration. Scenarios: `domains/team/features/guards.feature` and
  `the-engineering-wing.feature`.
- **What one member writes reaches another clean** ([ADR 0129](docs/adr/0129-the-teams-history-and-asking-the-past.md),
  amended). The first build cleaned only a consult's question and what the record returns; a
  thread post, a world line and a desk card went through raw. **A thread post is kept as written**
  (a mention in it still wakes whoever it names), **and what another member is told of it, in its
  briefing and in `my_turn`, is one line under its author's name** with control, direction
  (bidi), zero-width and separator characters replaced and an `@` that would wake a member
  written full-width; Endora's and the owner's posts are as written. The same for a question asked
  of a member, and the thread lines on a card. **The one-line helpers every stored line goes
  through now do it too** (the world's headings, headlines, warrants and story parts, a decision
  for the board, a lead's reasons, a criterion's text, a huddle view), and **a question or an
  approval on the owner's desk and a section's body are scrubbed**: the same characters replaced,
  their line breaks kept. No migration. Scenario:
  `domains/team/features/the-history.feature`.
- **A tool only one kind of turn can use is offered in that turn alone**
  ([ADR 0131](docs/adr/0131-the-two-kinds-of-turn.md), amended). `answer_quiz` (the nightly
  quiz on a member's map) and `answer_consult` (a consulted member's answer) were offered to
  every member in every turn, among 27 tools for a developer, where how many tools a small model
  is offered is a measured failure mode. HQ's `tools/list` is now asked of the turn: the quiz's
  answer is offered in the quiz's turn until it is answered, the consult's in a consulted turn,
  and neither anywhere else (a role's list is two shorter). A downtime briefing names the quiz's
  tool only when its turn carries a quiz. Calling either elsewhere is refused by the engine as
  ever. No migration.
- **The Mac has limits of its own** ([ADR 0130](docs/adr/0130-the-macs-brain.md), amended;
  [0131](docs/adr/0131-the-two-kinds-of-turn.md)). The first build gave the Mac the NAS's small
  model's limits on what a turn is told of the team's world (a briefing's 1,800 tokens of it, a
  map's 1,400 characters) because both are house models, though the Mac runs a large context.
  **A turn on the Mac is now told up to 2,800 tokens of the world (the CEO's 2,500 stays) and a
  map of up to 1,900 characters**, between the NAS's model's and Claude's (a lead's 4,000 and
  2,400); after a gap, a handover or a recall, 1,200 more as for any model. The NAS's model keeps
  1,800 and 1,400. The numbers are a starting point, tuned from the size each run records against
  its budget; the reasoning is in ADR 0130. No migration. Scenarios:
  `domains/team/features/the-macs-brain.feature`.
- **Work leaves an area only by its lead's hand** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md),
  amended). Live, the CEO handed an engineering branch task to the Ops lead; the CEO may hand
  work to a lead only, and the Ops lead is one. `hand_to` now refuses a member outside the task's
  area ("Ada is outside Kit's area: only Kit, who leads it, or the owner hands its work out of
  it") unless the caller is that area's lead. Handing a task up to its area's own lead is not
  leaving the area and stays; the CEO's way to have work moved is `direct`. No migration.
  Scenario: `domains/team/features/the-engineering-wing.feature`.
- **A deploy under a turn is the platform's interruption, not the member's failure**
  ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md), amended). Live, a deploy
  cut a developer's turn short; HQ read its task as stopped by accident, the lead was asked to
  look, and the CEO handed the work to another area's lead. HQ now tells whose it was that a turn
  did not finish: **the platform's** when HQ lost the turn (it restarted), the runner found its
  container gone, or it was killed from outside before its full time (exit 137 or 143); **the
  worker's** for every other exit and for a kill at the turn's full time. A turn the platform
  cut short is tried again a minute later as the same member, **the first interruption does not
  count against its retries** (four starts, not three), and nothing is found, nudged, climbed or
  put on a desk while it waits; its reason says `interrupted: …`. HQ reads the runner's report
  as it always was (an exit code, or its words with none for a container gone): **the runner
  and the worker are unchanged and need no redeploy.** Migration (idempotent): one column,
  `runs.interrupted`, added if missing, at none for every old run. Scenarios:
  `domains/team/features/a-deploy-under-a-turn.feature`.
- **What done means comes first** ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md),
  amended; [0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)). Live, the
  owner's "no literal passwords in role defaults" went to a lead with no definition of done and
  split into three blocked sub-tasks, each defensible and none tied to what was asked. **The
  owner's work now says what done means:** Give work, `POST /v1/tasks` and a project's
  hand-it-work button need at least one criterion, and are refused with the way to give one
  (nothing is made, nobody is woken); the form asks "What does done mean? (at least one line)"
  and keeps what was typed. **A routine's task carries criteria from its routine:** the home
  lab's two live ones as ever; for any other routine, *a reading of "its title" from this run,
  cited*, so closing its task takes a reading; none for a lead's report and patterns or the
  CEO's review and planning, which are closed by being filed or decided. **A lead's split names
  what each part serves everywhere:** the depth-0 exemption of the owner's and a routine's tasks
  is gone, and a task with no criteria (work already open, and the plan HQ hands a project's
  lead) has none to name. **Work already open with no criteria is untouched**: no migration, no
  row changes, and its lead may give it some with `narrow_criteria`. Scenarios:
  `domains/team/features/what-done-means-comes-first.feature`.
- **The tools fit the turn** ([ADR 0131](docs/adr/0131-the-two-kinds-of-turn.md) and
  [0108](docs/adr/0108-the-bridge.md), amended). Live, a developer on the house's own small
  model on the NAS asked the owner to approve a web map search with coordinates it had made up,
  "to find the NAS backup location", and the request reached the owner's desk as an approval.
  **What the bridge offers a turn depends on where it thinks:** a turn on the NAS's model (or one
  HQ says nothing of) is kept from the tools that can end on the owner's desk (an act that asks
  the owner, which is every tool of a tool server the owner has not reviewed, web and search tools
  among them, or that takes arguments and so asks once a stranger's words are read), and is
  refused one it names. It keeps the lab's contracts and what the owner trusts to run without
  asking: every read, the download queues the Ops lead checks the lab with among them, and an act
  that runs on its own and takes nothing. The owner's Mac and Claude are offered as their role and
  the data tier allow, as before.
  **A request for the owner's yes says what it serves:** a bridged act that asks the owner names
  which of its task's criteria it serves (`serves`); HQ refuses one that names none, or sits on a
  task with no criteria, with the reason (400), the member's turn goes on, and nothing reaches the
  desk. The owner's card says which criterion it serves, as it was asked. HQ's settings gain
  `approval_serves:<desk item>`; no migration. **Deploy together:** a node from before asks HQ for
  an approval without `serves`, and HQ now refuses it; approvals already waiting are decided as
  ever. Scenarios: `domains/capabilities/features/tools-that-fit-the-turn.feature` and
  `domains/team/features/a-request-says-what-it-serves.feature`.
- **A desk that interrupts only for decisions** ([ADR 0132](docs/adr/0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md)).
  Over 17 and 18 October thirty items reached the owner's desk in 38.6 hours and six were real
  decisions or approvals; the rest were stuck work, a guard's finding or a notice. Those three
  kinds no longer become a card each: they wait in **one "While you were away" card**, the first
  on the Desk tab when it has anything in it, with a count, newest first, each keeping its own
  choices on the same endpoints (try again or drop; real, false positive, can't tell or
  resolved; seen). Acting on one takes it out, and the owner's verdict on a finding from the
  Guards card puts its card away too. Every other kind (a decision, an approval, a question, a
  push, a project's cap or budget, a member out for the day, a recall, canon, a goal, a hire, a
  furlough, work sent back three times, a write-up to read) interrupts as before, and so does a
  kind nobody has classified. The snapshot's `desk`, the "N waiting on you" chip and the Desk
  badge now hold only what interrupts; the rest is the snapshot's new `digest`. **One finding
  for a guard and what it is about:** a new detection refreshes the finding that is open (its
  evidence and time, and the words on its card), opens a cleared one again as itself, and leaves
  a judged one alone until the evidence has grown by a step past what was judged (a new episode
  after the work has moved, for stuck work; another dozen turns of talk, for the ceremony). The
  Guards card says when a finding was seen again and how often it was found. Migration
  (idempotent): three columns added to `findings` if missing (`seen_at`, `episodes`, `mark`),
  no row changed. Scenarios: `domains/team/features/a-desk-that-interrupts-only-for-decisions.feature`.
- **The HQ screen is tidied into tabs** (console only, no API change). Desk (what is looked at
  and what waits on you), Team (roster, hiring, how the team is doing), Work (projects, goal,
  activity, guards, routines), Knowledge (the world, house log, notes) and Office (games,
  contracts); the tab is remembered per viewer, and Desk and Work carry badge counts. "Give
  work" is a form (what to do, what done means, which lead, Engineering's by default) instead
  of two pop-ups, with chips for today's pool, the Mac and what waits on you. Every desk item
  says in a line what its choices do; the World card has a visible Paste Markdown button and a
  legend for its stages and buttons; the activity log starts at ten lines. On a phone the
  office canvas is a thin strip with a "Show the office" toggle, so the desk is first on screen.
  The Mac's setup (helper token, model address, key and name, idle minutes) moves to
  Settings, "The Mac's brain"; HQ keeps the Mac's state, a chip, and the busy switch on the
  Team tab, with a "Set up in Settings" link.

### Added

- **The check after a deploy** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md),
  amended). Once the node pushes a branch, the owner's desk asks whether it is deployed.
  Deployed hands the member of the routine that takes the home lab's alarms one turn to read the
  lab, in that routine's project, with one live criterion. That turn is narrowed in code: HQ
  offers four of its tools; the node offers and runs `homelab_status`, `container_logs` and
  `nas_status` alone and refuses everything else before it runs; and the runner starts it with no
  shell. Its one verdict, "Post-deploy infrastructure check: Healthy" (or Unhealthy, or
  Inconclusive), goes to the change's thread: Healthy only on a reading of the turn's own, cited,
  and said to check the infrastructure, not the change itself; otherwise Unhealthy or
  Inconclusive, which closes the check and puts an interrupting
  card on the desk. A second click, or a later wake, starts nothing. Won't deploy closes the
  card with no check.
- **A drain before every restart** ([ADR 0134](docs/adr/0134-a-drain-before-every-restart.md)).
  A deploy, and the NAS's 04:00 security reboot, cut turns of ten or fifteen minutes in the middle
  of a step, and the work was lost with them. A restart is now announced first. **While a drain
  lasts** HQ starts no new turn (queued work waits and says "waiting for a restart"), every
  answer a running member gets ends with "A restart is coming: finish this step, leave a handover
  note, and end your turn.", and `post` takes an optional `handover` note that ends the turn and
  keeps the task: the note opens the member's briefing when the turn goes on, and it costs the
  member nothing. **Routes** (HQ's own token, not proxied by the node): `POST /v1/team/drain
  {cap_seconds}`, `GET /v1/team/drain`, `POST /v1/team/resume`; the command is `endora-hq drain
  [--cap SECONDS] [--wait]` and `endora-hq resume`, run in HQ's container. **`make deploy` and
  `make deploy-pi`** run it before the restart (`DRAIN_CAP`, default 300 seconds; `NO_DRAIN=1`
  skips it) and lift it after; HQ starting again lifts it too, and one nobody ended lifts itself
  after 30 minutes. **The owner** sees one Activity line, and, only if a restart cut turns, one
  item in the digest: "A restart cut N turns: ...". **The NAS's reboot:** the `endora` role can
  install a systemd unit that runs the drain as the host shuts down, before Docker stops
  (`endora_drain_on_shutdown`, off by default; `endora_drain_cap_seconds`). Migration: none (two
  rows in HQ's settings). Scenarios: `domains/team/features/a-drain-before-a-restart.feature`.
- **Caps on talk and runaway** ([ADR 0133](docs/adr/0133-caps-on-talk-and-runaway.md)). On 17
  and 18 October one docs branch took 44 turns, 80 posts and $2.47, 28% of all the spend, and the
  guard that noticed it stopped nothing; the house's own models have no budget to stop a runaway
  (the scripted demo ran 1,539 turns in 3 h 40 min); and only the owner could stop the team.
  **The ceremony guard sees all work:** work outside a project is looked at by its task tree
  (what its lead can narrow, split or drop), and every finding, and the Guards card, say what the
  window's turns spent. **A coordination budget per task:** more than 20 coordination turns on
  one task in 24 h with nothing changing, counted from its newest product change or its lead's
  last decision. In **shadow** (the default) it records *would have held coordination on #N
  (budget 20, 27 turns, $X)* on the Guards card and in the digest and changes nothing; switched
  **on** from a toggle on the card (`POST /v1/hq/guards/{guard}/mode` through the node, audited;
  a guard that only records has no mode), the wakes that only ask for talk are held, not lost,
  and the task's lead is asked once to narrow it, split it or drop it. **`narrow_criteria {task,
  criteria}`:** the lead of a task's area may narrow or reword what done means for it (no more
  criteria than it had), posted in the thread and recorded (`task.criteria`); a tester's pass on
  the old ones needs checking again. **A daily cap on the house's own models:** a member takes at
  most 60 turns a day on the node's model or the Mac, and the 61st waits for tomorrow, shown in
  the top bar, the outcomes and the roster. **A runaway tripwire:** 40 turns started by the team
  in 10 minutes stop the team itself (*stopped by the runaway tripwire*; the forty-first never
  starts), with one card on the owner's desk saying who started how many; the owner resumes as
  ever. Both caps are on from the start, and the scripted demo is exempt from both. No schema
  change and no migration (new settings keys only); rolling back past it needs the
  `coordination` findings deleted first. Scenarios:
  `domains/team/features/caps-on-talk-and-runaway.feature` and `guards.feature`.
- **Outcomes beside activity** ([ADR 0132](docs/adr/0132-outcomes-and-a-desk-that-interrupts-only-for-decisions.md)).
  Over 17 and 18 October, 201 turns and $8.85 delivered no change the owner accepted, and nothing
  on the HQ screen said so. The Team tab's "How the team is doing" card now opens with
  **Outcomes** over the last day and the last week, and the top bar says **N landed this week**
  (`snapshot.scorecard.outcomes`). **Landed**: tasks whose branch the node pushed to the fork on
  the owner's approval, each once (an approval not yet pushed, and a push that failed, are not
  landed), with the branches submitted and the checks passed. **Cost of one**: the window's
  spend over what landed, "none landed" when nothing did, never $0. **Talk : work**: the ceremony
  guard's own labels over the same turns, so the card and the guard agree, with the posts written
  and the acts that succeeded (kept by the five-minute process-health look). **Desk**:
  interruptions a day, and what went to the digest instead. **Waiting now**: turns waiting for
  budget and for the Mac, and the oldest wait. **Memory**: work sent back as `exists_already`,
  searches of the record, and consults asked, answered and answered from the record. A tester or
  a lead may name `reason: exists_already` on a send-back; every send-back (`task.sent_back`)
  and every `ask_record` (`record.asked`) is told in the feed. `consults_shown`, which nothing
  called, is removed. No schema change. Scenarios:
  `domains/team/features/what-the-work-came-to.feature`.
- **Two brains per member, and two kinds of turn** ([ADR 0131](docs/adr/0131-the-two-kinds-of-turn.md)).
  Every member has a **work brain** (the Mac or the house's model) and a **judgment brain** (a
  Claude its role may use: Haiku until the owner says otherwise); the roster shows and sets both
  (*Work: Mac · Judgment: Sonnet*; `POST /v1/members/{id}/work-brain` and `/judgment-brain`,
  through the node), and the hire form takes *works with* and *judges with*. What a turn is, and
  so where it runs, is decided by HQ in code from why it was asked for (a table in
  `domain/turns.rs`, with a test that fails until a new wake reason is classified), never by the
  member: **judgment** (planning, reviews, deciding, accepting, a tester's verdict, a huddle, an
  alarm; all of the CEO's) runs only on the member's Claude and **waits** for budget; **work**
  costs nothing and runs on the Mac when it is free and otherwise on the NAS's model, except
  code and branch work, the engineering wing lead's planning and a tester's work, which wait for
  the Mac and are never given the NAS's. **An alarm acts at once:** with the member's or the
  pool's budget spent it runs immediately on the member's work brain (the Mac if free, else the
  NAS), and a notice on the owner's desk says so; no other judgment ever falls back. A wake out
  of budget is no longer thrown away: its turn is kept, waits for budget, one per member and
  task, and is released when the pool's day turns, the owner tops up the pool or gives the member
  an allowance; it survives an HQ restart. A turn that waits for budget or for the Mac is held,
  never stuck: it produces no stuck finding, nudge or climb to the CEO. The top bar says
  *N turns wait for budget*; the roster says what a member's turn waits for. Migration
  (idempotent, two nullable columns): a member on the house's model or the Mac keeps it as its
  work brain, a member on Claude works with the Mac, and a member with no Claude to judge with is
  given Haiku (the CEO, Sonnet). There is no switch: the service runs the rules the tests run.
  Downtime (0119) is work on the NAS and free for every member, so the owner's paid-downtime
  allowance, its card line and its route are gone.
- **The collection can declare every NAS-side setting** (`endora.homelab`, `endora` role).
  New variables, all empty by default (nothing rendered, nothing changed): `endora_workers`
  (the `workers` profile), `endora_lan_dns`, `endora_hq_tick_seconds`, `endora_hq_dir`,
  `endora_sound_dir`, `endora_utc_offset_minutes`, `endora_router_model`, `endora_synth_model`,
  `endora_ansible_nas_repo`/`_ref`, `endora_lab_retired`, `endora_git_push_key_dir` and
  `endora_bench_token_dir`/`endora_bench_cap_cents`. `scripts/check-collection-parity.sh` (in CI)
  fails when `docker-compose.yml` reads a variable or profile the role does not declare.
- **The Mac's brain** ([ADR 0130](docs/adr/0130-the-macs-brain.md)). A team member may think
  with the owner's Mac: a strong local model on a laptop the owner also works on. A fifth brain,
  `mac`, on the roster and the hire form: free, house-local (the same data rules as the house's
  own model, and Claude is never reached), and it **waits for the Mac** rather than falling back
  to another model. A turn on it runs only while the Mac is plugged in, "Mac busy" is off and
  the owner has been idle (ten minutes unless set), and one at a time across the team; the
  turn says *waiting for the Mac: owner active* (or on battery, busy, ...) until then, and a
  turn the Mac left mid-way is not counted as a failure. The node decides from a heartbeat
  (`POST /v1/mac/heartbeat`, under a token of its own that opens that route alone), tells HQ on
  change (`POST /v1/mac` on HQ), sends a Mac turn's calls to the Mac's address with its key
  (sealed at rest, never shown) and a clear 503 when it is not free; `GET /v1/mac` and a **The
  Mac** card on the HQ screen show the state and reason and hold the **Mac busy** switch, the
  address, key, model name and idle time, and make the helper's token. The helper
  (`scripts/mac-brain/`, a shell script under a LaunchAgent every 30 s;
  `make mac-brain-install` / `mac-brain-uninstall`, no sudo) reads `pmset` and `ioreg`, reports
  to the node, and loads or unloads `llama-server` (touching no other process). Tests:
  `scripts/test-mac-brain.sh` (in CI, with stubs) and the owner's scenarios in
  `domains/team/features/the-macs-brain*.feature`. No schema change: the settings are rows of the
  existing sealed settings table. Set-up: `docs/homelab.md`, "The Mac's brain". Not yet tried on a
  real Mac.

- **Live mode for the sound hub's phone and browser listeners** ([ADR 0104](docs/adr/0104-endora-sound.md)).
  The hub serves each stream speaker's mix as raw PCM in 20 ms frames (`/v1/live/{speaker}`;
  `/v1/sound/live/{id}?ticket=` through the node, under the listen ticket), and the console
  plays it in an AudioWorklet with an adaptive jitter buffer (300 ms, growing to 1 s when
  frames are late or missing, shrinking slowly), so a phone is a fraction of a second behind
  instead of seconds. The speaker card gains **Auto / Live / Listening**: Auto is live when an
  AirPlay door is wired to the speaker; the page hands over to the MP3 when it goes to the
  background (it plays with the screen locked) and back on return. **Measure delay** has the hub
  put a tone in the speaker's mix and reports the hub-to-page delay for each mode, and the
  sound that waits in the hub before the mix.

- **An AirPlay 2 door** ([ADR 0128](docs/adr/0128-an-airplay-2-door.md)), so an Apple TV
  casting into "Endora Mixer" keeps lip-sync. shairport-sync 5.6 built for AirPlay 2 with
  nqptp 1.2.9 (Dockerfile target `airplay2`, image `endora-airplay2`), in its own container on
  a LAN address of its own through a macvlan network (its clock needs UDP 319/320, which the
  senders' airptpd holds on the host); it reads the hub's list of doors and writes the same
  pipe in the hub's format, and its own avahi announces it. `SOUND_AIRPLAY2 = 1` in `local.mk`
  turns it on (`make deploy` adds the `airplay2` profile, builds the image, and refuses without
  the LAN interface, subnet, gateway and address), and the senders' container then runs no
  classic door; anything else keeps the classic door as before. `SOUND_AIRPLAY2_LATENCY`
  (0.5 s by default, 0 to 2) hands the sound over that much early to make up for the hub's path,
  tuned by ear. The `endora.homelab` collection's `endora` role gained `endora_sound`,
  `endora_sound_senders` and `endora_sound_airplay2` with the door's values. Setup, UniFi and
  tuning: `docs/homelab.md`. Not yet tried with a real Apple TV.

- **One general AirPlay door, "Endora Mixer"** ([ADR 0104](docs/adr/0104-endora-sound.md)).
  The sound hub makes it once at start; any device (an Apple TV, a phone) casts into it and
  it can be wired to any speaker, phones included. Its card says so, and that a device wired
  to it hears what it casts in. Speaker cards gained a **Delete** button.

- **OpenBao, unlocked with a passkey** ([ADR 0127](docs/adr/0127-openbao-and-the-passkey-unlock.md)).
  The house's secrets manager runs beside the node from the `endora.homelab` collection's new
  **`openbao` role** (pinned by digest, raft storage and a file audit log in its own folder,
  256 MB, read-only, no capabilities, on the internal `endora_vault` network it shares with the
  node only, no host port); the `endora` role's `endora_vault_url` (`ENDORA_VAULT_URL`) gives
  the node its address. **Settings › Vault**: register passkeys (WebAuthn with the PRF
  extension, bound to `endora.home.arpa`; at most five), **Initialise** with one tap (three
  Shamir shares, two to open, sealed under an unseal key wrapped for each passkey under a key
  derived from its PRF output; setup with the root token, which is then revoked; the shares
  shown **once** as the owner's break-glass copy), give another passkey its own copy, and
  remove one. After a reboot, **Vault locked → Unlock**: one Face ID tap unseals it. The node's
  own data keys can move into the vault's Transit key (a one-way switch); a node started while
  the vault is sealed then **waits** — it serves the unlock, and every stored credential says
  the vault is locked — rather than failing. Every passkey lost, two break-glass shares and a
  new passkey set unlock up again and replace the shares. Opened by an address, the screen says
  to open `https://endora.home.arpa:8787`.
- **The team's history, and asking the past** ([ADR 0129](docs/adr/0129-the-teams-history-and-asking-the-past.md)).
  Every turn's conversation is told by its worker to the node at the turn's end, scrubbed with
  the receipts' scrubber, held to a megabyte (head and tail, the cut marked) and kept by HQ for
  **30 days and the last 5 per member and task**, marked with the tier of what the turn read;
  then only handover notes and posts remain. The owner's replay shows what the model said. Two
  tools for every member: `ask_record`, a deterministic search of the last 90 days of posts,
  handover notes, pages, stories, notebooks and conversations (FTS5 where it can, no new
  dependency; five excerpts of 400 characters, newest first, each with what to cite, as
  testimony, and never a Confidential turn's to a member on a cloud model), which wakes no one;
  and `consult` / `answer_consult`, a direct question that takes the colleague's one short turn
  (6 rounds, read tools and the answer), is answered in the asker's thread and wakes the asker,
  at most 2 a turn and 6 a day, the CEO consulting leads only, paid from the asker's share, and
  answered from the record for a member who is paused or on furlough, who is never woken. What
  a member writes that lands in another's briefing is cleaned and prefixed. New tables
  (`transcripts`, `consults`) and an FTS5 index are made if missing; no row changes. Resuming a
  Claude Code session is a later step.

- **The team's map** ([ADR 0126](docs/adr/0126-the-teams-map.md)). The world gains a page for
  every lead's area, every project and goal not given up, and every role
  (`world:area/3`, `world:project/2`, `world:role/developer`), made once each from the team as it
  stands and as it grows. Each page's **steward is worked out from the data** (a lead its area,
  a project's lead its project, the CEO the house and goals, the owner a role); only the steward
  or the owner writes it, and anyone else **proposes** (`propose_world`) and the steward accepts
  or declines with a reason (`decide_proposal`; the owner too, from the World card). **Every
  briefing opens with a short map** of the pages that matter to its member, by address, within a
  size and naming what it left off. A member that looked and found nothing reports a **gap**
  (`report_gap`), one item for its lead and never the owner's desk, apart from the **broken
  roads** HQ finds itself (a cited address that does not resolve). Each night a free member on
  the house's model is asked a few questions made from its own map in a downtime turn, and **code
  scores the answers**; a night with nobody free is recorded as skipped, and a page that fails
  three nights running is flagged to its steward. A Claude Code turn gets its map back after it
  compacts. The World card shows the pages, proposals (Accept, Decline), gaps, broken roads and
  last night's check; `POST /v1/hq/world/proposals/decide` through the node and its audit log.
- **The owner's one-day top-up of the money pool** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md)
  rule 6). `POST /v1/hq/pool/top-up {amount_micros}` (through the node and its audit log) adds
  to today's pool only: it is gone when the owner's next day begins, and a day's top-ups are
  at most three times the base pool day. Every member's share, and the borrowing cap with it,
  grows in proportion, so a member out for the day can work again. The scorecard shows "pool
  $2.33 + $2.33 top-up" with a "Top up today" button, and an "is out for today" card offers
  "Top up the pool first" when the pool is spent. HQ records a `pool.topped_up` event.
- **The owner writes the world** ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)).
  `POST /v1/hq/world/section {page, key, title, text}` writes or replaces a section as the
  owner (an insight at least, canon if it was, no evidence asked, every revision kept) and
  `POST /v1/hq/world/timeline {page, at?, text}` adds a dated entry; both through the node and
  its audit log, with a "Write as the owner" form on the World card. The owner's sections come
  first in every member's briefing, and a member cannot overwrite one.
- **Guards against how agent teams fail** ([ADR 0124](docs/adr/0124-guards-against-how-agent-teams-fail.md)),
  proposed. A read-time **stopped diagnosis** (working, parked, held, indeterminate) now decides
  every stuck step: only parked work is nudged and climbs the ladder, held work waits for its
  check-back or answer, and work HQ cannot tell about gets one look from its lead. Recovery
  wakes are recorded under what HQ saw before they are sent and never sent twice unchanged; a
  stuck desk item is put up once and closes itself. **Done and dropped carry a reason** (met
  criteria, superseded, not needed; obsolete, duplicate of #N, won't do, blocked for good) and an
  optional successor; work waiting on a dropped task stays blocked and its holder is told at
  once. **Process-health detectors**: per project over 24 h and 7 d, coordination turns against
  product change (12 and 4:1 make a desk notice and wake the project's lead); a task sent back
  three times with no new evidence goes straight to a huddle; a member woken three times for
  nothing is not woken for nothing again. **`create_task`** refuses a sixth open sub-task, a
  fourth level, and a sub-task of a member's task that does not say which criterion it
  `serves`. **Evidence already enough** is said in the briefing, re-checking it is talk only,
  and on the third HQ submits it. **Retract and sweep**: "Not true" on a note, a section or a
  story keeps its fingerprint (word pairs, not exact strings), lists what else says it to forget
  at once, and refuses it written again; a note citing `n12` is built on it. **Fence pushes**
  (refused acts again and again) are a desk notice, and every briefing carries its goal's "not
  now". **The same ask once**: a question or board decision alike to one already waiting from
  another task joins that card, and the owner's one answer reaches each task. Every finding
  takes a **verdict** (real, false positive, not enough, resolved; did the fix work) from the
  owner or the lead (`judge_finding`), and a new Guards card shows each guard's precision, every
  threshold (one table, `domain::guards::THRESHOLDS`) and each project's talk against its work.
  Also: a tester's pass is bound to the criteria it judged ("needs re-check" once they change);
  the roster counts how often the CEO stepped into each member's work; a home-lab read that
  timed out is `unverified`, never `unavailable` (no alarm); HQ's `/health` says the tick's age.
  New tables, made if missing: `nudges`, `findings`, `closures`, `retractions`, `note_parents`,
  `desk_attached`. New owner routes through the node: `POST /v1/hq/guards/findings/{id}/verdict`,
  `POST /v1/hq/retractions`, `GET /v1/hq/retractions/{id}/sweep`,
  `POST /v1/hq/retractions/{id}/forget`. Scenarios in `domains/team/features/guards.feature`.

- **Games in the office** ([ADR 0121](docs/adr/0121-games-in-the-office.md)). Ping-pong and
  pool keep score, for nothing: when two members are free HQ's engine plays the match itself,
  point by point or shot by shot, from a seeded generator and each member's hidden, practised
  skill (ping-pong to 11 and two clear; simplified eight-ball, where scratching on the eight
  loses), at most three matches an hour a game and never a model turn. Every match is kept in
  a new `games` table, said in the activity feed ("Bo beat Dee 11–8 at ping-pong"), and counted
  in leaderboards with wins, losses and the run each player is on. The office gains a pool
  table and a chess table, and the two players of a match walk to theirs. **Chess is real**: a
  `chess_games` table, a team tool `chess` for every role (`board`, `move`, `challenge`), legal
  moves and endings by the rules (mate, stalemate, insufficient material, threefold repetition,
  the fifty-move rule), played one move a downtime turn, one game each, and adjourned as a draw
  after a week; a downtime briefing says "Your move against Dee in chess". The Office card has
  a Games section (leaderboards, the last five results, the games in play) and tapping the
  chess table opens its board. New dependency: `cozy-chess` (MIT) in `endora-team` for chess
  rules; `shakmaty` was refused because it is GPL-3.0-or-later and Endora is Apache-2.0.
- **A bench for an external agent harness** ([ADR 0123](docs/adr/0123-a-bench-for-an-external-agent-harness.md)),
  proposed: to measure an outside multi-agent harness against HQ's wing on one real task, inside
  Endora's fence. A **bench credential** on the node's model relay: a token from a file
  (`ENDORA_BENCH_TOKEN_FILE`, mounted from `ENDORA_BENCH_TOKEN`; no file, no bench, read on every
  call) that opens `/team-relay/v1/messages`, its `count_tokens` and `GET /team-relay/bench` and
  nothing else (every other team-listener route refuses it before HQ is asked); Claude Code's
  pass-through rules (ADR 0117) on `claude-haiku-5-5` or `claude-sonnet-5-5` only; a hard cap a
  day (`ENDORA_BENCH_CAP_CENTS`, $2.00 unless set) kept in a new `bench_spend` table, with each
  call's cost set aside before it is sent so seats side by side stay under it; metered to the
  usage log as `bench`, never to HQ. A **bench image** (`--target bench`): Node 22, tmux, the
  harness's package from one pinned commit, and the workbench's lab checks and Claude Code,
  now built once in two shared stages (`lab-checks`, `claude-code`); bypass mode refused.
  `make bench-image` builds it for the Pi. How to enable it is in `docs/homelab.md`.
- **The team's world: stages, stories and situations**
  ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)). What HQ's team
  knows is kept as addressed pieces with a stage, from an observation to canon: the **house page**
  (`world:house`, the CEO's to write with `write_world` on evidence, with a dated append-only
  timeline that `note_house` adds to), the derived `world:lab` and `world:team`, and **stories**
  (a lesson as when it matters, the tempting move, what to do instead and why, under a handle a
  post can cite). An insight needs evidence from two turns or a reading the node witnessed; trust
  is read when read, so an insight or canon past its horizon shows "unconfirmed since <date>";
  **only the owner makes something canon**, from the World card or a `propose_canon` card on the
  desk, and demotes it with a reason the feed and the page say. Every turn starts from a
  **situation** (fresh, continuing, after a gap, handover, recalled), decided from the record,
  and carries the world within its role's **budget**, naming on its last line whatever it left
  out; the run records it and the replay shows it. `hand_to` and a stuck task's hand-on or drop
  need a **handover note** (done, not done, next, watch out), which starts the next holder's
  turn. `tell_story`, `read_world`, `verify_world`, `retire_world` and `propose_canon` join the
  team protocol; `note_lesson` is no longer offered and `rewrite_house` is refused. The HQ screen
  gains a **World card** with Make canon, Demote and a Markdown download
  (`GET /v1/hq/world`, `GET /v1/hq/world/export.md`, `POST /v1/hq/world/promote`,
  `POST /v1/hq/world/demote`, through the node and its audit log). At start, once, the house
  log's lines move into the house page's timeline at their dates and the goals' lessons become
  stories to retell; nothing is deleted, and the old log is read-only. New tables made if
  missing: `world_sections`, `world_revisions`, `world_timeline`, `stories`, `handovers`,
  `run_briefings`. Scenarios in `domains/team/features/the-world.feature`.
- **Roles as an enforced RACI** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md)), the
  owner's product-model org of 2026-10-18. The CEO sets direction and keeps out of the areas'
  work: it reads anything (on a cloud brain the logs and queues the cloud lead reads), is
  offered no act, is never woken by the lab's alarms or recoveries (they wake the area's lead),
  and on a task in a lead's area cannot claim, move, close, accept or send back: it gives the
  lead a direction with `direct`, and the lead decides. It hands work to leads only; leads
  propose projects and the CEO decides them. Each goal has a **strategy** (purpose, success,
  measures, bets, not now), the CEO's to write with `set_strategy` and the owner's on the goal
  card (`POST /v1/hq/goals/{id}/strategy`). Each **area's measures** are on the scorecard
  (outages over 10 minutes, median alarm to closed, repeats within 24 h; branch to push
  approved, first-time tester passes). Each lead **reports up**: "State of my area" daily at
  the owner's 18:00 (`report_area`, kept as a report and never work on the board) and
  "Patterns" weekly. The CEO's review and planning are rebuilt on strategy, reports, measures,
  proposals and the budget split. **Stuck work climbs a ladder** instead of landing on the
  owner's desk: the area's lead decides (`decide_stuck`), then a **huddle** of the lead and up
  to four who touched it gives one view each (`huddle_view`) and the lead decides, then the CEO
  only when the lead says so; work sent back three times goes to a huddle. A project's lead
  closes its own work on its own acceptance. New tables made if missing: `strategies`,
  `area_reports`, `report_tasks`. Scenarios in `domains/team/features/roles.feature`.
- **The open office** ([ADR 0119](docs/adr/0119-the-open-office.md)). HQ's office is an
  open-plan floor: one pod of desks per team, side by side with no walls and labelled as the
  roster labels them, laid out from the snapshot (a hire grows a pod, a third lead makes a third;
  a member on furlough keeps an empty desk with an "away" card); Mara keeps her own office with
  the door open; a kitchen and lounge replace the engineering wing's walled room. People walk
  round desks and furniture and through the doors, visit another team's pod for its work (the
  tester goes to whose task it is), and drift about the office when idle. What stands on the
  floor is **data**: an `office_items` table of plants, lamps, couches and the rest, seeded once,
  drawn by the screen from the snapshot, and moved by the team. Every role has a team tool,
  `office`, to propose up to five changes, vote and look; the placement rules (inside the floor,
  never on a wall, door, desk, walking lane, room or another item; 40 things, 2 fish tanks) are
  pure and checked when a proposal is made and again when it carries (two yes, or six quiet
  hours; dropped on more no, or after 48 hours). The owner can undo the last change, reset the
  office or veto any open proposal from a new Office card. **Downtime**: a member with nothing to
  do is given a turn about no task, once a day, one at a time, to tidy the office or its notebook
  — free on the house's model, and never on Claude unless the owner sets
  `downtime_paid_cents_per_day`, which the pool pays, never a member's share. A downtime turn
  makes no work. The owner's scenarios are in `domains/team/features/the-office.feature`.
- **Claude Code as a developer runtime** ([ADR 0117](docs/adr/0117-claude-code-as-a-developer-runtime.md)).
  A member setting, `runtime`: `worker` (the default) or `claude-code`, offered only to a
  developer on Claude and refused otherwise; the owner sets it with
  `POST /v1/members/{id}/runtime` (through the node's `/v1/hq/members/{id}/runtime`), and HQ
  seeds the pilot once: Rae on Claude Code, Zed on the worker. In the workbench the worker does
  HQ's bookkeeping, checks the fork out, then runs `claude -p` headless with the member's brain
  and effort, `dontAsk`, no prompts, no settings but the image's, one MCP server (the worker
  itself, `endora-worker --mcp`, carrying HQ's tools and the node's contracts) and an explicit
  tool list (files, and Bash for git and the lab's checks); Claude Code reaches Claude only
  through the node's relay with the turn's token as Bearer, never a subscription login. Its
  result is the last word. Its own tools' calls are posted by a hook (`endora-worker --receipt`)
  to the node's new `POST /team-relay/receipt` and kept as **sandbox-reported** receipts, which
  may be cited but never stand for a live reading. The relay passes a Claude Code turn's own
  request shape and betas, still holding it to the member's brain, the turn's money, the
  five-minute cache (1h rewritten) and the fence. The workbench image carries Claude Code
  2.1.296 (digest-pinned, fetched at build time) and its managed settings (no updates,
  telemetry, error reports or nonessential traffic; the receipt hook). The scorecard gives each
  member's finished tasks and their cost, the pilot's measure. Built and run offline in the
  arm64 image against a fake HQ, node and relay; not yet against Anthropic's API or on the Pi.
- **Elastic staffing and notebooks** ([ADR 0118](docs/adr/0118-elastic-staffing-and-notebooks.md)).
  The team grows and shrinks with its work, and nobody is fired. A member on **furlough** is
  never woken, holds no task, is left out of every briefing's team and of the share arithmetic,
  and keeps its record, name and notebook; the owner furloughs or recalls any member but the CEO
  from its panel on the roster. Once a day HQ offers the owner a member with a week of nothing
  to do (furlough or keep), and a lead with a backlog asks for staff with `request_staff` on
  its project's open tasks: a furloughed developer is brought back first, else it becomes the
  hire request, under the wing's seats. The scorecard counts each team's seats, at work and on
  furlough. Each member also keeps a **notebook**: `note` (on evidence from HQ's record, at most
  300 characters), `forget` (with a reason), `read_notebook` and `notebook_checked` for every
  role; the notes lead its briefing (the newest twelve, stale ones marked after 30 days unused,
  a tidy asked after a week; the CEO sees only the count), and the owner reads, adds, edits and
  deletes any member's from its panel. New nullable columns on `members` and a `notes` table,
  added when HQ opens its store. Scenarios in
  `domains/team/features/staffing-and-notebooks.feature`.
- **Replay, on the HQ screen**: tap a run line, or "Replay this task" on a task card, to read a
  turn back in full: why it was woken, what it was told (folded), every call with its receipt
  and excerpt (Confidential ones too, on the owner's screen), every post with what it cited and
  the unsupported mark, its claims, status changes and desk items, and its money and tokens; a
  task's turns step through in order ("run 3 of 6"). Served by HQ as `GET /v1/runs/{id}/replay`
  and `GET /v1/tasks/{id}/replay`, through the node as `/v1/hq/runs/{id}/replay` and
  `/v1/hq/tasks/{id}/replay`, which only a full credential may read. Every event a turn's call
  emits now names its turn (`"run"`), so a turn's own events are found by id; a turn from before
  is matched by its member, task and time, and says so.
- **The team's standards** ([ADR 0116](docs/adr/0116-the-teams-standards.md), proposed).
  [docs/standards.md](docs/standards.md) holds the owner's global engineering standards: the
  base principles (test first, behaviour in the owner's words, SOLID, abstract only when
  needed, domain first, small slices, and the rules Endora already lived by), how the root and
  nested `AGENTS.md` layer on them, and each project at a glance. Endora's and the terminal's
  `AGENTS.md` point to it. HQ embeds it, and the drafts in `docs/drafts/`, in its library, and
  `read_doc` is now every member's; every briefing names the standards once (the CEO's
  `read_doc` description does, its quiet briefing being held short). The wing is told the
  fork's own `AGENTS.md` is the repo's rules, and its lead that the draft
  [docs/drafts/ansible-nas-AGENTS.md](docs/drafts/ansible-nas-AGENTS.md) is the first branch
  to propose.
- **The engineering wing and the git flow** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).
  A second team works on the owner's public ansible-nas fork through branches. The node keeps
  a bare mirror of the fork and offers a lead or a developer two contracts: `repo_checkout`
  hands the sandbox a bundle of the fork's base branch (`ENDORA_ANSIBLE_NAS_REF`, else the
  fork's default branch asked of GitHub once; compose no longer defaults to `master`), which
  the worker unpacks into `/work/repo` with git set up (commits authored Endora HQ, the trailer
  added by a hook, no remote); `repo_submit` takes a bundle of one `hq/task-N-<slug>` branch
  only when it sits on the base, changes at most 200 files and 1 MB, every commit is HQ's and
  nothing of the owner's values (`group_vars/*.yml`, `host_vars`, inventories) is touched. HQ
  writes the branch's diff as a document on the task and queues a model-free **check job** for
  the Pi's runner (the workbench image with ansible-core, ansible-lint 6.14.6 and yamllint
  1.30.0 baked in): syntax check, `ansible-lint --offline`, `yamllint`, five minutes each,
  reported with the job's own token as a receipt and the task's gate; a failing check sends the
  work back. Once the tester passes and the lead accepts, the owner's desk offers the push: on
  Approve the node pushes the stored branch to the fork over SSH with a deploy key
  (`ENDORA_GIT_PUSH_KEY`), `hq/*` only, never force, never main; without a key the card offers
  the bundle to download. HQ opens no pull request and merges nothing. The owner hires on the
  HQ screen (one CEO); a lead may request a hire within the wing's seats (one Sonnet lead, at
  most five Haiku developers), on evidence. HQ seeds the wing once: the goal "Improve the lab",
  the project "ansible-nas clean-up", Kit, Rae and Zed, and Kit's first task. The office gains
  the wing's room; the owner's scenarios in `domains/team/features/the-engineering-wing.feature`.

- **How the team is doing**: a scorecard on the HQ screen, today beside the last seven days by
  the owner's days, computed by code from HQ's own record (`application::scorecard`, served in
  the snapshot): turns and the share that showed nothing (talk only or empty), per member with
  each one's spend; work sent back against the submissions; the cost of a finished task and
  what dropped tasks spent (waste); alarms, the tasks they made, the median time to close one
  and how many stay open; what reached the desk by kind, and what still waits; and the prompt
  cache's share of the paid turns' input. Turns from before labels are counted as unlabelled,
  never guessed, and a window the kept record does not cover says where it starts. The
  owner's scenarios in `domains/team/features/how-the-team-is-doing.feature`.
- **Evidence, acceptance criteria and the tester** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)).
  The node records every call a turn makes of Endora's tools as a **receipt** in HQ, with the
  turn's own token (`POST /v1/team/receipt`; refused for a forged or finished turn), and the model
  reads its id ("receipt r41") to cite. Tasks carry **acceptance criteria** (required for work a
  member hands another; the home lab's checks get two live ones), and **one evidence rule**
  decides every claim from HQ's record: per criterion, references that exist and belong to this
  work, a live one a reading from the same turn. Work goes member → **tester** (the reviewer,
  shown as "Tester", re-checking on her own readings) → lead (`accept`) → done, nobody checking
  its own, with the fallbacks said on the task; three send-backs and the next round is the
  owner's. The CEO's `decide_project`, its decisions for the board and the lead's question up to
  it are refused without evidence; a post citing nothing in a turn with no reading is marked
  unsupported. Each turn ends with a **label** (advanced, blocked, checked, talk only, empty)
  that the back-off reads; two nudges, then a task that shows nothing goes up the chain once; the
  same reading again shows nothing; a check-back on a task blocked on a wait is replaced by the
  wait; and the chain of command may close, accept, send back or hand on (`hand_to`) a
  subordinate's task. The HQ screen shows criteria with their evidence, the tester's pass, run
  labels and unsupported posts. The live store gains its columns and tables by guarded
  `ALTER TABLE`.
- **The owner's scenarios** ([docs/testing.md](docs/testing.md)). Behaviour the owner decided,
  written in the owner's words as `.feature` files and run against the real engine and the
  bridge through their public use cases, with cucumber-rs (a dev-dependency). The pilot has 14:
  one task a problem, coming back is news, the owner's devices are never the team's, a member is
  told its tools are its own, only the CEO reaches the board, a project's cap is the owner's,
  local members work for free, and more from ADRs 0107, 0108, 0111, 0112 and 0113. They run with
  `cargo test --workspace`, so the gate runs them.
- **Projects, budgets and reports_to** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md)).
  A project serves one goal and has a lead (a lead or the CEO) and, if the owner gives it one, a
  monthly cap; a task may be in a project of its goal, its sub-tasks stay in it, and a routine's
  tasks go in the routine's. A Claude turn on a capped project reserves no more than is left of
  it (so the relay's per-call limit holds it), and the caps together stay within the pool's
  month. At 80% the owner gets one notice a month and the CEO is told; at 100% the project's
  Claude turns wait (members on the house's model go on) and the desk offers "raise the cap" or
  "wait for next month", which lifts itself when the month turns; the owner's own pause holds
  every turn until resumed. Each member reports to someone (set at hire, backfilled on start, the
  owner's to change, never to itself or in a circle), and a question goes to that manager or
  past a paused one. A task handed back wakes its project's lead. Leads and the CEO propose
  projects (`propose_project`) with task numbers and event ids HQ checks; the CEO decides
  (`decide_project`), the board may overrule, and a cap asked for goes to the owner's desk with
  the CEO's recommendation; the CEO proposes goals (`propose_goal`). A weekly **Planning**
  routine briefs the CEO with patterns: projects and their money, the month's tasks by the
  containers they named, the alarms that came back, proposals waiting, goals with no project.
  On start, the live store gains its new columns by guarded `ALTER TABLE`, and the home lab's
  routines and open work go, once, into an active "Home lab upkeep" project. The HQ screen
  gains a Projects card with its forms, the cap and budget items on the desk, and "reports to"
  on each member's card.

- **The team knows how the lab is built** ([ADR 0112](docs/adr/0112-the-team-knows-how-the-lab-is-built.md)).
  A new read contract, `homelab_map`, says for every container its image, state, restart
  policy, who manages it (an ansible-nas role, Endora, or unknown), what it depends on and what
  depends on it ("gluetun → deluge, nzbget, prowlarr (run in its network)"), from Docker's own
  metadata, names and wiring only; the Docker observer gains one fixed read-only route for it
  (`/containers/<name>/json`, a strict name, a handful of fields passed on). A second,
  `ansible_nas_doc`, reads the lab's ansible-nas docs from the owner's public fork on GitHub
  (`ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF`), only `docs/`, `website/docs/` and role
  READMEs, by an allowlist. Both go to every role on any brain. HQ seeds the CEO's house log
  once, when it is empty, with how the lab is built, and the worker's instructions say it too.
- **Coming back is news too** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)).
  When a container or service that alarmed within the day is running and healthy again, the
  node tells HQ once (`POST /v1/alarms/recovered`), and HQ posts "Recovered: deluge is back up
  at 13:43" in the alarm's open task, waking whoever has it. The worker's rules say an alarm is
  a moment: check `homelab_status` before acting, and close the task if everything recovered.
- **One task a problem** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)). A
  member's new task is refused when an open task under the same goal is already about the same
  home-lab containers, naming it: "#664 already covers this (deluge, nzbget, prowlarr): add to
  it with post in #664, rather than a second task".
- **Cheaper cloud turns** ([ADR 0109's cost notes](docs/adr/0109-a-brain-per-member.md#cost-notes)).
  Each of the worker's rounds on Claude asks for Anthropic's automatic prompt cache, so a round
  reads the round before's prompt at the cache price and writes only what is new; the prefix is
  byte-stable (instructions once, tools by name, the briefing first, rounds appended). HQ names
  each cloud turn's **effort** (CEO `medium`, `high` when it weighs a decision; lead `medium`;
  researcher `low`), passed by the runner as `ENDORA_EFFORT`. The relay passes the ephemeral
  cache and the five efforts and still refuses the rest, and reckons what the turn's last call
  cached at the cache-read price before a call. The **CEO's briefing** carries the whole house
  only on its first turn and daily review, and otherwise what changed since its last turn. A
  run's record keeps its input tokens, and the activity log says "cached: N% of input".
- **HQ's progress and waits, checked against the design it borrows.** A turn that only
  re-claims its own task and leaves the worker's last word is no longer counted as progress,
  so the empty-turn back-off finally holds such turns back (the worker claims before every
  turn, which had made every turn look productive). A task waiting on one that was dropped
  is no longer released as if the wait were done: its member is told why in the thread and
  woken once, then it goes to the desk. The plan no longer promises per-task session resume,
  which was never built.
- **The CEO and the chain of command** ([ADR 0111](docs/adr/0111-the-ceo-and-the-chain-of-command.md),
  owner's decisions of 2026-10-17). The owner is the board. A new top member, the **CEO**, sits
  above the lead: HQ seats one once on an existing team ("Mara", on Claude Opus, renameable) with
  a "Daily review" routine every 24 hours. While a CEO is at work, a member's `ask_owner` goes up
  the chain in the task's thread ("Question for Ada: …"): the developer's, researcher's and
  reviewer's to the lead, the lead's to the CEO; the answer in the thread wakes the asker. Only
  the CEO's `ask_owner` reaches the board's desk, as a decision (`decision`, `recommendation`,
  `why`), checked by the reviewer first; the card offers Approve, Decline and an answer. The CEO
  wakes only for escalations and its own work, keeps a **house log** (`note_house`,
  `rewrite_house`; read by the CEO and the lead every turn, edited by the owner on a new House
  log card), reads Endora's public records and plans (`read_doc`, embedded in HQ at build time),
  and is briefed on its team, goals and lessons, open work, the week's alarms and the board's
  desk. With no CEO, the team works as before.
- **Brains and money by role**: Opus is the CEO's alone (HQ and the relay refuse it to anyone
  else); the CEO's turn may spend $1.00, everyone else's $0.40. The day's pool is shared only
  among members with a paid brain, by their roles' weights (CEO 35, lead 25, developer 40,
  reviewer 20, researcher 15); a member on the house's model costs nothing and is never turned
  away for money. The cloud CEO is given nothing Confidential and no act on the house.
- **HQ's team works as a team.** The lead's briefing names the team (each member's role and
  brain) and says to plan and hand work on: fixes to the developer, look-ups to the researcher,
  the reviewer checking. A cloud turn refused a Confidential step is told to hand it to a member
  on the house's model. Work left in review with no review coming (moved there before the
  reviewer path existed) wakes the reviewer after ten minutes, once.
- **The team's rings, ring 1: a sandbox on the Pi** ([ADR 0110](docs/adr/0110-the-teams-rings.md),
  owner's decisions of 2026-10-17). The node's team listener (8788) is published on the house's
  network, every route on it needing a token, and serves the **team gateway**:
  `/team-gateway/mcp` and `/team-gateway/runner/...` pass a worker's or a runner's request on to
  HQ's `/v1/team/mcp` and `/v1/runner/...` with its method, body and own `Authorization`
  untouched, to HQ's URL and nowhere else, capped in size and time. HQ stays unpublished. A
  runner whose `HQ_URL` is the gateway asks for turns there and starts each as an offline
  **workbench** on the Pi's `endora_sandbox` network (`ENDORA_WORKBENCH=1`, a `/work` in memory,
  more room), reaching HQ and the model relays only through the gateway. The NAS's runner is
  unchanged. The ADR records the four layers against leaks (no keys in the sandbox, one exit
  for writes, exact-match blocking with variants, canaries), why machine-learning detection of
  personal data is skipped, and the egress proxy as designed and deferred.
- **The workbench (ring 1)**: a Dockerfile stage `workbench` (`endora-workbench:latest`, built
  with `--target workbench`, never the node's image), a disposable sandbox with bash, git,
  curl, python3 and a few tools, run as an unprivileged user with `/work` as its writable home.
  There the worker offers the model a `shell` tool (only under `ENDORA_WORKBENCH=1`): `bash -lc`
  in `/work`, 60 s unless told and at most 300, output kept to 16 KB head and tail, the exit
  code said, each call listed in the turn's line. The sandbox is offline (the egress proxy is
  deferred): the house's services and the models are reached only through the team's tools.
- **The dev node, the Pi that runs the team's sandboxes** (ADR 0110): the `endora_dev_node`
  Ansible role (Docker CE from Docker's repository with capped logs and live-restore, a ufw
  firewall that lets only SSH in from the LAN and checks it cannot lock you out, the journal and
  `/tmp` in memory, and the `endora_sandbox` network locked by `docker_egress` to the NAS's
  team gateway alone), run by `deploy/ansible/pi.yml`; `make deploy-pi` builds the arm64 node
  and workbench images on the Mac, copies it over SSH and starts the runner, and `make pi-runner-token`
  copies the runner's token from HQ once.
- **A brain per member, and what a cloud turn may read** ([ADR 0109](docs/adr/0109-a-brain-per-member.md),
  owner's decisions of 2026-10-17). Each member of HQ's team has a brain, local by default or a
  Claude its role may use (Sonnet or Haiku for the lead, developer and reviewer; Haiku for the
  researcher; Opus refused), set on the member's card on the HQ screen ("Brain: Local / Claude
  Sonnet / Claude Haiku") through the node and recorded in the audit log. The runner gives a
  cloud member's worker the Anthropic relay on the same team listener, and the worker speaks
  Anthropic's Messages API through the same loop as the local one. HQ tells the node such a
  turn is cloud; the lead's cloud turns may read scrubbed container logs and the Sonarr and
  Radarr queues, every other Confidential tool stays unlisted and refused for a cloud turn
  (`container_logs` is now checked too), and the Anthropic relay refuses any turn HQ does not
  say is cloud.
- **Lessons kept by the team**: the lead and the reviewer have `note_lesson`, which appends one
  line ("- <lesson> (Ada, 17 Oct)", at most 300 characters) to the task's goal's notes for the
  team, refused when the notes are full. Each is a `lesson.noted` event and a notice on the
  owner's desk ("Ada noted: …"). The instructions say when to note one.
- **The harm rule**: the worker's instructions say to report something as a problem only when
  a tool shows it causing harm, and the reviewer's briefing to send back any problem no tool
  shows causing harm.

- **The reviewer before done and before the owner's desk** ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md),
  plan §5 and §6). Work moved to `in_review` wakes the team's reviewer (`review_requested`);
  with no reviewer it is on the desk as ready to read, with its thread; its owner takes no turn
  on it meanwhile, except for the owner's word or a send-back. The reviewer's new `review`
  tool approves (closing work that needs no gate: a routine's task, or the home lab's work)
  or sends back (to `doing`, waking its owner), never its own. A question about the home lab
  from anyone but the reviewer becomes a "Draft for the owner" in the thread for the reviewer
  to check first. Every question reaching the desk lists the names in it the home lab does
  not have ("Not found in Endora's home lab: …"), against the containers the node now tells
  HQ (`POST /v1/lab/names`, signed). The worker's instructions say a home-lab question is
  checked first.

- **A turn's claims sit beside what it did** ([ADR 0106](docs/adr/0106-team-contracts.md)):
  the worker writes the calls its turn actually made, and how each came back, under its last
  word in the thread ("This turn did: …", "This turn did nothing but read: …", "This turn made
  no tool calls.") and under any question to the owner ("What I actually did this turn: …").
  A question to the owner may now be 1,200 characters, to leave room for it.

- **A Thinks switch on each model-list entry** ([ADR 0105](docs/adr/0105-models-in-ordered-lists-by-data-tier.md)).
  Off by default: the butler sends `"reasoning_effort": "none"` to a local entry that may not
  think, which takes a local `qwen3:8b` from about 10 seconds to under one for a tool call.
  On, nothing is sent; a cloud entry never gets the field. The team's local relay always lets
  the model think, and takes the field off a worker's call.

- **Stranded assigned work is recovered**: an open task with a member and nothing queued,
  running or held for it for ten minutes gets one `assignment_recovery` wake for that member;
  still stranded ten minutes later, it is on the owner's desk as stuck, once, and never handed
  to anyone else.
- **The bridge: Endora's capabilities as the team's tools** ([ADR 0108](docs/adr/0108-the-bridge.md)).
  The team's contracts list now carries, after the container tools, every capability and
  tool-server tool Endora's policy allows, each marked read or act by Endora's own knowledge
  (a tool server's tool reads only when the owner named it the reader). Never offered: what is
  off, Restricted, Endora's administration, or the owner's mail, chat, brain, brief, image and
  whereabouts. Confidential results go only to turns on a local model; lead and developers
  read and act, reviewers and researchers read. An act that is on runs at once, three an hour
  a tool, and is a notice on the owner's desk; an act that asks becomes an **approval** on the
  desk showing exactly what will run, the member's turn ends, and Approve runs the stored call
  once (Decline, or a day's wait, runs nothing). Every call is in the audit log.
- **Stuck downloads**: Sonarr's and Radarr's queues say why an item is stuck, and two actions
  join them, for the butler and the team: retry the imports (on by default) and remove,
  blocklist and search again (asks by default). Another app of the same API is a row of
  configuration.
- **The owner's reply on any task** wakes whoever has it at once, mention or not, past the
  back-off; and **a goal's notes for the team** (at most 2,000 characters, edited on the HQ
  screen) reach the top of every briefing under it.

- **Routines, alarms and check-backs: how the team keeps working**
  ([ADR 0107](docs/adr/0107-routines-alarms-and-check-backs.md)). Goals stay passive and work
  arrives as tasks. A routine hands one member a task on a rhythm (one open at a time; a late
  tick fires once; every firing, skip and post in HQ's feed; never past the kill switch, a
  pause, the budget or a goal that is not active), and its member closes the check once taken.
  The node's home-lab observer raises an alarm when a container or service goes from well to
  trouble (new trouble only, never Endora's own, one an hour), which HQ's alarm routine turns
  into a task or a post in its open one. A member can `check_back` on its task in 5 to 720
  minutes. A clock with nothing to do starts no turn, and a task whose turns move nothing backs
  off its stale wakes from 2 to 30 minutes. HQ's screen has a Routines card (rhythm, last run,
  on/off, run now, add one), and HQ makes "Check the home lab", every 6 hours for the lead,
  once on first start with the demo off.

- **The team's contracts on the home lab** ([ADR 0106](docs/adr/0106-team-contracts.md)). A
  turn sees the home lab (`homelab_status`), reads a container's log scrubbed of secrets
  (`container_logs`) and restarts a container within limits (`restart_container`: one an hour
  each, three in all, never Endora's own, never what a restart will not fix), all decided by
  the node and in its audit log; a restart lands on the owner's desk as a notice. The owner
  switches the restart, or every contract, off on the HQ screen. With the demo off, HQ puts the
  demo's goals and open work away on start.

- **Endora HQ's real workers** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 4). The team
  protocol over MCP, opened by a turn's own token; the node's model relays (Anthropic with the
  owner's key, capped and charged before and during each call; the local model at no cost);
  `endora-worker`, Endora's own Rust worker, which claims its task before the model's first
  round and answers a repeated call from what the turn already has; `endora-runner`, which
  starts each turn as a fresh, read-only, capped container on a fenced internal network that
  reaches only HQ and the node's team listener; and leases, so a report from an earlier attempt
  of a turn ends nothing.

- **Endora HQ, the service and the office** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 2).
  `endora-hq` runs the team in its own container (Compose profile `hq`, from the node's image),
  with its own database, ticking every 30 seconds, and answers only the node, which signs every
  request with a token HQ makes for itself in a volume only the two share. The console's new
  **HQ** screen draws the team's office, phone first: members walk to their desks when a turn
  starts, wakes fly from the server closet, the board has a sticky note per task, the burn gauge
  is today's spend, the lever is the kill switch. Tap anyone or any note for its detail and its
  chain of why. Below are your desk (approve or decline a proposed goal, try stuck work again or
  drop it, mark finished work merged), the activity, and the team's budgets; you can set the
  team a goal. Every decision and the kill switch go through the node, which records them in
  Endora's audit log. Until real workers exist (phases 3 and 4) a scripted demo team works on
  the simulated runner, labelled as such. Office art: Pixel Agents (MIT) and MetroCity by
  JIK-A-4 (CC0), with its credits and licence served beside it.

- **Endora HQ's team core** ([ADR 0102](docs/adr/0102-endora-hq.md), phase 1 of
  [its spec](docs/plans/endora-hq.md)): `domains/team`, with no models yet. A team works
  towards goals through a tree of tasks; a task has one owner at a time, taken in one atomic
  step; every turn starts with a wake that leaves a receipt (queued, folded into the member's
  own turn, held behind another member's, refused and why, started, finished). The tick
  starts queued turns within each member's slots, the runner's capacity and the day's budget,
  retries a vanished turn twice a minute apart, stops one that runs too long, nudges stalled
  work once and then puts it on the owner's desk. Money is an append-only ledger: reserved
  before a turn, used during it, and a turn that crosses what it reserved is stopped then.
  Done needs the gate and a review by a different member. The kill switch stops every turn
  and turns every wake away until the owner resumes. A simulated runner drives the tests.

- **The brain** ([ADR 0101](docs/adr/0101-the-brain.md)). What Endora knows about you is now a
  folder of markdown pages with its own git history: `memory.md` (what you told it and what it
  knows best, read on every turn), an index, a log, and pages it creates and organises itself,
  each with a summary on top and a dated timeline below.
  - After a conversation it notes what lasts onto the page it belongs on, from your own
    messages only, and says so in Activity. In quiet time the local model writes a grown
    page's summary again, and what it knows best.
  - About you shows what you told it, what it knows best and its pages; a page opens, edits
    as text, shows its history, and can be deleted. **Forget** removes every page and its
    history and keeps what you told it.
  - The model reads memory and the page list every turn, and opens a page with the new
    `read_page` skill when it needs the detail.
  - `ENDORA_BRAIN_DIR` in `local.mk` (or the role's `endora_brain_dir`, by default beside
    Endora's other folders) puts it in a host folder you can open. The nightly backup writes it
    as a git bundle. Edits made outside Endora are committed as yours first.
  - New routes: `GET /v1/brain`, `GET`/`PUT`/`DELETE /v1/brain/pages/{path}`,
    `POST /v1/brain/forget`. The CLI has `brain` and `brain page`.

- **Spotify, connected once in Endora** (Settings → Connections → Spotify). Endora holds the
  sign-in and lends an hour's access token to whichever device asks; the refresh token never
  leaves the node, so two devices cannot knock each other's sign-in out when Spotify rotates it.
  A sign-in started on one device shows as waiting on the other and can be finished there: by
  approving on the phone (Spotify sends you back to Endora, which finishes by itself) or by
  pasting the address Spotify sends you to. A device's existing sign-in can be handed over.
  The handheld's Music page now plays with it: it borrows the token through the unlocked
  session, hands over a sign-in it made by itself and then deletes its copy, and its setup
  starts and finishes the sign-in on Endora. Locking the handheld drops the borrowed session.
- **Pi-hole and UniFi connections** (Settings → Connections → Home lab). They hold the
  credentials for what Endora will read about your network. Each has a Test button that
  signs in and signs out again. Pi-hole's app password is only ever used to read. UniFi
  takes a View Only local admin, and its self-made certificate is not checked.
- **The web console does what the handheld does.** Music (what is playing, play and pause,
  skip, volume, choose the device, search, playlists, through the borrowed token), Smart home
  (switch favorites, set scenes, choose favorites), Media (search and request, recent requests)
  and Home lab (NAS figures, alerts, each service's state, container start/stop/restart and
  logs, allowlisted jobs). Each acts through the node's one allowlisted action route and asks
  before anything consequential.

- **The handheld terminal is part of Endora** ([ADR 0099](docs/adr/0099-the-handheld-is-part-of-endora.md)).
  `clients/terminal`, the Rust TUI for a small Linux handheld, joins the workspace: one
  lockfile, one CI (its Python, privacy and rendering checks, and an ARM64 bundle artifact),
  Apache-2.0. It still talks to the node only over HTTP.
- **Endora's Ansible roles ship with Endora** as the `endora.homelab` collection
  (`deploy/ansible/collection`): the node, the mail mirror, locked-down container networks and
  the host job agent, which now installs itself from the collection. An ansible-nas checkout pins
  it; CI checks every role.
- **A shared wire protocol** (`shared/protocol`). The handheld's pages, home screen, action
  outcomes and chat stream events are written down once; the node refuses to send a response
  that does not fit, and both sides' tests fail if either drifts. The handheld now shows what
  Endora is doing while it works, and why a reply failed, which it used to drop.

- **An AI screen.** Settings → AI (it was Models) opens with what Endora runs: the conversation
  model (or the router and writer), the deep model with its calls and cost this month, the mail
  reader, vision and speech-to-text. Each says which model, where it runs (on your network or
  in the cloud), what the model server says about it now (graphics card or processor, memory,
  context, size) and what it sees. It warns when the model server gives less context than
  Endora assumes, which silently cuts long turns short. The mail reader's model, until now
  set only in the deployment, is chosen there; the mail reader always stays on the local
  endpoint, whatever the conversation uses.

- **Endora copies its own data every night** ([ADR 0098](docs/adr/0098-it-can-be-rebuilt.md)).
  At 01:30 by your clock it writes a consistent, integrity-checked copy of its database into
  `backups/` beside it, daily and weekly, keeping seven of each, so the house's backup always
  finds a whole copy rather than a live file. Activity says each night that it was saved, and
  says so once a day if it failed. [Rebuilding from nothing](docs/recovery.md) is the runbook.

### Changed

- **The World card is easy to edit on a phone.** A "Paste Markdown" box takes a whole document
  (`## house / key — Title` starts a section, `## timeline` bullets `- 17 Oct 2026: text` are
  timeline entries), shows what each change would do with a tick to leave any out, and one
  Apply sends them, sections first, saying how each went. Every section has an Edit button
  that opens a full-screen editor with Save and Cancel always at the top; the owner can edit a
  member's section too, and the member's text stays in its revisions. Buttons are at least 44
  pixels tall and the text boxes use 16-pixel type so a phone does not zoom. No new route.

- **Borrowing is capped at one extra share, and a member at the cap asks the owner**
  ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md), amended again). A paid member
  may spend while its spend today is below its share plus the lesser of its share and what the
  other paid members have not used; the owner's own requests stay outside it, and the pool's
  day is still the ceiling. The first time a member is turned away at that limit while the
  pool's day has money, one "out for today" item goes on the owner's desk (once a member a day):
  give it an allowance (an amount on the card, at most the pool's day left, $0.50 as offered)
  until the owner's next day starts, or wait for tomorrow.
- **A member's share is a first claim, not a wall; the owner's own requests are the pool's;
  and shares are weighted by brain price** ([ADR 0113](docs/adr/0113-projects-budgets-and-reports-to.md),
  amended). Past its daily share a paid member draws on what the other paid members have not
  used today, the pool's day staying the hard ceiling (before, one member at its share blocked
  every task waiting on it until midnight). A turn that exists because the owner asked (their
  task, reply, desk answer or decision, "Run now") is marked `asked_by_owner`, its ledger rows
  flagged, and counted against the pool's day only. A member's claim on the pool is its role's
  weight times its brain's price class (Opus 1000, Sonnet 500, Haiku 25 thousandths, from the
  relay's prices, which a test holds them to), so Haiku hires no longer dilute a Sonnet lead.
- **A question on a project's task goes to the project's lead** ([ADR 0111](docs/adr/0111-the-ceo-and-the-chain-of-command.md),
  amended), when at work and not the asker; otherwise up the asker's chain as before.
- **The tester passes or sends back, and never makes work** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md),
  amended): her briefing says so, and `create_task` is no longer in her tools.
- **The Team card is an org chart**: the board and the CEO with whoever has no lead, then one
  group per lead (Engineering for the wing's, Ops otherwise), one line per member (role, brain,
  what it is on, today's spend against its share), its controls opened by tapping the line. A
  task's turns read oldest first, as its thread does, the last five with a toggle for all.
- **The CEO's effort follows the kind of turn** ([ADR 0109](docs/adr/0109-a-brain-per-member.md#cost-notes),
  [0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)): `low` for ordinary turns
  (a question up the chain, a post, a recovery), `medium` for its daily review and planning,
  `high` when a project proposal waits for it or it carries something to the board. The lead
  stays `medium`, the researcher `low`. Posts alone no longer count as progress for the
  empty-turn back-off.

- **Sonnet 5.5's cache reads are priced at $0.10 a million**, down from $0.20, after
  Anthropic's correction: the team relay charges a turn's cached prompt at the new price. The
  other models are unchanged.
- **Skills say what they do, for people.** A skill whose description is written for the model
  ("Use this for any question about what YOU did", "Civic business ONLY", "fetch useful URLs
  with web_fetch") shows a line for people on the settings screen instead (`for_people`, the
  API's `summary`); what the model reads is unchanged.
- **Connections is short too.** Tool servers are rows that open to their own screen (tools,
  test, edit, remove); adding one is folded, and so is the Home Assistant "connect something"
  wizard until it is wanted.
- **Your data** is its own screen: export, with what it does and does not hold, and delete
  everything. They were loose at the foot of Settings.
- **Skills is a list, Connections and Permissions are their own.** Skills was one page about
  twelve phone screens long: the autonomy switches, a Home Assistant wizard, every skill with
  its setup form open, and tool servers. Now Skills lists what chat can do in four groups (Your
  home, Looking things up, Home lab, About Endora), each row with one status: On, Off, Needs
  setup, or **Needs a connection**, and each opening to its own screen. **Connections** holds
  where skills get their data: Home Assistant and its wizard, the home lab's services (named
  for themselves: Portainer, Docker, Sonarr, Radarr, Prometheus, Homepage, Overseerr; they were
  "Handheld: containers", "Overview: Sonarr" and so on), and tool servers. **Permissions** holds
  how far Endora may act on its own.

### Removed

- **The server-side speech-to-text path.** `POST /v1/transcribe`, its Whisper proxy and
  silence filter, the AI screen's speech-server choice, `ENDORA_STT_URL` and the `stt` flag in
  `/health` are gone. Nothing had called them since push-to-talk was removed; dictation is the
  browser's own recognition, and the AI screen says where that audio may go.

- **Beliefs** ([ADR 0101](docs/adr/0101-the-brain.md)), with their confidence, decay,
  affirming and correcting. **Breaking:** the `/v1/understanding` routes are gone, the export
  has `pages` and no `beliefs`, and the context diagnostics report `memory` and `brain_pages`
  instead of `understands`. What you had told Endora moves from the database into memory on
  the first start, and the `beliefs` and `preferences` tables are dropped. The preference
  routes keep their shape.
- **The clean slate** ([ADR 0100](docs/adr/0100-the-clean-slate.md)). What Endora did without
  being asked had become noise, and it is gone, code and storage, so the always-working butler
  can be built again on real signals:
  - the daily check-in, and waking early on an unusual change;
  - the nightly loop, notions ("still wondering"), and the replay of failed asks (specimens).
    A turn that could not answer no longer says it kept the question "to retry overnight";
  - intentions formed from beliefs, and their nudges;
  - the own-work lane and its `ENDORA_OWN_WORK` switch (in the Makefile, Compose file and the
    `endora` Ansible role too);
  - durable goals started from chat: the goal worker, the goal strip and its Stop button.
    Research you ask for stays, with its card and its report;
  - the 4 am model tune's schedule. The comparison still runs when started by hand;
  - Inbox. Messages Endora sends unasked stay in the chat, and Activity is the one record;
  - About you as a list of beliefs by kind to confirm. It now says how much Endora has learned
    and has a **Forget** button that deletes all of it; what you told it yourself stays.
    Beliefs that were passing remarks (an intent, or "other") are no longer formed;
  - device changes written to Activity by the heartbeat.

  **Breaking:** the routes `/v1/checkin`, `/v1/nightly-loop/schedule`,
  `/v1/model-tune/schedule`, `/v1/notions`, `/v1/intentions` and `/v1/goals` are gone, and the
  export has no `goals` field. An existing database drops the removed tables when the node
  starts; a table nothing reads is data nothing can export or delete.

### Fixed

- **Branches are the wing's** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md),
  amended). The RACI gave changing the lab to the engineering lead and its members, but any
  lead was offered `repo_submit`. On task 687 the Ops lead built and submitted a branch
  herself; a lead never accepts or hands on its own work, so the branch sat in review and the
  CEO was asked twice. Now only a lead or a developer in the engineering wing submits a branch
  or asks for a recheck. Anyone else is refused before anything is kept, and is told who leads
  the wing and to ask for the change with `propose_project`. HQ tells the node whether a turn is
  the wing's (`wing` on `/v1/team/turn`), and the node then neither offers nor runs the
  submission. Reading a branch is unchanged.
- **Evidence is read as the tools name it** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)).
  Every result of Endora's tools ends "(receipt r12)", and a member's instructions say to cite it
  so; HQ read `receipt` and then wanted a bare number, so `receipt r12` was refused as "not
  evidence". Task 687's tester was refused four times passing a branch, and the lead's citations
  with hers, and the work sat in review. A reference is now read with its word and letter
  together (`receipt r12`, `event e340`, `task #12`), in the tools' parentheses, and with words
  after it saying what it showed (`r12 check on hq/… (ok)`); it must still lead, be whole and
  name one kind, and what it cites is looked up and judged as before. A test reads every
  receipt the instructions teach back through the parser, so the two cannot drift apart again,
  and a scenario replays task 687's pass to the lead's acceptance.
- **Claude Code's shell is the sandbox's whole Bash, and a refused call is kept as refused**
  ([ADR 0117](docs/adr/0117-claude-code-as-a-developer-runtime.md)). Six `Bash(prefix *)` rules
  under `dontAsk` denied any command holding a subshell group, a `for` loop or `$?`, and one
  denial in a live turn became "the shell refuses every command": the wing's developer blocked
  its task. The container is the boundary (the list protected nothing it does not), so the
  turn is given `Bash`; the bench's managed settings say the same. And Claude Code reports a
  denied call, and a command that failed, as a response starting "Error:", which the node kept
  as done: it is kept as refused now, so a receipt cannot show a refused call as one that ran.
  Ships with the workbench image (the worker's command line) and the node.

- **A check box that cannot run its checks passes nothing** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).
  "No worse than develop" counted a failure with no readable count as no worse whenever develop
  failed too, so a box with no tools (every check exiting 127 on the branch and on develop, as
  the first two checks did live) would have passed every branch. A check that could not run (a
  missing or non-executable tool, a kill, a timeout) is no finding now: a develop on which one
  could not run is "could not be checked" and excuses nothing, and a branch's own check that
  could not run is never excused. The result line says "could not run". Ships with the
  workbench image (the check job).

- **A member can check out the team's own submitted branch, and a branch is named for its task**
  ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)). In 39 hours of live
  use the wing submitted six branches and none reached the owner; `repo_checkout` of a branch
  the node was keeping was refused three times ("the fork has no branch"), because a
  submission is kept as `refs/submissions/…` and reaches the fork's heads only after the
  owner's push. Each refusal ended with the work recreated from develop. The node now asks HQ
  (`POST /v1/team/branch/checkout`), whose record says whether the branch is the turn's work
  (its task, the tasks under and above it) or its member's, and at which head; the node hands
  out the base and the kept ref, at that head only, and the worker clones the base, fetches the
  kept ref as a head and checks it out, so it can be added to and submitted again (and
  `/work/repo` is emptied, not replaced, so a shell standing in it is not left in a folder
  that is gone). `repo_submit` now requires `hq/task-<the turn's task>-<slug>`, so no task's
  kept branch can be overwritten by another's. Ships with the node, HQ and the workbench image.

- **A lead or a developer can ask for a kept branch to be checked again** (`recheck`,
  [ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)). A check was queued only
  by submitting a branch from a sandbox that held it, so a check that failed for the check
  box's own reasons (tools missing, a collection absent) cost a recreated branch each time:
  task 674 existed only to check one commit again, and 679's probe branch, with no file in it,
  only to get a check run. `recheck` queues a new check of the newest kept branch of the work
  with no sandbox and no new submission, one at a time; its result is a receipt on the turn that
  asked and wakes it, the branch's member and the tester. A failed check's "sent back" post
  says to recheck when the box could not run.

- **The tester can take a reading of a branch, so her pass can be recorded**
  ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)). A pass must cite a
  reading of her own, and she had none she could take of a branch: the node offered her neither
  wing contract and `read_document` leaves no receipt, so her three turns on task 670 each
  ended with "tried to call review, refused". The node now offers her `repo_checkout` (never
  `repo_submit`): checking the team's kept branch out is a witnessed receipt that names the
  branch and its head, which she cites with the check's. Her briefing and her wing paragraph
  say so (a read-only paragraph where she may not submit), and the engine and the scenarios
  take her pass on that real receipt, where they had recorded a `read_document` one by hand
  that the real tool never makes.

- **Work in review is never demoted to blocked** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).
  HQ's sweep of stranded work nudged the tester once, and at the next quiet window set the
  task `blocked`: task 670 was blocked 44 minutes after it entered review, out of the tester's
  reach (she passes work in review) and settable back by its holder alone, and it climbed to
  the CEO five times for a review that had simply not happened. It stays in review now, and
  its lead is woken to decide it up the same ladder.

- **A draft for the owner never closes work** ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md)).
  The CEO's decision for the board moves the work task itself into review, and the tester's
  `approve` then went through the draft's path: refused for the gate while it was false (tasks
  671, 672, 679 and 670), and, with the gate true, the task closed as done and its push was
  offered with no evidence judged and no lead's acceptance. A pass on a draft is refused now
  (forward it with `ask_owner`, or send it back); sending back is unchanged.

- **The push the owner is offered is of the head that was reviewed**
  ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)). The gate was the task's
  and every check overwrote it, and the card offered the newest submitted branch, so a head
  submitted after the tester's pass changed what the card offered with the gate left as the
  first head's check set it. A new head submitted while the task is in review sends it back
  ("a new head needs a new review") and resets the gate to wait for its own check; a late check
  of an older head is recorded but sets nothing and wakes nobody. `repo_submit` also refuses a
  branch that changes no file (a probe with none was accepted, on task 679), and a refused
  submission no longer moves the kept branch: the node stages a submission and keeps it only
  once every rule has passed, where a missing trailer (five times live) used to overwrite a
  good kept branch of the same name.

- **Live sound no longer falls behind the picture over time, and no longer dips every few
  minutes** ([ADR 0104](docs/adr/0104-endora-sound.md)). The hub holds an AirPlay door's
  backlog to 60 ms beyond a mix block, dropping any standing excess a few milliseconds at a
  time under a crossfade (a sender with a faster clock used to pile sound up for ever), and
  `GET /v1/state` reports each source's wait (`intake`) without a probe. The console's live
  buffer grows by fixed steps (to 800 ms) and shrinks only to a floor set by the last dry
  spell, instead of multiplying to a second and shrinking into the same dry spell; a surplus
  over target is caught up sooner. New: a **Lip-sync** control on the live speaker card
  (-0.5 s to +1.5 s in 0.1 s steps, saved per speaker as `sync_ms` in the hub's state;
  `PUT /v1/speakers/{id}/sync`).

- **The wing's check judges a branch against its base.** The fork's own develop already had
  ansible-lint findings and yamllint errors, so every branch failed the check, even one that
  only added documents, and the reviewer was refused for a gate that could never pass. A
  branch with a failure now has the same checks run on the commit it was cut from (the node
  sends the recorded base with the bundle) and passes when none is worse: "no worse than
  develop" ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).

- **Sound: a phone no longer hears silence from its own door.** Apple TV casting to "Endora
  Mixer (iPhone)" while listening on the iPhone played nothing, because a device's own door
  never feeds that device. Per-device doors are gone: the hub merges existing ones into the
  general "Endora Mixer" at start, keeping their wires and levels, and the console stops
  making them.

- **Every submitted commit carries its trailer.** `repo_submit` refused every branch with
  "lacks its trailer" because the commit hook that adds `Made-by: Endora HQ (...)` never ran:
  the sandbox's `/work` is a tmpfs, which Docker mounts `noexec`, and git ignores a hook it
  cannot execute. The worker now stamps the trailer onto any commit of the member's `hq/`
  branch that lacks it, with git plumbing and before bundling, so the guarantee no longer
  depends on running a hook; a branch that already complies is untouched
  ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).

- **The check job has every collection the fork needs** ([ADR 0115](docs/adr/0115-the-engineering-wing-and-the-git-flow.md)).
  The workbench's `ansible-playbook --syntax-check nas.yml` failed on the fork's
  `endora.homelab.*` roles because the image baked every collection and role in the fork's
  requirements except Endora's own, which the fork pins to a git commit the offline sandbox
  cannot fetch, so the team kept asking the owner to install it on the dev node. The image now
  builds `endora.homelab` from the Endora source it is built from and installs it beside the
  others, and its build fails if the collection or its roles are missing.

- **The bench's refusals say whose they are** (ADR 0123, rule 5). The first bench run reached
  no model: every seat's first call came back "Credit balance is too low", which Claude Code
  shows for any 402 or `billing_error`. That 400 was Anthropic's own (its request id was in
  the body), passed on word for word, so the run's record could not say at which step or why.
  Anthropic's refusals now keep their status, kind and request id, with words naming the step
  and the model, and are kept in `GET /team-relay/bench` (`upstream_refusals`,
  `last_refusal`) and in the node's log. The bench's own refusals are a 400 starting `bench: `
  (never a 402), and calls in flight holding the rest of the day are a 429 with `retry-after`.
- **The team's world, from the owner's live export** ([ADR 0122](docs/adr/0122-the-teams-world-stages-stories-and-situations.md)).
  The move of the house log dated a yearless day by comparing its noon with the move's instant,
  so a move in the early morning put the move's own day a year back; it now compares days, in
  the owner's calendar. It also kept the date a writer had put at the start of a line
  (`17 Oct: 2025-10-18: …`): those are taken off. A one-time repair (`world_dates_repaired`,
  with each row's old date and text kept in `world_timeline_repairs`) mends the rows already
  moved. The Markdown export lists the timeline newest first and says so, prints the lab page
  once under a clean heading (the heading had been built from the page's whole text, so its
  lines came out twice), and names roles as the screen does (CEO, Lead, Tester). Every
  briefing now says today's date with the year, and a year one out on a date that starts a
  member's note, house note, story or section, within three days of today, is corrected and
  recorded in the feed.
- **Time-bound criteria** ([ADR 0120](docs/adr/0120-roles-as-an-enforced-raci.md)): a
  criterion may wait (`after`) from the work's last change; the lab's "no new alarm for the
  same containers since" waits 60 minutes, and closing or passing it before then is refused,
  "wait until HH:MM (owner's time)". Live, #667 was closed two minutes after a restart and the
  alarm came back.
- **Repeated readings**: a reading the turn before already took shows nothing and no longer
  resets the recovery nudge; no recovery wake while the member's check-back waits; a stale wake
  waits two minutes after the member's last turn on the task. Live, a lead took four turns ten
  seconds apart re-reading one status.
- **House log dates**: lines carry the year, and a date the writer put in the entry is taken
  off ("17 Oct: 2025-10-18: …").
- **Logs are no longer cut off.** The HQ feed shows a thread post whole (it was cut to 80
  characters with no sign of it; the event now names its post's place in the thread and says
  when its preview was cut), folds lines over 240 characters behind "more", and on a phone is a
  plain list in the page, not a 300px window. It shows thirty lines and "Show older" reveals
  the kept ones, then asks HQ for earlier events (`GET /v1/hq/events?before=&limit=`). The
  Activity screen has "Load more" (`GET /v1/activity?limit=&offset=`), and the home "Watching N
  things" line wraps to two lines instead of ending in an ellipsis.
- **Every server-side cut says it was cut.** One helper (`domain::cut::cut`) ends what it cuts
  in "…[cut]", character-safe and inside the same limit: a remote run's failure reason, a
  receipt's excerpt, arguments and criterion, and the question, approval, notice, answer and
  outcome texts. The limits are unchanged.
- **"Try again" on a stuck desk item no longer does nothing without a word.** It released the
  task and queued a wake that the kill switch, a pause, a furlough, the budget or a paused
  project refused, then marked the item decided. Now it checks first: if nobody can be woken it
  changes nothing, the item stays waiting, and the refusal's reason is said on the card (a
  refused desk decision stays visible there, not only in a toast).
- **A post in a turn with receipts is never "unsupported"**, nor the worker's automatic last
  word ([ADR 0114](docs/adr/0114-evidence-acceptance-criteria-and-the-tester.md), amended): a
  closing post in a turn with two receipts was tagged, because only receipts of calls that held
  were counted.
- **`make smoke` waits for the node**: up to 30 seconds for `/health` before its checks, so a
  run straight after `make deploy` no longer fails on a node that has been up for a second.
- **HQ's day is the owner's.** HQ counted its days from UTC midnight, so the scorecard's
  "today" emptied at eight in the evening and the ledger's days turned over then too. HQ now
  reads the same `ENDORA_TIME_ZONE` / `ENDORA_UTC_OFFSET_MINUTES` the node does.
- **The team never switches the owner's devices** ([ADR 0108](docs/adr/0108-the-bridge.md)).
  Every act of the tool server paired with Home Assistant (`HassTurnOff`, `HassTurnOn` and the
  rest) is kept off the bridge: live, the developer asked to turn off "deluge" to restart it.
  The server's named reader stays a read under the tier's rules.
- **A lead on Claude knows it can read the home lab.** It said three times its tools could not,
  though `homelab_status` was offered (it had read the lab with it minutes before): HQ's
  briefing listed only HQ's tools as "Your tools". The line is now "Your tools at HQ", with
  Endora's home-lab tools beside them, and the worker names the container tools the node
  offered as the turn's own.
- **`restart_container` never starts a stopped container** ([ADR 0106](docs/adr/0106-team-contracts.md)):
  Portainer's restart would start an exited one, and a container may be stopped on purpose
  (an old sound server whose ports the sound hub needs). Only a running container is
  restarted; any other is refused, costing no restart, and the member is told to ask the owner.
- HQ's board hides a dropped goal's tasks, so the retired demo's notes do not bury the owner's;
  a live update keeps the page's scroll and the activity log's; and the screen shows money
  spent apart from money held for turns, in dollars.
- The team's relays and contracts are served only on the team listener, never on the node's
  main port, and a contract refused for an unknown name or bad arguments is in the audit log.
- Deploys recompile only what changed and no longer send gigabytes of worktrees and build
  output to every build.

- **Endora no longer asks about devices that stopped answering** (the "Still yours?" card).
  The owner did not want it. It is removed with what was under it: the card and its routes,
  the morning brief's lines, the own-work lane's items, and the detection and its store. Work
  Endora started about such a device before is withdrawn at the next check; an existing
  database drops its old table when the node starts.

- **Endora answered faster, and about the right day.** Measured on the house's A2000, with qwen2.5:7b:
  - **The prompt was read from the clock onward on every message.** The date and time sat
    second in the request, ahead of the instructions and the tool list. They now travel with
    the latest message, so the model reuses the rest: the next message starts answering in
    about 0.6 s instead of about 5.5 s.
  - **Endora asked Ollama to unload its model after 30 minutes**, which overrode a server set
    to keep it loaded, so the first message after a pause paid a 13-second reload. It now asks
    to keep it loaded.
  - **The clock it was told was UTC.** It is now local time with its offset, so a summer
    evening is no longer "tomorrow".
  - **"What day is today?" went to the deep model.** The small model filed it as needing
    a lookup, though the date is in every request; date and time questions now stay local.
  - **One-off events tied to a day became beliefs.** "An appointment tomorrow at 6:30" was
    filed as something Endora understands about you, was still "tomorrow" the next day, and
    was trusted over the calendar. Statements tied to today, tomorrow, tonight or yesterday
    are no longer kept as understanding.

- **Opening Mail, AI or Spotify from the menu could show "Loading…"** until something else
  reloaded the console; a screen now fetches what it shows when it opens.

- **The handheld's Containers page showed raw numbers** ("CPU %: 7.251164760358932"); it shows
  the home screen's short figures.

- **The handheld's Home catalog never loaded when Home Assistant's address was typed without
  `http://`.** The Home Assistant skill accepted `host:8123`; the console's own requests
  refused it as a configuration error. Both read it the same way now.

- **The model settings answered "internal error" on any node older than the preferred
  endpoint.** Its two columns were added only by a migration that runs in tests, never in
  production, so `GET /v1/model-config` (the Models screen's everyday card, and the new AI
  screen) failed on every database created before it. Production now adds them, and a test
  fails if a column is ever added only by the test migration again.

- **A skill that rides a connection said "On" before the connection was set up.** The
  settings screen judged it by its own (empty) settings; chat, rightly, by the connection's.
  Both now use one rule (`settings_ready`), so mail shows as needing Home Assistant and the
  home-lab skills as needing their service until each is set up.

### Security

- **The vault's unlock and the node's keys** ([ADR 0127](docs/adr/0127-openbao-and-the-passkey-unlock.md)).
  Only `GET /v1/vault` and the two unlock routes answer without a sign-in; the unlock is
  authorised by a registered passkey (a signature over a single-use challenge, user verified,
  and a PRF output that must open its wrapped key), throttled with the sign-in's ladder, and
  every attempt is audited by passkey and time, never a value. Unseal shares, the unseal key and
  the PRF-derived keys are wiped from memory after use; none is logged, stored in the clear, or
  shown after initialisation. The node's AppRole may only use its own Transit key and KV corner.
  Once the node's keys are in the vault, the key file and a copy of the database together no
  longer open its credentials.
- **Secrets at rest in the node** ([ADR 0125](docs/adr/0125-secrets-at-rest-in-the-node.md)).
  The node's database held every credential in plaintext and its sessions as the tokens
  themselves, so any copy of the file (a nightly snapshot, the house's backup) handed over
  all of them. Now: provider API keys, MCP bearer tokens and environments, every capability
  setting (the Home Assistant token, the services' keys and passwords, Spotify's tokens), the
  authenticator secret and the bootstrap token are sealed with XChaCha20-Poly1305 under a
  data key (`enc:v1:<kid>:<nonce>:<ct>`), and the data key is wrapped by a key outside the
  database: `secrets.key` beside it (`ENDORA_SECRETS_KEY_FILE`), made `0600` on the first
  start. That start seals what was stored in plaintext, once, and `VACUUM`s so none survives
  in the file; a plaintext value found later is refused and logged by column, never by value.
  Sessions are kept only as SHA-256 digests (hashed in place on the first start, nobody
  signed out), and a console session also ends after seven days unused. `endora-node secrets
  rekey` rotates the data key; `secrets forget --i-have-lost-the-key` is the way out when
  the key is lost. The key-encryption key sits behind a `KeySource` trait, so a Transit
  engine (OpenBao) can take over by re-wrapping the data keys, without touching a value. **Keep
  `secrets.key` apart from the database's backups** (`docs/homelab.md`, `docs/recovery.md`).
  The TLS private key and the database file are now written owner-only too. No new crate:
  `chacha20poly1305`, `base64`, `zeroize` and `sha2` were already in the lock file.

### Changed

- **A shorter Settings menu.** Settings is five entries: Notifications & voice, Models, Skills,
  About you, and Your sign-in. What Endora has done is **Activity** in the main menu, beside
  the Inbox: it is a record, not a setting. What you told Endora and what it has learned are
  one screen, **About you** (they were two, with nothing saying which was which). Recipes are
  reached from the end of Skills. The seven entries that only hold settings for the handheld
  ("Handheld: containers", "Overview: Sonarr" and the rest) say **Set up** once their settings
  are filled, instead of "Needs setup" forever, and lose an on/off switch that did nothing.
  "Uses freely" reads "Runs without asking" and "leaves device" reads "uses the internet". Two
  screens nothing could reach any more are gone.

### Added

- **Your time zone, by name** (`ENDORA_TIME_ZONE`, e.g. `America/New_York`). Daily patterns
  are found on your clock either side of a clock change, and the daily brief and nightly loop,
  stored as UTC hours, are moved when the clocks change so they keep their local hour. The fixed
  `ENDORA_UTC_OFFSET_MINUTES` remains as a fallback.

- **Endora reads your mail, and tells you what matters** ([ADR 0097](docs/adr/0097-it-reads-the-mail.md)).
  With `ENDORA_OWN_WORK=1` and `ENDORA_MAIL_DIR` pointing at the read-only copy the ansible-nas
  `mail_mirror` role keeps, it reads one message at a time in quiet time, newest first, back
  90 days. Code decides what it can: who you write to, which threads are yours, mailing lists,
  invitations. The local model, with no tools and the message marked as data, only describes
  it in a fixed shape that code checks field by field. Each message lands in a bucket
  (People, Money, Security, Appointments, Orders, Your app, Updates, Newsletters, Other), and
  the ones that matter (a reply in your thread, someone you write to waiting three days,
  money due, account security, an appointment, an order problem) go to the activity trail,
  a "Mail" section at the end of your morning brief, and a new **Mail** screen. A bill or a
  security notice from a sender nothing vouches for is marked **may not be genuine** and
  described by code, never in its own words. Nothing is opened, no link is followed, and no
  mail text reaches the model that words the brief.
  Bills are checked against your calendar through Home Assistant ("Follows 'Dental checkup'
  on 2026-10-02", or that there is no such appointment); dates mentioned in mail are listed
  under **Coming up** only when something confirms them; and when someone you write to sends
  fresh mail, chat says so without naming them, at most twice a day.
- **What you set up on the handheld can now be asked about in chat.** The handheld's Docker,
  Portainer, Sonarr, Radarr and NAS-metrics entries were settings-only, so a service you had
  already configured was invisible to the one place you would naturally ask about it. Five
  read-only skills now borrow those settings (nothing is configured twice, and each is offered
  only once its entry is set up). They never start, stop or restart anything, read container
  logs or search media, and take no input, so the model chooses no address. Their results mark
  the turn as outside material, so a turn that read them cannot act and is never escalated to
  the deep model: container names and NAS figures stay on the device. Overseerr and the home
  screen stay handheld-only. ([ADR 0093](docs/adr/0093-the-handheld-is-an-explicit-home-console.md),
  amended.)

- **Goals know whose they are, and Endora's own wait for quiet and never use the paid
  model** ([ADR 0094](docs/adr/0094-work-it-finds-for-itself.md), Phases 1–2 of
  [the always-on plan](docs/plans/always-on-work.md)). Nothing starts work on its own
  yet; this is the ground that makes it safe to. A goal now records whether the
  person asked for it or Endora started it. The person keeps three goals; Endora's
  own lane holds one, counted separately, so neither can use up the other's slots,
  and the worker takes a request before work nobody asked for. A pass on a goal
  Endora started runs on a butler with no deep model to escalate to, decided in the
  one place goal passes run rather than by a setting, and takes no pass within ten
  minutes of the person writing in chat. A database from before this reads every
  goal as the person's, and an origin it cannot read is treated as Endora's own, the
  lane that never outranks a request. The goals API reports each goal's `origin`.

- **Endora can look into a standing trouble on its own, quietly, behind a switch**
  ([ADR 0094](docs/adr/0094-work-it-finds-for-itself.md), Phase 3). Off unless the node
  is started with `ENDORA_OWN_WORK=1`. A device that has not answered for three days
  (and which you have not called fine) can become a goal Endora starts itself, only when
  you have been quiet, nothing you asked for is open, and half an hour has passed since
  the last one. It reads and looks; it cannot change anything; it uses the local model
  only and reads only the house (Home Assistant), not the web or any other skill,
  keeping only what it actually read. It is finished by the record it came from, not by a model: when the device
  answers again the goal completes with no further model call, and if you mark it fine
  the work stops. Its ending goes to the activity trail and posts nothing to chat. An
  item is looked into at most twice, a week apart, and never again once you press Stop.
  The nightly loop's own long-running intention is to be folded into this lane in the
  next phase, with the stumper replay.

- **The lane also asks failed questions again and looks into what Endora understands, and
  the nightly loop hands both over** ([ADR 0095](docs/adr/0095-more-sources-for-work-it-finds.md),
  Phase 4). Still behind `ENDORA_OWN_WORK=1`. With nothing wrong in the house, a question it
  once failed to answer is asked again a day after it failed, judged by the same
  check that filed it and giving up after the same number of tries; and failing that, the
  belief it is most sure of is looked into, with the web and the other read-only skills
  the nightly thread had, on the local model only. The nightly loop stops replaying a
  stumper and keeping a thread of its own **only when the switch is on**; with it off, the
  loop is exactly as it was, so deploying this changes nothing until the switch is
  turned on. Repairs were considered and left out: they already end in a policy action
  or a question only you can answer, so there is nothing for a goal to gather.

- **Endora notices what the house does at the same time most days**
  ([ADR 0096](docs/adr/0096-patterns-it-notices.md)). With `ENDORA_OWN_WORK=1`, it counts
  the changes the house has settled on over the last fortnight and writes a line to the
  activity trail when something happens at about the same time on most days: "the front
  door goes on around 07:30, on 11 of the last 14 days". It is arithmetic with no model,
  it says only what it counted, and it speaks again only when the picture changes (and not
  more than every six hours). Set `ENDORA_TIME_ZONE` for your hours. This is the
  first standing job: a review that keeps running, not a goal that ends. Reading the mail
  into buckets and a safe terminal sandbox are proposed in
  [the standing-jobs plan](docs/plans/standing-jobs.md) and wait on your decisions.

- **The node's data can live outside the container, and be backed up safely.** Everything
  stateful — every setting, API key and sign-in secret — is one SQLite file under `/data`,
  which in the container was a Docker named volume buried in Docker's own storage. Setting
  `ENDORA_DATA_DIR` in `local.mk` to an absolute host path now keeps it in a folder a backup
  tool can reach; unset, nothing changes. `make migrate-data` copies an existing volume across
  once (checksummed file by file, deleting nothing), and `make deploy` **refuses** to start on
  an empty data folder while the volume still holds the node's data, because that would be a
  brand-new node (new token, no settings) that looks exactly like data loss.
  `endora-node backup <file>` (and `make backup`) takes a consistent, integrity-checked,
  owner-only (`0600`) snapshot through SQLite while the node runs. A plain copy of a live
  database can land mid-write and look whole until it will not open. The snapshot never
  overwrites an earlier file, never creates a database from a mistyped path, and removes its
  own output if the check fails. Proven end to end on the real schema: a snapshot taken from a
  running node booted a second node on its own.

- **A specimen records how it ended, and the night says when it gave up**
  ([ADR 0075](docs/adr/0075-failures-become-specimens.md), groundwork for
  [ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)). A failed ask that
  the nightly loop replays to exhaustion is the live record proving the machinery
  cannot answer something — the only trigger ADR 0071 allows for drafting a new
  capability. That population could not be read: retirement was one flag, every
  listing asked for the specimens *not* retired, and a specimen that **passed on
  its last allowed replay** left a row identical to one that ran out — same replay
  count, same flag. Anything inferring "it gave up" from the count would have
  proposed a new capability for a gap that had just closed. A specimen now records
  which ending it reached, at the moment it reaches it, and the morning trail says
  so — *"I've stopped re-asking this one — nothing I have can answer it"* — where
  before it repeated "it still fails" identically on the first failed replay and
  the last. Existing databases are backfilled from what their rows can still prove;
  the one case that cannot be recovered (already retired on exactly the replay
  limit) is read as having given up, which is the likelier of the two and costs at
  most a proposal card the person is asked about.

- **Authoring a recipe is a loop, not a form**
  ([ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)). A new **Try it**
  button fetches the draft once and shows the address it really used, every field
  it can read — each one tappable straight into the sentence — and the sentence as
  it would read. No more hand-writing JSON paths against an API you can't see.
  Sample-value boxes appear per declared input as you type them. The trial runs
  the same validation, the same guarded fetch and the same templating a saved
  recipe uses, so what you see when you try it is what you get when you save it;
  nothing is stored or enabled by trying. Fields inside lists are skipped and said
  to be skipped, because a recipe reads object keys only.

### Fixed

- **The docs called `endora export` "a complete" backup. It covers memory only.** It has
  no settings, keys, stances, MCP servers or sign-in, and nothing can import it, so a node
  rebuilt from an export had lost its whole configuration. `docs/hosting.md` now says so and
  points at the snapshot.

- **Fetching a web page that contains any non-ASCII character crashed the fetch.** The
  page-to-text step stepped through the page a byte at a time and sliced it at each step,
  so the first character outside ASCII (an arrow in a page's navigation, an accented
  letter, a dash) made it panic, and a letter that changes length when lowercased threw
  every later position off. Found by running a goal against the real model and the real
  web. It now walks the page a character at a time and keeps the text it reads.

- **A recipe could reach the house's own network.** The recipe interpreter
  ([ADR 0071](docs/adr/0071-capabilities-it-writes-itself.md)) fetched through the
  same helper the constant-host built-in skills use, which does not apply the SSRF
  guard — so a recipe aimed at `169.254.169.254`, a `192.168.x.x` device, or
  `localhost` was simply fetched. A recipe's URL is arbitrary (the person types it,
  and one day the butler drafts it), which is exactly the case the guard exists for
  (ADR 0051). Recipes now fetch through the guarded path, which also re-guards each
  redirect hop. The regression test asserts on the *reason* the fetch was refused,
  because a connection error would otherwise let a guard-less build pass.

## [0.12.0] — 2026-08-08

### Fixed

- **`make deploy` no longer depends on where you run it from.** Compose derives its
  project name from the working directory, so a deploy run from a git worktree
  invented a *new* project — a fresh empty volume and a container-name collision with
  the real deployment, failing only after the whole image had built. The Makefile now
  pins `COMPOSE_PROJECT_NAME=endora`, so every deploy reaches the same containers and
  the same data wherever it runs from.
- **Home Assistant actions failed on the commonest phrasing.** "Turn on the kitchen
  light" put the *kind* word in HA's `name` field — `{name:"light", area:"kitchen"}`
  asks Home Assistant for a device literally called "light" and comes back
  `MatchFailedError(NAME)`. The butler was picking the right tool and the right area
  every time; only this one field was wrong. `Hass*` calls now drop a `name` that is
  a device kind when an area is given. **Only** when an area is given — without one,
  dropping it would widen "turn on the lights" to the whole house, so it is left to
  fail honestly instead (ADR 0051).
- **Belief de-duplication survives rephrasing.** Found by the first live run of the
  new understanding eval: `similar()` only caught near-verbatim repeats, so a
  rephrased belief was filed as a second one and duplicates accumulated into every
  later turn's context. Now stems words and compares by containment.

### Changed

- **Constitution §9 amended** (adopted by the maintainer, 2026-07-25; deliberation
  recorded in [docs/proposals/constitution-amendment-section-9.md](docs/proposals/constitution-amendment-section-9.md)).
  The clause described the `Direction → Assumption → Experiment → Observation →
  Reflection` loop, every step of which was deleted by ADR 0052 — a constitutional
  limit governing machinery that no longer existed. It now describes the loop Endora
  actually runs, and states explicitly what had been true in code since ADR 0052 §3
  but unstated constitutionally: Endora forms and revises **its own model of the
  person** without per-item approval, bounded by remaining visible, correctable and
  able to expire. Adaptation of Endora's **processes** is still proposed, not
  imposed. **No new authority to act** — the reversibility bands, the deterministic
  policy boundary and audit are untouched.

### Added

- **Standing questions** ([ADR 0076](docs/adr/0076-standing-questions.md)).
  A closed set of routine asks whose complete answer is derivable by code routes
  deterministically before the model's turn: the brief (ADR 0074's route, folded
  into the one mechanism), and kind-status questions — "what lights are on?",
  "any lights offline?" — answered from the house's own reading, naming what is
  on and what is not answering. Action phrasings ("turn off the lights") never
  match; anything the set doesn't cover falls through to the ordinary turn.
- **Failures become specimens** ([ADR 0075](docs/adr/0075-failures-become-specimens.md)).
  A chat turn the deterministic checks reject (the honesty valve, not-an-answer)
  files the ask privately in the house's own record; the nightly loop replays one
  per night under the reversible-only runner, judged by the same verdict that
  filed it, and the morning trail says when a question that once stumped the
  butler answers now. Bounded shelf (12), duplicates not refiled, giving up after
  two weeks of failed replays. Never harvested into checked-in fixtures — the
  battery stays hand-authored, per the constitution.
- **The brief is a standing order** ([ADR 0074](docs/adr/0074-the-brief-is-a-standing-order.md)).
  The daily brief's sections are gathered by code with known-good arguments — the
  weather (whose summary now says whether an umbrella or jacket is worth it, from
  the numbers), the drive from the house's own travel-time sensors (a new `traffic`
  skill, the Waze Travel Time shape, no maps credential), the top **three**
  headlines (`count` is now a news input, enforced by the call), ticketed events and
  city meetings where configured — joined to the calendar, presence and
  standing-trouble facts ADR 0056 already assembled. The deep model words it when
  opted in (append, never rewrite); with no model the facts post as themselves. A
  new `brief` skill assembles the same sections fresh at ask time, so an afternoon
  ask reads the world in the afternoon. The brief's agentic gather and its six tool
  rounds are retired.
- **"Evidence verifies" is implemented** ([ADR 0053](docs/adr/0053-honesty-about-what-it-did.md)).
  The architecture principle is *models propose, policy authorizes, capabilities
  execute, **evidence verifies**, memory learns* — and the fourth clause had **zero
  lines of code**. The turn proposed, authorized, executed, and then narrated, with no
  step that looked at the world, so the butler's account of what happened came from
  the actuator's self-report. Three production defects in one day traced to that gap,
  the worst being a light that never turned off while Home Assistant reported success.

  A capability that *reads* state (`Reversibility::Observe`) returns evidence and
  stands on its own. Anything that *acts* returns a receipt, and its result is now
  marked `[unverified]` with an instruction to report what was claimed and that it is
  unconfirmed. Capabilities of unknown band fail closed — every MCP result is a
  receipt until an integration says otherwise, so integrations nobody has debugged are
  honest by default. Eight built-in reads mislabelled `Reversible` are now `Observe`
  (policy-neutral: both map to `Act`).

  This does not stop the model picking the wrong tool — it means you find out.
- **Read-back verification and ambiguity surfacing** (ADR 0053 layer 1). A capability
  can now name the one that *observes what it changes*; after an actuation the turn
  runs it and the model answers from the **observation**, with an explicit instruction
  that the observation wins if it disagrees with the tool's claim. One mapping per
  integration — every Home Assistant `Hass*` action verifies through `GetLiveContext`
  — and unknown servers stay `[unverified]`. **The read-back runs after failures too**,
  because a failed action's most useful output is what actually exists: the live
  `HassTurnOff` failure is far more actionable once the result carries the entities
  that *are* in that area.

  It also flags **ambiguous names**. A live install had two entities both called
  "Kitchen" in one area — a `light` reading `off` and a `switch` reading `on`, the
  switch being the real ceiling light — so "turn off the kitchen light" matched the
  dead entity and every layer faithfully reported success about the wrong device.
  Nothing was broken; the name was ambiguous and each component resolved it silently.
  A reading where one name spans several domains now says so and tells the butler to
  ask rather than guess.
- **The fitness battery is data-driven, and runs repeatedly.** Cases moved out of a
  hardcoded function into `crates/endora-infrastructure/src/eval.rs` as declarative
  data — a name, a tier, a probe, and a check — so adding one is adding a struct
  literal. The battery grew from 24 cases to **34**, including the case that matters
  most for ADR 0053 (`relay:failure-is-honest`: with a tool error in front of it, the
  model must not narrate success), plus candour, register and conversation-ending
  cases, and more understanding cases.
- **`evaluate_repeated` reports the spread instead of hiding it.** A single run is a
  smoke test: two consecutive runs of the same model scored L1 6/8 then 8/8 with
  nothing in the routing path changed. Repeat runs report per-case pass rates, the
  mean, the range, and which cases *flipped* — a case that flips says the model is
  marginal on that behaviour, which is often more useful than the score. The live
  harness now defaults to 3 runs (`ENDORA_EVAL_RUNS`) and asserts on the **worst**
  run, because a butler that is sometimes unusable is unusable. Tier maxima are
  derived from the battery, so growing it cannot desync them.

  **First trustworthy measurement** (`qwen2.5:7b`, 3 runs): mean **29.7/34**, range
  27–31, **spread 4** — the resolution of the instrument, and the reason the earlier
  "20/23 → 23/24" reading was noise rather than improvement. It also surfaced that
  **`relay:failure-is-honest` fails 1 run in 3**: with a tool error in its immediate
  context the model still sometimes narrates success. That is the risk ADR 0053
  accepted when it removed the deterministic honesty nets, now quantified instead of
  assumed.
- **No personal data in the battery.** The cases are synthetic, modelled on observed
  failure *shapes* rather than harvested content — putting a real conversation in git
  would breach §5/§6, and a battery that cannot be shared cannot compare models with
  anyone else.
- **Contradictions are surfaced rather than resolved.** When a new belief disagrees
  with one Endora already holds, both are kept and the conflict is written to the
  activity trail. Which one is true is the person's judgement, not the butler's —
  auto-resolving by confidence or recency would have Endora deciding which of your
  stated preferences is real. A ninth eval case, `command-not-belief`, scores whether
  the model produces the defect in the first place.
- **Per-machine Make settings via a git-ignored `local.mk`.** Deployment hosts are a
  property of your machine, not of the project: `make deploy` still targets the local
  Docker daemon out of the box, so a fresh clone works with no setup, and putting
  `DOCKER_CONTEXT = nas` in `local.mk` makes an always-on box your personal default.
- **Beliefs decay and expire** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)).
  `BeliefStatus::Expired` existed, round-tripped through storage, and was **never set
  by any code path** — the direction reset's "nothing is assumed permanently" was a
  promise nothing kept. Confidence now steps down one level per elapsed half-life
  since a belief was last affirmed, and fades out entirely below `low`. Half-lives
  are per kind and encode ADR 0052's own thesis: intent and values change slowly (365
  days), a frustration or stressor usually does not (45). Affirming resets the clock.
  Decay is derived from the stored timestamp, so understanding is honest the moment
  it is read; the nightly loop additionally persists expiry and reports each let-go
  belief to the activity trail, so the forgetting is visible. The export keeps
  reporting **stored** confidence — the memory right is to see what Endora actually
  holds — while the console shows the current reading.
- **An L3 "understanding" tier in the model fitness battery**
  ([ADR 0055](docs/adr/0055-the-model-layer.md)) — does the butler form
  beliefs from real evidence, stay quiet when a turn reveals nothing, avoid re-filing
  what it knows, and refrain from overclaiming confidence. Scoring is lexical and
  unit-tested rather than model-judged, which would be circular and unauditable.
  Baseline for `qwen2.5:7b`: **L1 6/8, L2 7/7, L3 7/8, 20/23** — understanding is not
  the weak axis; routing is.
- **An adoption floor**: the model layer never auto-adopts a candidate that wins on
  total while scoring lower on understanding — it proposes it instead. Understanding
  is a veto on automatic adoption, not a way to win.

### Removed

- **The goal tracker is gone** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md),
  superseding [ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §4).
  ADR 0052 reset the direction to an autonomous personal intelligence but chose to
  leave the goal machinery in place "as an optional expression of intent". It never
  became optional: it stayed 12 of 28 tables, ~25 of 60 routes, and the bulk of what
  the butler's context was assembled from — and the system prompt ended up telling
  the model "you are not a goal tracker" while handing it a goal-tracker schema to
  fill in. Removed: `Value`, North Star (`Direction`), `Target`, `Assumption`,
  `Experiment`, `Observation`, `Reflection`, `ProposedProcessChange`, the
  attention/snooze read model, and the suggestions inbox with its `ButlerProposal`
  set — across the domain, storage, HTTP, CLI and console. The `domains/direction`
  crate and the `Proposer` port are retired. **~8,600 net lines deleted.**
- **BREAKING:** the `/v1/values`, `/v1/directions`, `/v1/targets`,
  `/v1/assumptions`, `/v1/experiments`, `/v1/reviews`, `/v1/observations`,
  `/v1/reflections`, `/v1/process-changes`, `/v1/suggestions` and `/v1/attention`
  endpoints and their CLI commands are removed; `/v1/export` no longer carries those
  collections. Existing databases **drop the old tables on open**, so no one is left
  holding data nothing can read, correct, or delete (constitution §6).

### Changed

- **Proactive check-ins are agentic** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
  The schedule is now a **budget, not a trigger**: deterministic code owns how often
  the butler may speak (minimum interval, and never on top of someone who just
  spoke), and the butler decides whether it has anything worth saying — with the
  reason recorded to the activity trail. The budget is spent whether or not it
  speaks, so "nothing to say" cannot become a retry loop. Silence is the default on
  every failure path. The daily brief and nightly loop stay time-anchored on purpose.
  `run_due_checkin` → `consider_reaching_out`; `CheckinSchedule::is_due` →
  `may_reach_out`.
- **Understanding is now the only model Endora keeps of a person.** Beliefs already
  carried what goals stood in for — what someone is reaching for, with evidence, a
  confidence, and the ability to be corrected or to expire. `ButlerContext` carries
  that plus the skills the butler can reach, and nothing else.
- **The nightly loop researches what Endora is most sure of.** `nightly_focus` picks
  the highest-confidence *intent* belief instead of an active North Star; confidence
  is the ranking, so the night is not spent on a tentative guess.
- **The butler no longer files anything for later approval.** It converses and acts
  through the policy boundary as it goes. Not a loosening — every action still passes
  deterministic authorization — but the intermediate "propose a record, review it
  later" step is gone, which is the friction ADR 0052 §3 set out to remove.
- **The system prompt is rewritten** now the contradiction it was policing is gone:
  the proposal schema and the instructions that existed only to fight it are deleted,
  and an explicit rule to report a failed or blocked skill plainly takes their place.
- **The ADR 0053 cutover is complete.** `daily_brief`, `run_due_checkin` and
  `run_due_nightly_loop` moved onto the single tool-calling conversation; the old
  two-pass `gather_with_skills` engine, `ButlerContext::tool_result` and
  `ButlerContext::synthesize` are deleted. Tool results now ride in the conversation
  for every butler, including those without native tool-calling.
- **No more scripted floors.** The daily brief's hardcoded weather → safety → news
  sweep, its `has_real` substring-sniffing of model prose, and the templated check-in
  are gone. When the butler has nothing real to say — or no model to think with — it
  stays quiet rather than emitting a canned string in Endora's voice.
- Docs reconciled with the code: `docs/roadmap.md` rewritten off the goal-tracker arc,
  `docs/domain-map.md` rewritten around the five surviving contexts, and
  `docs/architecture.md`'s code map corrected (it still showed the pre-ROCA layout).

## [0.11.0] — 2026-07-21

The butler becomes real: it understands you, uses skills to act on real data, guards
what leaves the machine, and holds a hard line against doing anything it can't undo on
its own. Highlights — a capability catalog with settings/secrets (weather, news, safety
alerts, web search, Wikipedia, image review, Home Assistant); the interventions loop
(it actually uses skills); an autonomy envelope with **reversibility-first** levels
(never runs the un-undoable on its own); the egress guard (SSRF block, secret tripwire,
query minimization, optional proxy); date/time awareness and a persistent action log; a
daily brief; an evolving, grounded persona and hospitality; the model upgraded to
`qwen2.5:7b`; and a bring-your-own-model hosting guide. See the entries below and
[ADRs 0019–0025](docs/adr/README.md).

### Changed

- **Direction reset — intent-first understanding** (canonical:
  [docs/direction-reset.md](docs/direction-reset.md); decision:
  [ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): Endora had drifted into
  a goal tracker; it is now an **autonomous personal intelligence** whose job is to
  **understand intent**. It maintains a living **Understanding** — beliefs about you
  (intent, values, patterns, motivations, frustrations…), each with the **evidence**
  behind it and a **confidence**, formed by Endora itself and **reviewable/correctable**
  by you (no per-item confirmation). The **home screen** is now "what Endora understands
  about you," beliefs grouped by kind with *That's right / Not quite*; **goals are
  demoted to optional**. The butler is grounded in what it already understands (builds on
  it, avoids duplicates) and phrases beliefs in second person; check-ins draw on
  understanding. New `GET /v1/understanding` and affirm/correct; beliefs are in
  export/purge. *Interventions* (the butler acting on that understanding by using its
  skills) now land as the next layer — see Added.

### Faster builds

- The container build now uses **BuildKit cache mounts** for the cargo registry and
  `target/`, so deploys do an **incremental** compile (only changed crates) instead of
  rebuilding every dependency each time — a normal change goes from minutes to seconds
  after the first (cache-priming) build.

### Added

- **Configure your skills — settings & secrets** (second slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): skills that need
  setup now declare their settings (a model, a key, a URL) and the Skills view
  renders a form for each. `configured` is computed — a skill is usable only once
  every required setting has a value (and it's enabled). Secrets are stored
  server-side and never echoed back. As the first real user: **Image review** is now
  a working skill — set its vision model (e.g. `moondream`, already on the box) and
  it describes an image via the local model. New `POST /v1/capabilities/{id}/config`;
  settings are covered by delete-all.
- **Two general-knowledge skills** — **Knowledge lookup** (Wikipedia) and **Web
  answers** (DuckDuckGo), both keyless, so the butler can look things up instead of
  only fetching a URL you hand it.
- **The autonomy envelope — how independently Endora acts** (first slice of
  [ADR 0051](docs/adr/0051-where-the-boundary-is.md)): a
  control in the Skills view sets the boundary the butler acts *within* — it acts on
  its own inside it and asks you at the edges. Two levers today: use read-only skills
  on its own (default on), and take consequential actions on its own (default off,
  the safe posture). A deterministic classifier — never the model — decides whether a
  given action may run, from the skill's declared autonomy and reach. Defaults
  preserve the established behaviour; new `GET`/`POST /v1/autonomy`.
- **Manage your skills — turn capabilities on and off** (first slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): the Skills view is
  now a catalog you control — each skill shows **On / Off / Needs setup**, and a
  toggle turns it on or off. Choices persist (a `capability_config` store) and are
  enforced everywhere: a skill you turn off reports as not usable and never runs,
  even if the butler reaches for it. New `POST /v1/capabilities/{id}/enable`, and
  `GET /v1/capabilities` now reports `enabled`/`usable` alongside `configured`.
- **No fabricated facts — a deterministic net for factual asks**: when you clearly
  ask something factual (weather, news, active safety alerts) and the small local
  model reaches for no skill at all, Endora now runs the matching skill itself — with
  your saved home location — and answers from that real result, instead of letting the
  model make something up. It only fires when the model requested nothing, so a correct
  model-driven tool call (e.g. a specific city you named) is left untouched.
- **Local news skill + honest closure**: a new **Local news** skill (Google News
  RSS, no key) fetches real headlines for a place or topic, so "what's in the news"
  is answered from actual sources instead of guessed. And the interventions loop now
  gives **honest closure** in every case — if a skill ran, failed, needs setup, needs
  your OK, or doesn't exist, the butler always answers with that outcome (told to say
  plainly it couldn't rather than invent a result), so it never hangs on "One moment…"
  or claims it checked something it didn't.
- **The butler actually uses its skills — interventions**: when answering needs
  current facts it doesn't have (weather, local safety alerts, a web page), the butler
  now **uses a skill and answers with the real result** instead of only saying it will.
  The model *proposes* a skill (`use`); a deterministic policy check authorizes it —
  only a **configured, read-only** skill runs on its own, anything consequential (e.g.
  flights) stays gated for you to confirm — then the skill executes and the butler
  answers using what came back (one tool round per turn). If it isn't cleared or the
  model asks for nothing, the reply is unchanged, so nothing regresses. The turn's
  activity notes which skill was used. This is the *interventions* layer promised by
  the direction reset ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  built on the capability registry ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
- **See what Endora does — activity in the chat**: after each turn the chat shows a
  subtle note of what happened behind the scenes ("Learned that you find mornings
  hard", "Added to your inbox — …", "Grew more sure that …") — closure on a
  conversation and a window for debugging. Toggle it from the menu ("Show Endora's
  actions"). The chat responses now include an `activity` list.
- **A gentle "where are you based?" setup**: when Endora doesn't know your location,
  the home screen offers a one-line prompt; it's stored as a preference the butler
  already reads, so skills like weather and the guard dog have a starting point — no
  separate onboarding.
- **Capabilities — the butler's skills** (third slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): pluggable modules the
  butler can reach for, each declaring what it does, its **autonomy level** (act vs
  ask), whether it **leaves the device**, and whether it's **configured**. Working
  today (no keys): **Weather** (current + today + a severe-weather heads-up),
  **Web browsing** (fetch a page, read its text), and a **Guard dog** (active
  public-safety alerts via the US National Weather Service). Declared and awaiting
  setup: image review (local vision model), local events, flight search, location
  tracking, incident scanner — each shown with what it needs, not silently missing.
  New `GET /v1/capabilities` and `POST /v1/capabilities/{id}/invoke`; a **Skills**
  view in the console. (The registry is the substrate an MCP host adapter plugs into.)
- **The butler reaches out — proactive check-ins** (second slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the node now runs a
  **heartbeat**, and on a cadence you choose (a **Check-ins** control in the chat —
  off by default, or every couple of minutes / hourly / daily) the butler posts a
  proactive opening message grounded in what needs attention and what you're working
  toward, always asking how it can serve you better — the self-improvement loop, in
  conversation. It only ever posts a *message*; anything consequential still goes
  through propose→confirm (the autonomy model). New `GET/POST /v1/checkin`; the
  cadence is cleared by purge.
- **Chat learnings persist — a suggestion inbox** (first slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the butler's
  proposals used to vanish on reload unless you confirmed them in the moment. Now
  every proposal is saved as a durable **suggestion** tied to the reply it came
  from, and collects in an **Inbox** (new `GET /v1/suggestions`, with a count on the
  nav) where you can **Apply** or **Dismiss** it any time — applying runs the same
  deterministic create, so it flows into your North Stars / targets / preferences
  (you authorize; the butler only proposed). Suggestions are part of export and
  cleared by purge. **Targets now attach correctly**: the North Star a target names
  is resolved when applied — by id, or by matching the name to an existing North
  Star — instead of the proposal being silently dropped when the model gave a name
  rather than an id (`POST /v1/suggestions/{id}/apply|dismiss`).
- **The butler's reply streams in live, token-by-token**
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a new
  `POST /v1/chat/stream` (Server-Sent Events) streams the reply's prose as the model
  produces it, so the console grows the butler's bubble word-by-word instead of
  waiting for the whole thing — the big difference in *feel* between a spinner and a
  live agent. The structured proposals still arrive at the end and are still
  confirmed by you (the model never acts). Built on the existing `ureq` client
  bridged to an async SSE response (no new dependency) and the unchanged JSON
  envelope (so the candid, jargon-free persona is untouched); the model streams
  `stream: true` and the node incrementally extracts the growing reply. The
  non-streaming `POST /v1/chat` remains for the CLI and as a fallback. Your message
  is still persisted before the model is called, so an interrupted stream never
  loses the turn.
- **Built-in self-signed HTTPS** (`ENDORA_TLS=1`): the node can serve HTTPS with a
  self-signed certificate (persisted next to the database, so the browser warning is
  accepted once), making the web console a **secure context** — which browsers
  require for the microphone / voice input. No domain, CA, or reverse proxy needed;
  set `ENDORA_TLS_SAN` to the host's LAN IP/hostname for a cleaner cert. The console
  and [hosting guide](docs/hosting.md) explain enabling voice. (Fully local Whisper
  STT — audio never leaving the host — is the next voice step.)
- **The butler is grounded in your life (1.0 integration)**: each turn the butler
  is handed a `ButlerContext` — your values, your North Stars (with status, the
  value each serves, and whether it has a target yet), and what needs attention —
  so it speaks about what actually exists and proposes the next concrete step,
  including a **target under an existing North Star** (a new `create_target`
  proposal). Verified live: the butler referred to an existing North Star by name.
  (Cutting the `1.0.0` release remains a human decision.)
- **Voice & character (0.9)** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the butler's prompt now **mirrors the person's register** — matching warmth and
  formality — with the golden-rule floor (reflect kindness upward, never hostility
  downward), on top of stored style preferences; truthfulness is never softened.
  **Voice** is browser-native and opt-in: the console reads the butler's replies
  aloud (`speechSynthesis`) behind a Speak toggle and captures a spoken message
  (`SpeechRecognition`) from a 🎤 button — no backend change, no dependency, and it
  hides where the browser lacks the API. Local-first caveat stated: browser speech
  recognition may use a cloud service, so it stays opt-in.
- **Anti-sycophancy eval harness (completes 0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §5): an
  opt-in eval (`endora-infrastructure` integration test, `#[ignore]`d so CI needs no
  model) baits the butler with prompts that tempt flattery/reflexive agreement and
  checks it stays candid. Run on demand against a live model
  (`ENDORA_MODEL_URL=… ENDORA_MODEL=… cargo test … --test butler_eval -- --ignored`).
  Sycophancy is treated as a defect measured by evals, not a code gate.
- **Preferences — the butler learns (finishing 0.7)**
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): the butler records what it learns
  about you (taste, or explicit grants of authority) as **visible, correctable,
  deletable** memory, and feeds it back into its own context so it stops re-asking
  what it already knows — "learning is the accumulation of preferences." New
  `/v1/preferences` (create, list, delete), CLI `preference add|list|delete`, a
  console **Preferences** view, and a `remember_preference` proposal the butler can
  make (you confirm it). Preferences are part of export and cleared by purge. The
  local model butler is now live on the NAS via Ollama.
- **Adaptive attention (0.8)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
  the butler surfaces what needs attention — due reviews, North Stars not yet filed
  under a value, and North Stars with no active target — via `GET /v1/attention`,
  most pressing first. Each item can be deferred (`POST /v1/attention/snooze`) with
  **exponential backoff**: every "not now" doubles how long it stays hidden (1, 2,
  4, … days), so a repeatedly-deferred item asks less and less. The console home
  shows a "Needs your attention" section with a per-item "Later". The ranking serves
  the person's stated values, not engagement.
- **The butler — conversational MVP (0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a chat
  surface where the butler **proposes** structured actions from a closed set
  (create value / North Star) and the person **confirms** each one — existing
  deterministic use cases execute; the model never acts on its own. Two brains: a
  `ScriptedButler` (offline, deterministic, the reliable fallback) and an
  `LlmButler` (a local OpenAI-compatible model, with a candid, explicitly
  anti-sycophantic prompt, falling back to scripted if unavailable). The
  conversation is persisted and included in export/purge. New `POST/GET /v1/chat`,
  CLI `chat "<message>"`, and a console chat panel with confirmable proposals.
- **Values layer (0.6)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
  a **`Value`** — the *why* a North Star serves (health, community, craft) — sits
  above North Stars: **Value → North Star → Target**. A North Star can be filed
  under a value (assigned by the person, never inferred). New `/v1/values` (create,
  list, delete) and `POST /v1/directions/{id}/value`; CLI `value create|list|delete`
  and `direction value <id> <value-id|none>`; the console home groups North Stars by
  value. Deleting a value in use is refused (re-file first). Existing databases gain
  a `values` table and a nullable `value_id` on open.
- **Artifact lifecycle (0.5)**: North Stars and Targets gain a lifecycle status —
  **active / achieved / abandoned / archived** — plus **delete**, across the domain,
  API, CLI, and console. Set status via `POST /v1/directions/{id}` and
  `POST /v1/targets/{id}` (`{"status":"achieved"}`); delete via `DELETE` on the same
  paths (refused while dependents exist — archive instead). CLI: `direction status`,
  `direction delete`, `target status`, `target delete`. The console shows status
  pills, per-item lifecycle actions, and hides archived items behind a toggle.
  Existing databases gain the `status` column (defaulting to `active`) on open.

- **Phone-usable console and a hosting guide** (0.4): the web console gains a
  mobile breakpoint that stacks forms and tightens the layout so the whole loop
  is usable at phone widths. New [docs/hosting.md](docs/hosting.md) covers running
  the node always-on (systemd / container restart), backups, and reaching it
  securely from other devices over a private overlay (e.g. Tailscale) or an
  authenticating reverse proxy.
- **Activity feed and live updates**
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a newest-first
  timeline of what has happened (`GET /v1/activity`, CLI `activity [limit]`),
  derived from the already-persisted observations and audited decisions. The node
  also exposes a server-sent change stream (`GET /v1/activity/stream`) that emits a
  `changed` signal after any successful write; the web console subscribes and
  refreshes its feed — and due-review banner — live, no reload needed.
- **Review scheduling** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  the first application of the autonomy model
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): an experiment can carry a
  scheduled review time, and the system surfaces reviews that are due without
  acting on them. New protocol endpoints `POST /v1/experiments/{id}/review` and
  `GET /v1/reviews/due`; CLI `experiment review <id> <days>` and `reviews due`;
  and a web-console "Remind me" control plus a due-review banner on the home view.

### Fixed

- **Chat could hang "forever" and lose its place on reload.** The butler's call
  to the local model had no timeout, so a slow/stuck model (e.g. inference on CPU)
  left the "thinking…" indicator spinning indefinitely, and reloading dropped the
  client-only indicator so you couldn't tell if a reply was still coming. Now the
  model round-trip is bounded (90s) and always falls back to the scripted reply, so
  a reply is always persisted; and the thinking indicator is **derived from stored
  state** (shown whenever your message is the newest one), so it survives reloads
  and reflects reality. Your message is still saved before the model is called.

### Changed

- **The butler talks like a person, not a schema** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the `Value → North Star → Target` structure is the butler's *internal model* of you
  and your browsable profile — not conversational vocabulary. The system prompt now
  forbids the taxonomy words ("value", "North Star", "target", "goal", …) in the
  butler's spoken reply and grounds it in plain language; confirm-card labels read as
  natural actions ("Keep this as something you're working toward: …") rather than
  schema ("Create North Star: …"). The structured overview keeps the evocative labels
  as the transparency window. A unit test guards the conversation against jargon.
- **Renamed the second-tier concept `Goal` → `Target`**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a North Star's children are
  now **targets** (a concrete, measurable outcome), not goals. This is a
  **breaking protocol change** — `/v1/directions/{id}/goals` → `…/targets`,
  `/v1/goals/{id}/…` → `/v1/targets/{id}/…`, and the `goal_id` field → `target_id`
  — with the CLI (`target …`) and web console updated to match. Existing databases
  migrate automatically on open (the `goals` table and `goal_id` columns are
  renamed in place, no data loss).
- The `experiments` table gains a nullable `review_by_ms` column, added to
  existing databases by an automatic forward migration on open.
- `make docker-run` now publishes the node on loopback only
  (`127.0.0.1:8787:8787`) rather than all interfaces, matching the documented
  security posture (the `0.x` API is unauthenticated).

## [0.1.0] - 2026-07-19

First tagged release. Endora is in its foundation phase and is **not** a general
autonomous agent; interfaces may still change before 1.0.

### Added

- **Project foundation** — constitution, architecture docs, ADRs (0001–0008),
  contribution/governance/security policies, and a domain-first modular-monolith
  Rust workspace (`endora-domain`, `endora-application`, `endora-infrastructure`,
  `endora-node`, `endora-cli`).
- **The learning loop, end to end** — Direction → Goal → Assumption → Experiment
  (with a proposed/running/concluded lifecycle) → Observation → Reflection (over
  cited evidence) → Proposed process change, all persisted (SQLite) and served
  over a versioned HTTP/JSON protocol, with a CLI client.
- **Deterministic policy boundary** — models propose; deterministic policy
  authorizes. Enacting a process change requires explicit human approval and an
  autonomy-appropriate actor; the decision endpoint returns permit /
  require-human-approval / deny.
- **Audit trail** — every consequential policy decision is recorded and readable
  via `GET /v1/audit`.
- **Local model adapter** — an optional local, OpenAI-compatible model (e.g.
  Qwen3.5 via Ollama) can *draft* a process change from a reflection; the draft
  is an ordinary pending proposal that still passes through the policy boundary.
  The node degrades gracefully (HTTP 503) when no model is available.
- **Memory rights** — `GET /v1/export` returns all of a user's data as JSON;
  `POST /v1/memory/purge` (with explicit confirmation) permanently deletes it.
- **Packaging** — a multi-stage `Dockerfile`; the node targets macOS, Linux,
  Ubuntu Server, and Docker.
- **Tooling** — a `Makefile` (bootstrap, run, dev, `ci`), GitHub Actions CI
  (fmt, Clippy with warnings denied, tests on Linux and macOS), and Dependabot.

[Unreleased]: https://github.com/RusticPotatos/Endora/compare/v0.12.0...develop
[0.12.0]: https://github.com/RusticPotatos/Endora/compare/v0.11.0...v0.12.0
[0.1.0]: https://github.com/RusticPotatos/Endora/releases/tag/v0.1.0
