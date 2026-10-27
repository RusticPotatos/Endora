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
