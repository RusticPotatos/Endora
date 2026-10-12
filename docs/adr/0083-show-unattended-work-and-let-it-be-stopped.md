# 0083 — Show unattended work and let it be stopped

## Status

Accepted, 2026-10-12. Prepared with AI assistance (Claude).

## Context

ADR 0082 made every way a goal ends reach the chat once. The console itself still
had no goal screen: running work could not be seen except through the raw API,
could not be stopped from the browser, and a finished goal left nothing in the
console beyond the single chat message. The node already exposed
`POST /v1/goals/{key}/cancel` and every goal in `GET /v1/export`, which the
console loads on each refresh, so the data and the verb existed with nothing
using them.

## Decision

No new endpoint. The console's chat view gains a strip (`viewGoals`) beside the
research strip:

- **Running work** shows its objective, its phase, how many findings it has so
  far and its next step, with a Stop button that posts to the existing cancel
  route. The goal's key goes through `encodeURIComponent` in the button's
  `data-act`, because the dispatcher splits on `:` and a key may contain one.
- **Ended work** shows the five newest ended goals, each expandable to the same
  text the chat used. The server composes it: `goal_json` gains an `outcome`
  field, set only once a goal has ended, from `Goal::outcome_notice`, so the two
  surfaces cannot disagree about what happened.
- `outcome_notice` now covers every status. A cancelled goal reads "You
  cancelled…" with its reason labelled "Why it stopped", never as a shortfall;
  an unfinished one reads "Still working on…". Completed, partial, blocked and
  failed wording is unchanged from ADR 0082.
- While any research or goal is unfinished, the existing periodic refresh runs
  (`pollActiveWork`, formerly `pollActiveResearch`). Rendering goes through the
  same escaping as research and chat, so objectives and findings taken from web
  pages stay text.

No new dependency, framework or setting.

## Validation and limits

The console render check covers every state's visible label, running work's
phase and finding count, hostile objective text staying text, each ended goal's
outcome, the Stop control appearing only on running work, the strip appearing in
the chat view, and a recorded request showing that Stop posts to the encoded
cancel route with a credential. The existing verb and route scans confirm the
new button leads somewhere and the route exists. Rust tests cover the `outcome`
field for running and cancelled goals and the new wording per status.

Not established: this was rendered only in the Node check, not in a real browser
or on a phone. Progress updates by the periodic refresh, not push: a worker
checkpoint does not emit a change event. Only the finding count and phase are
shown, not per-step timing or live tokens. Stopping records the cancellation
durably; this ADR adds no new end-to-end proof that an in-flight tool call
dispatches nothing further afterwards. The goal list is read from the full
export, which grows with the number of goals ever created.
