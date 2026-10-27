Feature: A deploy under a turn is the platform's interruption, not the member's failure
  A deploy restarts HQ, the node and the runner in turn, and a turn caught by one of them did
  nothing wrong (ADR 0124, amended). Live, a deploy cut a developer's turn short, HQ read the
  task as stopped by accident, its lead was asked to look, and the CEO handed it to another
  area's lead. Now HQ tries the turn once more as the same member, does not hold the lost one
  against the member's retries, and raises nothing: no finding, no recovery wake, no lead asked
  to look, nothing on the owner's desk. Only the first interruption is free.

  Background:
    Given HQ keeps its team in a file
    And Mara is the CEO
    And Ada is the lead
    And Zed is the developer
    And the goal "Keep the home lab healthy"
    And Zed has the task "Cap sonarr's memory"

  Scenario: A deploy that restarts HQ under a turn costs the member nothing
    When Zed's turn starts
    And HQ restarts
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" waits to be tried again
    And that was an interruption, and Zed has all his retries
    And Zed was not nudged about "Cap sonarr's memory"
    And Ada was not asked to look at "Cap sonarr's memory"
    And the owner's desk is empty
    When a minute passes
    And a tick passes
    Then Zed's turn starts on "Cap sonarr's memory"

  Scenario: Only the first interruption is free
    When Zed's turn starts
    And HQ restarts
    And a tick passes
    And a minute passes
    And Zed's turn starts
    And HQ restarts
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" waits to be tried again
    And that was the second interruption, and Zed has one retry fewer
