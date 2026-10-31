# 0119 — The open office

## Status

Amended by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Amends
[0118](0118-elastic-staffing-and-notebooks.md) (downtime, and what a downtime turn may write in a
notebook) and [0102](0102-endora-hq.md)'s office view (the walled wing and the four anonymous
desks are gone). Built: the open floor on the HQ screen, the office as data (`office_items`,
proposals, votes, history), the `office` tool, the downtime wake and its scheduler, the owner's
undo, reset, veto and allowance. The owner's scenarios in
`domains/team/features/the-office.feature`.

Amended 2026-10-19 by [0131](0131-the-two-kinds-of-turn.md): a downtime turn is work on the
house's own model, which costs nothing, so **every** member who is at work qualifies for it,
whatever it judges with, and nobody's downtime is paid. The owner's allowance for paid members'
downtime (`downtime_paid_cents_per_day`, its card line and its route, below) is removed.

## Context

The owner, looking at the office on their phone, in their words:

- "Huge empty space. A lab of computers. It doesn't feel right."
- On why it matters: "your systems are designed the way the org is structured" (Conway's law).
  "If we want the teams to be agile, working as a startup, the teams should look, work, interact
  as one. It's ok for them, during down time, to intermingle."
- On the objects: "the objects are all fixed. Decorations and plants can't be moved. Would like
  the agents to decide themselves to decorate, change things, plan things as a group.
  Especially if we have down time. Let them do something."

The office drew four anonymous desks across the middle, a walled room for the engineering wing,
and every plant, sofa and lamp at a coordinate in the screen's source. Nothing about it said who
worked with whom, and nothing in it could change without a code change and a deploy.

## Decision

**Owner decisions, 2026-10-18.**

1. **Pods by team, on an open floor.** The middle of the floor is one **pod** for each team, side
   by side with no walls, spread across the floor when they fit in one row: a team is a lead
   and those who report to it, up the chain (the same
   rule as the roster; members with no lead above them but the CEO join the first pod). A pod is
   a cluster of desks, two to a row, one for every member of the team, labelled as the roster
   labels the team ("Ops · Ada", "Engineering · Kit"). Between the two desks of a row is an
   aisle where their members sit, each on a chair at its own desk's end by its computer. The layout is computed from the snapshot
   on every refresh: a hire grows a pod, a third lead makes a third pod, and a team of more than
   six goes three to a row. A member on furlough keeps its desk, drawn empty with a small "away"
   card, after those at work. The walled wing and the four anonymous desks are gone; the board
   room, the server closet with the burn gauge, ping-pong and the front door stay. **Mara keeps
   her own office, with the door drawn open.** A kitchen and lounge, with a couch, a coffee
   machine and a table, replace the wing's room and are everyone's.
2. **Movement shows who works with whom.** A working member sits at its pod desk. When its
   running turn's task is in another team's project, it walks to that project's lead's desk (to
   Mara's door when she leads it); the tester on someone's task walks to whose it is; a member
   whose question goes to someone walks to them. An idle member drifts between ping-pong, the
   kitchen's chairs, the tap, the couches, the coffee machine and what else stands on the floor,
   picking a place at random, so the teams mix; two at the table's ends play. The walking is a
   search over the floor's cells round desks and furniture, and through the open doors of the
   two rooms people walk into, nowhere else. With the device's reduced-motion setting on,
   nobody drifts.
3. **The office is data.** An `office_items` table holds what the team may move, from a fixed
   catalog of sixteen kinds (plant, cactus, fern, palm, poster, lamp, rug, couch, whiteboard,
   coffee machine, fish tank, bookshelf, bean bag, string lights, clock, art): its kind, its
   tile on the 32 by 20 grid, who placed it (a member, `endora` or `owner`) and when. Desks,
   rooms and walls are the layout's, not items. The table is made if missing and seeded once
   from a default layout (the old decorations placed afresh on the new plan: plants, the cactus,
   the shelf, the lamp, the whiteboard, the couches, the coffee machine, the posters, the
   picture and the wall clock); what the team removes later does not come back. The snapshot
   carries the items and the catalog, and the screen draws them from data. Ping-pong and the
   kitchen table are fixtures of the layout, not items: the catalog is fixed, and neither is in
   it.
4. **Placement rules are pure, and one table.** An item is inside the floor; a floor thing is
   not in the back wall and a wall thing hangs only on the free wall; not on a door or the step
   in front of one; not in the band the pods stand in, whatever the teams are; not in a walking
   lane; not in a room or on fixed furniture; not on another item; at most 40 items on the floor
   and at most 2 fish tanks (a fun limit). The limits per kind are one table in the domain
   (`office::CATALOG`). The band is reserved as a whole, so the rules need not know the teams.
   Each refusal says which rule.
5. **Deciding together.** Every role has the team tool `office`: `propose` (one to five
   changes, each place, move or remove, and why), `react` (yes or no, once) and `look` (the items
   by number, and what is open). A proposal is checked by the rules when made, and again when it
   would carry; a change that no longer fits drops the whole proposal. It is said in the activity
   feed ("Dee proposes: move the fern to the lounge; add a fish tank in the commons — 'the
   kitchen needs life'") and shown in the Office card on the HQ screen. It carries on more yes
   than no with at least two yes (the proposer's own is counted), or after six hours with no
   "no"; it is dropped on more no than yes, or after 48 hours. A vote is one a member; the
   proposer's yes is its own. A carried change is made, logged (`office.changed`, who and what),
   and kept so the owner can undo it. **A proposal is not work:** it is not a task, not a card
   on the board or the owner's desk, and not on the scorecard. The "#office" thread is the
   proposals themselves, with their votes and reasons, in the Office card and the feed: a
   standing task for it would have been a task that is never done and every member could claim.
   A member's reason is text, escaped wherever it is shown.
6. **The owner is the landlord.** In the Office card: **Undo last change** (the floor goes back
   as it was before the newest change, one step at a time), **Reset the office** (the seeded
   layout; it can itself be undone) and **Veto** on any open proposal. Through the node, in the
   audit log, as every owner action is. The team never needs the owner to approve a decoration,
   and the owner can always take it back.
7. **Downtime.** A new wake reason, `downtime`. Once a day (the owner's day), at most one member
   is woken for a downtime turn, and only one such turn runs at a time. A member qualifies when
   it is at work (not paused, not on furlough), is not the CEO, has no live turn, holds no open
   task and has no routine due, has not had downtime today, did not finish a downtime turn
   within the last twelve hours, and thinks with the house's model, which is free. The twelve
   hours are what "never two in a row" means here: a member that had one just before midnight
   does not have another just after it. A member who has had none longest goes first.
   **Paid members are never woken for downtime** unless the owner sets
   `downtime_paid_cents_per_day` (default 0): then paid members' downtime turns may spend that
   many cents a day **together**, reserved from what is left of it and of the pool's day, and
   charged to the **pool, never to a member's share**, by the path an owner's own request takes.
   Paid members have the `office` tool in ordinary turns all the same and may react while they
   work. The setting is in the Office card ("Paid members' downtime: $0.00 a day — change"),
   from `POST /v1/office/downtime`, at most what the pool's day holds. Downtime turns do not
   keep a member from being offered for furlough, and are not on the scorecard: they are not
   work.
8. **What a downtime turn is told, and may do.** It has no task. Its briefing says it is
   downtime; lists the things on the floor with their numbers and rough places (and the size of
   those bigger than a tile), how many there are of each kind and which kinds there are none of,
   what the member placed itself, the open proposals and how it voted, what it may place and the
   rules, and suggests, if something would make the office more the member's own (its notebook
   may say what it likes): react to a proposal, propose one small change, or note an idea in its
   notebook (the tools of [0118](0118-elastic-staffing-and-notebooks.md); a note rests on
   evidence, and the briefing names the latest event to cite); leaving the office as it is is a
   good turn too. *Amended 2026-10-20:* the sizes, the counts, the member's own things and
   "leaving it is a good turn" were added after live downtime turns, offered only things to do,
   each proposed the same thing (a plant), aimed at the rug's own tiles and slid one off when
   refused. It may not make work: `create_task`, `hand_to`,
   `propose_project`, `propose_goal`, `request_hire` and `request_staff` are refused in a
   downtime turn, with the reason. Its label is the ordinary label of the turn: an `office`
   proposal or vote is a change through HQ.
9. **A switch for it.** The engine's downtime is a setting the service turns on (`Settings`);
   the scripted demo and the engine's older tests leave it off, so an idle scripted team does not
   wander.

## Consequences

- The office on the screen says how the team is organised, and moves with it; a team's pod is
  its own, a visitor to it is someone working with it. It is not a planning tool.
- Decorating costs nothing on the house's model, and cannot cost more than the owner's allowance
  on Claude, which starts at none.
- A bad proposal cannot block a desk or a door, however it is voted: the rules are the
  enforcement, not the model.
- New data on the live store: four tables (`office_items`, `office_proposals`, `office_votes`,
  `office_history`), made if missing when HQ opens the store; no row changes, and the seed is
  made once.
- The pods stand in a band six tiles deep. A team of more than nine members, or more teams than
  the band holds in two shelves, will have desks sharing the room; the rules still hold, since
  they reserve the band, not the desks.
- Pods are laid out on the screen, not in HQ. The reserved band in the rules and the band the
  pods stand in are two copies of one decision, kept in step by a test on each side.

## Deferred

- Team events and outings beyond decorating: planning things as a group (the owner's words)
  stops at the office for now.
- A member moving an item without a proposal, or the owner placing one by hand.
- Items the owner may pin so the team cannot move them.
- Pictures for the kinds that have only a pixel shape (the fish tank, the lamp, the rug, the
  bean bag, the whiteboard, the coffee machine, posters, string lights).

## Alternatives considered

- **Pods by project.** Rejected by the owner: a team is people who work together; a project is
  work, and moves between them.
- **A standing "#office" task for the discussion.** Rejected: it would be on the board and the
  scorecard as work that never ends, and the engine's wakes, claims and nudges are all about
  tasks that do.
- **Anyone moves anything.** Rejected: a model proposes; deterministic policy authorizes, and a
  decoration is still a change the others live with.
- **Paid downtime out of a member's own share.** Rejected: a member's share is for its work, and
  downtime is the team's; the pool pays, within what the owner allows.
- **Ping-pong and the kitchen table as items.** Rejected for now: the catalog is fixed, and
  neither is in it.
