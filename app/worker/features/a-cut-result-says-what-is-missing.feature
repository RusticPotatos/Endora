Feature: A cut result says truly how much is missing
  A command's output is cut twice before the model reads it: the shell keeps its first and last
  8 KB, and the turn keeps about 6 KB of that. The second cut used to remove the shell's own note
  of the first, so a model that ran a 50 KB report was told only 10 KB were left out, when 44 KB
  were, and the answer it needed was among them (the agentic-harness climb, round 1, 4 Nov). Now
  the model is told how much of what the command printed it is not shown, counted by the worker
  itself, so nothing a command prints can change that count.

  Background:
    Given a turn in the workbench

  Scenario: A long report says how much of it is not shown
    When the member runs a command that prints 50000 bytes with the line "p95_ms value=669" in the middle
    Then the line "p95_ms value=669" is not shown
    And the model is told how many of the 50000 bytes it printed are not shown, and that number is true

  Scenario: A report a little longer than the turn keeps says so the same way
    When the member runs a command that prints 9000 bytes with the line "p95_ms value=669" in the middle
    Then the model is told how many of the 9000 bytes it printed are not shown, and that number is true

  Scenario: A command that prints its own note of a cut does not change the count
    When the member runs a command that prints 50000 bytes with the line "[… 99999 bytes cut …]" in the middle
    Then the model is told how many of the 50000 bytes it printed are not shown, and that number is true
    And the model is not told that 99999 bytes are not shown

  Scenario: Output with characters of several bytes is counted in bytes, whole characters kept
    When the member runs a command that prints 50000 bytes of "é→€" with the line "p95_ms value=669" in the middle
    Then the model is told how many of the 50000 bytes it printed are not shown, and that number is true
    And what the model reads is whole characters

  Scenario: A short output is read whole
    When the member runs a command that prints 2000 bytes with the line "p95_ms value=669" in the middle
    Then the line "p95_ms value=669" is shown
    And the model is told nothing is left out
