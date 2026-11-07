# 0143 - HQ opens on the agents map

## Status

Accepted, 2026-11-07, by the owner (decision M6). AI-assisted (Claude). Amends
[0136](0136-endora-is-hq.md) (HQ's first screen) and [0119](0119-the-open-office.md) and
[0121](0121-games-in-the-office.md) (the studio and the office's games become a secondary place).

## Context

M5 (7 Nov) released the agents map, design E from the overview-maps competition: the owner
with their desk by project, and each member with the open items it holds. It was meant as HQ's
overview. As released (362e467f), HQ still opened on the studio, drawn above five tabs (Work,
Manage, Health, Activity, Chess). The map was a separate screen behind an "Agents map" button.
The owner: "Agents map should be the primary view or that was the goal. Remove and redesign the
system interface."

The product reset's principle (7 Nov) is *the engine owns the work, the CLI controls it, the UI
observes*. A screen that opens on a decorative office and hides the record of who holds what
works against it.

## Decision

1. **HQ opens on the map** on every device: a fresh entry, a reload, the menu's HQ, the old
   agents screen (`go("agents")`, `agents:open:map`) and any unknown screen. The tab the old
   screen remembered in this browser (`endora.hq.tab.v4`, `tab.v3`) is read once and removed.
   No place is remembered as a landing.
2. **The map is the shell.** The bar keeps Give work and the kill switch on every place. Under
   it are the chips, then the map (the hero line, you and your desk by project, the members with
   their items). **Your decisions** (the desk and *While you were away*) stand beside the map on
   a wide screen and under it on a phone. The map and its decisions are one place, so the owner's
   word is given where the work is seen.
3. **Everything else is a secondary place**, reached from one quiet row ("More of HQ") and left
   by "◀ Agents map" or Escape. The places are **Open work**, **Projects & team** (the old
   Manage), **Health**, **Activity** (HQ's record, as the old tab was), **Chess league** and **Office**.
   **Looking at** is the detail page a member, an item or a studio tap opens, with a way back to
   where it was opened. There is no tab strip and no second primary navigation.
4. **The studio is the Office's**: drawn and animated only while the Office shows, beside the
   office's games and its chess board. It is kept, not deleted (the owner declined cutting the
   office chess board before), and it no longer plays above every screen.
5. **No control is new, and none is lost.** Each control the five tabs offered has a place,
   listed in [the inventory](../proposals/hq-map-first-action-inventory-2026-11-07.md) and
   enforced by the console check. The map reads `/v1/hq/team-map` with HQ's own snapshot and
   poll, and changes nothing.

## Consequences

- The console check holds the shell to this: the map shows first, old tabs and links land where
  they now live, every old action is present, and the studio appears only in the Office. Its
  owner-readable scenarios are `app/node/src/web/features/*.feature`, run by the check itself
  (no test dependency for the console).
- Each place has a density budget, as each tab had.
- HQ's counters (tick, running, queued, retrying, held, coalesced) moved from under the studio
  to the top of Health.
- Running now stays in Activity; on the map, each member's live line opens its turn's replay.

## Alternatives considered

- **Make the map the default tab.** Rejected by the owner's words: the competing studio and
  the tab strip would stay.
- **Delete the studio.** It conflicts with the owner's earlier decision to keep the office's
  chess board, and nothing requires it once the studio is secondary.
- **Remember the last place.** A reload would then land off the map, which the owner ruled out.
