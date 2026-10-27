Feature: How the team is doing
  The HQ screen's scorecard says whether the team's work is good, in numbers
  computed from HQ's own record: turns that showed nothing, work sent back,
  what a finished task cost and what dropped work wasted. No model's word
  counts; what the record cannot tell (a turn from before labels) is shown as
  unlabelled, never guessed.

  Background:
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"

  Scenario: A turn that only talks showed nothing
    Given Linus has the task "Look at sonarr"
    When Linus posts "Probably fine."
    And Linus's turn ends
    Then the scorecard counts 1 turn today, 1 of them showing nothing

  Scenario: What a dropped task cost is waste
    Given Linus has the task "Tidy the old stack"
    When Linus's turns on "Tidy the old stack" spend $0.10 and then $0.20
    And the owner drops the task "Tidy the old stack"
    Then the scorecard puts $0.30 of today's spend on dropped tasks
