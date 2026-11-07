Feature: Endora reaches the house's vault at the address it is given
  The vault moved out of Endora into a role of its own (ADR 0127, amended 2026-11-07), so
  Endora's node is one client of it, reaching it at the address the owner gives it
  (ENDORA_VAULT_URL). An address the node cannot use is refused when the node starts, saying
  why, rather than showing a vault that never answers. A credential never belongs in that
  address, and the refusal never repeats one. When the vault does not answer, the console is
  told which address the node tried.

  Scenario: The address is used as given
    Given the vault's address is "http://openbao:8200/"
    When the node reads its vault address
    Then the node reaches the vault at "http://openbao:8200"

  Scenario: No address, no vault
    Given the vault's address is ""
    When the node reads its vault address
    Then the node has no vault

  Scenario: An address without a scheme is refused
    Given the vault's address is "openbao:8200"
    When the node reads its vault address
    Then the node refuses it, saying "http:// or https://"

  Scenario: A credential in the address is refused and not repeated
    Given the vault's address is "http://root:hunter2@openbao:8200"
    When the node reads its vault address
    Then the node refuses it, saying "no credential"
    And the refusal does not contain "hunter2"

  Scenario: The console is told which address does not answer
    Given the vault's address is "http://openbao:8200"
    And nothing answers there
    When the console asks how the vault is
    Then it is told the vault is "unreachable" at "http://openbao:8200"

  Scenario: A vault set up outside Endora is not offered to the passkey
    Given the vault's address is "http://openbao:8200"
    And the vault there was initialised by the owner and is sealed
    When the console asks how the vault is
    Then it is told the vault is "locked" at "http://openbao:8200"
    And it is told Endora did not set it up
