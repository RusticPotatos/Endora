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

  Scenario: Ops spots the pattern and proposes the fix
    Given the home lab has the containers "deluge, prowlarr, sonarr"
    And the home lab's work is Ada's project
    When Endora raises the alarm "deluge, prowlarr exited at 23:43"
    And Endora raises the alarm "deluge, prowlarr exited at 01:44"
    And Ada's weekly patterns turn comes
    Then Ada's turn is told "deluge, prowlarr: 2 alarms"
    When Ada proposes a project on the alarms that came back
    Then the project "Fix what keeps exiting" is proposed
    And Mara has never been woken
    When Mara's daily review comes
    Then Mara's turn is told "- #2 Fix what keeps exiting (by Ada"
    When Mara accepts the project "Fix what keeps exiting" because "the same containers exited twice"
    Then the project "Fix what keeps exiting" is active

  # The owner's amendment of 18 October 2026: work kept going round a process, getting
  # stuck, then landing on the owner's desk. A lead decides; a huddle helps it decide.
  # ADR 0139: before any person, the member takes it a model tier up.
  Scenario: Stuck work climbs the ladder, not straight to the board
    Given Linus is the developer
    And Linus has the task "Clear the stuck downloads"
    When "Clear the stuck downloads" sits with nobody coming back to it
    Then Linus takes "Clear the stuck downloads" a model tier up
    When "Clear the stuck downloads" sits with nobody coming back to it
    Then Ada is asked to decide "Clear the stuck downloads"
    And the owner's desk is empty
    When "Clear the stuck downloads" sits with nobody coming back to it
    Then a huddle is called on "Clear the stuck downloads"
    When Linus gives the view "One file in the queue cannot be imported. Removing it clears the queue." on "Clear the stuck downloads"
    And Ada decides "Clear the stuck downloads": proceed, "remove the one file, then read the queue again"
    Then the task "Clear the stuck downloads" is todo
    And the owner's desk is empty
    And Mara has never been woken

  Scenario: A time-bound check waits its time
    When Endora raises the alarm "deluge exited at 23:43"
    And Ada restarts deluge and reads the lab
    And Ada closes "Check the home lab" as done on her reading
    Then it is refused, saying "wait until"
    When Ada checks back in an hour
    And an hour passes
    And Ada reads the lab again and closes "Check the home lab" as done on her reading
    Then the task "Check the home lab" is done
