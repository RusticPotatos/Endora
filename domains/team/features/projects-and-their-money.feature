Feature: Projects, their caps and their money
  A project serves one goal and may have a monthly cap (ADR 0113). A lead or
  the CEO proposes one; the CEO decides it; a cap is the owner's to grant.
  Money is only ever spent on judgment, which runs on Claude (ADR 0131): work,
  on the Mac or the house's model, costs nothing and is never turned away for
  it. A decision the money cannot pay for waits, and starts when it can.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Ada thinks with Claude
    And Linus is the developer
    And the goal "Keep the home lab healthy"

  Scenario: A spent cap holds a project's decisions, never its work
    Given the project "Upkeep" led by Ada with a cap of $0.50 a month
    And Ada has the task "Look at sonarr" in the project "Upkeep"
    When Ada's turns on "Look at sonarr" spend $0.40 and then $0.06
    Then the project "Upkeep" is paused for its budget
    When Ada is woken to decide "Look at sonarr"
    And a tick passes
    Then Ada's turn on "Look at sonarr" is waiting for budget
    When Linus is handed the task "Read the logs" in the project "Upkeep"
    Then Linus's turn starts on "Read the logs"

  # A member's daily share is its first claim on the pool, not a wall (ADR 0113, amended).
  # Ada on Sonnet has $1.37 of the day's $2.33 beside Mara on Opus (amended 2026-11-01).
  Scenario: A member past its share borrows what the others left
    Given Ada has spent $1.40 of her share today
    Then Ada's share for today is spent
    When Ada is woken to decide "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"

  # Round 3: shares followed price, so Ada on Haiku had $0.19 of a $16.60 day and stopped with
  # money in the pool (D21). The owner, 1 Nov: most of the money goes to Haiku, the cheapest
  # paid model and the one doing most of the work; the climbs to Sonnet and Opus draw on what is
  # left (ADR 0113, amended 2026-11-01).
  Scenario: Most of the day's money goes to the member on Haiku
    Given Cy is the researcher
    And Cy thinks with Claude Haiku
    Then Cy's share for today is more than half the pool's day
    And Cy's share for today is more than Ada's on Sonnet and Mara's on Opus together

  # What the owner asks for is paid from the pool's day, never the member's share.
  Scenario: The owner's own request is the pool's, not the member's
    Given Ada has the task "Look at sonarr"
    And Ada is woken to decide "Look at sonarr"
    When Ada's turn on "Look at sonarr" spends $0.40
    Then the pool shows $0.40 spent today
    And Ada's share shows $0.00 spent today

  # Borrowing stops at one more share; then the decision waits for the owner's word. Less
  # than an opening call on her Claude ($0.08 on Sonnet) left is as good as none. With Cy on
  # Haiku, who claims most of the day (ADR 0113, amended 2026-11-01), Ada's share is $0.57 and
  # her day reaches $1.14.
  Scenario: A member may borrow one more share, then asks the owner
    Given Cy is the researcher
    And Cy thinks with Claude Haiku
    And Ada has spent $1.00 of her share today
    When Ada is woken to decide "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"
    When Ada's turn on "Look at sonarr" spends $0.10
    And Ada is woken to decide "Look at radarr"
    And a tick passes
    Then Ada's turn on "Look at radarr" is waiting for budget
    And the owner's desk holds one request about Ada being out for the day
    When the owner gives Ada $0.08 more today
    Then Ada's turn starts on "Look at radarr"

  # Seed (D21, round 3): Ada had the day's allowance and reached her limit again with $3.78 in
  # the pool; HQ asked about a member once a day, so no card came and her work waited for the
  # next day. Capped again after an allowance, a member is asked about again.
  Scenario: A member out for the day again after the owner's allowance is asked about again
    Given Cy is the researcher
    And Cy thinks with Claude Haiku
    And Ada has spent $1.00 of her share today
    When Ada is woken to decide "Look at sonarr"
    And Ada's turn on "Look at sonarr" spends $0.10
    And Ada is woken to decide "Look at radarr"
    And a tick passes
    Then the owner's desk holds one request about Ada being out for the day
    When the owner gives Ada $0.10 more today
    And Ada's turn on "Look at radarr" spends $0.10
    And Ada is woken to decide "Look at sonarr"
    And a tick passes
    Then Ada's turn on "Look at sonarr" is waiting for budget
    And the owner's desk holds one request about Ada being out for the day
