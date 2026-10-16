# 0101 — The brain

## Status

Accepted, 2026-10-16, by the owner through a design interview (step 2 of
[the always-on butler plan](../plans/always-on-butler.md), questions Q1 to Q11 of that
round). Prepared with AI assistance (Claude).

This replaces the beliefs of [0052](0052-what-it-knows-about-you.md) as the place Endora keeps
what it knows about the person. Their confidence, decay, affirming and correcting go with
them; the rules for what is worth keeping stay. Nothing in
[the constitution](../constitution.md) changes, and its memory rights now cover the pages too.

## Context

Beliefs were rows in the database: a statement, a kind, a confidence that faded, cards to
confirm or correct. Used for real, they went stale and repeated themselves, and the owner
could only read them through cards. The clean slate ([0100](0100-the-clean-slate.md))
removed what produced the noise and started fresh.

The owner wanted what Endora knows to be something they can open, read and edit like notes,
in a format any agent could read, and for the models to find who the person is in one place
rather than piecing it together from settings and beliefs. They wanted the database to hold
app settings and configuration only.

## Decision

**The brain is a folder of markdown pages with its own git history.**

- `memory.md` is read whole on every turn. **What you told me** holds where they are based,
  how to address them and their stated preferences (the settings screen writes these lines).
  **What I know best** holds the few things that matter most, at most thirty lines.
- `index.md` lists every other page with its one line; `log.md` says what changed, when and
  why.
- Every other page has a `# Title`, a summary of what is true now, and a `## Timeline` of
  dated lines that are only ever added to. Endora creates and organises the pages itself, in
  up to three levels of folders. Pages link with `[[wikilinks]]`.

**Where it lives.** `ENDORA_BRAIN` in the node (`/brain` in the container), mounted from
`ENDORA_BRAIN_DIR` (or the `endora` role's `endora_brain_dir`), by default beside Endora's
other folders. The nightly backup writes it as a git bundle beside the database copies,
kept the same way. Git is in the image for this.

**Every change is a commit.** Anything edited outside Endora (from the Mac, on the share) is
committed first, as "Edited outside Endora", so a change of the person's is never overwritten
unseen. Page edits are small changes to the text, never a re-rendering: a fact is one
inserted line, a section edit keeps any line the person wrote there, and a fact cannot write a
heading.

**The folder is not trusted.** It is writable from outside and the node runs as root, so git
runs with hooks, fsmonitor, signing and system and global configuration off, and without the
template; no read or write follows a symlink; page names allow only `a-z`, `0-9`, `-` and up
to three levels, so a name can never leave the folder.

**What reaches the model.** Memory, whole but for its title, and one line per other page, in
the part of the prompt that changes rarely, so the prompt cache holds until something is
learned. A new skill, `read_page`, opens a page when the conversation needs its detail. The
deep model's door disguises memory and the page list as it did beliefs; research and the
brief's wording carry neither.

**How it learns.** After a conversation turn (no skill ran), the model proposes lasting facts,
each naming the page it belongs on. The same rules as before decide what is kept: no command
the person gave, nothing about Endora's own conduct, nothing tied to a day, no one-off ask or
passing remark. A kept fact is one dated line on its page and a line in Activity. A fact aimed
at memory, or at no page, goes to `unsorted`. **The pass reads only the person's own
messages**: a reply of Endora's may relay a mail or a web page, and a fact taken from that would
ride in every later turn from memory.

**How it organises.** In quiet time, and never while a turn runs, the local model (the one
chosen on the AI screen, always on the local endpoint) writes one grown page's summary again
from its timeline, then "What I know best" from the page list. A page whose latest change was
the person's is theirs and is not folded; "What I know best" is theirs once they have edited
memory and the section is not empty. What the model writes must be a summary or nothing
changes.

**The person's control.** About you shows what they told it, what it knows best and its pages;
a page opens rendered, edits as text, shows its history and can be deleted (not memory).
**Forget** removes every page and its history and keeps what they told it. **Delete
everything** removes the brain too. **Export** carries every page.

**The database** keeps app settings and configuration. The `beliefs` and `preferences` tables
go: what the person had told Endora moves into memory once, on the first start, and an existing
database drops both.

**The API.** `GET /v1/brain`; `GET`, `PUT` and `DELETE /v1/brain/pages/{path}`;
`POST /v1/brain/forget`. The `/v1/understanding` routes are gone, the export has `pages` and
no `beliefs`, and the context diagnostics report `memory` and `brain_pages`. The preference
routes keep their shape, backed by memory.

## Consequences

- One place, in a format any agent reads. When the agent company of the plan's step 7 exists,
  its workers still get only what Endora derives for a task, never the pages: the brain stays
  inside Endora's information boundary.
- The owner can read and fix what Endora knows in an editor, and every change, theirs or
  Endora's, is in the history.
- Model output never edits a page whole: facts are appended and summaries replaced, so a bad
  model answer costs one summary at most, and the timeline it came from is still there.
- A page can grow past what a turn carries; it is cut with a note rather than crowding the
  turn, and the index carries at most sixty pages.

## Alternatives considered

- **Keep beliefs, add pages beside them.** Two places for the same knowledge was the problem
  the owner named.
- **GBrain itself**, with its server and Postgres. More moving parts than a folder and git,
  for no gain at this size.
- **A Rust git library** instead of the `git` program. A large dependency for what a handful of
  commands do; the program is run with everything that could be planted in the folder turned
  off.
- **The whole brain in every prompt.** It would crowd the 16k window and break the prompt cache
  with every note.
- **Learning from the whole conversation**, as beliefs did. Rejected for the injection reason
  above.
