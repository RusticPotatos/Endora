Feature: Only the CEO reaches the board
  The owner is the board (ADR 0111). A member's question goes to whom it
  reports (ADR 0113), the lead's to the CEO, and only the CEO's reaches the
  owner's desk, as a decision with a recommendation.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"

  Scenario: Only the CEO reaches the board
    Given Linus has the task "Sonarr keeps running out of memory"
    When Linus asks "May I raise sonarr's memory limit to 2 GB?"
    Then the question goes to Ada
    And the owner's desk is empty
    When Ada asks "Linus wants sonarr at 2 GB. Spend it?"
    Then the question goes to Mara
    And the owner's desk is empty
    When Mara puts the decision "Give sonarr 2 GB?" to the board, recommending "Yes", because "it is the one container short of memory"
    Then the owner's desk holds 1 decision

  Scenario: The CEO is not woken for being named
    Given Ada has the task "Plan the week"
    When Ada posts "@Mara have a look at this"
    Then Mara is not woken: not_for_the_ceo

  Scenario: Opus is the CEO's alone
    When the owner sets Ada's brain to "claude-opus-5-5"
    Then it is refused
    And Mara thinks with "claude-opus-5-5"
