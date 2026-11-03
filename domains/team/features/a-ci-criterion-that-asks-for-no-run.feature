Feature: A CI criterion that asks for no run
  In round 3 of the baseline experiment, item 955's criterion 2 was "It reports today's
  offenders without failing CI on them". HQ's CI check said no workflow runs on a push of the
  branch, so the criterion "cannot be met as the fork stands", and HQ would not pass it (D25).
  The criterion asks what CI does with the rule's findings, which running CI's own steps on
  the head shows; the judge did, and found the failures develop has and the new findings as
  warnings. HQ's refusal is for a criterion that asks for a CI run's result ("A CI run on the
  branch passes"); for any other criterion about CI, HQ's checks say what the check of the head,
  which runs the lint steps CI runs, gave (ADR 0142, amended 2026-11-01).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Report the secret defaults", with the criteria
      | It reports today's offenders without failing CI on them |

  # Seed: 955's check said "cannot be met as the fork stands", and HQ would not pass it.
  Scenario: A criterion about what CI does with the work is measured on the head, not refused
    When the loop hands out its items
    And Bo submits the branch "hq/task-1-secret-defaults", whose workflow "ci.yml" runs on a push to "main, develop"
    And the check passes: "syntax ok, ansible-lint 14 findings (develop 14), yamllint ok: no more findings than develop: met"
    And Bo asks for review of "Report the secret defaults"
    Then HQ's checks on "Report the secret defaults" say "asks what CI's steps make of the work, not for a run's result"
    And HQ's checks on "Report the secret defaults" say "check c1 of hq/task-1-secret-defaults passed: syntax ok, ansible-lint 14 findings (develop 14)"
    When Rae passes "Report the secret defaults" on a reading of her own
    Then it is not refused
    And the task "Report the secret defaults" is done
