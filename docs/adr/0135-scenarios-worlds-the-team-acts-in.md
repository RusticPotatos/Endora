# 0135 - Scenarios: worlds the team acts in, which only the world closes

## Status

Accepted, 2026-10-20, by the owner, as phase 5 of the plan towards the Holodeck (the phase 0
audit of 2026-10-19). Prepared with AI assistance (Claude). Builds on
[0121](0121-games-in-the-office.md)'s chess (a world kept by HQ, its rules in code, played through
one tool), on [0113](0113-projects-budgets-and-reports-to.md) as amended on 2026-10-20 (a
project lists the tools its work is given), and on [0114](0114-evidence-acceptance-criteria-and-the-tester.md)
(a model's word is not evidence). Built: everything below, with one scenario, the lighthouse.

## Context

The Holodeck is the team acting in simulated worlds, to try configurations of members and brains
against each other on work whose outcome is known. Its first gate (G3 of the owner's roadmap):
a tiny world that resets and replays, where a model's claim can never change the world. HQ
already had most of it: tasks, turns, briefings, budgets, events and replay know nothing of the
home lab, and chess showed a world kept in HQ's store and changed only by a tool checked in code.
What it lacked was a world that ends a task by itself, a record of every action that is never
pruned or changed, and a way to give a turn that world's tool and nothing else.

## Decision

1. **A scenario is a task.** The owner starts one (`POST /v1/scenarios {project, member, seed}`,
   through the node's `/v1/hq/scenarios`) in a project, for a member. Its description says
   what the world is and what can be done in it. Starting it again with the same seed is its
   reset: a new task, the same world.
2. **Its project gives it the scenario tool alone.** A scenario starts only in a project whose
   list of tools (0113, amended) is exactly `["scenario"]`, so its turns are offered their own
   four tools and `scenario`, nothing of the node's (no lab, bridge or repo tool) and no shell,
   and the three layers that enforce the list refuse anything else.
3. **The world is a pure function of its seed and its log.** Its rules are in the domain
   (`domain/scenario.rs`), deterministic from a seed (splitmix64, as chess's matches). Its state
   is never stored: it is the seed's world with the log's accepted actions replayed in order.
4. **The log is append-only.** Every action a member takes is kept in `scenario_actions` (which
   turn, which member, the action, whether the world accepted it, and what the world said),
   refused ones too; triggers refuse any change or deletion, as for the ledger and receipts. A
   refused action changes nothing and costs nothing in the world. Each action is also an event,
   `scenario.acted`, for the feed and replay.
5. **Only the world closes it.** The rules say when it is solved or failed; HQ then closes the
   task in code (done, met criteria; or dropped, blocked for good), posts the world's words in
   its thread, and never asks a tester or a lead to judge it. A member's `set_status` on a
   scenario's task is refused, whatever it claims.
6. **The owner reads it step by step** (`GET /v1/scenarios/{task}`, through the node): its seed,
   where it stands, how it ended and why, and its log, who did what, what was refused and what
   the world said.

The first scenario is **the lighthouse**: its lamp is dark, for a cause the seed hides (the bulb
or the fuse). Three actions (`inspect`, `test_circuit`, `replace_part`), each with a precondition
and a cost in minutes, an hour to light it, and two ways to do so in time.

## Amendment (2026-10-20): Silent Station, a crew in one world

Phase 6 of the plan, and the roadmap's G4: a mission a crew plays end to end, with consequences
the world enforces, not a scripted chat. A research station has stopped transmitting; the
crew must find out why and restore contact before its life support runs out, without hurting
anyone (`domain/station.rs`).

- **The world.** The hidden cause is always a burnt-out comms relay, with an anomaly the seed
  picks: an ion storm on the power junction (routing power through it before it is analysed
  hurts the engineer) or a magnetic field (harmless, but it scatters a call). Seen from outside,
  the array looks intact: misleading, not arbitrary. Six actions, `scan_station`,
  `inspect_comms`, `analyze_anomaly`, `route_power`, `send_probe` and `attempt_contact`, each
  with a precondition and a cost in ticks and the ship's energy. Two ways succeed (repair the
  power path, or place a probe as a relay once the anomaly is understood); the ship's energy
  pays for one of them, not both.
- **Outcomes, scored.** Success (contact, nobody hurt: 100 less 5 a tick, at least 50),
  partial (the cause found, contact not made before life support ran out: 30), failure (someone
  hurt, or time out with the cause unknown: 0). A mission the owner stops is dropped as any task.
- **A crew of three.** The owner starts it for three different members (`kind:
  "silent_station"`, `crew: [captain, science, engineer]`). The captain's task is the mission;
  science and the engineer each have a part of it. All three act in the mission's one world and
  one log, each only as its role: the captain sends the probe and makes the call, science scans
  and analyses, the engineer inspects the comms and routes power. An accepted action wakes the
  rest of the crew on their own parts; that is bounded, as every accepted action spends the
  station's time. The world closes the mission and its parts together.
- **The owner's view** of a mission adds its state (power, life support, energy, hazard, the
  probe, contact), its score, its crew by role, each step's role, and, once it is over, the
  anomaly.

## Amendment (2026-10-20): trials, conditions compared

Phase 7 of the plan, and the roadmap's G5: repeated trials whose records can be compared. A
**trial** is one scenario played under two conditions or more, each condition on the same
seeds, so that one thing changes between them (who plays, and so on which brains) and the
world, its version and its seeds do not.

- The owner starts one (`POST /v1/trials {name, project, kind, conditions: [{label,
  members}], seeds}`, through the node's `/v1/hq/trials`): every condition on every seed, at
  most 24 runs, under a name used once. Each run's configuration (world, version, seed, and
  each player's role and brains as they were) is kept as the run starts, with the brain its
  turns will really run on as the engine places them (`turn_brain`): the first live trial
  recorded a reviewer as "local", its work brain, while a tester's work is the Mac's (0131).
- The comparison (`GET /v1/trials/{name}`) is summed from the record, never from a model's
  account: per condition, runs done, not done and unfinished, mean score and time over the runs
  that ended, actions refused, runs in which someone was hurt, turns, turns that changed
  nothing, cost, and the brains the turns really ran on; then every run with its own; and a
  short report in Markdown to keep or share, which says that a few runs show a difference,
  not a reason. Each run's step-by-step log is the scenario's own view.
- Not measured yet: hand-offs between members, and the owner's interventions.

## Amendment (2026-10-20): chess as a world, and what the first live run taught

The owner wanted to see a chess game move by move, which model played what, and the illegal
moves; none of it was kept, and no game had ever been played (moves came only in downtime
turns, nine in all). Chess is now a third world (`domain/chess_match.rs`, by the rules of
0121's chess): the owner starts a match between two different members (`kind: "chess"`,
`players: [white, black]`); white holds the match and black a part of it; every move tried
goes in the log, illegal ones and moves out of turn too; a legal move ends the mover's turn
and wakes the other side; a side that looks on the other's move is told its turn has ended;
the rules end it (mate, stalemate, repetition, too little material) or the move limit does,
at 60 half-moves, as a draw. Its view adds the moves as written down, the board (FEN and
rows), the result and why, and each side's legal and illegal moves (`by_role`, which a trial's
runs carry too, so two brains can be compared move for move, colours swapped between
conditions). Office chess in downtime is unchanged.

From the first live run: the world's last word now says the task is closed and the turn has
ended, and the worker ends any turn HQ says has ended (three of five turns had gone on to
`set_status`); a trial records the brain each player's turns will really run on
(`turn_brain`), since a tester's work is the Mac's (0131), whatever its own work brain.

## Amendment (2026-10-20): what the first live mission taught

The first live Silent Station (task 710) found three holes. The crew routed power and then
sent the probe as well, leaving the ship 5 energy: every action was refused, no time passed,
and the mission could never end. **A ship that can pay for no action ends the mission**, as
partial when the cause was found and failure otherwise. Its parts, waiting for their world by
design, looked stuck: the stuck-work guards blocked them and the ladder woke the project's lead
to decide, twice, on Claude. **The stuck-work guards and the ladder leave a scenario's tasks
alone**: their world wakes their players. And the lead, woken on a part, acted in its role.
**A role is its task's member's**: the scenario tool is offered to, and acts for, that member
alone. A world that has ended with its tasks open is closed by the tick.

Then the live match stalled: black's turn on the Mac ran past its time and was stopped, the
wake that would have brought it back had been folded into that turn, and, with the guards now
leaving scenarios alone, nothing woke it again. **A world whose players have no turn coming
and that has sat idle ten minutes has them woken again** (a match's side to move; otherwise
every player still at it), **three times at most with nothing done; then the world abandons
it**, dropping its tasks and saying why. Any action resets the count.

*Amended 2026-10-28:* a match is not abandoned. Live, Local claimed a checkmate that was not
there and stopped moving, so the game went unrated: stopping was a way out of a game. **The
side to move forfeits** (`forfeit <side>` in the log, on its last turn) and the other side
wins. **A side may also resign** (the scenario tool's `resign`, kept as `resign <side>`). Both
replay from the log like moves, so a league game ended either way is rated.

## Amendment (2026-10-20): a trial may vary the briefing

The first Silent Station trial varied who captains, and three of four missions ended the same
way: the engineer routed power while the captain sent the probe too, and the energy for the
call was gone. The crew acted side by side, with no plan. A trial's condition may now carry a
briefing (`brief: "plan_first"`, for a mission), kept in each run's configuration, so the crew
and the seeds stay fixed and the briefing is what changes. Plan first is told and held: the
captain is told to post the crew's plan (the scenario tool's `plan`, with its text: who does
what, in which order, and which one way to restore contact) before anyone acts, and the others
to wait for it and do their part; the world refuses every action until a plan is posted. The
plan is free, every member's look shows it, the crew is woken to it, and it is kept in the log
(`plan <text>`), so the mission replays. Its mark is `silent_station_plan`.

## Amendment (2026-10-25): the model league

The owner wants the models compared at a game that tests skill, every one against every other,
in a table that ranks them: "everyone should play each other at least once". Tokens may be
spent on it when there is no work. A game that tests nothing (ping-pong and pool, played in code
from a seed, ADR 0121) spends none.

- **The owner starts it** (`POST /v1/league {project, brains}`, through the node's
  `/v1/hq/league`): two brains or more by name, in a project that holds scenarios. A Claude
  plays only from a **capped** project: the cap is what the league may spend in a month, and a
  Claude side whose project has no cap left waits, as any judgment out of budget does.
- **Each side is pinned to its brain**, whoever sits at the board: two free members play it, and
  their turns on it run on the side's brain alone (a Claude as judgment, paid from the league's
  project; the Mac waiting for the Mac; the house's model on the NAS). So a game measures the
  model, not the member's role or its own brains.
- **Seated by what a role may think with.** The relay lets a role think only with its own
  Claudes (0111), so a side is played only by a member whose role may use its brain (Sonnet is
  never a researcher's). Opus is the CEO's alone and the CEO does not play, so the league
  refuses it.
- **One game at a time, when nothing else is**: the next game starts only when no world is being
  played, no scenario waits to run again and two members are free (no turn, no open task, no
  routine due). The pair that has played least goes next, so every pair meets before any meets
  twice; a rematch swaps the colours.
- **The table is Elo**, from 1200 with K 32, summed from the finished games in the order played
  (`domain/league.rs`): arithmetic over the record. A game abandoned with no result is not
  rated; since 2026-10-28 a match is forfeited instead (above), so it is. The Holodeck card lists the brains ranked, one line each, how many pairs have yet to
  meet, and the games, each opening its board and log.

## Amendment (2026-10-30): the other worlds retired, chess kept

The owner decided on 2026-10-28, and confirmed on 2026-10-30, to retire the lighthouse, Silent
Station, the trials and the owner's re-run of the Holodeck (the queue that started every
scenario again). The owner prefers chess; the other worlds cost turns and code and were not
being used. What stays: the chess world with its resigning and forfeits, the model league (start,
tick, reset, ratings, and its pause), and the live board on the Holodeck card, stepped through
move by move. The office chess board
(0121) is a separate thing and is untouched.

- **Nothing starts them any more.** Their domain modules (`domain/scenario.rs`,
  `domain/station.rs`, `domain/trials.rs`), the engine's trials, the re-run queue and its tick,
  their actions in the `scenario` tool, `POST /v1/trials`, `GET /v1/trials/{name}`,
  `POST /v1/scenarios/rerun` (and the node's `/v1/hq/...` passes) and their console screens are
  gone. `POST /v1/scenarios` starts a chess match alone and refuses any other kind, saying why.
- **History is kept, not deleted.** No migration touches the store. A retired world's rows
  (its task and parts, its log of actions, its `scenario:<task>` mark, a round's label, a
  queued re-run, a trial's record) stay where they were. Reading the Holodeck passes over them:
  a mark that is not chess is read as no world, so they are left out of the list, their page
  is not found, the tick neither closes, wakes nor restarts them, and an old one left open
  holds the league back no more than any other task does. Such a task is still left alone by
  the stuck-work guards, as a world's task was.
- **One game at a time** now means only that no match is being played (and the league's project
  is not paused); the card's note that the league waits on queued scenarios goes with the queue.

## Consequences

- A scenario costs nothing on the house's models, and its project may be capped at $0.
- The log grows with every action and is never pruned: small, and the record a trial compares.
- What a scenario does not have yet: a panel on the owner's screen (the owner reads it through
  the API), more than one world, and the trials that compare configurations (phase 7).

## Alternatives considered

- **Keep the world's state in a table.** Rejected: two records of one world can disagree; a
  state derived from the log cannot, and replay is then the same code as play.
- **Let a tester judge the outcome.** Rejected: the point of a simulation is that the outcome is
  known; a model judging it brings back what the roadmap's G3 rules out.
- **Log only the accepted actions.** Rejected: which actions models try and the world refuses
  is what a comparison of brains most wants to see, and chess lost exactly that.
