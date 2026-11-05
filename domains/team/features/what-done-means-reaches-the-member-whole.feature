Feature: What done means reaches the member whole
  Set 3 of the baseline experiment, item 1049: the owner's third criterion for simpleeval's set
  comprehensions was 582 characters and ended "plain SimpleEval still raises FeatureNotAvailable
  for a set comprehension". HQ kept a criterion's first 200 characters, so the member read it cut
  at "the loop variable i…[cut]", wrote no test for that last requirement, and the blind judge
  failed the work for it. 21 of the set's 49 criteria were longer than 200. A criterion is now
  kept whole up to 1,000 characters, and one longer is refused, saying which, never cut.

  # Seed: a criterion cut at 200 characters fails the Then.
  Scenario: A long criterion is read whole in the member's briefing
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "simpleeval"
    And the loop "simpleeval" starts at Bo's rank
    And the loop "simpleeval" has the item "Support set comprehensions", with the criteria
      | EvalWithCompoundTypes evaluates set comprehensions: `{x % 3 for x in [1, 2, 3, 4]}` -> {0, 1, 2} and `{(x, y) for x in [1, 2] for y in [1, 2] if x != y}` -> {(1, 2), (2, 1)}; the loop variable is not visible after evaluation; a set comprehension producing more than MAX_COMPREHENSION_LENGTH items raises IterableTooLong; plain SimpleEval still raises FeatureNotAvailable for a set comprehension. |
    When the loop hands out its items
    Then Bo's turn starts on "Support set comprehensions"
    And Bo's briefing says "plain SimpleEval still raises FeatureNotAvailable for a set comprehension."
