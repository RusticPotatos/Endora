Feature: Guards against how agent teams fail
  The owner's words of 18 October 2026: "Work keeps going through a process,
  getting stuck, then lands on my desk." A team of models fails in ways no
  single turn shows: it talks about work instead of doing it, it bounces a
  task between its worker and its checkers, it copies a wrong claim from note
  to note, and it polishes proof that was already enough. Each guard is a
  rule over HQ's record (ADR 0124), and each finding takes the owner's or the
  lead's verdict, so HQ knows how often a guard is right.

  Background:
    Given Ada is the lead
    And the goal "Keep the home lab healthy"

  # Seed: Ceremony::over never holding fails the desk step.
  Scenario: Talking about work isn't work
    Given the project "Home lab upkeep" led by Ada
    And Ada has the task "Plan the backups" in the project "Home lab upkeep"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And HQ looks at how the team is doing
    Then the owner's desk holds a finding "Talk about work"
    And Ada is woken about it
    And the guards card says "12 coordination turns, 0 product changes in 24 h"
    When the owner says the finding was real
    Then the guard "ceremony" is 100% real

  # Seed: the ceremony counting only the tasks inside a project finds nothing here, and the
  # desk step fails; the finding without the spend fails the cost step.
  Scenario: Talking about work outside a project isn't work either, and the finding says what it cost
    Given Linus is the developer
    And Dee is the tester
    When Linus is handed the task "Tidy the docs"
    And Linus's turn starts
    And Linus's turn ends
    And Dee reviews "Tidy the docs" 12 times at $0.05 a turn and changes nothing
    And HQ looks at how the team is doing
    Then the owner's desk holds a finding "Talk about work"
    And the finding says "12 coordination turns, 0 product changes in 24 h ($0.60 spent)"
    And the guards card says "Tidy the docs"
    And Ada is woken about it

  # Seed: narrow_criteria open to every role fails the refusal step; shut to the lead fails the
  # narrowing step; one that lets it widen fails the last.
  Scenario: A lead narrows what done means for work in its area, and nobody else may
    Given Linus is the developer
    And Linus has the task "Tidy the docs" that is done when both "The links work" and "The headings are consistent" hold
    When Linus's turn starts
    And Linus tries to narrow what done means for "Tidy the docs" to "The links work"
    Then it is refused, saying "only the lead of the task's area narrows its criteria"
    When Linus's turn ends
    And Ada is woken about "Tidy the docs"
    And Ada tries to give "Tidy the docs" three criteria
    Then it is refused, saying "the task has 2 and you gave 3"
    When Ada narrows what done means for "Tidy the docs" to "The links work"
    Then the task "Tidy the docs" is done when "The links work"
    And HQ says in "Tidy the docs": "Ada narrowed what done means"

  # Seed: after_sent_back returning before the carousel's check fails the huddle step.
  Scenario: A task bounced three times goes to a huddle
    Given Grace is the tester
    And Linus is the developer
    And Linus has the task "Look at sonarr"
    When Linus submits "Look at sonarr" for review
    And Grace sends back "Look at sonarr" because "no reading of sonarr"
    And Grace's turn ends
    And Linus submits "Look at sonarr" for review
    And Grace sends back "Look at sonarr" because "still no reading"
    And Grace's turn ends
    And Linus submits "Look at sonarr" for review
    And Grace sends back "Look at sonarr" because "the same again"
    Then a huddle is called on "Look at sonarr", because it was "sent back 3 times with no new evidence between"
    And the owner's desk is empty

  # Seed: not_retracted always passing fails the refusal step.
  Scenario: A retracted claim can't come back
    Given Linus is the developer
    And Linus has the task "Look at sonarr"
    When Linus writes the note "Sonarr needs a restart every night."
    And Linus's turn ends
    And Ada is woken about "Look at sonarr"
    And Ada writes the note "As we found, sonarr needs a restart every night or the queue sticks."
    And Ada's turn ends
    And the owner says Linus's note "Sonarr needs a restart every night." is not true
    Then the sweep finds Ada's note "As we found, sonarr needs a restart every night or the queue sticks."
    When the owner forgets what the sweep found
    Then Ada has no notes
    When Linus is woken about "Look at sonarr"
    And Linus writes the note "Reminder: sonarr needs a restart every night."
    Then it is refused, saying "this says again what was retracted on"

  # Seed: submit_when_enough returning at once fails the in_review step.
  Scenario: Enough evidence means submit
    Given Linus is the developer
    And Linus has the task "Find why sonarr restarts", done when "The cause is found"
    When Linus takes "Find why sonarr restarts" and reads sonarr's logs
    And Linus is woken and reads sonarr's logs again
    Then Linus's briefing says "Your evidence already meets the criteria: submit now"
    When Linus is woken and reads sonarr's logs again
    And Linus is woken and reads sonarr's logs again
    Then the task "Find why sonarr restarts" is in_review
    And HQ says in "Find why sonarr restarts": "HQ submits it for Linus, on that evidence."

  # Live, work waiting on a dropped task is told at once and stays blocked. Now it is not made to
  # wait on one in the first place.
  # Seed: the waits check skipped for dropped work fails the refusal step.
  Scenario: Work is not made to wait on what was dropped
    Given Ada has the task "Try the old way"
    And Ada has the task "Read the logs"
    When the owner drops the task "Try the old way"
    And the owner hands Ada the task "Restore from the backup", waiting on "Try the old way"
    Then it is refused: "#1 was dropped, so what a task waits on it for will not happen: leave it out of waits_on, or wait on the task that carries its work on"
    And the team has 2 tasks
    When the owner hands Ada the task "Restore from the backup", waiting on "Read the logs"
    Then it is not refused

  # A turn that ended before turns were labelled may have been the talk that puts the project over,
  # or the change that keeps it under. HQ says it cannot tell; it neither blames nor clears.
  # Seed: Ceremony::over reading an unread turn as talk fails the card step; as a change, the
  # same; either way a finding opens on a count HQ does not have and fails the desk step.
  Scenario: A guard that cannot tell says so, and opens no finding
    Given the project "Home lab upkeep" led by Ada
    And Ada has the task "Plan the backups" in the project "Home lab upkeep"
    When Ada talks about "Plan the backups" for 12 turns and changes nothing
    And one of Ada's turns on "Plan the backups" ended before turns were labelled
    And HQ looks at how the team is doing
    Then the guards card says "11 coordination turns, 0 product changes in 24 h, 1 turn it cannot read"
    And the guards card cannot tell whether "Home lab upkeep" is talking more than working
    And the owner's desk holds no finding
    And Ada is not woken by a guard
