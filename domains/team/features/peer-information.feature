Feature: Peer information is retained without scheduling work
  The owner authorizes two members to exchange information. Sending a message
  is neither a task nor a wake, and history stays until the owner deletes it.

  Background:
    Given the owner authorizes two members to exchange information

  Scenario: The ordinary consult tool reaches a colleague without a task
    When Member1 asks Member2 with the ordinary consult tool
    Then accepting the request has not started any work
    When the assistance delivery tick runs
    Then only Member2 receives one assistance turn
    And the assistance turn cannot delegate work
    When Member2 answers the assistance request twice with the same answer
    Then the owner sees one answered request with the original author and project

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

  Scenario: An explicit request starts one expert and keeps one attributed answer
    When Member1 explicitly asks Member2 for read-only assistance
    Then accepting the request has not started any work
    When the assistance delivery tick runs
    Then only Member2 receives one assistance turn
    And the assistance turn cannot delegate work
    When Member2 answers the assistance request twice with the same answer
    Then the owner sees one answered request with the original author and project

  Scenario: Disabling assistance cancels work and re-enabling does not resume it
    When Member1 explicitly asks Member2 for read-only assistance
    And the assistance delivery tick runs
    And the owner disables assistance
    Then the request is visibly cancelled
    When the owner re-enables assistance
    Then cancelled assistance starts no further turns

  Scenario: Losing a worker after its accepted answer does not repeat the work
    When Member1 explicitly asks Member2 for read-only assistance
    And the assistance delivery tick runs
    And Member2 answers the assistance request twice with the same answer
    And Member2's worker disappears after answering
    And 2 minutes pass
    Then only Member2 receives one assistance turn
    And the owner sees one answered request with the original author and project

  Scenario: An unavailable worker ends with a cause after bounded attempts
    When Member1 explicitly asks Member2 for read-only assistance
    And Member2's worker is unavailable
    And three bounded delivery attempts have passed
    Then the owner sees a failed assistance request with its cause
    When 2 minutes pass
    Then the owner sees a failed assistance request with its cause
