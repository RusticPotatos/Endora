Feature: Reviews are capped at Haiku
  In round 2 of the baseline experiment, Sonnet's and Opus's reviews were $1.90 of the first
  $2.97, and Kit spent $3.00 of a day's pool by 08:15 (D7). The owner, 31 Oct: no review above
  Haiku's rank. Work done below it is reviewed one rank up, within the cap; work done at it or
  above is reviewed by another member at Haiku's rank, else the nearest below, else the nearest
  above. Harder work reaches stronger models by its climb, not by review; HQ's mechanical checks
  carry the rest. A member never reviews its own work, nor work she had a hand in: an item
  whose branches carry her commits or on which she took turns of work (ADR 0142, amended
  2026-11-01, D24).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"

  Scenario: Work done below Haiku is reviewed one rank up, which is within the cap
    Given the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Bo submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"

  Scenario: Work done at Haiku's rank is reviewed by another member at Haiku's rank
    Given Cy is hired as a developer on the house's model
    And Cy's model is "haiku"
    And the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada submits "Fix the lint" for review
    Then Cy is woken to review "Fix the lint"
    And Kit is not woken to review "Fix the lint"

  Scenario: Work done at Sonnet's rank is reviewed at Haiku's rank
    Given the loop "ansible-nas" starts at Kit's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Kit submits "Fix the lint" for review
    Then Ada is woken to review "Fix the lint"
    And Mara is not woken to review "Fix the lint"
    And the thread of "Fix the lint" says "Ada reviews this."

  Scenario: With nobody else on Haiku, work done at it is reviewed below it, not by Sonnet
    Given the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"
    And Kit is not woken to review "Fix the lint"

  # Seed: the member one rank up was always reviewer, Sonnet included, whatever did the work.
  Scenario: With nobody on Haiku, work done below it is not reviewed by Sonnet
    Given Ada's model is "deepseek"
    And the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"
    And Kit is not woken to review "Fix the lint"

  # Seed (round 3, 952, D24): Ada's turns kept work in progress on it, it climbed to Kit, who
  # squashed her commits into his, and Ada then reviewed it and closed it. HQ asked only who
  # submitted the head.
  Scenario: A member who had a hand in the work does not review it
    Given the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada's turn on "Fix the lint" runs out of room, keeping its work on "fix-the-lint"
    And Rae sends "Fix the lint" back 3 times: "the lint still fails"
    Then "Fix the lint" is Kit's
    When Ada's turn ends
    And Kit submits "Fix the lint" for review
    Then the newest review of "Fix the lint" is asked of Rae
    And Ada is not woken to review "Fix the lint"
