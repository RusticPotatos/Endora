Feature: A lint criterion is measured against develop
  In round 3 of the baseline experiment, item 949's branch pinned all seven tags and linted
  exactly as develop does (ansible-lint 14 and 14, yamllint 6 and 6), which is how Claude Code's
  arm and round 2's 931 were accepted. HQ's check measured "no worse than develop", yet the
  reviewer sent it back on "passes" read literally, the next member asked the owner, and it
  ended blocked: the judge verifies it, HQ reports it not done (D22). HQ's check now states its
  measure, "no more findings than develop: met"; HQ's checks give that verdict for a criterion
  about lint, and the reviewer is told to take it, as for CI. A blocked item's submission that
  passed its check stays listed on Manage, its bundle a download away (ADR 0142, amended
  2026-11-01).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Pin the image tags", with the criteria
      | Every image tag is pinned      |
      | ansible-lint and yamllint pass |
    When the loop hands out its items
    And Bo submits the branch "hq/task-1-pin-the-image-tags"
    And the check passes: "syntax ok, ansible-lint 14 findings (develop 14), yamllint 6 errors (develop 6): no more findings than develop: met"
    And Bo asks for review of "Pin the image tags"

  # Seed: the check said "no worse than develop", and the reviewer read "pass" literally.
  Scenario: The check's verdict is the measure of a lint criterion, and the reviewer is told so
    Then HQ's checks on "Pin the image tags" say "is measured by the check of the head against develop: check c1 of hq/task-1-pin-the-image-tags passed"
    And HQ's checks on "Pin the image tags" say "no more findings than develop: met"
    And Rae's turn starts on "Pin the image tags"
    And Rae's briefing says "take the check's verdict for a criterion about lint"
    And Rae's briefing says "it compares each linter's count of findings with develop's"

  # Seed: 949 ended blocked, and its work survived only in the node's mirror.
  Scenario: A blocked item's submission that passed its check stays where delivery reaches it
    When Rae sends "Pin the image tags" back: "ansible-lint does not pass: 14 findings"
    And Bo blocks "Pin the image tags", saying "the criterion asks for a clean lint, which develop is not"
    Then Manage shows "hq/task-1-pin-the-image-tags" as kept, its item blocked
