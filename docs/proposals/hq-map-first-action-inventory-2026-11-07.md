# HQ map-first: every owner action and where it goes (7 Nov 2026)

The owner's decision M6 (7 Nov 2026): "Agents map should be the primary view or that was the goal.
Remove and redesign the system interface." At 362e467f the console still opened HQ on the studio
above five tabs (Work, Manage, Health, Activity, Chess), and the agents map (M5, design E) was a
separate screen behind an "Agents map" button in HQ's bar. This file is the inventory written
**before** the redesign: every action the owner can take in HQ today, and the place it has
afterwards. Nothing is dropped: no approval, stop or control, no retained record, permission or
task state. Code that no longer has a caller is removed once its action has a new home.

The principle it follows is the product reset's (7 Nov): *the engine owns the work, the CLI
controls it, the UI observes*. The map shows what HQ's records say. Its only controls are the
ones the owner already had, and each still calls the same node route. No control is new.

## The new shell

HQ opens on **the map** on every device. That holds for a fresh entry, a reload, the menu's HQ
and any old route or kept choice. The map is the screen's spine:

- **The bar**: Endora HQ, the runaway tag when HQ stopped the team, **Give work** and the **kill
  switch** (Resume while it is pulled). Below it, the chips: what waits on you (it jumps to your
  decisions), today's pool, turns waiting for budget or tomorrow, balances, pushed this week.
- **The map** (design E, M5): the hero line, which says what runs now, what waits to start and
  what is blocked. Then **you**, with your desk by project, and each member with the open items it
  holds, grouped by project. Last come the items nobody holds and who is away. A line moves only
  while a turn runs and was heard from recently. An item opens its Live Work map, and a member
  opens its detail.
- **Your decisions**, beside the map on a wide screen and under it on a phone: the desk (its
  first five whole, the rest one line each) and *While you were away*. Every card keeps its own
  choices.
- **More of HQ**: one quiet row of links at the foot of the map to the secondary places below.
  It is not a tab strip. Each place opens as its own page with "◀ Agents map" to return.

Secondary places: **Open work**, **Projects & team**, **Health**, **Activity**, **Chess league**
and **Office**. **Looking at** is the detail page that a member, an item or a studio tap opens.

## Inventory

Destination keys: **D** decisions/review (on the map) · **G** give work (the bar) · **X** project,
task or member detail · **H** history · **S** secondary administration, health or settings.

### Safety and control (always in the bar)

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Kill switch (`POST /v1/hq/stop`, confirm) | HQ bar; the studio's lever | **Bar** (every HQ place); the lever stays in the Office's studio |
| Resume (`POST /v1/hq/resume`) | HQ bar | **Bar** |
| Runaway tag (stopped by the tripwire) | HQ bar | **Bar** |

### Decisions and review (D)

