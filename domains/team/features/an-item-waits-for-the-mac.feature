Feature: An item waits for the Mac rather than blocking
  The review of round 2's code (31 Oct) found that an item failing at Bo's rank, with only Rae
  above him and the Mac away, was blocked for good: "failed at every rank". The Mac is the
  owner's laptop, away for an evening at a time, so the item waits at Bo's rank instead, with
  nobody woken on it, and goes up to Rae once the Mac is free again (ADR 0142, amended
  2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"

  # Seed: with only the away Mac above, the item was blocked and put on the owner's desk.
  Scenario: An item that fails while only the away Mac is above waits for it
    Given the loop "ansible-nas" has the item "Find what stops gluetun"
    When the owner pauses Mara
    And the owner pauses Kit
    And the owner pauses Ada
    And the node says the Mac is not ready: "in use"
    And the loop hands out its items
    And 12 hours pass, with HQ ticking every few minutes
    Then "Find what stops gluetun" waits at Bo's rank, not blocked, with nothing on the owner's desk
    When the node says the Mac is ready
    And the loop hands out its items
    Then "Find what stops gluetun" has no failures and is still Rae's
