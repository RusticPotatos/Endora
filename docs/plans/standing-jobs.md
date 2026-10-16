# Standing jobs: what Endora keeps doing, not only what it fixes

Proposed 2026-10-15. AI-assisted. The first job (house rhythms) is built and accepted in
[ADR 0096](../adr/0096-patterns-it-notices.md). The rest is a proposal that needs the
owner's decisions, listed at the end. [The always-on plan](always-on-work.md) is about work
that ends; this is about work that does not.

## What the goals are today, and what they are missing

The lane (ADR 0094/0095) starts a goal for a device that has stopped answering, a question
the butler failed, or a belief it is sure of. All three are **problems**. The owner asked for
something else: things it is constantly doing and iterating on, such as sorting the mail into
buckets and surfacing what matters, and finding patterns in the house over the day. Those are
not problems and they do not end. A goal is the wrong shape for them. A **standing job** is a
scheduled review over records Endora holds, deterministic wherever it can be, that speaks only
when the picture changes, and that leaves a living result.

| Job | Reads | Says | Needs from the owner |
|---|---|---|---|
| House rhythms | the transition log (built) | "the front door goes on around 07:30, on 11 of 14 days" | `ENDORA_UTC_OFFSET_MINUTES` for their hours |
| Inbox triage | the mailbox (does not exist yet) | "3 things worth a look", and the bucket counts | a decision on how it reads mail |
| Terminal sandbox | nothing; it is where work is done | not a job: a place for a job to run scripts | a decision on where it runs |

## House rhythms (built)

See ADR 0096. The one thing to know: it reads what the house's sensing records. If Home
Assistant exposes the router's connected devices as presence entities, their coming and
going is in the log and shows up here. If the owner means traffic and bandwidth through the
router, that is a different source (Prometheus, the router's own stats), and it would be the
next job that reads the same way: aggregate by hour of day in code, and let a model only put
the sentence into words.

## Inbox triage (built, first slice: ADR 0097)

*Built 2026-10-15.* The mirror runs on the NAS (Gmail over IMAP with an app password, iCloud
when added). Endora indexes every message, reads the last 90 days in quiet time through the
quarantined reader, sorts, flags what matters, and doubts forgeries by the receiving server's
verdicts. Still to build from the design below: bills checked against the calendar, dates
corroborated across messages and shown, and the chat ping for a person the owner writes to
often. The rest of this section is the design as proposed.

## Inbox triage (as proposed)

**What the owner asked for:** read the mail, find the truly important things, sort it into
buckets, keep doing it.

**What exists:** the `mail` skill reads a count and the latest subject and sender through a
Home Assistant sensor. It cannot list the inbox, and Endora holds no mail credential. That is
the real gap, and it is a trust decision, so it is the owner's.

**How it could read mail, least privilege first:**

1. **A read-only copy on the NAS (recommended).** `mbsync` (or `offlineimap`) runs on the
   NAS with an app password only the NAS holds, and keeps a Maildir. Endora gets a read-only
   mount of that directory. Endora never holds a mail credential, never talks to the mail
   server, and cannot send or change anything even if it were fully compromised. The
   cost: the sync tool is one more thing to run and keep updated.
2. **IMAP held by Endora, read-only (`BODY.PEEK`).** Simpler to set up and Endora owns the
   credential. A real trade: anything that gets into the node gets the mailbox.
3. **The Home Assistant IMAP sensor, extended.** Gives a latest message only. Not a list.

**How it would decide what matters.** By signals in the headers, which code can count, and
not by asking a model which emails matter (the model's 7b ceiling is measured in this repo,
and the plan already requires this). For example: a sender the owner has written to, a
reply in a thread the owner started, unread for days, a calendar invitation, a message from
a service with an order or a security code, and the opposite signals (`List-Unsubscribe`,
`Precedence: bulk`). The buckets are Endora's own view and live in its own database, not in
the mailbox: **it does not file, label, archive or delete anything.** That would reach
outside the box and needs the capability opened and the autonomy envelope widened, which is
a separate decision and not recommended until the buckets have been trusted for a while.

**What a model would do.** Put one plain sentence on each of the few messages that the
signals picked out, and nothing else. It runs on the local model only (the lane's rule), so
mail text never leaves the house. Headers first, as the existing skill's doctrine says: a
body is where one-time codes and reset links live. Reading bodies, even a short snippet, is
a later step with its own decision, because mail is other people's words and the model reading
it must not be able to act on it (ADR 0064 already makes a turn that read mail unable to act).

**What it would say.** A digest in the activity trail, and later the brief: "3 messages
worth a look: …", the bucket counts, and what changed since last time. Chat only if
something is both new and from a person the owner writes to often.

**Before any of this:** the egress disguise would have to learn mail's line shapes before a
chat turn may read mail and then escalate to the paid model (the disguise recognises two line
shapes today). The lane does not need that because it is local-only, which is a reason to
build this as a standing job and not as a chat skill first.

## Terminal sandbox (proposed)

**What the owner asked for:** somewhere for Endora to do things in a terminal that are safe.

**Why it is useful.** The strongest use is not running arbitrary commands. It is letting the
model write a small script to analyse data (the transition log, a CSV of network readings, a
mail digest) and run it, instead of describing the analysis in prose. That moves work from the
model's weakest skill (arithmetic over many rows) to the machine's strongest, and it is how a
standing job could iterate: write a better analysis, run it, compare.

**What makes it safe is the jail, not the model.** Models propose; the jail authorizes.

- One throwaway container per run on the NAS's Docker, removed afterwards.
- No network at all (`--network none`). No access to the host, to the node's database, to
  credentials, or to the mail. It is handed a **copy** of the data it was asked to analyse,
  read-only, and nothing else.
- A non-root user, all capabilities dropped, a read-only root filesystem, a small tmpfs for
  scratch, `no-new-privileges`, and hard limits on memory, CPU, processes and wall-clock time.
- A local image with a few tools (Python, `jq`, `sqlite3`, coreutils), built and pinned, never
  pulled at run time.
- Output capped, returned as text. A script's output is data to the model, not an instruction
  (the same rule as any tool result).

**What it is not.** A container shares the host's kernel, so this is strong against a model
that makes a mistake or is talked into something by a hostile email, and it is not a guarantee
against someone with a kernel exploit. If that matters, gVisor or a rootless runtime is the
next step. It is not an interactive shell for the owner, and it does not replace the existing
terminal job agent, which runs a fixed catalogue of deploy tasks and is deliberately unable to
run anything else.

## Decisions needed from the owner

1. **Mail:** which of the three ways of reading it, or none. Recommended: the read-only copy
   on the NAS. Whatever is chosen, the credential is entered by the owner through a private
   setup step, never by Endora or by an assistant.
2. **Mail:** headers only to start (recommended), or short body snippets.
3. **Sandbox:** may it be built on the NAS's Docker, and is "no network" acceptable (it means
   it cannot install packages or fetch anything, by design).
4. **Network:** does "home network" mean the devices Home Assistant already sees, or router
   traffic and bandwidth? The second needs a source that is not yet connected.
5. **Hours:** set `ENDORA_TIME_ZONE` so patterns are in the owner's time, across clock changes.

## What can be built before any of those

Everything except the readers. The bucketing rules over a list of message headers are pure
functions and can be built and tested on synthetic mail now; the sandbox jail's command line
can be built and tested against a fake `docker`, as the backup work was. Neither touches a
real mailbox or a real container until the owner decides.
