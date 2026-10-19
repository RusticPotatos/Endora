# 0126 — The team's map

## Status

Accepted, 2026-10-18. Prepared with AI assistance (Claude). The second of three builds of the
team's memory and history, agreed with the owner when [0122](0122-the-teams-world-stages-stories-and-situations.md)
was accepted; asking the past (transcripts, searching the record, consulting a member) comes
after. Builds on [0120](0120-roles-as-an-enforced-raci.md) (who stewards what) and amends 0122
(pages beyond the house; who may write one; `read_world`'s addresses). Built: everything below,
and the owner's scenarios in `domains/team/features/the-map.feature`.

## Context

The world of 0122 gave the team one page of its own, the house's, and the CEO's briefing carried
it. That fixed what the team knew, not whether a member knew where to look. The failure that
matters is a member that has no idea a thing exists, and so builds it again: more search does not
help, because the failure happens before the search. A developer asked to tell the owner when a
branch passes does not find the notice that already does it, because nothing in its briefing says
where such a thing would be.

An area's knowledge, a project's and a role's had no home. A lead read the house page and
nothing of its own area; a member who learned something about a project could say it in a post and
nothing more; and nobody could tell whether the pages that did exist could be found by someone who
had only a short description of them.

## Decision

### A page for every area, project, goal and role

- Pages: `world:area/<n>` for each lead (by the lead's member number, so a rename moves
  nothing), `world:project/<n>`, `world:goal/<n>` and `world:role/<role>` (`ceo`, `lead`,
  `developer`, `researcher`, `tester`), beside the house, lab and team pages. A page has sections
  at stable addresses (`world:area/3#backups`) and a dated timeline, exactly as the house page
  does; the same stages, the same evidence rule and the same trust read at read time (a role,
  project, goal or house page 90 days, an area's 30).
- **Made from the team as it stands**, once each: at start, and on every tick for a lead hired
  or a project or goal made since. A project or goal given up gets none. Nothing is changed or
  deleted, so a second start makes none (`migrate_map`).
- **Who stewards a page is worked out from the data, never from a name written down**: an area
  its lead; a project its lead (the CEO when the CEO leads it, or its lead is gone); the house and
  every goal the CEO; a role the owner; the lab and team pages nobody, because HQ derives them.
  Move a project's lead and its page's stewardship moves with it.
- **Only the steward or the owner writes a stewarded page** (`write_world`, now the CEO's and the
  leads'; refused for anyone else with "kept by <steward>: propose the change with
  `propose_world`"). A role's page, until the owner writes it, shows what the role is for, from
  the roles' own definitions, as a default HQ shows and never stores.

### Proposals

A member that does not keep a page proposes a section to it (`propose_world {page, anchor?,
heading, headline, body?, evidence}`), on at least one reference in the record. The steward
accepts or declines (`decide_proposal {id, accept, reason?, stage?, warrant?}`): an acceptance
writes the section as an observation, or as an insight when the evidence warrants one; a decline
says why. Only the steward decides; a canon section, or one the owner wrote, is the owner's to
replace, so the steward's acceptance of such a change is refused. The owner decides any proposal
from the World card (`POST /v1/hq/world/proposals/decide`), and alone decides one to a role's page.
A member has at most five open, the same proposal again is that one, and a steward proposing to
its own page is told to write it. The steward's next briefing lists what waits for it; the
proposer's says how it went, once.

### The map

Every briefing opens, right after who the member is, with **its map**: the pages that matter to
this member by address, one line each. First its role's page, its area's, its task's project and
the house (and for a lead, the projects it leads); then the lab, the team and its task's goal;
then, wide and shallow, every other area and current project, and for the CEO every goal and role,
each with the best (canon first) three sections of the pages that matter, by address. Each line
says what the page is and who keeps it. It is composed within a size (1,400 characters for a member
on the NAS's model, 1,900 on the Mac, 2,400 on Claude), leaves off the optional lines first and then the
recommended ones, the last first, and **names what it left off** on its closing lines, which also
say "looked and it is not here? say so with `report_gap`". It counts toward the member's memory
budget (0122) as a core piece. The run records it among the pieces it was told.

`read_world` reads any address: a page, a section, a story. A section shows its routes out: each
address it cites, and whether it leads anywhere.

### Gaps, and broken roads

Two ways a map fails, kept apart because they are mended differently.

- A **gap** is knowledge that is missing. A member that looked on its map and found nothing says
  so (`report_gap {what, where_looked}`). It goes to the member's lead (to the CEO for a lead, or a
  member with no lead) as **one item however many report it**, never to the owner's desk. The
  steward's next briefing lists it, and `close_gap {id, how}` closes it, saying how.
- A **broken road** is a way that leads nowhere: a section cites an address (`world:…` or
  `story:…`) that does not resolve, or names a successor that does not. Code finds them each time
  they are asked for, so they are as true as the record this minute; a section retired without a
  successor breaks every road to it, and one superseded sends readers on to its successor. They are
  shown to the steward of the page that cites them and on the World card.

### The nightly quiz, on the free model

Whether the map carries is tested, not assumed. Between one and six in the morning in the owner's
time, if a member on the house's model is free (at work, not the CEO, no live turn, no wake held, no
open work, and no downtime turn running), it is woken for a **downtime turn** that carries a few
questions code made from that member's own map. It answers with `answer_quiz {answers}` and **code
scores each answer** against the same facts, so the check costs nothing and no paid turn is
spent on it. If no such member is free by six, the night is **recorded as skipped**, with why.

| Question | Asks | An answer is right when |
| --- | --- | --- |
| which page | "Which page says how the lab is built?" | it says that page's address (`world:lab`) |
| who keeps | "Who keeps world:area/3?" | it names the steward (a name, "the CEO", "the owner", "HQ") |
| address of | "What is the address of the page titled …?" | it says the address |
| section address | "What is the address of the section … on world:…?" | it says the section's address |

At most six a night, one of each kind in turn about different pages, starting at a different place
each night so that over some nights every page is asked about. An address counts when it is said
with `world:` (or is the whole answer); an answer over 120 characters is not one. A page **fails a
night** when it was asked about and no question about it was right; one that fails each of its last
three nights is **flagged** to its steward's next briefing and to the World card, which shows each
page's score over the last seven answered quizzes. A quiz not answered in twelve hours is let go
and marks no page. Nothing rides on it for the member.

### After Claude Code compacts

Claude Code compacts a long turn's conversation, and with it the map the briefing opened with. The
worker keeps that block in the turn's own folder, and the workbench's managed settings run a
`SessionStart` hook with the `compact` matcher (`endora-worker --map`) that prints it back. The
hook never blocks a turn. The worker's own turns do not compact.

### The owner's side

The World card gains, from `world_shown`'s `map`: every page with its steward, its sections and
its quiz score (flagged ones marked); the proposals waiting, with Accept and Decline (which asks
why), and the last few decided; the open gaps; the broken roads; and last night's map check. The
"Write as the owner" form takes a page. The Markdown export carries each page under its own
heading with its steward. Route: `POST /v1/hq/world/proposals/decide {id, accept, why?}` through
the node and its audit log.

**Stored** in tables made if missing (`world_pages`, `world_proposals`, `world_gaps`,
`world_quizzes`). No existing row changes; nothing is deleted. The engine's `quiz` setting (the
service turns it on; the scripted demo has none) gates the quiz.

## Consequences

- A member knows what exists, and where, from its first line; it is told what its map left off and
  how to say a thing is missing. The map is a few hundred tokens a briefing, inside the budgets of
  0122, and the briefings of the CEO and the leads still do not carry the whole house.
- Knowledge about an area, a project or a role has a home with a keeper, and a member that learns
  something has a way that is neither writing a page it does not keep nor leaving it in a post.
- The quiz measures the map without spending anything: a map line a small model cannot use shows up
  as a page that fails, and its steward is told. A night with nobody free is a recorded gap in the
  measure, not a silent one.
- Curation is still work: stewards decide proposals, close gaps and mend roads, and the briefing
  only asks, never forces.

## Deferred

- Derived `projects`, `tools` and `who-knows` pages; marking a section `core` for a role and a
  probe per section; relations between sections beyond the addresses a section cites.
- `read_doc` by section.
- A gap or broken road that touches canon reaching the owner's desk; asking a steward to retire or
  confirm a section unused for 60 days; marking a written section contradicted by a derived page.
- Running the quiz on a paid model on demand.
- Seeding a role's page from `docs/standards.md` as stored sections: the role's page shows a default
  until the owner writes it.
- Transcripts, searching the record and consulting a member (the third build).
- The compaction hook is configured and tested as configuration and as the worker's file and
  output; it has not yet been seen to run inside a compacting Claude Code turn.

## Alternatives considered

- **Stewards written down by name.** Rejected: a name goes stale the day a lead changes; the
  record already says who leads what.
- **One page for everything, with sections per area.** Rejected: its keeper would be the CEO for
  all of it, and the leads would still not own what they know.
- **A quiz asked of a paid model.** Rejected for the nightly run: it would cost money to check
  something a small model on the house's hardware tests more strictly.
- **A model judging the answers.** Rejected: the model is not the enforcement boundary; the
  questions and the scoring are code, over facts the map already holds.
- **Gaps on the owner's desk.** Rejected: most are the area's to answer, and the owner's desk is
  for what only the owner decides.
