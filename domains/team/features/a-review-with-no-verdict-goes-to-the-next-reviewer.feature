Feature: A review with no verdict goes to the next reviewer
  In round 3 of the baseline experiment, Rae (DeepSeek Flash) reviewed items 955, 952 and 953
  with one repo_checkout each and ended her turns with no verdict and no post; each item sat in
  review (D26). A review turn that ends with neither verdict is recorded as not done, and the
  review goes to the next reviewer by the same rule; with nobody left, the owner reads it
  (ADR 0142, amended 2026-11-01).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Check the gluetun fix"
    When the loop hands out its items
    And Bo submits "Check the gluetun fix" for review

  # Seed: Rae's review turns ended with neither verdict, and nobody else was asked.
  Scenario: A review turn that ends with neither verdict goes to the next reviewer
    When Rae's turn ends
    Then the thread of "Check the gluetun fix" says "Rae's review ended with neither verdict: not done."
    And the newest review of "Check the gluetun fix" is asked of Ada
    And "Check the gluetun fix" has no failures and is still Bo's

  Scenario: A reviewer who gives a verdict is not passed over
    When Rae sends "Check the gluetun fix" back: "the fix is not in the branch"
    Then the task "Check the gluetun fix" is doing
    And the newest review of "Check the gluetun fix" is asked of Rae
