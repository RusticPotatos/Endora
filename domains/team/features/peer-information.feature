Feature: Peer information is retained without scheduling work
  The owner authorizes two members to exchange information. Sending a message
  is neither a task nor a wake, and history stays until the owner deletes it.

  Background:
    Given the owner authorizes two members to exchange information

  Scenario: A retry preserves one attributed message and schedules nothing
    When Member1 sends the information "The fixture check returned 503." with a stable retry key
    And Member1 sends the information "The fixture check returned 503." with a stable retry key
    Then the conversation has exactly one attributed message
    And sending information created no task wake or run

  Scenario: Retained information does not expire with model transcripts
    When Member1 sends the information "The fixture check returned 503." with a stable retry key
    And 31 days pass
    Then the retained information still says "The fixture check returned 503."

  Scenario: Owner deletion immediately removes access
    When Member1 sends the information "The fixture check returned 503." with a stable retry key
    And the owner deletes the information conversation
    Then the deleted information conversation cannot be read
