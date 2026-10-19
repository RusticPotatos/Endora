Feature: The Mac as a work brain
  A member may work with the owner's Mac (ADR 0130): a strong model on a laptop the owner
  uses all day. Every member has two brains (ADR 0131), and the Mac is one of the two it may
  work with. The node tells HQ whether the Mac is free. Work that needs the Mac, such as a
  tester reading a diff, waits until it is: it never falls back to the house's own model, and
  its wake is not lost. Only one turn runs on the Mac at a time.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Zed is the tester
    And Rae is the tester
    And the goal "Keep the home lab healthy"

  Scenario: The Mac is one of the two brains the owner may choose to work with
    When the owner sets Zed's brain to "local"
    Then Zed works with "Local" and judges with "Claude Haiku"
    When the owner sets Zed's brain to "mac"
    Then Zed works with "Mac" and judges with "Claude Haiku"

  Scenario: A turn on the Mac costs nothing
    Given the node says the Mac is ready
    When Zed is handed the task "Cap sonarr's memory"
    Then Zed's turn starts on "Cap sonarr's memory"
    And Zed's turn runs on "mac"
    And Zed's turn reserved nothing
    And the pool has paid nothing

  Scenario: A turn waits while the owner is using the Mac and runs when it is free
    Given the node says the Mac is not ready: "owner active"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "owner active"
    And Zed has no turn running
    When the node says the Mac is ready
    Then Zed's turn starts on "Cap sonarr's memory"

  Scenario: Only one turn runs on the Mac at a time
    Given the node says the Mac is ready
    And Zed has the task "Cap sonarr's memory"
    And Rae has the task "Tidy the compose files"
    When Zed's turn starts
    And a tick passes
    Then Rae has no turn running
    And Rae's turn on "Tidy the compose files" is waiting for the Mac: "another turn is using the Mac"
    When Zed's turn ends
    Then Rae's turn starts on "Tidy the compose files"

  Scenario: On battery nothing runs
    Given the node says the Mac is not ready: "on battery"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    And an hour passes
    And the node says the Mac is not ready: "on battery"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "on battery"
    And Zed has no turn running

  Scenario: The Mac busy switch holds everything
    Given the node says the Mac is not ready: "busy"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "busy"
    And Zed has no turn running

  Scenario: Nothing falls back to another brain
    Given the node says the Mac is not ready: "owner active"
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    And an hour passes
    And the node says the Mac is not ready: "owner active"
    And a tick passes
    Then Zed works with "Mac" and judges with "Claude Haiku"
    And Zed has no turn running
    And the pool has paid nothing

  Scenario: A turn that does not need the Mac is not held by it
    Given the node says the Mac is not ready: "on battery"
    And Zed has the task "Cap sonarr's memory"
    And Ada has the task "Read the lab's disk use"
    When Ada's turn starts
    Then Ada's turn runs on "local"
    And Zed has no turn running

  # The Mac runs a large context, so its model is told more of the world than the NAS's small
  # one, and less than a lead has on Claude (ADR 0130, amended).
  Scenario: A briefing on the Mac carries more of the world than one on the NAS's model
    Given the node says the Mac is ready
    And Ada has the task "Read the lab's disk use"
    When Ada's turn starts
    Then Ada's turn runs on "mac"
    And Ada's briefing keeps to its budget of 2800 tokens

  Scenario: The NAS's model keeps its small briefing
    Given the node says the Mac is not ready: "on battery"
    And Ada has the task "Read the lab's disk use"
    When Ada's turn starts
    Then Ada's turn runs on "local"
    And Ada's briefing keeps to its budget of 1800 tokens

  Scenario: A node that has gone quiet counts as the Mac being away
    Given the node says the Mac is ready
    And 5 minutes pass without the node speaking
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "the node has not said lately"
    And Zed has no turn running

  Scenario: A new HQ does not guess that the Mac is free
    Given the node has said nothing about the Mac
    When Zed is handed the task "Cap sonarr's memory"
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for the Mac: "the node has not said yet"

  Scenario: A turn cut short because the Mac went away is not held against it
    Given the node says the Mac is ready
    And Zed has the task "Cap sonarr's memory"
    When the owner comes back to the Mac under Zed's turn 4 times
    And the node says the Mac is ready
    Then Zed's turn starts on "Cap sonarr's memory"
