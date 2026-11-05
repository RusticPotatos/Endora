Feature: Every pass fits one fresh turn
  Ralph's rule: each pass of the loop starts fresh and does one thing within it; what it learned
  is carried in files, not in the conversation (ADR 0142, amended 2026-10-30). HQ's work turns
  broke it: measured turns read 110,000 to 200,000 prompt tokens, and a long one on the Mac ran
  its calls into the relay's timeout. Now a work turn ends cleanly when its prompt reaches its
  budget, and the same member goes on in a fresh turn, from HQ's handover instead of the whole
  conversation. Running out of room is working, not failing; when it counts as a failure after
  all is "A long item makes progress across turns". The worker's word that a turn ran out of
  room is not enough: HQ goes on only when the relay metered at least half the turn's budget,
  which is 60,000 prompt tokens on the house's own models (ADR 0142, amended 2026-10-31).

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"
    And the owner sets the loop "ansible-nas" to 1 item at a time
    And the loop "ansible-nas" has the item "Find what stops gluetun", done when "gluetun is running and healthy"
    And a tick passes

  Scenario: A turn that runs out of room goes on with the same member, from a handover
    Given "Find what stops gluetun" is Bo's
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo's turn runs out of room after reading 50,000 prompt tokens, having said "The VPN credentials look stale."
    Then Bo is woken to continue "Find what stops gluetun"
    And "Find what stops gluetun" is Bo's
    And "Find what stops gluetun" has no failures
    And the thread of "Find what stops gluetun" shows Endora's handover to Bo
    When Bo's turn starts
    Then Bo's briefing says "out of room, continuation 1 of 3"
    And Bo's briefing says "container_logs gluetun: AUTH_FAILED from the provider"
    And Bo's briefing says "Bo: The VPN credentials look stale."

  Scenario: A turn that says it ran out of room but read little is not continued
    Given "Find what stops gluetun" is Bo's
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo's turn runs out of room after reading 5,000 prompt tokens, having said "The VPN credentials look stale."
    Then the answer says "The turn is not continued: HQ did not see its prompt reach its budget."
    And Bo is not woken to continue "Find what stops gluetun"

  Scenario Outline: A turn on a cached model counts its conversation once
    Every call of a turn resends the conversation so far. Claude's and DeepSeek's providers
    cache the part a call repeats, so on them the budget holds the conversation, each byte
    counted once; summing every resend counted Rae's 50 KB conversation as 238 KB (955, 1 Nov).
    The house's own models read every resend again, slowly, so their turns still sum them.
    Then a turn on <model> counts its budget by <counting>

    Examples:
      | model    | counting         |
      | haiku    | its conversation |
      | sonnet   | its conversation |
      | opus     | its conversation |
      | deepseek | its conversation |
      | local    | every call       |
      | mac      | every call       |
