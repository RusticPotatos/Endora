# 0097 — It reads the mail

## Status

Accepted, 2026-10-15, by the owner through a design interview (recorded in
[the standing-jobs plan](../plans/standing-jobs.md)) and confirmed before building. The mail
itself is copied to the NAS by the `mail_mirror` role of the owner's ansible-nas, read-only
at the provider. Prepared with AI assistance (Claude).

## Context

The owner's words: look through the mail for anything truly important, sort it into buckets,
keep doing it. Mail is the most hostile text Endora will ever read: anyone can send it, and a
message can be written to instruct whatever reads it. It is also the most private.

## Decision

**Code decides everything it can; a model only describes.** For each message, code extracts
the facts a header and a MIME tree carry: sender, reply-to, recipients, the thread it belongs
to, whether the owner wrote it, whether the owner has written to its sender before, mailing-list
markers, the provider's authentication results, link domains, attachment names. Buckets and
"important" are decided from those wherever they can be.

**A quarantined reader, with no tools, is the only thing that reads a body.** It is the local
model and only the local model, given one message with every link replaced by `[link]`, told the
message is data and not instructions, and asked for one fixed shape: a bucket from a fixed list,
one plain sentence, a few yes/no flags, an amount, and up to three dates. Code validates that
shape and throws away anything else; the sentence is cut to length and stripped of addresses
and links. The reader cannot call a skill, fetch, or act, so a message that tries to instruct it
can at worst produce a wrong sentence. The butler never sees a body: only the checked digest,
marked as other people's words (ADR 0064), so a turn that reads it cannot act.

**Nothing is opened.** No link is followed, no remote image loaded, no attachment opened or
parsed; only its name, type and size are recorded.

**Endora keeps a digest, not the mail.** Per message: when, from whom, bucket, sentence,
flags, why it matters, and evidence. Bodies stay in the read-only copy on the NAS.

**Important** is a person the owner has written to, a reply in a thread the owner started,
money due, account security, an appointment or invitation, an order problem, and a real person
the owner has not answered in three days. Newsletters and promotions never are.

**Bills get evidence, never a verdict.** Authentication, whether the sender is new, lookalike
domains, links that go somewhere else, a reply-to that differs, how the amount compares with
earlier bills from the same sender, and the nearest calendar appointment (60 days before to 14
after). Endora never pays, clicks, or says a bill is safe.

**Dates are listed only when corroborated:** said in two or more messages, by a trusted sender,
or matching an earlier calendar event. Others are held back and counted.

**It reads in quiet time, on the local model, and speaks rarely**, on the same switch as the
rest of the lane (`ENDORA_OWN_WORK`): new mail first, then back over the last 90 days; older
mail is used only for lookups (has this sender written before). Results go to the activity trail,
a section appended to the morning brief after it is worded (so no mail text reaches the model
that words it, which may be the paid one), a console page, and chat only for something new from
a person the owner writes to often.

## As built (2026-10-15)

- A `mail` context (`domains/mail`) holds the facts, the sorting rules and the digest; the
  orchestration layer reads one message at a time (`usecases::read_one`) through an
  `UntrustedReader` port, implemented on the local model client in JSON mode, built in the
  composition from the local endpoint and never from the configurable or deep chain.
- **Forgery is checked before a sentence is believed.** Measured on the NAS's own model with
  synthetic mail: a phishing message that instructed the reader was kept out of *People* by
  code, but was filed as important account security with the attacker's own words as its
  sentence. So for the kinds of mail worth forging (money, security, orders, appointments,
  the app), the receiving server's verdicts are read: a failed check, or nothing vouching for
  the sender (neither DKIM nor DMARC passing), a reply-to elsewhere, or a near miss of a
  domain the owner hears from, marks it **may not be genuine**, and code writes its line
  instead of the model. New senders and links to other domains are recorded as evidence.
- **Dates are kept only if written.** The same run showed the model working out "Thursday"
  as the wrong day. A date is kept only if its day of the month appears in the message.
- Shown: a trail line for recent mail that matters; a section appended to the morning brief;
  a Mail screen in the console.
- **The calendar, through Home Assistant.** Read-only, through the house's existing connection
  (no new credential). A bill gets the nearest calendar entry from 60 days before to 14 after
  that shares a word with the sender ("Follows 'Dental checkup' on 2026-10-02"), or the note
  that there is none. Matching is by shared words, named as crude: it can only add evidence.
- **Coming up.** A date from mail is shown only when someone the owner writes to said it, two
  or more messages say it, or an earlier calendar entry shares a word with it; one already on
  the calendar that day is not repeated, and a message that may not be genuine contributes
  nothing. The rest are counted as held back.
- **The ping is nameless.** For fresh mail (under two hours old) from someone the owner writes
  to, chat says "Someone you write to sent mail that may need you", at most twice a day. It
  names nobody and quotes nothing, because chat is history the deep model may later see, and
  the escalation door disguises only the person's own messages.

## Consequences

- Reading a message costs a local-model call, a few seconds each on the NAS's GPU, and only
  when nobody is talking to it.
- The reader is a 7b model; its sentences will sometimes be wrong. Buckets and importance lean
  on code, which is why.
- `mail-parser` is a new dependency, justified: MIME is large, old and adversarial, and a
  hand-written parser is the more dangerous choice.
