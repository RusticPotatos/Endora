Feature: The owner's new work is taken up at once
  HQ takes up work on its tick, every thirty seconds on the NAS. Work the owner
  gave used to wait for the next tick, so it sat for anything up to the whole
  interval: 8.7 seconds in the agentic-harness climb's live run on 4 Nov, before
  any member had read a word of it. Giving work now asks HQ for a pass at once.
  The tick still runs on its own clock, and nothing else asks for a pass.

  Background:
    Given Sol is the developer
    And the goal "Ship the watch tracker"

  Scenario: Giving work asks HQ for a pass, and that pass takes it up
    When Sol is handed the task "Fix the failing test"
    Then HQ has been asked for 1 pass
    When the pass HQ was asked for runs
    Then Sol is woken about it

  Scenario: Giving several things at once asks for a pass each, and one pass takes them all up
    Given Ada is the lead
    When Sol is handed the task "Fix the failing test"
    And Ada is handed the task "Plan the release"
    Then HQ has been asked for 2 passes
    When the pass HQ was asked for runs
    Then Sol has a turn on "Fix the failing test"
    And Ada has a turn on "Plan the release"

  Scenario: A member's own split asks for no pass
    Given Sol has the task "Fix the failing test"
    And the pass HQ was asked for runs
    When Sol splits off the task "Write the regression test" under "Fix the failing test"
    Then HQ has been asked for 1 pass

  Scenario: Work the owner may not give asks for no pass
    When the owner gives Sol work with no criteria
    Then HQ has been asked for no pass

  Scenario: The pass HQ was asked for still keeps the owner's kill switch
    When the owner pulls the kill switch
    And Sol is handed the task "Fix the failing test"
    Then HQ has been asked for 1 pass
    When the pass HQ was asked for runs
    Then Sol has no turn on "Fix the failing test"

