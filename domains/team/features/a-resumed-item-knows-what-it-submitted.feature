Feature: A resumed item knows what it submitted
  In the minimum-viable-autonomy climb an item's turn was killed after its repair was committed
  and before it was submitted. HQ tried the turn again; its briefing said "Nothing new in the
  thread since your last turn" and its checkout said "Go on from it", and neither said the branch
  had never been submitted. Two items of eight so interrupted lost the repair: one closed its check
  done as if the commit were the repair, one never submitted it. HQ keeps every submission it
  records; a turn tried again, and any turn on an item that has a submission, is now told the
  newest one HQ recorded, with its full head, or that HQ has recorded none, and that a commit is
  not a submission. A revision submitted again, which a turn unsure of its submission may do,
  records nothing new: one revision is one submission.

  Background:
    Given Sol is the developer
    And the goal "Ship the watch tracker"
    And Sol has the task "Fix the failing test"
    And Sol's turn starts

  # Seed: a briefing that says nothing of submissions fails the Then.
  Scenario: A turn tried again after an interruption, with nothing submitted, is told so
    When the runner reports Sol's turn on "Fix the failing test" killed from outside
    And the pass HQ was asked for runs
    And the owner replies "Please carry on" on "Fix the failing test"
    And the pass HQ was asked for runs
    And Sol's turn starts
    Then Sol's turn on "Fix the failing test" is running again
    And Sol's briefing says "HQ has recorded no submission for this item as finished work; the node may hold one HQ never heard of"
    And Sol's briefing says "A commit is not a submission"

  # Seed: a briefing that names no head fails the Then.
  Scenario: A turn tried again after its submission is told the head HQ recorded
    When Sol submits the branch "hq/task-1-fix-the-failing-test"
    And the runner reports Sol's turn on "Fix the failing test" killed from outside
    And the pass HQ was asked for runs
    And the owner replies "Please carry on" on "Fix the failing test"
    And the pass HQ was asked for runs
    And Sol's turn starts
    Then Sol's briefing says "HQ's newest submission for this item: hq/task-1-fix-the-failing-test at a1b2c3d4e5f60718293a4b5c (submitted), acknowledged as finished work"

  # Seed: a record per call, a second check or a second post fails a Then.
  Scenario: The same revision submitted again records nothing new
    When Sol submits the branch "hq/task-1-fix-the-failing-test"
    And Sol submits the branch "hq/task-1-fix-the-failing-test"
    Then "Fix the failing test" has 1 branch record
    And "Fix the failing test" has 1 check queued
    And the thread of "Fix the failing test" says "Submitted branch hq/task-1-fix-the-failing-test" once

  # Seed: a check against the newest record alone, which a work-in-progress keep of the same head
  # hides, fails the first Then.
  Scenario: A finished submission kept again as work in progress, then retried, is still one submission
    When Sol submits the branch "hq/task-1-fix-the-failing-test"
    And Sol keeps the branch "hq/task-1-fix-the-failing-test" as work in progress at the same head
    And Sol submits the branch "hq/task-1-fix-the-failing-test"
    Then "Fix the failing test" has 1 finished submission
    And "Fix the failing test" has 1 check queued
    And the thread of "Fix the failing test" says "Submitted branch hq/task-1-fix-the-failing-test" once

  # Seed: a rule that took a work-in-progress keep for the finished submission fails the Thens.
  Scenario: Work kept only in progress can still be offered as finished
    When Sol keeps the branch "hq/task-1-fix-the-failing-test" as work in progress at the same head
    And Sol submits the branch "hq/task-1-fix-the-failing-test"
    Then "Fix the failing test" has 1 finished submission
    And "Fix the failing test" has 1 check queued

  # Seed: a rule that matched the branch name alone fails the Thens.
  Scenario: A new full head on the same branch is a new submission
    When Sol submits the branch "hq/task-1-fix-the-failing-test"
    And Sol submits the branch "hq/task-1-fix-the-failing-test" at a new head
    Then "Fix the failing test" has 2 finished submissions
    And "Fix the failing test" has 2 checks queued

  # Seed: telling every first turn about submissions it cannot have fails the Then.
  Scenario: A first turn on an item with nothing submitted is not told of submissions
    Then Sol's briefing does not say "submission"
