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

  # The wing's first 39 hours: six branches submitted, none reached the owner. One docs-only
  # branch, from the check that passes on develop's own findings to the owner's push card.
  Scenario: A docs-only branch reaches the owner's push card on develop's own findings
    Given Kit's planning turn is over
    And Kit hands Rae the task "Add the missing app pages" in the wing, with the criteria
      | Branch is cut from develop and named hq/task-N-add-the-missing-app-pages |
      | Diff only adds files under website/docs/applications                      |
      | The check is no worse than develop                                        |
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-missing-app-pages"
    And Rae's turn ends
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 14 errors (develop 14): no worse than develop"
    Then the gate of "Add the missing app pages" has passed
    # The check's pass wakes her; her sandbox is new, and the kept branch comes back whole, at
    # the head that was submitted.
    When Rae's turn starts
    And Rae checks out "hq/task-2-add-the-missing-app-pages"
    Then the checkout is the submitted head of "Add the missing app pages"
    When Rae submits "Add the missing app pages" for review citing the submit and the check receipts
    # The check is not the tester's own reading: her checkout of the branch is.
    And Dee passes "Add the missing app pages" citing the check receipt alone
    Then it is refused for "her own readings"
    When Dee checks out "hq/task-2-add-the-missing-app-pages"
    And Dee passes "Add the missing app pages" citing her checkout and the check receipt
    Then it is not refused
    When Dee's turn ends
    And Kit accepts "Add the missing app pages"
    Then the task "Add the missing app pages" is done
    And the owner's desk holds a push of "hq/task-2-add-the-missing-app-pages" at the submitted head, saying "checks passed"
    When the owner approves the push
    Then Endora is asked to push "hq/task-2-add-the-missing-app-pages"
    When the node says it pushed: "Pushed hq/task-2-add-the-missing-app-pages to the fork: merge it on GitHub."
    Then Rae is woken about it

  # Task 687: the tester cited her readings as every tool result names them, "receipt r228",
  # and with what each showed after it; she was refused for it four times and the branch sat in
  # review. A reference is read as the tools write it.
  Scenario: A tester's pass cites receipts as the tools name them
    Given Kit's planning turn is over
    And Kit hands Rae the task "Add the drift check" in the wing, with the criteria
      | Branch is cut from develop and named hq/task-N-add-the-drift-check |
      | The check is no worse than develop                                 |
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-drift-check"
    And Rae's turn ends
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 16 errors (develop 16): no worse than develop"
    And Rae's turn starts
    And Rae submits "Add the drift check" for review citing the submit and the check receipts
    And Dee checks out "hq/task-2-add-the-drift-check"
    And Dee passes "Add the drift check" citing her checkout and the check as the tools name them
    Then it is not refused
    When Dee's turn ends
    And Kit accepts "Add the drift check"
    Then the task "Add the drift check" is done

  # Task 687: the Ops lead built and submitted a branch herself, could neither accept her own
  # work nor hand it on, and the CEO was asked twice. Changing the lab is the wing's (ADR 0120):
  # outside it a submission is refused, naming the wing's lead and how to ask for the change.
  Scenario: The Ops lead's change to the lab is the wing's to build
    Given Kit's planning turn is over
    And Ada has the task "Add the drift check"
    When Ada's turn starts
    And Ada tries to submit the branch "hq/task-2-add-the-drift-check"
    Then it is refused: "changing the lab is the engineering wing's: Kit leads it and its developers submit the branches. Ask for the change with propose_project, with its criteria and evidence; the CEO decides and the wing builds it"
    And no branch is kept

  # Task 670 sat 44 minutes in review, was set blocked out of the tester's reach, and climbed
  # to the CEO five times for a review that had simply not happened.
  Scenario: Work left in review is never blocked, and its lead is woken
    Given Kit's planning turn is over
    And Kit hands Rae the task "Add the missing app pages" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-missing-app-pages"
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 14 errors (develop 14): no worse than develop"
    And Rae submits "Add the missing app pages" for review
    And "Add the missing app pages" sits with nobody coming back to it
    Then Kit is asked to decide "Add the missing app pages"
    And the task "Add the missing app pages" is in_review

  # A CEO's draft for the board moves the work task itself into review; passing it closed
  # the work, and offered its push, with no evidence judged.
  Scenario: A draft for the owner never closes the work
    Given Kit's planning turn is over
    And Kit hands Rae the task "Add the missing app pages" in the wing
    When Rae's turn starts
    And Rae submits the branch "hq/task-2-add-the-missing-app-pages"
    And Rae's turn ends
    And Mara is woken about "Add the missing app pages"
    And Mara puts the decision "Should the wing take a second lead?" to the board, recommending "Not yet", because "One branch waits"
    And the check passes: "syntax ok, ansible-lint 25 findings (develop 25), yamllint 14 errors (develop 14): no worse than develop"
    Then the gate of "Add the missing app pages" has passed
    And the task "Add the missing app pages" is in_review
    When Dee checks out "hq/task-2-add-the-missing-app-pages"
    And Dee passes "Add the missing app pages" citing her checkout and the check receipt
    Then it is refused for "draft for the owner"
    And the task "Add the missing app pages" is in_review
    And the owner's desk holds no push

  # The CEO directs and does not move an area's work (ADR 0120, amended). Live, it handed an
  # engineering branch task to the Ops lead.
  Scenario: The CEO cannot hand the wing's branch work to the Ops lead
    Given Kit's planning turn is over
    And Kit hands Rae the task "Tidy the deluge role" in the wing
    And Mara has the task "Review the house"
    When Mara tries to hand "Tidy the deluge role" to Ada
    Then it is refused: "Ada is outside Kit's area: only Kit, who leads it, or the owner hands its work out of it. Ask Kit in the task's thread (the CEO directs: direct, with evidence), and Kit decides"
    And the task "Tidy the deluge role" is still held by Rae
    And Ada has never been woken

  # Work closed as superseded says what overtook it: a lead who closes his plan so, naming
  # nothing, is refused and the plan stays open (ADR 0124, amended).
  # Seed: superseded accepted with no successor fails the refusal step.
  Scenario: The lead closes his plan as superseded only by naming the work that carries it on
    Given Kit hands Rae the task "Tidy the deluge role" in the wing
    When Kit closes the plan as superseded, naming no successor
    Then it is refused: "work done as superseded names the task that carries it on: give its number as `successor`"
    And the plan is still open
    When Kit closes the plan as superseded by "Tidy the deluge role"
    Then it is not refused
    And the plan is done
