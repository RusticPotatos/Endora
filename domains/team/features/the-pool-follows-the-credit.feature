Feature: The pool follows the Claude credit
  The owner's plan grants Claude API credit each month and takes back what is left of it on
  the day the plan renews: use it or lose it (the owner, 2026-10-31). So each day's pool is
  what the grant has left, spread over the days until it renews, counting today. It is never
  less than the floor, $95 a month over 30 days, nor more than three times the floor, and
  credit the owner bought, which does not expire, never raises it (ADR 0113, amended
  2026-10-31). Shares, caps and the owner's top-ups work on it as before.

  Background:
    Given a pool of $95 a month
    And Ada is the lead
    And Ada thinks with Claude
    And the goal "Keep the home lab healthy"
    And it is 31 October 2026

  Scenario: Credit that would expire raises today's pool
    When the node says the Claude grant has $70.23 left and renews on 2026-11-18
    Then today's pool is $3.90
    And the pool says "Sized from the Claude credit: $70.23 left, 18 days to the 18th."

  Scenario: Today's pool stays as it was sized while the team spends
    Given the node says the Claude grant has $70.23 left and renews on 2026-11-18
    And Ada has spent $1.00 of her share today
    When the node says the Claude grant has $69.23 left and renews on 2026-11-18
    Then today's pool is $3.90

  Scenario: The next day the pool is sized again
    When the node says the Claude grant has $70.23 left and renews on 2026-11-18
    And a day passes
    Then today's pool is $3.17
    When the node says the Claude grant has $66.33 left and renews on 2026-11-18
    Then today's pool is $3.90
    And the pool says "Sized from the Claude credit: $66.33 left, 17 days to the 18th."

  Scenario: A top-up still adds on top
    When the node says the Claude grant has $70.23 left and renews on 2026-11-18
    And the owner tops up the pool by $1.00
    Then today's pool is $4.90

  Scenario: A day's top-ups stop at five times the floor's day
    When the owner tops up the pool by $10.00
    And the owner tops up the pool by $5.00
    Then a top-up of $1.00 is refused: "$0.83 may still be added today"
    And today's pool is $18.17

  Scenario: Without word of the credit, or without a grant, the floor
    Then today's pool is $3.17
    And the pool says "Sized from the floor, $95.00 a month over 30 days: the Claude credit has not been heard today."
    When the node says no plan grant is set
    Then today's pool is $3.17
    And the pool says "Sized from the floor, $95.00 a month over 30 days: no plan grant of Claude credit is set."

  Scenario: Bought credit never raises the pool
    When the node says the Claude grant has $0.00 left and renews on 2026-11-18
    Then today's pool is $3.17

  Scenario: On the last day the pool is held at three times the floor
    Given it is 17 November 2026
    When the node says the Claude grant has $60.00 left and renews on 2026-11-18
    Then today's pool is $9.50
    And the pool says "Held at the ceiling, three times the floor: the Claude credit has $60.00 left, 1 day to the 18th."
