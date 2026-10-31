Feature: A CI run HQ cannot get
  In round 2 of the baseline experiment, task 932's criterion "A CI run on the branch passes" was
  never met, and HQ finished without noticing: the fork's workflow runs on a push only to main
  and develop, so a branch HQ pushes (hq/task-N-...) never gets a run (D6). The owner, 31 Oct:
  until the workflow also runs on hq/ branches, HQ says when a CI criterion cannot be met rather
  than finishing without it. HQ reads the workflow's push filter in the branch itself, by rule,
  and the item cannot be passed on that criterion (ADR 0142, amended 2026-10-31).

  Background:
    Given the five ranked members Mara, Kit, Ada, Rae and Bo
    And the loop "ansible-nas"
    And the loop "ansible-nas" has the item "Run the lint rules' tests in CI", with the criteria
      | The workflow runs tests/test_lint_rules.py |
      | A CI run on the branch passes              |

  # Seed: 932's branch was pushed, no run came, and nothing said so.
  Scenario: A branch no workflow runs on a push of is reported, and is not passed
    When the loop hands out its items
    And Bo submits the branch "hq/task-1-ci-lint-rules", whose workflow "ci.yml" runs on a push to "main, develop"
    And Bo asks for review of "Run the lint rules' tests in CI"
    Then HQ's checks on "Run the lint rules' tests in CI" say "no CI run can be had for hq/task-1-ci-lint-rules"
    And HQ's checks on "Run the lint rules' tests in CI" say "cannot be met as the fork stands"
    And Rae is woken to review "Run the lint rules' tests in CI"
    When Rae passes "Run the lint rules' tests in CI" on a reading of her own
    Then it is refused for "no CI run can be had for hq/task-1-ci-lint-rules"
    And the task "Run the lint rules' tests in CI" is in_review

  Scenario: A branch a workflow runs on a push of is told when the run comes
    When the loop hands out its items
    And Bo submits the branch "hq/task-1-ci-lint-rules", whose workflow "ci.yml" runs on a push to "hq/**"
    And Bo asks for review of "Run the lint rules' tests in CI"
    Then HQ's checks on "Run the lint rules' tests in CI" say "ci.yml runs on a push to hq/task-1-ci-lint-rules: a run comes only once the branch is pushed"
