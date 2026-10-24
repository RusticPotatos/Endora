Feature: Task work asks colleagues through one bounded assistance lifecycle
  A real task may ask a colleague in its authorized project conversation.
  The answer is retained for people watching, without approving the task.

  Scenario: A task retries its request without creating an older consultation
    Given the owner has assigned classified task work in an authorized conversation project
    When Member1 requests task help through the canonical protocol twice
    Then accepting the request has not started any work
    And task help has no legacy request or automatic task completion
    When the assistance delivery tick runs
    Then only Member2 receives one assistance turn
    And the assistance turn has no shell or node capabilities
    And the assistance turn cannot delegate work
    When Member2 answers the assistance request twice with the same answer
    Then the owner sees one answered request with the original author and project
    And task help has no legacy request or automatic task completion
    When the requester records its own inconclusive finding
    Then the owner sees the retained requester finding
    When the raw worker transcripts expire
    Then the owner sees the retained requester finding
    And task help has no legacy request or automatic task completion
    When the owner deletes the conversation and its retained finding
    Then the conversation history is unavailable
