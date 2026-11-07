Feature: The HQ agents map
  The owner's decision M5 (7 Nov 2026): one read-only map of the Core team, the open items each
  member holds grouped by project, and what waits on the owner, from HQ's own records. A line
  moves only for a turn that is running now; a still map says plainly that nothing is running.

  Background:
    Given the team is one team of models
      | member | model  |
      | Mara   | opus   |
      | Kit    | sonnet |
      | Ada    | haiku  |
      | Sol    | haiku  |
      | Rae    | mac    |
      | Bo     | local  |
    And the loop "ansible-nas" for the goal "Keep the fork tidy"

  Scenario: While a member's turn runs, its line is the only one that moves
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the loop hands out its items
    Then the agents map has 1 turn running, and only Bo's line moves
    And on the agents map Bo holds "Raise sonarr's memory" under "ansible-nas"

  Scenario: Once the turns end nothing moves, and what waits on the owner is counted
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the loop hands out its items
    And Bo asks the owner "May sonarr have 2 GB of the NAS's 16?"
    And every running turn ends
    Then the agents map says "Nothing is running. 1 card waits on you."
    And on the agents map Bo's "Raise sonarr's memory" asks the owner
    And the agents map's desk has 1 card under "ansible-nas"

  Scenario: A paused member is shown paused, never working
    Given the loop "ansible-nas" has the item "Raise sonarr's memory"
    When the owner pauses Bo
    Then the agents map shows Bo as "paused"
