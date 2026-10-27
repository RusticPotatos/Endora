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

  Scenario: Hiring is the owner's, and one CEO is enough
    When the owner hires Sam as a developer under Kit
    Then Sam reports to Kit
    And Sam works with "Local" and judges with "Claude Haiku"
    When the owner hires Second as a ceo under Mara
    Then it is refused for "the team has one CEO already"

  # The check after a deploy (ADR 0115, amended): the owner's Deployed hands the member of the
  # routine that takes the lab's alarms one turn to read the lab, with the lab's reads alone and
  # no shell. A healthy verdict rests on a reading of the turn's own, and comes back to the
  # change. A second click starts nothing.
  Scenario: Deployed starts one read-only check of the lab, and its healthy verdict comes back
    Given Kit's planning turn is over
    And the lab's routine is Ada's, in "Home lab upkeep"
    And Kit hands Rae the task "Add the drift check" in the wing, with the criteria
      | The check is no worse than develop |
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-drift-check"
    And Rae's turn ends
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 16 errors (develop 16): no worse than develop"
    And "Add the drift check" is accepted and pushed
    Then the owner's desk asks whether "hq/task-2-add-the-drift-check" is deployed
    When the owner says it is deployed
    Then it is not refused
    When Ada's turn on "Check the home lab after deploying hq/task-2-add-the-drift-check" starts
    Then Ada's turn is offered "claim, my_turn, post, set_status" alone
    And Ada's turn may use the lab's "homelab_status, container_logs, nas_status" alone, with no shell
    When Ada uses "create_task" in her turn
    Then it is refused for "not for a post-deploy check"
    When Ada reads the lab and closes "Check the home lab after deploying hq/task-2-add-the-drift-check", citing her reading
    Then it is not refused
    When Ada's turn ends
    Then the last post in "Add the drift check" starts "Post-deploy infrastructure check: Healthy."
    When the owner says it is deployed
    Then it is refused

  # A check whose one turn ends without a verdict is closed as inconclusive: the change is not
  # called healthy, and the owner is told on the desk.
  Scenario: A deploy check that ends without a verdict is inconclusive, never healthy
    Given Kit's planning turn is over
    And the lab's routine is Ada's, in "Home lab upkeep"
    And Kit hands Rae the task "Add the drift check" in the wing, with the criteria
      | The check is no worse than develop |
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-drift-check"
    And Rae's turn ends
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 16 errors (develop 16): no worse than develop"
    And "Add the drift check" is accepted and pushed
    And the owner says it is deployed
    And Ada's turn on "Check the home lab after deploying hq/task-2-add-the-drift-check" starts
    And Ada's turn ends
    Then the last post in "Add the drift check" starts "Post-deploy infrastructure check: Inconclusive."
    And the owner's desk holds a "deploy_check" card saying ": Inconclusive"
    And the task "Check the home lab after deploying hq/task-2-add-the-drift-check" is dropped
