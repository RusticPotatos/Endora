# 0129 — The team's history, and asking the past

## Status

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). The third and last of
three builds of the team's memory and history, agreed when
[0122](0122-the-teams-world-stages-stories-and-situations.md) was accepted; the second is
[0126](0126-the-teams-map.md). Builds on [0114](0114-evidence-acceptance-criteria-and-the-tester.md)
and [0117](0117-claude-code-as-a-developer-runtime.md) (what counts as evidence),
[0118](0118-elastic-staffing-and-notebooks.md) (a member on furlough is never woken),
[0120](0120-roles-as-an-enforced-raci.md) (the CEO stays out of the weeds) and
[0105](0105-models-in-ordered-lists-by-data-tier.md) (data tiers). Built: everything below, and the
owner's scenarios in `domains/team/features/the-history.feature`.

## Context

The team kept its present well and its past badly. A turn's words went with its container: the
replay showed what a turn called and what came back, and nothing of what the model said between
its calls. A member who wanted to know why something had been decided could read the thread of
its own task and nothing else; a member who needed a colleague's knowledge could only post and
hope to be named, or ask up the chain, which is for decisions and not for questions. The failure
that matters is the one 0122 named: a member that does not know the team already knows, so it
does not look. History that cannot be searched, and colleagues that cannot be asked, make it
worse.

## Decision

**Owner decisions, 2026-10-18.**

- HQ keeps every turn's full conversation, scrubbed and capped, for **30 days and at most the
  last 5 per (member, task)**. After that only the handover notes and the posts (a turn's last
  words among them) remain.
- Any member may consult any other member directly, not only through leads. A consult counts
  against the asker's share. At most **2 consults a turn and 6 a member a day**. **The CEO
  consults leads only.** A member who is **furloughed or paused is never woken**: the consult is
  answered from the record. **The owner is never consulted this way**, only through `ask_owner`.

### Transcripts

