Feature: A closing call carries the worker's own reading of its sandbox
  Correction-placement review, round 1 (qualification only: red until a ruling accepts an
  implementation). In climb 3 two items were closed while their sandbox held a committed repair
  the node never received (calibration HM1 at 5faf6aed; DEV3 DM1 at 6656b0d). HQ knows which full
  heads it took as finished work; only the sandbox knows what is there. So when the model moves
  its own task to in_review or done, the worker reads the sandbox's task branches with git and
  sends them beside the call, in place of anything the model wrote there; a sandbox it cannot
  read is reported unknown, never empty.

  Background:
    Given a turn in the workbench

  # Seed: a set_status sent as the model wrote it, with no reading, fails the Then.
  Scenario: Setting the task ready reports the kept branch and its full head
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    When the model replies "set_status 7 in_review"
    Then the turn ended moving its task on
    And HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  # Seed: a report the model wrote itself reaches HQ; the Then fails.
  Scenario: A sandbox report the model wrote is replaced by the worker's own reading
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    When the model replies "set_status 7 in_review claiming-an-empty-sandbox"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  # Seed: no report taken for an empty sandbox; the Then fails.
  Scenario: A workbench with no checkout reports no work of the task
    When the model replies "set_status 7 done"
    Then HQ was told the sandbox holds no work of the task

  # Seed: an unreadable checkout reported as empty, or not at all; the Then fails.
  Scenario: A checkout git cannot read is reported unknown
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the workbench's checkout cannot be read
    When the model replies "set_status 7 in_review"
    Then HQ was told the sandbox could not be read

  # Seed: a reading taken from checkout.json, which adoption rewrites, loses the kept head.
  Scenario: Taking over a kept checkout does not change what the closing call reports
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    When the model replies "repo_checkout hq/task-7-fix", then "set_status 7 in_review"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  # Round 2 controls. Seed for each: a reading that hides or moves the kept head fails the Then.
  Scenario: A non-hq name at the kept head does not hide the task branch
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the workbench's checkout also names its kept head "scratch"
    When the model replies "set_status 7 in_review"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  Scenario: A local base branch moved onto the kept head does not hide the task branch
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the workbench's base branch was moved onto the kept head
    When the model replies "set_status 7 in_review"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  # The worker reports the head as it is; whether it is the handover base is HQ's to judge from
  # the node's checkout receipt, never from a local base ref.
  Scenario: A task branch at its base with no commit is reported as it is
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the workbench's task branch is at its base, with no commit of its own
    When the model replies "set_status 7 in_review"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  Scenario: Fetching the node's latest beside the kept work does not change the reading
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the node hands out the team's kept branch "hq/task-7-fix" of the lab's fork
    When the model replies "repo_checkout hq/task-7-fix refresh", then "set_status 7 in_review"
    Then the model was told "Fetched the node's latest into your checkout" after its first reply
    And HQ was told the sandbox holds "hq/task-7-fix" at the kept head

  # The kept commit has no trailer: the worker stamps it before bundling, which moves the head.
  Scenario: After a submission the reading names the stamped head the node was sent
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    And the node hands out the team's kept branch "hq/task-7-fix" of the lab's fork
    When the model replies "repo_submit hq/task-7-fix", then "set_status 7 in_review"
    Then HQ was told the sandbox holds "hq/task-7-fix" at the head the node was sent
