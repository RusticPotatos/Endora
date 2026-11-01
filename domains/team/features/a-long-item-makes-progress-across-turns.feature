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

  # Seed (round 3, 951, D23): submitted at a head whose check passed, then a turn ran out of
  # room and the worker kept the same head as work in progress. Keeping it shut the gate and
  # queued no check, so both of the reviewer's passes were refused, with no route to reopen it.
  Scenario: Work kept at the head its check passed leaves the gate as it was
    When Kit submits the branch "hq/task-1-secret-defaults"
    And the check passes: "syntax ok, ansible-lint 14 findings (develop 14), yamllint 6 errors (develop 6): no new findings against develop: met"
    And Kit's turn on "A lint rule for secret defaults" runs out of room, keeping the head it submitted on "secret-defaults"
    And Kit takes "A lint rule for secret defaults" and reads gluetun's logs: "healthy"
    And Kit asks for review of "A lint rule for secret defaults", citing his reading
    And Ada passes "A lint rule for secret defaults" on a reading of her own
    Then it is not refused
    And the task "A lint rule for secret defaults" is done

  # Seed (round 3, 950, D27): on the house model Bo met a fresh, empty sandbox with the fork's
  # tools held back beside weather and news tools, ran git init with a made-up remote, took the
  # failures for an offline sandbox, and blocked the item.
  Scenario: A turn on a loop's item has the fork's tools up front, and is told its sandbox starts empty
    When Kit's turn on "A lint rule for secret defaults" runs out of room, keeping its work on "secret-defaults"
    Then Kit's turn on "A lint rule for secret defaults" loads the fork's checkout and submit up front
    And what Kit reads of "A lint rule for secret defaults" says "Your sandbox starts empty each turn: check out the kept branch first."
