# HQ house pages: corrections ready to write, and what waits on the owner (3 November 2026)

Prepared from section 3 of `knowledge-fidelity-2026-11-03.md`, split by what the evidence can
settle. Nothing here is written to HQ: the house pages are the owner's, and writing them is a
production change.

**The one verified observation.** On the NAS, `gluetun-stack-restart.timer` is disabled and
inactive (read 06:05 and again 09:25 UTC, 3 Nov 2026), and ansible-nas develop has
`gluetun_periodic_restart_enabled: false` since `b3ad52a` (27 Oct). That is the state now. It says
nothing about whether the timer should stay off: that is the owner's decision, still open.

## A. Ready to write: corrections of fact only

These change what a page says the lab *is*, to match the reading above. They set no policy and
touch no priority.

### A1. `world:house#known-facts`, first bullet (owner, 18 Oct)

**Says now:** gluetun's group is restarted every 2 hours by a hand-made systemd timer, not in
ansible-nas; such a restart is expected, not a fault.

**Replace with:**
> - gluetun's 2-hourly group restart (gluetun, deluge, nzbget, prowlarr) is not running: on 3 Nov
>   2026 `gluetun-stack-restart.timer` read disabled and inactive on the NAS. It is declared in
>   ansible-nas (`gluetun_periodic_restart_enabled`, false on develop since `b3ad52a`, 27 Oct),
>   not hand-made. Whether it stays off is not yet decided.

The other bullets and the heading stay as they are (see C).

### A2. `world:area/1#2-hourly-gluetun-timer-alarms` (Ada, 20 Oct)

**Says now:** every 2 hours the owner's timer restarts gluetun; "Mara (CEO) decided: keep the
timer."

**Supersede, with this why:**
> The timer read disabled and inactive on the NAS on 3 Nov 2026 (ansible-nas `b3ad52a`, 27 Oct),
> so it no longer explains exited alarms for gluetun's group. The earlier "keep the timer"
> predates the owner's timer-off window (post 5195, 26 Oct) and the retirement of roles (ADR 0142).
> Its future setting is the owner's decision, not yet made.

### Outcome (3 November 2026)

- **A1 written** through the owner API and read back: `world:house#known-facts` revision 2, by the
  owner; the other three bullets unchanged. It is the current authoritative statement.
- **A2 not written, and left unchanged (owner's decision).** It is a legacy section from the retired
  area system: the live build has no area pages, so the owner API refuses `area/1`. It is no longer
  in worker briefings (only the house page is), but the record search still finds it until about
  18 January 2027 (90 days from its last update, 20 Oct). No direct database write, search change,
  area-page support or migration for it.

## B. Waits on the owner: priorities and rules

Not prepared for writing. Each is the owner's own wording of priorities or rules.

| Page | What is stale | Owner decision |
|---|---|---|
| `world:house#priorities` item 1 | Asks to bring the timer into ansible-nas, which is done (`roles/gluetun/defaults/main.yml`, `roles/nas_maintenance/defaults/main.yml`). | Whether to replace the item, and with what. The earlier draft ("settle #796: keep it off or turn it back on") is offered only as a starting point. |
| `world:house#how-we-work`, first two bullets | Names a CEO, leads and a ladder that ADR 0142 retired. | Whether to rewrite the house rules to match ADR 0142. |
| The timer itself | Off now (observation above). | Whether it stays off. |

## C. Uncertain: left unchanged

| Fact | Where | Why it stays |
|---|---|---|
| Four downloads stuck on import, no import tool yet | house known-facts | No current evidence either way. |
| owntone, owntone-spotify, mongodb-old-9.0 retired | house known-facts; area/1 | Matches the house map, not re-read live. |
| Priorities 2 and 3 (AGENTS.md draft, Docker-socket access, Watchtower) | house priorities | Watchtower's coverage changed 31 Oct; whether these still stand is the owner's call. |

## Not touched

Record-search ranking (ADR 0129), no new framework, no benchmark.
