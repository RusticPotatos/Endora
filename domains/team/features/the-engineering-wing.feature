Feature: The engineering wing and the git flow
  The wing works on the lab's ansible-nas fork through branches (ADR 0115).
  A branch is checked on the dev node, read by the tester, accepted by the
  lead, and reaches the fork when the owner approves the push. Since 31 Oct the
  push is HQ's own, without a card, unless the owner puts the cards back
  (accepted-work-reaches-the-fork.feature); these scenarios have the cards.
  HQ opens no pull request. The wing's seats are a rule in code; hiring is the
  owner's.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Dee is the tester
    And the goal "Keep the home lab healthy"
    And the engineering wing

  Scenario: A branch that fails its checks is sent back
    Given Kit's planning turn is over
    And the owner hands Rae the task "Tidy the deluge role" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-tidy-the-deluge-role"
    And Rae submits "Tidy the deluge role" for review
    And the check of "hq/task-2-tidy-the-deluge-role" fails: "yamllint failed"
    Then the task "Tidy the deluge role" is doing
    And Rae is woken about it
    And the owner's desk holds no push

  Scenario: Hiring is the owner's, and a title is a name only
    When the owner hires Sam as a developer
    Then Sam's model is "Local"
    When the owner hires Second as a ceo
    Then it is not refused
