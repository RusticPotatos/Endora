Feature: A branch outside the lab is never given the lab's check
  Set 3 of the baseline experiment, item 1048: Sol's simpleeval fix was submitted and verified
  (174 tests in its sandbox), and its gate was open: the item's criteria are an outside
  repository's gate. Sol, wanting a receipt to cite, asked for the branch to be checked again.
  HQ queued the lab's check on it; the check box fetched the bundle where the lab's branches
  live, got the node's 404, and the failed check shut the gate and counted as one of the item's
  failures, twice. The lab's check is ansible's and says nothing of another repository, so it is
  not queued for one, and the member is told how such work is evidenced (ADR 0115, amended
  2026-11-01).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "simpleeval"
    And the owner names "https://github.com/example/tiny" as the repository of the loop "simpleeval"
    And the loop "simpleeval" has the item "Walk shared containers once", with the criteria
      | Self-referential containers evaluate |
    And the loop hands out its items
    And Bo submits the branch "hq/task-1-walk-shared-containers-once"

  # Seed: a recheck that queues the lab's check on any branch fails the Thens.
  Scenario: Asking for the lab's check of an outside branch is refused, and the gate stays open
    When Bo asks for "Walk shared containers once" to be checked again
    Then it is refused, saying "no check of the lab's applies to https://github.com/example/tiny"
    And it is refused, saying "set_status in_review"
    And no check is queued for "Walk shared containers once"
    And the gate of "Walk shared containers once" is open

  Scenario: The submission is the evidence the work goes to review on
    When Bo asks for "Walk shared containers once" to be checked again
    And Bo asks for review of "Walk shared containers once"
    Then it is not refused
    And the task "Walk shared containers once" is in_review
