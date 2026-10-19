Feature: A turn waiting for the Mac survives HQ restarting
  A wake held for the Mac is kept in HQ's store, not in its memory (ADR 0130). A new HQ does
  not know whether the Mac is free until the node says, and does not guess.

  Scenario: A held wake is still there after HQ restarts, and runs once the node says the Mac is free
    Given HQ keeps its team in a file
    And Mara is the CEO
    And Zed is the tester
    And the goal "Keep the home lab healthy"
    And the node says the Mac is not ready: "owner active"
    And Zed has the task "Cap sonarr's memory"
    When a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "owner active"
    When HQ restarts
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "the node has not said yet"
    And Zed has no turn running
    When the node says the Mac is ready
    Then Zed's turn starts on "Cap sonarr's memory"
