# Always-on butler: the design

Agreed with the owner in a design interview on 2026-10-16 (questions Q1–Q53). This is the
plan for Endora to be always working on something for its owner: noticing, deciding, handling
what it can, and telling them only at the right level. It replaces the background features
that produced noise. Prepared with AI assistance (Claude). Step 1, the clean slate, is built
([ADR 0100](../adr/0100-the-clean-slate.md)); the rest is not yet. Each part lands on its own
branch with the full gate, and the architecture changes get an ADR.

## 1. What goes, and why

The background work Endora already had looked busy and produced junk:

- beliefs filed from one-off questions;
- an event pinned to "tomorrow" that went stale;
- intentions duplicated four times;
- a daily check-in;
- a nightly loop forming more of the same;
- an own-work lane that re-asked failed questions;
- notions that never produced anything.

Its sources were wrong, a 7B model judged what mattered, and nothing knew what the owner
cares about. Removed:

- **About you** in its current form: kinds, confidence pills, "That's right / Not quite".
- **Notions**, **intentions** and their nudges, the **daily check-in**, the **nightly
  loop**, the **own-work lane** (and `ENDORA_OWN_WORK`), and the durable-goal machinery
  other than explicitly requested research.
- **Inbox.** Unprompted messages stay in the chat, and Activity is the single record.
- The 24 learned beliefs and 4 intentions are deleted ("start fresh"). What the owner told
  Endora themselves stays: where they are based, how to address them, stated preferences.
- The 4 am model self-tune is turned **off**, to be replaced by a model comparison run on
  purpose.

Kept: the morning brief (later the morning plan), and research the owner asks for (one card
while it runs, the result as a reply).

## 2. Knowledge: markdown files, organised by Endora

What Endora knows about the owner lives in **markdown files**, not the database. The layout
follows GBrain's: pages in folders, each with the current truth on top and a dated,
append-only timeline below, `[[wikilinks]]` between pages. Endora reads and writes them
itself; there is no GBrain server or Postgres.

- **No fixed structure.** Endora creates and organises its own pages and categories as it
  learns: work, routine, home and lab, hobbies (watches, tech), the owner's apps, and so on.
- **What it learns** is appended to the relevant page's timeline and folded into the summary
  on top. It never stores one-off questions, events, or anything tied to a day.
- **Where:** a folder on the NAS with local git history, never pushed anywhere public, and
  covered by the existing home backup. It can be edited from the console or opened from the
  Mac in any editor.
- **About you** becomes these pages, shown and edited in the console.

## 3. The agenda loop

One loop runs continuously over **real signals**: mail, calendar, home lab alerts,
containers, backups, network (UniFi, Pi-hole), media requests, the owner's app repositories,
and anything the owner has said they care about. For each signal it decides to **handle**
it, **look into** it (work over hours or days, with a reason), **note** it, or **tell** the
owner. Every decision is in Activity with its reason. At most **3** pieces of work run at
once, within a daily budget, and the owner can stop any of them; a stop teaches it not to
pursue that kind of thing again.

**How it tells the owner**, in four levels. Everything appears in Endora, and Discord is an
optional channel later:

1. **Handled:** done quietly, a line in Activity.
2. **FYI:** on the "Now" line and in the next summary.
3. **Needs you:** pinned at the top of the chat until answered.
4. **Urgent:** the same, plus Discord once set up. Never used for something that can wait.

A **"Now" line** at the top of the chat shows that it's working, e.g. "Watching 6 things ·
last checked the NAS 2 min ago · 1 needs you", and opens Activity. **Activity** records what
Endora did (actions, messages it wrote, research finished, settings changed, one line per
mail check), not raw device changes. Those still feed the chat's "what changed lately".

**Weekday support** comes first, because the owner's routine matters most and Mon–Fri is busy.
Endora decides how to support it rather than following a fixed checklist; a morning plan and
an end-of-day wrap are the expected first forms.

## 4. What it may do

- **On its own:** reversible things in the house and lab. Restart a container that stopped
  unexpectedly, retry a failed backup, request media the owner asked about, turn off
  something left on, organise what it learns.
- **Always asks:** anything involving money, messages to people, deleting, buying, or
  settings it cannot undo. The existing permission settings remain the hard limit.
- **Untrusted content never steers it.** Mail, web pages and anything outside the owner's
  own messages are data, never instructions. Any action whose reason traces back to such
  content always asks.

## 5. Models, by the book

