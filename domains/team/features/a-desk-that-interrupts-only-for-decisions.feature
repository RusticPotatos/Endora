Feature: A desk that interrupts only for decisions
  Live, over 17 and 18 October 2026, thirty items reached the owner's desk in 38.6 hours, about
  nineteen a day, and six of them were real decisions or approvals: the rest were stuck work, a
  guard's finding or a notice. One task was flagged by the stuck guard four times, and a ceremony
  finding came back three times after the owner marked it resolved. The desk now interrupts only
  for what needs the owner's word (ADR 0132). Stuck work, findings and notices wait in one
  "while you were away" digest, newest first, each with the choices it always had; one finding is
  kept for a task and a guard, refreshed as it is seen again; and what the owner judged is not
  raised again until there is new evidence.

  # --- What interrupts, and what waits ---------------------------------------------------------

  Scenario: A decision for the board still interrupts
    Given Mara is the CEO
    And Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Sonarr keeps running out of memory"
    When Linus asks "May I raise sonarr's memory limit to 2 GB?"
    And Ada asks "Linus wants sonarr at 2 GB. Spend it?"
    And Mara puts the decision "Give sonarr 2 GB?" to the board, recommending "Yes", because "it is the one container short of memory"
    Then the owner is interrupted by 1 item
    And the digest is empty

  Scenario: Stuck work waits in the digest and does not interrupt
    Given Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Clear the stuck downloads"
    When "Clear the stuck downloads" is left stuck with no lead to take it
    Then the owner is interrupted by 0 items
    And the digest holds 1 item: stuck work on "Clear the stuck downloads"

  Scenario: A guard's finding waits in the digest and does not interrupt
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the project "Home lab upkeep" led by Ada
    And Ada has the task "Plan the backups" in the project "Home lab upkeep"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the owner is interrupted by 0 items
    And the digest holds 1 item: a finding "Talk about work"

  Scenario: Acting on an item takes it out of the digest
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    When Endora tells the owner "Linus restarted sonarr: it was unhealthy. It is now running."
    Then the digest holds 1 item: a notice "Linus restarted sonarr"
    When the owner marks it seen
    Then the digest is empty

  Scenario: The outcomes count what interrupted the owner and what went to the digest
    Given Mara is the CEO
    And Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Sonarr keeps running out of memory"
    When Linus asks "May I raise sonarr's memory limit to 2 GB?"
    And Ada asks "Linus wants sonarr at 2 GB. Spend it?"
    And Mara puts the decision "Give sonarr 2 GB?" to the board, recommending "Yes", because "it is the one container short of memory"
    And Endora tells the owner "Linus restarted sonarr: it was unhealthy. It is now running."
    Then the outcomes count 1 interruption and 1 item in the digest this week

  # --- One finding for a task and a guard ------------------------------------------------------

  Scenario: A second look at stuck work refreshes the one finding
    Given Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Clear the stuck downloads"
    When "Clear the stuck downloads" is left stuck with no lead to take it
    And an hour passes
    Then there is 1 stuck finding on "Clear the stuck downloads"
    And the stuck finding on "Clear the stuck downloads" was last seen again over an hour after it was found
    And the digest holds 1 item: stuck work on "Clear the stuck downloads"

  Scenario: Work that stops again after a look is the same finding, put in the digest again
    Given Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Clear the stuck downloads"
    When "Clear the stuck downloads" is left stuck with no lead to take it
    And Linus looks at "Clear the stuck downloads" and leaves it where it was
    Then the digest is empty
    When "Clear the stuck downloads" is left stuck with no lead to take it
    Then there is 1 stuck finding on "Clear the stuck downloads"
    And the digest holds 1 item: stuck work on "Clear the stuck downloads"

  # --- What the owner judged -------------------------------------------------------------------

  Scenario: Stuck work the owner judged is not raised again until it has moved and stopped again
    Given Linus is the developer
    And the goal "Keep the home lab healthy"
    And Linus has the task "Clear the stuck downloads"
    When "Clear the stuck downloads" is left stuck with no lead to take it
    And the owner judges the stuck finding on "Clear the stuck downloads" resolved
    Then the digest is empty
    When Linus looks at "Clear the stuck downloads" and leaves it where it was
    And "Clear the stuck downloads" is left stuck with no lead to take it
    Then there is 1 stuck finding on "Clear the stuck downloads"
    And the digest is empty
    When Linus moves "Clear the stuck downloads" on
    And "Clear the stuck downloads" is left stuck with no lead to take it
    Then there are 2 stuck findings on "Clear the stuck downloads"
    And the digest holds 1 item: stuck work on "Clear the stuck downloads"

  Scenario: A resolved ceremony finding is not raised again until the talk has grown
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    And the project "Home lab upkeep" led by Ada
    And Ada has the task "Plan the backups" in the project "Home lab upkeep"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the digest holds 1 item: a finding "Talk about work"
    When the owner says the finding is resolved
    Then the digest is empty
    When 14 days pass
    And HQ looks at how the team is doing
    And Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the digest is empty
    And there is 1 finding of "Talk about work"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the digest holds a finding "Talk about work" once
    And there are 2 findings of "Talk about work"
