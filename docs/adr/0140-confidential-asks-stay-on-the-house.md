# 0140 - Confidential asks stay on the house's models

## Status

Accepted, 2026-10-25, by the owner. AI-assisted. Amends
[0138](0138-one-conversation-and-assistance-lifecycle.md) (a Confidential turn may not ask) and
[0139](0139-one-team-escalate-by-tier.md) (a colleague's answer on a tier at least the asker's).
The Confidential guard (ADR 0105, 0108) is otherwise unchanged.

## Context

After ADR 0139 opened help to the whole team, no agent was offered it in production. The
team's real work reads the home lab through `container_logs`, `home_assistant` and the lab's
status, whose results are Confidential, and ADR 0138 withheld help from any turn that read
Confidential data, because a colleague might think with Claude. Help that real work can never
reach is not help.

The guard's purpose is that Confidential data does not leave the house: a Confidential result is
never given to a turn on a cloud model. A colleague who answers on the house's own models (the
NAS's model or the Mac) keeps it inside the house.

## Decision

1. **A Confidential turn may ask a colleague.** An ask is Confidential when the asking turn read
   Confidential data; the taint is permanent, so it is read from the turn, not stored again.
2. **On the house's models only.** A Confidential ask is refused unless the asked colleague and
   the asker both do their work on the house's models. The colleague's turn that reads the
   question, and the asker's turn that reads the answer, are refused to any turn on Claude.
3. **The taint travels.** The colleague's turn inherits the asker's Confidential mark, and the
   asker's result turn inherits the colleague's, so what either writes afterwards carries it.
4. **Unchanged.** `send_message` from a Confidential turn stays refused (its messages are read by
   any participant's turn, whatever its model). A Confidential ask's answer is never raised to a
   tier on Claude; ADR 0139's "at least the asker's tier" applies to asks that are not
   Confidential. Bounds, deadlines, sources and the owner's off switch are unchanged.

## Consequences

- Real work, which reads the lab, can ask a colleague; the answer is free and stays home.
- A member whose work runs on Claude can be asked only what is not Confidential.
- The owner's Work screen shows Confidential asks as it shows every other: it is the house.
