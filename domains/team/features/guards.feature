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
