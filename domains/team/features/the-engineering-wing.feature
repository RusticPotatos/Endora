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

  Scenario: The wing's branch reaches the owner only on approval
    Given Kit's planning turn is over
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-tidy-the-deluge-role"
    And the check of "hq/task-2-tidy-the-deluge-role" passes
    Then the owner's desk holds no push
    When Rae submits "Tidy the deluge role" for review
    And Dee passes "Tidy the deluge role" on the check and her own reading
    And Kit accepts "Tidy the deluge role"
    Then the task "Tidy the deluge role" is done
    And the owner's desk holds a push of "hq/task-2-tidy-the-deluge-role"
    When the owner approves the push
    Then Endora is asked to push "hq/task-2-tidy-the-deluge-role"
    When the node says it pushed: "Pushed hq/task-2-tidy-the-deluge-role to the fork: merge it on GitHub."
    Then Rae is woken about it

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

  Scenario: The wing may not grow past its seats
    When Kit's turn starts
    And Kit asks to hire a developer on "claude-sonnet-5-5"
    Then it is refused for "Claude Haiku or the house's model"
    When Kit asks to hire a developer on "claude-haiku-5-5"
    Then the owner's desk holds 1 hire request
    When the owner approves the hire
    Then the wing has 4 members
    Given the owner hires Lee as a developer under Kit
    And the owner hires Max as a developer under Kit
    Then the wing has 6 members
    When Kit asks to hire a developer on "claude-haiku-5-5"
    Then it is refused for "the wing seats 5 of its 5 developers"

  Scenario: Hiring is the owner's, and one CEO is enough
    When the owner hires Sam as a developer under Kit
    Then Sam reports to Kit
    And Sam works with "Local" and judges with "Claude Haiku"
    When the owner hires Second as a ceo under Mara
    Then it is refused for "the team has one CEO already"
