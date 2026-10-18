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

  # A member's daily share is its first claim on the pool, not a wall (ADR 0113, amended).
  Scenario: A member past its share borrows what the others left
    Given Ada has spent $1.00 of her share today
    Then Ada's share for today is spent
    When Endora hands Ada the task "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"

  # What the owner asks for is paid from the pool's day, never the member's share.
  Scenario: The owner's own request is the pool's, not the member's
    Given Ada has the task "Look at sonarr"
    When Ada's turn on "Look at sonarr" spends $0.40
    Then the pool shows $0.40 spent today
    And Ada's share shows $0.00 spent today

  # Borrowing stops at one more share; then the member waits for the owner's word.
  Scenario: A member may borrow one more share, then asks the owner
    Given Ada has spent $1.20 of her share today
    When Endora hands Ada the task "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"
    When Ada's turn on "Look at sonarr" spends $0.025
    And Endora hands Ada the task "Look at radarr"
    Then Ada's turn is refused: out_of_budget
    And the owner's desk holds one request about Ada being out for the day
    When the owner gives Ada $0.05 more today
    And Endora hands Ada the task "Look at mylar"
    Then Ada's turn starts on "Look at mylar"
