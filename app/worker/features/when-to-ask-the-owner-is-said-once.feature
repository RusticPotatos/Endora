Feature: When to ask the owner is said once, the same everywhere
  The architecture challenge (round 1, 5 November) found the guidance on what may be asked
  stated in five places that contradicted each other. The worker's own home-lab rule said
  "Anything else on the home lab is the owner's to decide: ask with ask_owner", while HQ's
  ask_owner said "only what the owner alone decides". In climb 2's Haiku baseline (round 13) a
  member asked the owner about a pending, non-security kernel update. One statement (climb 2, S3),
  from Endora's kernel, now says when to ask, in the worker's instructions and in HQ's tool. It
  covers needs the task names and needs the member finds, and it stays guidance: what may run is
  enforced where it runs.

  Background:
    Given a turn in the workbench

  # Seed: the old "anything else ... ask" rule kept, or the one statement missing, fails a Then.
  Scenario: A turn offered the home lab's tools is told when to ask, once, and nothing contrary
    Given the node hands out "develop" of the lab's fork
    And the node offers the home lab's tools too
    When the model replies "repo_checkout develop"
    Then the model's instructions say "Ask the owner only for a decision your own authority does not cover"
    And the model's instructions say "for one you discover yourself"
    And the model's instructions say "it is not a reason to propose the step"
    And the model's instructions say "Asking requests a decision and changes nothing"
    And the model's instructions say "try to work around a refusal"
    And the model's instructions do not say "Anything else on the home lab is the owner's to decide"

  Scenario: A turn outside the lab is told the same
    Given the node hands out "main" of a repository outside the lab
    When the model replies "repo_checkout main"
    Then the model's instructions say "Ask the owner only for a decision your own authority does not cover"
    And the model's instructions say "for one you discover yourself"
    And the model's instructions do not say "Anything else on the home lab is the owner's to decide"
