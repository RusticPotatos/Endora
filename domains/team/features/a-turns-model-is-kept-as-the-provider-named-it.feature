Feature: A turn's model is kept as the provider named it
  HQ kept only the model a turn asked for, by its alias, so the record could not show which
  model actually answered: the agentic-harness climb could not tell, on 4 Nov, whether Endora
  and Claude Code had run on the same model. The node now tells HQ, with each call's charge, the
  provider's own name for the model that answered, and HQ keeps every name it is told beside
  the turn, with how many calls each answered. A call whose answer named no model is kept as
  not known; the alias asked for never stands in for it.

  Background:
    Given Sol is the developer
    And Sol thinks with Claude Haiku
    And the goal "Ship the watch tracker"
    And Sol has the task "Fix the failing test"
    And Sol's turn starts

  Scenario: The model the provider named is kept beside the alias asked for
    When the node says a call of Sol's turn was answered by "claude-haiku-5-5-20261001"
    And the node says a call of Sol's turn was answered by "claude-haiku-5-5-20261001"
    Then Sol's turn asked for "claude-haiku-5-5"
    And Sol's turn was answered by "claude-haiku-5-5-20261001" in 2 calls
    And Sol's turn has no call whose model is not known

  Scenario: Every model that answered is kept, none overwritten
    When the node says a call of Sol's turn was answered by "claude-haiku-5-5-20261001"
    And the node says a call of Sol's turn was answered by "claude-haiku-5-5-20261115"
    Then Sol's turn was answered by "claude-haiku-5-5-20261001" in 1 call
    And Sol's turn was answered by "claude-haiku-5-5-20261115" in 1 call

  Scenario: A call whose answer named no model is kept as not known
    When the node says a call of Sol's turn was answered without naming its model
    Then Sol's turn has 1 call whose model is not known
    And Sol's turn was answered by no named model