- **Private content** (mail, notes, the house) is read by the **local model** only.
- **Decisions, escalations and code review** use a big model, given short summaries the
  local model wrote and never raw private content:
  1. **Claude through an Anthropic API key**, paid from the monthly API credits included
     with the owner's Max plan;
  2. **Sign in with ChatGPT** (OpenAI's official route for open-source, self-hosted apps);
  3. **DeepSeek** as the last fallback.
- **Never** the owner's Max or ChatGPT subscription login inside Endora's own code. Anthropic
  forbids third-party use of subscription credentials. Running `claude -p` in an always-on
  loop is allowed in principle, but it would risk the owner's Max account under the
  "ordinary, individual usage" clause.

## 6. Code: written, tested and reviewed like production

Endora's agents may write and run code, but only through this gate:

1. Tests written first and failing (red), then code until they pass (green), in a throwaway
   container.
2. Review by a model in a **separate session with a fresh context** (the same model is
   allowed, never the same context window).
3. Merge only when tests pass **and** the review approves. The check is code, not the
   model's word.

**Environments:**

- **Dev:** the agent node. Nothing real is reachable from it.
- **Test:** a staging copy (staging Endora, fake Home Assistant, fake mail), where code must
  work end to end.
- **Prod:** the owner's real systems.

**Promotion:**

- Dev → test is automatic after green tests and review.
- Test → prod is automatic only for kinds of action already allowed (section 4), after
  validation, with an automatic rollback.
- Any new kind of access waits for the owner's approval on the pull request.

**The company:** the engineering work runs on **Paperclip** (MIT) on the agent node: an org
chart of agents (engineer, tester, reviewer, ops), each with a budget, with the owner as the
board. The owner approves strategy, budgets, new access, and anything that tripped a halt.
Endora's agenda loop files tasks into it. It starts with a trial run on one real task,
judged against this section before anything depends on it; if it cannot hold to these
rules, the fallback is a lean version built into Endora. Later, a read-only **pixel office**
screen in the console, in AgentOffice's style, shows each agent at a desk with what it is
working on. It is a view only and cannot act.

Code lives in a private **Forgejo** server on the NAS. Agents work through pull requests,
with a token scoped to their own repositories.

## 7. Isolation and tripwires

- **Hardware:**
  - **dev node:** a Raspberry Pi 5 (8 GB), on the SD card for now (a USB SSD and the 27 W
    supply later);
  - **test environment:** an isolated container stack on the NAS, with outbound access
    blocked;
  - **prod:** the real systems.
- **Network:** the agent node sits on its own switch port and VLAN. Claude writes the UniFi
  rules, the owner enters them, and Claude verifies them from the node. Only the approved
  destinations are reachable (Forgejo, specific NAS services, the approved model APIs and
  search); everything else is blocked and logged. Agents never get root, so a second
  firewall on the node is out of their reach too.
- **Hard limits in code:**
  - the host allowlist;
  - no installing, no creating accounts or credentials, no changing its own permissions or
    allowlists, no messages to people;
  - per-hour caps on actions, tool calls, requests to one host, and model spend;
  - a fixed action list that only the owner can extend.
- **Tripwires that halt the loop:**
  - any attempt at more privilege than granted (sudo, the Docker socket, host files, new
    credentials, wider scopes);
  - any attempt to reach anything outside the allowlist;
  - the same action repeated over and over, many failures in a row, or hitting a cap.

  The loop stops, raises an urgent "Needs you", and only the owner can resume it. There is a
  **Stop everything** button in the console and a switch on the NAS that works even if the
  console doesn't.
- **An append-only log** of every decision and action (trigger, reason, action, result) that
  Endora can add to but never change or delete, shown in Activity and backed up.

## 8. Where it is built

- Endora code: the Endora repository (Apache-2.0).
- **Ansible**, all safe for public repositories, with every address, key and token in the
  vault and only fictional defaults in the repo:
  - Forgejo, a general-purpose server, as a role in the owner's **ansible-nas** fork;
  - the agent node, its firewall and the test environment, which are Endora-specific, in
    Endora's own `endora.homelab` collection, installed and pinned by ansible-nas.
- Every push is preceded by the personal-data check.

## 9. Order

1. **Clean slate:** the removals in section 1, Inbox folded into Activity, the fresh start
   of knowledge.
2. **The brain:** markdown pages, About you over them, learning that files into pages.
3. **The agenda loop:** signals, the four levels, the Now line, the action list, caps,
   tripwires, the append-only log, and the Stop switch. First signals: the home lab, mail
   and calendar, and media.
4. **Models:** the Anthropic API key from the Max credits, Sign in with ChatGPT, DeepSeek.
5. **The agent node:** hardening the Pi, the agent user, the UniFi VLAN rules (written by
   Claude, entered by the owner), and Forgejo.
6. **The pipeline:** dev → test → prod with the review gate; the test environment on the NAS.
7. **A lean agent company built into Endora** (changed 2026-10-16, after reading Paperclip's
   source), then the pixel office view. It borrows six of Paperclip's patterns: heartbeat
   and wake triggers; budgets reserved before dispatch, with auto-pause; an independent
   reviewer; "Allowed / Ask first / Off" per tool; propose-then-decide decisions for the
   owner; and the LLM-wiki layout for the brain. Endora does not run Paperclip: it is unsafe
   by default and its audit log can be deleted. A Paperclip trial, sandboxed on the NAS and
   connected to nothing, is a separate piece of work in the ansible-nas fork.

Each step is merged only with the owner's OK.
