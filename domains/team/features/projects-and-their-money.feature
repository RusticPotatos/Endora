Feature: Projects, their caps and their money
  A project serves one goal and may have a monthly cap (ADR 0113). A lead or
  the CEO proposes one; the CEO decides it; a cap is the owner's to grant.
  Money is only ever a cloud member's: a member on the house's model costs
  nothing and is never turned away for it.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Ada thinks with Claude
    And Linus is the developer
    And the goal "Keep the home lab healthy"

  Scenario: A project's cap is the owner's
    Given Ada has the task "Look over the lab"
    And Mara has the task "Weigh the proposals"
    When Ada proposes the project "Backups" with a cap of $5 a month
    Then the project "Backups" is proposed
    And the project "Backups" has no cap
    And the owner's desk is empty
    When Mara accepts the project "Backups" because "backups failed three times"
    Then the project "Backups" is active
    And the project "Backups" has no cap
    And the owner's desk holds a cap request for "Backups" of $5.00 a month
    When the owner grants the cap at $4
    Then the project "Backups" has a cap of $4.00

  Scenario: Local members work for free
    Given the project "Upkeep" led by Ada with a cap of $0.50 a month
    And Ada has the task "Look at sonarr" in the project "Upkeep"
    When Ada's turns on "Look at sonarr" spend $0.40 and then $0.06
    Then the project "Upkeep" is paused for its budget
    When Ada is woken about "Look at sonarr"
    Then Ada's turn is refused: out_of_budget
    When Linus is handed the task "Read the logs" in the project "Upkeep"
    Then Linus's turn starts on "Read the logs"
