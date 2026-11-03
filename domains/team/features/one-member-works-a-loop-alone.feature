Feature: One member works a loop alone
  Round 4's piping audit (item E of the baseline experiment; the owner, 1 Nov: "let the agents
  be agents") asks whether HQ's own process, not the models, causes the failures. One arm
  settles it: a loop the owner sets to be worked by one named member alone (ADR 0142, amended
  2026-11-01). Its items go to that member only, one at a time. The member checks out, works,
  checks its own criteria and sets the item ready (in review), and the item is done once HQ's
  checks have passed: the check of its newest head, and the mechanical checks at review. Nobody
  reviews it, a failure never climbs the ranks or hands it to anyone, and a turn that runs out
  of room goes on with the same member. Its ninth failure, three ranks' worth (the team arm's
  ceiling), blocks it with the reason and asks the owner, as the team's top rank does. The turn,
  the tools, the checks and the delivery are the team's own; the briefing says she works alone. The member takes no other loop's work and reviews none of it, so the team's
  arm and the solo arm share nobody.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Sol    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"
    And the loop "solo arm"
    And the owner sets the loop "solo arm" to be worked by Sol alone

  Scenario: The item goes to the named member, and is done when HQ's checks pass and she sets it ready
    The answer to setting it ready says what the item is now: in run 5316 it said "in_review" of
    an item HQ had already closed.
    Given the loop "solo arm" has the item "Document the media role": "Write roles/media/README.md from the role's defaults."
    When a tick passes
    Then "Document the media role" is Sol's
    And Sol is woken to take "Document the media role"
    When Sol takes "Document the media role" and reads media's logs: "media: listening on 8096, ready"
    And Sol submits the branch "hq/document-the-media-role", changing "roles/media/README.md"
    And Sol sets "Document the media role" ready, citing her reading
    Then it is refused for "the gate has not passed"
    When the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol asks for review of "Document the media role", citing her reading
    Then the task "Document the media role" is done
    And the thread of "Document the media role" says "HQ's checks passed and Sol set it ready: done, with no review"
    And the answer says "is done, not in_review"
    And no member is asked to review "Document the media role"

  Scenario: Her briefing says she works alone, and that setting it ready closes it
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turn starts
    Then Sol's briefing says "You work this loop alone"
    And Sol's briefing says "closes it, with no review"

  Scenario: A CI criterion no push can meet is hers to block, with the reason
    Given the loop "solo arm" has the item "Run the lint rules' tests in CI", with the criteria
      | The workflow runs tests/test_lint_rules.py |
      | A CI run on the branch passes              |
    And a tick passes
    When Sol submits the branch "hq/task-1-ci-lint-rules", whose workflow "ci.yml" runs on a push to "main, develop"
    And the check passes: "syntax ok, ansible-lint ok, yamllint ok"
    And Sol sets "Run the lint rules' tests in CI" ready, citing the item for each criterion
    Then it is refused for "no CI run can be had for hq/task-1-ci-lint-rules"
    And it is refused for "block the item with the reason, for the owner to decide"
    And the task "Run the lint rules' tests in CI" is doing

  Scenario: Failed checks stay with her, and climb to nobody
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol takes "Pin the images" and reads media's logs: "media: ready"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "ansible-lint: 2 new warnings"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "ansible-lint: 1 new warning"
    And Sol submits the branch "hq/pin-the-images", changing "roles/media/defaults/main.yml"
    And the check of "hq/pin-the-images" fails: "yamllint: 1 new error"
    Then "Pin the images" has 3 failures and is still Sol's
    And Kit is not woken to take "Pin the images"
    And the owner's desk holds nothing about "Pin the images"

  Scenario: A turn that runs out of room goes on with her, however often
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turns on "Pin the images" run out of room 12 times, keeping nothing
    Then "Pin the images" has 3 failures and is still Sol's
    And Sol is woken to continue "Pin the images"
    And Kit is not woken to take "Pin the images"

  Scenario: The ninth failure blocks it with the reason, and the owner is asked
    Given the loop "solo arm" has the item "Pin the images": "Pin roles/media off the latest tag."
    And a tick passes
    When Sol's turns on "Pin the images" run out of room 36 times, keeping nothing
    Then the task "Pin the images" is blocked
    And the owner's desk holds one question about "Pin the images"
    And the thread of "Pin the images" says "failed 9 times with Sol working this loop alone"
    And Kit is not woken to take "Pin the images"

  Scenario: She takes no other loop's work, and reviews none of it
    Given the loop "ansible-nas" starts at Ada's rank
    And the loop "ansible-nas" has on its plan, in this order
      | Fix the lint   |
      | Tidy the roles |
    When a tick passes
    Then "Fix the lint" is Ada's
    And "Tidy the roles" is Kit's
    When Ada submits "Fix the lint" for review
    Then Rae is woken to review "Fix the lint"
    And Sol is not woken to review "Fix the lint"
