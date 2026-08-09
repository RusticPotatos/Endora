# 0062 — One permission surface

## Status

Accepted (2026-08-03). Amends [0051](0051-where-the-boundary-is.md)'s mechanism while
keeping its boundary; exercises the widened autonomy grant of 2026-08-02.

## Context

Eight stored axes answered the one question *"may this tool run right now?"*:

```text
enabled · open_irreversible · confirm · trust_all · reader nomination ·
auto_external · auto_consequential · the skills-config file's off/ask/auto
```

Each was added for a real reason and each is tested — and together they produced this
week's bugs, every one of which was **two axes disagreeing**: the trust toggle that stored a
flag while the per-tool flags stayed shut; a server reading "Allow all: On" with all eight
tools blocked; a read-only search tool classed as an actuator because the reader nomination
is one field doing a second job.

The person named the disease precisely: too many patterns, each defensible, diluting the
system's ability to work as one thing. The permission model had become a place where a
correct change to one axis was a bug in another.

And the deepest axis never learns. **Every MCP tool lands in the irreversible band forever**
because a server's self-report is not evidence ([0054](0054-other-peoples-services.md)) —
correct on first contact, and absurd on the hundredth: the record holds thirteen read-back
confirmed outcomes proving a light switch is a light switch, and the band still files it
next to a wire transfer. This system's whole thesis is that evidence beats claims. The
permission model was the one place that thesis did not reach.

## Decision

### One stored stance per tool: `off` · `ask` · `auto`

Everything else about a tool's permission is **derived, never stored**
([0054]'s own rule, applied to the rulebook itself). Absent means the band's default:

| band | default |
| --- | --- |
| Observe | `auto` — a read reports the world |
| Reversible | `ask` |
| Irreversible (and every unproven MCP tool) | `off` — deny-by-default, visible but blocked |

The envelope stays, as the person's two global dials, because it is the boundary [0051]
exists to draw. The reader nomination stays, because it is a **verification** concern
([0053](0053-honesty-about-what-it-did.md)) that was moonlighting as a permission bit —
it no longer carries one.

`trust_all` survives with exactly one meaning: a tool arriving on that server gets `ask`
instead of `off`. It is no longer consulted at decision time.

Deleted outright: `open_irreversible`, `confirm`, the opened-overrides map, and every code
path that reconciled them.

### The record graduates a tool

A tool in `ask` whose read-back has **confirmed real change enough times** may act without
asking — computed from outcomes at composition time, derived and never stored.

This is the authorized shape exactly: deterministic finding (a count of `changed: true`
observations, in code), reversible action (the observations are the proof), disclosure
(every autonomous act already lands in the activity trail and the reply's action
disclosure — ADR 0053 — which says more than a one-time event would). The model proposes nothing here and cannot:
maturity is arithmetic, the same instrument as a notion's citations ([0057](0057-thinking-between-turns.md)).

Bounds, all load-bearing:

- **`PROVEN_AFTER` confirmed observations** (3) — the same maturity arithmetic as notions.
- Only `changed: Some(true)` counts. A claim without read-back is the thing that can be
  untrue; an unread effect proves nothing.
- Graduation lifts `ask` to `auto` **only while the envelope allows consequential
  actions** — narrow the envelope and every graduate asks again.
- `off` graduates to nothing. A stance the person set is a decision, and the record does
  not overrule decisions ([0060](0060-what-the-turn-is-offered.md) learned this pointing
  the other way).

### What burns

The permission ladder in `classify` is rewritten from `(band, stance, proven, envelope)`
alone. The API keeps its routes — the console's Allow/Block buttons now write stances — but
the stored model behind them is one column. Existing installs migrate: `enabled=false → off`,
`confirm=true → ask`, `open_irreversible=true → ask`, opened MCP tools → `ask`, everything
else absent.

## Consequences

- **A proven tool stops asking.** The butler turns the lights off at night without a tap,
  because thirteen observations say it can be undone by morning. This is the single largest
  step toward the person's stated goal this repository has taken.
- **Two axes cannot disagree when there is one axis.** The bug class of the week is
  structurally gone.
- **First contact is unchanged.** A brand-new tool is `off`; nothing about graduation
  reaches a tool nobody vetted.
- The console's finer distinctions collapse — "opened but confirm-each-use" and "enabled
  but blocked" become one word. Deliberate: those distinctions are the dilution.
- Migration risk: a stance derived wrongly is a tool that asks when it should act, or is
  blocked when it was open. The migration errs toward `ask`, the recoverable direction.

## Rejected

- **Keeping the axes and documenting them better.** The bugs were not documentation bugs.
- **Letting the model request graduation.** Maturity is arithmetic or it is nothing.
- **Graduating on claims** (`changed` unset counting toward proof). The claim is the thing
  that can be untrue; that is the oldest rule here.
- **Storing the proven set.** Derived-never-stored is what keeps it honest under a purge
  and correct after an outcome is deleted.
- **A percentage threshold instead of a count.** A ratio invites gaming by volume and reads
  as tunable; a count is an argument somebody has to win in code review.