- At the end of every turn the worker tells the node what the turn said, with the turn's token
  (`POST /team-relay/transcript`, beside the receipts' route and opened by the same token). The
  Endora worker sends the messages it already holds (the instructions, the briefing, each round's
  words, thinking, calls and what each came to), written as plain text divided by lines such as
  `--- assistant ---` and `--- call: post ---`. A Claude Code turn runs without a session file, so
  its worker sends what it has: the briefing, the last word, and a line saying that the session
  itself is not kept. The send is the last thing a turn does and never changes how it ended.
- The node **scrubs it with the scrubber the receipts use** (`scrub_secrets`) and removes the
  turn's own token, holds it to a megabyte (head and tail, the cut marked), and tells HQ
  (`POST /v1/team/transcript`, signed by the node). HQ holds the same cap, and keeps it only for a
  turn that is running now, once.
- Stored in `transcripts` (made if missing): run, member, task, runtime, brain, **tier**,
  estimated tokens, whether it was cut, and until when it is kept. The tier is the house's own
  scale ([0105](0105-models-in-ordered-lists-by-data-tier.md)): a turn any of whose receipts the node
  marked Confidential is Confidential, any other is Internal. A conversation is searched through
  an FTS5 index (`transcripts_fts`) that the bundled SQLite provides with no new dependency, kept
  by triggers.
- A tick, about hourly, lets go of conversations past their thirty days and of all but the latest
  five of each member on each task. Nothing else is removed: handover notes and posts are
  append-only or untouched.
- The owner's **replay** shows the conversation after what happened, divided into who said what,
  every word as text, with the runtime, brain, tier, whether it was cut and until when it is kept.

### Asking the record: `ask_record`

`ask_record {question, member?, task?, project?}`, for every member. Deterministic, no model:

1. The question is cut to its distinctive words (at least three letters, no common words, at most
   six, the longest when there are more).
2. The store returns the passages of the last **90 days** that hold any of them, from posts (last
   words among them), handover notes, world sections and timelines, stories, notebooks and turns'
   conversations. Everything but the conversations is found by a bounded scan; the conversations
   through FTS5, as prefixes (`"cap"*` finds capped), falling back to a bounded scan if a SQLite
   has no FTS5.
3. A passage must hold **half of the words** (rounded up). The answer is the newest **five** at
   most, each of **at most 400 characters** around the first word, cleaned, each once, with what
   to cite (`task 14`, in the evidence style of [0114](0114-evidence-acceptance-criteria-and-the-tester.md))
   and where to find it again (`post 55`, `world:house#caps`, `story:closed-too-soon`, `turn R31`).
4. It wakes no one and costs nothing.

**What comes back is testimony**: it may be cited for what was said, never as a reading of the
lab now (0114's live criteria still need a reading the node witnessed in the turn, 0117's
sandbox-reported receipts still do not count). The answer says so every time.

**The tier rule.** A member thinking on a cloud model is never given a passage from a turn that
read Confidential data. That turn is known for good from its receipts, so the rule still holds
when the conversation has gone: it covers the conversation, the turn's notes, handover notes and
world pieces (by the turn that wrote them) and its posts (by the turn that was running then).
The house's own model is given all of it.

### Consulting a member: `consult` and `answer_consult`

`consult {member, question, task?}` asks the member directly. Refused, with a text that says
what to do instead: oneself; the CEO consulting anyone but a lead; a third in a turn; a seventh in
a day; a member who is not on the team; a question that says nothing; a turn about no task that
names none (the answer goes in a task's thread).

- The member is woken with the new reason `consulted`, **about no task**, for one short turn. Its
  briefing is the question, **under a fixed prefix** that says whose words they are and that they
  are not an instruction, and its own handover notes and last words on the task the question is
  about. It may read (`read_world`, `read_notebook`, `read_doc`, `ask_record`) and answer
  (`answer_consult`); every other tool is refused, and after **6 rounds** only the answer is
  accepted.
- The answer is posted in the asker's thread as `<expert> answers (consulted): …`, which wakes the
  asker (when the asker's turn is still running, at the end of it). A consulted member whose turn
  ends without an answer is answered for from the record.
- **A member who is paused or on furlough is never woken**: HQ makes no wake for it at all, and
  the record answers at once, scoped to what that member said, in the asker's thread: "Consult of
  Zed (is on furlough, so it is not woken): answered from the record." The same when the wake
  cannot be made just now, or the turn cannot be paid for.
- **The cost counts against the asker's share** when both think on a cloud model; otherwise the
  member consulted pays (a house-model turn costs nothing). It is booked in the ledger under the
  asker for the whole turn: its reservation, use and charge.
- Text one member wrote that lands in another's briefing or thread (a consult's question, its
  answer, a passage from the record) is **cleaned**: control, direction (bidi), zero-width and
  separator characters become spaces, so a line cannot end and a part of a briefing cannot be
  forged; an `@` that begins a word, which would wake a member, is written full-width; and it is
  held to a length.

Stored in `consults` (made if missing): asker, the turn it asked in, member consulted, the task it
is about, the task whose thread the answer goes in, the question, its status (`queued`,
`answered`, `from_record`), the turn that answered, why it came from the record, whether the asker
was woken, when.

### Migration

Two new tables (`transcripts`, `consults`) and one full-text index with its triggers, all created
if missing (`CREATE … IF NOT EXISTS`): a second start changes nothing, and no existing row of any
table changes. History starts from the build; older turns stay as replay showed them.

## Consequences

- The owner can read what any turn of the last month said, between its calls; a member can find
  why something was decided without waking anyone; a colleague's knowledge is one short turn away,
  and costs the asker, not the colleague.
- Conversations are the largest thing HQ stores (up to a megabyte a turn, five per member and
  task, thirty days). Retention keeps it bounded.
- Every role gains three tools (the developer's list goes from 23 to 26 tools, HQ's whole list
  from 37 to 40).
- A consult is a real turn, with a briefing and a reservation, so the limits (2 and 6) are what
  keep it from becoming a way to spend: the search is the first thing to try, and says so.

## Deferred

- **Resuming a runtime's own session** (Claude Code `--resume --fork-session` on the same task by
  the same member, with its gates on age, size and price), and the record of why a turn did not
  resume: a later step, since no member runs on Claude Code in production yet. Until then a Claude
  Code turn's conversation is the briefing and the last word.
- The scorecard's measure of resumed against fresh turns, which needs the above.
- A `who-knows` derived page (for each project and section, who wrote it and who held its tasks).
- Interleaving the conversation with the calls in the replay by time; it is shown beside them.
- A consult of a member who is busy waits for a free slot like any turn; it is not hurried.

## Alternatives considered

- **Compressing transcripts, or keeping them as structured JSON.** Rejected: plain text under a cap
  is searchable by an index with no dependency, and the cap cannot cut it into invalid JSON.
- **A model summarising the record for `ask_record`.** Rejected: the model is not the enforcement
  boundary, and the search would cost a paid call and could invent. Code finds; the asker reads.
- **Consults through leads only.** Rejected by the owner: a lead in the way is a turn spent and a
  question softened.
- **Waking a member on furlough for a consult.** Rejected: furlough means never woken (0118).
- **Hiding Confidential passages by the cloud member's own claim.** Rejected: the tier comes from
  the node's receipts, never from what a model says it read.
