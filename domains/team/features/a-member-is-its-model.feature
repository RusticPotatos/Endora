Feature: A member is its model
  Every turn of a member's runs on its model (ADR 0131, amended by ADR 0142): the two brains a
  member once had, one to work and one to judge, are one. A turn on Claude is paid from the
  pool; a turn that cannot run on its model now waits, and says why. It is never given another
  brain, and a wake is never thrown away for money.

  Background:
    Given a pool of $0.50 a day

  # --- Any member may be any model -------------------------------------------------------

  # Since one team (ADR 0142) a title is a name only: Opus is no member's alone.
  Scenario: The owner may give Opus to any member
    Given Bo is the developer
    And Cy is the researcher
    And the goal "Keep the home lab healthy"
    When the owner sets Bo's brain to "claude-opus-5-5"
    Then it is not refused
    When the owner sets Cy's brain to "claude-opus-5-5"
    Then it is not refused

  # --- A paid turn waits for the money ------------------------------------------------------

  Scenario: A decision for a member out of budget waits, and runs after the reset
    Given Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    And Ada has no turn running
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
    And Ada's turn runs on "claude-haiku-5-5"
    And the budget chip counts 0 turns waiting

  Scenario: A decision waits for a top-up of the pool
    Given Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    When the owner tops up the pool by $0.50
    Then Ada's turn starts on "Cap sonarr's memory"

  Scenario: A turn on the house's model costs nothing
    Given Ada is the lead
    And the goal "Keep the home lab healthy"
    When Ada is handed the task "Look at sonarr"
    Then Ada's turn starts on "Look at sonarr"
    And Ada's turn runs on "local"
    And Ada's turn reserved nothing
    And the pool has paid nothing

  # --- A turn that waits is a wake, not a stall --------------------------------------------

  Scenario: Work whose turn waits for budget is held, not stuck, and nothing climbs
    Given Linus is the developer
    And Linus thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Linus's decisions
    When Linus is handed the task "Clear the stuck downloads"
    And a tick passes
    Then Linus's turn on "Clear the stuck downloads" is waiting for budget
    When the team is left alone for 6 hours
    Then the work on "Clear the stuck downloads" is held, not stuck
    And nobody was nudged, sent to decide or climbed to about "Clear the stuck downloads"
    And the owner's desk holds no stuck work

  Scenario: Work whose turn waits for the Mac is held, not stuck
    Given Linus is the developer
    And Linus thinks with "Mac"
    And the goal "Keep the home lab healthy"
    And the node says the Mac is not ready: "owner active"
    When Linus is handed the task "Clear the stuck downloads"
    And a tick passes
    Then Linus's turn on "Clear the stuck downloads" is waiting for the Mac: "owner active"
    When the team is left alone for 6 hours
    Then the work on "Clear the stuck downloads" is held, not stuck
    And nobody was nudged, sent to decide or climbed to about "Clear the stuck downloads"
    And the owner's desk holds no stuck work

  # --- Nothing is dropped ------------------------------------------------------------------

  Scenario: A wake out of budget is kept, not refused
    Given Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    Then no wake was refused for budget

  Scenario: Wakes for the same decision collapse into one waiting turn
    Given Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And Ada is woken to decide "Cap sonarr's memory"
    And Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    Then Ada has 1 turn waiting for budget
    And the budget chip counts 1 turn waiting

  Scenario: A turn waiting for budget is still there after HQ restarts
    Given HQ keeps its team in a file
    And Ada is the lead
    And Ada thinks with "Claude Haiku"
    And the goal "Keep the home lab healthy"
    And the pool's day is spent on Ada's decisions
    When Ada is woken to decide "Cap sonarr's memory"
    And a tick passes
    And HQ restarts
    And a tick passes
    Then Ada's turn on "Cap sonarr's memory" is waiting for budget
    And the budget chip counts 1 turn waiting
    When a day passes
    Then Ada's turn starts on "Cap sonarr's memory"
