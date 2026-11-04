Feature: The relay keeps its own count of model requests
  The agentic-harness climb compares Endora with Claude Code, and the comparison has to prove
  that every model request a turn made is accounted for. HQ's only record of a call was the
  usage report the node sends it, so a report that was lost would hide that the call happened.
  The node now keeps its own count of each model request it passes on: when it began and
  ended, which model was asked for, which model the provider said answered, how it ended, and
  whether HQ was told. It keeps nothing of what was said: no prompt, no reply, no key.

  Scenario: An answered call is counted with the model that answered
    Given the provider answers as "claude-sonnet-5-5-20261001"
    When a member's turn makes one model call
    Then the node counts 1 model request, which was answered
    And the node says "claude-sonnet-5-5-20261001" answered it
    And the node says HQ was told

  Scenario: A call HQ never heard about is still counted
    Given the provider answers as "claude-sonnet-5-5-20261001"
    And HQ is not taking usage reports
    When a member's turn makes one model call
    Then the node counts 1 model request, which was answered
    And the node says "claude-sonnet-5-5-20261001" answered it
    And the node says HQ was not told

  Scenario: A call the provider could not answer is counted as failed, with no model
    Given the provider is overloaded
    When a member's turn makes one model call
    Then the node counts 1 model request, which failed
    And the node names no model for it

  Scenario: A stream that was cut is counted as interrupted, not answered
    Given the provider cuts its streams short
    When a member's turn makes one streamed model call
    Then the node counts 1 model request, which was interrupted
    And the node does not count it as answered
