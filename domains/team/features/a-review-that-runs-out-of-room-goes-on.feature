Feature: A review that runs out of room goes on
  The review of round 2's code (31 Oct) found that only the member holding an item went on in a
  fresh turn when its turn reached its prompt's budget; a reviewer's turn that did was never
  continued, and the review stalled. The reviewer now goes on in a fresh review turn, up to three
  times, as the holder does; past that the owner is asked about the item, which is no failure
  of the worker's (ADR 0142, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"

  # Seed: Rae's review ended at its budget and nobody was woken on the item again.
  Scenario: A reviewer whose turn runs out of room reviews afresh
    Given the loop "ansible-nas" has the item "Check the gluetun fix"
    When the loop hands out its items
    And Bo submits "Check the gluetun fix" for review
    And Rae's turn on "Check the gluetun fix" ends at its prompt's budget
    Then Rae has been woken on "Check the gluetun fix" 2 times
    And "Check the gluetun fix" has no failures and is still Bo's

  Scenario: Past three fresh reviews, the owner is asked
    Given the loop "ansible-nas" has the item "Check the gluetun fix"
    When the loop hands out its items
    And Bo submits "Check the gluetun fix" for review
    And Rae's turn on "Check the gluetun fix" ends at its prompt's budget 4 times in a row
    Then Rae has been woken on "Check the gluetun fix" 4 times
    And the owner's desk holds one question about "Check the gluetun fix"
    And "Check the gluetun fix" has no failures and is still Bo's
