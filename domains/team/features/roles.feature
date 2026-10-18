Feature: Roles as an enforced RACI
  The owner's decision of 18 October 2026 (ADR 0120): "Everything the CEO is
  doing I would expect the ops team to do and propose to the CEO." The CEO
  sets direction and owns the success; the lead of an area runs it, decides
  there, and proposes. The CEO reads anything, and never acts on the lab,
  is never woken by its alarms, and never closes an area's work: she
  directs its lead, who decides.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And the goal "Keep the home lab healthy"
    And the routine "Check the home lab" for Ada, which takes the home lab's alarms

  Scenario: The CEO sets direction and never touches the lab
    When Endora raises the alarm "deluge exited at 23:43"
    Then Ada is woken about it
    And Mara has never been woken
    When Ada asks "Deluge keeps exiting: plan a gluetun change?"
    Then the question goes to Mara
    When Mara closes "Check the home lab" as done
    Then it is refused: "this is Ada's area: the CEO does not claim, move, close, accept or send back its work. Give Ada a direction with direct (task, direction, evidence), and Ada decides"
    When Mara directs Ada on "Check the home lab": "Propose the gluetun change, with its evidence."
    Then Ada is woken about it
    And Ada's turn is told "Mara, the CEO, directs you: Propose the gluetun change, with its evidence."

  Scenario: A time-bound check waits its time
    When Endora raises the alarm "deluge exited at 23:43"
    And Ada restarts deluge and reads the lab
    And Ada closes "Check the home lab" as done on her reading
    Then it is refused, saying "wait until"
    When Ada checks back in an hour
    And an hour passes
    And Ada reads the lab again and closes "Check the home lab" as done on her reading
    Then the task "Check the home lab" is done
