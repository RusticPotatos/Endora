Feature: A drain before every restart
  A deploy, and the NAS's own security reboot at four in the morning, restart HQ and the runner
  under turns that take ten or fifteen minutes. Since ADR 0131 such a turn is tried again without
  counting against its member, but what it had done since its last note was lost. Now a restart is
  announced first (ADR 0134): nothing new starts, the turns under way are told to wrap up and may
  end with a handover note while keeping their task, and whoever restarts waits until no turn is
  running, or for the cap. What the restart still cuts is tried again and told to the owner once,
  in the digest, and only then. HQ starting again ends the announcement; so does half an hour.

  Background:
    Given HQ keeps its team in a file
    And Ada is the lead
    And Zed is the developer
    And the goal "Keep the home lab healthy"
    And Zed has the task "Cap sonarr's memory"

  # Seed: a drain that lets the queue start fails the second step; one that drops the queued
  # turn fails the first Then; one that never lifts fails the last.
  Scenario: A drain starts no new turn, and the queued work waits for the restart
    When the owner announces a restart
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" is waiting for a restart
    And Zed has no turn running
    When the owner calls the restart off
    And a tick passes
    Then Zed's turn starts on "Cap sonarr's memory"

  # Seed: an answer without the notice fails the first Then; a wrap-up that hands the task on,
  # or leaves it parked for a nudge, fails the "still has" step; a note that is not kept fails
  # the briefing step.
  Scenario: A running member is told to wrap up, and ends its turn with a handover and keeps its task
    When Zed's turn starts
    And the owner announces a restart
    And Zed looks at what his turn is for
    Then Zed is told "A restart is coming: finish this step, leave a handover note, and end your turn."
    When Zed leaves the handover note "Raise the limit to 512 MB, then restart sonarr" on "Cap sonarr's memory"
    Then Zed's turn on "Cap sonarr's memory" waits to be tried again
    And Zed still has "Cap sonarr's memory"
    And no turn is running, and the restart can go ahead
    And no finding is open
    And the owner's desk is empty
    When the owner calls the restart off
    And a minute passes
    And a tick passes
    Then Zed's turn starts on "Cap sonarr's memory"
    And Zed's briefing says "Raise the limit to 512 MB, then restart sonarr"

  # Seed: a count of running turns that stays at one after the turn ends fails the last step.
  Scenario: The restart waits for the turn under way, and goes ahead when none is running
    When Zed's turn starts
    And the owner announces a restart
    Then the restart must wait for 1 running turn
    When Zed's turn ends
    Then no turn is running, and the restart can go ahead

  # Seed: a restart that does not count the turn it cut fails the digest step; one that raises a
  # card instead of a digest item fails the desk step; one that loses the turn fails the retry.
  Scenario: A turn still running at the cap is cut and tried again, and the digest says so
    When Zed's turn starts
    And the owner announces a restart that waits 60 seconds
    And 61 seconds pass
    Then the restart's cap has passed with 1 turn still running
    When HQ restarts
    And a tick passes
    Then Zed's turn on "Cap sonarr's memory" waits to be tried again
    And that was an interruption, and Zed has all his retries
    And the digest holds 1 item: a notice "A restart cut 1 turn: Zed on #"
    And the owner is interrupted by 0 items
    When a minute passes
    And a tick passes
    Then Zed's turn starts on "Cap sonarr's memory"

  # Seed: a notice raised for a clean drain fails the digest step; no feed line fails the last.
  Scenario: A drain that cut nothing is silent
    When Zed's turn starts
    And the owner announces a restart
    And Zed's turn ends
    And HQ restarts
    And a tick passes
    Then the owner's desk is empty
    And the digest is empty
    And the Activity says the restart came after 1 turn ended and 0 were cut

  # Seed: a drain that is not kept in the file fails the first Then; one that outlives HQ's start
  # fails the second.
  Scenario: HQ starting again lifts the drain, and the work goes on
    When the owner announces a restart
    And HQ restarts
    Then a restart is still announced
    When a tick passes
    Then no restart is announced
    And Zed's turn starts on "Cap sonarr's memory"

  # Seed: no age limit fails the last three steps; a limit that only hides the drain, and never
  # lifts it, fails the Activity step.
  Scenario: A drain nobody ended lifts itself after thirty minutes
    When the owner announces a restart
    And 29 minutes pass
    Then a restart is still announced
    When 2 minutes pass
    Then no restart is announced
    And the Activity says the drain ended because nobody ended it
    And Zed's turn starts on "Cap sonarr's memory"
