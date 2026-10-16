# 0103 — The agenda loop

## Status

Accepted, 2026-10-16, by the owner through a design interview (questions Q1–Q9). Prepared
with AI assistance (Claude). Step 3 of [the always-on butler plan](../plans/always-on-butler.md).
The first slice is the containers, media downloads, mail and calendar; the home lab's alerts,
backups, uptime, disk and Endora's own nightly copy follow as further sources of the same loop.

## Context

Endora answers when spoken to, and a few things run beside the chat (the brief, the mail
reader, the home lab's dashboard), each with its own way of saying something. None of them
decides what deserves the owner's attention, nothing does anything about what it sees, and the
owner cannot tell that Endora is watching at all. The clean slate ([0100](0100-the-clean-slate.md))
removed the Inbox and the old proactive notices, and named this loop as what brings unattended
work back, with Activity as its record.

## Decision

One loop, in its own bounded context (`domains/agenda`), turns **signals** into **items**.

- **Signals come from what Endora already reads**, with no new credentials: the containers
  and the Sonarr and Radarr queues (the home-lab observer), the mail the reader has sorted,
  and the calendar for today and tomorrow. Each source reports a full observation or nothing;
  a source that could not be read changes nothing, so a failed read never closes an item.
- **Fixed rules decide**, per kind of signal: *handle*, *look into*, *note*, or *tell* at a
  level. The model does not decide, and in this slice it does not write the sentences either:
  they are fixed wording over checked facts. The owner can re-level any kind in Settings,
  within what that kind allows.
- **Four levels.** *Handled* (and noted): a line in Activity. *FYI*: on the Now line and in
  the next morning brief. *Needs you*: a card pinned at the top of the chat until the owner
  taps Done or Dismiss. *Urgent*: the same, plus one push through Home Assistant's
  notifications, at most five a day.
- **Handle means one action: restarting a container**, through the existing allowlisted
  container action, which refuses Endora's own and the other protected containers. Only a
  container seen to go from running to *crashed* (a non-zero exit other than the 137 and 143
  that a deliberate stop leaves) or to *unhealthy* is restarted. A container already stopped
  when the loop starts, or stopped cleanly, was most likely stopped on purpose and is left
  alone. At most once an hour per container and ten a day; a container that stops again
  inside the hour is not restarted but told as needs you, and a third restart in a day is
  urgent ("keeps dying").
- **Look into means watch with a deadline**: a container stuck restarting (30 minutes, then
  urgent) or a download stuck in the queue (a day, then needs you). It closes itself when the
  signal clears. At most three at once; past that, a new one is told at its deadline's level
  straight away.
- **One item, not a stream.** A signal that persists is one item, updated in place; when it
  clears, the item closes with the time. Mail items are events, not states: an FYI closes
  after a day, and a needs-you item stays until answered.
- **Stop teaches.** Every item has Stop, which records a rule ("leave the container *x*
  alone", "don't tell me about mail from *y*") and closes the item. Rules are listed in
  Settings and can be undone.
- **Mail never steers it.** Mail can only produce items that tell; nothing a message says
  can make the loop act. Mail that may not be genuine is never more than FYI, and an urgent
  push about mail names neither the sender nor the subject.
- **The mail ping folds in.** The fixed chat message about someone writing becomes an
  ordinary item: FYI from someone the owner writes to, needs you when they are waiting on a
  reply. Items are not chat messages, so nothing about mail enters chat history.
- **The Now line** at the top of the chat says the loop is working ("Watching 4 things ·
  checked the NAS 1 min ago · 1 needs you") and opens Activity.

## Consequences

- Restarts now leave a line in Activity, with the reason; before, the container action left
  none.
- The home-lab observer reports each container's id and exit code, which the restart and the
  crashed-or-stopped rule need.
- The loop keeps its items, restarts, taught rules, re-levels and pushes in its own tables in
  the node's database.
- More sources (alerts, backups, uptime, disk, the nightly copy, then the network and the
  owner's app repositories) are new signal kinds with their own rules, not a new mechanism.
- Model-written sentences, and a large model proposing, wait for a later slice (and step 4's
  API key); when they come, they word or propose, and the rules still decide.

## Alternatives considered

- **A model deciding what matters.** Rejected for now: the owner's earlier experience was of
  stale and invented "beliefs"; fixed rules are predictable, testable and easy to re-level.
- **Restarting every stopped container.** Rejected: Docker already restarts crashed
  containers that have a restart policy, so one that sits stopped was usually stopped on
  purpose.
- **Posting items as chat messages.** Rejected: chat is history a paid model may later see,
  and items about mail must stay in the house; pinned cards are drawn by the console instead.
- **A separate service, like HQ.** Rejected: the loop needs the node's own readers, actions
  and stores, and is small.
