Feature: A probe's observer starts before the workbench
  The runner has no cucumber harness, by design: its dependencies are kept tiny because it
  holds Docker's socket. This file is the owner-readable spec, mirrored one to one by unit
  tests in app/runner/src/runner.rs, each named after its scenario.

  A benchmark probe asks whether a tool that failed was tried again. What the agent says of its
  tool is no evidence, so the runner can start a small observer for the item first, and then
  start the turn's workbench inside the observer's network, where the tool reaches it on
  loopback. The observer's records stay in a container the agent cannot reach. It is off unless
  the runner is told which projects to watch, and HQ only says which project a turn is in.

  Background:
    Given the runner has the workbench, as the Pi's does
    And the item "Publish the release" is in project 57

  Scenario: Nothing is observed unless the runner was told to
    Given the runner was given no projects to watch
    When a turn on the item starts
    Then the turn's container is made exactly as before
    And no observer is made

  Scenario: A project the runner does not watch has no observer
    Given the runner watches projects 57 and 58
    When a turn starts on an item in project 99
    Then no observer is made

  Scenario: A watched project's observer starts before its workbench and holds the fence
    Given the runner watches project 57
    When a turn on the item starts
    Then the observer is started before the workbench
    And the observer has the runner's own image and its own fixed command
    And the observer has a random token of its own and nothing else of the turn's
    And the observer has no mount or volume, only a megabyte in memory for its /data
    And the observer shares no processes and has no privileges
    And the workbench joins the observer's network
    And the workbench is told where the observer is, on 127.0.0.1
    And the workbench is read-only, with no capabilities and no new privileges, as before

  Scenario: An observer that never says it is serving is taken down and the turn never runs
    Given the runner watches project 57
    And the observer starts but never says it is serving
    When a turn on the item starts
    Then the observer is killed and removed
    And no workbench is made
    And HQ is told the turn never ran, because the observer did not become ready

  Scenario: The item's next turn finds the same observer
    Given the runner watches project 57
    And a turn on the item has run
    When another turn on the item starts
    Then the same observer is used, with its history
    And no second observer is made

  Scenario: An observer that is gone is never replaced by an empty one
    Given the runner watches project 57
    And a turn on the item has run
    And the item's observer has died
    When another turn on the item starts
    Then no new observer and no workbench is made
    And HQ is told the turn never ran, because the evidence is unavailable
    When a third turn on the item starts
    Then it is refused the same way

  Scenario: A let-go item's observer is kept half an hour, then removed
    Given the runner watches project 57
    And a turn on the item has run
    When HQ says the item is closed
    Then the item's sandbox is let go and the observer stays
    And the observer is still there 29 minutes later
    When 30 minutes have passed since the item was let go
    Then the observer is removed

  Scenario: A turn with no shell has no observer
    Given the runner watches project 57
    When a turn on the item starts with no shell
    Then no observer is made

  Scenario: An observer an earlier runner left is taken back
    Given the runner watches project 57
    And an earlier runner left the item's observer running
    When the runner starts and another turn on the item starts
    Then the same observer is used
    And no second observer is made
