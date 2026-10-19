Feature: The team's map
  The team's world has a page for every area, project and role, each with a steward
  worked out from who leads it (ADR 0126). A member who does not keep a page proposes to
  it, and the steward decides, saying why. Every briefing opens with a short map of the
  pages that matter to that member. A member who looks and finds nothing says so, apart
  from the broken roads HQ finds itself, and each night a free member is quizzed on the
  map, in a turn on the house's own model that costs nothing, scored by code.

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

  Scenario: A developer finds what exists from its map, and reports a gap to its lead when there is none
    Given Ada has the task "Look at backups"
    When Ada writes "Desk notices" on "area/Ada" citing the task "Look at backups"
    And Ada's turn ends
    And Linus is handed the task "Tell the owner when a branch passes"
    And Linus's turn starts
    Then Linus's map names "world:area/Ada#desk-notices"
    And Linus reading "world:area/Ada#desk-notices" sees "Nightly, at 03:00."
    When Linus reports a gap "how a restore drill is run", looked in "world:area/Ada, world:lab"
    And Linus's turn ends
    And Ada is handed the task "Check the NAS"
    And Ada's turn starts
    Then Ada's briefing says "how a restore drill is run"
    And the owner's desk is empty

  Scenario: A retired section breaks the roads that cite it, and the steward of the citing page is told
    Given Ada has the task "Look at backups"
    And Mara has the task "Review the house"
    When Ada writes "Restores" on "area/Ada" citing the task "Look at backups"
    And Mara writes "Disks" on the house page, saying "Disks: see world:area/Ada#restores", citing the task "Look at backups"
    Then no road is broken
    When Ada retires "world:area/Ada#restores" because "restores moved"
    Then the road from "world:house#disks" to "world:area/Ada#restores" is broken
    When Mara's turn ends
    And Mara is handed the task "Review the house again"
    And Mara's turn starts
    Then Mara's briefing says "Broken roads"
    And Mara's briefing says "world:house#disks cites world:area/"

  Scenario: A lead's briefing is its map, its area's sections and the house canon
    Given the owner writes "The disk" on "house" saying "The NAS has 16 GB."
    And the owner makes "world:house#the-disk" canon
    And Ada has the task "Look at backups"
    When Ada writes "Backups" on "area/Ada" citing the task "Look at backups"
    And Ada's turn ends
    And Ada is handed the task "Look at the router"
    And Ada's turn starts
    Then Ada's map names "world:area/Ada#backups"
    And Ada's briefing says "The NAS has 16 GB."
    And Ada's briefing keeps to its budget of 2800 tokens
    And nothing was left out of Ada's briefing unnamed

  Scenario: Each morning the World shows last night's map check, scored by code
    Given the nightly quiz is on
    And it is 2:00 in the morning
    When HQ looks at the night
    Then a quiz is asked of a member, in a turn on the house's model that costs nothing
    When that member answers the quiz from its map
    Then the World shows last night's quiz with every question right
    And the World shows no page as failing the quiz

  Scenario: With no member free all night, the night is skipped and the World says so
    Given the nightly quiz is on
    And Ada has the task "Look at backups"
    And Bo has the task "Look at the router"
    And Linus has the task "Look at sonarr"
    And it is 2:00 in the morning
    When HQ looks at the night
    Then no quiz is asked
    When it is 6:30 in the morning
    And HQ looks at the night
    Then the World shows last night's quiz as skipped because "free"

  Scenario: A page that fails the quiz three nights running is flagged to its steward
    Given the nightly quiz is on
    And it is 2:00 in the morning
    When the night's quiz is answered wrongly three nights running
    Then the World shows "area/Ada" as failing the quiz
    When Ada is handed the task "Look at backups"
    And Ada's turn starts
    Then Ada's briefing says "failed the nightly quiz"
