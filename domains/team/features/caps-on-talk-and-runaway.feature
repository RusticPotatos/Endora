Feature: Caps on talk and runaway
  Live, 17 and 18 October 2026, one docs branch took 44 turns, 80 posts and $2.47, 28% of all the
  spend, and nothing held it: the guard that counted talk saw only tasks inside a project, and
  noticed, but never stopped anything. A task now has a budget of talk (ADR 0133): twenty
  coordination turns in a day with nothing changing. Over it, the talk that would start more turns
  is held, not lost, and the task's lead is asked once to narrow it, split it or drop it. A new
  authority is tried before it is given, so the budget starts in shadow: it records what it would
  have held, and holds nothing, until the owner switches it on.

  Background:
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Ada has the task "Plan the backups"
    When Ada's turn starts
    And Ada's turn ends

  # --- The budget of talk ------------------------------------------------------------------

  # Seed: a budget that holds in shadow fails the queued step; one that never records fails the
  # finding step.
  Scenario: In shadow, a task over its budget of talk is recorded and nothing is held
    When Linus talks about "Plan the backups" for 21 turns and changes nothing
    And HQ looks at how the team is doing
    Then the owner's desk holds a finding "Coordination over budget"
    And the finding says "would have held coordination on #1 (budget 20, 21 turns, $0.00)"
    And the coordination guard is in shadow
    When Ada's turn starts
    And Ada's turn ends
    And Linus is asked to talk about "Plan the backups" again
    Then Linus's wake is queued, not held
    And Ada was not asked to decide anything about "Plan the backups"

  # Seed: a hold that drops the wake fails the last step; a second decision fails the second
  # "asked once" step; a lead's narrowing that does not renew the budget fails the last.
  Scenario: Switched on, the budget holds more talk, asks the lead once, and lets go when she narrows
    Given the owner switches the coordination guard on
    When Linus talks about "Plan the backups" for 21 turns and changes nothing
    And Linus is asked to talk about "Plan the backups" again
    Then Linus's wake is held
    And Ada is asked once to narrow, split or drop "Plan the backups"
    When Linus is asked to talk about "Plan the backups" again
    Then Linus's wake is a duplicate of the one held
    And Ada is asked once to narrow, split or drop "Plan the backups"
    When Ada's turn starts
    And Ada narrows what done means for "Plan the backups" to "The plan names three backups"
    And Ada's turn ends
    Then Linus's turn starts on "Plan the backups"

  # Seed: a mode that does not persist fails the last step.
  Scenario: The owner puts the budget back in shadow, and what was held goes
    Given the owner switches the coordination guard on
    When Linus talks about "Plan the backups" for 21 turns and changes nothing
    And Linus is asked to talk about "Plan the backups" again
    Then Linus's wake is held
    When Ada's turn starts
    And Ada's turn ends
    And the owner puts the coordination guard back in shadow
    And a tick passes
    Then Linus's turn starts on "Plan the backups"

  # --- A day's turns on the house's own models ----------------------------------------------

  # The house's own models cost nothing, so no budget stops a member that has run away with them:
  # the scripted demo once ran 1,539 turns in 3 h 40 min.

  # Seed: no cap fails the waiting step; a cap that loses the wake fails the last step.
  Scenario: A member's sixty-first turn of a day on the house's own models waits until tomorrow
    When Linus takes 60 turns on "Plan the backups", two minutes apart
    And Linus is asked to talk about "Plan the backups" again
    And a tick passes
    Then Linus's turn on "Plan the backups" is waiting for tomorrow
    And the top bar counts 1 turn waiting for tomorrow
    And the work on "Plan the backups" is held, not stuck
    When a day passes
    Then Linus's turn starts on "Plan the backups"
    And the top bar counts 0 turns waiting for tomorrow

  # --- A runaway ---------------------------------------------------------------------------

  # Only the owner could stop a team: the plan's tripwires were never built.

  # Seed: no tripwire fails the first step; one that lets the forty-first start fails the count;
  # a resume that does not clear the slate fails the last step.
  Scenario: A runaway stops the team itself, and the owner's resume starts it again
    When Ada and Linus are woken again and again, a turn every 5 seconds
    Then HQ has stopped the team, saying "stopped by the runaway tripwire"
    And 40 turns were started, and no more
    And the owner's desk holds a tripwire card naming Ada and Linus
    When the owner resumes the team
    Then the owner's desk is empty
    When Linus is asked to talk about "Plan the backups" again
    Then Linus's turn starts on "Plan the backups"

  # Seed: the owner's stop recorded as the tripwire's fails the first step; one that leaves a
  # card on the desk fails the last.
  Scenario: The owner's own kill switch is not the tripwire's
    When the owner pulls the kill switch
    Then the team is stopped by the owner
    And the owner's desk is empty
