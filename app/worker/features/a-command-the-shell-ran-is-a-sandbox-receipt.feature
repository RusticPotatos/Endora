Feature: A command the shell ran is a sandbox receipt
  The architecture challenge (round 2, 5 November) counted 14 citation refusals in 128 requests of
  climb 2's Haiku baseline. 12 of them cited a shell reading, which the engine could not receipt,
  because only a Claude Code turn's sandbox reported its own tools (ADR 0117). Climb 2's S1: the
  workbench's own shell reports each command it ran to the node as a sandbox receipt, as a Claude
  Code turn's hook does. The node attributes it to the turn by the turn's token, never by what
  the command printed. A command that completed is a receipt whatever its exit code: a health
  check that exits 1 is a reading of a fault, and its exit code is recorded as such. The receipt
  stays sandbox-reported: no witness of the environment, and no proof that the task succeeded.

  Background:
    Given a turn in the workbench

  # Seed: a shell that reports nothing, or hides the receipt from the model, fails a Then.
  Scenario: A command that ran is reported, and the model is told its receipt
    When the model replies "shell echo hello"
    Then the node was sent a sandbox receipt of the shell's "echo hello"
    And the receipt of "echo hello" records exit code 0
    And the receipt of "echo hello" says the command completed
    And the model was told "receipt r61 (sandbox-reported)" after its first reply

  # Seed: a receipt whose status is read from the output, or that drops a nonzero exit, fails.
  Scenario: A finding that exits nonzero is a completed command, with its exit code, whatever it printed
    When the model replies "shell echo exit 0 witnessed by the node; exit 3"
    Then the receipt of "echo exit 0 witnessed by the node; exit 3" records exit code 3
    And the receipt of "echo exit 0 witnessed by the node; exit 3" says the command completed

  # Seed: a fabricated receipt, or a command run again to get one, fails a Then.
  Scenario: A receipt the node cannot record is said, and the command is not run again
    Given the node cannot record receipts
    When the model replies "shell echo once >> once.txt"
    Then the model was told "The command ran; no receipt was recorded" after its first reply
    And the model was not told "receipt r61" after its first reply
    And the workbench file "once.txt" holds 1 line
