Feature: The open office
  The team is one team (ADR 0119, 0142). The office is an open floor drawn from
  its layout, where the members work and, in their downtime, play: downtime on
  the house's own model costs nothing. Chess is played by the rules.

  Background:
    Given Ada is the lead
    And Bo is the developer
    And Cy is the researcher
    And the office has its default layout

  Scenario: Downtime on the house's model costs nothing
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

  # Seed (round 3, 1 Nov): downtime and chess turns kept running beside the round's items, on the
  # house's model and in the runner's places. The owner: cut the time-wasters while a round runs.
  Scenario: Downtime waits while a round of the experiment is under way
    Given downtime is on
    And a round of the experiment is under way, with Cy working its item "Pin the images"
    When a tick passes
    Then Ada has no downtime turn
    And Bo has no downtime turn
    When the round's item "Pin the images" is dropped
    And a tick passes
    Then Ada has a downtime turn that costs nothing

  Scenario: Chess is played by the rules
    Given downtime is on
    When a tick passes
    And Ada challenges Bo to chess
    Then the activity feed says "Ada challenges Bo to chess"
    When Ada plays "e5" at chess
    Then it is refused for "is not legal here"
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

  # Office chess turns averaged 27,760 prompt tokens in the week to 30 October, the largest
  # cost line, for a move that needs only the board.
  Scenario: A chess move is offered its turn and the board alone
    Given downtime is on
    When a tick passes
    And Ada's turn starts
    Then Ada's turn is offered "chess, my_turn" alone
    And Ada's turn is offered none of the node's tools
    And Ada's briefing does not say "Your map of the team's world"

  Scenario: A game of chess is written out in PGN
    Given downtime is on
    And Ada and Bo are in a game of chess after "e4 e5 Bc4 Nc6 Qh5 Nf6"
    When a tick passes
    And Ada plays "Qxf7" at chess
    Then the chess game written out in PGN reads
      """
      [Event "Endora office chess"]
      [Site "Endora HQ"]
      [Date "2026.10.17"]
      [Round "-"]
      [White "Ada"]
      [Black "Bo"]
      [Result "1-0"]
      [Termination "normal"]

      1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# {checkmate} 1-0
      """
