Feature: A consultation answer and its status survive together
  An unavailable store must not leave a partial answer in a task thread.
  A retry after storage recovers keeps one answer, including after HQ restarts.

  Background:
    Given HQ keeps its team in a file
    And Member1 is the lead
    And Member2 is the developer
    And the goal "Synthetic investigation"
    And Member1 has the task "Check fixture"

  Scenario: A failed completion leaves no answer and a retry keeps one
    When Member1's turn starts
    And Member1 consults Member2 about "Check fixture": "Which fixture failed?"
    And Member2's turn starts
    And the consultation status store fails
    And Member2 tries to answer the consult: "The fixture cache failed."
    Then it is refused for "synthetic completion failure"
    And the task "Check fixture" has 0 consultation answers
    When consultation storage recovers
    And Member2 answers the consult: "The fixture cache failed."
    Then the task "Check fixture" has 1 consultation answers
    When HQ restarts
    Then the task "Check fixture" has 1 consultation answers
