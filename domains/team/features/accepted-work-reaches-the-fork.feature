Feature: Accepted work reaches the fork without a card
  The owner, 31 Oct: merging accepted work "is just not something I need to review", and no
  "Ready to merge" or "Push" cards on the desk. Accepted branches are listed on Manage. A branch
  that passed its review, its check and HQ's mechanical checks is pushed to the fork by the node
  once its privacy scan is clean; a scan that finds anything pushes nothing and puts one card on
  the desk saying what and where, never the value. Merging the fork's develop is a setting, off
  while the baseline experiment's rounds run. A change that needs something of the owner still
  reaches the desk; the rest is a notice that clears itself. A setting puts the cards back
  (ADR 0115, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Add the drift check", with the criteria
      | The check is no worse than develop |
    And the loop hands out its items
    And Bo submits the branch "hq/task-1-add-the-drift-check"
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Bo asks for review of "Add the drift check"

  Scenario: Accepted work is listed on Manage, and nothing reaches the desk
    When Rae passes "Add the drift check" on a reading of her own
    Then the owner's desk holds no "ready_to_merge" card
    And the owner's desk holds no push
    And Manage lists "hq/task-1-add-the-drift-check" for "Add the drift check" as accepted, waiting to merge
    And the node is asked to push "hq/task-1-add-the-drift-check"

  Scenario: A clean branch is pushed without a card, and the notice clears itself
    When Rae passes "Add the drift check" on a reading of her own
    And the node pushes "hq/task-1-add-the-drift-check", its privacy scan clean
    Then the owner's desk holds nothing about "Add the drift check"
    And Manage shows "hq/task-1-add-the-drift-check" as on the fork, waiting to merge
    And the node is not asked to push "hq/task-1-add-the-drift-check"

  Scenario: A branch the privacy scan holds is not pushed, and one card says what and where
    When Rae passes "Add the drift check" on a reading of her own
    And the node's privacy scan of "hq/task-1-add-the-drift-check" finds "a real network address" at "roles/drift/defaults/main.yml line 3"
    Then the owner's desk holds one "privacy" card saying "a real network address (roles/drift/defaults/main.yml line 3)"
    And Manage shows "hq/task-1-add-the-drift-check" as held by the privacy scan
    And the node is not asked to push "hq/task-1-add-the-drift-check"
    When the owner sends it back
    Then the task "Add the drift check" is doing
    And the last post in "Add the drift check" starts "Not pushed: the privacy scan"

  Scenario: The owner may push a held branch anyway, having read what was found
    When Rae passes "Add the drift check" on a reading of her own
    And the node's privacy scan of "hq/task-1-add-the-drift-check" finds "a phone number" at "commit a1b2c3d4e5f6 message"
    And the owner pushes it anyway
    Then the node is asked to push "hq/task-1-add-the-drift-check" without its scan

  Scenario: A change that needs something of the owner says so on the desk
    When Rae passes "Add the drift check" on a reading of her own
    And the node pushes "hq/task-1-add-the-drift-check", finding it needs "vault_drift_token" at "roles/drift/defaults/main.yml"
    Then the owner's desk holds one "needs_owner" card saying "vault_drift_token (roles/drift/defaults/main.yml)"

  Scenario: Merging develop is off while the rounds run, so a pushed branch stays listed
    When Rae passes "Add the drift check" on a reading of her own
    And the node pushes "hq/task-1-add-the-drift-check", its privacy scan clean
    Then merging develop is off
    And the node is not asked to merge "hq/task-1-add-the-drift-check"

  Scenario: With merging on, HQ has the pushed branch merged into develop
    Given the owner turns merging develop on
    When Rae passes "Add the drift check" on a reading of her own
    And the node pushes "hq/task-1-add-the-drift-check", its privacy scan clean
    Then the node is asked to merge "hq/task-1-add-the-drift-check"
    When the node merges "hq/task-1-add-the-drift-check" into develop
    Then Manage no longer lists "hq/task-1-add-the-drift-check"
    And the last post in "Add the drift check" starts "merged hq/task-1-add-the-drift-check into develop"

  Scenario: The owner can put the cards back
    Given the owner puts the merge cards back on the desk
    When Rae passes "Add the drift check" on a reading of her own
    Then the owner's desk holds one "ready_to_merge" card saying "Ready to merge: Add the drift check"
    And the owner's desk holds one "push" card saying "Push hq/task-1-add-the-drift-check to your fork"
    And the node is not asked to push "hq/task-1-add-the-drift-check"

  Scenario: A ready-to-merge card already waiting stays until the owner answers it
    Given the owner puts the merge cards back on the desk
    When Rae passes "Add the drift check" on a reading of her own
    And the owner takes the merge cards off the desk again
    Then the owner's desk holds one "ready_to_merge" card saying "Ready to merge: Add the drift check"
    When the owner marks it merged
    Then the owner's desk holds no "ready_to_merge" card
