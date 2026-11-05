Feature: A kept checkout says a commit is not a submission
  The sandbox keeps an item's work between turns. In the minimum-viable-autonomy climb a turn was
  killed after it committed its repair and before it submitted it; the turn tried again called
  repo_checkout and read "Already checked out ... Go on from it", took the commit for the repair,
  and never submitted it. The worker now says the kept branch's full head, and that a commit is not
  a submission: unless the briefing names that head as HQ's newest submission for the item, it is
  not submitted, and submitting a revision HQ already has records nothing new.

  Background:
    Given a turn in the workbench

  # Seed: an "already there" that names no head, or says nothing of submitting, fails a Then.
  Scenario: A kept checkout with a commit names its head and says it is not yet a submission
    Given the workbench kept a checkout on "hq/task-7-fix" with a commit since its base
    When the model replies "repo_checkout hq/task-7-fix"
    Then the model was told "Already checked out" after its first reply
    And the model was told the kept branch's full head after its first reply
    And the model was told "A commit is not a submission" after its first reply
    And the kept checkout still holds its commit
