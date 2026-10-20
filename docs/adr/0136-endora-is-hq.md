# 0136 - Endora is HQ

## Status

Proposed, 2026-10-20. Prepared with AI assistance (Claude). Waits for the owner's yes; the
questions it leaves to them are at the end of [the proposal](../proposals/hq-first.md). Nothing
it removes has been removed.

When accepted it supersedes the chat parts of earlier records, each of which will carry a dated
note: the turn and its context ([0056](0056-how-it-behaves-toward-you.md),
[0060](0060-what-the-turn-is-offered.md), [0061](0061-answers-worth-keeping.md),
[0067](0067-one-way-to-the-deep-model.md), [0068](0068-facts-are-arguments-not-recall.md),
[0072](0072-the-base-has-rungs-too.md), [0073](0073-a-dead-session-heals-itself.md), and
[0085](0085-a-question-about-the-world-is-not-answered-from-the-house.md) to
[0091](0091-what-the-deep-model-costs.md)); the brief and standing questions
([0074](0074-the-brief-is-a-standing-order.md), [0076](0076-standing-questions.md)); research
([0077](0077-seeing-something-through.md)); recipes
([0071](0071-capabilities-it-writes-itself.md)); the agenda loop
([0103](0103-the-agenda-loop.md)); the chat's parts of the model lists
([0105](0105-models-in-ordered-lists-by-data-tier.md): the composer's lock, the Deep list and
Ask deep); and, if the owner agrees, the brain ([0101](0101-the-brain.md)) and the mail reader
([0097](0097-it-reads-the-mail.md)).

## Context

Endora began as a personal butler: a chat with tools, a morning brief, things it noticed on its
own. Since [0102](0102-endora-hq.md) it also hosts HQ, a team of agents that does the home lab's
work and plays the Holodeck's scenarios, and the owner now spends their time there. Of the chat,
in the owner's words, it "is just kinda bad", and Endora should be primarily HQ.

[0100](0100-the-clean-slate.md) is how a removal was done before: the owner chose removal of code
and storage over switching things off, because off-but-present code still has to compile and be
understood, and a table nothing reads is data nothing can export or delete (constitution
section 6).

HQ leans on the node, but not on the chat. Its turns reach models through the node's relay,
which reads the AI providers and the Everyday list; reach the house's tools through the bridge
([0108](0108-the-bridge.md)), which reads each skill's stance and the Connections; reach the lab
through contracts ([0106](0106-team-contracts.md)), which read the home-lab observer
([0092](0092-the-home-lab-is-a-server-owned-read-view.md)); and every owner action is written to
the audit log. None of that reads the conversation, the brain, the brief, the autonomy envelope
or the agenda loop. The agenda loop and HQ's contracts both restart containers, each with its
own ledger.

## Decision

1. **HQ is the console's landing** on every device. The per-device Start on setting goes.
2. **The chat is retired, code and storage**: the turn and the `Butler` port, the conversation,
   the daily brief, research, the chat's outcomes, repairs and reliability, recipes, the
   autonomy envelope, the deep model and Ask deep, and the agenda loop. The node drops their
   tables when it starts, as 0100 did, and a fresh database never creates them. The handheld
   loses its chat tab.
3. **Kept as shared infrastructure**: sign-in, the vault, Activity and the audit log, the AI
   providers and the Everyday list, skills with their stances and settings (shown as the tools
   the team may use), tool servers, Connections, the home lab, and the house's own screens
   (music, speakers, smart home, media), which never depended on the chat.
4. **Settings holds six sections**: AI, Connections, Tools, Your sign-in, Vault, Your data.
5. **The work goes in order**: prepare without visible change, then hide (reversible), then the
   handheld, then a copy the owner takes, then removal, then the documents. A step deploys and
   is looked at before the next starts.
6. **Your data stops deleting what the team needs.** Delete everything no longer wipes the model
   lists or the skills' settings.

## Consequences

- Endora does nothing unasked outside HQ and the home lab's own pages: no brief, no pushes to
  the phone. A ping when the desk needs a decision would be new work.
- Roughly 25,000 lines and their tests go; `endora-application` shrinks to what the node still
  composes, and HQ no longer builds against the butler's contexts.
- What the chat knew is gone after the copy. The export loses its chat fields: a breaking change,
  marked so in the commits and the changelog.
- The constitution's preamble and section 9 still describe a butler forming beliefs about the
  person. Rewording them is the owner's, through section 11; it is proposed separately.
- HQ's own record is still in no export and no delete. That gap predates this record and is
  closed in its own piece.

## Alternatives considered

- **Hide the chat and keep its code.** Rejected for the reasons 0100 gives; hiding is used only
  as a short reversible step before removal.
- **Rebuild the chat on HQ** (a chat with the team). Not now: the owner's complaint is the chat
  itself, and HQ's task box already gives work to the team in words.
- **Keep the agenda loop for the house.** Rejected: its screens were the chat's, and it is a
  second restarter beside HQ's contracts on the same containers. HQ's routines and alarms
  ([0107](0107-routines-alarms-and-check-backs.md)) already watch the lab.
