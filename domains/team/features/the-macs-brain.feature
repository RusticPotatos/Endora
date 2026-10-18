Feature: The Mac's brain
  A member may think with the owner's Mac (ADR 0130): a strong model on a laptop the owner
  uses all day. The node tells HQ whether the Mac is free; the member's turn waits until it is.
  Only one turn runs on the Mac at a time, and a member on the Mac never falls back to Claude
  or to the house's own model: the work waits, and its wake is not lost.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Zed is the developer
    And Rae is the developer
    And the goal "Keep the home lab healthy"

  Scenario: The Mac is one more brain the owner may choose
    When the owner sets Zed's brain to "mac"
    Then Zed thinks with "mac"
    And a Mac turn is not paid from the team's pool

  Scenario: A turn waits while the owner is using the Mac and runs when it is free
    Given Zed thinks with the Mac
    And the node says the Mac is not ready: "owner active"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "owner active"
    And Zed has no turn running
    When the node says the Mac is ready
    Then Zed's turn starts on "Cap sonarr's memory"

  Scenario: Only one turn runs on the Mac at a time
    Given Zed thinks with the Mac
    And Rae thinks with the Mac
    And the node says the Mac is ready
    And Zed has the task "Cap sonarr's memory"
    And Rae has the task "Tidy the compose files"
    When Zed's turn starts
    And a tick passes
    Then Rae has no turn running
    And Rae's turn on "Tidy the compose files" is waiting for the Mac: "another turn is using the Mac"
    When Zed's turn ends
    Then Rae's turn starts on "Tidy the compose files"

  Scenario: On battery nothing runs
    Given Zed thinks with the Mac
    And the node says the Mac is not ready: "on battery"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    And an hour passes
    And the node says the Mac is not ready: "on battery"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "on battery"
    And Zed has no turn running

  Scenario: The Mac busy switch holds everything
    Given Zed thinks with the Mac
    And the node says the Mac is not ready: "busy"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "busy"
    And Zed has no turn running

  Scenario: Nothing falls back to another brain
    Given Zed thinks with the Mac
    And the node says the Mac is not ready: "owner active"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    And an hour passes
    And the node says the Mac is not ready: "owner active"
    And a tick passes
    Then Zed still thinks with "mac"
    And Zed has no turn running
    And the pool has paid nothing

  Scenario: A member on the house's own model is not held by the Mac
    Given Zed thinks with the Mac
    And the node says the Mac is not ready: "on battery"
    And Zed has the task "Cap sonarr's memory"
    And Rae has the task "Tidy the compose files"
    When Rae's turn starts
    Then Zed has no turn running

  Scenario: A node that has gone quiet counts as the Mac being away
    Given Zed thinks with the Mac
    And the node says the Mac is ready
    And 5 minutes pass without the node speaking
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "the node has not said lately"
    And Zed has no turn running

  Scenario: A new HQ does not guess that the Mac is free
    Given Zed thinks with the Mac
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "the node has not said yet"

  Scenario: A turn cut short because the Mac went away is not held against it
    Given Zed thinks with the Mac
    And the node says the Mac is ready
    And Zed has the task "Cap sonarr's memory"
    When the owner comes back to the Mac under Zed's turn 4 times
    And the node says the Mac is ready
    Then Zed's turn starts on "Cap sonarr's memory"
