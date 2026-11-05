Feature: A checkout outside the lab carries no rule of the lab's
  The defect-remediation climb (O8, 5 November) found that a checkout of a public repository
  outside the lab told the member "Never edit, cite or copy from group_vars/*.yml, host_vars or any
  inventory", the lab fork's rule, though since ADR 0115's amendment of 5 November that rule holds
  in the fork alone and the node takes an outside repository's own inventory code. The node now
  says, from the repository the owner set on the item's project, when a checkout is of one outside
  the lab; the worker then tells the model what holds there instead, and the fork's checkout keeps
  the fork's rule.

  Background:
    Given a turn in the workbench

  # Seed: a checkout text that ignores which repository it is fails a Then.
  Scenario: A developer's checkout outside the lab
    Given the node hands out "main" of a repository outside the lab
    When the model replies "repo_checkout main"
    Then the model was told "Checked out main" after its first reply
    And the model was told "outside the lab" after its first reply
    And the model was told "Never put the owner's own values" after its first reply
    And the model was not told "any inventory" after its first reply

  Scenario: The team's kept branch outside the lab
    Given the node hands out the team's kept branch "hq/task-7-fix" of a repository outside the lab
    When the model replies "repo_checkout hq/task-7-fix"
    Then the model was told "Checked out the team's kept branch hq/task-7-fix" after its first reply
    And the model was told "outside the lab" after its first reply
    And the model was not told "any inventory" after its first reply

  Scenario: A reviewer's checkout outside the lab
    Given the node hands out the team's kept branch "hq/task-7-fix" of a repository outside the lab
    And the turn may only read
    When the model replies "repo_checkout hq/task-7-fix"
    Then the model was told "You read; you submit and push nothing" after its first reply
    And the model was told "outside the lab" after its first reply
    And the model was not told "any inventory" after its first reply

  # Seed: a rule dropped for every checkout fails the Thens.
  Scenario: A developer's checkout of the lab's fork keeps the fork's rule
    Given the node hands out "develop" of the lab's fork
    When the model replies "repo_checkout develop"
    Then the model was told "Checked out develop" after its first reply
    And the model was told "any inventory" after its first reply
    And the model was not told "outside the lab" after its first reply

  Scenario: A reviewer's checkout of the lab's fork keeps the fork's rule
    Given the node hands out the team's kept branch "hq/task-7-fix" of the lab's fork
    And the turn may only read
    When the model replies "repo_checkout hq/task-7-fix"
    Then the model was told "any inventory" after its first reply

  # Seed: instructions that say every checkout is the lab's fork, or forbid every inventory
  # unscoped, fail the Thens.
  Scenario: The instructions scope the fork's rule to the fork
    Given the node hands out "main" of a repository outside the lab
    When the model replies "repo_checkout main"
    Then the model's instructions say "In the lab's fork, never edit"
    And the model's instructions do not say "repo_checkout brings the lab's ansible-nas fork"
