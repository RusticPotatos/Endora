Feature: The team knows how the lab is built
  The CEO's house log is seeded once with how the lab is built, and only when
  it is empty (ADR 0112): a log with text in it is never overwritten, and one
  the owner clears stays clear.

  Scenario: Endora tells the team how the lab is built, once
    When HQ starts with an empty house log
    Then the house log says "The home lab is managed by ansible-nas"
    When the owner clears the house log and HQ starts again
    Then the house log stays empty
