Feature: The model league
  The owner's league seats each brain at the board against every other, and rates them by
  their results (ADR 0135). Since one team (ADR 0142) Opus is no longer the CEO's alone, so it
  can be seated in the league like any other Claude; a Claude plays only from a capped project.

  Background:
    Given Ada is the lead
    And Bo is the developer
    And the goal "Holodeck"
    And the Holodeck project "Chess" with a cap of $5 a month

  Scenario: Opus plays in the league
    When the owner starts the league in "Chess" with "claude-opus-5-5" and "local"
    Then it is not refused
    And the league's table lists "Claude Opus" and "Local"
