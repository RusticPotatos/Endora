Feature: Reviews are capped at Haiku
  In round 2 of the baseline experiment, Sonnet's and Opus's reviews were $1.90 of the first
  $2.97, and Kit spent $3.00 of a day's pool by 08:15 (D7). The owner, 31 Oct: the member one
  rank up reviews, but never above Haiku unless the work came from Haiku or above; HQ's
  mechanical checks carry the rest. A member never reviews its own work (ADR 0142, amended
  2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"

  Scenario: Work done below Haiku is reviewed one rank up, which is within the cap
    Given the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Bo submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"

  Scenario: Work done at Haiku's rank is reviewed one rank up, as before
    Given the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada submits "Fix the lint" for review
    Then Kit is woken to review "Fix the lint"

  # Seed: the member one rank up was always reviewer, Sonnet included, whatever did the work.
  Scenario: With nobody on Haiku, work done below it is not reviewed by Sonnet
    Given Ada's model is "deepseek"
    And the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has the item "Fix the lint"
    When the loop hands out its items
    And Ada submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"
    And Kit is not woken to review "Fix the lint"
