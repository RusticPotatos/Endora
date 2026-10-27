Feature: The open office
  The team is one team (ADR 0119, 0142). The office is an open floor drawn from
  its layout, where the members work and, in their downtime, play: downtime
  costs nothing, since it is work on the house's own model whatever a member
  judges with (ADR 0131). Chess is played by the rules.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Bo is the developer
    And Cy is the researcher
    And the office has its default layout

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
