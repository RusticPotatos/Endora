# 0141 - Confidential data is open to Claude; Restricted stays home

## Status

Accepted, 2026-10-25, by the owner. AI-assisted. Amends the Confidential rule of
[0105](0105-models-in-ordered-lists-by-data-tier.md) and [0108](0108-the-bridge.md) as the team
uses it, the cloud lead's list of [0109](0109-a-brain-per-member.md) and
[0120](0120-roles-as-an-enforced-raci.md), and
[0140](0140-confidential-asks-stay-on-the-house.md), which it makes unnecessary. Restricted is
unchanged.

## Context

The Confidential tier kept the owner's data from turns on a cloud model: container logs, the
media queues, Home Assistant, the network's view (Pi-hole, UniFi), mail, the daily brief, images
the owner sent, and Endora's record of them. A cloud lead or CEO saw only container logs and the
media queues. The team's real work reads exactly this data, so every guard built on it (help
withheld, then allowed only on the house's models; receipts hidden from cloud readers; record
passages withheld) stood between the team and its work. The owner, asked what would start going
to Anthropic and offered a narrower choice, chose: Confidential open to Claude, Restricted stays
home.

## Decision

1. **Confidential is open to every turn, on any model.** A turn on Claude may be given a
   Confidential tool's result, is shown Confidential receipt excerpts, and finds passages from
   turns that read Confidential data in the record.
2. **Restricted stays home.** A movement history, what the owner marks 🔒 and account numbers
   are still never offered to any turn, and only a local model sees them where Endora uses them.
3. **No help limits by tier.** A turn that read Confidential data may ask any colleague, whose
   answer runs on its tier as for any ask (0139), and may send information (0137). ADR 0140's
   house-only rule is withdrawn.
4. **Marks stay.** A turn still records that it read Confidential data, for the owner's history
   and the record; the mark no longer withholds anything from a cloud model.

## Consequences

- What the team reads of the house and the owner's life is sent to Anthropic when a Claude turn
  reads it. That cannot be recalled once sent.
- Leads, the CEO, escalations and colleagues on Claude work from the same facts as the house's
  models; cost is unchanged by this decision alone, since work turns stay on the house.
- Restricted data keeps its guard; a new tool's result that is not listed defaults to Restricted.
