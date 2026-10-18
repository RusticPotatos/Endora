# 0118 — Elastic staffing and notebooks

## Status

Accepted, 2026-10-17, by the owner. Prepared with AI assistance (Claude). Amends
[0113](0113-projects-budgets-and-reports-to.md) (the scorecard counts each team's seats; a
member on furlough takes no share) and [0115](0115-the-engineering-wing-and-the-git-flow.md)
(`request_staff`, beside `request_hire`; the wing's seats count a member on furlough). Built:
furlough and recall, the daily idle look, `request_staff`, the owner's two buttons on the
roster, the scorecard's seats, the notebook tools and tables, the briefing's notes and tidy,
the owner's notebook routes and panel. The owner's scenarios in
`domains/team/features/staffing-and-notebooks.feature`.

## Context

Two things the owner said, in their words, about the team they run:

- "If they have too much idle staff that could reduce and expand as needed." The team was a
  fixed roster: a member with nothing to do still took a place in every briefing and a share of
  the pool, and a lead with work piling up could only ask for a hire. The owner does not want
  anyone fired: the team's members have a name, a record and (below) a notebook, and what
  shrinks and grows is how many are at work.
- On memory: it is good, "but if each member had a memory and makes mistakes maybe they are too
  old". A member that writes down what it learned will also write down what it got wrong, and
  nothing ever corrects it. Notes have to be honest and tidy, or they become the thing that
  makes the next turn worse.

Both directions of staffing are the owner's to approve. A model proposes; deterministic
policy authorizes.

## Decision

**Owner decisions, 2026-10-17.**

1. **Furlough** is a state of a member, apart from the owner's pause. A member on furlough is
   never woken (the wake is refused as `furloughed`), holds no task, is left out of every
   briefing's team list, and is left out of the share arithmetic (it has no claim on the pool,
   and the others' shares grow). It keeps its record, its name, whom it reports to and its
   notebook. Nobody is ever deleted: no route, no engine method and no query removes a member,
   and there is a test that says so. Stored as two nullable columns on `members`
   (`furloughed_at`, `recalled_at`, added by a guarded `ALTER TABLE`, empty); a member is on
   furlough when it has the first and not the second. Work is not handed to a member on
   furlough (`create_task` and `hand_to` refuse with the reason), and a turn queued for one is
   stopped. The owner furloughs or recalls any member **except the CEO**, from the member's
   panel on the roster (`POST /v1/members/{id}/furlough` and `/recall`, through the node and its
   audit log); a member holding open work is not furloughed (the refusal names the count): the
   work is handed on first.
2. **The idle look**, once a day on the first tick of the owner's day. For each member that is
   not the CEO, not a lead and not on furlough: if it holds no open task, is assigned none,
   has no routine of its own switched on, and has been woken (which is what being handed
   something is) or taken a turn in none of the last seven days, one item goes on the owner's
   desk: "Rae has had nothing to do since 16 Oct: furlough, or keep?" The week is counted from
   the later of the last wake or turn, the day the team first met the member (HQ keeps no
   join date, so the look records it the first time it sees a member), and its last recall.
   **Furlough** furloughs it; **Keep** leaves it, and it is not offered again for seven days.
   One item at a time per member.
3. **Recall on backlog.** A lead (Lead role only, refused otherwise in code) asks for staff
   for one of its projects with `request_staff` { project, why, evidence }. Evidence is task
   references, each checked to exist, be open and be in that project, and there must be at
   least **twice the project's active developers** of them (at least one); fewer is refused
   with the count ("you cited 3 open tasks of the project, but it has 2 active developers: a
   request cites at least 4"). A project's active developers are the developers at work
   (neither paused nor on furlough) who report to the project's lead or hold an open task in
   it. Only the project's lead asks for its staff. If a developer is on furlough (the lead's
   own first, else the longest out), the desk item offers **Bring back** (`recall`); with none
   it becomes the hire request of 0115, a developer on Claude Haiku, under the same seats and
   the same one-request-at-a-time rule. Approving brings the member back reporting to the lead
   who asked, or hires, with an event and a post in the lead's thread.
4. **The scorecard** counts each team's seats, how many are at work and how many are on
   furlough (the lead's team: the lead and whoever reports up to it), served in the snapshot's
   scorecard and drawn under "How the team is doing".
5. **Notebooks.** Each member has a notebook of notes: its words (one line, at most 300
   characters), the evidence it rests on (at least one reference for a member's note, each
   looked up by the one rule of 0114 and refused naming the first that is missing), when it
   was written, in which turn, and when it was last used. A new table `notes`; a member's
   notebook holds at most 60 notes. Every role is offered four tools, none of which names a
   member, so a member can only ever write its own notebook: `note` { text, evidence },
   `forget` { note, reason } (a reason is needed; forgetting is a tidy), `read_notebook`
   (all of them, newest first, with their evidence) and `notebook_checked` (looked over,
   nothing to change: a tidy). A note forgotten is gone from the notebook and said in the
   event feed with its words and reason.
6. **In the briefing**, first after the member's name: "Your notes (N):", each with its number
   and words, newest first, at most 12 notes and about 1,200 characters, the rest summed up
   ("and 3 older notes: use read_notebook"). A note not used for 30 days is marked
   "(stale)". A note counts as used when the member's post cites its number (`n12`) or cites
   evidence the note rests on. The **CEO's briefing is kept short**: it shows only the count
   and `read_notebook`. A week after a member's last tidy (or, never tidied, its oldest note)
   its briefing adds: "Tidy your notes: forget any that are wrong or no longer true (cite
   why)." at most once a day until it uses `forget` or `notebook_checked`.
7. **The owner's side.** The member's panel on the roster shows its notebook (every word
   escaped, with the evidence refs, "stale", or "from the owner") with Edit and Delete on each
   note and Add note; an owner's note needs no evidence and says whose it is. Routes through
   the node and its audit log: `GET /v1/hq/members/{id}/notebook`, `POST` the same to add,
   `POST /v1/hq/members/{id}/notebook/{note}` to edit and `DELETE` it to delete (HQ's own
   route for the last is a POST: its client has no DELETE).
8. **A furloughed member keeps its notebook,** and on its first briefing after a recall it is
   told: "You were furloughed from 17 Oct to 19 Oct; read your notes first."

## What is enforced in code

The CEO is never furloughed; a wake for a member on furlough is refused; work is not handed to
one; a member with open work is not furloughed; only a lead calls `request_staff`, only for
its own project, on tasks that exist, are open and are in that project, and not fewer than
twice the project's developers; the seats of 0115 hold for a hire made this way; a member's
notebook is its own (no tool takes a member; `forget` of another's note is "not in your
notebook"); a note needs words, at most 300 characters and evidence that is in HQ's record;
only the owner writes without evidence. No member is ever removed.

## Decisions the owner's words left open

- A member on furlough still counts as a seat in the wing, so a recall never needs a seat and a
  hire request is refused while the wing's seats are all taken, furloughed or not.
- "At the owner's day start" is the first tick of the owner's day (their offset from UTC).
- A member's idleness is read from the record HQ keeps: tasks held or assigned now, enabled
  routines, wakes and turns. A member with a long task that makes no wakes is not idle, since it
  holds the task.
- The hire a `request_staff` falls back to is a developer on Claude Haiku (the wing's agreed
  brain); the owner can still change the brain afterwards.
- A notebook is capped at 60 notes so it cannot grow without a tidy, and "used" is judged from
  posts only.
- The tidy is asked for at most once a day once it is due, so a member that ignores it is not
  asked on every turn.

## Consequences

- A quiet team shrinks to who is needed and the pool is shared among fewer, and it grows back
  by the owner's yes; no one ever has to be hired again for work they did before.
- A cloud member reads its notes on every turn, so the briefing is a few hundred characters
  longer when it has them; the CEO's is not.
- Notes can still be wrong: evidence shows a note had a reading behind it when it was written,
  not that it is true now. The tidy, the stale mark and the owner's Delete are the remedies.
- New data on the live store: two nullable columns on `members`, a `notes` table and its
  index, added when HQ opens the store; no row changes.

## Amendment, 2026-10-18 ([0119](0119-the-open-office.md))

- **Downtime notes.** A member with nothing to do may be given a downtime turn (a turn about no
  task, once a day, free on the house's model). Its briefing suggests a team idea for its
  notebook, and names the latest event to cite, since a note still rests on evidence from HQ's
  record. The notebook tools are unchanged, and a downtime turn has them beside `office`.
- **Downtime is not activity.** Its wakes and turns do not count as a task, wake or turn in the
  idle look: a member whose only turns in a week were downtime is still offered for furlough.

## Deferred

- Members proposing their own furlough, or the lead proposing another member's.
- Automatic hiring or furlough, without the owner's yes: stays tabled.
- A furlough that ends on its own date.

## Alternatives considered

- **Firing.** Rejected by the owner: a member has a name and a record, and a notebook worth
  keeping.
- **Furlough as the owner's pause.** Rejected: a pause is a short stop that still counts in the
  team and the shares; a furlough changes the team's size and its arithmetic.
- **A weekly tidy routine for every member.** Rejected as overkill: the ask is part of the
  briefing of a member that has notes.
- **Notes without evidence.** Rejected: a member that makes mistakes would remember them as
  facts.
