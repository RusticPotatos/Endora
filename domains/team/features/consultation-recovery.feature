Feature: Retiring old peer execution preserves readable history
  Pending legacy questions end explicitly without invented answers or new work.
  Repeating startup cleanup after HQ restarts preserves their original history.

  Background:
    Given HQ keeps its team in a file
    And Member1 is the lead
    And Member2 is the developer
    And the goal "Synthetic investigation"
    And Member1 has the task "Check fixture"

  Scenario: A pending old question is cancelled and still readable after restart
    When Member1's turn starts
    And a retained pending legacy question from Member1 to Member2 on "Check fixture" exists
    And HQ performs protocol v2 legacy cleanup
    Then the legacy question on "Check fixture" is cancelled and readable
    And the task "Check fixture" has 0 consultation answers
    And nobody was woken by the question
    When HQ restarts
    And HQ performs protocol v2 legacy cleanup
    Then the legacy question on "Check fixture" is cancelled and readable
    And the task "Check fixture" has 0 consultation answers
    And nobody was woken by the question
