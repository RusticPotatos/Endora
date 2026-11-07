Feature: An item is not closed over work HQ never took as finished
  Correction-placement review, round 1 (qualification only: these scenarios are red until a
  ruling accepts an implementation). In climb 3 two items were closed while their sandbox held a
  committed repair the node never received: calibration HM1 closed "no branch changes" at
  5faf6aed, and DEV3 DM1 closed "no changes needed; no branch submitted" at 6656b0d. A local check
  run in the kept tree passed because of the unsubmitted change itself, and the next task redid
  the work. The worker reads its sandbox (trusted code, never the model's words) and reports it
  beside a closing set_status; HQ, whose record says which full heads it took as finished work,
  refuses to close over a head it never took as finished. Work kept only in progress is
  preserved, not finished. Leaving work unsubmitted stays the member's judgment, but it is said,
  naming the exact head, and kept in the thread. A sandbox the worker could not read is unknown,
  never "nothing outstanding".

  Background:
    Given Sol is the developer
    And the goal "Ship the watch tracker"
    And Sol has the task "Fix the failing test"
    And Sol's turn starts
    And Sol took "Fix the failing test" and read media's logs: "media: listening on 8096, ready"

  # Seed: HQ ignoring the sandbox report closes the item; the Thens fail.
  Scenario: Setting an item ready over a commit HQ never took is refused
    When Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at "5faf6aed00000000000000aa"
    Then it is refused, saying "hq/task-1-fix at 5faf6aed00000000000000aa"
    And it is refused, saying "never submitted as finished work"
    And the task "Fix the failing test" is doing

  # Control: the head HQ took as finished closes as before.
  Scenario: Setting an item ready over the head HQ took as finished is not refused
    When Sol submits the branch "hq/task-1-fix"
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at "a1b2c3d4e5f60718293a4b5c"
    Then it is not refused

  # Seed: a rule that took a work-in-progress keep for finished work closes; the Thens fail.
  Scenario: Work kept only in progress is not finished work
    When Sol keeps the branch "hq/task-1-fix" as work in progress at the same head
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at "a1b2c3d4e5f60718293a4b5c"
    Then it is refused, saying "kept only as work in progress"
    And the task "Fix the failing test" is doing

  # Seed: a rule that matched the branch name alone closes; the Thens fail.
  Scenario: A commit on top of the finished submission is not finished work
    When Sol submits the branch "hq/task-1-fix"
    And Sol sets "Fix the failing test" ready, the worker reporting "hq/task-1-fix" at "a1b2c3d4e5f60718293a4b5d"
    Then it is refused, saying "hq/task-1-fix at a1b2c3d4e5f60718293a4b5d"
    And the task "Fix the failing test" is doing

  # Control: a healthy inspection that made no branch closes without one (9ceda55c's route).
  Scenario: A sandbox holding no work of the item closes as before
    When Sol sets "Fix the failing test" ready, the worker reporting no work of the item
    Then it is not refused

  # Seed: an unreadable sandbox taken for an empty one closes; the Thens fail.
  Scenario: A sandbox the worker could not read is not taken for an empty one
    When Sol sets "Fix the failing test" ready, the worker reporting it could not read the sandbox
    Then it is refused, saying "could not be read"
    And the task "Fix the failing test" is doing

  # Seed: a close with no report at all (a forged or old worker) taken for an empty sandbox.
  Scenario: A closing call with no sandbox report from the worker is not taken for an empty one
    When Sol sets "Fix the failing test" ready with no sandbox report
    Then it is refused, saying "could not be read"

  # Seed: no way to leave work unsubmitted, or one that does not keep it in the thread.
  Scenario: Leaving work unsubmitted is said, naming its exact head, and kept in the thread
    When Sol sets "Fix the failing test" ready, leaving "hq/task-1-fix" at "5faf6aed00000000000000aa" unsubmitted because "the experiment was wrong", the worker reporting "hq/task-1-fix" at "5faf6aed00000000000000aa"
    Then it is not refused
    And the thread of "Fix the failing test" says "left hq/task-1-fix at 5faf6aed00000000000000aa unsubmitted: the experiment was wrong" once

  # Seed: a discard that names another head than the sandbox holds closes; the Thens fail.
  Scenario: Leaving a different head unsubmitted does not cover the one the sandbox holds
    When Sol sets "Fix the failing test" ready, leaving "hq/task-1-fix" at "1111111100000000000000aa" unsubmitted because "old try", the worker reporting "hq/task-1-fix" at "5faf6aed00000000000000aa"
    Then it is refused, saying "hq/task-1-fix at 5faf6aed00000000000000aa"
    And the task "Fix the failing test" is doing

  # Control: blocking keeps the work where it is; nothing is closed.
  Scenario: Blocking the item over unsubmitted work is not refused
    When Sol sets "Fix the failing test" blocked, the worker reporting "hq/task-1-fix" at "5faf6aed00000000000000aa"
    Then it is not refused
