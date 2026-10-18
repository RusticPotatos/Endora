Feature: The team's map
  The team's world has a page for every area, project and role, each with a steward
  worked out from who leads it (ADR 0126). A member who does not keep a page proposes to
  it, and the steward decides, saying why. Every briefing opens with a short map of the
  pages that matter to that member. A member who looks and finds nothing says so, apart
  from the broken roads HQ finds itself, and each night a member on the free model is
  quizzed on the map, scored by code.

  Background:
    Given Mara is the CEO
    And Ada is the lead
    And Bo is the lead
    And the owner hires Linus as a developer under Ada
    And the goal "Keep the home lab healthy"
    And HQ has made the map's pages

  Scenario: A section keeps its address when it is promoted or rewritten
    Given Ada has the task "Look at backups"
    And Bo has the task "Look at the router"
    When Ada writes "Backups" on "area/Ada" citing the task "Look at backups"
    Then "world:area/Ada#backups" stands as an observation
    When the owner makes "world:area/Ada#backups" canon
    And Ada's turn ends
    And the owner writes "Backups" on "area/Ada" saying "Backups run nightly at 03:00."
    Then "world:area/Ada#backups" stands as canon
    And Bo reading "world:area/Ada#backups" sees "Backups run nightly at 03:00."

  Scenario: A developer proposes a change to the ops page, and its lead decides
    Given Linus has the task "Fix the proxy"
    When Linus writes "Backups" on "area/Ada" citing the task "Fix the proxy"
    Then it is refused for "kept by Ada"
    When Linus proposes "Backups" to "area/Ada" citing the task "Fix the proxy"
    And Linus's turn ends
    And Ada is handed the task "Look at backups"
    And Ada's turn starts
    Then Ada's briefing says "Linus proposes"
    When Ada declines the proposal because "the backups move next week"
    Then no section is on "area/Ada"
    When Ada's turn ends
    And Linus is handed the task "Fix the DNS"
    And Linus's turn starts
    Then Linus's briefing says "declined by Ada: the backups move next week"
    When Linus proposes "Backups" to "area/Ada" citing the task "Fix the DNS"
    And Linus's turn ends
    And Ada is handed the task "Check the NAS"
    And Ada accepts the proposal
    Then "world:area/Ada#backups" stands as an observation
