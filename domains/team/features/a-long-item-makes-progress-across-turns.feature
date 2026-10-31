Feature: A long item makes progress across turns
  Round 2 of the baseline experiment (31 Oct) gave the ansible-nas loop a lint rule with tests
  (item 937). Its turns ran out of room more than twelve times across Rae, Ada and Kit. Each
  turn read the item's thread again, by then dozens of handovers long, and what the turn had
  written in its sandbox was gone when it ended, so the next started the rule again. Every
  fourth turn out of room counted as a failure, so the item climbed three ranks with no check
  ever failed: $2.14 spent and no branch kept (D12). A turn now reads the newest handover, not
  every one; the work a turn leaves is kept on a branch the next turn checks out; and a turn
  out of room that kept new work is no failure (ADR 0142, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "A lint rule for secret defaults"
    When the loop hands out its items

  # Seed: every handover and every turn's last word stayed in what a turn read.
  Scenario: A turn reads the newest handover, not every one
    When Kit's turns on "A lint rule for secret defaults" run out of room 6 times, keeping nothing
    Then what Kit reads of "A lint rule for secret defaults" holds 1 handover
    And what Kit reads of "A lint rule for secret defaults" says older handovers were left out
    And the thread of "A lint rule for secret defaults" still holds every handover

  # Seed: the sandbox's work was gone at the end of every turn.
  Scenario: The work a turn leaves is kept for the next
    When Kit's turn on "A lint rule for secret defaults" runs out of room, keeping its work on "secret-defaults"
    Then no check is queued for "A lint rule for secret defaults"
    And what Kit reads of "A lint rule for secret defaults" says to check out its kept branch "secret-defaults"

  # Seed: every fourth turn out of room was a failure, however much it had done.
  Scenario: Turns out of room that keep new work are no failure
    When Kit's turns on "A lint rule for secret defaults" run out of room 6 times, each keeping new work
    Then "A lint rule for secret defaults" has no failures and is still Kit's

  Scenario: Turns out of room that keep nothing still count
    When Kit's turns on "A lint rule for secret defaults" run out of room 4 times, keeping nothing
    Then "A lint rule for secret defaults" has 1 failure and is still Kit's
