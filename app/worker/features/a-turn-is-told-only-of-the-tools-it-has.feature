Feature: A turn is told only of the tools it has
  The architecture challenge (round 2, 5 November) found the worker's fixed instructions naming
  the home lab's tools in turns that were not offered them: a repository turn offered image_tags
  was told "homelab_status and container_logs only look" and "restart_container is a safe fix",
  and two calls in climb 2's Haiku baseline went to tools the turn did not have. Climb 2's S4:
  each tool-specific rule is said only when its tool is the turn's, from what the node and HQ
  actually offer; what is offered and enforced is unchanged.

  Background:
    Given a turn in the workbench

  # Seed: the home lab's rules said whenever any node tool is offered fail the Thens.
  Scenario: A repository turn offered image_tags is told nothing of the home lab's tools
    Given the node hands out "main" of a repository outside the lab
    And the node offers "image_tags" too
    When the model replies "repo_checkout main"
    Then the model's instructions do not say "homelab_status"
    And the model's instructions do not say "restart_container"
    And the model's instructions do not say "The home lab tools are Endora's contracts"
    And the model's instructions do not say "managed by ansible-nas"
    And the model's instructions say "try to work around a refusal"

  # Seed: every lab rule said for one lab tool fails the Thens.
  Scenario: A turn offered one of the lab's tools is told of that one
    Given the node hands out "develop" of the lab's fork
    And the node offers "homelab_status" too
    When the model replies "repo_checkout develop"
    Then the model's instructions say "this turn they are yours: homelab_status."
    And the model's instructions do not say "restart_container"
    And the model's instructions do not say "container_logs"

  # Seed: a rule for an HQ tool HQ did not offer fails the Then (this HQ offers no check_back).
  Scenario: A turn offered the lab's tools keeps their limits, and is told of check_back only when it has it
    Given the node hands out "develop" of the lab's fork
    And the node offers the home lab's tools too
    When the model replies "repo_checkout develop"
    Then the model's instructions say "one restart of a container an hour, three in all"
    And the model's instructions say "look and diagnose first"
    And the model's instructions do not say "call check_back"
    And the model's instructions say "confirmation is still outstanding"
    And the model's instructions do not say "say the fix is done"
    And the model's instructions do not say "homelab_map"

  # Rounds 40–41 (these Thens were written after round 40's implementation, then revised red
  # first in round 41). Seed: a restart-only turn told its tools let it look, or that the
  # thread's old readings will do, fails the Thens.
  Scenario: A turn whose only lab tool acts needs a fresh reading, and still confirms a fix
    Given the node hands out "develop" of the lab's fork
    And the node offers "restart_container" too
    When the model replies "repo_checkout develop"
    Then the model's instructions say "They act and do not read the lab"
    And the model's instructions say "context, not the state now"
    And the model's instructions say "say so and leave the step undone"
    And the model's instructions do not say "never say you cannot read the home lab"
    And the model's instructions do not say "act only on a reading the thread already holds"
    And the model's instructions say "one restart of a container an hour, three in all"
    And the model's instructions say "diagnose first from a fresh reading taken this turn"
    And the model's instructions say "confirmation is still outstanding"
    And the model's instructions say "do not call the problem fixed"
