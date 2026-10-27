Feature: A desk that interrupts only for decisions
  Live, over 17 and 18 October 2026, thirty items reached the owner's desk in 38.6 hours, about
  nineteen a day, and six of them were real decisions or approvals: the rest were stuck work, a
  guard's finding or a notice. One task was flagged by the stuck guard four times, and a ceremony
  finding came back three times after the owner marked it resolved. The desk now interrupts only
  for what needs the owner's word (ADR 0132). Notices wait in one "while you were away" digest,
  newest first. Stuck work and the coordination guards no longer reach the desk at all: in the
  loops a stall is one of the item's failures (ADR 0142).

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
