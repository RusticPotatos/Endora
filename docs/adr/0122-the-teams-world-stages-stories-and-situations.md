# 0122 — The team's world: stages, stories and situations

## Status

Amended by [0142](0142-one-team-of-loops.md), 2026-10-27: one team of loops.

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). The first of three
builds of the team's memory and history; the map (areas, addresses, routes and probes) and
asking the past (transcripts, searching the record, consulting a member) come after. Amends
[0111](0111-the-ceo-and-the-chain-of-command.md) (the house log becomes the house page),
[0118](0118-elastic-staffing-and-notebooks.md) (notes have a stage; the briefing's notes are a
piece of the world, within a budget), [0114](0114-evidence-acceptance-criteria-and-the-tester.md)
(its evidence rule is the world's) and [0120](0120-roles-as-an-enforced-raci.md) (who stewards
what). Built: everything below, and the owner's scenarios in
`domains/team/features/the-world.feature`.

## Context

What the team knew was a pile. The house log was one 6,000-character text the CEO rewrote
whole, so a rewrite could drop a fact without a word, and every lead read all of it every turn.
Lessons were one-line rules in a goal's notes, which only ever grew: a rule a lesson, with
nothing to say which still held. Notebooks had evidence and a stale mark, but nothing else
did, and nothing was ever settled: a hunch noted once and a fact confirmed every night read
the same. And every turn was briefed the same way whether its member had been at the work a
minute ago, was back from a week's furlough, or had just been handed someone else's task,
with no word of what was left out.

The failure that matters is not forgetting: it is a member that does not know what the team
already knows, and so does not look. The design HQ borrows answers it with knowledge kept as
addressed pieces that each say how established they are, lessons kept as short stories with a
name, and a briefing composed for the situation the turn starts from, within a budget that
never cuts silently. Endora takes those shapes and keeps its own rules: models propose and code
authorizes, the owner is the board, evidence comes from HQ's record, and nothing persists in a
turn's container.

## Decision

**Owner decisions, 2026-10-18.** The world lives in HQ's store, with a Markdown export (a read
route and a download on the HQ screen). Full transcripts and questions across areas come later.

### Pages and sections

- The **house page** (`world:house`) is written, and stewarded by the CEO: current-truth
  **sections**, each with a heading, a one-line headline (what a briefing shows), a short
  Markdown body without page or section headings, a stage, a warrant and its evidence; and a
  dated, append-only **timeline** below them.
- **Derived pages** are written by HQ from code and the record each time they are read, so they
  are never stale: `world:lab` (how the lab is built, ADR 0112) and `world:team` (who is at
  work, in what role, on what brain, reporting to whom).
- A section's **address** is `world:<page>#<anchor>`; the anchor comes from its first heading
  and never changes, whatever its stage or later heading. A retired or superseded section still
  resolves and says why, or where to go. A story's address is `story:<handle>`.
- Every write is a **revision**, kept append-only (a trigger refuses to change or remove one),
  with who, which turn and why; every write, retirement, promotion and demotion is also a line
  on the house page's timeline and an event in the feed (`world.changed`).

### Stages, and trust read when read

