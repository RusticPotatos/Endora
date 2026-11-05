Feature: Caps on talk and runaway
  A runaway team is stopped by HQ itself, and a member's turns on the house's own models stop for
  the day when they keep showing nothing: they cost nothing, so no budget stops them. Round 3
  (D15): Bo, on the house's model, stopped at a fixed sixty turns a day whatever his turns
  achieved. A count is not what should stop a free model; waste is (ADR 0133, amended
  2026-11-01). (The budget of talk, ADR 0133, went with the company: ADR 0142.)

  Background:
    Given Ada is the lead
    And Linus is the developer
    And the goal "Keep the home lab healthy"
    And Ada has the task "Plan the backups"
    When Ada's turn starts
    And Ada's turn ends

  # --- A day's turns on the house's own models ----------------------------------------------

  # The house's own models cost nothing, so no budget stops a member that has run away with them:
  # the scripted demo once ran 1,539 turns in 3 h 40 min.

  # Seed: no cap fails the waiting step; a cap that loses the wake fails the last step.
  Scenario: A member whose turns on the house's own models keep showing nothing waits until tomorrow
    When Linus takes 10 turns on "Plan the backups" that only talk, two minutes apart
    And Linus is asked to talk about "Plan the backups" again
    And a tick passes
    Then Linus's turn on "Plan the backups" is waiting for tomorrow
    And the top bar counts 1 turn waiting for tomorrow
    And the work on "Plan the backups" is held, not stuck
    When a day passes
    Then Linus's turn starts on "Plan the backups"
    And the top bar counts 0 turns waiting for tomorrow

  # Seed (D15): the fixed sixty a day stopped this member's sixty-first turn.
  Scenario: A member whose turns on the house's own models do work is not stopped by a count
    When Linus takes 61 turns on "Plan the backups" that each take a reading, two minutes apart
    And Linus is asked to talk about "Plan the backups" again
    And a tick passes
    Then Linus's turn starts on "Plan the backups"
    And the top bar counts 0 turns waiting for tomorrow

  # --- A runaway ---------------------------------------------------------------------------

  # Only the owner could stop a team: the plan's tripwires were never built. Turns that show
  # nothing are stopped by the waste check first; the tripwire catches turns that look busy.

  # Seed: no tripwire fails the first step; one that lets the forty-first start fails the count;
  # a resume that does not clear the slate fails the last step.
  Scenario: A runaway stops the team itself, and the owner's resume starts it again
    When Ada and Linus are woken again and again, a turn every 5 seconds, each taking a reading
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
