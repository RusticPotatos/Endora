Feature: Elastic staffing and notebooks
  The team grows and shrinks with its work, and nobody is ever fired (ADR 0118).
  A member with nothing to do for a week is offered to the owner for furlough;
  a lead with a backlog gets a furloughed member back before anyone is hired;
  both are the owner's to approve. Each member keeps notes of its own, and a
  note rests on evidence from HQ's record, so what a member remembers is what
  it has read and not what it guessed.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Dee is the tester
    And the goal "Keep the home lab healthy"
    And the engineering wing

  Scenario: An idle member is offered for furlough, never fired
    Given 8 days pass
    Then the owner's desk offers to furlough "Rae"
    And the owner's desk offers to furlough "Zed"
    When the owner approves furloughing "Rae"
    Then Rae is on furlough
    And Rae is still on the team
    And waking Rae is refused as furloughed
    But Zed is at work

  Scenario: A lead with a backlog gets a furloughed member back first
    Given Zed is on furlough
    And the wing's project has 2 open tasks
    When Kit's turn starts
    And Kit asks for staff for the wing's project, citing its 2 open tasks
    Then the owner's desk holds a request to recall "Zed"
    And the owner's desk holds 0 hire requests
    When the owner approves the recall
    Then Zed is at work
    And Zed reports to Kit

  Scenario: A note needs evidence
    Given Kit hands Rae the task "Look at the proxy" in the wing
    When Rae's turn starts
    And Rae notes "The proxy restarts clean." citing nothing
    Then it is refused for "evidence"
    When Rae notes "The proxy restarts clean." citing "task 999"
    Then it is refused for "task 999"
    And Rae's notebook holds 0 notes
    When Rae notes "The proxy restarts clean." citing the task "Look at the proxy"
    Then Rae's notebook holds 1 note

  Scenario: A furloughed member keeps its notes
    Given Rae noted "The proxy restarts clean." resting on the wing's first task
    When the owner furloughs Rae
    And 2 days pass
    And the owner recalls Rae
    Then Rae's notebook holds 1 note
    Given Kit hands Rae the task "Look at the proxy" in the wing
    When Rae's turn starts
    Then Rae's briefing says "You were furloughed from 17 Oct to 19 Oct; read your notes first."
    And Rae's briefing says "The proxy restarts clean."
