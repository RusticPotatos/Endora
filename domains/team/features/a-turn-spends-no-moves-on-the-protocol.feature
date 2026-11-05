Feature: A turn spends no moves on the protocol
  The house-model replay of 1 Nov and round 3's store (D30): turns opened by calling my_turn,
  which repeats the briefing their first message already holds, and claim, for an item that was
  already theirs. The owner, 1 Nov: cut the time-wasters, "let the agents be agents". A turn on
  the item its member holds is offered neither; HQ still reads the briefing and claims for the
  worker before the first round, as before (ADR 0142, amended 2026-11-01).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "A lint rule for secret defaults"
    When the loop hands out its items

  # Seed (round 3, D30): turns spent their first moves on my_turn and claim.
  Scenario: A turn on its own item is offered no protocol moves
    Then Kit's turn on "A lint rule for secret defaults" is offered neither my_turn nor claim
    And what Kit reads of "A lint rule for secret defaults" says "A lint rule for secret defaults"