Every desk kind keeps its choices and its help line: goal, stuck, ready_to_merge, ready_to_read,
question (with answer box), notice, approval (shows exactly what runs), project_cap and
project_budget (amount box), rounds, hire, canon, out_for_day (amount box, "Top up the pool
first"), tripwire (always whole), push (or "Download the bundle"), privacy, needs_owner.

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Decide a desk card (`POST /v1/hq/desk/{id}`) | Work tab, Your desk | **D**: Your decisions on the map |
| A refused decision, said on its card | Work tab | **D** (unchanged) |
| Download a branch bundle (`GET /v1/hq/bundles/…`) | Work tab desk card | **D** |
| Top up the pool from an out-for-day card | Work tab desk card | **D** |
| While you were away: act on a digest card | Work tab | **D**, under the desk |
| "N waiting on you" chip | chips → Work tab | Chips → jumps to Your decisions |
| Desk by project | Agents map screen, "You" | **Map**, "You" (now links to the decisions) |

### Giving work (G)

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Give work form: title, done criteria, loop (`POST /v1/hq/tasks`) | HQ bar | **G**: the bar, on every place |
| Hand a project work (prompts) | Manage, Projects | **S**: Projects & team (unchanged) |

### Project, task and member detail (X)

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Open an item's Live Work map | Agents map; Looking at | **Map** item tap; Looking at (unchanged) |
| Replay a task or turn | Looking at; Activity | Unchanged |
| Looking at a task: status, chain of why, evidence, documents, turns | Work tab | **X**: Looking at page |
| Reply in a task's thread (`POST /v1/hq/tasks/{id}/post`) | Looking at | **X** |
| Move an open task into a project (`POST /v1/hq/tasks/{id}/project`) | Looking at | **X** |
| Looking at a member: doing, run, brain, budget, turns | Work tab (studio tap) | **X**: from a member on the map, or the studio |
| All turns of a task or member ("all turns") | Looking at | **X** |
| Studio taps: the board's goal, shelves, your corner, what the team knows, the chess table | Work tab, Looking at | **X**, opened from the Office's studio |
| Open tasks list, newest first | Work tab | **S**: Open work |
| Find a task by number or words | Work tab | **S**: Open work |
| Every EL- number opens its task | everywhere | Unchanged (opens Looking at) |
| Holodeck: open, step, watch, copy or save PGN, close | Work tab fold | **S**: Open work (fold) |

### History (H)

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Running now, Watch a turn | Activity tab | **H** Activity; on the map, each member's live line opens its turn's replay |
| Latest twenty, open a long line whole | Activity tab | **H**: Activity |
| Show more: paged past, Newer and Older, Escape | Activity tab | **H** (full screen as before) |
| A line's link opens the item, member or turn, with a way back | Activity tab | **H**; the way back returns to Activity |
| The record of what Endora did | menu, Activity | Unchanged (the console's own Activity screen) |

### Secondary administration, health and settings (S)

| Action (route) | Where at 362e467f | Destination |
|---|---|---|
| Money card, pool top-up (`POST /v1/hq/pool/top-up`), cost by model fold, credits link | Manage | **Projects & team** |
| Goals: set one (`POST /v1/hq/goals`), goal notes (`POST /v1/hq/goals/{id}/note`) | Manage | **Projects & team** |
| Projects: make, cap, tools, hand work, pause, resume, drop (`/v1/hq/projects…`) | Manage | **Projects & team** |
| Routines: add, switch, run now (`/v1/hq/routines…`) | Manage | **Projects & team** |
| Accepted work, its two switches (`POST /v1/hq/delivery`) | Manage fold | **Projects & team** |
| Team board: a member's controls, brain (confirm for paid), rename | Manage | **Projects & team** |
| Hire a member (`POST /v1/hq/members`) | Manage fold | **Projects & team** |
| Contracts switches (`POST /v1/hq/contracts`) | Manage | **Projects & team** |
| What the team knows: world pages, canon, demote, write, paste, export; the house | Manage fold | **Projects & team** |
| Scorecard, outcomes, what failed (with its link to the Home lab), the team's turns | Health tab | **Health** |
| HQ's counters (tick, running, queued, retrying, held, coalesced, today, open tasks) | under the studio | **Health**, first line |
| The model league: live boards, bracket, standings, finished games, PGN | Chess tab | **Chess league** |
| The studio (canvas, its legend and taps; the kill lever) | above every tab | **Office** (secondary, kept) |
| The office's games and the office chess board (ADR 0121) | Manage, Office fold | **Office**, beside the studio |
| The Mac and the credits' setup | Settings → AI (links from HQ) | Unchanged |

### Routes, kept choices and links

| Old | Handled as |
|---|---|
| A fresh entry or a reload | The map (no kept place is restored) |
| `endora.hq.tab.v4` / `tab.v3` in this browser | Read once and removed. The landing is the map either way |
| The "agents" screen (`agents:open:map`, `go("agents")`) | Redirected to HQ's map |
| `hq:tab:<old tab>` links (work, now, desk, manage, team, knowledge, office, health, activity, chess) | Mapped to their place: work, now and desk → the map; manage, team and knowledge → Projects & team; office → Office; the rest to their own |
| A URL hash or path | The console has never routed by URL; any hash still opens the map |
| An unknown screen name | HQ's map (as before) |

## Retired

- The five-tab strip and its arrow keys: the primary navigation that competed with the map.
- The studio drawn above every tab: the superseded office overview. It is kept, secondary, under
  Office.
- The separate agents map screen and its "Agents map" bar button: the map *is* HQ now.
- The per-tab memory in this browser.

No conversation surface exists in the console at 362e467f. ADR 0139's Conversations card is
described in `docs/where-things-go.md` but is not drawn, so there is nothing of it to move.
