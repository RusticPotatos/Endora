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

  # The owner's Chess tab (31 Oct): every league game, a page at a time, newest first, each
  # with its result, its moves and its PGN. The league is not monthly: a round is its games
  # since the owner's last reset, and the list keeps every round's.
  Scenario: The owner reads every finished league game, newest first
    When the owner starts the league in "Chess" with "claude-haiku-5-5" and "local"
    And white resigns the league's game in play
    And the owner resets the league
    And white resigns the league's game in play
    And the owner resets the league
    Then the league's finished games read, newest first
      | round | white        | black | result    | half-moves |
      | 2     | Claude Haiku | Local | black_won | 0          |
      | 1     | Claude Haiku | Local | black_won | 0          |
    And the league's round is 3, since a reset on "17 Oct"
