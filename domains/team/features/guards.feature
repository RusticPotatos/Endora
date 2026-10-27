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
