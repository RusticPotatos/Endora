Feature: A handover when work changes hands
  Rung 5 of closing the gap to Claude Code: fewer handoffs (ADR 0142, amended 2026-10-30).
  When an item changed hands (a climb after three failures, a paused member's hand-up, a
  reviewer's send-back), the next member started over: in the baseline of 28 October the same
  container logs were read at every rank, and a send-back's reasons had to be found again. Now
  HQ writes a handover from the record, with no model: what the item asks and what is cited of
  each, why it was sent back, the readings already taken, to cite rather than take again, and
  the members' last words. The next member is told it, and it is posted on the item's thread as
  Endora's, so the owner sees what was handed over.

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

  Scenario: An item that climbs a rank carries what was tried to the next member
    Given "Find what stops gluetun" is Bo's
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo posts "The VPN credentials look stale."
    And Rae sends "Find what stops gluetun" back 2 times: "gluetun still exits after a minute."
    And Bo takes 3 more turns on "Find what stops gluetun" that show nothing
    Then "Find what stops gluetun" is Rae's
    And the thread of "Find what stops gluetun" shows Endora's handover to Rae
    When Rae's turn starts
    Then Rae's briefing says "failed 3 times at Bo's rank"
    And Rae's briefing says "1. gluetun is running and healthy"
    And Rae's briefing says "container_logs gluetun: AUTH_FAILED from the provider"
    And Rae's briefing says "Rae: gluetun still exits after a minute."

  Scenario: A paused member's item goes one rank up with a handover
    Given "Find what stops gluetun" is Bo's
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo's turn ends
    And the owner pauses Bo
    And a tick passes
    Then "Find what stops gluetun" is Rae's
    And the thread of "Find what stops gluetun" shows Endora's handover to Rae
    When Rae's turn starts
    Then Rae's briefing says "Bo is paused"
    And Rae's briefing says "container_logs gluetun: AUTH_FAILED from the provider"

  Scenario: A send-back gives the worker the reviewer's reasons and his own readings to cite
    Given "Find what stops gluetun" is Bo's
    When Bo takes "Find what stops gluetun" and reads gluetun's logs: "AUTH_FAILED from the provider"
    And Bo asks for review of "Find what stops gluetun", citing his reading
    And Rae sends "Find what stops gluetun" back: "The health check still fails on line 12."
    Then the thread of "Find what stops gluetun" shows Endora's handover to Bo
    When Bo's turn starts
    Then Bo's briefing says "sent back by Rae"
    And Bo's briefing says "Rae: The health check still fails on line 12."
    And Bo's briefing says "container_logs gluetun: AUTH_FAILED from the provider"
    And Bo's briefing says "1. gluetun is running and healthy [cited"
