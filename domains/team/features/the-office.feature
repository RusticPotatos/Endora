Feature: The open office
  The teams should look, work and interact as one (ADR 0119). The office is an
  open floor with a pod of desks for each team, and what stands on it is data
  the team may change: its plants, its lamps, its couches. The team decides
  together what to change, in its downtime, which costs nothing: it is work on
  the house's own model, whatever a member judges with (ADR 0131). The owner is
  the landlord: they can undo, reset or veto. Nothing may ever block a desk or a
  door.

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

  Scenario: Downtime costs nothing, whatever a member judges with
    Given downtime is on
    When a tick passes
    Then Ada has a downtime turn that costs nothing
    And Bo has no downtime turn
    And Cy has no downtime turn
    When Ada tries to create a task in her downtime turn
    Then it is refused for "downtime"
    And Ada's turn runs on "local"
    When Ada's turn ends
    And a tick passes
    Then Bo has a downtime turn that costs nothing
    When Bo's turn starts
    Then Bo's turn runs on "local"
    And the pool has paid nothing

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

  Scenario: Ping-pong keeps score, and costs nothing
    Given the games are on
    When 25 minutes pass
    And 25 minutes pass
    Then ping-pong has been played
    And every ping-pong match was first to 11, two clear
    And the activity feed says " at ping-pong"
    And the ping-pong leaderboard names the players
    And no match woke anyone or ran a turn

  Scenario: Chess is played by the rules
    Given downtime is on
    When a tick passes
    And Ada challenges Bo to chess
    Then the activity feed says "Ada challenges Bo to chess"
    When Ada plays "e5" at chess
    Then it is refused for "legal moves are"
    When Ada plays "e4" at chess
    Then the chess moves are "e4"
    When Ada plays "d4" at chess
    Then it is refused for "not your move"
    When Ada's turn ends
    And a tick passes
    And Bo plays "e5" at chess
    Then the chess moves are "e4 e5"
    And the activity feed says "Bo played e5 against Ada at chess"

  Scenario: Chess ends when the rules say so
    Given downtime is on
    And Ada and Bo are in a game of chess after "e4 e5 Bc4 Nc6 Qh5 Nf6"
    When a tick passes
    And Ada plays "Qxf7" at chess
    Then the activity feed says "Ada beat Bo at chess by checkmate"
    And the chess game is over
    When Ada plays "a3" at chess
    Then it is refused for "no game of chess"
