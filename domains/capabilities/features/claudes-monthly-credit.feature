Feature: Claude's monthly credit
  Anthropic does not say its balance, so Endora's is the figure the owner last read from
  Anthropic's Console, less what Endora has paid since (the owner, 2026-10-30). A plan such as
  Claude Max grants API credit each month, which expires when the plan renews and is spent
  before the credit the owner bought. On the renewal day, at the owner's midnight, the grant is
  whole again and what is left of the bought credit carries over; a plan that renews on a day a
  month does not have renews on that month's last day.

  Background:
    Given the owner's clock is 4 hours behind UTC
    And Claude's plan grants $100 a month, renewing on day 15
    And on 2026-10-01 the owner read $130 on Anthropic's Console, $30 of it bought

  Scenario: What Endora pays comes off the month's grant first, then off the bought credit
    When Endora pays Claude $40
    Then $60 of the grant is left, and $30 of the bought credit
    When Endora pays Claude $70
    Then $0 of the grant is left, and $20 of the bought credit

  Scenario: The grant is whole again on its day, and the bought credit carries over
    Given Endora pays Claude $110
    When it is 2026-10-14 at 23:59
    Then the plan has not renewed
    And $0 of the grant is left, and $20 of the bought credit
    When it is 2026-10-15 at 00:01
    Then the plan has renewed
    And $100 of the grant is left, and $20 of the bought credit

  Scenario: A plan renewing on the 31st renews on a short month's last day
    Given Claude's plan grants $100 a month, renewing on day 31
    Then after 2026-11-01 the plan next renews on 2026-11-30
    And after 2027-02-01 the plan next renews on 2027-02-28
