Feature: What done means comes first
  An ask with no definition of done lets a team wander. Live, the owner's "no literal
  passwords in role defaults" went to a lead with nothing to check it against and split into
  three blocked sub-tasks, each one defensible and none of them tied to what was asked. The
  owner's work now says what done means, and a lead's split of it says which part of that each
  piece serves (ADR 0124, amended).

  Background:
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"

  # Seed: owner_may_hand_out accepting an empty list fails the refusal step.
  Scenario: Work with no definition of done is not given
    When the owner gives Ada the work "Remove literal passwords from the role defaults" with no criteria
    Then it is refused, saying "what done means"
    And the team has no task
    And Ada has never been woken

  Scenario: Work with a definition of done is given, and its lead is woken for it
    When the owner gives Ada the work "Remove literal passwords from the role defaults", done when "No role default holds a password"
    Then it is not refused
    And the task "Remove literal passwords from the role defaults" is done when "No role default holds a password"
    And Ada is woken about it

  # Seed: refusing every split that names no `serves`, whether or not its task has criteria,
  # fails the Then.
  Scenario: Work already open with no criteria has none to name, so its split is free
    Given Ada has the task "Keep sonarr up"
    When Ada's turn starts
    And Ada hands Linus the part "Read sonarr's queue" of "Keep sonarr up", done when "The queue is read"
    Then it is not refused
    And the team has 2 tasks
