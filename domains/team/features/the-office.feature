Feature: The open office
  The teams should look, work and interact as one (ADR 0119). The office is an
  open floor with a pod of desks for each team, and what stands on it is data
  the team may change: its plants, its lamps, its couches. The team decides
  together what to change, in its downtime, which costs nothing unless the owner
  says so. The owner is the landlord: they can undo, reset or veto. Nothing may
  ever block a desk or a door.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Bo is the developer
    And Cy is the researcher
    And the office has its default layout

  Scenario: The team decorates its own office, together
    When Bo proposes to add a "fish tank" at 10, 3 because "the kitchen needs life"
    Then the activity feed says "Bo proposes: add a fish tank in the commons"
    And the office has no "fish tank"
    And the proposal is not a task on the board or a card on the owner's desk
    When Cy votes yes on the proposal
    Then the office has a "fish tank" at 10, 3
    And the activity feed says "The office changed: add a fish tank in the commons"
    When the owner undoes the last change
    Then the office has no "fish tank"

  Scenario: A proposal nobody objects to carries after six quiet hours, and one the team votes down does not
    When Bo proposes to add a "lamp" at 14, 5
    And 7 hours pass
    Then the office has a "lamp" at 14, 5
    When Cy proposes to add a "fish tank" at 10, 3
    And Ada votes no on the proposal
    And Bo votes no on the proposal
    Then the office has no "fish tank"
    And the activity feed says "more said no than yes"

  Scenario: The owner vetoes a proposal and resets the office
    When Bo proposes to add a "fish tank" at 10, 3
    And the owner vetoes the proposal
    And 7 hours pass
    Then the office has no "fish tank"
    When Bo proposes to add a "fish tank" at 10, 3
    And Cy votes yes on the proposal
    And the owner resets the office
    Then the office has no "fish tank"
    And the office has the default layout

  Scenario: Downtime costs nothing unless the owner says so
    Given downtime is on
    And Bo thinks with Claude
    And Cy thinks with Claude
    When a tick passes
    Then Ada has a downtime turn that costs nothing
    And Bo has no downtime turn
    And Cy has no downtime turn
    When Ada tries to create a task in her downtime turn
    Then it is refused for "downtime"
    When the owner allows 2 cents a day for paid members' downtime
    And Ada's turn ends
    And a tick passes
    Then Bo has a downtime turn charged to the pool, not to Bo's share

  Scenario: Nothing may block a desk or a door
    When Bo proposes to add a "plant" at 30, 11
    Then it is refused for "door"
    When Bo proposes to add a "plant" at 3, 9
    Then it is refused for "desks"
    When Bo proposes to add a "plant" at 12, 7
    Then it is refused for "walking lane"
    When Bo proposes to add a "plant" at 22, 3
    Then it is refused for "room"
    And the activity feed does not say "Bo proposes: add a plant"
