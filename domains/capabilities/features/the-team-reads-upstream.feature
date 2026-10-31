@mirror
Feature: The team reads upstream ansible-nas, and never writes to it
  The owner's lab is built from a fork of ansible-nas, and upstream ansible-nas goes on without
  it. In round 2 of the baseline experiment HQ's task 936, "List what upstream ansible-nas
  changed that this fork lacks", was blocked: the team's sandbox is offline, and repo_checkout
  hands it only the fork's develop. Claude Code did the same task by fetching upstream from
  GitHub. The node may reach GitHub; the sandbox may not. So the node keeps upstream's default
  branch beside its mirror of the fork, refreshed on the same schedule, and every checkout
  carries it as upstream/<branch>, read-only: the member compares the two offline with git.
  Upstream is never a push target: a branch naming it, or built on its commits, is refused.

  Background:
    Given the owner's fork of ansible-nas, worked from "develop"
    And upstream ansible-nas, whose default branch is "master", with two commits the fork lacks

  # Seed: a checkout with only the fork's develop in it fails the second Then.
  Scenario: A checkout of the fork carries upstream, to read offline
    When a member checks out the fork
    Then the checkout holds the fork's "develop"
    And it holds upstream's "master" as "upstream/master"
    And offline, "git log develop..upstream/master" lists the two commits upstream has

  # Seed: fetching upstream on every checkout, or never again, fails one of the Thens.
  Scenario: Upstream is refreshed on the fork's schedule
    Given a member checked out the fork
    When upstream gains a commit
    Then a checkout within five minutes shows upstream as it was
    But a checkout five minutes later shows upstream's new commit

  # Seed: making an unreachable upstream fail the refresh fails the Then.
  Scenario: The fork is still checked out when upstream cannot be reached
    Given upstream cannot be reached
    When a member checks out the fork
    Then the checkout holds the fork's "develop"
    And it holds no upstream

  Scenario: Upstream is not checked out on its own
    When a member asks to check out "upstream/master"
    Then it is refused, saying "every checkout of develop carries it as upstream/master"

  Scenario: A branch named for upstream is never taken
    When a member submits a branch named "upstream/master"
    Then it is refused, saying "upstream ansible-nas is read-only"

  # Seed: without the upstream rule a merge of upstream is refused only for its commits'
  # authors, which says nothing of upstream: the Then fails.
  Scenario: A branch built on upstream's commits is refused
    When a member submits "hq/task-936-sync" with upstream's master merged into develop
    Then it is refused, saying "carries upstream's commits"

  Scenario: A change read from upstream and made as HQ's own commit is taken
    When a member submits "hq/task-936-port" with one file copied from upstream as HQ's own commit
    Then it is taken

  Scenario: The node never pushes to upstream
    Given a member's branch "hq/task-936-port" was taken
    When the node is asked to push it to upstream
    Then it is refused, saying "upstream ansible-nas is read-only"