| Stage | Who sets it | What it needs |
| --- | --- | --- |
| observation | a page's steward, or anyone for a story in its own scope | at least one reference in HQ's record (0114's rule) |
| field note | a member, in its own notebook (0118) | as 0118 |
| insight | the steward only | a one-line warrant, and references from two different turns or a reading the node witnessed in this turn (a sandbox-reported receipt does not count) |
| canon | **the owner only** | the owner's word, from the World card or a proposal on the desk; the piece keeps its warrant and its evidence's snapshots |
| superseded | the steward | the address of what replaces it |
| retired | the steward | the reason |

- **Trust is worked out at read time, never stored.** An insight or canon whose last
  confirmation is older than its horizon (the house page and house stories 90 days, an area's
  and a member's stories 30), or none of whose evidence is still in HQ's record, reads as
  "unconfirmed since <date>" and counts as an observation in briefings and on the World card,
  though nobody edited it. Its steward confirms it again with `verify_world` on fresh evidence.
- **Evidence keeps a snapshot**: each reference is kept with what the record said of it when it
  was cited (a receipt's tool and excerpt, a task's title, an event's kind), so a piece still
  shows its evidence after the feed lets an old event go.
- **Canon is the owner's alone.** The CEO and the leads propose a trusted insight with
  `propose_canon {address, warrant}`; it reaches the owner's desk with its warrant and evidence
  (one card per address), and only Approve makes it canon. Nobody writes the canon stage, and a
  steward cannot rewrite or retire canon. The owner **demotes** canon to an insight, an
  observation or retired, always with a reason, which the piece keeps and the feed and the
  house page's timeline say.

### Stories

- A lesson is a **story** in four short parts (each at most 300 characters): when it matters,
  the tempting move, what to do instead, and why; under a **handle** of two or three words
  (`closed-too-soon`), unique across the team, that a post or a send-back can cite instead of
  retelling it. Citing a handle marks the story used.
- **Scope by role**: the CEO's stories are the house's, a lead's its area's, the tester's her
  lead's area's, anyone else's its own. A member is told the house's, its area's and its own.
  An insight is its scope's steward's: the CEO for the house, the lead for its area, a member
  for its own.
- **Taught by situation**: a story is told whole in the situations it is taught in (by default
  fresh, after a gap, handover and recalled), otherwise in one line, its handle and what to do
  instead.
- `tell_story` replaces `note_lesson`, which is no longer offered; a worker that still calls it
  gets a story marked **to retell**. A lead's briefing asks it, at most once a day, to retell up
  to three of its area's lessons as stories, under the same handle.

### Situations and budgets

Each turn starts from a **situation**, decided by code from the record, in this order:

| Situation | When | What it adds to the briefing (core: never left out) |
| --- | --- | --- |
| recalled | first turn after a furlough (0118) | what changed since the furlough began (core), "read your notes first" (core), its notes (core) |
| handover | a handover note to it on the task is newer than its last turn on it, or it now holds work others took turns on and took none itself | the handover note first (core), or that it is new to the work |
| fresh | its first turn ever, or its first in this project | stories taught on fresh told whole; the house page's insights besides its canon |
| after a gap | its last turn on the task a day ago or more, or its last turn at all a week ago | what changed on the house page and in its stories since (core) |
| continuing | otherwise | the house page and what its timeline gained since its last turn (the CEO and leads) |
| resumed | a runtime that carries its own conversation | not chosen yet: named for the later build |

The **memory budget** (the world's pieces: the house page, the lab page, stories, the notebook,
the situation's own pieces; not the task, its thread or the tools), at about four characters a
token:

| Member | Continuing, fresh | After a gap, handover, recalled |
| --- | --- | --- |
| CEO | 2,500 | 3,700 |
| Lead | 4,000 | 5,200 |
| Developer, tester, researcher | 3,000 | 4,200 |
| Anyone on the NAS's model | 1,800 | 3,000 |
| Anyone on the Mac, the CEO keeping its own ([0130](0130-the-macs-brain.md), amended) | 2,800 | 4,000 |

Over budget, the optional pieces are left out first and then the recommended, the biggest first
each time; the core never. **Each piece left out is named** on the briefing's last line, with how
to read it ("Not included this turn, to keep to your briefing's budget: story:…. Read any of them
with read_world (your notes with read_notebook)."). If the core alone runs over, the briefing
still goes out and the run records it. The CEO and the leads are shown the house page's current
sections (canon first) and how the lab is built (the CEO when it starts afresh or comes back);
other members its canon. The CEO's briefings stay short: on a continuing turn its world is the
house page, the house's stories and its notebook's count.

**Each run records** its situation, the names of the pieces it was told and left out, their size
and its budget (`run_briefings`, one row a run, its latest attempt's), shown in the replay as
"Started from".

### Handover notes

`hand_to`, and a stuck task's `decide_stuck` that hands it on or drops it, need a **handover
note** `{done, not_done, next, watch_out}`, each said, at most 1,200 characters in all; without
one they are refused. The note is kept per task, from whom, to whom and in which turn
(append-only), said in the feed (`task.handed_over`), and starts the next holder's first turn on
the task.

### What a turn may do

Every member: `read_world {address}` (what it reads counts as used) and `tell_story`. The CEO:
`write_world` (the house page), `note_house` (a dated line on its timeline), `verify_world`,
`retire_world`, `propose_canon`. A lead: `verify_world` and `retire_world` for what it stewards,
and `propose_canon`. A write to a page or scope that is not the caller's is refused naming its
steward. `rewrite_house` is refused for a release with what to use instead, then removed.

### The owner's side

The HQ screen's **World card**: the house page's sections by stage (canon first, unconfirmed ones
said so, with warrant and reason), its timeline, the derived pages, the stories (marked to retell
where they are), **Make canon** and **Demote…** (which asks why), and **Download the world
(Markdown)**. A proposal for canon is a desk card (Make it canon, Not canon). Routes through the
node and its audit log: `GET /v1/hq/world`, `GET /v1/hq/world/export.md`,
`POST /v1/hq/world/promote {address, why?}`, `POST /v1/hq/world/demote {address, to, why}`.

### Moving what the team kept before (once, nothing deleted)

At start, once (the mark `world_migrated` is set in the same transaction as the move, so a
second start changes nothing, and a failed move changes nothing):

- **The house log** (`settings.house_log`): each line `- 17 Oct 2026: …` or `- 17 Oct: …` becomes
  a line of the house page's timeline at noon of that day in the owner's time (a date with no
  year is the latest not after the move), by "the house log". The lines Endora seeded (how the lab
  is built) are not copied: they are the lab page, derived from the same code. A line with no
  date takes the date of the line before it (or the move's) and is marked "(undated in the house
  log)"; a line over 500 characters is kept whole in 500-character pieces. The log itself stays,
  read-only for a release: the owner's rewrite is refused once it has moved, and the screen shows
  it read-only.
- **Lessons** in goals' notes (`- <lesson> (<who>, <day> <month>)`) become stories marked to
  retell: what to do instead is the lesson, the handle its first three words (`-2`, `-3` when
  taken), at the observation stage, in the area of the lead who noted it or the tester's lead (a
  writer no longer on the team: the house's), dated when it was noted, and keeping its line. The
  same line in two goals is one story. The goals' notes are not changed; briefings leave out the
  lines that are stories now, and the owner's own notes stay goal notes.
- **Notebooks** are not moved: the `notes` table is kept as it is and read as a stage, a member's
  note a field note and the owner's an insight, worked out when read (no column, no row
  changed). The cap, the tidy and the stale mark are unchanged. This is the least risky choice:
  a member's notes keep their numbers, their evidence and their tools.
- The CEO is asked, on its turns while the house page has no sections, to write what is true of
  the house now from its timeline, as insights on evidence.

**Stored** in tables made if missing (`world_sections`, `world_revisions`, `world_timeline`,
`stories`, `handovers`, `run_briefings`) and the settings `world_migrated`, `canon_proposed:<item>`
and `world_retell_asked:<member>`. No existing row changes; nothing is deleted.

## Consequences

- A lead no longer reads the whole house log every turn: it reads the house page's sections
  and what its timeline gained since its last turn, within its budget. Until the CEO writes the
  sections, the newest timeline lines stand in.
- Briefings can be a few hundred characters longer in the dear situations, and are never cut
  without the cut being named; the replay shows each turn's situation and what it left out.
- Curation is work: the CEO writes and confirms the house page, leads confirm their stories, and
  the owner alone settles canon. Trust that lapses is shown, never hidden.
- Lessons stop piling up in goals' notes; old ones wait as stories to retell.

## Deferred

- Area, project and role pages; proposals to a page; the map and its quiz; "not on the map" gaps
  and broken roads: **built, [0126](0126-the-teams-map.md)**. Still deferred: `read_doc` by
  section, and relations between pieces beyond the addresses a section cites.
- Transcripts, searching the record and consulting a member: **built, [0129](0129-the-teams-history-and-asking-the-past.md)**.
  Still deferred: resuming a runtime's own conversation (the `resumed` situation is named but
  never chosen).
- A second prompt-cache mark from the worker through the relay, for reuse across a member's
  turns close together.
- Promoting a note from a notebook into a page; a stored stage on notes.
- Marking a written section contradicted by a derived page; asking a steward to retire or
  confirm a piece unused for 60 days; a digest of a long thread for a handover.
- Evidence references on a handover note (the store keeps them; the tools do not take them yet).

## Alternatives considered

- **The world as a folder of Markdown with its own history.** Considered, and not chosen by the
  owner for now: one store, one backup, edits from HQ's screen and rules held by HQ's code; the
  export gives the Markdown.
- **Trust stored and swept by a job.** Rejected: a stored stage goes stale the day after it is
  written; read-time trust cannot.
- **Cutting a briefing to fit.** Rejected: a member does not know what it was not told; naming
  what was left out lets it read on.
- **Lessons left as one-line rules.** Rejected: a rule per incident grows without end; a story
  carries the moment, so it applies where no rule was written.
- **Moving notebook notes into a new table.** Rejected as riskier than reading the notes as they
  are, with the same result for now.

## Amendments

- 2026-10-18: the move compares a yearless date with the move's *day*, not its instant, strips a
  date the writer put at the start of a line, and is mended once (`world_dates_repaired`) where
  an early-morning move had dated it a year back; the one change a timeline row ever takes, with
  the old value kept in `world_timeline_repairs`. Every briefing states today's date with its
  year, and a member's text starting with a date whose year is one out on today's date is
  corrected and recorded. The owner may write a section or a timeline entry directly
  (`POST /v1/hq/world/section`, `/timeline`); such a section is an insight at least and a
  member cannot overwrite it.
- 2026-10-18, by [0126](0126-the-teams-map.md): the world has a page for every area, project, goal
  and role beside the house, and an address is a page's slug and a section's anchor
  (`world:area/3#backups`). `write_world`, `verify_world` and `retire_world` act on any page the
  caller stewards, so the leads write their areas' and projects' pages, not only the CEO the
  house's; anyone else proposes (`propose_world`) and the steward decides. Every briefing opens
  with the member's map, a core piece of its memory budget. The decisions above about the house
  page are unchanged.
