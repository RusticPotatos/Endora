Feature: The engineering wing and the git flow
  The wing works on the lab's ansible-nas fork through branches (ADR 0115).
  A branch is checked on the dev node, read by the tester, accepted by the
  lead, and reaches the fork only when the owner approves the push. HQ never
  pushes on its own, opens no pull request and merges nothing. The wing's
  seats are a rule in code; hiring is the owner's.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Dee is the tester
    And the goal "Keep the home lab healthy"
    And the engineering wing

  Scenario: A branch that fails its checks is sent back
    Given Kit's planning turn is over
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-tidy-the-deluge-role"
    And Rae submits "Tidy the deluge role" for review
    And the check of "hq/task-2-tidy-the-deluge-role" fails: "yamllint failed"
    Then the task "Tidy the deluge role" is doing
    And Rae is woken about it
    And the owner's desk holds no push

  Scenario: Hiring is the owner's, and a title is a name only
    When the owner hires Sam as a developer under Kit
    Then Sam reports to Kit
    And Sam's model is "Local"
    When the owner hires Second as a ceo under Mara
    Then it is not refused

