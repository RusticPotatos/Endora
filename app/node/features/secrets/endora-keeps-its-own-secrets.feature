Feature: Endora keeps its own secrets
  OpenBao is the owner's own home service, not Endora's (ADR 0144). Endora keeps and owns its
  secrets in its built-in encrypted store (ADR 0125): it starts with no vault address and no
  vault network, seals what the owner gives it under its own key file, and asks no other
  service to open them. A database an older Endora used with the vault is left as it was.

  Scenario: The node starts and keeps a secret with no vault
    Given a new node with no vault address and no vault network
    When the node starts
    And the owner saves the token "placeholder-token-0144" for "home-assistant"
    Then the node reads the token "placeholder-token-0144" back for "home-assistant"
    And "placeholder-token-0144" is nowhere in the database in the clear
    And every data key is wrapped by the node's own key file

  Scenario: An older node's vault tables are left as they were
    Given a node whose database holds an older Endora's vault tables, with one passkey
    When the node starts
    Then the vault tables are still there, with one passkey

  Scenario: Keys an older Endora moved into OpenBao are refused, not lost
    Given a node whose data keys an older Endora moved into OpenBao
    When the node starts
    Then it refuses to start, saying "OpenBao"
    And the data keys are still there as they were
