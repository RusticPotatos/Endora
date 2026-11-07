Feature: An item is not closed over work HQ never took as finished
  Correction-placement review, rounds 1-2 (qualification only: red until a ruling accepts an
  implementation). In climb 3 two items were closed while their sandbox held a committed repair
  the node never received: calibration HM1 closed "no branch changes" at 5faf6aed, and DEV3 DM1
  closed "no changes needed; no branch submitted" at 6656b0d. A local check run in the kept tree
  passed because of the unsubmitted change itself, and the next task redid the work.
  The worker reads its sandbox (trusted code, never the model's words) and reports the task's
  hq/ branches with their full heads beside a closing set_status. The guard applies only where
  the node's own record shows the item has a sandbox checkout: a successful repo_checkout
  receipt the node wrote for a turn on it. There HQ will not accept a close over a reported head
  it never took as finished work, unless the call names that exact head as left unsubmitted,
  with a reason, which is kept in the thread; sandbox-only work may disappear when the item
  closes. A head at the default branch's revision the node handed over (named in full in its
  receipt) is no work; a kept branch's checkout is no baseline. Work kept only in progress is not
  finished. Without a checkout receipt (the owner, HQ itself, a turn with no workbench), or with no
  report at all (an older worker), nothing changes; a report HQ cannot understand is refused. Uncommitted edits are outside this rule. Heads are named here
  (the handover head, the unsubmitted head, ...); the steps use full 40-character object ids.

  Background:
    Given Sol is the developer
    And the goal "Ship the watch tracker"
    And Sol has the task "Fix the failing test"
    And Sol's turn starts
    And Sol took "Fix the failing test" and read media's logs: "media: listening on 8096, ready"

  # ---- Where the guard applies: the node recorded a checkout for the item ----

  # Seed: HQ ignoring the sandbox report closes the item; the Thens fail.
  Scenario: Setting an item ready over a commit HQ never took is refused
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is refused, naming "hq/task-1-fix" at the unsubmitted head
    And it is refused, saying "never submitted as finished work"
    And the task "Fix the failing test" is doing

  # Control: the head HQ took as finished closes as before.
  Scenario: Setting an item ready over the head HQ took as finished is not refused
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol submitted the branch "hq/task-1-fix" at the finished head
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the finished head
    Then it is not refused

  # Seed: a rule that took a work-in-progress keep for finished work closes; the Thens fail.
  Scenario: Work kept only in progress is not finished work
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol kept the branch "hq/task-1-fix" at the finished head as work in progress
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the finished head
    Then it is refused, saying "kept only as work in progress"
    And the task "Fix the failing test" is doing

  # Seed: a rule that matched the branch name alone closes; the Thens fail.
  Scenario: A commit on top of the finished submission is not finished work
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol submitted the branch "hq/task-1-fix" at the finished head
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the later head
    Then it is refused, naming "hq/task-1-fix" at the later head
    And the task "Fix the failing test" is doing

  # Control: a task branch made at the handed-over revision, with nothing committed, is no work.
  Scenario: A task branch at the revision the node handed over is no work
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the handover head
    Then it is not refused

  # Control: a healthy inspection that made no branch closes without one (9ceda55c's route).
  Scenario: A sandbox holding no work of the item closes as before
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, the worker reporting no work of the item
    Then it is not refused

  # Seed: an unreadable sandbox taken for an empty one closes; the Thens fail.
  Scenario: A sandbox the worker could not read is not taken for an empty one
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, the worker reporting it could not read the sandbox
    Then it is refused, saying "could not be read"
    And the task "Fix the failing test" is doing

  # Seed: a report HQ cannot understand taken for an empty one closes; the Thens fail.
  Scenario: Where the node recorded a checkout, a report HQ cannot understand is refused
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, the worker's report naming a branch with no head
    Then it is refused, saying "could not be understood"
    And the task "Fix the failing test" is doing

  # Control: an older worker sends no report; its close is left as it was, checkout or not.
  Scenario: Where the node recorded a checkout, a close with no report is left as it was
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready with no sandbox report
    Then it is not refused

  # Seed: a kept branch's checkout taken as a no-work baseline lets work in progress close.
  Scenario: A checkout of kept work in progress is no baseline
    Given the node recorded Sol's checkout of the kept branch "hq/task-1-fix" at the finished head
    When Sol kept the branch "hq/task-1-fix" at the finished head as work in progress
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the finished head
    Then it is refused, saying "kept only as work in progress"

  # Seed: an abbreviated receipt matched on its twelve characters lets a commit close.
  Scenario: An older, abbreviated checkout receipt proves the checkout but not the revision
    Given the node recorded Sol's checkout of "develop" at the handover head, abbreviated as older nodes wrote it
    When Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the handover head
    Then it is refused, naming "hq/task-1-fix" at the handover head

  # Seed: no way to leave work unsubmitted, or one that does not keep it in the thread.
  Scenario: Leaving work unsubmitted is said, naming its exact head, and kept in the thread
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, leaving "hq/task-1-fix" at the unsubmitted head unsubmitted because "the experiment was wrong", the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is not refused
    And the thread of "Fix the failing test" records "hq/task-1-fix" at the unsubmitted head left unsubmitted because "the experiment was wrong"

  # Seed: a disposition that names another head than the sandbox holds closes; the Thens fail.
  Scenario: Leaving a different head unsubmitted does not cover the one the sandbox holds
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready, leaving "hq/task-1-fix" at the other head unsubmitted because "old try", the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is refused, naming "hq/task-1-fix" at the unsubmitted head
    And the task "Fix the failing test" is doing

  # Control: blocking keeps the work where it is; nothing is closed.
  Scenario: Blocking the item over unsubmitted work is not refused
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" blocked, the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is not refused

  # ---- A disposition is no exemption from the ordinary refusals (controls, green today) ----

  Scenario: Leaving work unsubmitted does not excuse missing evidence
    Given the node recorded Sol's checkout of the base at the handover head
    When Sol sets "Fix the failing test" ready citing nothing, leaving "hq/task-1-fix" at the unsubmitted head unsubmitted because "not wanted", the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is refused, saying "this needs evidence"
    And the task "Fix the failing test" is doing

  Scenario: Leaving work unsubmitted does not let another member close the item
    Given the node recorded Sol's checkout of the base at the handover head
    And Kit is the tester
    And Kit has the task "Review the watch tracker"
    And Kit's turn starts
    When Kit sets "Fix the failing test" ready, leaving "hq/task-1-fix" at the unsubmitted head unsubmitted because "not wanted", the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is refused
    And the task "Fix the failing test" is doing

  # ---- Where the guard does not apply: no checkout the node recorded ----

  # Control: owner, internal, non-workbench and older-worker paths are unchanged.
  Scenario: Without a checkout the node recorded, a close with no sandbox report is unchanged
    When Sol sets "Fix the failing test" ready with no sandbox report
    Then it is not refused

  # Control: a report on an item with no recorded checkout changes nothing either.
  Scenario: Without a checkout the node recorded, a reported head changes nothing
    When Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at the unsubmitted head
    Then it is not refused
